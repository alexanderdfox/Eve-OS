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

QEMU_SHARE="${QEMU_SHARE:-}"
if [[ -z "$QEMU_SHARE" ]]; then
  if [[ -d /opt/homebrew/share/qemu ]]; then
    QEMU_SHARE=/opt/homebrew/share/qemu
  elif [[ -d /usr/local/share/qemu ]]; then
    QEMU_SHARE=/usr/local/share/qemu
  else
    echo "error: set QEMU_SHARE (directory with edk2-aarch64-code.fd)" >&2
    exit 1
  fi
fi

CODE="$QEMU_SHARE/edk2-aarch64-code.fd"
# Homebrew QEMU 10.x uses edk2-arm-vars.fd as NVRAM template for aarch64 (see share/qemu/firmware/60-edk2-aarch64.json).
VARS_SRC="$QEMU_SHARE/edk2-aarch64-vars.fd"
if [[ ! -f "$VARS_SRC" ]]; then
  VARS_SRC="$QEMU_SHARE/edk2-arm-vars.fd"
fi
if [[ ! -f "$CODE" || ! -f "$VARS_SRC" ]]; then
  echo "error: need $CODE and a vars template (edk2-aarch64-vars.fd or edk2-arm-vars.fd)" >&2
  exit 1
fi

VARS_RUN="${VARS_RUN:-$ROOT/utm/arm-uefi/.qemu-edk2-vars.run.fd}"
mkdir -p "$(dirname "$VARS_RUN")"
cp -f "$VARS_SRC" "$VARS_RUN"

ACCEL=hvf
CPU=host
if [[ "$TCG" == 1 ]]; then
  ACCEL=tcg
  CPU=max
fi

if [[ "$SERIAL_TO_TERMINAL" == 1 ]]; then
  SERIAL_ARGS=(-serial mon:stdio)
else
  SERIAL_ARGS=(-serial null)
fi

echo "FAT=$FAT"
echo "CODE=$CODE  VARS=$VARS_RUN  accel=$ACCEL  serial_terminal=$SERIAL_TO_TERMINAL"
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
