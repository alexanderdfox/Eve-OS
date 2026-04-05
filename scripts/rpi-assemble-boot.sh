#!/usr/bin/env bash
# Produce rpi/dist/boot/ — FAT root you can copy to an SD card’s boot partition.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
FW="$ROOT/rpi/firmware/boot"
DIST="$ROOT/rpi/dist/boot"

if [[ ! -f "$ROOT/rpi/dist/kernel8.img" ]]; then
  echo "error: run scripts/rpi-build.sh first" >&2
  exit 1
fi
if [[ ! -f "$FW/start.elf" ]]; then
  echo "error: run scripts/rpi-fetch-firmware.sh first" >&2
  exit 1
fi

mkdir -p "$DIST"
cp -f "$FW/start.elf" "$FW/fixup.dat" "$DIST/"
[[ -f "$FW/start4.elf" && -f "$FW/fixup4.dat" ]] && cp -f "$FW/start4.elf" "$FW/fixup4.dat" "$DIST/"
[[ -f "$FW/bootcode.bin" ]] && cp -f "$FW/bootcode.bin" "$DIST/"
cp -f "$FW/LICENCE.broadcom" "$DIST/"
cp -f "$ROOT/rpi/dist/kernel8.img" "$DIST/"
cp -f "$ROOT/rpi/config.txt" "$DIST/"

echo "OK: boot files in $DIST"
echo "Copy everything in that folder to the first (FAT32) partition of a Raspberry Pi SD card."
