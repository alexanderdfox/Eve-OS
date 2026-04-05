#!/usr/bin/env bash
# Run EVE AArch64 UEFI app under QEMU on Apple Silicon (HVF) or elsewhere (TCG).
# Requires: Homebrew qemu (edk2-aarch64-*.fd), mtools (mformat/mcopy).
# Usage: ./scripts/arm-uefi-run.sh [--tcg] [--serial]
#   Default: QEMU display + -serial null. Add --serial for guest UART on this terminal.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if [[ -f "$HOME/.cargo/env" ]]; then
  # shellcheck source=/dev/null
  source "$HOME/.cargo/env"
fi

"$ROOT/scripts/arm-uefi-build.sh"

if ! command -v mformat &>/dev/null || ! command -v mcopy &>/dev/null; then
  echo "error: install mtools: brew install mtools" >&2
  exit 1
fi

TGT="$(cargo metadata --format-version 1 --no-deps | python3 -c "import json,sys; print(json.load(sys.stdin)['target_directory'])")"
EFI="$TGT/aarch64-unknown-uefi/release/bootaa64.efi"
FAT_IMG="$TGT/arm-uefi-fat.img"
VARS_MUTABLE="$TGT/arm-uefi-vars.fd"

QEMU_SHARE="${QEMU_SHARE:-}"
if [[ -z "$QEMU_SHARE" ]]; then
  if [[ -d /opt/homebrew/share/qemu ]]; then
    QEMU_SHARE=/opt/homebrew/share/qemu
  elif [[ -d /usr/local/share/qemu ]]; then
    QEMU_SHARE=/usr/local/share/qemu
  else
    echo "error: set QEMU_SHARE to the directory containing edk2-aarch64-code.fd" >&2
    exit 1
  fi
fi

CODE="$QEMU_SHARE/edk2-aarch64-code.fd"
VARS_SRC="$QEMU_SHARE/edk2-aarch64-vars.fd"
if [[ ! -f "$VARS_SRC" ]]; then
  VARS_SRC="$QEMU_SHARE/edk2-arm-vars.fd"
fi
if [[ ! -f "$CODE" || ! -f "$VARS_SRC" ]]; then
  echo "error: missing $CODE or NVRAM template (edk2-aarch64-vars.fd / edk2-arm-vars.fd)" >&2
  exit 1
fi

cp -f "$VARS_SRC" "$VARS_MUTABLE"

rm -f "$FAT_IMG"
dd if=/dev/zero of="$FAT_IMG" bs=1048576 count=64 status=none
mformat -F -i "$FAT_IMG" ::
mmd -i "$FAT_IMG" ::EFI
mmd -i "$FAT_IMG" ::EFI/BOOT
mcopy -i "$FAT_IMG" -D o "$EFI" ::EFI/BOOT/BOOTAA64.EFI

ACCEL=hvf
CPU=host
SERIAL_TO_TERMINAL=0
for a in "$@"; do
  case $a in
    --tcg)
      ACCEL=tcg
      CPU=max
      ;;
    --serial)
      SERIAL_TO_TERMINAL=1
      ;;
  esac
done

if [[ "$SERIAL_TO_TERMINAL" == 1 ]]; then
  SERIAL_ARGS=(-serial mon:stdio)
else
  SERIAL_ARGS=(-serial null)
fi

echo "QEMU_SHARE=$QEMU_SHARE  accel=$ACCEL cpu=$CPU  serial_terminal=$SERIAL_TO_TERMINAL"
exec qemu-system-aarch64 \
  -machine "virt,accel=$ACCEL" \
  -cpu "$CPU" \
  -m 512M \
  "${SERIAL_ARGS[@]}" \
  -display default \
  -drive "if=pflash,format=raw,readonly=on,file=$CODE" \
  -drive "if=pflash,format=raw,file=$VARS_MUTABLE" \
  -drive "if=none,format=raw,file=$FAT_IMG,id=disk0" \
  -device virtio-blk-device,drive=disk0
