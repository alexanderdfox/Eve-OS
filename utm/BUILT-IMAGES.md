Images produced by:  ./scripts/build-all-images.sh
==================================================

Compile smoke test (correct per-target checks; do not rely on `cargo check --workspace`):
  ./scripts/verify-repo.sh

Archive older img/iso trees with version labels:  ./scripts/archive-utm-release.sh
  → utm/archive/<label>/  (layout: utm/archive/README.md)

Unified setup (build → utm/ paths → UTM):  utm/SETUP-ALL-DEVICES.md
Absolute paths helper:                    ./scripts/print-eve-paths.sh

x86_64 (QEMU / UTM emulated PC)
  utm/eve-bios.img       — BIOS boot disk (rust-osdev bootloader + kernel)
  utm/eve-bios.qcow2     — same, qcow2 for UTM import (./scripts/utm-mkqcow2.sh)
  utm/eve-uefi.img       — UEFI boot disk (same kernel; use with OVMF in QEMU)
  utm/eve-uefi.qcow2     — same, qcow2 for UTM import
  utm/eve-x86_64.iso — hybrid UEFI+BIOS ISO (PC + UTM x86_64; xorriso + Syslinux memdisk;
                        optional isohdpfx USB layout — install/pc-x86-64-iso/)
  utm/eve-i686.iso    — 32-bit i686 Multiboot (ISOLINUX + mboot.c32; `make iso-i686` / `make i686-media`)
  utm/eve-i686.img    — same kernel on FAT superfloppy (needs `syslinux --install`; often from Linux)
  utm/eve-i686.qcow2  — qcow2 of eve-i686.img when present
  utm/eve-install-target.qcow2 — empty VirtIO install target (from eve-install-target.raw)
  USB thumb drive:        ./scripts/x86-usb-write.sh [--bios|--uefi|--iso] — utm/X86-USB-BOOT.md
  Unified USB (UEFI+BIOS): install/pc-x86-64-unified-usb/INSTALL.md  (--iso → eve-x86_64.iso)
  BIOS image refresh:     ./scripts/sync-x86-bios-img.sh → utm/eve-bios.img
  UEFI image refresh:     ./scripts/sync-x86-uefi-img.sh → utm/eve-uefi.img (+ GPT flags if sgdisk)
  GPT ESP boot bits:      ./scripts/x86-uefi-gpt-boot-flags.sh utm/eve-uefi.img
  PC BIOS USB guide:      install/pc-x86-64-bios-usb/INSTALL.md
  PC UEFI USB guide:      install/pc-x86-64-uefi-usb/INSTALL.md

Raspberry Pi AArch64 (QEMU raspi3b/raspi4b or SD boot)
  utm/rpi/kernel8-pi3.img — BCM2837 profile
  utm/rpi/kernel8-pi4.img — BCM2711 profile
  utm/rpi/Eve-Pi3.utm     — ready-made UTM VM (raspi3b; ./scripts/rpi-utm-mkbundle.sh)
  utm/rpi/Eve-Pi4.utm     — ready-made UTM VM (raspi4b)
  Refresh bundles:         make utm-rpi  (or ./scripts/rpi-utm-sync.sh)

AArch64 UEFI (QEMU virt + EDK2; Apple Silicon HVF; Asahi Linux U-Boot on M1)
  utm/arm-uefi/bootaa64.efi
  utm/arm-uefi/eve-arm-uefi-fat.img — ESP-sized FAT (same bytes as x86 ESP: 3 MiB); mtools + script
  utm/arm-uefi/eve-arm-uefi-fat.qcow2 — qcow2 for UTM VirtIO disk import
  utm/arm-uefi/eve-arm-uefi.img     — full GPT disk, same sector layout + size as utm/eve-uefi.img
  utm/arm-uefi/eve-arm-uefi.qcow2   — qcow2 for UTM / USB-style full disk
  QCOW2 refresh (all disk targets):  ./scripts/utm-mkqcow2.sh [--force]
  USB (AArch64 UEFI hardware):      ./scripts/x86-usb-write.sh --arm-uefi <whole-disk>
  Native Mac (Asahi): utm/ASAHI-M1-UEFI-SETUP.md

Intermediate copies also exist under target/ and rpi/dist/ as built by Cargo/scripts.

Feature matrix (input · net · persist · storage)
------------------------------------------------

| Artifact | Input | Network | Settings persist | Storage |
|----------|-------|---------|------------------|---------|
| `utm/eve-bios.img` / `eve-uefi.img` | PS/2 + UHCI/OHCI USB HID; xHCI companion fallback only | VirtIO / RTL8139 / RTL8168 / e1000 / PCnet; HTTP/HTTPS + chunked bodies | RAM-only (x86); edit in SYS | VirtIO-blk install (2 disks in QEMU) |
| `utm/eve-x86_64.iso` | same as disk images | same | same | same |
| `utm/eve-i686.iso` | same (i686 kernel) | same | same | same |
| `utm/rpi/kernel8-pi*.img` | PL011 serial keyboard + SGR mouse | VirtIO-MMIO when firmware allows scan | RAM-only | none |
| `utm/arm-uefi/bootaa64.efi` | UEFI Simple/Absolute pointer + ConIn keyboard | VirtIO-MMIO when VM heuristics / NVRAM allow | UEFI NVRAM (SAVE SETTINGS) | none |
