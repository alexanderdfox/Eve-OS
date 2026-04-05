#!/usr/bin/env bash
# Print newest eve-os build.rs disk path for a given Cargo profile (release|debug).
# Usage: eve-disk-out.sh <cargo_target_dir> <release|debug> <bios.img|uefi.img>
set -euo pipefail
TGT="${1:?target dir}"
PROFILE="${2:?release or debug}"
NAME="${3:?bios.img or uefi.img}"
DIR="$TGT/$PROFILE/build"
if [[ ! -d "$DIR" ]]; then
  echo ""
  exit 0
fi
files=()
while IFS= read -r -d '' f; do
  files+=("$f")
done < <(find "$DIR" -path "*/out/$NAME" -type f -print0 2>/dev/null)
if ((${#files[@]} == 0)); then
  echo ""
  exit 0
fi
newest=""
newest_m=0
for f in "${files[@]}"; do
  if [[ "$(uname -s)" == Darwin ]]; then
    m=$(stat -f %m "$f" 2>/dev/null || echo 0)
  else
    m=$(stat -c %Y "$f" 2>/dev/null || echo 0)
  fi
  if [[ "$m" -gt "$newest_m" ]]; then
    newest_m=$m
    newest=$f
  fi
done
echo "$newest"
