#!/usr/bin/env bash
# Run on Asahi Linux (aarch64) as root. Installs EFI/EVE/BOOTAA64.EFI to the ESP and
# merges grub.d/41_eve_os into GRUB, then regenerates grub.cfg.
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BUNDLE="$(cd "$SCRIPT_DIR/.." && pwd)"
ESP="${ESP:-/boot/efi}"
GRUB_SNIPPET_SRC="$BUNDLE/grub.d/41_eve_os"
GRUB_SNIPPET_DST="/etc/grub.d/41_eve_os"
EFI_SRC="$BUNDLE/EFI/EVE/BOOTAA64.EFI"
EFI_DST_DIR="$ESP/EFI/EVE"
EFI_NAME="BOOTAA64.EFI"

die() { echo "error: $*" >&2; exit 1; }

if [[ "${1:-}" == --remove ]]; then
  [[ "$(uname -m)" == aarch64 ]] || die "this script targets Asahi Linux (aarch64)"
  [[ "$(id -u)" -eq 0 ]] || die "run as root: sudo $0 --remove"
  [[ -f "$GRUB_SNIPPET_DST" ]] && rm -f "$GRUB_SNIPPET_DST"
  if [[ -d "$ESP/EFI/EVE" ]]; then
    rm -f "$ESP/EFI/EVE/$EFI_NAME"
    rmdir "$ESP/EFI/EVE" 2>/dev/null || true
  fi
  if command -v grub-mkconfig &>/dev/null; then
    if [[ -d /boot/grub2 ]]; then
      grub-mkconfig -o /boot/grub2/grub.cfg
    elif [[ -d /boot/grub ]]; then
      grub-mkconfig -o /boot/grub/grub.cfg
    fi
  elif command -v update-grub &>/dev/null; then
    update-grub
  fi
  echo "OK: removed Eve GRUB entry and $ESP/EFI/EVE/$EFI_NAME"
  exit 0
fi

[[ "$(uname -m)" == aarch64 ]] || die "this script targets Asahi Linux (aarch64)"
[[ "$(id -u)" -eq 0 ]] || die "run as root: sudo $0"
[[ -d "$ESP" ]] || die "ESP not mounted at $ESP (try: ESP=/your/esp sudo $0)"
[[ -f "$GRUB_SNIPPET_SRC" ]] || die "missing $GRUB_SNIPPET_SRC"
[[ -f "$EFI_SRC" ]] || die "missing $EFI_SRC — on your build machine run: ./scripts/populate-from-repo.sh"

mkdir -p "$EFI_DST_DIR"
if [[ -f "$EFI_DST_DIR/$EFI_NAME" ]]; then
  cp -a "$EFI_DST_DIR/$EFI_NAME" "$EFI_DST_DIR/${EFI_NAME}.bak.$(date +%Y%m%d%H%M%S)" 2>/dev/null || true
fi
cp -f "$EFI_SRC" "$EFI_DST_DIR/$EFI_NAME"
echo "OK: installed $EFI_DST_DIR/$EFI_NAME"

cp -f "$GRUB_SNIPPET_SRC" "$GRUB_SNIPPET_DST"
chmod 0755 "$GRUB_SNIPPET_DST"
echo "OK: installed $GRUB_SNIPPET_DST"

if command -v grub-mkconfig &>/dev/null; then
  if [[ -d /boot/grub2 ]]; then
    grub-mkconfig -o /boot/grub2/grub.cfg
  elif [[ -d /boot/grub ]]; then
    grub-mkconfig -o /boot/grub/grub.cfg
  else
    die "could not find /boot/grub or /boot/grub2"
  fi
elif command -v update-grub &>/dev/null; then
  update-grub
else
  die "install grub2-tools or run grub-mkconfig manually"
fi

echo ""
echo "Done. Reboot → GRUB → \"Eve OS (AArch64 UEFI demo)\"."
echo "Remove: sudo $0 --remove"
echo "See: $BUNDLE/INSTALL.md"
