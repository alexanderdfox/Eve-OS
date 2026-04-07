#!/usr/bin/env bash
# Build AArch64 UEFI binary and refresh utm/arm-uefi/ for UTM / Asahi / USB.
#
# Layout parity with x86 utm/eve-uefi.img (rust-osdev bootloader GPT):
#   • 6272 × 512-byte disk, ESP partition sectors 34..6177 (6144 sectors ≈ 3 MiB)
#   • Same GPT boot attribute bits via x86-uefi-gpt-boot-flags.sh
#   • FAT volume label EVE_OS (matches hybrid x86 ISO -V)
#
# Artifacts:
#   utm/arm-uefi/eve-arm-uefi-fat.img — raw ESP filesystem only (attach as VirtIO disk in UTM)
#   utm/arm-uefi/eve-arm-uefi.img     — full GPT disk (flash like eve-uefi.img: x86-usb-write --arm-uefi)
#
# The payload is AArch64 UEFI only; the full Eve browser/kernel remains x86_64 (see README on FAT).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

"$ROOT/scripts/arm-uefi-build.sh"

if ! command -v mformat &>/dev/null || ! command -v mcopy &>/dev/null; then
  echo "error: install mtools: brew install mtools" >&2
  exit 1
fi

TGT="$(cargo metadata --format-version 1 --no-deps | python3 -c "import json,sys; print(json.load(sys.stdin)['target_directory'])")"
EFI="$TGT/aarch64-unknown-uefi/release/bootaa64.efi"
OUT="$ROOT/utm/arm-uefi"
mkdir -p "$OUT"

# Identical geometry to utm/eve-uefi.img (see: sgdisk -p utm/eve-uefi.img).
DISK_SECTORS=6272
ESP_FIRST=34
ESP_LAST=6177
ESP_SECTORS=$((ESP_LAST - ESP_FIRST + 1))

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

README="$TMP/README.md"
cat >"$README" <<'EOF'
Eve OS — AArch64 UEFI system partition (same GPT + ESP size as x86 utm/eve-uefi.img).

  EFI/BOOT/BOOTAA64.EFI   Default removable-media boot path
  EFI/EVE/BOOTAA64.EFI    GRUB chainload path (Asahi: scripts/asahi-grub-add-eve.sh)

This program is AArch64 only. Full Eve (browser, VirtIO net, HTTP) is the x86_64
kernel: utm/eve-uefi.img or utm/eve-bios.img with QEMU / UTM emulated PC.

Volume label EVE_OS matches the x86 hybrid ISO volume id.
EOF

ESP_RAW="$TMP/esp.raw"
dd if=/dev/zero of="$ESP_RAW" bs=512 count="$ESP_SECTORS" status=none
# FAT on small ESP (no -F: mformat picks FAT12/16 suitable for size).
mformat -i "$ESP_RAW" -v EVE_OS ::
mmd -i "$ESP_RAW" ::EFI
mmd -i "$ESP_RAW" ::EFI/BOOT
mmd -i "$ESP_RAW" ::EFI/EVE
mcopy -i "$ESP_RAW" -D o "$EFI" ::EFI/BOOT/BOOTAA64.EFI
mcopy -i "$ESP_RAW" -D o "$EFI" ::EFI/EVE/BOOTAA64.EFI
mcopy -i "$ESP_RAW" -D o "$README" ::README.TXT

FAT_IMG="$OUT/eve-arm-uefi-fat.img"
cp -f "$ESP_RAW" "$FAT_IMG"

DISK_IMG="$OUT/eve-arm-uefi.img"
if command -v sgdisk &>/dev/null; then
  dd if=/dev/zero of="$DISK_IMG" bs=512 count="$DISK_SECTORS" status=none
  # -a 1 keeps ESP at 34..6177 like rust-osdev eve-uefi.img (default alignment would move start to 2048).
  sgdisk --clear \
    -a 1 \
    --new=1:"$ESP_FIRST":"$ESP_LAST" \
    --typecode=1:ef00 \
    --change-name=1:boot \
    "$DISK_IMG"
  dd if="$ESP_RAW" of="$DISK_IMG" bs=512 seek="$ESP_FIRST" conv=notrunc status=none
  "$ROOT/scripts/x86-uefi-gpt-boot-flags.sh" "$DISK_IMG"
else
  echo "warning: sgdisk not found — install gptfdisk; skipping eve-arm-uefi.img (FAT only)" >&2
  rm -f "$DISK_IMG"
fi

cp -f "$EFI" "$OUT/bootaa64.efi"
ls -la "$OUT"
echo "OK: $OUT — utm/ARM-UEFI-SETUP.md | Asahi: utm/ASAHI-M1-UEFI-SETUP.md"
echo "    Flash AArch64 UEFI USB: sudo $ROOT/scripts/x86-usb-write.sh --arm-uefi <whole-disk>"
