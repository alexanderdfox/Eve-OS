Eve on Raspberry Pi 3 and 4 — GUI and network (honest matrix)
=============================================================

This document matches what the **kernel-rpi** binary actually does today.

GUI (both Pi 3–class and Pi 4–class builds)
-------------------------------------------
  • The AArch64 kernel requests a **32 bpp framebuffer** from the VideoCore
    mailbox (same path on real firmware and on QEMU `raspi3b` / `raspi4b` when
    the mailbox is emulated).
  • After a full-screen gradient splash, it runs the **shared `arm_run` UI**
    (browser, SYS, VirtIO-MMIO net on AArch64) over the framebuffer.
  • **Keyboard** comes from **PL011 UART0** (ANSI / CSI sequences on the serial
    console). **Pointer** uses **xterm SGR mouse** on the same serial stream when
    the host terminal supports it; the QEMU **display window** does not feed the
    guest mouse. Use **Tab / arrow keys** when serial mouse is unavailable.

Network (Pi 3 vs Pi 4 hardware)
-------------------------------
  • **`kernel-rpi` today:** networking in the shared UI is **VirtIO-MMIO** when
    the firmware enables MMIO scan (see `nic::aarch64` / UEFI NVRAM). The **QEMU
    `usb-net` device** from `run-raspi-qemu.sh` is **not** consumed yet — there is
    no USB Ethernet driver in this kernel. The script still adds it so your QEMU
    command matches common tutorials and is ready if a driver appears later.
  • **Real Pi hardware:** **Pi 3 / 3B+ / Zero 2 W:** onboard Ethernet is via the
    USB controller; many setups use a USB Ethernet adapter. Bringing either up
    requires **USB stack + appropriate MAC driver** work, not a single-line change.
  • **Pi 4 / 400:** the built-in port is the **Broadcom GENET** controller; it
    needs a dedicated driver in this kernel for bare-metal use.
  • **QEMU** Pi machines may not model Ethernet the same as real hardware; do
    not assume that “works in QEMU” implies a path on a physical Pi without
    matching drivers.

Quick try in QEMU (from repo root)
----------------------------------
  chmod +x scripts/run-raspi-qemu.sh   # once, if needed

    ./scripts/run-raspi-qemu.sh pi3
    ./scripts/run-raspi-qemu.sh pi4

  Omitting the argument defaults to **pi3**. The script builds a missing
  `rpi/dist/kernel8-pi3.img` or `kernel8-pi4.img` via `scripts/rpi-build.sh`.
  By default the script uses **`-serial stdio -monitor none`** (keyboard goes to
  guest UART) and passes **`-usb`** + **`usb-net`** + user NAT when
  `RPI_QEMU_NET` is on. **`usb-kbd` is off by default** — QEMU may deliver keys
  there while Eve has no USB-HID driver, which looks like a dead keyboard.
  Optional: `RPI_QEMU_USB_KBD=1`, `RPI_QEMU_NET=0`, `RPI_QEMU_SERIAL_MON=1`
  (`-serial mon:stdio`; use **Ctrl-a c** to switch serial vs monitor).

  Add **-display none** to the `qemu-system-aarch64` invocation inside the
  script if you want serial-only.

  **UTM:** paste **`utm/qemu-extra-rpi.args`** into “Additional QEMU
  Arguments” together with `-M`, `-m`, `-kernel`, and `-serial` — see
  `utm/RPI-UTM-SETUP.md`.

Related docs
------------
  • `rpi/RPI-IMAGES.md` — SD card workflow and supported boards.
  • `utm/RPI-UTM-SETUP.md` — UTM on macOS, same QEMU machine names.
