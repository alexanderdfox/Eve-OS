#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT"

usage() {
  cat <<'EOF'
eve4mac.sh — Apple Silicon / Mac workflows for Eve OS

  --full-os   Full x86_64 Eve in QEMU (macOS or Linux). See utm/MAC-M1-PRO.md
  --asahi     On Linux: sync AArch64 UEFI + sudo GRUB install (default if no flag).
  --arm-only  Build utm/arm-uefi only (no GRUB; OK on macOS).
  --help      This help.

Full desktop Eve is x86_64-only; M1 native chainload is the small UEFI demo only.
EOF
}

case "${1:-}" in
  --help|-h)
    usage
    exit 0
    ;;
  --full-os|--x86)
    shift
    exec "$ROOT/scripts/run-eve-x86-macos.sh" "$@"
    ;;
  --arm-only)
    exec "$ROOT/scripts/arm-uefi-sync.sh"
    ;;
  --asahi)
    shift
    ;;
esac

# --- Asahi path (run on Asahi Linux aarch64, not on macOS) ---
if [[ "$(uname -s)" == "Darwin" ]]; then
  echo "error: this default mode installs GRUB on Asahi Linux — not on macOS." >&2
  echo "  Full Eve OS on Mac:  $0 --full-os" >&2
  echo "  Build ARM UEFI only: $0 --arm-only" >&2
  echo "  Doc: $ROOT/utm/MAC-M1-PRO.md" >&2
  exit 1
fi

SYNC_SCRIPT="./scripts/arm-uefi-sync.sh"
GRUB_SCRIPT="./scripts/asahi-grub-add-eve.sh"

for script in "$SYNC_SCRIPT" "$GRUB_SCRIPT"; do
  if [[ ! -x "$script" ]]; then
    echo "error: missing or not executable $script" >&2
    exit 1
  fi
done

echo "Syncing Eve AArch64 UEFI..."
"$SYNC_SCRIPT"

EFI_OUT="$ROOT/utm/arm-uefi/bootaa64.efi"
[[ -f "$EFI_OUT" ]] || {
  echo "error: expected $EFI_OUT after sync" >&2
  exit 1
}

echo "Adding Eve entry to GRUB..."
sudo "$GRUB_SCRIPT" "$EFI_OUT"

echo "OK: Eve AArch64 UEFI synced and GRUB entry installed."
