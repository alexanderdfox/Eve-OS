Eve OS — one USB stick: UEFI and/or legacy BIOS (Syslinux)
===========================================================

Use this when you want **one** flash drive that can boot **either**:

  • **UEFI** — same EFI payload as **`utm/eve-uefi.img`** (El Torito EFI image inside the ISO).

  • **Legacy BIOS / CSM** — **ISOLINUX** on the ISO runs **Syslinux memdisk**, which loads
    **`eve-bios.img`** as an emulated disk (same MBR layout as **`utm/eve-bios.img`**).

That combined image is **`utm/eve-x86_64.iso`**, built from the same **`eve-bios.img`** and
**`eve-uefi.img`** sources by **`./scripts/build-x86-iso.sh`**. You do **not** copy the three
files separately onto the stick: the ISO **embeds** the BIOS and UEFI boot paths; **`dd`**
the whole ISO to the USB.

1) Build the three artifacts (or at least the ISO inputs)
-----------------------------------------------------------
From the repo root:

  ./scripts/sync-x86-bios-img.sh
  ./scripts/sync-x86-uefi-img.sh
  ./scripts/build-x86-iso.sh

Or **`./scripts/build-all-images.sh`** (runs **`build-x86-iso.sh`** when `xorriso` and
`sgdisk` are installed).

  • **`utm/eve-bios.img`** — used inside the ISO for the BIOS/memdisk path.
  • **`utm/eve-uefi.img`** — ESP is extracted into the ISO for the UEFI path.
  • **`utm/eve-x86_64.iso`** — what you **`dd`** for the unified USB.

**macOS:** full BIOS+hybrid ISO needs Syslinux BIOS files; see **`install/pc-x86-64-iso/INSTALL.md`**
and **`./scripts/download-syslinux-bios.sh`**. Without them, the script may build a **UEFI-only**
ISO (still fine for UEFI PCs and UTM x86_64 + UEFI).

2) Write the USB (whole disk — erases the drive)
------------------------------------------------
Linux / macOS, as **root**:

  sudo ./scripts/x86-usb-write.sh --iso /dev/sdX      # Linux example
  sudo ./scripts/x86-usb-write.sh --iso /dev/disk3    # macOS example

Type **YES** when prompted. On macOS the script prefers **`rdiskN`** for speed.

3) Boot the PC
--------------
  • **UEFI:** firmware boot menu → UEFI USB / removable entry (often shows the stick twice;
    pick the **UEFI** one). **Secure Boot** usually must be **off** (unsigned loader).

  • **BIOS / CSM:** firmware boot menu → legacy / non-UEFI USB entry, or enable CSM and pick
    the stick. You should see **ISOLINUX**, then Eve boots via **memdisk** + **`eve-bios.img`**.

If the stick does not appear after **`dd`**, try **`./scripts/x86-uefi-gpt-boot-flags.sh`** on
**`utm/eve-uefi.img`** and rebuild the ISO, or flash **`--uefi`** / **`--bios`** raw images
instead (**`install/pc-x86-64-uefi-usb/INSTALL.md`**, **`install/pc-x86-64-bios-usb/INSTALL.md`**).

4) When to use raw images instead of `--iso`
--------------------------------------------
  • **`--bios`** or **`--uefi`** (`x86-usb-write.sh`) — single boot mode, slightly simpler
    layout; some firmware is pickier with ISO-hybrid sticks.

  • **`--iso`** — one stick for **both** Syslinux/BIOS and UEFI; good for “try either mode”
    and for **UTM** (attach ISO as CD/DVD — **`install/pc-x86-64-iso/INSTALL.md`**).

See also: **`utm/X86-USB-BOOT.md`**, **`install/REAL-HARDWARE.md`**, **`utm/BUILT-IMAGES.md`**
