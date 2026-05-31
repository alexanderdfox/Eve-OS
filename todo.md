# Eve OS — issues to fix

Actionable gaps found by scanning source, driver stubs, and project docs (`WhatsWorking.md`, `utm/TODO-PLATFORMS.md`, `install/REAL-HARDWARE.md`, etc.). Ordered roughly by user impact on x86_64, then cross-platform work.

---

## High impact — input & bare metal

- [ ] **EHCI USB HID** — `kernel/src/ehci.rs` logs companion presence and returns false; full-speed HID through EHCI (split transactions) is not implemented — use OHCI/UHCI companion or **`xhci_native`**.
- [x] **Native xHCI USB HID (MVP)** — `kernel/src/xhci_native.rs`: boot keyboard + one boot mouse on xHCI-only root ports; companion audit in `xhci.rs`. Real-hardware validation still open.
- [ ] **AArch64 USB HID host** — `kernel/src/usb_hid/stub.rs` is a no-op on non-x86 targets. Pi and bare-metal ARM need a USB stack + HID or documented VirtIO-input path.

---

## High impact — networking & browser fetch

- [ ] **HTTP `Transfer-Encoding: chunked`** — not decoded (`kernel/src/net.rs`, `utm/NETWORK-BROWSER.md`). Many HTTP/1.1 sites hang or hit `HTTP TIMEOUT`.
- [ ] **TLS wall clock** — no RTC/NTP; cert validation uses build epoch + guest uptime only (`kernel/src/eve_tls.rs`). Expired/not-yet-valid certs may misbehave vs real UTC.
- [ ] **vmxnet3 driver** — attach/MAC/ring scaffolding only; full UPT queue activation incomplete (`kernel/src/vmxnet3.rs`, `WhatsWorking.md`).
- [ ] **Broadcom bge / tg3-class NIC** — stub only, always `None` on probe (`kernel/src/bge.rs`).
- [ ] **IPv6** — stack is IPv4-only (`kernel/src/main.rs`, `WhatsWorking.md`).

---

## High impact — UI gaps users can see

- [ ] **Browser back / forward buttons** — rendered in chrome but clicks are no-ops (`kernel/src/gfx.rs` ~4487). Implement history or hide/disable the buttons.
- [ ] **Static IP octet editing** — SYS supports static mode but UI cannot edit individual octets yet (`kernel/src/settings.rs`).
- [ ] **Settings persistence on x86** — RAM-only; NVRAM saver exists for AArch64 UEFI only (`kernel/src/settings_persist.rs`).

---

## Platform — Raspberry Pi

- [ ] **Pi 4 / 400 GENET Ethernet** — onboard port needs a dedicated driver for bare-metal use (`rpi/PI3-PI4-GUI-NETWORK.md`).
- [ ] **Pi USB Ethernet** — QEMU `usb-net` device is not consumed; no USB host + MAC driver in tree (`rpi/PI3-PI4-GUI-NETWORK.md`).
- [ ] **Pi USB keyboard** — Eve has no USB-HID driver; `run-raspi-qemu.sh` keeps `usb-kbd` off by default to avoid “dead keyboard” confusion. UART/serial input only today.
- [ ] **Real Pi vs QEMU networking** — VirtIO-MMIO works in VM-like firmware; physical Pi hardware paths differ and need matching drivers.

---

## Platform — AArch64 UEFI

- [ ] **Reboot / shutdown on AArch64** — `kernel/src/power/stub.rs` spins forever; no ACPI/UEFI reset integration from the shared kernel path.
- [ ] **VirtIO-MMIO scan gating** — MMIO probe at `0x0a00_0000` is off by default on bare metal (correct for safety) but must be enabled explicitly for each VM/firmware combo (`kernel/src/nic/aarch64.rs`).

---

## Storage

- [ ] **AHCI / SATA / NVMe / USB mass storage** — not implemented as boot or general storage (`WhatsWorking.md`). Only VirtIO-blk disk install (two-disk clone) exists on x86.

