Eve — Raspberry Pi kernels in UTM (macOS)
==========================================

**Same workflow as other targets:** build → `utm/rpi/*.img` → UTM. Full matrix:
`utm/SETUP-ALL-DEVICES.md`. Paths: `./scripts/print-eve-paths.sh`.

These are AArch64 bare-metal images (UART + framebuffer). They are NOT the x86 PC
image (use utm/eve-bios.img + UTM-SETUP.md for QEMU x86_64).
Current input path on `kernel-rpi`: serial ANSI keyboard + serial mouse reporting
(xterm SGR) bridged into the shared ARM UI loop.

1) Build and install kernels + ready-made UTM bundles
   From the Eve repo root:

     ./scripts/rpi-utm-sync.sh
     # or: make utm-rpi

   Produces:
     utm/rpi/kernel8-pi3.img   — BCM2837 profile (Pi 3, 3B+, CM3, Zero 2 W)
     utm/rpi/kernel8-pi4.img   — BCM2711 profile (Pi 4, 400)
     utm/rpi/Eve-Pi3.utm       — double-click in UTM (raspi3b, serial + usb-net)
     utm/rpi/Eve-Pi4.utm       — double-click in UTM (raspi4b, serial + usb-net)

   **Quick path (UTM 4.7.5):** double-click **`utm/rpi/Launch-Eve-Pi3.command`**
   (or `Launch-Eve-Pi4.command`). Requires `brew install qemu`. This avoids a UTM 4.7.5
   bug where the GUI injects **`virtio-serial-pci`** (no PCI bus on `raspi*`).

   **UTM `.utm` bundles** (`Eve-Pi3.utm`, `Eve-Pi4.utm`) are for a future UTM with
   experimental Pi support ([utmapp/UTM#7608](https://github.com/utmapp/UTM/pull/7608));
   on **UTM 4.7.5** they fail with `No 'PCI' bus found for device 'virtio-serial-pci'`.

   Or: `make qemu-rpi3` / `make qemu-rpi4` from Terminal.

   Requires: nightly Rust, aarch64-unknown-none, llvm-tools-preview
   (same as rpi/RPI-IMAGES.md).

2) Manual UTM setup (skip if you use the `.utm` bundles above)
   Create a VM → Emulate → ARM64, then paste QEMU extra args (section 3).

3) Manual QEMU extra arguments
   In UTM → your VM → QEMU → “Additional QEMU Arguments” (wording varies),
   add ONE of the following single lines. Use the full path to your Eve
   clone (adjust USER and path).

   **Network (QEMU devices, same as `./scripts/run-raspi-qemu.sh` with NAT on):**
   append the contents of **`utm/qemu-extra-rpi.args`** to your extra-args
   line (after the `-kernel …` fragment, on the same line or merged). That
   adds **user NAT** (`usb-net`, guest-visible SLIRP). QEMU 5.1+ USB on
   `raspi3b`/`raspi4b` is required for `usb-net`.

  Eve’s **kernel-rpi** does not implement USB HID or USB Ethernet drivers.
  **Keyboard** must go to **PL011 UART** (use `-serial stdio -monitor none` and
  type in the terminal / serial pane — not `usb-kbd`, which QEMU may bind
  input to while the guest never reads it). **Mouse** in the QEMU **video**
  window is not wired to the guest; pointer comes from **xterm SGR** on the
  serial stream (same terminal) or use **Tab / arrows** in the UI.

   Pi 3–class kernel (most common for QEMU “raspi3b”):

     With graphics (default QEMU window shows a framebuffer splash):

    -M raspi3b -m 1G -kernel "/Users/USER/Desktop/Eve/utm/rpi/kernel8-pi3.img" -serial stdio -monitor none

     Same with user NAT (paste `utm/qemu-extra-rpi.args` at the end):

    -M raspi3b -m 1G -kernel "/Users/USER/Desktop/Eve/utm/rpi/kernel8-pi3.img" -serial stdio -monitor none -usb -netdev user,id=rpi0,ipv6=off -device usb-net,netdev=rpi0

     Serial only (no video window):

    -M raspi3b -m 1G -kernel "/Users/USER/Desktop/Eve/utm/rpi/kernel8-pi3.img" -serial stdio -monitor none -display none

   Pi 4–class kernel (needs QEMU that supports raspi4b; try this if Pi 3
   machine fails with the pi4 image):

    -M raspi4b -m 2G -kernel "/Users/USER/Desktop/Eve/utm/rpi/kernel8-pi4.img" -serial stdio -monitor none

     With NAT (append `utm/qemu-extra-rpi.args`):

    -M raspi4b -m 2G -kernel "/Users/USER/Desktop/Eve/utm/rpi/kernel8-pi4.img" -serial stdio -monitor none -usb -netdev user,id=rpi0,ipv6=off -device usb-net,netdev=rpi0

     (Add “-display none” on that line too if you want no GUI.)

   **UTM “Network” mode:** “Shared Network” (NAT) is the host side; it does not
   replace the guest-visible `usb-net` device above. If UTM also adds a default
   virtio or other NIC and QEMU errors on duplicate backends, remove the extra
   NIC in UTM’s UI and keep only the arguments you paste here.

   Notes:
   - The kernel brings up a 32 bpp framebuffer via the VideoCore mailbox and
     draws a simple splash; HDMI / the QEMU display window should not stay black.
  - For serial-driven keyboard/mouse, prefer `-serial stdio -monitor none`.
    `mon:stdio` shares monitor + serial on one stream and is less predictable for input.
   - If `-M raspi4b` is unknown, your QEMU is too old — use the Pi3 kernel
     line with `-M raspi3b` only.
   - Remove or avoid duplicate `-machine` / `-m` flags if UTM already sets them;
     merge into one consistent command in “Open QEMU in Terminal” if needed.

4) What you should see
   Serial: a few lines starting with “EVE / Raspberry Pi AArch64”, including
   either “Framebuffer: 32 bpp…” or a mailbox failure fallback message.

   Display: omit “-display none” (or use UTM’s normal display) to see the
   gradient splash and colored bars. If the mailbox path fails on unusual
   firmware, you still get UART on GPIO14/15 at 115200 8N1.

5) Real hardware SD cards
   For SD images and firmware blobs, use scripts/rpi-all.sh (one SOC at a time)
   or rpi-build-all.sh + rpi-assemble-boot.sh with RPI_SOC=pi3 or pi4.
   See rpi/RPI-IMAGES.md.

6) Troubleshooting (UTM / QEMU)

   **`Invalid SMP CPUs 8` / max 4 on raspi4b** — UTM defaulted CPU count to your
   host core count. Re-run `make utm-rpi` (bundles pin `CPUCount` to 4) or set
   **4 cores** manually in the VM System settings.

   **`No 'PCI' bus found for device 'virtio-serial-pci'`** — UTM **4.7.5** always
   adds SPICE/guest-agent PCI devices; Raspberry Pi QEMU machines have **no PCI**.
   Use **`utm/rpi/Launch-Eve-Pi3.command`** (or Pi4) or `make qemu-rpi3` instead of
   the `.utm` bundle until UTM ships Pi machine fixes (see utmapp/UTM PR #7608).
