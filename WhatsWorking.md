# What’s working in Eve

Snapshot of **x86_64** guest behavior (QEMU / UTM / bare metal where noted). Sources: `kernel/src/main.rs`, `kernel/src/html.rs`, driver modules, `utm/BROWSER-LIMITS.md`.

---

## Ethernet / networking (PCI)

| Component | Works | Notes |
|-----------|:-----:|-------|
| **VirtIO net** | Yes | Primary path for QEMU / UTM; full TX/RX with VirtIO header handling. |
| **RTL8139** | Yes | Legacy QEMU “rtl8139” device. |
| **RTL8168 / 8169** | Yes | MMIO C+ ring driver (`rtl8168.rs`); real hardware / some VMs. |
| **Intel e1000 / e1000e-class IDs** | Yes | Same descriptor path as e1000; extra PCI IDs for e1000e-style devices (verify per hypervisor). |
| **AMD PCnet (pcnet)** | Yes | QEMU `pcnet` device. |
| **vmxnet3** | Partial | PCI attach + MAC read + incremental TX/RX ring scaffolding in-tree (`vmxnet3.rs`); not full UPT parity yet. |
| **Broadcom bge / tg3-class** | No | Stub only; no firmware/MMIO bring-up in tree. |
| **802.11 / Wi‑Fi MAC** | No | SYS shows SSID/PSK UI only; no WPA / no PHY driver (`utm/WIFI-80211.md`). |
| **IPv6** | No | IPv4 stack only. |

**IP configuration (SYS):** SLIRP defaults (`10.0.2.x`), DHCP client, or static IPv4 — works with a supported NIC and stack enabled.

---

## Storage

| Component | Works | Notes |
|-----------|:-----:|-------|
| **virtio-blk** | Yes | Used for disk install (clone disk A → B) when two disks present. |
| **AHCI / SATA / NVMe / USB mass storage** | No | Not implemented as boot/browser storage in this kernel. |

---

## USB & input

| Component | Works | Notes |
|-----------|:-----:|-------|
| **PS/2 keyboard & mouse** | Yes | i8042; ImPS/2 4-byte mouse packets. |
| **UHCI** | Yes | I/O; USB HID boot keyboard + boot mice (with USB poll in SYS). |
| **OHCI** | Yes | MMIO; same HID usage as UHCI. |
| **USB HID multi-mouse** | Yes | Up to 12 pointers when USB poll on. PS/2 keeps the **primary** cursor until a USB boot mouse produces a good interrupt IN; then USB mice use lower slots and PS/2 can use another slot when both are active. |
| **xHCI** | Partial | PCI attach path + HID fallback routing to legacy USB backends; native xHCI HID path still incomplete (`xhci.rs`). |
| **EHCI** | Partial | PCI hook exists; FS-through-EHCI not finished (`ehci.rs`). |

**PS/2 vs USB (when USB poll is on):** the kernel still polls enumerated USB HID devices even while falling back. **Keyboard:** PS/2 is ignored only after USB boot keyboard IN succeeds; many consecutive failed INs re-enable PS/2 (same stall threshold as mice). **Mouse:** see the multi-mouse row — enumeration alone does not move PS/2 off the primary pointer.

**Bare metal laptops:** often **xHCI-only** → built-in keyboard/touchpad may not work without PS/2 or UHCI/OHCI (`install/REAL-HARDWARE.md`).

---

## Display, audio, power

| Component | Works | Notes |
|-----------|:-----:|-------|
| **Framebuffer (bootloader GOP / VESA)** | Yes | Main Eve UI on x86_64. |
| **GPU acceleration** | No | No native GPU command submission; linear framebuffer only. |
| **MIDI (UI flag)** | Partial | SYS toggle / channel; not a full audio stack review here. |
| **PCI MMIO “audio” detection** | UI only | Enumerated for SYS display; not a full sound driver. |
| **Reboot / shutdown** | Yes | From SYS via `power` module (platform-dependent). |
| **Epilepsy / photosensitivity notice** | Yes | Shown at boot before browser (`Screen::EpilepsyWarning`). |

**Other platforms**

| Platform | Works | Notes |
|----------|:-----:|-------|
| **kernel-rpi** | Yes (core UI path) | Uses shared `arm_run` loop with browser/settings/log UI, serial keyboard/mouse parsing, and framebuffer rendering. |
| **kernel-arm-uefi** | Yes (core UI path) | Uses shared `arm_run` loop with same UI/event flow as RPi path (platform HW features still differ). |

---

## Web stack (fetch)

| Feature | Works | Notes |
|---------|:-----:|-------|
| **ARP** | Yes | |
| **DNS (UDP)** | Yes | e.g. SLIRP `10.0.2.3` (`utm/NETWORK-BROWSER.md`). |
| **TCP** | Yes | Single-connection style client. |
| **HTTP/1.0** | Yes | `GET`, response parsing; not full HTTP/1.1 feature set. |
| **HTTPS (TLS 1.3)** | Yes | Encrypted with in-tree verified provider/trust anchors path (`eve_tls.rs`, `net.rs`). |
| **HTTP/2, HTTP/3, QUIC** | No | |
| **Cookies, sessions, auth** | No | |
| **WebSockets** | No | |
| **Downloads / file picker** | No | |
| **JavaScript** | Partial (in-house marker VM) | `<script>` tags are still stripped, but optional `eve-script:` bytecode marker can execute via in-house VM when SYS toggle is enabled. |
| **Major-browser parity** | No | No full Chromium/Firefox/Safari-equivalent engine behavior yet. |
| **Broad modern web platform APIs** | No | No DOM API surface parity (workers, storage APIs, media APIs, canvas/WebGL, etc.). |

---

## HTML / CSS rendering (`html.rs`)

| Feature | Works | Notes |
|---------|:-----:|-------|
| **Plain text / block flow** | Yes | Line-based layout; not a full CSS box model. |
| **`<b>`, `<i>`** | Yes | |
| **Limited colors** | Yes | From `<style>` and inline `style="..."` (subset). |
| **Basic HTML entities** | Yes | |
| **`<script>`** | Removed | Never run. |
| **`<iframe>`, `<object>`** | Skipped | Subtrees not rendered. |
| **`<embed>`** | Dropped | Open tag dropped. |
| **`<meta>`, `<link>`, `<base>`** | Ignored | |
| **Safe `href` handling** | Partial | `javascript:`, `vbscript:`, `data:` on `<a>` don’t get link styling; not a full URL policy engine. |
| **Full CSS** | No | No flex/grid, tables-as-layout, z-index stacking model, etc. |
| **Rich CSS/layout parity** | No | No full CSS2.2/CSS3+ layout and cascade parity with major browsers. |
| **WebAssembly / plugins** | No | |

**Practical limits:** rendered lines are capped (`BROWSER_MAX_LINES`, `BROWSER_LINE_CAP` in `html.rs`); large pages truncate with UI feedback.

---

## Summary

- **Good for:** VirtIO or common emulated NICs, simple HTTP/HTTPS pages, read-only HTML subset, PS/2 or UHCI/OHCI USB HID, framebuffer UI on x86_64 guests.
- **Not there yet:** Major-browser parity, full vmxnet3/Broadcom production drivers, full native xHCI HID, rich CSS/layout parity, and broad modern web platform APIs.

For deeper browser/network detail, see `utm/BROWSER-LIMITS.md` and `utm/NETWORK-BROWSER.md`.
