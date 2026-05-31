// SPDX-License-Identifier: MIT OR Apache-2.0
//
//! Eve — TempleOS-inspired ring-0 guest (x86_64): browser chrome, VirtIO user-NAT HTTP, SYS prefs.
//! **Browser:** HTTP body is rendered by an **HTML5-oriented / CSS subset** engine (`html.rs`).
//! **React / client JS bundles / full CSS3** are **not** supported (no JS VM, no flex/grid engine).
//! Use **static HTML** or **SSR** for pages meant to load in Eve; see **`utm/BROWSER-LIMITS.md`**.
//! **JSON-LD / JSON `type=`** scripts are skipped inertly; **`eve-script:`** bytecode may run when
//! **SYS → BROWSER SCRIPT VM** is on.
//!
//! # Device drivers actually in this tree (x86_64)
//!
//! - **Keyboard / mouse (implemented):** PS/2 (i8042, 3- and 4-byte ImPS/2 mouse packets); USB HID boot keyboard + up to 12 boot mice via **UHCI** (I/O), **OHCI** (MMIO), or **native xHCI** (`xhci_native`) when no companion exists. With **USB poll** on, USB mice use cursors **0..N−1** and the **PS/2** mouse uses cursor **N** (when **N < 12**) so all pointers work together — see `ps2.rs`, `uhci.rs`, `ohci.rs`, `xhci_native.rs`, `usb_hid.rs`.
//! - **Keyboard / mouse (partial):** **EHCI** companion HID is not implemented (`ehci.rs`); xHCI on real laptops may still need hardware validation.
//! - **Networking (implemented):** **VirtIO net**, **Realtek RTL8139**, **RTL8168/8169** (MMIO C+), **Intel e1000 / e1000e-class PCI IDs**, **AMD PCnet** (QEMU `pcnet`) — ARP, DNS, TCP, HTTP/1.0 — SYS **IP MODE**: SLIRP (`10.0.2.x`), **DHCP**, or **static** — see `virtio_net.rs`, `rtl8139.rs`, `rtl8168.rs`, `e1000.rs`, `pcnet.rs`, `nic.rs`, `net.rs`, `net_ipv4.rs`, `url.rs`.
//! - **Disk install (QEMU / VirtIO):** With **two** `virtio-blk` PCI disks, the **INSTALL** tab clones disk 1 → disk 2 sector-by-sector, then sets **GPT ESP boot attributes** (or **MBR active** on partition 1) on the target — see `gpt_boot_patch.rs`, `virtio_blk.rs`, `install/pc-x86-64-disk-install/`.
//! - **Browser boot:** A **photosensitivity / epilepsy** notice, then a **California age** attestation, then the main UI (**Enter** / **Space** / **Continue** on each). With networking, the default URL is **`https://alexanderdfox.github.io/TempleOSWebShrine/`**, fetched after that; the UI starts in **BIOS-style full page** (no title bar / tabs / URL strip / status) until **F6** restores chrome — see `gfx.rs`.
//! - **Networking (partial):** **vmxnet3** has attach/MAC/ring scaffolding; **Broadcom bge** remains
//!   limited; **802.11** (SSID/PSK in SYS are UI-only — no WPA/802.11 MAC; see `utm/WIFI-80211.md`);
//!   IPv6 not implemented.
//! - **TLS:** `https://` uses TLS 1.3 with in-tree verified provider/trust anchors (`eve_tls.rs`).
//! - **Bluetooth (not implemented):** SYS toggle is a placeholder — no HCI or stack.
//!
//! **QEMU / UTM / PC:** same guest code; **USB poll** in SYS enables UHCI/OHCI multi-mice plus PS/2 on a separate cursor slot.
//! **Raspberry Pi** (`kernel-rpi/`): runs shared `arm_run` UI loop over UART/framebuffer with serial
//! keyboard/mouse input parsing.
//!
//! # Real PC (bare metal x86_64)
//!
//! - **Boot:** use `utm/eve-bios.img` or `utm/eve-uefi.img` on a USB stick — see `install/REAL-HARDWARE.md`
//!   and `utm/X86-USB-BOOT.md`.
//! - **Display:** bootloader-provided framebuffer (GOP / VESA) when firmware allows.
//! - **Input:** PS/2 and **UHCI USB HID** only; most laptops are **xHCI-only** (no built-in keyboard driver yet).
//! - **Network:** **VirtIO**, **RTL8139**, **RTL8168/8169**, **e1000/e1000e-class**, or **PCnet**; TCP/IP uses SYS **IP MODE** (SLIRP / DHCP / static). Bare metal
//!   shows **NET: NODRV** without a supported PCI Ethernet device — see `install/REAL-HARDWARE.md`.
//!
//! “Add all drivers” is not a single feature: pick one concrete next target (e.g. DHCP for LAN).

#![no_std]
#![no_main]

use core::mem::MaybeUninit;

