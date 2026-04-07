#!/usr/bin/env bash
# Build all RPi AArch64 kernels and copy them next to UTM docs (utm/rpi/).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
"$ROOT/scripts/rpi-build-all.sh"
mkdir -p "$ROOT/utm/rpi"
cp -f "$ROOT/rpi/dist/kernel8-pi3.img" "$ROOT/utm/rpi/kernel8-pi3.img"
cp -f "$ROOT/rpi/dist/kernel8-pi4.img" "$ROOT/utm/rpi/kernel8-pi4.img"
ls -la "$ROOT/utm/rpi/"
echo "OK: UTM-ready kernels in utm/rpi/ — utm/RPI-UTM-SETUP.md (overview: utm/SETUP-ALL-DEVICES.md)"
