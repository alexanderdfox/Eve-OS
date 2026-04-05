#!/usr/bin/env bash
# Build every ship-ready image: x86 BIOS + UEFI disks, RPi Pi3/Pi4 kernels, AArch64 UEFI FAT.
# Usage: ./scripts/build-all-images.sh
# Requires: nightly Rust, aarch64-unknown-none, aarch64-unknown-uefi, llvm-tools (see repo docs).
# Optional: mtools (brew install mtools) for utm/arm-uefi/eve-arm-uefi-fat.img; without it, only bootaa64.efi is copied.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if [[ -f "$HOME/.cargo/env" ]]; then
  # shellcheck source=/dev/null
  source "$HOME/.cargo/env"
fi

rustup target add x86_64-unknown-none aarch64-unknown-none aarch64-unknown-uefi 2>/dev/null || true

echo "========== 1/4 x86_64 Eve (kernel + BIOS/UEFI disk images, release) =========="
cargo build --release -p eve-os

TGT="$(cargo metadata --format-version 1 --no-deps | python3 -c "import json,sys; print(json.load(sys.stdin)['target_directory'])")"
BIOS="$("$ROOT/scripts/eve-disk-out.sh" "$TGT" release bios.img || true)"
UEFI_IMG="$("$ROOT/scripts/eve-disk-out.sh" "$TGT" release uefi.img || true)"

if [[ -z "$BIOS" || ! -f "$BIOS" ]]; then
  echo "error: bios.img not found under $TGT (expected after eve-os build.rs)" >&2
  exit 1
fi

mkdir -p "$ROOT/utm"
cp -f "$BIOS" "$ROOT/utm/eve-bios.img"
echo "OK: $ROOT/utm/eve-bios.img"

if [[ -n "$UEFI_IMG" && -f "$UEFI_IMG" ]]; then
  cp -f "$UEFI_IMG" "$ROOT/utm/eve-uefi.img"
  echo "OK: $ROOT/utm/eve-uefi.img"
else
  echo "warning: uefi.img not found under $TGT (optional for cargo build output layout)" >&2
fi

echo "========== 2/4 Raspberry Pi AArch64 (BCM2837 + BCM2711) =========="
"$ROOT/scripts/rpi-build-all.sh"
mkdir -p "$ROOT/utm/rpi"
cp -f "$ROOT/rpi/dist/kernel8-pi3.img" "$ROOT/utm/rpi/kernel8-pi3.img"
cp -f "$ROOT/rpi/dist/kernel8-pi4.img" "$ROOT/utm/rpi/kernel8-pi4.img"
echo "OK: $ROOT/utm/rpi/kernel8-pi3.img"
echo "OK: $ROOT/utm/rpi/kernel8-pi4.img"

echo "========== 3/4 AArch64 UEFI (QEMU virt / Apple Silicon) =========="
if command -v mformat &>/dev/null && command -v mcopy &>/dev/null; then
  "$ROOT/scripts/arm-uefi-sync.sh"
else
  echo "warning: mtools missing — run: brew install mtools (skipping eve-arm-uefi-fat.img)"
  "$ROOT/scripts/arm-uefi-build.sh"
  EFI="$TGT/aarch64-unknown-uefi/release/bootaa64.efi"
  if [[ ! -f "$EFI" ]]; then
    echo "error: $EFI not found" >&2
    exit 1
  fi
  mkdir -p "$ROOT/utm/arm-uefi"
  cp -f "$EFI" "$ROOT/utm/arm-uefi/bootaa64.efi"
  echo "OK: $ROOT/utm/arm-uefi/bootaa64.efi (no FAT image)"
fi

echo "========== 4/4 Summary =========="
ls -la "$ROOT/utm/eve-bios.img" 2>/dev/null || true
ls -la "$ROOT/utm/eve-uefi.img" 2>/dev/null || true
ls -la "$ROOT/utm/rpi/" 2>/dev/null || true
ls -la "$ROOT/utm/arm-uefi/" 2>/dev/null || true
echo "All image build steps finished."
echo "  utm/BUILT-IMAGES.txt        — list of artifacts"
echo "  utm/SETUP-ALL-DEVICES.txt   — same workflow for every VM target"
echo "  ./scripts/print-eve-paths.sh — absolute paths for UTM / QEMU"
