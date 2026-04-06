#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT"

if [[ ! -x "./scripts/arm-uefi-sync.sh" ]]; then
  echo "error: missing ./scripts/arm-uefi-sync.sh" >&2
  exit 1
fi
if [[ ! -x "./scripts/asahi-grub-add-eve.sh" ]]; then
  echo "error: missing ./scripts/asahi-grub-add-eve.sh" >&2
  exit 1
fi

./scripts/arm-uefi-sync.sh
sudo ./scripts/asahi-grub-add-eve.sh

echo "OK: Eve AArch64 UEFI synced and GRUB entry installed."
