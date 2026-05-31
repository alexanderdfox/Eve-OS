# Platform parity TODO

Roadmap to bring **networking**, **multi-pointer / USB HID**, **HTML / CSS / JS**, **keyboard**, and the full **Eve GUI** (`kernel` + `gfx` + browser stack) to every supported boot target.

**Today:** The full browser stack (`kernel/` — `gfx`, `html`, `net`, USB HID on x86) ships on **x86_64** (BIOS + UEFI). **`kernel-rpi/`** and **`kernel-arm-uefi/`** run the shared **`arm_run`** UI loop (browser chrome, SYS, LOG) with serial/UEFI input and framebuffer output; platform drivers (USB HID on Pi, bare-metal GENET, xHCI-only laptops, etc.) remain **work remaining** below.

---

## Legend

| Column | Meaning |
|--------|---------|
| **x86 BIOS** | PC images using legacy BIOS boot (`utm/eve-bios.img`, Syslinux path) — same guest kernel as UEFI once loaded. |
| **x86 UEFI** | PC images with GPT + ESP (`utm/eve-uefi.img`, OVMF). |
| **ARM UEFI** | AArch64 UEFI payload (`kernel-arm-uefi/`) — QEMU `virt` / UTM EDK2 / Apple Silicon UTM per `utm/ARM-UEFI-SETUP.md`, `utm/ASAHI-M1-UEFI-SETUP.md`. |
| **Pi 3 family** | `kernel-rpi` with `--features soc_pi3` (Pi 3, Zero 2, 3B+). |
| **Pi 4 family** | `kernel-rpi` with `--features soc_pi4` (Pi 4, 400). |

---

## 1. Shared kernel / crate strategy

- [ ] Decide **one Eve “desktop” kernel** vs **shared `no_std` core library** linked by x86, ARM UEFI, and RPi binaries (avoid duplicating `gfx`, `html`, `net`, USB stacks three ways).
- [ ] Abstract **framebuffer** (stride, format, raw slice) behind a trait used by `gfx` (x86 `FrameBufferInfo` today; Pi `fb.rs`; UEFI GOP).
- [ ] Abstract **time / timers** for TCP, USB scheduling, and UI polling.
- [ ] Abstract **PCI / MMIO discovery** (x86 PCI vs Pi MMIO base vs UEFI protocols).

---

## 2. Networking

| Task | x86 BIOS | x86 UEFI | ARM UEFI | Pi 3 | Pi 4 |
|------|----------|----------|----------|------|------|
| Driver present (VirtIO net / on-board NIC) | VirtIO PCI (QEMU) — extend for bare metal NICs | same as BIOS | need `virt` virtio-mmio or device-tree NIC + driver | USB Ethernet / GENET / etc. — not started | same |
| ARP / IP / TCP / DNS stack wired to NIC | largely in `kernel/` — validate on real hardware | same | port stack + HAL | port stack + HAL | port stack + HAL |
| HTTP / browser fetch path | working in QEMU path — test BIOS vs UEFI boot disks | same | port | port | port |

---

## 3. Keyboard

| Task | x86 BIOS | x86 UEFI | ARM UEFI | Pi 3 | Pi 4 |
|------|----------|----------|----------|------|------|
| PS/2 (i8042) | implemented | same | N/A | N/A | N/A |
| USB HID keyboard (UHCI/OHCI/xHCI) | partial (UHCI/OHCI HID; xHCI incomplete) | same | need xHCI on `virt` or firmware USB | need USB host stack + HID | same |
| Raw input → `UiState` / chrome | implemented on x86 | same | new integration | new integration | new integration |

---

## 4. Pointer / multiple mice

| Task | x86 BIOS | x86 UEFI | ARM UEFI | Pi 3 | Pi 4 |
|------|----------|----------|----------|------|------|
| PS/2 mouse | implemented | same | N/A | N/A | N/A |
| USB HID boot mice (multi) | UHCI/OHCI — see `usb_hid` | same | USB stack + HID | USB stack + HID | USB stack + HID |
| Cursor save-under + emoji presets (`cursor_emoji`, `gfx`) | implemented | same | port with framebuffer HAL | port | port |

---

## 5. HTML / CSS / JS

| Task | x86 BIOS | x86 UEFI | ARM UEFI | Pi 3 | Pi 4 |
|------|----------|----------|----------|------|------|
| HTML subset renderer (`html.rs`) | implemented | same | port with `alloc`/buffers as needed | port | port |
| CSS layout / styling (current subset) | partial — extend deliberately | same | same | same | same |
| **JavaScript** | not executed today (`<script>` stripped per `main.rs` doc) — need engine or strict sandbox policy | same | same | same | same |
| Integration with network (fetch, decode, paint) | QEMU VirtIO path | same | after NIC | after NIC | after NIC |

---

## 6. Full GUI (browser chrome, SYS settings, tabs)

| Task | x86 BIOS | x86 UEFI | ARM UEFI | Pi 3 | Pi 4 |
|------|----------|----------|----------|------|------|
| `gfx.rs` UI + `UiState` | implemented | same | port | port | port |
| Font / blit performance on target | OK in QEMU | test native res | measure GOP | tune for FB | tune for FB |
| Settings persistence (optional) | RAM-only today | same | NVRAM / file on ESP? | SD / flash? | same |

---

## 7. Boot / firmware-specific checks

- [ ] **x86 BIOS vs UEFI:** Re-run full input + net + UI smoke tests on **both** image types (`install/`, `utm/X86-USB-BOOT.md`) — same `kernel` binary, different firmware paths.
- [ ] **ARM UEFI:** Replace minimal `kernel-arm-uefi` loop with either chained full kernel load or merged binary using GOP + polled input + net.
- [ ] **Pi:** Bring up USB controller + HID (or UART-only debug keyboard) before expecting browser UX; document HDMI framebuffer limits vs resolution.

---

## 8. Documentation (when closing gaps)

- [ ] Update `utm/BUILT-IMAGES.md` / `utm/SETUP-ALL-DEVICES.md` with which features work per image.
- [ ] Add per-platform “known good” QEMU or hardware commands for net + USB HID regression.

---

*Last aligned with repo layout: `kernel/` (x86_64), `kernel-rpi/`, `kernel-arm-uefi/`, workspace `eve-os` bootloader images.*
