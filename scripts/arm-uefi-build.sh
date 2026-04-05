#!/usr/bin/env bash
# Build BOOTAA64.EFI (AArch64 UEFI application) for QEMU virt + EDK2.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if [[ -f "$HOME/.cargo/env" ]]; then
  # shellcheck source=/dev/null
  source "$HOME/.cargo/env"
fi

rustup target add aarch64-unknown-uefi 2>/dev/null || true

echo "Building kernel-arm-uefi (aarch64-unknown-uefi, release)…"
cargo build -p kernel-arm-uefi --release --target aarch64-unknown-uefi

TGT="$(cargo metadata --format-version 1 --no-deps | python3 -c "import json,sys; print(json.load(sys.stdin)['target_directory'])")"
EFI="$TGT/aarch64-unknown-uefi/release/bootaa64.efi"
if [[ ! -f "$EFI" ]]; then
  echo "error: expected $EFI" >&2
  exit 1
fi
ls -la "$EFI"
echo "OK: $EFI"
