#!/usr/bin/env bash
# Download minimal Raspberry Pi GPU boot blobs (Broadcom license — see rpi/RPI-IMAGES.txt).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BOOT="$ROOT/rpi/firmware/boot"
mkdir -p "$BOOT"
BASE="https://github.com/raspberrypi/firmware/raw/master/boot"
for f in start.elf fixup.dat bootcode.bin LICENCE.broadcom; do
  out="$BOOT/$f"
  if [[ -f "$out" ]]; then
    echo "skip existing $f"
    continue
  fi
  echo "fetch $f"
  curl -fsSL -o "$out" "$BASE/$f"
done
echo "OK: firmware in $BOOT"
