# Eve OS — phase completion status

Progress against [`action-plan.md`](action-plan.md), derived from the codebase and docs as of the Phase 0–4 work on `main`. Update this file when a phase item lands or exit criteria are verified.

**Legend:** ✅ done · ⚠️ partial · ❌ not started · 🔲 not verified (code exists, smoke/hardware not recorded)

---

## Summary

| Phase | Focus | Done | Bar |
|-------|--------|------|-----|
| **0** | CI, docs, smoke matrix | **80%** | ████████░░ |
| **1** | Browser & network reliability | **70%** | ███████░░░ |
| **2** | x86 input (xHCI / EHCI) | **85%** | ████████░░ |
| **3** | Settings, persist, polish | **90%** | █████████░ |
| **4** | HAL & shared core | **30%** | ███░░░░░░░ |
| **5** | Platform networking | **5%** | ░░░░░░░░░░ |
| **6** | Storage (optional) | **10%** | █░░░░░░░░░ |
| **7** | Web platform (deferred) | **0%** | ░░░░░░░░░░ |

**Overall (Phases 0–4, weighted by plan priority):** ~**71%**

Phases **5–7** are intentionally later; they are not included in that rollup.

---

## Phase 0 — Foundation (80%)

**Exit criteria:** CI exists; docs match code; anyone can run a documented 10-minute x86 smoke test.

| # | Action | Status | Notes |
|---|--------|--------|-------|
| 0.1 | GitHub Actions + `verify-repo.sh` | ✅ | `.github/workflows/verify.yml` |
| 0.2 | Fix `utm/TODO-PLATFORMS.md` narrative | ✅ | Intro + Phase 4 ADR |
| 0.3 | Per-image feature matrix | ⚠️ | `utm/BUILT-IMAGES.md` exists; rows stale (xHCI, persist) |
| 0.4 | Document regression commands | ✅ | `utm/SETUP-ALL-DEVICES.md` smoke section |
| 0.5 | Record x86 BIOS vs UEFI smoke | 🔲 | No test notes / recorded run in repo |

**Blocking exit:** refresh feature matrix; run and log BIOS + UEFI checklist (0.5).

---

## Phase 1 — Browser & network reliability (70%)

**Exit criteria:** Default Shrine + chunked test sites load in QEMU; toolbar matches implementation.

| # | Action | Status | Notes |
|---|--------|--------|-------|
| 1.1 | HTTP chunked body decode | ✅ | `kernel/src/net.rs` |
| 1.2 | Browser history or honest chrome | ✅ | 16-entry stack; `<` / `>` wired in `gfx.rs` |
| 1.3 | Improve TLS time source | ❌ | Build epoch + main-loop ticks only (`eve_tls.rs`) |
| 1.4 | URL / href policy | ⚠️ | `javascript:` / `data:` blocked on `<a>`; not fully documented |
| 1.5 | Static IP octet editing in SYS | ✅ | `gfx.rs` + `settings.rs` |

**Blocking exit:** firmware/ACPI RTC for TLS (1.3); optional CI or logged QEMU fetch smoke.

---

## Phase 2 — x86 input on modern PCs (85%)

**Exit criteria:** Keyboard works on one xHCI-only target (QEMU minimum; hardware preferred).

| # | Action | Status | Notes |
|---|--------|--------|-------|
| 2.1 | xHCI → companion audit | ✅ | `xhci.rs` logging + `backend_label()` |
| 2.2 | Native xHCI HID MVP | ✅ | `xhci_native.rs` — boot keyboard + one mouse |
| 2.3 | EHCI implement or document | ✅ | `ehci.rs` logs companion; explicit no-HID path |
| 2.4 | Real-hardware validation | 🔲 | `REAL-HARDWARE.md` updated; no logged laptop test |

**Blocking exit:** QEMU xHCI-only smoke logged; bare-metal xHCI-only machine (2.4).

---

## Phase 3 — Settings, persistence & polish (90%)

**Exit criteria:** x86 settings survive reboot; no misleading “connected” Wi‑Fi UX.

