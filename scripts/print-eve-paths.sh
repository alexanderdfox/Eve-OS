#!/usr/bin/env bash
# Print absolute paths to all Eve `utm/` images (for UTM drives, QEMU -kernel, etc.).
# Usage: ./scripts/print-eve-paths.sh
# Copy EVE_ROOT=... into your shell if you want $EVE_ROOT in QEMU argument lines.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

echo "EVE_ROOT=$ROOT"
echo "---"
for rel in \
  install/REAL-HARDWARE.txt \
  install/pc-x86-64-unified-usb/INSTALL.txt \
  utm/eve-bios.img \
  utm/eve-uefi.img \
  utm/eve-x86_64.iso \
  utm/qemu-extra.args \
  utm/qemu-extra-q35.args \
  utm/qemu-extra-rpi.args \
  utm/qemu-extra-arm-uefi.args \
  utm/rpi/kernel8-pi3.img \
  utm/rpi/kernel8-pi4.img \
  utm/arm-uefi/bootaa64.efi \
  utm/arm-uefi/eve-arm-uefi-fat.img \
  utm/arm-uefi/.qemu-edk2-vars.run.fd
do
  p="$ROOT/$rel"
  if [[ -e "$p" ]]; then
    echo "$p"
  else
    echo "# (missing) $p"
  fi
done
