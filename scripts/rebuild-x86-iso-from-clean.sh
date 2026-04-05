#!/usr/bin/env bash
# `cargo clean`, rebuild release images, refresh Syslinux BIOS files, rebuild hybrid ISO.
# Fixes stale kernel / broken ISOLINUX layout (e.g. memdisk reboot loops).
#
# Usage: ./scripts/rebuild-x86-iso-from-clean.sh
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if [[ -f "$HOME/.cargo/env" ]]; then
  # shellcheck source=/dev/null
  source "$HOME/.cargo/env"
fi

echo "==> cargo clean"
cargo clean

echo "==> release UEFI + BIOS disk images"
"$ROOT/scripts/sync-x86-uefi-img.sh"
"$ROOT/scripts/sync-x86-bios-img.sh"

if [[ -x "$ROOT/scripts/download-syslinux-bios.sh" ]]; then
  echo "==> Syslinux BIOS files (isolinux, ldlinux, libcom32, memdisk, isohdpfx)"
  "$ROOT/scripts/download-syslinux-bios.sh"
fi

rm -f "$ROOT/utm/eve-x86_64.iso"
echo "==> ISO"
"$ROOT/scripts/build-x86-iso.sh"

echo "OK: $ROOT/utm/eve-x86_64.iso"
