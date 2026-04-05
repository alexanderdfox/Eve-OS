#!/usr/bin/env bash
# Copy the bootloader BIOS disk image into utm/ for UTM import.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if [[ -f "$HOME/.cargo/env" ]]; then
  # shellcheck source=/dev/null
  source "$HOME/.cargo/env"
fi

echo "Building eve-os (kernel + bios.img)…"
cargo build

TGT="$(cargo metadata --format-version 1 --no-deps 2>/dev/null | python3 -c "import json,sys; print(json.load(sys.stdin)['target_directory'])")"
IMG="$("$ROOT/scripts/eve-disk-out.sh" "$TGT" debug bios.img || true)"

if [[ -z "$IMG" || ! -f "$IMG" ]]; then
  echo "error: bios.img not found under $TGT — run: cargo build" >&2
  exit 1
fi

mkdir -p "$ROOT/utm"
cp -f "$IMG" "$ROOT/utm/eve-bios.img"
ls -la "$ROOT/utm/eve-bios.img"
UEFI="$("$ROOT/scripts/eve-disk-out.sh" "$TGT" debug uefi.img || true)"
if [[ -n "$UEFI" && -f "$UEFI" ]]; then
  cp -f "$UEFI" "$ROOT/utm/eve-uefi.img"
  ls -la "$ROOT/utm/eve-uefi.img"
  echo "OK: UEFI disk at utm/eve-uefi.img"
fi
echo "OK: imported disk ready at utm/eve-bios.img"
echo "Next: utm/UTM-SETUP.txt (x86) or utm/SETUP-ALL-DEVICES.txt (all targets)."
