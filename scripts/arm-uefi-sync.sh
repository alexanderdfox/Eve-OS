#!/usr/bin/env bash
# Build AArch64 UEFI binary and refresh utm/arm-uefi/ for UTM import (FAT + README).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

"$ROOT/scripts/arm-uefi-build.sh"

if ! command -v mformat &>/dev/null || ! command -v mcopy &>/dev/null; then
  echo "error: install mtools: brew install mtools" >&2
  exit 1
fi

TGT="$(cargo metadata --format-version 1 --no-deps | python3 -c "import json,sys; print(json.load(sys.stdin)['target_directory'])")"
EFI="$TGT/aarch64-unknown-uefi/release/bootaa64.efi"
OUT="$ROOT/utm/arm-uefi"
mkdir -p "$OUT"

FAT_IMG="$OUT/eve-arm-uefi-fat.img"
rm -f "$FAT_IMG"
dd if=/dev/zero of="$FAT_IMG" bs=1048576 count=64 status=none
mformat -F -i "$FAT_IMG" ::
mmd -i "$FAT_IMG" ::EFI
mmd -i "$FAT_IMG" ::EFI/BOOT
mcopy -i "$FAT_IMG" -D o "$EFI" ::EFI/BOOT/BOOTAA64.EFI

cp -f "$EFI" "$OUT/bootaa64.efi"
ls -la "$OUT"
echo "OK: UTM bundle under $OUT — utm/ARM-UEFI-SETUP.txt | Asahi M1: utm/ASAHI-M1-UEFI-SETUP.txt"
