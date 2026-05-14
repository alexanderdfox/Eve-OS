#!/usr/bin/env bash
# Build the i686 Multiboot `kernel-i686` binary and wrap it in bootable **ISO** and/or **raw IMG**
# (FAT superfloppy + Syslinux → mboot.c32 → kernel). BIOS / legacy CSM and QEMU `-cdrom` / `-hda`.
#
# Requires: nightly Rust, xorriso, mtools (mformat, mcopy, mmd). Syslinux files: distro packages or
#   ./scripts/download-syslinux-bios.sh (adds mboot.c32 + mbr.bin). Raw IMG also needs the `syslinux`
#   installer in PATH (common on Linux; often missing on macOS — ISO still builds).
#
# Usage: ./scripts/build-i686-media.sh [both|iso|img] [release|debug]
# Env:   EVE_I686_DEBUG=1  → force debug kernel profile
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$ROOT"

MODE="${1:-both}"
PROFILE="${2:-release}"
if [[ "${EVE_I686_DEBUG:-}" == 1 ]]; then
  PROFILE="debug"
fi

die() { echo "error: $*" >&2; exit 1; }

case "$MODE" in both|iso|img) ;; *)
  die "usage: $0 [both|iso|img] [release|debug]"
  ;;
esac

case "$PROFILE" in release|debug) ;; *)
  die "second arg must be release or debug"
  ;;
esac

if [[ -f "$HOME/.cargo/env" ]]; then
  # shellcheck source=/dev/null
  source "$HOME/.cargo/env"
fi

if ! command -v rustup &>/dev/null || ! rustup toolchain list 2>/dev/null | grep -q '^nightly'; then
  die "install nightly: rustup toolchain install nightly"
fi

if [[ "$PROFILE" == "release" ]]; then
  "$ROOT/scripts/build-kernel-i686.sh" release
else
  "$ROOT/scripts/build-kernel-i686.sh"
fi

TGT="$(cargo metadata --format-version 1 --no-deps | python3 -c "import json,sys; print(json.load(sys.stdin)['target_directory'])")"
KERN="$TGT/i686-unknown-none/$PROFILE/kernel-i686"
[[ -f "$KERN" ]] || die "missing $KERN (kernel-i686 build failed?)"

ISO_OUT="${EVE_I686_ISO:-$ROOT/utm/eve-i686.iso}"
IMG_OUT="${EVE_I686_IMG:-$ROOT/utm/eve-i686.img}"

