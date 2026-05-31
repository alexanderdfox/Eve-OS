Eve OS — real hardware (physical x86_64 PC)
===========================================

This file is the **bare-metal** counterpart to QEMU / UTM docs. Eve is developed
and tested mainly under emulation; on a real PC you should expect **UI and input**
to work when the firmware and chips match what the kernel implements. **Internet
demo (TCP/HTTP)** needs a **supported PCI Ethernet NIC** (see below). The guest
defaults to **DHCP** for IPv4 on real LANs; use **SYS → IP MODE** for QEMU SLIRP
or static addresses.

Quick links
-----------
  • In-QEMU “install to second VirtIO disk” (not bare metal): `install/pc-x86-64-disk-install/INSTALL.md`
  • Wi‑Fi / 802.11 (no MAC driver; PCI probe + SYS VID:DID): `utm/WIFI-80211.md`
  • Flash a USB stick: `utm/X86-USB-BOOT.md`
  • BIOS / MBR image: `install/pc-x86-64-bios-usb/INSTALL.md`
  • UEFI / GPT image: `install/pc-x86-64-uefi-usb/INSTALL.md`
  • Hybrid ISO: `install/pc-x86-64-iso/INSTALL.md`
  • Keyboard / mouse issues (UTM/QEMU, same input stack): `utm/UTM-SETUP.md` section 8
  • Raspberry Pi (different kernel): `rpi/RPI-IMAGES.md`, `rpi/PI3-PI4-GUI-NETWORK.md`

1) Build and write the correct image
-------------------------------------
From the repo root:

  ./scripts/build-all-images.sh

Pick the image that matches **how the PC boots**:

  • **Legacy BIOS / CSM** → `utm/eve-bios.img` → `sudo ./scripts/x86-usb-write.sh --bios /dev/…`
  • **Native UEFI** (typical 2015+ PCs) → `utm/eve-uefi.img` → `sudo ./scripts/x86-usb-write.sh --uefi /dev/…`

Use the **whole disk** (e.g. `/dev/sdb`, `/dev/disk3`), not a partition. Confirm
the device twice — `dd` erases the stick.

2) Firmware checklist
---------------------
  • **Secure Boot:** Eve is not Microsoft-signed. Disable Secure Boot or use a
    firmware menu that allows **unsigned** / **other OS** EFI loaders.
  • **UEFI vs legacy:** If the stick was written as **UEFI**, enable UEFI boot and
    pick the **UEFI: USB** entry. If the machine is BIOS-only, use **eve-bios.img**
    and enable **CSM / legacy USB boot** where applicable.
  • **USB boot:** Some laptops hide external boot until “USB boot” or “external
    device boot” is enabled in setup.
  • **GPT ESP flags (UEFI):** If the USB does not appear, run
    `./scripts/x86-uefi-gpt-boot-flags.sh utm/eve-uefi.img` (needs `sgdisk`) and
    re-flash. See `install/pc-x86-64-uefi-usb/INSTALL.md`.
  • **Bootloader framebuffer:** Disk images embed `boot.json` requesting at least
    **640×480** so more firmware still supplies GOP/VESA; bootloader logs to **serial**
    when the platform exposes it.

