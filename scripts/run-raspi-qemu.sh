#!/usr/bin/env bash
# Run Eve kernel-rpi in QEMU (raspi3b / raspi4b) with serial on stdio and a display window.
#
# Input: the guest only reads **PL011 UART0** (see kernel-rpi). Keyboard must reach that UART
# (stdin here). Mouse is **xterm SGR** on the same serial stream (see kernel-rpi); the QEMU
# **display window does not** feed the guest pointer — use a terminal that forwards SGR mouse, or
# Tab / arrow keys. Do **not** add usb-kbd expecting it to work: QEMU may route keys there while
# the guest has no USB-HID driver, so keys vanish.
#
# Usage: ./scripts/run-raspi-qemu.sh [pi3|pi4]
#   RPI_QEMU_NET=0       — omit -netdev / usb-net (if your QEMU/UTM setup already provides NICs).
#   RPI_QEMU_USB_KBD=1  — add usb-kbd (still unused by Eve; only if you know you need it).
#   RPI_QEMU_SERIAL_MON=1 — use -serial mon:stdio (monitor + UART multiplexed; Ctrl-a c toggles).
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
if [[ "${RPI_QEMU_USB_KBD:-0}" != 0 ]]; then
  NET_USB+=(-device usb-kbd)
fi
if [[ "${#NET_USB[@]}" -gt 0 ]]; then
  NET_USB=(-usb "${NET_USB[@]}")
fi

# Plain stdio → guest PL011. `mon:stdio` multiplexes the QEMU monitor and often looks like a
# dead keyboard until you toggle (Ctrl-a c); default avoids that. UTM/docs match `-monitor none`.
SERIAL_ARGS=(-serial stdio -monitor none)
if [[ "${RPI_QEMU_SERIAL_MON:-0}" != 0 ]]; then
  SERIAL_ARGS=(-serial mon:stdio)
fi

exec qemu-system-aarch64 \
  -M "$MACHINE" \
  -m "$MEM" \
  -kernel "$IMG" \
  "${SERIAL_ARGS[@]}" \
  "${NET_USB[@]}"
