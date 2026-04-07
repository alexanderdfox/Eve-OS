#!/usr/bin/env bash
# Boot an existing AArch64 UEFI FAT disk in QEMU (virt + EDK2).
# Default image: utm/arm-uefi/eve-arm-uefi-fat.img
# Usage: ./scripts/arm-uefi-boot-img.sh [disk.img] [--tcg] [--serial]
#   Default: QEMU window only (-display default, -serial null). Use --serial for mon:stdio.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

FAT="$ROOT/utm/arm-uefi/eve-arm-uefi-fat.img"
TCG=0
SERIAL_TO_TERMINAL=0
while [[ $# -gt 0 ]]; do
  case $1 in
    --tcg)
      TCG=1
      ;;
    --serial)
      SERIAL_TO_TERMINAL=1
      ;;
    -*)
      echo "error: unknown option: $1 (try --tcg or --serial)" >&2
      exit 1
      ;;
    *)
      FAT="$1"
      ;;
  esac
  shift
done

if [[ ! -f "$FAT" ]]; then
  echo "error: FAT image not found: $FAT" >&2
  echo "  Run: ./scripts/arm-uefi-sync.sh  or  ./scripts/build-all-images.sh" >&2
  exit 1
fi

# shellcheck source=edk2-aarch64-qemu-firmware.sh
source "$ROOT/scripts/edk2-aarch64-qemu-firmware.sh"
if ! resolve_edk2_aarch64_qemu_firmware; then
  edk2_aarch64_firmware_hint
  exit 1
fi

VARS_RUN="${VARS_RUN:-$ROOT/utm/arm-uefi/.qemu-edk2-vars.run.fd}"
mkdir -p "$(dirname "$VARS_RUN")"
cp -f "$VARS_SRC" "$VARS_RUN"

ACCEL=tcg
CPU=max
if [[ "$TCG" == 0 ]]; then
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

echo "FAT=$FAT"
echo "CODE=$CODE  vars_template=$VARS_SRC  VARS_run=$VARS_RUN  accel=$ACCEL  serial_terminal=$SERIAL_TO_TERMINAL"
exec qemu-system-aarch64 \
  -machine "virt,accel=$ACCEL" \
  -cpu "$CPU" \
  -m 512M \
  "${SERIAL_ARGS[@]}" \
  -display default \
  -drive "if=pflash,format=raw,readonly=on,file=$CODE" \
  -drive "if=pflash,format=raw,file=$VARS_RUN" \
  -drive "if=none,format=raw,file=$FAT,id=disk0" \
  -device virtio-blk-device,drive=disk0 \
  -netdev user,id=n0,ipv6=off \
  -device virtio-net-device,netdev=n0 \
  -device qemu-xhci,id=xhci \
  -device usb-kbd,bus=xhci.0,port=1
