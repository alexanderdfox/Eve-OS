#!/usr/bin/env bash
# Convert Eve raw disk images under utm/ to qcow2 for UTM import (sparse, copy-on-write).
# Pi kernel blobs (utm/rpi/kernel8-*.img) are loaded with QEMU -kernel, not as drives — skipped.
# Usage: ./scripts/utm-mkqcow2.sh [--force]
# Prereq: qemu-img (brew install qemu)
# After rebuild: re-run this script or `make utm-qcow2` so UTM picks up fresh qcow2 files.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

force=0
if [[ "${1:-}" == "--force" ]]; then
  force=1
fi

QEMU_IMG="${QEMU_IMG:-}"
if [[ -z "$QEMU_IMG" ]] && command -v qemu-img &>/dev/null; then
  QEMU_IMG=qemu-img
fi
if [[ -n "$QEMU_IMG" ]]; then
  echo "using qemu-img: $QEMU_IMG"
else
  echo "using: scripts/qcow2-convert.py (qemu-img not in PATH)"
fi

# raw path (relative to repo) -> qcow2 path (relative to repo)
# Missing raw files are skipped with a note.
DISK_PAIRS=(
  "utm/eve-bios.img|utm/eve-bios.qcow2"
  "utm/eve-uefi.img|utm/eve-uefi.qcow2"
  "utm/eve-i686.img|utm/eve-i686.qcow2"
  "utm/eve-install-target.raw|utm/eve-install-target.qcow2"
  "utm/arm-uefi/eve-arm-uefi-fat.img|utm/arm-uefi/eve-arm-uefi-fat.qcow2"
  "utm/arm-uefi/eve-arm-uefi.img|utm/arm-uefi/eve-arm-uefi.qcow2"
)

# Fall back to build/ copies when utm/ raw is missing (e.g. arm-uefi before sync).
fallback_raw() {
  local rel="$1"
  local p="$ROOT/$rel"
  if [[ -f "$p" ]]; then
    echo "$p"
    return 0
  fi
  local base
  base="$(basename "$rel")"
  if [[ -f "$ROOT/build/$base" ]]; then
    echo "$ROOT/build/$base"
    return 0
  fi
  return 1
}

convert_one() {
  local raw_rel qcow_rel raw qcow
  raw_rel="${1%%|*}"
  qcow_rel="${1#*|}"
  raw="$(fallback_raw "$raw_rel")" || {
    echo "skip (no raw): $raw_rel"
    return 0
  }
  qcow="$ROOT/$qcow_rel"
  mkdir -p "$(dirname "$qcow")"

  if [[ -f "$qcow" && "$force" -eq 0 ]]; then
    # Re-convert when raw is newer than qcow2.
    if [[ "$raw" -nt "$qcow" ]]; then
      :
    else
      echo "OK (up to date): $qcow_rel"
      return 0
    fi
  fi

  echo "converting: $raw_rel → $qcow_rel"
  if [[ -n "$QEMU_IMG" ]]; then
    "$QEMU_IMG" convert -O qcow2 -p "$raw" "$qcow"
    "$QEMU_IMG" info "$qcow" | sed -n '1,3p'
  else
    python3 "$ROOT/scripts/qcow2-convert.py" "$raw" -o "$qcow"
  fi
  ls -la "$qcow"
}

echo "========== utm qcow2 images (disk targets for UTM) =========="
for pair in "${DISK_PAIRS[@]}"; do
  convert_one "$pair"
done

echo ""
echo "Pi kernels (not disk images — use -kernel in UTM QEMU extras):"
for k in utm/rpi/kernel8-pi3.img utm/rpi/kernel8-pi4.img; do
  if [[ -f "$ROOT/$k" ]]; then
    ls -la "$ROOT/$k"
  else
    echo "  (missing) $k"
  fi
done

echo ""
echo "Done. Import *.qcow2 in UTM Drives; see utm/BUILT-IMAGES.md and ./scripts/print-eve-paths.sh"
