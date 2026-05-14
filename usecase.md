# Eve OS — use cases

Eve OS is a small **in-kernel** operating system: framebuffer UI, a minimal **HTTP/HTTPS** client with a constrained **HTML** renderer, settings and logs, and optional **multi-pointer** USB HID on x86_64. It is aimed at **demos, education, and experimentation**, not at replacing a general-purpose desktop or phone OS.

For capability matrices and limits, see [`WhatsWorking.md`](WhatsWorking.md), [`utm/BROWSER-LIMITS.md`](utm/BROWSER-LIMITS.md), and [`utm/NETWORK-BROWSER.md`](utm/NETWORK-BROWSER.md). Images and build flow: [`README.md`](README.md), [`utm/UTM-SETUP.md`](utm/UTM-SETUP.md), [`install/REAL-HARDWARE.md`](install/REAL-HARDWARE.md).

---

## Who it is for

- **Students and hobbyists** learning how a kernel can drive a framebuffer, parse HTML, speak TCP/TLS, and talk to PCI devices without a full Unix userspace.
- **Emulator users** (QEMU, UTM on Apple Silicon, similar) who want a self-contained guest that boots quickly and fetches simple pages over **user NAT** or a lab LAN.
- **Developers** extending Eve’s drivers or UI: the codebase is bounded enough to read end-to-end compared with a mainstream OS.

---

## Primary use cases

### 1. Read-only “kiosk browser” for simple pages

Open known-good **HTTP** or **HTTPS** URLs (small pages work best; response size and line counts are capped). The default home page is **[TempleOS Web Shrine](https://alexanderdfox.github.io/TempleOSWebShrine/)** (HTTPS). Use Eve as a **single-purpose viewer** for documentation sites, status pages, or your own static HTML served on a LAN or from the host (for example QEMU **`10.0.2.2`** mapping, described in [`utm/UTM-SETUP.md`](utm/UTM-SETUP.md)).

**Fit:** Plain HTML, limited styling, no expectation of full CSS or modern site frameworks.

### 2. Network stack and TLS teaching lab

Demonstrate **ARP**, **DNS**, **TCP**, and **TLS 1.3** in a real guest with a visible status line and log tab. Eve uses a deliberately small stack; stepping through behavior is easier than in a full browser engine.

**Fit:** Courses or self-study on protocols; not PKIX-heavy “trust the whole Web” scenarios (see browser limits for certificate handling).

### 3. QEMU / UTM demo on a laptop

Boot **`build/eve-bios.img`**, **`build/eve-uefi.img`**, or the hybrid **`build/eve-x86_64.iso`** (or run **`cargo run -p eve-os`** from a dev checkout). Append the repo’s **QEMU extra arguments** so **VirtIO net** and the documented **USB HID** topology match what the kernel expects.

**Fit:** Demos at a desk; follow [`utm/UTM-SETUP.md`](utm/UTM-SETUP.md) for keyboard, mouse, and NAT.

### 4. Settings, logs, and diagnostics

Use **SYS** for NIC mode, IP mode (SLIRP vs DHCP vs static), toggles that reflect what the kernel actually implements, and **LOG** for serial-style diagnostics. **Disk install** appears when the platform exposes two **virtio-blk** disks—useful for clone-to-second-disk experiments in QEMU.

**Fit:** Operators who want a minimal control surface without SSH or a shell.

### 5. AArch64 bring-up (Raspberry Pi or UEFI sample)

**`kernel-rpi`** and **`kernel-arm-uefi`** share much of the same UI loop as x86, with platform-appropriate input (for example serial on Pi) and framebuffer where available. Use these when exploring Eve’s UI on non-PC targets described under “Other platforms” in [`WhatsWorking.md`](WhatsWorking.md).

**Fit:** Porting and UI experiments; not a turnkey Pi desktop today.

### 6. Bare-metal smoke on a PC tower

Boot from a USB stick written per **`install/`** guides when the machine has **legacy BIOS or UEFI** aligned with the image, a supported **PCI Ethernet** chip for the in-tree drivers, and **PS/2 or UHCI/OHCI** USB paths for input where possible.

**Fit:** Towers and older laptops with visible PS/2 or routed USB 1.1/2.0 companions. Read [`install/REAL-HARDWARE.md`](install/REAL-HARDWARE.md) before expecting laptop built-in keyboards on **xHCI-only** firmware.

---

## When not to use Eve OS

- **Daily driver** browsing, banking, or logging into arbitrary SaaS (HTML, TLS trust, and scripting are intentionally limited).
- **Wi‑Fi laptops** as the only network path (no in-tree 802.11 MAC driver; Ethernet or emulated VirtIO is the realistic path).
- **Storage-heavy workflows** (no AHCI/NVMe/USB mass storage as a general boot volume in this kernel).
- **Production multi-user security** without your own threat model review; Eve is a research/education surface, not a hardened product line.

---

## Summary

| You want… | Eve is a good fit if… |
|-----------|------------------------|
| A tiny guest that fetches simple web pages | You control the page complexity and accept HTML/CSS limits. |
| A readable kernel + UI codebase | You are learning or extending systems code. |
| Predictable QEMU/UTM networking | You use the documented NIC and `10.0.2.0/24` assumptions or adjust SYS IP mode. |
| Laptop bare-metal everywhere | You read `install/REAL-HARDWARE.md` and accept input/NIC constraints. |

Questions of “does feature X exist?” should defer to [`WhatsWorking.md`](WhatsWorking.md) first; that file is maintained as the project’s feature snapshot.
