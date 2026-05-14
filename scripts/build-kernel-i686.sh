#!/usr/bin/env bash
# Build the 32-bit Multiboot `kernel-i686` binary (`kernel` crate, feature `kernel-bin-i686`).
# Requires nightly for `-Z build-std`.
#
# Usage: ./scripts/build-kernel-i686.sh [release]
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
PROFILE="${1:-}"
REL=()
if [[ "$PROFILE" == "release" ]]; then
  REL=(--release)
fi

if ! command -v rustup >/dev/null 2>&1; then
  echo "rustup not found" >&2
  exit 1
fi
if ! rustup toolchain list | grep -q '^nightly'; then
  echo "Install nightly: rustup toolchain install nightly" >&2
  exit 1
fi

exec cargo +nightly build -p kernel \
  --features kernel-bin-i686 \
  -Z json-target-spec \
  -Z build-std=core,compiler_builtins \
  --target "$ROOT/kernel/i686-unknown-none.json" \
  "${REL[@]}"
