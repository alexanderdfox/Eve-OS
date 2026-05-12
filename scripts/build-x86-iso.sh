#!/usr/bin/env bash
# Build a hybrid x86_64 ISO: UEFI (El Torito EFI image) + BIOS (ISOLINUX + memdisk → eve-bios.img).
#
# UEFI path: ESP extracted from eve-uefi.img (same as raw USB UEFI). Works on PCs (UEFI) and
#   UTM “Emulate x86_64” with UEFI firmware (e.g. M1/M2/M3 Macs).
# BIOS path: Syslinux memdisk loads eve-bios.img as an emulated hard disk (MBR). PCs with CSM
#   or UTM with BIOS firmware.
#
# When Syslinux ships isohdpfx.bin, xorriso also applies -isohybrid-mbr + -isohybrid-gpt-basdat
# so the same .iso is more likely to boot from a USB stick written with `dd` (UEFI + legacy).
#
# Requires: sgdisk (gptfdisk), xorriso; for BIOS also isolinux.bin + ldlinux.c32 + libcom32.c32 + memdisk
#   (optional libutil.c32; same Syslinux version). isolinux.cfg uses LINUX memdisk (not KERNEL).
#   (Debian/Ubuntu: sudo apt install isolinux syslinux-common xorriso gdisk)
#   (Fedora: sudo dnf install syslinux xorriso gdisk)
#   macOS: brew install gptfdisk xorriso — if isolinux.bin is missing, either run
#   ./scripts/download-syslinux-bios.sh (kernel.org Syslinux 6.03 → tools/syslinux-bios/)
#   or the script defaults to EVE_ISO_UEFI_ONLY=1 (no warning). Full hybrid: Linux packages
#   or the download script + eve-bios.img.
#
# Prereq images: ./scripts/sync-x86-uefi-img.sh && ./scripts/sync-x86-bios-img.sh
#
# Usage: ./scripts/build-x86-iso.sh [utm/eve-uefi.img] [output.iso]
#        EVE_ISO_UEFI_ONLY=1 ./scripts/build-x86-iso.sh   # UEFI El Torito only (no Syslinux)
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
UEFI_IMG="${1:-$ROOT/utm/eve-uefi.img}"
OUT="${2:-$ROOT/utm/eve-x86_64.iso}"
BIOS_IMG="$ROOT/utm/eve-bios.img"

die() { echo "error: $*" >&2; exit 1; }

[[ -f "$UEFI_IMG" ]] || die "missing $UEFI_IMG — run: ./scripts/sync-x86-uefi-img.sh"
command -v sgdisk &>/dev/null || die "install sgdisk (gptfdisk / gdisk package)"
command -v xorriso &>/dev/null || die "install xorriso"

find_isolinux() {
  local f
  for f in \
    "$ROOT/tools/syslinux-bios/isolinux.bin" \
    /usr/lib/ISOLINUX/isolinux.bin \
    /usr/share/syslinux/isolinux.bin \
    /usr/lib/syslinux/isolinux.bin \
    /usr/lib/syslinux/bios/isolinux.bin; do
    [[ -f "$f" ]] && { echo "$f"; return 0; }
  done
  local pfx
  pfx="$(brew --prefix syslinux 2>/dev/null)" || true
  if [[ -n "$pfx" ]]; then
    for f in \
      "$pfx/share/syslinux/isolinux.bin" \
      "$pfx/lib/syslinux/bios/isolinux.bin"; do
      [[ -f "$f" ]] && { echo "$f"; return 0; }
    done
  fi
  return 1
}

