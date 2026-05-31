Eve — one workflow for every device (macOS + UTM / QEMU)
=========================================================

Use the **same steps** for every variant: prerequisites → build → import paths under
`utm/` → UTM or QEMU → re-sync after code changes. Details vary by CPU; this file is
the checklist; each `utm/*-SETUP.md` is the deep dive for one variant.

0) Prerequisites (once per machine)
   - Rust **nightly**, `components = rust-src, llvm-tools-preview` (see `rust-toolchain.toml`).
   - Targets:

       rustup target add x86_64-unknown-none aarch64-unknown-none aarch64-unknown-uefi

   - **Homebrew:** `brew install qemu` (all QEMU-backed setups). For AArch64 UEFI FAT images also: `brew install mtools`.
   - **UTM** (utm.app) when you use a GUI VM.

1) Build everything (recommended)
   From the Eve repo root:

     ./scripts/build-all-images.sh

   Refreshes, under `utm/`:

     eve-bios.img, eve-uefi.img, eve-*.qcow2 (x86)
     rpi/kernel8-pi3.img, rpi/kernel8-pi4.img, rpi/Eve-Pi3.utm, rpi/Eve-Pi4.utm
     arm-uefi/bootaa64.efi, arm-uefi/eve-arm-uefi-*.img, arm-uefi/eve-arm-uefi-*.qcow2

   List: `utm/BUILT-IMAGES.md`

   Faster / single-target refreshes:

     ./scripts/utm-sync.sh              # x86 BIOS (+ UEFI disk if built)
     ./scripts/rpi-utm-sync.sh          # Pi3 + Pi4 kernels only
     ./scripts/arm-uefi-sync.sh         # AArch64 UEFI bundle only

2) Paths helper (paste into UTM / QEMU without typos)
   From the repo root:

     ./scripts/print-eve-paths.sh

   Exports `EVE_ROOT` and prints absolute paths to every `utm/` image this project ships.

   **Physical x86_64 PC (USB boot, firmware, input/NIC limits):** `install/REAL-HARDWARE.md`

3) Quick reference — same columns for every device
   ---------------------------------------------------------------------------
   | What you run    | UTM mode      | Main artifact              | Full doc |
   ---------------------------------------------------------------------------
   | x86_64 PC BIOS  | Emulate x86_64| utm/eve-bios.qcow2 (or .img) | UTM-SETUP.md |
   | x86_64 PC UEFI  | Emulate x86_64| utm/eve-uefi.qcow2 + OVMF (or .img / eve-x86_64.iso) | UTM-SETUP.md §7, install/pc-x86-64-iso/ |
   | Raspberry Pi 3  | Emulate ARM64 | utm/rpi/Eve-Pi3.utm (or kernel8-pi3.img) | RPI-UTM-SETUP.md |
   | Raspberry Pi 4  | Emulate ARM64 | utm/rpi/Eve-Pi4.utm (or kernel8-pi4.img) | RPI-UTM-SETUP.md |
   | AArch64 UEFI    | Virtualize ARM64 | utm/arm-uefi/*.qcow2 (or *.img) + EDK2 pflash | ARM-UEFI-SETUP.md |
   ---------------------------------------------------------------------------

4) QEMU “extra arguments” (VirtIO net on x86 only)
   For the **x86** Eve kernel, append the contents of:

     utm/qemu-extra.args        (i440FX “PC” templates)
     utm/qemu-extra-q35.args    (Q35 / UEFI — ICH9 HDA + same NIC/USB)

   to UTM’s QEMU extras field (virtio-net + user NAT + USB devices). **RPi** and **AArch64 UEFI**
   use different machines — do not paste the x86 line onto those VMs; follow the
   per-doc QEMU one-liners instead.

5) Display vs serial (aligned where hardware allows)
   - **x86:** Framebuffer UI is the main output in the QEMU window; optional serial.
   - **Raspberry Pi:** Framebuffer in the window when you omit `-display none`;
     kernel **UART** text and **keyboard** go to the **first serial** — in UTM,
     attach a serial console. Prefer **`-serial stdio -monitor none`** so typing
     reaches the guest PL011; **`mon:stdio`** multiplexes the monitor and often
     looks like a dead keyboard until you toggle (**Ctrl-a c**). See RPI-UTM-SETUP.md.
   - **AArch64 UEFI:** `./scripts/arm-uefi-boot-img.sh` defaults to **display only**
     (`-serial null`); add `--serial` if you want UEFI text on the host terminal.

6) After you change code
   Re-run **build-all-images.sh** (or the matching single-target script from §1), then
   restart the VM or replace the imported `utm/` file so the guest loads the new bits.

7) Pointers
   - x86 PC:        utm/UTM-SETUP.md
   - x86 USB boot:  install/pc-x86-64-unified-usb/INSTALL.md (one stick: --iso), install/pc-x86-64-bios-usb/INSTALL.md (BIOS), install/pc-x86-64-uefi-usb/INSTALL.md (UEFI), utm/X86-USB-BOOT.md, ./scripts/x86-usb-write.sh [--bios|--uefi|--iso], ./scripts/sync-x86-bios-img.sh, ./scripts/sync-x86-uefi-img.sh
   - x86 UEFI ISO:  install/pc-x86-64-iso/INSTALL.md, ./scripts/build-x86-iso.sh
   - Raspberry Pi:  utm/RPI-UTM-SETUP.md
   - AArch64 UEFI:  utm/ARM-UEFI-SETUP.md
   - Mac Apple Silicon (which path is “full OS” vs demo): utm/MAC-M1-PRO.md
   - Asahi / M1 Pro native UEFI: install/linux-asahi-m1/ (bundle), utm/ASAHI-M1-UEFI-SETUP.md, ./scripts/asahi-grub-add-eve.sh
   - Image list:    utm/BUILT-IMAGES.md

8) Regression smoke (≈10 min, x86 QEMU)
   From repo root after `./scripts/build-all-images.sh`:

   **Compile check (all targets):**

     ./scripts/verify-repo.sh

   **BIOS disk + VirtIO net + USB HID:**

     cargo run --release -p eve-os

   **UEFI disk (Q35 + OVMF):**

     cargo run --release -p eve-os -- --uefi

   **In-guest checklist (both images):**
   - Epilepsy + age notices dismiss; browser loads default HTTPS Shrine (or offline HTML if net off).
   - **SYS → INTERNET** on; status shows ARP → DNS → TCP → page text.
   - URL bar **GO**, **back/forward** (after visiting a second URL), **R** reload.
   - **SYS → IP MODE → STATIC** — edit STATIC IP / GATEWAY / DNS octets (tap row, type digits).

   See also `utm/NETWORK-BROWSER.md` and `install/REAL-HARDWARE.md` for bare-metal limits.
