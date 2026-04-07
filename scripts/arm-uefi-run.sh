#!/usr/bin/env bash
# Run EVE AArch64 UEFI app under QEMU on Apple Silicon (HVF) or elsewhere (TCG).
# Requires: QEMU AArch64 UEFI firmware (see scripts/edk2-aarch64-qemu-firmware.sh), mtools.
# Adds VirtIO net (user NAT, ipv6=off) + xHCI + usb-kbd to match utm/qemu-extra-arm-uefi.args.
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

# shellcheck source=edk2-aarch64-qemu-firmware.sh
source "$ROOT/scripts/edk2-aarch64-qemu-firmware.sh"
if ! resolve_edk2_aarch64_qemu_firmware; then
  edk2_aarch64_firmware_hint
  exit 1
fi

cp -f "$VARS_SRC" "$VARS_MUTABLE"

rm -f "$FAT_IMG"
dd if=/dev/zero of="$FAT_IMG" bs=1048576 count=64 status=none
mformat -F -i "$FAT_IMG" ::
mmd -i "$FAT_IMG" ::EFI
mmd -i "$FAT_IMG" ::EFI/BOOT
mcopy -i "$FAT_IMG" -D o "$EFI" ::EFI/BOOT/BOOTAA64.EFI

# HVF is Apple Silicon only; KVM only when host is aarch64. Intel Mac / x86 Linux need TCG.
ACCEL=tcg
CPU=max
SERIAL_TO_TERMINAL=0
FORCE_TCG=0
for a in "$@"; do
  case $a in
    --tcg)
      FORCE_TCG=1
      ;;
    --serial)
      SERIAL_TO_TERMINAL=1
      ;;
  esac
done
if [[ "$FORCE_TCG" == 0 ]]; then
  case "$(uname -s):$(uname -m)" in
    Darwin:arm64)
      ACCEL=hvf
      CPU=host
      ;;
    Linux:aarch64)
      if [[ -r /dev/kvm ]]; then
        ACCEL=kvm
        CPU=host
      fi
      ;;
  esac
fi

if [[ "$SERIAL_TO_TERMINAL" == 1 ]]; then
  SERIAL_ARGS=(-serial mon:stdio)
else
  SERIAL_ARGS=(-serial null)
fi

echo "EDK2 code=$CODE  vars_template=$VARS_SRC  accel=$ACCEL cpu=$CPU  serial_terminal=$SERIAL_TO_TERMINAL"
exec qemu-system-aarch64 \
  -machine "virt,accel=$ACCEL" \
  -cpu "$CPU" \
  -m 512M \
  "${SERIAL_ARGS[@]}" \
  -display default \
  -device virtio-gpu-pci \
  -drive "if=pflash,format=raw,readonly=on,file=$CODE" \
  -drive "if=pflash,format=raw,file=$VARS_MUTABLE" \
  -drive "if=none,format=raw,file=$FAT_IMG,id=disk0" \
  -device virtio-blk-device,drive=disk0 \
  -netdev user,id=n0,ipv6=off \
  -device virtio-net-device,netdev=n0 \
  -device qemu-xhci,id=xhci \
  -device usb-kbd,bus=xhci.0,port=1
