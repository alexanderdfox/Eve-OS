Eve OS — UTM setup (macOS)
==========================

**Same workflow as every other Eve target:** build → use files under `utm/` → UTM.
See `utm/SETUP-ALL-DEVICES.md` for the full matrix; this file is the x86 PC detail.

For **native AArch64 UEFI** on Apple Silicon (QEMU `virt` + EDK2, not Pi):

  ./scripts/arm-uefi-sync.sh
  utm/ARM-UEFI-SETUP.md

For Raspberry Pi in UTM (QEMU raspi3b/raspi4b + kernel8 images), run
  ./scripts/rpi-utm-sync.sh
and read utm/RPI-UTM-SETUP.md.

For Raspberry Pi SD card (real hardware), see rpi/RPI-IMAGES.md and scripts/rpi-all.sh.

You need a nightly Rust toolchain and UTM (utm.app). On Apple Silicon, the **full** Eve OS
is always an **emulated x86_64** guest (TCG — slow but complete). Read **`utm/MAC-M1-PRO.md`**
for Terminal vs UTM vs Asahi; **`./scripts/run-eve-x86-macos.sh`** is the quick QEMU path.

1) Build and copy the boot disk(s)
   From the Eve repo root, either:

     ./scripts/utm-sync.sh

   (debug `cargo build`, BIOS disk only → `utm/eve-bios.img`)

   Or build **every** image (release): x86 BIOS + UEFI, RPi ×2, AArch64 UEFI:

     ./scripts/build-all-images.sh

   See `utm/BUILT-IMAGES.md` for the full list.

   `utm-sync.sh` copies `bios.img` to:

     utm/eve-bios.img

   **Physical PC + USB stick (x86_64):** after a release build has produced the
   images, use `./scripts/x86-usb-write.sh` — see `utm/X86-USB-BOOT.md`.

2) Create a new UTM virtual machine
   - Open UTM → Create a New Virtual Machine.
   - Choose Emulate (not Virtualize). Pick an x86_64 / PC template if offered.
   - Architecture: x86_64, variant “PC” or “Q35” is fine.

3) Attach the disk
   - In Drives / Storage, add a drive and Import the file:
       utm/eve-bios.img
   - Interface: try IDE first if the guest does not boot; VirtIO Block also
     works with the rust-osdev bootloader in many setups.
   - Do not point UTM at a random path inside Cargo’s target dir; always use
     the copied `utm/eve-bios.img` after each rebuild (re-run utm-sync.sh).

