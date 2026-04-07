Eve OS on Mac with Apple Silicon (M1 / M1 Pro / M2 / M3)
=========================================================

The **full** Eve browser + GUI + VirtIO networking stack is an **x86_64** kernel (`kernel/`).
It does **not** run natively on ARM64 — not inside the small `kernel-arm-uefi` payload, and not
on bare-metal Apple Silicon without a VM.

You have **two** supported ways to use Eve on an M1-class Mac:

──────────────────────────────────────────────────────────────────────────
A) **Full Eve OS** — QEMU / Terminal (recommended for development on macOS)
──────────────────────────────────────────────────────────────────────────

  1. Install QEMU:  `brew install qemu`
  2. From the repo root:

       ./scripts/run-eve-x86-macos.sh

     Or equivalently:

       export EVE_QEMU_M=1024M   # optional; script defaults this on macOS wrapper
       cargo run --release

     **UEFI / Q35** variant:

       ./scripts/run-eve-x86-macos.sh -- --uefi

  3. The guest uses **TCG** (software CPU emulation). The first launch may take a while while
     Rust builds the kernel. **Performance is slower than Linux+KVM** — that is expected on Apple
     Silicon emulating x86_64.

  4. Networking matches QEMU user NAT: guest **10.0.2.15**, gateway **10.0.2.2**, DNS **10.0.2.3**
     (same as `utm/UTM-SETUP.md`).

**UTM (GUI)** with the same disk layout + extras: import `utm/eve-bios.img` or UEFI image, then paste
arguments from `utm/qemu-extra.args` or `utm/qemu-extra-q35.args` into UTM’s “Additional QEMU
arguments” — see **`utm/UTM-SETUP.md`**.

──────────────────────────────────────────────────────────────────────────
B) **Asahi Linux on the same Mac** — AArch64 UEFI *demo* from GRUB
──────────────────────────────────────────────────────────────────────────

  After installing **Fedora Asahi** (or another Asahi distro) on the machine, you can **chainload**
  Eve’s `BOOTAA64.EFI` from GRUB. That payload is **only** the UEFI demo (GOP splash + serial), not the
  x86 desktop. Use GRUB’s default entry to boot Linux again.

  Docs: **`utm/ASAHI-M1-UEFI-SETUP.md`**, **`./scripts/asahi-grub-add-eve.sh`**.

  On **Linux (Asahi)**, from the repo:

    ./eve4mac.sh

  installs/syncs that demo. On **macOS**, run **`./eve4mac.sh --help`** — the default script is for
  Asahi, not for the full OS VM.

──────────────────────────────────────────────────────────────────────────
What “fully working” means today
──────────────────────────────────────────────────────────────────────────

  • **Full OS** on M1 Pro ⇒ **x86_64 QEMU/UTM** (section A).

  • **Native ARM** on the Mac ⇒ **demo only** (section B), by design — see
    **`utm/TODO-PLATFORMS.md`** for porting the big stack to AArch64.

  • **Physical x86 PC** ⇒ USB images / BIOS or UEFI — **`install/REAL-HARDWARE.md`**.

Quick links:  **`utm/SETUP-ALL-DEVICES.md`**, **`eve4mac.sh --help`**, **`scripts/print-eve-paths.sh`**