| # | Action | Status | Notes |
|---|--------|--------|-------|
| 3.1 | x86 settings persistence | ✅ | VirtIO-blk last sector (`settings_persist_disk.rs`); ARM UEFI NVRAM |
| 3.2 | Honest SYS placeholders | ⚠️ | Wi‑Fi **DEMO ONLY**; BT **NOT IMPL**; HDA **DETECT** — demo SSIDs remain |
| 3.3 | AArch64 reboot / shutdown | ✅ | UEFI `ResetSystem` via `power/stub.rs` + `kernel-arm-uefi` |

**Blocking exit:** update `BUILT-IMAGES.md` persist column; optional Wi‑Fi row simplification.

---

## Phase 4 — HAL & shared core (30%)

**Exit criteria (ongoing):** traits adopted; no duplicated `html` / `net` / `gfx` per platform.

| # | Action | Status | Notes |
|---|--------|--------|-------|
| 4.1 | Decide kernel layout (ADR) | ✅ | `utm/TODO-PLATFORMS.md` §1 |
| 4.2 | Framebuffer trait | ⚠️ | `hal/framebuffer.rs`; `gfx` still uses raw slices |
| 4.3 | Timer trait | ⚠️ | `hal/timer.rs`; wired from `NetStack::poll`; USB not yet |
| 4.4 | Bus discovery trait | ⚠️ | `X86PciDiscover` only; no Pi / VirtIO-MMIO backend |

**Follow-ups:** migrate all FB paths to `FramebufferSurface`; `VirtioMmioDiscover`; USB timeout wiring.

---

## Phase 5 — Platform networking (5%)

**Exit criteria (per milestone):** DHCP + HTTP fetch to Shrine on that platform.

| Milestone | Target | Status | Notes |
|-----------|--------|--------|-------|
| 5a | vmxnet3 (VMware / ESXi) | ⚠️ | Scaffolding in `vmxnet3.rs`; TX/RX incomplete |
| 5b | bge / tg3 (Broadcom) | ❌ | Stub only (`bge.rs`) |
| 5c | Pi 4 GENET | ❌ | Not started |
| 5d | Pi USB Ethernet | ❌ | No USB host + MAC driver |

---

## Phase 6 — Storage optional (10%)

| # | Action | Status | Notes |
|---|--------|--------|-------|
| 6.1 | AHCI / boot-from-SATA policy | ❌ | VirtIO-blk install path only on x86 |
| 6.2 | NVMe / USB mass storage | ❌ | Deferred per plan |

**In tree today:** VirtIO-blk two-disk install clone on x86 only.

---

## Phase 7 — Web platform expansion (0%)

Explicitly **deferred** until Phases 1–3 are closed. No scheduled work.

HTTP/2+, QUIC, cookies, full CSS, ECMAScript engine, 802.11 MAC, IPv6, etc. — see [`todo.md`](todo.md).

---

## Doc drift (does not change code %, affects trust)

| Document | Issue |
|----------|--------|
| [`todo.md`](todo.md) | Many Phase 0–3 items still unchecked though implemented |
| [`WhatsWorking.md`](WhatsWorking.md) | xHCI, chunked HTTP, disk persist, history not reflected |
| [`utm/BUILT-IMAGES.md`](utm/BUILT-IMAGES.md) | Matrix still says xHCI companion-only, x86 RAM-only persist |

Refreshing these is part of closing **Phase 0** exit criteria.

---

## Related documents

| Document | Role |
|----------|------|
| [`action-plan.md`](action-plan.md) | Sequenced plan and exit criteria |
| [`todo.md`](todo.md) | Full issue backlog |
| [`WhatsWorking.md`](WhatsWorking.md) | Feature snapshot (needs sync) |
| [`utm/TODO-PLATFORMS.md`](utm/TODO-PLATFORMS.md) | Platform parity + HAL ADR |

---

*When a phase item ships, update the table above and the summary bar. Prefer verifying exit criteria in QEMU or hardware before marking a phase 100%.*