4) Networking (VirtIO default; rtl8139 / e1000 optional)
   The same VirtIO user-NAT + USB keyboard setup is what **`cargo run`** (repo
   root, `eve-os` crate) passes to **`qemu-system-x86_64`**: you get matching
   behavior in Terminal and in UTM as long as the extra-args file is appended.

   Eve supports **virtio-net-pci**, **Realtek rtl8139** (`-device rtl8139`), or **Intel e1000-class** (`-device e1000`, `-device e1000-82545em`, …).
   In UTM’s QEMU settings, find the field
   for extra QEMU arguments (name varies by UTM version, e.g. “Additional
   QEMU Arguments”) and append the contents of one of:

     utm/qemu-extra.args        — PC / i440FX-style templates
     utm/qemu-extra-q35.args    — Q35 / UEFI (ICH9 HDA)

   Use **`utm/qemu-extra.args`** for typical “PC” / i440FX templates. For **Q35**
   (recommended with OVMF / UEFI), use **`utm/qemu-extra-q35.args`** instead so
   the HDA controller is **ICH9** (`ich9-intel-hda`), which matches the chipset.

   Both files include **virtio-net** + `-netdev user,…` (**SLIRP**: `ipv6=off,net=10.0.2.0/24,host=10.0.2.2,restrict=off`), Intel HDA (host audio
   via coreaudio on macOS), and **USB**: two **8-port hubs** (QEMU allows at most 8
   ports per hub) with **12 `usb-mouse`** plus **`usb-kbd`** on root port 2. Eve uses
   **PS/2 (i8042)** whenever USB HID polling is **off**, or as the **primary** pointer until USB
   boot mice actually deliver reports (enumeration alone is not enough). With **USB poll** on in
   Settings, **keyboard and mice** can come from **UHCI/OHCI HID boot** once transfers are healthy.
   Each working **`usb-mouse`** gets its own on-screen cursor; when USB mice are active, the
   **PS/2** pointer can use another slot so trackpad + USB mice can move together (up to 12 pointers).
   **USB poll defaults on** so `usb-kbd` / `usb-mouse` work immediately with the repo layout when
   the controller path works; turn it **off** if UHCI/OHCI transfers are unstable and you want
   PS/2-only fallback.
  HDA uses **`hda-output`** (playback only) so
   macOS CoreAudio does not try to open a capture (`adc`) voice that often fails.

     … -audiodev coreaudio,id=eve0 -device intel-hda -device hda-output,audiodev=eve0 …

   On Linux hosts, replace `coreaudio` with `alsa` (or another backend your QEMU
   lists under `qemu-system-x86_64 -audiodev help`). Eve does not implement an
   HDA driver yet; the device is present so PCI shows “AUD YES” and future
   drivers can use it.    Web audio is via a normal browser on the host using the
   URL in the Eve chrome bar.

   **Serial / debug log (COM1, 115200 8N1)**

   The kernel prints **`[EVE]`**-prefixed lines on **COM1** (NIC probe, framebuffer size, IP mode,
   DHCP / SLIRP fallback, fetch host, network errors, panics). Repo **`cargo run -p eve-os`** adds
   **`-serial stdio`** so those lines show in the same terminal as QEMU. To disable that (e.g. you
   attach COM1 in UTM or pass your own **`-serial`**), set **`EVE_QEMU_SERIAL=none`** before
   launching. For UTM, add a serial backend in **Additional QEMU arguments** if you need a log file
   — avoid two **`-serial stdio`** definitions on the same VM.

   **Internet — HTTP / HTTPS pages (QEMU / UTM + user NAT)**

   The guest stack is fixed to QEMU user networking: guest **`10.0.2.15`**, gateway
   **`10.0.2.2`**, DNS **`10.0.2.3`**. Status line should move through **`I ARP`**
   → **`TCP`** → TLS (for **`https://`**) → **`GET`** → **`WWW`** plus a byte counter.

   **`http://`** and **`https://`** both work. **HTTPS uses TLS 1.3** but **does not
   verify server certificates** on the bare-metal kernel (no PKIX / `ring` on this
   target) — see **`utm/BROWSER-LIMITS.md`**. Treat HTTPS as **encrypted wire only**
   unless you fully trust the path.

   Checklist:

   1. Append **`utm/qemu-extra.args`** or **`utm/qemu-extra-q35.args`** so the VM
      has **`-device virtio-net-pci,…`** and **`-netdev user,…`** matching **`cargo run -p eve-os`**
      (explicit **10.0.2.0/24** subnet — see **`utm/NETWORK-QEMU-UTM.md`**). For **e1000**- or **rtl8139**-only VMs, swap **`-device virtio-net-pci,…`** for **`-device e1000,netdev=n0`** or **`-device rtl8139,netdev=n0`** (keep the same **`-netdev user,…`**).
      More detail: **`utm/NETWORK-BROWSER.md`**, **`utm/NETWORK-QEMU-UTM.md`**.
   2. Open **SYS** (F1): **NIC** must not be **OFF** (default **VirtIO**). **Internet stack**
      must be **ON** (default). Wi‑Fi toggles do not matter for QEMU NAT.
   3. Fetched HTML is capped at about **12 KiB** and rendered as a small subset
      (no JavaScript) — **`utm/BROWSER-LIMITS.md`**.
   4. **Default home URL** is **`https://www.google.com/`** (or HTTP on older
      builds). To try a host-local page: on the **host**, from the Eve repo run
      **`python3 -m http.server 8080 --directory demo/qemu-http-test`**, then in
      Eve’s URL bar open **`http://10.0.2.2:8080/`** (QEMU maps **`10.0.2.2`**
      to the host).

   Turn **Internet stack** **OFF** in SYS to stop background polling.

   If UTM already adds its own network device, you can get **two NICs** and the wrong one may
   be probed first — **remove** the template NIC in UTM’s device list and rely only on the
   **virtio-net + user** line above, **or** use **Shared Network** without duplicating devices.

   **Bridged** mode in UTM does **not** work with Eve’s hardcoded **10.0.2.15** stack today; use
   **Shared** (NAT) for HTTP/HTTPS in the guest. See **`utm/NETWORK-QEMU-UTM.md`**.

   **Mouse / pointer:** Eve drives **PS/2** or **USB HID boot** (`usb-mouse` in the
   extra args). PS/2 stays the **primary** pointer until USB boot mouse reports succeed.
   A **USB tablet** alone is not the same protocol—use PS/2 + mouse,
   or keep the bundled `usb-mouse` devices. After kernel updates, re-run
   `./scripts/utm-sync.sh` so `utm/eve-bios.img` picks up fixes (e.g. PCI UHCI on
   function 1+ for `pc`/BIOS).

5) RAM and CPU
   - 256–512 MiB RAM is enough for the demo.
   - Emulated x86_64 on Apple Silicon uses TCG; expect low FPS.

6) After kernel changes
   - Run ./scripts/utm-sync.sh again, then restart the VM (or reset) so it
     loads the new `eve-bios.img`.

