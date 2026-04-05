#!/usr/bin/env bash
# Run Eve kernel-rpi in QEMU (raspi3b / raspi4b) with serial on stdio and a display window.
# Attaches QEMU user NAT + usb-net and a USB keyboard (guest drivers not in kernel-rpi yet).
# Usage: ./scripts/run-raspi-qemu.sh [pi3|pi4]
#   RPI_QEMU_NET=0  — omit -netdev / usb-net (if your QEMU/UTM setup already provides NICs).
#   RPI_QEMU_USB_KBD=0 — omit -usb / usb-kbd.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SOC="${1:-pi3}"

case "$SOC" in
  pi3)
    MACHINE=raspi3b
    MEM=1G
    IMG="$ROOT/rpi/dist/kernel8-pi3.img"
    ;;
  pi4)
    MACHINE=raspi4b
    MEM=2G
    IMG="$ROOT/rpi/dist/kernel8-pi4.img"
    ;;
  *)
    echo "usage: $0 [pi3|pi4]" >&2
    exit 1
    ;;
esac

if [[ ! -f "$IMG" ]]; then
  echo "Kernel image missing; building $SOC and copying to $IMG …"
  RPI_SOC="$SOC" "$ROOT/scripts/rpi-build.sh"
  mkdir -p "$ROOT/rpi/dist"
  cp -f "$ROOT/rpi/dist/kernel8.img" "$IMG"
fi

NET_USB=()
if [[ "${RPI_QEMU_NET:-1}" != 0 ]]; then
  NET_USB+=(-netdev user,id=rpi0,ipv6=off -device usb-net,netdev=rpi0)
fi
if [[ "${RPI_QEMU_USB_KBD:-1}" != 0 ]]; then
  NET_USB+=(-device usb-kbd)
fi
if [[ "${#NET_USB[@]}" -gt 0 ]]; then
  NET_USB=(-usb "${NET_USB[@]}")
fi

exec qemu-system-aarch64 \
  -M "$MACHINE" \
  -m "$MEM" \
  -kernel "$IMG" \
  -serial mon:stdio \
  "${NET_USB[@]}"
