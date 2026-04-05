#!/usr/bin/env bash
# Add Eve AArch64 UEFI (BOOTAA64.EFI) to GRUB on Asahi Linux without touching m1n1 or U-Boot.
# Boot chain stays: Apple firmware → m1n1 → U-Boot → EFI/BOOT/BOOTAA64.EFI (GRUB) → your OSes + new menu.
#
# Usage (on Asahi, aarch64, ESP mounted e.g. /boot/efi):
#   ./scripts/arm-uefi-sync.sh    # build utm/arm-uefi/bootaa64.efi first
#   sudo ./scripts/asahi-grub-add-eve.sh
#   sudo ./scripts/asahi-grub-add-eve.sh /path/to/bootaa64.efi
#   sudo ./scripts/asahi-grub-add-eve.sh --remove
#
# Requires: root, GRUB2, ESP at /boot/efi (override ESP=...).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
DEFAULT_EFI="$REPO_ROOT/utm/arm-uefi/bootaa64.efi"
GRUB_SNIPPET="/etc/grub.d/41_eve_os"
EFI_DEST_DIR="${EFI_DEST_DIR:-EFI/EVE}"
EFI_NAME="BOOTAA64.EFI"
ESP="${ESP:-/boot/efi}"

die() { echo "error: $*" >&2; exit 1; }

if [[ "$(uname -m)" != aarch64 ]]; then
  die "run this on Asahi Linux (aarch64). On macOS use utm/ARM-UEFI-SETUP.txt + QEMU."
fi

if [[ "${1:-}" == "--help" || "${1:-}" == -h ]]; then
  sed -n '1,20p' "$0" | tail -n +2
  exit 0
fi

if [[ "${1:-}" == --remove ]]; then
  [[ "$(id -u)" -eq 0 ]] || die "run as root for --remove"
  [[ -f "$GRUB_SNIPPET" ]] && rm -f "$GRUB_SNIPPET"
  if [[ -d "$ESP/$EFI_DEST_DIR" ]]; then
    rm -f "$ESP/$EFI_DEST_DIR/$EFI_NAME"
    rmdir "$ESP/$EFI_DEST_DIR" 2>/dev/null || true
  fi
  echo "Removed Eve GRUB snippet and EFI files. Regenerating GRUB config…"
  if command -v grub-mkconfig &>/dev/null; then
    if [[ -d /boot/grub2 ]]; then
      grub-mkconfig -o /boot/grub2/grub.cfg
    elif [[ -d /boot/grub ]]; then
      grub-mkconfig -o /boot/grub/grub.cfg
    else
      die "could not find /boot/grub or /boot/grub2 for grub-mkconfig -o"
    fi
  elif command -v update-grub &>/dev/null; then
    update-grub
  else
    die "install grub2-tools or run: grub-mkconfig -o /boot/grub/grub.cfg"
  fi
  echo "OK: Eve menu entry removed; default GRUB / other OS entries unchanged."
  exit 0
fi

SRC_EFI="${1:-$DEFAULT_EFI}"
[[ -f "$SRC_EFI" ]] || die "missing EFI binary: $SRC_EFI (run ./scripts/arm-uefi-sync.sh)"

[[ "$(id -u)" -eq 0 ]] || die "run as root (sudo)"

[[ -d "$ESP" ]] || die "ESP not mounted at $ESP (set ESP=/your/esp)"

TARGET_DIR="$ESP/$EFI_DEST_DIR"
mkdir -p "$TARGET_DIR"
if [[ -f "$TARGET_DIR/$EFI_NAME" ]]; then
  cp -a "$TARGET_DIR/$EFI_NAME" "$TARGET_DIR/${EFI_NAME}.bak.$(date +%Y%m%d%H%M%S)" 2>/dev/null || true
fi
cp -f "$SRC_EFI" "$TARGET_DIR/$EFI_NAME"
echo "OK: installed $TARGET_DIR/$EFI_NAME"

# UUID of the filesystem hosting the ESP (for GRUB search)
SRC_DEV=""
if command -v findmnt &>/dev/null; then
  SRC_DEV=$(findmnt -n -o SOURCE "$ESP" 2>/dev/null || true)
fi
[[ -n "$SRC_DEV" ]] || die "findmnt could not resolve block device for $ESP"

UUID=""
if command -v blkid &>/dev/null; then
  UUID=$(blkid -s UUID -o value "$SRC_DEV" 2>/dev/null | head -1 || true)
fi
[[ -n "$UUID" ]] || die "blkid UUID not found for $SRC_DEV"

CHAIN="/${EFI_DEST_DIR}/${EFI_NAME}"
cat >"$GRUB_SNIPPET" <<EOF
#!/bin/sh
cat <<GRUBEOF
menuentry "Eve OS (AArch64 UEFI demo)" --class efi --class gnu-linux --id eve-aarch64-uefi {
  insmod part_gpt
  insmod fat
  search --no-floppy --fs-uuid --set=root $UUID
  chainloader $CHAIN
  boot
}
GRUBEOF
EOF
chmod 0755 "$GRUB_SNIPPET"
echo "OK: wrote $GRUB_SNIPPET (ESP UUID $UUID, chain $CHAIN)"

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
  die "need grub-mkconfig or update-grub; then run: grub-mkconfig -o /boot/grub/grub.cfg"
fi

echo ""
echo "Done. Reboot → GRUB → choose \"Eve OS (AArch64 UEFI demo)\"."
echo "Default boot is still your existing GRUB default (Linux / other entries intact)."
echo "Remove with: sudo $0 --remove"
echo "Docs: utm/ASAHI-M1-UEFI-SETUP.txt"
