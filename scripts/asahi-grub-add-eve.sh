#!/usr/bin/env bash
# Add Eve AArch64 UEFI (BOOTAA64.EFI) to GRUB on Asahi Linux
# + Install pretty GRUB theme (fixed + safe)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
DEFAULT_EFI="$REPO_ROOT/utm/arm-uefi/bootaa64.efi"

GRUB_SNIPPET="${GRUB_SNIPPET:-/etc/grub.d/42_eve_os}"
EFI_DEST_DIR="${EFI_DEST_DIR:-EFI/EVE}"
EFI_NAME="BOOTAA64.EFI"
ESP="${ESP:-/boot/efi}"

THEME_DIR="/boot/grub/themes/eve"
GRUB_DEFAULT="/etc/default/grub"

die() { echo "error: $*" >&2; exit 1; }

# --- checks ---
[[ "$(uname -m)" == "aarch64" ]] || die "run this on Asahi Linux (aarch64)"
[[ "$(id -u)" -eq 0 ]] || die "run as root (sudo)"
[[ -d "$ESP" ]] || die "ESP not mounted at $ESP"

# --- remove ---
if [[ "${1:-}" == "--remove" ]]; then
  echo "Removing Eve entry + theme..."

  rm -f "$GRUB_SNIPPET"
  rm -rf "$THEME_DIR" 2>/dev/null || true

  if [[ -d "$ESP/$EFI_DEST_DIR" ]]; then
    rm -f "$ESP/$EFI_DEST_DIR/$EFI_NAME"
    rmdir "$ESP/$EFI_DEST_DIR" 2>/dev/null || true
  fi

  sed -i '/GRUB_THEME=/d' "$GRUB_DEFAULT" || true

  echo "Updating GRUB..."
  if command -v grub2-mkconfig &>/dev/null; then
    grub2-mkconfig -o /boot/grub2/grub.cfg
  elif command -v grub-mkconfig &>/dev/null; then
    grub-mkconfig -o /boot/grub/grub.cfg
  elif command -v update-grub &>/dev/null; then
    update-grub
  else
    die "no GRUB config tool found"
  fi

  echo "✅ Removed."
  exit 0
fi

# --- EFI source ---
SRC_EFI="${1:-$DEFAULT_EFI}"
[[ -f "$SRC_EFI" ]] || die "missing EFI binary: $SRC_EFI"

# --- install EFI ---
TARGET_DIR="$ESP/$EFI_DEST_DIR"
mkdir -p "$TARGET_DIR"

if [[ -f "$TARGET_DIR/$EFI_NAME" ]]; then
  cp -a "$TARGET_DIR/$EFI_NAME" \
    "$TARGET_DIR/${EFI_NAME}.bak.$(date +%Y%m%d%H%M%S)" || true
fi

cp -f "$SRC_EFI" "$TARGET_DIR/$EFI_NAME"
sync

echo "Installed EFI → $TARGET_DIR/$EFI_NAME"

# --- detect ESP ---
command -v findmnt &>/dev/null || die "findmnt not available"
command -v blkid &>/dev/null || die "blkid not available"

SRC_DEV=$(findmnt -n -o SOURCE "$ESP")
UUID=$(blkid -s UUID -o value "$SRC_DEV" | head -1)
[[ -n "$UUID" ]] || die "could not get UUID"

echo "ESP device: $SRC_DEV"
echo "ESP UUID:   $UUID"

CHAIN="/${EFI_DEST_DIR}/${EFI_NAME}"

# --- write GRUB entry (FIXED SAFE VERSION) ---
cat > "$GRUB_SNIPPET" <<EOF
#!/bin/sh
cat <<'GRUBEOF'
menuentry "Eve OS (AArch64 UEFI demo)" --class eve --class efi --class gnu-linux --id eve-aarch64-uefi {
    insmod part_gpt
    insmod fat
    if search --no-floppy --file /usr/lib/grub/arm64-efi/chain.mod --set=chainroot; then
      insmod (\$chainroot)/usr/lib/grub/arm64-efi/chain.mod
    elif search --no-floppy --file /boot/grub2/arm64-efi/chain.mod --set=chainroot; then
      insmod (\$chainroot)/boot/grub2/arm64-efi/chain.mod
    elif search --no-floppy --file /boot/grub/arm64-efi/chain.mod --set=chainroot; then
      insmod (\$chainroot)/boot/grub/arm64-efi/chain.mod
    elif search --no-floppy --file /grub2/arm64-efi/chain.mod --set=chainroot; then
      insmod (\$chainroot)/grub2/arm64-efi/chain.mod
    elif search --no-floppy --file /grub/arm64-efi/chain.mod --set=chainroot; then
      insmod (\$chainroot)/grub/arm64-efi/chain.mod
    else
      echo "error: arm64-efi chain.mod not found — Fedora: dnf install grub2-efi-aa64-modules"
      echo "error: Debian/Ubuntu: apt install grub-efi-arm64  then re-run asahi-grub-add-eve.sh"
      sleep 15
    fi
    search --no-floppy --fs-uuid --set=root ${UUID}
    chainloader (\$root)${CHAIN}
    boot
}
GRUBEOF
EOF

