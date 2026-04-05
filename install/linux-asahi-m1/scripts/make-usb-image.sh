#!/usr/bin/env bash
# Build a 64 MiB FAT image with EFI/BOOT/BOOTAA64.EFI for USB / external boot testing.
# Requires mtools. Run after populate-from-repo.sh (needs EFI/EVE/BOOTAA64.EFI).
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BUNDLE="$(cd "$SCRIPT_DIR/.." && pwd)"
SRC="$BUNDLE/EFI/EVE/BOOTAA64.EFI"
OUT="$BUNDLE/eve-asahi-m1-usb.img"

command -v mformat &>/dev/null && command -v mcopy &>/dev/null || {
  echo "error: install mtools (mtools package)" >&2
  exit 1
}
[[ -f "$SRC" ]] || {
  echo "error: missing $SRC — run ./scripts/populate-from-repo.sh first" >&2
  exit 1
}

rm -f "$OUT"
dd if=/dev/zero of="$OUT" bs=1048576 count=64 status=none
mformat -F -i "$OUT" ::
mmd -i "$OUT" ::EFI
mmd -i "$OUT" ::EFI/BOOT
mcopy -i "$OUT" -D o "$SRC" ::EFI/BOOT/BOOTAA64.EFI
ls -la "$OUT"
echo "OK: write to USB with: sudo dd if=$OUT of=/dev/sdX bs=4M conv=fsync status=progress"