use bootloader_api::config::Mapping;
use bootloader_api::{entry_point, BootInfo};
use kernel::gfx::{CursorEngine, SettingsTextFocus, UiState, MAX_CURSORS};
use kernel::hal::{BusDiscover, X86PciDiscover};
use kernel::net::{NetPhase, NetStack};
use kernel::ps2::{scancode_set1_to_ascii, Ps2Event};
use kernel::settings::{DiskInstallPhase, NicChoice, PlatformCaps, Screen};
use kernel::{diag_log, font, gfx, html, log_buffer, nic, pci, power, ps2, serial, usb_hid, virtio_blk};

/// `NetStack` is ~130 KiB (TLS/plaintext buffers). Initialized in `.bss` via `NetStack::static_initial`
/// so boot does not spill that struct on the kernel stack (triple fault → firmware reboot loop).
#[allow(static_mut_refs)]
static mut NET_STACK: NetStack = NetStack::static_initial();

/// `CursorEngine` save-unders live in `.bss` via `CursorEngine::static_initial`. `UiState` is still
/// `MaybeUninit` because it depends on framebuffer dimensions at runtime.
#[allow(static_mut_refs)]
static mut UI_STATE: MaybeUninit<UiState> = MaybeUninit::uninit();
#[allow(static_mut_refs)]
static mut CURSOR_ENG: CursorEngine = CursorEngine::static_initial();
static mut INET_SCRATCH: [u8; 2048] = [0u8; 2048];
#[allow(static_mut_refs)]
static mut DISK_SRC: MaybeUninit<kernel::virtio_blk::VirtioBlk> = MaybeUninit::uninit();
#[allow(static_mut_refs)]
static mut DISK_DST: MaybeUninit<kernel::virtio_blk::VirtioBlk> = MaybeUninit::uninit();
#[allow(static_mut_refs)]
static mut SETTINGS_BLK: MaybeUninit<kernel::virtio_blk::VirtioBlk> = MaybeUninit::uninit();
static mut SETTINGS_ON_DISK_SRC: bool = false;
static mut SETTINGS_BLK_READY: bool = false;
static mut INSTALL_SECTOR_BUF: [u8; 512] = [0u8; 512];

fn browser_scroll(state: &mut UiState, lay: &gfx::Layout, lines: i32) {
    if lines == 0 {
        return;
    }
    if lines < 0 {
        let d = (-lines) as usize;
        state.page_scroll_line = state.page_scroll_line.saturating_sub(d);
    } else {
        state.page_scroll_line = state.page_scroll_line.saturating_add(lines as usize);
    }
    gfx::browser_clamp_scroll(lay, state);
    state.browser_body_dirty = true;
}

/// Typing into SYS Wi‑Fi SSID / PSK when a field is focused.
fn settings_text_key(state: &mut UiState, ch: u8) -> bool {
    match state.settings_text_focus {
        SettingsTextFocus::None => return false,
        SettingsTextFocus::WifiSsid => match ch {
            0x08 => {
                if state.settings.wifi_ssid_len > 0 {
                    state.settings.wifi_ssid_len -= 1;
                }
            }
            c if c >= 32 && c < 127 => {
                if state.settings.wifi_ssid_len < state.settings.wifi_ssid.len() {
                    state.settings.wifi_ssid[state.settings.wifi_ssid_len] = c;
                    state.settings.wifi_ssid_len += 1;
                }
            }
            _ => return false,
        },
        SettingsTextFocus::WifiPsk => match ch {
            0x08 => {
                if state.settings.wifi_psk_len > 0 {
                    state.settings.wifi_psk_len -= 1;
                }
            }
            c if c >= 32 && c < 127 => {
                if state.settings.wifi_psk_len < state.settings.wifi_psk.len() {
                    state.settings.wifi_psk[state.settings.wifi_psk_len] = c;
                    state.settings.wifi_psk_len += 1;
                }
            }
            _ => return false,
        },
        SettingsTextFocus::DisplayWidth => match ch {
            0x08 => {
                state.settings.display_pref_width /= 10;
            }
            c @ b'0'..=b'9' => {
                let d = u16::from(c - b'0');
                state.settings.display_pref_width = state
                    .settings
                    .display_pref_width
                    .saturating_mul(10)
                    .saturating_add(d)
                    .min(7680);
            }
            _ => return false,
        },
        SettingsTextFocus::DisplayHeight => match ch {
            0x08 => {
                state.settings.display_pref_height /= 10;
            }
            c @ b'0'..=b'9' => {
                let d = u16::from(c - b'0');
                state.settings.display_pref_height = state
                    .settings
                    .display_pref_height
                    .saturating_mul(10)
                    .saturating_add(d)
                    .min(4320);
            }
            _ => return false,
        },
        SettingsTextFocus::HomeUrl => match ch {
            0x08 => {
                let n = state.settings.home_url_len as usize;
                if n > 0 {
                    state.settings.home_url_len = (n - 1) as u8;
                }
            }
            c if c >= 32 && c < 127 => {
                let n = state.settings.home_url_len as usize;
                if n < kernel::settings::PERSIST_HOME_URL_CAP {
                    state.settings.home_url[n] = c;
                    state.settings.home_url_len = (n + 1) as u8;
                }
            }
            _ => return false,
        },
        SettingsTextFocus::StaticIp | SettingsTextFocus::StaticGw | SettingsTextFocus::StaticDns => {
            let Some(octets) =
                UiState::static_ip_octets_mut(state.settings_text_focus, &mut state.settings)
            else {
                return false;
            };
            let sel = state.static_octet_sel.min(3) as usize;
            match ch {
                0x08 => {
                    octets[sel] /= 10;
                }
                c @ b'0'..=b'9' => {
                    let d = c - b'0';
                    octets[sel] = octets[sel]
                        .saturating_mul(10)
                        .saturating_add(d)
                        .min(255);
                }
                _ => return false,
            }
        }
    }
    true
}