find_memdisk() {
  local f
  for f in \
    "$ROOT/tools/syslinux-bios/memdisk" \
    /usr/lib/syslinux/modules/bios/memdisk \
    /usr/lib/syslinux/memdisk \
    /usr/share/syslinux/memdisk; do
    [[ -f "$f" ]] && { echo "$f"; return 0; }
  done
  local pfx
  pfx="$(brew --prefix syslinux 2>/dev/null)" || true
  if [[ -n "$pfx" ]]; then
    for f in \
      "$pfx/share/syslinux/memdisk" \
      "$pfx/lib/syslinux/bios/memdisk"; do
      [[ -f "$f" ]] && { echo "$f"; return 0; }
    done
  fi
  return 1
}

# MBR template for “USB isohybrid” (same package family as isolinux on most distros).
find_isohdpfx() {
  local f
  for f in \
    "$ROOT/tools/syslinux-bios/isohdpfx.bin" \
    /usr/lib/syslinux/bios/isohdpfx.bin \
    /usr/lib/ISOLINUX/isohdpfx.bin \
    /usr/share/syslinux/isohdpfx.bin \
    /usr/lib/syslinux/isohdpfx.bin; do
    [[ -f "$f" ]] && { echo "$f"; return 0; }
  done
  local pfx
  pfx="$(brew --prefix syslinux 2>/dev/null)" || true
  if [[ -n "$pfx" ]]; then
    for f in \
      "$pfx/lib/syslinux/bios/isohdpfx.bin" \
      "$pfx/share/syslinux/isohdpfx.bin"; do
      [[ -f "$f" ]] && { echo "$f"; return 0; }
    done
  fi
  return 1
}

# Syslinux 5+ ISOLINUX loads COM32 via ldlinux.c32 (same directory as isolinux.bin on the ISO).
find_ldlinux_c32() {
  local f
  for f in \
    "$ROOT/tools/syslinux-bios/ldlinux.c32" \
    /usr/lib/syslinux/modules/bios/ldlinux.c32 \
    /usr/lib/syslinux/bios/ldlinux.c32 \
    /usr/share/syslinux/ldlinux.c32; do
    [[ -f "$f" ]] && { echo "$f"; return 0; }
  done
  local pfx
  pfx="$(brew --prefix syslinux 2>/dev/null)" || true
  if [[ -n "$pfx" ]]; then
    for f in \
      "$pfx/lib/syslinux/bios/ldlinux.c32" \
      "$pfx/lib/syslinux/modules/bios/ldlinux.c32" \
      "$pfx/share/syslinux/ldlinux.c32"; do
      [[ -f "$f" ]] && { echo "$f"; return 0; }
    done
  fi
  return 1
}

find_libcom32_c32() {
  local f
  for f in \
    "$ROOT/tools/syslinux-bios/libcom32.c32" \
    /usr/lib/syslinux/modules/bios/libcom32.c32 \
    /usr/lib/syslinux/bios/libcom32.c32 \
    /usr/share/syslinux/libcom32.c32; do
    [[ -f "$f" ]] && { echo "$f"; return 0; }
  done
  local pfx
  pfx="$(brew --prefix syslinux 2>/dev/null)" || true
  if [[ -n "$pfx" ]]; then
    for f in \
      "$pfx/lib/syslinux/modules/bios/libcom32.c32" \
      "$pfx/lib/syslinux/bios/libcom32.c32"; do
      [[ -f "$f" ]] && { echo "$f"; return 0; }
    done
  fi
  return 1
}

find_libutil_c32() {
  local f
  for f in \
    "$ROOT/tools/syslinux-bios/libutil.c32" \
    /usr/lib/syslinux/modules/bios/libutil.c32 \
    /usr/lib/syslinux/bios/libutil.c32 \
    /usr/share/syslinux/libutil.c32; do
    [[ -f "$f" ]] && { echo "$f"; return 0; }
  done
  local pfx
  pfx="$(brew --prefix syslinux 2>/dev/null)" || true
  if [[ -n "$pfx" ]]; then
    for f in \
      "$pfx/lib/syslinux/modules/bios/libutil.c32" \
      "$pfx/lib/syslinux/bios/libutil.c32"; do
      [[ -f "$f" ]] && { echo "$f"; return 0; }
    done
  fi
  return 1
}

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

