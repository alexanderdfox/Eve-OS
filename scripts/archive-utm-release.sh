#!/usr/bin/env bash
# Copy current utm/ ship images into utm/archive/<label>/ with a MANIFEST.
# Usage: ./scripts/archive-utm-release.sh
# Env:
#   EVE_ARCHIVE_LABEL   — directory name under utm/archive/ (default: v<semver>[+githash])
#   EVE_ARCHIVE_APPEND_GIT — if 1 (default), append +<short-hash> when in a git repo
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

append_git="${EVE_ARCHIVE_APPEND_GIT:-1}"
version_line="$(grep -E '^version\s*=' "$ROOT/Cargo.toml" | head -1 || true)"
semver="$(echo "$version_line" | sed -n 's/^version[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p')"
if [[ -z "$semver" ]]; then
  echo "error: could not read version from $ROOT/Cargo.toml" >&2
  exit 1
fi

if [[ -n "${EVE_ARCHIVE_LABEL:-}" ]]; then
  label="$EVE_ARCHIVE_LABEL"
else
  label="v${semver}"
  if [[ "$append_git" == "1" ]] && git -C "$ROOT" rev-parse --is-inside-work-tree &>/dev/null; then
    short="$(git -C "$ROOT" rev-parse --short HEAD 2>/dev/null || true)"
    if [[ -n "$short" ]]; then
      label="${label}+${short}"
    fi
  fi
fi

# Sanitize directory name (no slashes)
label="${label//\//_}"
DEST="$ROOT/utm/archive/$label"
mkdir -p "$DEST/rpi" "$DEST/arm-uefi"

copied=0
copy_if() {
  local src="$1" rel="$2"
  local dir
  dir="$(dirname "$rel")"
  if [[ "$dir" != "." ]]; then
    mkdir -p "$DEST/$dir"
  fi
  if [[ -f "$src" ]]; then
    cp -f "$src" "$DEST/$rel"
    echo "  archived $rel"
    copied=$((copied + 1))
  fi
}

echo "Archiving to $DEST"
copy_if "$ROOT/utm/eve-bios.img" "eve-bios.img"
copy_if "$ROOT/utm/eve-uefi.img" "eve-uefi.img"
copy_if "$ROOT/utm/eve-x86_64.iso" "eve-x86_64.iso"
copy_if "$ROOT/utm/eve-x86_64-uefi.iso" "eve-x86_64-uefi.iso"
copy_if "$ROOT/utm/rpi/kernel8-pi3.img" "rpi/kernel8-pi3.img"
copy_if "$ROOT/utm/rpi/kernel8-pi4.img" "rpi/kernel8-pi4.img"
copy_if "$ROOT/utm/arm-uefi/bootaa64.efi" "arm-uefi/bootaa64.efi"
copy_if "$ROOT/utm/arm-uefi/eve-arm-uefi-fat.img" "arm-uefi/eve-arm-uefi-fat.img"

if [[ "$copied" -eq 0 ]]; then
  echo "warning: no utm images found to copy (build/sync first)." >&2
fi

{
  echo "eve-os Cargo.toml version: $semver"
  echo "archive directory label:    $label"
  echo "archived at (UTC):         $(date -u '+%Y-%m-%dT%H:%M:%SZ')"
  if git -C "$ROOT" rev-parse HEAD &>/dev/null; then
    echo "git HEAD:                  $(git -C "$ROOT" rev-parse HEAD)"
    echo "git describe:              $(git -C "$ROOT" describe --always --dirty 2>/dev/null || echo '?')"
  fi
  echo ""
  echo "files:"
  find "$DEST" -type f ! -name MANIFEST.txt | sed "s|^$DEST/||" | sort
} > "$DEST/MANIFEST.txt"

echo "OK: $DEST (see MANIFEST.txt)"
ls -la "$DEST"
