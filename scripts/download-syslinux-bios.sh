#!/usr/bin/env bash
# Download Syslinux BIOS files (isolinux.bin, memdisk, isohdpfx.bin) from kernel.org for macOS
# or any host where distro packages omit them. Pinned release matches common Linux distros.
#
# Output: tools/syslinux-bios/{isolinux.bin,ldlinux.c32,libcom32.c32,libutil.c32,memdisk,isohdpfx.bin}
#   (ldlinux.c32 + lib*.c32 next to memdisk on the ISO — avoids COM32 load failures / reboot loops.)
# Then run: ./scripts/build-x86-iso.sh
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
DEST="$ROOT/tools/syslinux-bios"
VER="${EVE_SYSLINUX_VERSION:-6.03}"
BASE="https://mirrors.edge.kernel.org/pub/linux/utils/boot/syslinux"
ARCHIVE="syslinux-${VER}.tar.gz"
URL="${BASE}/${ARCHIVE}"

die() { echo "error: $*" >&2; exit 1; }

command -v curl &>/dev/null || die "install curl"
command -v tar &>/dev/null || die "install tar"

mkdir -p "$DEST"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

echo "fetching $URL"
curl -fsSL -o "$TMP/$ARCHIVE" "$URL"

PREFIX="syslinux-${VER}"
tar -xzf "$TMP/$ARCHIVE" -C "$TMP" \
  "${PREFIX}/bios/core/isolinux.bin" \
  "${PREFIX}/bios/com32/elflink/ldlinux/ldlinux.c32" \
  "${PREFIX}/bios/com32/lib/libcom32.c32" \
  "${PREFIX}/bios/com32/libutil/libutil.c32" \
  "${PREFIX}/bios/memdisk/memdisk" \
  "${PREFIX}/bios/mbr/isohdpfx.bin"

cp -f "$TMP/${PREFIX}/bios/core/isolinux.bin" "$DEST/isolinux.bin"
cp -f "$TMP/${PREFIX}/bios/com32/elflink/ldlinux/ldlinux.c32" "$DEST/ldlinux.c32"
cp -f "$TMP/${PREFIX}/bios/com32/lib/libcom32.c32" "$DEST/libcom32.c32"
cp -f "$TMP/${PREFIX}/bios/com32/libutil/libutil.c32" "$DEST/libutil.c32"
cp -f "$TMP/${PREFIX}/bios/memdisk/memdisk" "$DEST/memdisk"
cp -f "$TMP/${PREFIX}/bios/mbr/isohdpfx.bin" "$DEST/isohdpfx.bin"

echo "wrote:"
ls -la "$DEST"
