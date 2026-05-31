#!/usr/bin/env bash
# Print absolute paths to all Eve `utm/` images (for UTM drives, QEMU -kernel, etc.).
# Usage: ./scripts/print-eve-paths.sh
# Copy EVE_ROOT=... into your shell if you want $EVE_ROOT in QEMU argument lines.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

echo "EVE_ROOT=$ROOT"
echo "Mac M1 / Apple Silicon (full OS vs native demo): $ROOT/utm/MAC-M1-PRO.md"
echo "---"
for rel in \
  install/REAL-HARDWARE.md \
  install/pc-x86-64-unified-usb/INSTALL.md \
  utm/eve-bios.img \
  utm/eve-bios.qcow2 \
  utm/eve-uefi.img \
  utm/eve-uefi.qcow2 \
  utm/eve-install-target.qcow2 \
  utm/eve-x86_64.iso \
  utm/eve-i686.iso \
  utm/eve-i686.img \
  utm/eve-i686.qcow2 \
  utm/qemu-extra.args \
  utm/qemu-extra-q35.args \
  utm/qemu-extra-rpi.args \
  utm/qemu-extra-arm-uefi.args \
  utm/rpi/kernel8-pi3.img \
  utm/rpi/kernel8-pi4.img \
  utm/arm-uefi/bootaa64.efi \
  utm/arm-uefi/eve-arm-uefi-fat.img \
  utm/arm-uefi/eve-arm-uefi-fat.qcow2 \
  utm/arm-uefi/eve-arm-uefi.img \
  utm/arm-uefi/eve-arm-uefi.qcow2 \
  utm/arm-uefi/.qemu-edk2-vars.run.fd
do
  p="$ROOT/$rel"
  if [[ -e "$p" ]]; then
    echo "$p"
  else
    echo "# (missing) $p"
  fi
done
for bundle in utm/rpi/Eve-Pi3.utm utm/rpi/Eve-Pi4.utm; do
  p="$ROOT/$bundle"
  if [[ -d "$p" && -f "$p/config.plist" ]]; then
    echo "$p"
  else
    echo "# (missing) $p"
  fi
done
for cmd in utm/rpi/Launch-Eve-Pi3.command utm/rpi/Launch-Eve-Pi4.command; do
  p="$ROOT/$cmd"
  if [[ -x "$p" ]]; then
    echo "$p"
  else
    echo "# (missing) $p"
  fi
done
