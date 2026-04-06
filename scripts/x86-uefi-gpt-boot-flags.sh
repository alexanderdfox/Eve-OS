#!/usr/bin/env bash
# Set GPT attribute bits on partition 1 (ESP) of eve-uefi.img so more PC firmware
# lists / boots the USB: bit 0 = required platform, bit 2 = legacy BIOS bootable
# (helps some USB boot menus even when using UEFI).
#
# Requires: sgdisk (gptfdisk) — brew install gptfdisk / apt install gdisk / dnf install gdisk
# Usage: ./scripts/x86-uefi-gpt-boot-flags.sh [path/to/eve-uefi.img]
#        Also used for utm/arm-uefi/eve-arm-uefi.img (same ESP geometry).
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
IMG="${1:-$ROOT/utm/eve-uefi.img}"

[[ -f "$IMG" ]] || {
  echo "error: missing $IMG" >&2
  exit 1
}

if ! command -v sgdisk &>/dev/null; then
  echo "warning: sgdisk not installed — skipping GPT boot flags on $IMG" >&2
  echo "  Install: brew install gptfdisk   (macOS)   or   apt install gdisk   (Debian/Ubuntu)" >&2
  exit 0
fi

# Eve's rust-osdev uefi.img is a single ESP as GPT partition 1.
sgdisk -A 1:set:0 "$IMG"
sgdisk -A 1:set:2 "$IMG"
echo "OK: GPT attributes set on partition 1 of $IMG (bits 0 and 2)"
