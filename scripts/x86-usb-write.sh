#!/usr/bin/env bash
# Write Eve x86_64 disk image to a USB stick (whole disk — erases the drive).
# PC BIOS USB: install/pc-x86-64-bios-usb/INSTALL.txt
# PC UEFI USB: install/pc-x86-64-uefi-usb/INSTALL.txt
# One stick, UEFI + legacy BIOS (Syslinux → memdisk → eve-bios.img): install/pc-x86-64-unified-usb/INSTALL.txt
#
#   --bios   Legacy/CSM boot (MBR), default if flag omitted — utm/eve-bios.img
#   --uefi   UEFI boot (GPT + ESP) — utm/eve-uefi.img
#   --iso    Hybrid ISO (El Torito UEFI + ISOLINUX/memdisk + embedded eve-bios.img) — utm/eve-x86_64.iso
#
# Usage:
#   ./scripts/x86-usb-write.sh --bios /dev/sdb
#   ./scripts/x86-usb-write.sh --uefi /dev/sdb
#   ./scripts/x86-usb-write.sh --iso /dev/sdb
#   ./scripts/x86-usb-write.sh /dev/disk3          # macOS, BIOS image
#
# Build images first:  ./scripts/build-all-images.sh  or  ./scripts/utm-sync.sh
# Hybrid ISO:          ./scripts/build-x86-iso.sh   (needs xorriso, sgdisk; Syslinux for BIOS path — see install/pc-x86-64-iso/)
#
# Linux: run as root. macOS: run as root; use whole disk e.g. /dev/disk3 (not disk3s1).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIOS_IMG="$ROOT/utm/eve-bios.img"
UEFI_IMG="$ROOT/utm/eve-uefi.img"
ISO_IMG="$ROOT/utm/eve-x86_64.iso"

die() { echo "error: $*" >&2; exit 1; }

MODE=bios
while [[ "${1:-}" == -* ]]; do
  case "$1" in
    --bios) MODE=bios; shift ;;
    --uefi) MODE=uefi; shift ;;
    --iso) MODE=iso; shift ;;
    -h|--help)
      echo "Usage: $0 [--bios|--uefi|--iso] <whole-disk-device>"
      echo "  --bios  MBR/legacy raw image (utm/eve-bios.img) — default"
      echo "  --uefi  GPT/ESP raw image (utm/eve-uefi.img)"
      echo "  --iso   Hybrid ISO: UEFI (El Torito, same payload as eve-uefi.img ESP) +"
      echo "          BIOS/CSM via ISOLINUX + memdisk loading eve-bios.img (utm/eve-x86_64.iso)"
      echo "          Build ISO first: ./scripts/build-x86-iso.sh — install/pc-x86-64-iso/"
      echo "  One-stick guide: install/pc-x86-64-unified-usb/INSTALL.txt"
      exit 0
      ;;
    *) die "unknown flag: $1" ;;
  esac
done

[[ "${1:-}" ]] || die "usage: $0 [--bios|--uefi] <whole-disk-device>"

DEV_RAW=$1
shift
[[ $# -eq 0 ]] || die "too many arguments"

case "$MODE" in
  uefi) IMG=$UEFI_IMG ;;
  iso) IMG=$ISO_IMG ;;
  *) IMG=$BIOS_IMG ;;
esac

if [[ "$MODE" == iso ]]; then
  [[ -f "$IMG" ]] || die "missing $ISO_IMG — run: ./scripts/build-x86-iso.sh (needs xorriso, sgdisk; Syslinux for full BIOS+UEFI hybrid — install/pc-x86-64-iso/)"
else
  [[ -f "$IMG" ]] || die "missing $IMG — run ./scripts/build-all-images.sh (UEFI needs release build with uefi.img)"
fi

whole_disk_ok() {
  local d=$1
  [[ "$d" =~ ^/dev/sd[a-z]$ ]] && return 0
  [[ "$d" =~ ^/dev/vd[a-z]$ ]] && return 0
  [[ "$d" =~ ^/dev/nvme[0-9]+n[0-9]+$ ]] && return 0
  [[ "$d" =~ ^/dev/disk[0-9]+$ ]] && return 0
  return 1
}

whole_disk_ok "$DEV_RAW" || die "use a whole disk (e.g. /dev/sdb, /dev/nvme0n1, /dev/disk3), not a partition"

[[ "$(id -u)" -eq 0 ]] || die "run as root: sudo $0 ..."

if [[ $(uname -s) != Darwin ]]; then
  [[ -b "$DEV_RAW" ]] || die "not a block device: $DEV_RAW"
fi

# macOS: prefer raw device for dd speed
DD_DEV=$DEV_RAW
if [[ "$DEV_RAW" =~ ^/dev/disk[0-9]+$ ]]; then
  RD="${DEV_RAW/disk/rdisk}"
  [[ -e "$RD" ]] && DD_DEV=$RD
fi

echo "Image:  $IMG ($(wc -c <"$IMG" | tr -d ' ') bytes)"
echo "Target: $DD_DEV"
echo ""
echo "This will DESTROY all data on $DEV_RAW"
read -r -p "Type YES to continue: " ok
[[ "$ok" == YES ]] || die "aborted"

OS=$(uname -s)
if [[ "$OS" == Darwin ]]; then
  diskutil unmountDisk force "$DEV_RAW" 2>/dev/null || true
else
  if command -v lsblk &>/dev/null; then
    while read -r p; do
      umount "$p" 2>/dev/null || true
    done < <(lsblk -nrp -o NAME "$DEV_RAW" 2>/dev/null | tail -n +2)
  fi
fi

echo "Writing (dd)…"
dd if="$IMG" of="$DD_DEV" bs=4M conv=fsync status=progress
sync
echo "OK: USB ready. Eject safely, then boot the PC from USB (BIOS boot menu or UEFI boot override)."