7) UEFI (optional)
   After `./scripts/build-all-images.sh` or `./scripts/utm-sync.sh` (when a debug
   or release build produced `uefi.img`), import:

     utm/eve-uefi.img

   as the boot disk and point UTM at **OVMF** / TianoCore x86_64 UEFI firmware
   (same pattern as QEMU `cargo run --release -p eve-os -- --uefi`).

   **Machine type:** use **Q35** (ICH9) / “QEMU Q35” if UTM exposes it — OVMF and
   the Eve kernel’s GOP path are unreliable on legacy i440fx. In “Additional
   QEMU Arguments”, you may need `-machine q35` if the template defaulted to PC,
   and paste **`utm/qemu-extra-q35.args`** (not the plain `qemu-extra.args`) so
   audio uses `ich9-intel-hda`.

   `build-all-images.sh` always copies the **release** `uefi.img`; `utm-sync.sh`
   copies the **debug** `uefi.img` — do not mix a release BIOS disk with a debug
   UEFI disk from an old `find` path (scripts now pick the newest file per profile).

   BIOS boot via `eve-bios.img` remains the default scripted path.

8) Troubleshooting: keyboard and mouse (UTM and QEMU)
  Eve’s x86_64 kernel uses **PS/2 (i8042)** and, when **USB poll** is on in
  Settings, **USB HID boot** devices behind **PCI UHCI/OHCI** (`kernel/src/ps2.rs`,
  `kernel/src/uhci.rs`, `kernel/src/ohci.rs`). **EHCI/xHCI** HID are not finished yet. Raspberry Pi /
   `kernel-rpi` has no in-guest USB keyboard or mouse stack — see
   `utm/RPI-UTM-SETUP.md`.

   **A. Use the correct extra QEMU arguments**
   - **i440FX / “PC” / BIOS templates:** paste the full contents of
     **`utm/qemu-extra.args`** into UTM’s “Additional QEMU Arguments”.
   - **Q35 / UEFI / OVMF:** use **`utm/qemu-extra-q35.args`** (ICH9 HDA + same USB
     layout). Do not mix the i440FX audio line with Q35.
   Missing `-usb`, **`usb-kbd`**, and the **`usb-mouse`** hub layout often explains
   “no USB input”; without extras, PS/2 may still work if QEMU’s default PS/2 is
   present, but VirtIO net and the tested USB topology will be wrong.

   **B. If the pointer or keys are dead or stuck (especially on Apple Silicon TCG)**
   - **USB poll** defaults **on**. If keys/pointer stall under TCG, turn it **off** in SYS to
     force PS/2-only input. With poll **on**, the kernel also **falls back to PS/2** after many
     consecutive failed USB HID interrupt INs (keyboard and mouse use the same stall threshold),
     and the **mouse** keeps PS/2 on the **primary** cursor until USB boot mouse reports succeed.
   - Turn **USB poll** **on** when you want **UHCI/OHCI** `usb-kbd` / `usb-mouse` (e.g.
     multi-pointer demo) and transfers are healthy.

   **C. USB tablet vs mouse**
   - A **USB tablet** alone is **not** a substitute for **`usb-mouse`** — different
     protocol. Keep the repo’s **`usb-mouse`** devices (and PS/2), or use PS/2 only
   per (B).

   **D. Focus and mouse grab**
   - Click inside the **guest display** so the window has keyboard focus.
   - If the cursor is captured, use UTM’s documented shortcut (often **Ctrl+Option**
     or **Ctrl+Command**) to release the pointer to the host.

   **E. Fresh disk image after kernel or bootloader fixes**
   - Run **`./scripts/utm-sync.sh`** (or **`./scripts/build-all-images.sh`**) and
     restart the VM so **`utm/eve-bios.img`** matches the kernel (e.g. PCI UHCI on
     function 1+ for `pc`/BIOS).

   **F. Compare with plain QEMU**
   - From the repo root: **`cargo run --release -p eve-os`** (BIOS) or
     **`cargo run --release -p eve-os -- --uefi`** (UEFI). If input works there but
     not in UTM, compare your pasted extra args character-for-character with
     **`utm/qemu-extra.args`** or **`utm/qemu-extra-q35.args`**.

   **G. Guest resets in a tight loop (firmware → bootloader → reset)**
   - Often a **kernel stack overflow** (triple fault) during early init. Eve keeps
     large network/cursor buffers in **`.bss`** and sets a **1 MiB** bootloader
     stack; always **`./scripts/utm-sync.sh`** after pulling kernel fixes so
     **`utm/eve-bios.img`** matches.
   - If you boot a **hybrid BIOS ISO** and loop at **ISOLINUX / memdisk**, that is a
     separate **Syslinux** issue — see **`install/pc-x86-64-iso/INSTALL.md`** (LINUX
     + memdisk + `*.c32` modules).
  - Fast isolation for UTM: boot from **raw disk images** first (not ISO):
      - BIOS firmware VM -> `utm/eve-bios.img`
      - UEFI firmware VM (Q35/OVMF) -> `utm/eve-uefi.img`
    If those work but ISO loops, rebuild ISO (`make iso-x86`) and re-attach.
  - On Apple Silicon, avoid mixing machine/firmware styles:
      - UEFI path: **Q35 + OVMF + `utm/qemu-extra-q35.args`**
      - BIOS path: **PC/i440FX + BIOS + `utm/qemu-extra.args`**
