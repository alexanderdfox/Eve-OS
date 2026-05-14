#!/usr/bin/env bash
# Run the i686 Multiboot guest under QEMU (i386 machine type).
# Picks boot media in order: utm/eve-i686.iso → utm/eve-i686.img → direct -kernel ELF.
# If no kernel binary exists, runs ./scripts/build-kernel-i686.sh release (nightly + build-std).
#
# Usage: ./scripts/qemu-i386.sh [extra qemu args…]
# Env:   EVE_QEMU_I386_EXTRA — optional extra qemu flags (space-separated; word-split, no quotes in paths)
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT"

die() { echo "error: $*" >&2; exit 1; }

if [[ -f "$HOME/.cargo/env" ]]; then
  # shellcheck source=/dev/null
  source "$HOME/.cargo/env"
fi

command -v qemu-system-i386 &>/dev/null || die "install QEMU i386 (e.g. brew install qemu)"

TGT="$(cargo metadata --format-version 1 --no-deps | python3 -c "import json,sys; print(json.load(sys.stdin)['target_directory'])")"
ISO="$ROOT/utm/eve-i686.iso"
IMG="$ROOT/utm/eve-i686.img"
K_REL="$TGT/i686-unknown-none/release/kernel-i686"
K_DBG="$TGT/i686-unknown-none/debug/kernel-i686"

run() {
  if [[ -z "${EVE_QEMU_I386_EXTRA:-}" ]]; then
    exec qemu-system-i386 -machine pc -m 128M -serial stdio -monitor none "$@"
  fi
  # shellcheck disable=SC2086
  exec qemu-system-i386 -machine pc -m 128M -serial stdio -monitor none $EVE_QEMU_I386_EXTRA "$@"
}

if [[ -f "$ISO" ]]; then
  echo "qemu-i386: booting CD-ROM $ISO" >&2
  run -cdrom "$ISO" "$@"
elif [[ -f "$IMG" ]]; then
  echo "qemu-i386: booting disk $IMG" >&2
  run -drive "file=$IMG,format=raw" "$@"
elif [[ -f "$K_REL" ]]; then
  echo "qemu-i386: direct kernel $K_REL" >&2
  run -kernel "$K_REL" "$@"
elif [[ -f "$K_DBG" ]]; then
  echo "qemu-i386: direct kernel (debug) $K_DBG" >&2
  run -kernel "$K_DBG" "$@"
else
  echo "qemu-i386: building kernel-i686 (release)…" >&2
  if ! rustup toolchain list 2>/dev/null | grep -q '^nightly'; then
    die "install nightly: rustup toolchain install nightly"
  fi
  "$ROOT/scripts/build-kernel-i686.sh" release
  [[ -f "$K_REL" ]] || die "still missing $K_REL after build"
  run -kernel "$K_REL" "$@"
fi