---

## Settings placeholders (SYS toggles with no backend)

- [ ] **802.11 / Wi‑Fi** — PCI probe + fake scan SSIDs in UI only; no WPA, beacon scan, or frame TX/RX (`utm/WIFI-80211.md`, `kernel/src/gfx.rs`).
- [ ] **Bluetooth** — SYS toggle only; no HCI or stack (`kernel/src/main.rs`, `kernel/src/settings.rs`).
- [ ] **HDA / audio** — enumerated for display; not a full sound driver (`install/REAL-HARDWARE.md`).

---

## Web stack & rendering

- [ ] **HTTP/2, HTTP/3, QUIC** — not supported (`WhatsWorking.md`).
- [ ] **Cookies, sessions, WebSockets, downloads** — not supported.
- [ ] **Full CSS layout** — tiny color/spacing subset only; no flex/grid/tables-as-layout (`kernel/src/html.rs`, `utm/BROWSER-LIMITS.md`).
- [ ] **Classic `<script>` / ECMAScript** — tags stripped; not a browser JS engine. Optional `eve-script:` bytecode VM is a scaffold (`kernel/src/script_runtime.rs`), not ECMAScript.
- [ ] **Safe `href` / URL policy** — partial; not a full mixed-content or scheme blocklist engine (`WhatsWorking.md`).
- [ ] **`<iframe>` / `<object>` / plugins / WebAssembly** — skipped or unsupported.

---

## Architecture & code sharing

- [ ] **Unify desktop kernel strategy** — decide single x86 kernel vs shared `no_std` core linked by x86, ARM UEFI, and RPi (`utm/TODO-PLATFORMS.md` §1).
- [ ] **Framebuffer HAL trait** — abstract stride/format/blit for x86 GOP, Pi mailbox FB, UEFI GOP (`utm/TODO-PLATFORMS.md` §1).
- [ ] **Timer / time source abstraction** — needed for TCP, USB scheduling, UI polling, and real TLS clocks.
- [ ] **PCI / MMIO discovery abstraction** — x86 PCI vs Pi MMIO vs UEFI protocols.

---

## Testing, CI & docs

- [ ] **Stale platform doc** — `utm/TODO-PLATFORMS.md` still says `kernel-rpi/` is “UART + splash only” and `kernel-arm-uefi/` is “GOP fill + idle”; both now run shared `arm_run` UI. Update the intro and parity tables.
- [ ] **Per-platform feature matrix** — `utm/BUILT-IMAGES.md` / `utm/SETUP-ALL-DEVICES.md` should state what works per image (`utm/TODO-PLATFORMS.md` §8).
- [ ] **x86 BIOS vs UEFI regression** — re-run full input + net + UI smoke on both image types; same kernel, different firmware paths (`utm/TODO-PLATFORMS.md` §7).
- [ ] **No CI workflow** — repo has `scripts/verify-repo.sh` but no `.github/workflows` to run it on push/PR.
- [ ] **Documented per-platform regression commands** — add known-good QEMU/hardware commands for net + USB HID (`utm/TODO-PLATFORMS.md` §8).

---

## Lower priority / incremental

- [ ] **HTTP/1.1 keep-alive reuse** — partial; chunked bodies remain the main gap.
- [ ] **GPU acceleration** — linear framebuffer only; no native GPU command submission.
- [ ] **MIDI toggle** — UI flag exists; full audio path not reviewed.
- [ ] **Pi HDMI framebuffer limits** — document resolution/performance constraints vs Eve UI expectations (`utm/TODO-PLATFORMS.md` §7).
- [ ] **Asahi M1 install bundle** — `install/linux-asahi-m1/EFI/EVE/PLACEHOLDER.md` until `populate-from-repo.sh` is run.

---

*Generated from repo scan. For current capability snapshots see `WhatsWorking.md`; for platform parity roadmap see `utm/TODO-PLATFORMS.md`.*
