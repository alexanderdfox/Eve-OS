#!/usr/bin/env bash
# Build AArch64 kernel8.img for Raspberry Pi (Pi 3 / Zero 2 / Pi 4 family UART profile).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if [[ -f "$HOME/.cargo/env" ]]; then
  # shellcheck source=/dev/null
  source "$HOME/.cargo/env"
fi

rustup target add aarch64-unknown-none 2>/dev/null || true

SOC="${RPI_SOC:-pi3}"
FEAT="soc_pi3"
if [[ "$SOC" == "pi4" ]]; then
  FEAT="soc_pi4"
fi

echo "Building kernel-rpi (feature $FEAT, target aarch64-unknown-none)…"
cargo build -p kernel-rpi --release --no-default-features --features "$FEAT" --target aarch64-unknown-none

TGT="$(cargo metadata --format-version 1 --no-deps | python3 -c "import json,sys; print(json.load(sys.stdin)['target_directory'])")"
ELF="$TGT/aarch64-unknown-none/release/kernel-rpi"
if [[ ! -f "$ELF" ]]; then
  echo "error: ELF not found at $ELF" >&2
  exit 1
fi

mkdir -p "$ROOT/rpi/dist"
OUT="$ROOT/rpi/dist/kernel8.img"

HOST="$(rustc -vV | sed -n 's/^host: //p')"
SYSROOT="$(rustc --print sysroot)"
TOOL="$SYSROOT/lib/rustlib/$HOST/bin/llvm-objcopy"
if [[ -x "$TOOL" ]]; then
  OBJCOPY=("$TOOL")
elif command -v llvm-objcopy &>/dev/null; then
  OBJCOPY=(llvm-objcopy)
else
  echo "error: install llvm-tools-preview: rustup component add llvm-tools-preview" >&2
  exit 1
fi

"${OBJCOPY[@]}" -O binary "$ELF" "$OUT"
ls -la "$OUT"
echo "OK: $OUT (flash FAT boot partition or use rpi-assemble-boot.sh)"
