Eve — bootable x86_64 USB thumb drive
=====================================

**Physical PC (expectations, NIC/input limits):** `install/REAL-HARDWARE.md`

**One USB = UEFI + legacy BIOS (Syslinux/memdisk + embedded eve-bios.img):**
`install/pc-x86-64-unified-usb/INSTALL.md` — build **`utm/eve-x86_64.iso`** then
**`sudo ./scripts/x86-usb-write.sh --iso /dev/…`**

**Hybrid ISO (UEFI + BIOS DVD/VM):** `install/pc-x86-64-iso/INSTALL.md` — `./scripts/build-x86-iso.sh`  
**PC + BIOS / legacy quick guide:** `install/pc-x86-64-bios-usb/INSTALL.md`  
**PC + UEFI quick guide:** `install/pc-x86-64-uefi-usb/INSTALL.md`  
**Refresh BIOS image only:** `./scripts/sync-x86-bios-img.sh`  
**Refresh UEFI image only:** `./scripts/sync-x86-uefi-img.sh`  
**GPT ESP boot attributes (pickier PCs):** `./scripts/x86-uefi-gpt-boot-flags.sh` (needs `sgdisk` / gptfdisk)

You flash a **raw disk image** onto a USB stick (whole device). Pick the image
that matches how the PC boots:

  • **BIOS / CSM (legacy)** → `utm/eve-bios.img`  (MBR, rust-osdev bootloader)
  • **UEFI only** (most current PCs, Secure Boot usually off) → `utm/eve-uefi.img`
    (GPT + ESP, same kernel as QEMU OVMF)

Build the images first:

  ./scripts/build-all-images.sh

  or refresh one target (release → `utm/`):

     ./scripts/sync-x86-bios-img.sh    # utm/eve-bios.img
     ./scripts/sync-x86-uefi-img.sh    # utm/eve-uefi.img

  Quick debug copy (not release): `./scripts/utm-sync.sh`

Write the stick (run as **root**; replaces everything on that disk):

  sudo ./scripts/x86-usb-write.sh --iso /dev/sdX      # unified: UEFI + ISOLINUX/BIOS (eve-x86_64.iso)
  sudo ./scripts/x86-usb-write.sh --bios /dev/sdX     # Linux: whole disk, e.g. sdb not sdb1
  sudo ./scripts/x86-usb-write.sh --uefi /dev/sdX

  macOS (whole disk, e.g. disk3 — check Disk Utility / diskutil list):

  sudo ./scripts/x86-usb-write.sh --iso /dev/disk3
  sudo ./scripts/x86-usb-write.sh --bios /dev/disk3
  sudo ./scripts/x86-usb-write.sh --uefi /dev/disk3

The script uses **`/dev/rdiskN` on macOS** when present (faster `dd`).

Safety: you must type **YES** to confirm. Use the correct device; there is no
undo.

After `dd`, eject the USB, plug it into the target PC, power on, open the
**boot menu** (vendor key: F12, F10, Esc, etc.) and choose the USB drive.

Notes:

  • Screen may be blank until the kernel brings up the framebuffer; PS/2 or USB
    input depends on firmware and Eve’s drivers (VirtIO net is for QEMU-style
    configs; bare metal has no virtio-net unless you add drivers).
  • **UEFI + Secure Boot:** our image is not Microsoft-signed; disable Secure
    Boot or enroll custom keys if your firmware allows.
  • For **QEMU/UTM** on a Mac, you do not need a USB; use `utm/eve-bios.img` as a
    virtual disk (see utm/UTM-SETUP.md).

See also: utm/BUILT-IMAGES.md, ./scripts/print-eve-paths.sh
