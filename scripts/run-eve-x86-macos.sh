#!/usr/bin/env bash
# Run the *full* Eve OS (x86_64 kernel + UI + VirtIO net) on a macOS host — including Apple Silicon.
# The guest is always x86_64 under QEMU TCG (slow but complete). Native M1 ARM only runs the tiny
# AArch64 UEFI demo (`kernel-arm-uefi`), not this stack — see utm/MAC-M1-PRO.md
#
# Requires: Rust nightly (rust-toolchain.toml), Homebrew `qemu`, `brew install qemu`.
# Usage:
#   ./scripts/run-eve-x86-macos.sh           # BIOS / i440FX boot disk (same as default `cargo run`)
#   ./scripts/run-eve-x86-macos.sh -- --uefi # Q35 + OVMF UEFI disk
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if [[ -f "$HOME/.cargo/env" ]]; then
  # shellcheck source=/dev/null
  source "$HOME/.cargo/env"
fi

if ! command -v qemu-system-x86_64 &>/dev/null; then
  echo "error: qemu-system-x86_64 not in PATH — on macOS: brew install qemu" >&2
  exit 1
fi

# Extra headroom for JIT + guest framebuffers under TCG (override anytime).
export EVE_QEMU_M="${EVE_QEMU_M:-1024M}"

echo "Eve x86_64 guest RAM: $EVE_QEMU_M (set EVE_QEMU_M to change)"
echo "First boot builds the kernel; expect a long pause. Docs: utm/MAC-M1-PRO.md"
# Cursor and some wrappers set RUSTUP_TOOLCHAIN to an invalid proxy name; clear so +nightly works.
exec env -u RUSTUP_TOOLCHAIN cargo run --release -- "$@"