START=$(sgdisk -i 1 "$UEFI_IMG" 2>/dev/null | sed -n 's/^First sector: *\([0-9][0-9]*\).*/\1/p')
LAST=$(sgdisk -i 1 "$UEFI_IMG" 2>/dev/null | sed -n 's/^Last sector: *\([0-9][0-9]*\).*/\1/p')
[[ -n "$START" && -n "$LAST" ]] || die "could not parse GPT partition 1 from $UEFI_IMG"
COUNT=$((LAST - START + 1))

STAGING="$TMP/iso"
mkdir -p "$STAGING/isolinux"
echo "Eve OS x86_64 — UEFI + BIOS hybrid ISO (see install/pc-x86-64-iso/INSTALL.md)" >"$STAGING/README.md"

EFI_IMG="$STAGING/efiboot.img"
dd if="$UEFI_IMG" of="$EFI_IMG" bs=512 skip="$START" count="$COUNT" status=none

# Some firmware and USB-writing tools expect an EFI path in the ISO filesystem itself.
# Keep El Torito EFI image as the boot source, but mirror BOOTX64.EFI when possible.
mkdir -p "$STAGING/EFI/BOOT"
if command -v mcopy &>/dev/null; then
  if mcopy -i "$EFI_IMG" ::/EFI/BOOT/BOOTX64.EFI "$STAGING/EFI/BOOT/BOOTX64.EFI" 2>/dev/null; then
    :
  else
    echo "note: could not mirror /EFI/BOOT/BOOTX64.EFI from efiboot.img (El Torito EFI boot still works)" >&2
  fi
else
  echo "note: mcopy not found — keeping empty /EFI/BOOT in ISO (El Torito EFI boot still works)" >&2
fi

# macOS: Homebrew Syslinux usually has no isolinux.bin — treat UEFI-only as normal (no warning).
if [[ "$(uname -s)" == Darwin && -z "${EVE_ISO_UEFI_ONLY+x}" ]]; then
  if ! ISOLINUX_PROBE="$(find_isolinux)"; then
    EVE_ISO_UEFI_ONLY=1
  else
    unset ISOLINUX_PROBE
  fi
fi

HYBRID=1
if [[ "${EVE_ISO_UEFI_ONLY:-}" == 1 ]]; then
  HYBRID=0
elif ! ISOLINUX_BIN="$(find_isolinux)"; then
  echo "warning: isolinux.bin not found — building UEFI-only ISO (set EVE_ISO_UEFI_ONLY=1 to silence)" >&2
  echo "  Debian/Ubuntu: sudo apt install isolinux syslinux-common" >&2
  HYBRID=0
elif ! MEMDISK_BIN="$(find_memdisk)"; then
  echo "warning: memdisk not found — building UEFI-only ISO" >&2
  echo "  Debian/Ubuntu: sudo apt install syslinux-common" >&2
  HYBRID=0
elif ! LDLINUX_C32="$(find_ldlinux_c32)"; then
  echo "warning: ldlinux.c32 not found — ISOLINUX would fail loading COM32 (e.g. memdisk); building UEFI-only ISO" >&2
  echo "  Run: ./scripts/download-syslinux-bios.sh  or  Debian/Ubuntu: sudo apt install syslinux-common" >&2
  HYBRID=0
elif ! LIBCOM32="$(find_libcom32_c32)"; then
  echo "warning: libcom32.c32 not found — memdisk may reboot-loop; building UEFI-only ISO" >&2
  echo "  Run: ./scripts/download-syslinux-bios.sh  or  Debian/Ubuntu: sudo apt install syslinux-common" >&2
  HYBRID=0
elif [[ ! -f "$BIOS_IMG" ]]; then
  echo "warning: missing $BIOS_IMG — run ./scripts/sync-x86-bios-img.sh — building UEFI-only ISO" >&2
  HYBRID=0
