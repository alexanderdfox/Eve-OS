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
  utm/eve-uefi.img       — UEFI boot disk (same kernel; use with OVMF in QEMU)
  utm/eve-x86_64.iso — hybrid UEFI+BIOS ISO (PC + UTM x86_64; xorriso + Syslinux memdisk;
                        optional isohdpfx USB layout — install/pc-x86-64-iso/)
  utm/eve-i686.iso    — 32-bit i686 Multiboot (ISOLINUX + mboot.c32; `make iso-i686` / `make i686-media`)
  utm/eve-i686.img    — same kernel on FAT superfloppy (needs `syslinux --install`; often from Linux)
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

AArch64 UEFI (QEMU virt + EDK2; Apple Silicon HVF; Asahi Linux U-Boot on M1)
  utm/arm-uefi/bootaa64.efi
  utm/arm-uefi/eve-arm-uefi-fat.img — ESP-sized FAT (same bytes as x86 ESP: 3 MiB); mtools + script
  utm/arm-uefi/eve-arm-uefi.img     — full GPT disk, same sector layout + size as utm/eve-uefi.img
  USB (AArch64 UEFI hardware):      ./scripts/x86-usb-write.sh --arm-uefi <whole-disk>
  Native Mac (Asahi): utm/ASAHI-M1-UEFI-SETUP.md

Intermediate copies also exist under target/ and rpi/dist/ as built by Cargo/scripts.
