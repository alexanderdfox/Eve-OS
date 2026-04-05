#!/usr/bin/env bash
# Build both AArch64 kernels: Pi3-class (BCM2837) and Pi4-class (BCM2711) raw images.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if [[ -f "$HOME/.cargo/env" ]]; then
  # shellcheck source=/dev/null
  source "$HOME/.cargo/env"
fi

rustup target add aarch64-unknown-none 2>/dev/null || true

echo "=== Building Pi3 / Zero2 / 3B+ profile (soc_pi3) ==="
RPI_SOC=pi3 "$ROOT/scripts/rpi-build.sh"
cp -f "$ROOT/rpi/dist/kernel8.img" "$ROOT/rpi/dist/kernel8-pi3.img"
ls -la "$ROOT/rpi/dist/kernel8-pi3.img"

echo "=== Building Pi4 / 400 profile (soc_pi4) ==="
RPI_SOC=pi4 "$ROOT/scripts/rpi-build.sh"
cp -f "$ROOT/rpi/dist/kernel8.img" "$ROOT/rpi/dist/kernel8-pi4.img"
ls -la "$ROOT/rpi/dist/kernel8-pi4.img"

echo "OK: rpi/dist/kernel8-pi3.img and rpi/dist/kernel8-pi4.img"