fn start_browser_fetch(inet: &mut NetStack, state: &mut UiState, inet_on: bool) {
    if state.history_skip_push {
        state.history_skip_push = false;
    } else {
        state.history_push_current();
    }
    if state.url_len == 0 || !inet_on {
        if state.url_len == 0 {
            return;
        }
        let hl = state.settings.home_url_len as usize;
        if hl > 0
            && state.url_len == hl
            && state.url[..hl] == state.settings.home_url[..hl]
        {
            let fallback = b"<!doctype html><html><body><h1>TempleOS Web Shrine (offline)</h1><p>&#128512; Offline mode</p><p>Network is not active yet.</p><p>When online, the default home is <code>https://alexanderdfox.github.io/TempleOSWebShrine/</code></p><p>Local QEMU demo: host <code>python3 -m http.server 8080 --directory demo/qemu-http-test</code> then <code>http://10.0.2.2:8080/</code></p></body></html>";
            let mut html_trunc = false;
            let mut scripts = false;
            html::format_document(
                fallback,
                &mut state.browser_line_count,
                &mut html_trunc,
                &mut scripts,
                state.settings.ui_palette().text_primary.tuple(),
            );
            state.fetch_err_len = 0;
            state.page_truncated = html_trunc;
            state.last_rendered_raw_len = fallback.len();
            state.page_scroll_line = 0;
            state.browser_body_dirty = true;
            state.status_dirty = true;
            return;
        }
        let msg = b"NET OFFLINE";
        let n = msg.len().min(state.fetch_err.len());
        state.fetch_err[..n].copy_from_slice(&msg[..n]);
        state.fetch_err_len = n;
        state.browser_line_count = 0;
        state.page_truncated = false;
        state.page_scroll_line = 0;
        state.browser_body_dirty = true;
        state.status_dirty = true;
        return;
    }
    inet.start_fetch(&state.url[..state.url_len]);
    state.browser_line_count = 0;
    state.last_rendered_raw_len = usize::MAX;
    state.page_scroll_line = 0;
    state.browser_body_dirty = true;
    state.status_dirty = true;
}

pub static BOOTLOADER_CONFIG: bootloader_api::BootloaderConfig = {
    let mut c = bootloader_api::BootloaderConfig::new_default();
    c.mappings.physical_memory = Some(Mapping::Dynamic);
    // Default is 80 KiB. Large static structs + TLS/network call depth need margin (guard page → reboot).
    c.kernel_stack_size = 1024 * 1024;
    c
};

entry_point!(kernel_main, config = &BOOTLOADER_CONFIG);

fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
    let phys_skew: u64 = boot_info.physical_memory_offset.into_option().unwrap_or(0);
    unsafe {
        serial::init();
        ps2::init();
        usb_hid::init(phys_skew);
    }

    let mut wlan_pci = [pci::PciFnId::default(); 8];
    let nwlan = unsafe { pci::enumerate_pci_class(0x02, 0x80, &mut wlan_pci) };
    let pci_wlan = nwlan > 0;
    let wlan_pci_count = nwlan.min(255) as u8;
    let wlan_first_vid = if nwlan > 0 {
        wlan_pci[0].vendor_id
    } else {
        0u16
    };
    let wlan_first_did = if nwlan > 0 {
        wlan_pci[0].device_id
    } else {
        0u16
    };
    let mut brcm_wlan_count: u8 = 0;
    let mut brcm_first_did: u16 = 0;
    for f in wlan_pci.iter().take(nwlan) {
        if f.vendor_id == 0x14E4 {
            if brcm_wlan_count == 0 {
                brcm_first_did = f.device_id;
            }
            brcm_wlan_count = brcm_wlan_count.saturating_add(1);
        }
    }
    let pci_eth;
    let pci_mm_audio;
    {
        let snap = X86PciDiscover.snapshot();
        pci_eth = snap.ethernet_count;
        pci_mm_audio = snap.mm_audio_present;
        diag_log::line2(b"bus ", b"PCI");
    }

    let mut net = unsafe { nic::AnyNic::probe(boot_info) };
    #[allow(static_mut_refs)]
    let inet = unsafe { &mut NET_STACK };
    if let Some(ref n) = net {
        diag_log::line2(b"nic ", n.driver_tag());
        diag_log::mac(n.mac());
        inet.seed_from_mac(n.mac());
    } else {
        diag_log::line(b"nic none");
    }

    let mut bfds = [(0u8, 0u8, 0u8); 8];
    let n_blk = unsafe { virtio_blk::enumerate(&mut bfds) };
    let mut boot_settings = kernel::settings::DeviceSettings::new();
    let mut settings_persist_supported = false;
    let disk_pair_ready = unsafe {
        if n_blk >= 2 {
            if let Some(mut src) = virtio_blk::VirtioBlk::init(
                bfds[0].0,
                bfds[0].1,
                bfds[0].2,
                boot_info,
                0,
                false,
            ) {
                if src.capacity >= 1 {
                    let _ = kernel::settings_persist_disk::load(&mut src, &mut boot_settings);
                    settings_persist_supported = true;
                    SETTINGS_ON_DISK_SRC = true;
                }
                if let Some(dst) = virtio_blk::VirtioBlk::init(
                    bfds[1].0,
                    bfds[1].1,
                    bfds[1].2,
                    boot_info,
                    1,
                    true,
                ) {
                    #[allow(static_mut_refs)]
                    {
                        DISK_SRC.write(src);
                        DISK_DST.write(dst);
                    }
                    true
                } else {
                    false
                }
            } else {
                false
            }
        } else if n_blk >= 1 {
            if let Some(mut disk) = virtio_blk::VirtioBlk::init(
                bfds[0].0,
                bfds[0].1,
                bfds[0].2,
                boot_info,
                0,
                false,
            ) {
                if disk.capacity >= 1 {
                    let _ = kernel::settings_persist_disk::load(&mut disk, &mut boot_settings);
                    SETTINGS_BLK.write(disk);
                    SETTINGS_BLK_READY = true;
                    settings_persist_supported = true;
                }
            }
            false
        } else {
            false
        }
    };

    let platform_caps = PlatformCaps::x86_persist(settings_persist_supported);
    diag_log::line2(b"caps input ", platform_caps.input_backend.label());
    diag_log::line2(b"caps usb ", platform_caps.usb_parity.label());
    diag_log::line2(b"caps wifi ", platform_caps.wifi_mode_label());
    diag_log::line2(b"caps net ", platform_caps.net_mode_label());
    diag_log::line2(b"caps save ", platform_caps.persist_label());

    if let Some(framebuffer) = boot_info.framebuffer.as_mut() {
        let info = framebuffer.info();
        diag_log::fb_wh(info.width as u32, info.height as u32);
        let buf = framebuffer.buffer_mut();
        let state = {
            #[allow(static_mut_refs)]
            unsafe {
                UI_STATE.write(UiState::new_with_settings(
                    info.width as i32,
                    info.height as i32,
                    pci_wlan,
                    wlan_pci_count,
                    wlan_first_vid,
                    wlan_first_did,
                    brcm_wlan_count,
                    brcm_first_did,
                    pci_eth,
                    pci_mm_audio,
                    platform_caps,
                    boot_settings,
                ));
                let s = UI_STATE.assume_init_mut();
                if net.is_some() {
                    s.net_ok = true;
                    if let Some(ref n) = net {
                        s.mac = *n.mac();
                    }
                }
                if disk_pair_ready {
                    s.disk_install_available = true;
                    s.disk_install_total = DISK_SRC
                        .assume_init_ref()
                        .capacity
                        .min(DISK_DST.assume_init_ref().capacity);
                    s.screen_after_epilepsy_notice = Screen::DiskInstall;
                }
                s
            }
        };

        #[allow(static_mut_refs)]
        let cursor_eng = unsafe { &mut CURSOR_ENG };
        let mut last_rx_drawn: u64 = state.net_rx;
        let mut last_inet_phase = state.inet_phase;
        let mut last_inet_bytes = state.inet_bytes;
        let mut last_net_ipv4 = state.net_ipv4;
        let mut boot_home_fetch_pending = true;

        loop {
            let inet_on = net.is_some()
                && state.settings.nic != NicChoice::Off
                && state.settings.internet_stack_enabled;
            if boot_home_fetch_pending && state.screen == Screen::Browser && state.url_len > 0 {
                boot_home_fetch_pending = false;
                start_browser_fetch(inet, state, inet_on);
            }
            unsafe {
                let n_usb = if state.settings.usb_polling_enabled && usb_hid::usb_mouse_active() {
                    usb_hid::usb_mouse_count().min(MAX_CURSORS)
                } else {
                    0
                };
                let ps2_mouse_slot = if n_usb > 0 {
                    if n_usb < MAX_CURSORS {
                        n_usb
                    } else {
                        usize::MAX
                    }
                } else {
                    0
                };

                if n_usb > 0 {
                    for i in 0..MAX_CURSORS {
                        state.cursor_active[i] = false;
                    }
                    for i in 0..n_usb {
                        state.cursor_active[i] = true;
                    }
                    if ps2_mouse_slot != usize::MAX {
                        state.cursor_active[ps2_mouse_slot] = true;
                    }
                } else {
                    state.cursor_active[0] = true;
                    for i in 1..MAX_CURSORS {
                        state.cursor_active[i] = false;
                    }
                }

                while let Some(ev) = ps2::poll_event() {
                    match ev {
                        Ps2Event::BrowserScroll { lines } => {
                            let lay = state.layout(&info);
                            if state.screen == Screen::Browser {
                                let n = gfx::browser_wheel_lines(&lay, state, lines);
                                browser_scroll(state, &lay, n);
                            } else if state.screen == Screen::Log {
                                gfx::log_scroll_by_wheel(state, &lay, lines);
                            } else if state.screen == Screen::Settings {
                                gfx::settings_scroll_by_wheel(state, &lay, lines.saturating_mul(36));
                            } else if state.screen == Screen::DiskInstall {
                                gfx::disk_install_scroll_by_wheel(state, &lay, lines.saturating_mul(32));
                            }
                        }
                        Ps2Event::Key { code, shift } => {
                            if usb_hid::usb_ps2_kbd_should_ignore()
                                && state.settings.usb_polling_enabled
                            {
                                continue;
                            }
                            if state.screen == Screen::EpilepsyWarning {
                                if let Some(ch) = scancode_set1_to_ascii(code, shift) {
                                    if ch == b'\n' || ch == b' ' {
                                        gfx::dismiss_epilepsy_notice(state);
                                        state.content_dirty = true;
                                    }
                                }
                                continue;
                            }
                            if state.screen == Screen::CaliforniaAgeNotice {
                                if let Some(ch) = scancode_set1_to_ascii(code, shift) {
                                    if ch == b'\n' || ch == b' ' {
                                        gfx::dismiss_california_age_notice(state);
                                        state.content_dirty = true;
                                    }
                                }
                                continue;
                            }
                            match code {
                                0x3B => {
                                    state.screen = Screen::Settings;
                                    state.content_dirty = true;
                                    continue;
                                }
                                0x3C => {
                                    state.screen = Screen::Browser;
                                    state.settings_text_focus = SettingsTextFocus::None;
                                    state.content_dirty = true;
                                    continue;
                                }
                                0x3D => {
                                    if state.screen == Screen::Settings {
                                        state.settings = state.settings.toggle_midi_channel();
                                        state.content_dirty = true;
                                    }
                                    continue;
                                }
                                0x3E if disk_pair_ready => {
                                    state.screen = Screen::DiskInstall;
                                    state.content_dirty = true;
                                    continue;
                                }
                                0x3F => {
                                    state.screen = Screen::Log;
                                    state.content_dirty = true;
                                    continue;
                                }
                                0x40 if state.screen == Screen::Browser => {
                                    state.bios_fullpage_browser = !state.bios_fullpage_browser;
                                    state.content_dirty = true;
                                    continue;
                                }
                                0x40 if state.screen == Screen::Log => {
                                    state.log_subtab = match state.log_subtab {
                                        gfx::LogSubtab::Live => gfx::LogSubtab::Serial,
                                        gfx::LogSubtab::Serial => gfx::LogSubtab::Live,
                                    };
                                    state.log_scroll_line = 0;
                                    state.log_stick_to_bottom = true;
                                    state.content_dirty = true;
                                    continue;
                                }
                                _ => {}
                            }

                            if state.screen == Screen::Browser {
                                if let Some(ch) = scancode_set1_to_ascii(code, shift) {
                                    match ch {
                                        0x08 => {
                                            if state.url_len > 0 {
                                                state.url_len -= 1;
                                                state.chrome_only_dirty = true;
                                            }
                                        }
                                        b'\n' => {
                                            start_browser_fetch(inet, state, inet_on);
                                        }
                                        c if state.url_len < state.url.len() - 1 => {
                                            if c >= 32 && c < 127 {
                                                state.url[state.url_len] = c;
                                                state.url_len += 1;
                                                state.chrome_only_dirty = true;
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            } else if state.screen == Screen::Settings {
                                if let Some(ch) = scancode_set1_to_ascii(code, shift) {
                                    if settings_text_key(state, ch) {
                                        state.content_dirty = true;
                                    }
                                }
                            }
                        }
                        Ps2Event::Mouse { buttons, dx, dy } => {
                            if ps2_mouse_slot == usize::MAX {
                                continue;
                            }
                            let s = ps2_mouse_slot;
                            state.cursor_btn[s] = buttons;
                            state.cursor_x[s] += i32::from(dx);
                            state.cursor_y[s] += i32::from(dy);
                            state.cursor_x[s] =
                                state.cursor_x[s].clamp(0, info.width as i32 - 1);
                            state.cursor_y[s] =
                                state.cursor_y[s].clamp(0, info.height as i32 - 1);
                        }
                    }
                }

                if state.settings.usb_polling_enabled {
                    // Poll every enumerated HID mouse so we can set `HID_MOUSE_XFER_OK` and recover
                    // after stalls; `n_usb` / `usb_mouse_active()` only gate PS/2 vs USB routing.
                    let n_poll_mice = usb_hid::usb_mouse_count().min(MAX_CURSORS);
                    if n_poll_mice > 0 {
                        for i in 0..n_poll_mice {
                            let polled = usb_hid::poll_hid_slot(i);
                            if n_usb > 0 {
                                if let Some((btn, dx, dy)) = polled {
                                    state.cursor_btn[i] = btn;
                                    state.cursor_x[i] += i32::from(dx);
                                    state.cursor_y[i] += i32::from(dy);
                                    state.cursor_x[i] =
                                        state.cursor_x[i].clamp(0, info.width as i32 - 1);
                                    state.cursor_y[i] =
                                        state.cursor_y[i].clamp(0, info.height as i32 - 1);
                                }
                            }
                        }
                    }
                    if usb_hid::usb_keyboard_active() {
                        while let Some((usage, shift)) = usb_hid::poll_usb_key_press() {
                            if state.screen == Screen::EpilepsyWarning {
                                if let Some(ch) = usb_hid::hid_usage_to_ascii(usage, shift) {
                                    if ch == b'\n' || ch == b' ' {
                                        gfx::dismiss_epilepsy_notice(state);
                                        state.content_dirty = true;
                                    }
                                }
                                continue;
                            }
                            if state.screen == Screen::CaliforniaAgeNotice {
                                if let Some(ch) = usb_hid::hid_usage_to_ascii(usage, shift) {
                                    if ch == b'\n' || ch == b' ' {
                                        gfx::dismiss_california_age_notice(state);
                                        state.content_dirty = true;
                                    }
                                }
                                continue;
                            }
                            match usage {
                                0x3A => {
                                    state.screen = Screen::Settings;
                                    state.content_dirty = true;
                                }
                                0x3B => {
                                    state.screen = Screen::Browser;
                                    state.settings_text_focus = SettingsTextFocus::None;
                                    state.content_dirty = true;
                                }
                                0x3C => {
                                    if state.screen == Screen::Settings {
                                        state.settings = state.settings.toggle_midi_channel();
                                        state.content_dirty = true;
                                    }
                                }
                                0x3D if disk_pair_ready => {
                                    state.screen = Screen::DiskInstall;
                                    state.content_dirty = true;
                                }
                                0x3E => {
                                    state.screen = Screen::Log;
                                    state.content_dirty = true;
                                }
                                0x3F if state.screen == Screen::Browser => {
                                    state.bios_fullpage_browser = !state.bios_fullpage_browser;
                                    state.content_dirty = true;
                                }
                                0x51 => {
                                    let lay = state.layout(&info);
                                    if state.screen == Screen::Browser {
                                        let pg = gfx::browser_scroll_slots(&lay, state) as i32;
                                        browser_scroll(state, &lay, pg);
                                    } else if state.screen == Screen::Log {
                                        gfx::log_scroll_by_wheel(state, &lay, 3);
                                    } else if state.screen == Screen::Settings {
                                        gfx::settings_scroll_by_wheel(state, &lay, 120);
                                    } else if state.screen == Screen::DiskInstall {
                                        gfx::disk_install_scroll_by_wheel(state, &lay, 96);
                                    }
                                }
                                0x52 => {
                                    let lay = state.layout(&info);
                                    if state.screen == Screen::Browser {
                                        let pg = -(gfx::browser_scroll_slots(&lay, state) as i32);
                                        browser_scroll(state, &lay, pg);
                                    } else if state.screen == Screen::Log {
                                        gfx::log_scroll_by_wheel(state, &lay, -3);
                                    } else if state.screen == Screen::Settings {
                                        gfx::settings_scroll_by_wheel(state, &lay, -120);
                                    } else if state.screen == Screen::DiskInstall {
                                        gfx::disk_install_scroll_by_wheel(state, &lay, -96);
                                    }
                                }
                                _ => {
                                    if state.screen == Screen::Browser {
                                        if let Some(ch) = usb_hid::hid_usage_to_ascii(usage, shift)
                                        {
                                            match ch {
                                                0x08 => {
                                                    if state.url_len > 0 {
                                                        state.url_len -= 1;
                                                        state.chrome_only_dirty = true;
                                                    }
                                                }
                                                b'\n' => {
                                                    start_browser_fetch(
                                                        inet,
                                                        state,
                                                        inet_on,
                                                    );
                                                }
                                                c if state.url_len < state.url.len() - 1 => {
                                                    if c >= 32 && c < 127 {
                                                        state.url[state.url_len] = c;
                                                        state.url_len += 1;
                                                        state.chrome_only_dirty = true;
                                                    }
                                                }
                                                _ => {}
                                            }
                                        }
                                    } else if state.screen == Screen::Settings {
                                        if let Some(ch) = usb_hid::hid_usage_to_ascii(usage, shift)
                                        {
                                            if settings_text_key(state, ch) {
                                                state.content_dirty = true;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                for i in 0..MAX_CURSORS {
                    if !state.cursor_active[i] {
                        state.cursor_btn[i] = 0;
                    }
                }
                state.mx = state.cursor_x[0];
                state.my = state.cursor_y[0];

                let mut click_dirty = false;
                for i in 0..MAX_CURSORS {
                    if !state.cursor_active[i] {
                        continue;
                    }
                    let left_now = state.cursor_btn[i] & 1;
                    let left_prev = state.prev_cursor_btn[i] & 1;
                    if left_now != 0 && left_prev == 0 {
                        let cx = state.cursor_x[i].clamp(0, info.width as i32 - 1) as usize;
                        let cy = state.cursor_y[i].clamp(0, info.height as i32 - 1) as usize;
                        if gfx::handle_click_at(state, &info, cx, cy) {
                            click_dirty = true;
                        }
                    }
                }
                if click_dirty {
                    state.content_dirty = true;
                }
                for i in 0..MAX_CURSORS {
                    state.prev_cursor_btn[i] = state.cursor_btn[i];
                }

                if state.power_reboot_request {
                    state.power_reboot_request = false;
                    if state.screen == Screen::Settings {
                        diag_log::line(b"power reboot");
                        power::system_reboot();
                        power::halt_forever();
                    } else {
                        diag_log::line(b"power reboot ignored (not in SYS)");
                    }
                }
                if state.power_shutdown_request {
                    state.power_shutdown_request = false;
                    if state.screen == Screen::Settings {
                        diag_log::line(b"power shutdown");
                        power::system_shutdown();
                        power::halt_forever();
                    } else {
                        diag_log::line(b"power shutdown ignored (not in SYS)");
                    }
                }

                if state.inet_reload_request {
                    state.inet_reload_request = false;
                    if state.url_len > 0 {
                        start_browser_fetch(inet, state, inet_on);
                    } else {
                        inet.reset_demo();
                        if let Some(ref n) = net {
                            inet.seed_from_mac(n.mac());
                        }
                        state.browser_line_count = 0;
                        state.last_rendered_raw_len = usize::MAX;
                        state.last_inet_page_gen = 0;
                        state.fetch_err_len = 0;
                        state.page_truncated = false;
                        state.browser_body_dirty = true;
                    }
                    state.status_dirty = true;
                }

                if disk_pair_ready {
                    if state.disk_install_start_request {
                        state.disk_install_start_request = false;
                        if state.disk_install_phase == DiskInstallPhase::Idle {
                            #[allow(static_mut_refs)]
                            {
                                let sr = DISK_SRC.assume_init_ref();
                                let dr = DISK_DST.assume_init_ref();
                                if sr.sector_size != 512 || dr.sector_size != 512 {
                                    let msg = b"NEED 512 BYTE SECTORS";
                                    let n = msg.len().min(state.disk_install_err.len());
                                    state.disk_install_err[..n].copy_from_slice(&msg[..n]);
                                    state.disk_install_err_len = n;
                                    state.disk_install_phase = DiskInstallPhase::Failed;
                                } else {
                                    let t = sr.capacity.min(dr.capacity);
                                    if t == 0 {
                                        let msg = b"ZERO CAPACITY";
                                        let n = msg.len().min(state.disk_install_err.len());
                                        state.disk_install_err[..n].copy_from_slice(&msg[..n]);
                                        state.disk_install_err_len = n;
                                        state.disk_install_phase = DiskInstallPhase::Failed;
                                    } else {
                                        state.disk_install_phase = DiskInstallPhase::Running;
                                        state.disk_install_cur = 0;
                                        state.disk_install_total = t;
                                    }
                                }
                            }
                            state.content_dirty = true;
                        }
                    }

                    if state.disk_install_phase == DiskInstallPhase::Running {
                        const CHUNK: u64 = 32;
                        let mut io_fail = false;
                        #[allow(static_mut_refs)]
                        {
                            let s = DISK_SRC.assume_init_mut();
                            let d = DISK_DST.assume_init_mut();
                            let sb = &mut INSTALL_SECTOR_BUF[..];
                            for _ in 0..CHUNK {
                                if state.disk_install_cur >= state.disk_install_total {
                                    #[allow(static_mut_refs)]
                                    {
                                        let d = DISK_DST.assume_init_mut();
                                        let _ = kernel::gpt_boot_patch::patch_install_target_boot(d);
                                    }
                                    state.disk_install_phase = DiskInstallPhase::Done;
                                    break;
                                }
                                if !s.read_sector(state.disk_install_cur, sb)
                                    || !d.write_sector(state.disk_install_cur, sb)
                                {
                                    io_fail = true;
                                    break;
                                }
                                state.disk_install_cur += 1;
                            }
                        }
                        if io_fail {
                            let msg = b"DISK READ OR WRITE FAILED";
                            let n = msg.len().min(state.disk_install_err.len());
                            state.disk_install_err[..n].copy_from_slice(&msg[..n]);
                            state.disk_install_err_len = n;
                            state.disk_install_phase = DiskInstallPhase::Failed;
                        }
                        state.content_dirty = true;
                    }
                }

                // QEMU `-netdev user` is always “linked” for the guest; no Wi‑Fi PHY.
                if inet_on {
                    if let Some(ref mut n) = net {
                        #[allow(static_mut_refs)]
                        inet.drive(n, &state.mac, &mut INET_SCRATCH[..], &state.settings);
                        state.net_ipv4 = inet.addrs.our;
                        state.inet_phase = inet.phase;
                        state.inet_bytes = inet.http_bytes;
                        let pl = inet.page_len.min(inet.page.len());
                        // Do not run the HTML renderer on a partial TCP/TLS body: `html::format_document`
                        // bails out inside `<script>` / `<style>` / … when the closing tag is not in the
                        // buffer yet, which yields zero lines (blank page) until the full response
                        // arrives. Wait until `NetPhase::Done` (or an error, which also ends the phase).
                        let fetch_settled = matches!(inet.phase, NetPhase::Done);
                        if fetch_settled
                            && (pl != state.last_rendered_raw_len
                                || inet.fetch_err_len != state.fetch_err_len
                                || inet.page_truncated != state.page_truncated
                                || inet.page_gen != state.last_inet_page_gen)
                        {
                            state.last_rendered_raw_len = pl;
                            state.last_inet_page_gen = inet.page_gen;
                            let fe = inet.fetch_err_len.min(state.fetch_err.len());
                            state.fetch_err[..fe].copy_from_slice(&inet.fetch_err[..fe]);
                            state.fetch_err_len = fe;
                            if inet.fetch_err_len > 0 {
                                state.browser_line_count = 0;
                                state.page_truncated = false;
                            } else {
                                let mut html_trunc = false;
                                let mut scripts = false;
                                html::format_document(
                                    &inet.page[..pl],
                                    &mut state.browser_line_count,
                                    &mut html_trunc,
                                    &mut scripts,
                                    state.settings.ui_palette().text_primary.tuple(),
                                );
                                if let Some(res) = kernel::script_runtime::run_page_eve_script(
                                    &inet.page[..pl],
                                    state.settings.browser_script_runtime_enabled,
                                ) {
                                    match res {
                                        Ok(_) => diag_log::line(b"script eve ok"),
                                        Err(_) => diag_log::line(b"script eve err"),
                                    }
                                }
                                state.page_truncated = inet.page_truncated || html_trunc;
                            }
                            state.browser_body_dirty = true;
                        }
                    }
                } else {
                    state.net_ipv4 = [0, 0, 0, 0];
                    state.inet_phase = NetPhase::Off;
                    state.inet_bytes = 0;
                }

                if let Some(ref n) = net {
                    if state.settings.nic != NicChoice::Off {
                        state.net_rx = n.rx_packets();
                        state.mac = *n.mac();
                    }
                }
                state.net_ok = net.is_some() && state.settings.nic != NicChoice::Off;
                if state.net_rx != last_rx_drawn
                    || state.inet_phase != last_inet_phase
                    || state.inet_bytes != last_inet_bytes
                    || state.net_ipv4 != last_net_ipv4
                {
                    last_rx_drawn = state.net_rx;
                    last_inet_phase = state.inet_phase;
                    last_inet_bytes = state.inet_bytes;
                    last_net_ipv4 = state.net_ipv4;
                    state.status_dirty = true;
                    if state.screen == Screen::Log {
                        state.content_dirty = true;
                    }
                }
            }
            if log_buffer::take_dirty() {
                if state.screen == Screen::Log {
                    state.content_dirty = true;
                }
            }
            if state.settings_save_requested {
                state.settings_save_requested = false;
                #[allow(static_mut_refs)]
                unsafe {
                    let saved = if SETTINGS_ON_DISK_SRC {
                        kernel::settings_persist_disk::save(
                            DISK_SRC.assume_init_mut(),
                            &state.settings,
                        )
                    } else if SETTINGS_BLK_READY {
                        kernel::settings_persist_disk::save(
                            SETTINGS_BLK.assume_init_mut(),
                            &state.settings,
                        )
                    } else {
                        false
                    };
                    if saved {
                        diag_log::line(b"settings saved disk");
                    }
                }
            }
            gfx::render_frame(buf, &info, state, &font::FONT_5X7, cursor_eng);
            unsafe {
                core::arch::asm!("pause", options(nomem, nostack, preserves_flags));
            }
        }
    } else {
        serial::puts(b"\r\n=== EVE OS (x86_64) ===\r\n");
        serial::puts(b"Kernel started, but the bootloader did not provide a framebuffer.\r\n");
        serial::puts(b"Check: UEFI GOP / BIOS VESA, external GPU, or try the other boot image (BIOS vs UEFI).\r\n");
        serial::puts(b"Docs: install/REAL-HARDWARE.md | utm/X86-USB-BOOT.md\r\n\r\n");
        serial::puts(b"When a display works: default IP mode is SLIRP (SYS for DHCP/static).\r\n");
        serial::puts(b"USB keyboards need UHCI/OHCI or (future) xHCI; many laptops are xHCI-only.\r\n");
        serial::puts(b"Halting. Attach serial capture on COM1 115200 8N1 if available.\r\n");
    }

    idle_forever();
}

fn idle_forever() -> ! {
    unsafe {
        core::arch::asm!("cli", options(nomem, nostack, preserves_flags));
    }
    loop {
        unsafe {
            core::arch::asm!("hlt", options(nomem, nostack));
        }
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    diag_log::line(b"panic");
    if let Some(s) = info.message().as_str() {
        diag_log::err_msg(s.as_bytes());
    }
    idle_forever()
}
