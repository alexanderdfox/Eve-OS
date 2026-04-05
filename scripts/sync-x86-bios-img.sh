#!/usr/bin/env bash
# Build eve-os (release) and copy bios.img → utm/eve-bios.img for PC BIOS USB / QEMU.
# Usage: ./scripts/sync-x86-bios-img.sh
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if [[ -f "$HOME/.cargo/env" ]]; then
  # shellcheck source=/dev/null
  source "$HOME/.cargo/env"
fi

echo "Building eve-os (release, produces bios.img via build.rs)…"
cargo build --release -p eve-os

TGT="$(cargo metadata --format-version 1 --no-deps | python3 -c "import json,sys; print(json.load(sys.stdin)['target_directory'])")"
BIOS="$("$ROOT/scripts/eve-disk-out.sh" "$TGT" release bios.img || true)"

if [[ -z "$BIOS" || ! -f "$BIOS" ]]; then
  echo "error: bios.img not found under $TGT after release build" >&2
  exit 1
fi

mkdir -p "$ROOT/utm"
cp -f "$BIOS" "$ROOT/utm/eve-bios.img"
ls -la "$ROOT/utm/eve-bios.img"
echo "OK: flash a BIOS USB with: sudo $ROOT/scripts/x86-usb-write.sh --bios <whole-disk>"
echo "Doc: install/pc-x86-64-bios-usb/INSTALL.txt"
