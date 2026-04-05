#!/usr/bin/env bash
# Run from anywhere; finds Eve repo via this script’s location. Builds AArch64 UEFI
# and copies bootaa64.efi into install/linux-asahi-m1/EFI/EVE/BOOTAA64.EFI.
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BUNDLE="$(cd "$SCRIPT_DIR/.." && pwd)"
ROOT="$(cd "$BUNDLE/../.." && pwd)"

if [[ ! -f "$ROOT/Cargo.toml" || ! -d "$ROOT/kernel-arm-uefi" ]]; then
  echo "error: could not find Eve repo root (expected Cargo.toml + kernel-arm-uefi)" >&2
  exit 1
fi

cd "$ROOT"
echo "Building AArch64 UEFI and refreshing utm/arm-uefi/…"
if command -v mformat &>/dev/null && command -v mcopy &>/dev/null; then
  "$ROOT/scripts/arm-uefi-sync.sh"
else
  echo "note: mtools not found — only copying bootaa64.efi (install: brew install mtools / apt install mtools)"
  "$ROOT/scripts/arm-uefi-build.sh"
  mkdir -p "$ROOT/utm/arm-uefi"
  TGT="$(cargo metadata --format-version 1 --no-deps | python3 -c "import json,sys; print(json.load(sys.stdin)['target_directory'])")"
  cp -f "$TGT/aarch64-unknown-uefi/release/bootaa64.efi" "$ROOT/utm/arm-uefi/bootaa64.efi"
fi

SRC="$ROOT/utm/arm-uefi/bootaa64.efi"
[[ -f "$SRC" ]] || { echo "error: missing $SRC" >&2; exit 1; }

mkdir -p "$BUNDLE/EFI/EVE"
cp -f "$SRC" "$BUNDLE/EFI/EVE/BOOTAA64.EFI"
ls -la "$BUNDLE/EFI/EVE/BOOTAA64.EFI"
echo "OK: bundle ready at $BUNDLE — copy to Asahi and run: sudo ./scripts/install-on-asahi.sh"
