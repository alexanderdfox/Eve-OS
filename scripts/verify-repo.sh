#!/usr/bin/env bash
# Smoke-check that Eve crates compile with their real targets.
# The `kernel` binary is behind `--features kernel-bin` (x86_64 + bootloader only); the library
# still type-checks for every host. `cargo check --workspace` is safe on AArch64 hosts.
#
# Usage: ./scripts/verify-repo.sh
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if [[ -f "$HOME/.cargo/env" ]]; then
  # shellcheck source=/dev/null
  source "$HOME/.cargo/env"
fi

echo "==> rustup targets (best effort)"
rustup target add x86_64-unknown-none aarch64-unknown-none aarch64-unknown-uefi 2>/dev/null || true

run() {
  echo ""
  echo "==> $*"
  "$@"
}

run cargo check -p eve-os
run cargo check --workspace
run cargo check -p kernel --target x86_64-unknown-none --features kernel-bin
run cargo check -p kernel-rpi --target aarch64-unknown-none --no-default-features --features soc_pi3
run cargo check -p kernel-rpi --target aarch64-unknown-none --no-default-features --features soc_pi4
run cargo check -p kernel-arm-uefi --target aarch64-unknown-uefi

echo ""
echo "==> shell scripts (bash -n)"
shopt -s nullglob
for f in "$ROOT"/scripts/*.sh; do
  bash -n "$f"
done
shopt -u nullglob

echo ""
echo "==> archive-utm-release.sh (syntax + run; cleans up test dir)"
if [[ -f "$ROOT/scripts/archive-utm-release.sh" ]]; then
  bash -n "$ROOT/scripts/archive-utm-release.sh"
  _arch_tmp="verify-repo-tmp"
  rm -rf "$ROOT/utm/archive/$_arch_tmp"
  EVE_ARCHIVE_LABEL="$_arch_tmp" EVE_ARCHIVE_APPEND_GIT=0 "$ROOT/scripts/archive-utm-release.sh"
  rm -rf "$ROOT/utm/archive/$_arch_tmp"
  echo "  archive script OK"
fi

echo ""
echo "==> i686 32-bit kernel (nightly + build-std; optional)"
if rustup toolchain list 2>/dev/null | grep -q '^nightly'; then
  TI686="$ROOT/kernel/i686-unknown-none.json"
  run cargo +nightly check -p kernel --lib -Z json-target-spec -Z build-std=core,compiler_builtins --target "$TI686"
  run cargo +nightly check -p kernel --features kernel-bin-i686 -Z json-target-spec -Z build-std=core,compiler_builtins --target "$TI686"
else
  echo "  (skip: install nightly to verify i686 — rustup toolchain install nightly)"
fi

echo ""
echo "OK: verify-repo.sh finished successfully."