chmod 0755 "$GRUB_SNIPPET"
echo "Created GRUB entry → $GRUB_SNIPPET"

# =========================
# 🎨 THEME INSTALL
# =========================

echo "Installing GRUB theme..."

mkdir -p "$THEME_DIR/icons" "$THEME_DIR/fonts"

# helper for imagemagick
img_convert() {
  local in="$1"
  local out="$2"
  local size="$3"

  if command -v magick &>/dev/null; then
    magick "$in" -resize "$size" "$out"
  elif command -v convert &>/dev/null; then
    convert "$in" -resize "$size" "$out"
  else
    mv "$in" "$out"
  fi
}

# background
cat > "$THEME_DIR/bg.ppm" <<'EOF'
P3
4 4
255
10 10 10  20 20 20  30 30 30  40 40 40
20 20 20  30 30 30  40 40 40  50 50 50
30 30 30  40 40 40  50 50 50  60 60 60
40 40 40  50 50 50  60 60 60  70 70 70
EOF

img_convert "$THEME_DIR/bg.ppm" "$THEME_DIR/background.png" "1920x1080"

# icons
make_icon() {
  local name="$1"
  local color="$2"

  local ppm="$THEME_DIR/icons/$name.ppm"
  local png="$THEME_DIR/icons/$name.png"

  cat > "$ppm" <<EOF
P3
8 8
255
$(yes "$color" | head -n 64)
EOF

  img_convert "$ppm" "$png" "64x64"
}

make_icon eve "0 200 255"
make_icon linux "255 140 0"
make_icon efi "180 180 180"

# font
if command -v grub-mkfont &>/dev/null && [[ -f /usr/share/fonts/TTF/DejaVuSans.ttf ]]; then
  grub-mkfont -s 18 -o "$THEME_DIR/fonts/dejavu.pf2" /usr/share/fonts/TTF/DejaVuSans.ttf
  FONT="DejaVu Sans 18"
else
  FONT="Unifont Regular 16"
fi

# theme config
cat > "$THEME_DIR/theme.txt" <<EOF
desktop-image: "background.png"
desktop-color: "#000000"

terminal-font: "$FONT"

+ boot_menu {
  left = 20%
  top = 20%
  width = 60%
  height = 50%

  item_font = "$FONT"
  item_color = "#cccccc"
  selected_item_color = "#ffffff"

  item_height = 40
  item_padding = 10

  icon_width = 32
  icon_height = 32
}

+ label {
  text = "Boot Menu"
  left = 0
  top = 8%
  width = 100%
  align = "center"
  font = "$FONT"
  color = "#ffffff"
}

+ label {
  text = "Use ↑ ↓ to navigate • Enter to boot"
  left = 0
  top = 85%
  width = 100%
  align = "center"
  font = "$FONT"
  color = "#888888"
}
EOF

# enable theme
grep -q GRUB_THEME "$GRUB_DEFAULT" \
  && sed -i "s|^GRUB_THEME=.*|GRUB_THEME=\"$THEME_DIR/theme.txt\"|" "$GRUB_DEFAULT" \
  || echo "GRUB_THEME=\"$THEME_DIR/theme.txt\"" >> "$GRUB_DEFAULT"

grep -q GRUB_GFXMODE "$GRUB_DEFAULT" || echo "GRUB_GFXMODE=auto" >> "$GRUB_DEFAULT"
grep -q GRUB_GFXPAYLOAD_LINUX "$GRUB_DEFAULT" || echo "GRUB_GFXPAYLOAD_LINUX=keep" >> "$GRUB_DEFAULT"

# =========================
# 🔄 UPDATE GRUB
# =========================

echo "Updating GRUB..."

if command -v grub2-mkconfig &>/dev/null; then
  grub2-mkconfig -o /boot/grub2/grub.cfg
elif command -v grub-mkconfig &>/dev/null; then
  grub-mkconfig -o /boot/grub/grub.cfg
elif command -v update-grub &>/dev/null; then
  update-grub
else
  die "no GRUB config tool found"
fi

echo ""
echo "✅ DONE!"
echo "Reboot → Enjoy your themed GRUB with Eve OS 🎉"
echo ""
echo "Remove everything with:"
echo "  sudo $0 --remove"
