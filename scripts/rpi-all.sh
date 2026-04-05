#!/usr/bin/env bash
# Fetch firmware, build kernel8.img, assemble rpi/dist/boot/ (optional: set RPI_SOC=pi4).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
"$ROOT/scripts/rpi-fetch-firmware.sh"
"$ROOT/scripts/rpi-build.sh"
"$ROOT/scripts/rpi-assemble-boot.sh"
echo "Done. See rpi/RPI-IMAGES.txt for the support matrix and flashing steps."