3) What should work on a real PC (x86_64 guest)
-----------------------------------------------
  • **Framebuffer:** The rust-osdev **bootloader** sets a GOP (UEFI) or VESA-style
    mode (BIOS path) when the firmware cooperates. A short blank screen at boot is
    normal. If **no** framebuffer appears, the kernel prints a short message on
    **COM1** (**115200 8N1**, port **0x3F8**) when that UART exists — useful with a
    USB–serial adapter on tower boards or `-serial stdio` in QEMU.
  • **Keyboard / mouse:**
      – **PS/2** (i8042) when the controller exists and firmware enables it.
      – **USB HID boot** via **PCI UHCI** (I/O) or **OHCI** (MMIO) when present, or via
        **xHCI companion** routing (SYS **USB HOST** shows `XHCI+OHCI` / `XHCI+UHCI`).
      – **xHCI-only** (no companion): **`xhci_native`** (Phase 2) enumerates boot keyboard + one boot
        mouse on root ports; SYS shows **`XHCI NAT`** when the keyboard path succeeds. **EHCI**
        split-transaction HID is **not implemented** — LOG may show `ehci: companion present; hid N/I`
        when firmware routes through EHCI companions. Many laptops are **xHCI-only**; built-in
        keyboard/trackpad may still fail until native xHCI is validated on that machine or firmware
        exposes PS/2.
      – **Working transfers, not just enumeration:** PS/2 **keyboard** stays in the mix until
        USB boot keyboard interrupt IN succeeds; after many failed INs the kernel uses PS/2 again.
        PS/2 **mouse** stays on the **primary** pointer until a USB boot mouse delivers good
        reports, so a “ghost” enumerated USB mouse does not strand the trackpad on a secondary
        cursor.
      – Try an **external USB keyboard** on a **USB 2.0 hub** or rear ports that
        some boards route through a companion UHCI — still not guaranteed.
      – **USB poll** defaults **on**. Turn it **off** for PS/2-only fallback if
        UHCI/OHCI HID input stalls on your host.

4) Networking on bare metal
---------------------------
  • **Packet drivers** in-tree include **VirtIO net**, **Realtek RTL8139** (**10EC:8139**),
    **RTL8168/8169**-class, **Intel e1000 / e1000e-class** PCI IDs, and **AMD PCnet**
    — see `kernel/src/nic.rs` / `install/` PCI lists. Unsupported NICs show **NET: NO-DRV**
    while user-disabled networking shows **NET: DISABLED**.
  • **Addressing:** Boot default is **DHCP** (DISCOVER on the cable). For QEMU user NAT,
    set **SYS → IP MODE → SLIRP** (**10.0.2.15** / **.2** / **.3**). **Static** defaults
    suit a typical LAN (**192.168.1.100** / **.1** / **8.8.8.8**).
  • **Wi‑Fi** PCI devices may appear in SYS for VID:DID only — there is **no 802.11 MAC**
    driver; use Ethernet or another machine for the browser demo.

  Using the **browser on bare metal** still needs a **matched PCI Ethernet driver** and
    a working **DHCP server** (or correct static/SLIRP mode).

5) Audio, Bluetooth, Wi‑Fi toggles
----------------------------------
  Settings rows for **Bluetooth**, **Wi‑Fi SSID/PSK**, and **HDA** are mostly
  **placeholders** — no full stack behind them on real hardware.

6) Raspberry Pi and other ARM boards
------------------------------------
  **kernel-rpi** is a separate AArch64 image but now runs the shared `arm_run` Eve UI
  loop (browser/settings/log) with serial keyboard/mouse input and framebuffer output.
  See `rpi/` and `utm/RPI-UTM-SETUP.md`.

7) If something fails
----------------------
  • **Black screen forever:** try the **other** boot style (UEFI vs BIOS image),
    a different monitor, or **CSM on/off**. Some GPUs/firmware need the UEFI path.
    Capture **COM1 115200** (see §3) to see whether the kernel is running without GOP.
  • **No keyboard:** PS/2 keyboard if the board has a port; USB poll on/off in
    Settings. On **xHCI-only** PCs, check LOG for `xhci nat: kbd ok` / SYS **USB HOST**
    `XHCI NAT`; try an external USB keyboard on rear ports. Built-in laptop input may
    still fail until validated on that hardware.
  • **No network:** expected on metal until more PCI NIC drivers and configurable
    addressing exist.

After kernel changes, rebuild and re-flash:

  ./scripts/build-all-images.sh
  sudo ./scripts/x86-usb-write.sh --uefi /dev/…   # or --bios
