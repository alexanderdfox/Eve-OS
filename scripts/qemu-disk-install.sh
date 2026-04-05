#!/usr/bin/env bash
# Boot Eve with VirtIO boot disk + empty VirtIO target for in-guest "INSTALL" clone.
# Prereq: ./scripts/build-all-images.sh or cargo build --release -p eve-os
# Usage: ./scripts/qemu-disk-install.sh [target.raw default: utm/eve-install-target.raw]
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if [[ -f "$HOME/.cargo/env" ]]; then
  # shellcheck source=/dev/null
  source "$HOME/.cargo/env"
fi

RAW="${1:-$ROOT/utm/eve-install-target.raw}"
if [[ ! -f "$RAW" ]]; then
  echo "Creating empty target image: $RAW (96 MiB)"
  qemu-img create -f raw "$RAW" 96M
fi

export EVE_QEMU_INSTALL_TARGET="$RAW"
echo "EVE_QEMU_INSTALL_TARGET=$RAW"
echo "Run INSTALL in the guest, then boot from that .raw alone."
exec cargo run --release -p eve-os