else
  cp -f "$ISOLINUX_BIN" "$STAGING/isolinux/isolinux.bin"
  cp -f "$LDLINUX_C32" "$STAGING/isolinux/ldlinux.c32"
  cp -f "$LIBCOM32" "$STAGING/isolinux/libcom32.c32"
  if tmpu="$(find_libutil_c32)"; then
    cp -f "$tmpu" "$STAGING/isolinux/libutil.c32"
  fi
  cp -f "$MEMDISK_BIN" "$STAGING/isolinux/memdisk"
  cp -f "$BIOS_IMG" "$STAGING/isolinux/eve-bios.img"
  # Use LINUX memdisk (not KERNEL): memdisk is COM32; KERNEL mishandles .c32 and can triple-fault → reboot loop.
  cat >"$STAGING/isolinux/isolinux.cfg" <<'CFG'
DEFAULT eve
PROMPT 1
TIMEOUT 50
LABEL eve
  SAY Booting Eve OS (BIOS / memdisk + eve-bios.img)…
  LINUX memdisk
  INITRD eve-bios.img
  APPEND harddisk
CFG
fi

ISOHYBRID_MBR=""
if tmp="$(find_isohdpfx)"; then
  ISOHYBRID_MBR="$tmp"
else
  echo "note: isohdpfx.bin not found — ISO still boots as CD/DVD (UEFI/BIOS); USB 'dd' hybrid MBR skipped (see install/pc-x86-64-iso/)" >&2
fi

# Optional: EVE_ISOHYBRID_MBR=/path/to/isohdpfx.bin
if [[ -n "${EVE_ISOHYBRID_MBR:-}" && -f "${EVE_ISOHYBRID_MBR}" ]]; then
  ISOHYBRID_MBR="${EVE_ISOHYBRID_MBR}"
fi

# MBR isohybrid must come after the BIOS El Torito boot options; GPT mark after the EFI image.
ISOHYBRID_PRE=()
ISOHYBRID_POST=()
if [[ -n "$ISOHYBRID_MBR" ]]; then
  ISOHYBRID_PRE+=(-isohybrid-mbr "$ISOHYBRID_MBR" --mbr-force-bootable -partition_offset 16)
fi
ISOHYBRID_POST+=(-isohybrid-gpt-basdat)

if [[ "$HYBRID" == 1 ]]; then
  xorriso -as mkisofs \
    -o "$OUT" \
    -R -J \
    -V 'EVE_OS' \
    -b isolinux/isolinux.bin \
    -c isolinux/boot.cat \
    -no-emul-boot \
    -boot-load-size 4 \
    -boot-info-table \
    ${ISOHYBRID_PRE[@]+"${ISOHYBRID_PRE[@]}"} \
    -eltorito-alt-boot \
    -e efiboot.img \
    -no-emul-boot \
    ${ISOHYBRID_POST[@]+"${ISOHYBRID_POST[@]}"} \
    "$STAGING"
  echo "OK: hybrid ISO (UEFI + BIOS + optional USB isohybrid) → $OUT"
else
  xorriso -as mkisofs \
    -o "$OUT" \
    -R -J \
    -V 'EVE_OS' \
    ${ISOHYBRID_PRE[@]+"${ISOHYBRID_PRE[@]}"} \
    -e efiboot.img \
    -no-emul-boot \
    ${ISOHYBRID_POST[@]+"${ISOHYBRID_POST[@]}"} \
    "$STAGING"
  echo "OK: UEFI-only ISO (El Torito EFI + GPT isohybrid when isohdpfx present) → $OUT"
fi

ls -la "$OUT"
echo "  UEFI: El Torito EFI (PCs, UTM x86_64 + UEFI firmware, most bare metal)"
echo "  BIOS: ISOLINUX + memdisk + eve-bios.img when hybrid build succeeded"
echo "  USB:  'dd' of ISO more likely to work when isohdpfx.bin was used (see note above)"
