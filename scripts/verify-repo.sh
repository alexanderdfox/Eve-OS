#!/usr/bin/env bash
# Smoke-check that Eve crates compile with their real targets.
# Do not use `cargo check --workspace` alone: the x86 kernel is x86_64-only and
# will fail when Cargo checks it for the host architecture.
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
run cargo check -p kernel --target x86_64-unknown-none
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
echo "OK: verify-repo.sh finished successfully."
