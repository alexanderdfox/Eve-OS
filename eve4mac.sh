#!/usr/bin/env bash
set -euo pipefail

# ===============================
# eve4mac.sh - Universal ARM64 UEFI + GRUB Setup
# Works on Debian/Ubuntu/Asahi/Fedora for Apple Silicon Macs
# ===============================

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT"

# Required scripts
SYNC_SCRIPT="./scripts/arm-uefi-sync.sh"
GRUB_SCRIPT="./scripts/asahi-grub-add-eve.sh"

# Check that scripts exist and are executable
for script in "$SYNC_SCRIPT" "$GRUB_SCRIPT"; do
    if [[ ! -x "$script" ]]; then
        echo "error: missing or not executable $script" >&2
        exit 1
    fi
done

# ===============================
# Step 1: Sync Eve AArch64 UEFI
# ===============================
echo "Syncing Eve AArch64 UEFI..."
"$SYNC_SCRIPT"

EFI_OUT="$ROOT/utm/arm-uefi/bootaa64.efi"
[[ -f "$EFI_OUT" ]] || {
    echo "error: expected $EFI_OUT after sync" >&2
    exit 1
}

# ===============================
# Step 2: Add Eve entry to GRUB
# ===============================
# asahi-grub-add-eve.sh expects a path to bootaa64.efi (or uses repo default).
echo "Adding Eve entry to GRUB..."
sudo "$GRUB_SCRIPT" "$EFI_OUT"

echo "OK: Eve AArch64 UEFI synced and GRUB entry installed."
