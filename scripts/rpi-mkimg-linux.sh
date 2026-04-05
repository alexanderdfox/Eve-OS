#!/usr/bin/env bash
# Build a raw SD card image (MBR + FAT32 boot) — Linux host with losetup/parted/mkfs.vfat.
# Usage: sudo ./scripts/rpi-mkimg-linux.sh [size_mb default 256]
set -euo pipefail

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "This script only runs on Linux (needs losetup). On macOS, copy rpi/dist/boot/* with Raspberry Pi Imager or dd a FAT partition manually." >&2
  exit 1
fi

if [[ "${EUID:-}" -ne 0 ]]; then
  echo "Run as root (sudo) for loop devices." >&2
  exit 1
fi

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SIZE_MB="${1:-256}"
IMG="$ROOT/rpi/eve-rpi-sdcard.img"

if [[ ! -d "$ROOT/rpi/dist/boot" ]]; then
  echo "error: run scripts/rpi-assemble-boot.sh first" >&2
  exit 1
fi

rm -f "$IMG"
dd if=/dev/zero of="$IMG" bs=1M count="$SIZE_MB" status=progress
LOOP="$(losetup -f --show "$IMG")"
cleanup() {
  losetup -d "$LOOP" 2>/dev/null || true
}
trap cleanup EXIT

parted -s "$LOOP" mklabel msdos
parted -s "$LOOP" mkpart primary fat32 1MiB 100%
parted -s "$LOOP" set 1 boot on
partprobe "$LOOP" || true
sleep 1

PART="${LOOP}p1"
[[ -b "$PART" ]] || PART="${LOOP}p1"

mkfs.vfat -F 32 -n EVEBOOT "$PART"
MNT="$(mktemp -d)"
mount "$PART" "$MNT"
cp -a "$ROOT/rpi/dist/boot/." "$MNT/"
sync
umount "$MNT"
rmdir "$MNT"

echo "OK: $IMG — flash with: dd if=$IMG of=/dev/sdX bs=4M status=progress conv=fsync"
