#!/usr/bin/env bash
# Pretty GRUB Theme + Eve OS integration (Asahi Linux, ARM64)

set -euo pipefail

THEME_DIR="/boot/grub/themes/eve"
GRUB_DEFAULT="/etc/default/grub"
GRUB_SNIPPET="/etc/grub.d/42_eve_os"

die() { echo "error: $*" >&2; exit 1; }

echo "== Pretty GRUB Installer =="

# --- checks ---
[[ "$(id -u)" -eq 0 ]] || die "run as root (sudo)"
[[ -d /boot/grub ]] || die "GRUB directory not found"

# --- create dirs ---
echo "Creating theme directories..."
mkdir -p "$THEME_DIR/icons" "$THEME_DIR/fonts"

# --- create background (simple gradient via ppm) ---
echo "Generating background..."
cat > "$THEME_DIR/background.ppm" <<'EOF'
P3
4 4
255
10 10 10   20 20 20   30 30 30   40 40 40
20 20 20   30 30 30   40 40 40   50 50 50
30 30 30   40 40 40   50 50 50   60 60 60
40 40 40   50 50 50   60 60 60   70 70 70
EOF

# Convert if possible
if command -v convert &>/dev/null; then
  convert "$THEME_DIR/background.ppm" -resize 1920x1080 "$THEME_DIR/background.png"
else
  mv "$THEME_DIR/background.ppm" "$THEME_DIR/background.png"
fi

# --- create simple icons (colored squares as placeholders) ---
echo "Creating icons..."

create_icon() {
  local name="$1"
  local color="$2"
  cat > "$THEME_DIR/icons/$name.ppm" <<EOF
P3
8 8
255
$(yes "$color" | head -n 64)
EOF

  if command -v convert &>/dev/null; then
    convert "$THEME_DIR/icons/$name.ppm" -resize 64x64 "$THEME_DIR/icons/$name.png"
    rm "$THEME_DIR/icons/$name.ppm"
  else
    mv "$THEME_DIR/icons/$name.ppm" "$THEME_DIR/icons/$name.png"
  fi
}

create_icon eve "0 200 255"
create_icon linux "255 140 0"
create_icon efi "180 180 180"

# --- font ---
echo "Installing font..."
if command -v grub-mkfont &>/dev/null && [[ -f /usr/share/fonts/TTF/DejaVuSans.ttf ]]; then
  grub-mkfont -s 18 -o "$THEME_DIR/fonts/dejavu.pf2" /usr/share/fonts/TTF/DejaVuSans.ttf
  FONT_NAME="DejaVu Sans 18"
else
  FONT_NAME="Unifont Regular 16"
fi

# --- theme config ---
echo "Writing theme..."
cat > "$THEME_DIR/theme.txt" <<EOF
desktop-image: "background.png"
desktop-color: "#000000"

terminal-font: "$FONT_NAME"

+ boot_menu {
  left = 20%
  top = 20%
  width = 60%
  height = 50%

  item_font = "$FONT_NAME"
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

  font = "$FONT_NAME"
  color = "#ffffff"
}

+ label {
  text = "Use ↑ ↓ to navigate • Enter to boot"
  left = 0
  top = 85%
  width = 100%
  align = "center"

  font = "$FONT_NAME"
  color = "#888888"
}
EOF

# --- patch GRUB defaults ---
echo "Configuring GRUB..."

grep -q "GRUB_THEME=" "$GRUB_DEFAULT" \
  && sed -i "s|^GRUB_THEME=.*|GRUB_THEME=\"$THEME_DIR/theme.txt\"|" "$GRUB_DEFAULT" \
  || echo "GRUB_THEME=\"$THEME_DIR/theme.txt\"" >> "$GRUB_DEFAULT"

grep -q "GRUB_GFXMODE=" "$GRUB_DEFAULT" \
  || echo "GRUB_GFXMODE=auto" >> "$GRUB_DEFAULT"

grep -q "GRUB_GFXPAYLOAD_LINUX=" "$GRUB_DEFAULT" \
  || echo "GRUB_GFXPAYLOAD_LINUX=keep" >> "$GRUB_DEFAULT"

# --- ensure Eve entry has icon class ---
if [[ -f "$GRUB_SNIPPET" ]]; then
  echo "Patching Eve OS entry for icon..."

  sed -i 's/menuentry "Eve OS.*/menuentry "Eve OS (AArch64 UEFI demo)" --class eve --class efi --class gnu-linux --id eve-aarch64-uefi {/' "$GRUB_SNIPPET"
fi

# --- rebuild grub ---
echo "Updating GRUB..."

if command -v grub2-mkconfig &>/dev/null; then
  if [[ -d /boot/grub2 ]]; then
    grub2-mkconfig -o /boot/grub2/grub.cfg
  else
    grub2-mkconfig -o /boot/grub/grub.cfg
  fi
elif command -v grub-mkconfig &>/dev/null; then
  grub-mkconfig -o /boot/grub/grub.cfg
elif command -v update-grub &>/dev/null; then
  update-grub
else
  die "no GRUB config tool found"
fi

echo ""
echo "✅ DONE!"
echo "Reboot to see your new themed GRUB menu 🎉"