find_isolinux() {
  local f pfx
  for f in \
    "$ROOT/tools/syslinux-bios/isolinux.bin" \
    /usr/lib/ISOLINUX/isolinux.bin \
    /usr/share/syslinux/isolinux.bin \
    /usr/lib/syslinux/isolinux.bin \
    /usr/lib/syslinux/bios/isolinux.bin; do
    [[ -f "$f" ]] && { echo "$f"; return 0; }
  done
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

find_ldlinux_c32() {
  local f pfx
  for f in \
    "$ROOT/tools/syslinux-bios/ldlinux.c32" \
    /usr/lib/syslinux/modules/bios/ldlinux.c32 \
    /usr/lib/syslinux/bios/ldlinux.c32 \
    /usr/share/syslinux/ldlinux.c32; do
    [[ -f "$f" ]] && { echo "$f"; return 0; }
  done
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
  local f pfx
  for f in \
    "$ROOT/tools/syslinux-bios/libcom32.c32" \
    /usr/lib/syslinux/modules/bios/libcom32.c32 \
    /usr/lib/syslinux/bios/libcom32.c32 \
    /usr/share/syslinux/libcom32.c32; do
    [[ -f "$f" ]] && { echo "$f"; return 0; }
  done
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
  local f pfx
  for f in \
    "$ROOT/tools/syslinux-bios/libutil.c32" \
    /usr/lib/syslinux/modules/bios/libutil.c32 \
    /usr/lib/syslinux/bios/libutil.c32 \
    /usr/share/syslinux/libutil.c32; do
    [[ -f "$f" ]] && { echo "$f"; return 0; }
  done
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

find_mboot_c32() {
  local f pfx
  for f in \
    "$ROOT/tools/syslinux-bios/mboot.c32" \
    /usr/lib/syslinux/modules/bios/mboot.c32 \
    /usr/lib/syslinux/bios/mboot.c32 \
    /usr/share/syslinux/mboot.c32; do
    [[ -f "$f" ]] && { echo "$f"; return 0; }
  done
  pfx="$(brew --prefix syslinux 2>/dev/null)" || true
  if [[ -n "$pfx" ]]; then
    for f in \
      "$pfx/lib/syslinux/modules/bios/mboot.c32" \
      "$pfx/lib/syslinux/bios/mboot.c32"; do
      [[ -f "$f" ]] && { echo "$f"; return 0; }
    done
  fi
  return 1
}

find_isohdpfx() {
  local f pfx
  for f in \
    "$ROOT/tools/syslinux-bios/isohdpfx.bin" \
    /usr/lib/syslinux/bios/isohdpfx.bin \
    /usr/lib/ISOLINUX/isohdpfx.bin \
    /usr/share/syslinux/isohdpfx.bin \
    /usr/lib/syslinux/isohdpfx.bin; do
    [[ -f "$f" ]] && { echo "$f"; return 0; }
  done
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

stage_syslinux_dir() {
  local dest="$1"
  mkdir -p "$dest/isolinux"
  cp -f "$(find_isolinux)" "$dest/isolinux/isolinux.bin"
  cp -f "$(find_ldlinux_c32)" "$dest/isolinux/ldlinux.c32"
  cp -f "$(find_libcom32_c32)" "$dest/isolinux/libcom32.c32"
  if tmpu="$(find_libutil_c32)"; then
    cp -f "$tmpu" "$dest/isolinux/libutil.c32"
  fi
  cp -f "$(find_mboot_c32)" "$dest/isolinux/mboot.c32"
  cp -f "$KERN" "$dest/isolinux/kernel-i686"
  cat >"$dest/isolinux/isolinux.cfg" <<'CFG'
DEFAULT eve
PROMPT 1
TIMEOUT 50
LABEL eve
  SAY Booting Eve OS i686 (Multiboot)…
  KERNEL mboot.c32
  APPEND /isolinux/kernel-i686
CFG
}

check_iso_prereqs() {
  command -v xorriso &>/dev/null || die "install xorriso (e.g. brew install xorriso)"
  ISOLINUX_BIN="$(find_isolinux)" || die "isolinux.bin not found — apt install isolinux / brew install syslinux, or ./scripts/download-syslinux-bios.sh"
  LDLINUX_C32="$(find_ldlinux_c32)" || die "ldlinux.c32 not found (syslinux-common / download-syslinux-bios.sh)"
  LIBCOM32="$(find_libcom32_c32)" || die "libcom32.c32 not found (syslinux-common / download-syslinux-bios.sh)"
  find_mboot_c32 &>/dev/null || die "mboot.c32 not found — run ./scripts/download-syslinux-bios.sh (or install syslinux-common)"
}

img_prereqs_ok() {
  command -v mformat &>/dev/null && command -v mcopy &>/dev/null || return 1
  command -v syslinux &>/dev/null || return 1
  find_mboot_c32 &>/dev/null || return 1
  find_ldlinux_c32 &>/dev/null || return 1
  find_libcom32_c32 &>/dev/null || return 1
}

build_iso() {
  check_iso_prereqs
  local TMP STAGING
  TMP=$(mktemp -d)
  trap 'rm -rf "$TMP"' RETURN
  STAGING="$TMP/iso"
  stage_syslinux_dir "$STAGING"
  mkdir -p "$(dirname "$ISO_OUT")"
  echo "Eve OS i686 — Multiboot demo (see Makefile: iso-i686)" >"$STAGING/README.txt"

  ISOHYBRID_MBR=""
  if tmp="$(find_isohdpfx)"; then
    ISOHYBRID_MBR="$tmp"
  else
    echo "note: isohdpfx.bin not found — ISO still boots as CD/DVD; USB 'dd' hybrid MBR skipped" >&2
  fi

  HY_ARGS=()
  if [[ -n "$ISOHYBRID_MBR" ]]; then
    HY_ARGS+=(-isohybrid-mbr "$ISOHYBRID_MBR" --mbr-force-bootable -partition_offset 16 -isohybrid-gpt-basdat)
  fi

  xorriso -as mkisofs \
    -o "$ISO_OUT" \
    -R -J \
    -V 'EVE_I686' \
    -b isolinux/isolinux.bin \
    -c isolinux/boot.cat \
    -no-emul-boot \
    -boot-load-size 4 \
    -boot-info-table \
    "${HY_ARGS[@]+"${HY_ARGS[@]}"}" \
    "$STAGING"
  ls -la "$ISO_OUT"
  echo "OK: i686 ISO → $ISO_OUT"
  echo "  QEMU: qemu-system-i386 -cdrom \"$ISO_OUT\" -serial stdio -monitor none"
}

build_img() {
  local strict="${1:-1}"
  if ! img_prereqs_ok; then
    if [[ "$strict" == 1 ]]; then
      die "IMG prerequisites failed (mtools + syslinux + mboot.c32/ldlinux.c32/libcom32.c32 — see script header)"
    fi
    return 1
  fi
  local TMPD STAGING
  TMPD="$(mktemp -d)"
  trap 'rm -rf "$TMPD"' RETURN
  STAGING="$TMPD/fatroot"
  mkdir -p "$STAGING/isolinux"
  cp -f "$(find_ldlinux_c32)" "$STAGING/isolinux/ldlinux.c32"
  cp -f "$(find_libcom32_c32)" "$STAGING/isolinux/libcom32.c32"
  if tmpu="$(find_libutil_c32)"; then
    cp -f "$tmpu" "$STAGING/isolinux/libutil.c32"
  fi
  cp -f "$(find_mboot_c32)" "$STAGING/isolinux/mboot.c32"
  cp -f "$KERN" "$STAGING/isolinux/kernel-i686"
  cat >"$STAGING/isolinux/syslinux.cfg" <<'CFG'
DEFAULT eve
PROMPT 1
TIMEOUT 50
LABEL eve
  SAY Booting Eve OS i686 (Multiboot)…
  KERNEL mboot.c32
  APPEND /isolinux/kernel-i686
CFG

  mkdir -p "$(dirname "$IMG_OUT")"
  rm -f "$IMG_OUT"
  # 32 MiB FAT superfloppy: 64 cylinders × 32 heads × 32 sectors × 512 B
  dd if=/dev/zero of="$IMG_OUT" bs=1M count=32 status=none
  mformat -i "$IMG_OUT" -c 64 -h 32 -s 32 -v EVE686 ::
  mmd -i "$IMG_OUT" ::/isolinux 2>/dev/null || true
  mcopy -i "$IMG_OUT" "$STAGING/isolinux/"* ::/isolinux/

  if ! syslinux --directory /isolinux --install "$IMG_OUT"; then
    rm -f "$IMG_OUT"
    if [[ "$strict" == 1 ]]; then
      die "syslinux --install failed (common on macOS without a working syslinux for disk images)"
    fi
    return 1
  fi

  ls -la "$IMG_OUT"
  echo "OK: i686 raw IMG (FAT superfloppy) → $IMG_OUT"
  echo "  QEMU: qemu-system-i386 -drive file=\"$IMG_OUT\",format=raw -serial stdio -monitor none"
}

mkdir -p "$ROOT/utm"

case "$MODE" in
  iso) build_iso ;;
  img) build_img 1 ;;
  both)
    build_iso
    if ! build_img 0; then
      echo "warning: i686 raw IMG not built (need mtools + syslinux; ISO is ready — see scripts/build-i686-media.sh)" >&2
    fi
    ;;
esac

mkdir -p "$ROOT/build"
[[ -f "$ISO_OUT" ]] && cp -f "$ISO_OUT" "$ROOT/build/eve-i686.iso" 2>/dev/null || true
[[ -f "$IMG_OUT" ]] && cp -f "$IMG_OUT" "$ROOT/build/eve-i686.img" 2>/dev/null || true
