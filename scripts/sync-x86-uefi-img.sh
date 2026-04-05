#!/usr/bin/env bash
# Build eve-os (release) and copy uefi.img → utm/eve-uefi.img for PC UEFI USB / QEMU.
# Usage: ./scripts/sync-x86-uefi-img.sh
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if [[ -f "$HOME/.cargo/env" ]]; then
  # shellcheck source=/dev/null
  source "$HOME/.cargo/env"
fi

echo "Building eve-os (release, produces uefi.img via build.rs)…"
cargo build --release -p eve-os

TGT="$(cargo metadata --format-version 1 --no-deps | python3 -c "import json,sys; print(json.load(sys.stdin)['target_directory'])")"
UEFI="$("$ROOT/scripts/eve-disk-out.sh" "$TGT" release uefi.img || true)"

if [[ -z "$UEFI" || ! -f "$UEFI" ]]; then
  echo "error: uefi.img not found under $TGT after release build" >&2
  exit 1
fi

mkdir -p "$ROOT/utm"
cp -f "$UEFI" "$ROOT/utm/eve-uefi.img"
ls -la "$ROOT/utm/eve-uefi.img"
"$ROOT/scripts/x86-uefi-gpt-boot-flags.sh" "$ROOT/utm/eve-uefi.img"
echo "OK: flash a UEFI USB with: sudo $ROOT/scripts/x86-usb-write.sh --uefi <whole-disk>"
echo "Doc: install/pc-x86-64-uefi-usb/INSTALL.txt"
