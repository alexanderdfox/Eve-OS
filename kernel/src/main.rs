// SPDX-License-Identifier: MIT OR Apache-2.0
//
//! Eve — TempleOS-inspired ring-0 guest (x86_64): browser chrome, VirtIO user-NAT HTTP, SYS prefs.
//! **Browser:** HTTP body is passed through a small **HTML/CSS subset** renderer (`html.rs`); **`<script>` is stripped** (not executed).
//!
//! # Device drivers actually in this tree (x86_64)
//!
//! - **Keyboard / mouse (implemented):** PS/2 (i8042, 3- and 4-byte ImPS/2 mouse packets); USB HID boot keyboard + up to 12 boot mice via **UHCI** (I/O) or **OHCI** (MMIO). With **USB poll** on, USB mice use cursors **0..N−1** and the **PS/2** mouse uses cursor **N** (when **N < 12**) so all pointers work together — see `ps2.rs`, `uhci.rs`, `ohci.rs`, `usb_hid.rs`.
//! - **Keyboard / mouse (partial):** **xHCI** / **EHCI** PCI hooks exist (`xhci.rs`, `ehci.rs`); HID on xHCI and FS-through-EHCI are not finished yet.
//! - **Networking (implemented):** **VirtIO net**, **Realtek RTL8139**, **RTL8168/8169** (MMIO C+), **Intel e1000 / e1000e-class PCI IDs**, **AMD PCnet** (QEMU `pcnet`) — ARP, DNS, TCP, HTTP/1.0 — SYS **IP MODE**: SLIRP (`10.0.2.x`), **DHCP**, or **static** — see `virtio_net.rs`, `rtl8139.rs`, `rtl8168.rs`, `e1000.rs`, `pcnet.rs`, `nic.rs`, `net.rs`, `net_ipv4.rs`, `url.rs`.
//! - **Disk install (QEMU / VirtIO):** With **two** `virtio-blk` PCI disks, the **INSTALL** tab clones disk 1 → disk 2 sector-by-sector — see `virtio_blk.rs`, `install/pc-x86-64-disk-install/`.
//! - **Browser boot:** A **photosensitivity / epilepsy** notice is shown first (full screen); **Enter**, **Space**, or **Continue** opens the OS. With networking, the default URL is **`https://www.google.com/`**, fetched after that; the UI starts in **BIOS-style full page** (no title bar / tabs / URL strip / status) until **F6** restores chrome — see `gfx.rs`.
//! - **Networking (stubs only):** **vmxnet3**, **Broadcom bge** — PCI hooks exist (`vmxnet3.rs`, `bge.rs`) but devices are not brought up yet; **802.11** (SSID/PSK in SYS are UI-only — no WPA/802.11 MAC; see `utm/WIFI-80211.txt`); IPv6.
//! - **TLS:** `https://` uses TLS 1.3 (**encrypted**). ** PKIX verification is not enabled** on this
//!   bare-metal target (`rustls-webpki`/`ring` do not build for `x86_64-unknown-none`) — treat HTTPS
//!   as **encryption-only**, not authenticated identity; see `eve_tls.rs` / `utm/BROWSER-LIMITS.txt`.
//! - **Bluetooth (not implemented):** SYS toggle is a placeholder — no HCI or stack.
//!
//! **QEMU / UTM / PC:** same guest code; **USB poll** in SYS enables UHCI/OHCI multi-mice plus PS/2 on a separate cursor slot.
//! **Raspberry Pi** (`kernel-rpi/`): UART + mailbox framebuffer only — no USB or Eve UI there yet.
//!
//! # Real PC (bare metal x86_64)
//!
//! - **Boot:** use `utm/eve-bios.img` or `utm/eve-uefi.img` on a USB stick — see `install/REAL-HARDWARE.txt`
//!   and `utm/X86-USB-BOOT.txt`.
//! - **Display:** bootloader-provided framebuffer (GOP / VESA) when firmware allows.
//! - **Input:** PS/2 and **UHCI USB HID** only; most laptops are **xHCI-only** (no built-in keyboard driver yet).
//! - **Network:** **VirtIO**, **RTL8139**, **RTL8168/8169**, **e1000/e1000e-class**, or **PCnet**; TCP/IP uses SYS **IP MODE** (SLIRP / DHCP / static). Bare metal
//!   shows **NET: NODRV** without a supported PCI Ethernet device — see `install/REAL-HARDWARE.txt`.
//!
//! “Add all drivers” is not a single feature: pick one concrete next target (e.g. DHCP for LAN).

#![no_std]
#![no_main]

use core::mem::MaybeUninit;

mod cursor_emoji;
mod font;
mod gfx;
mod html;
mod net;
mod net_ipv4;
mod power;
mod e1000;
mod nic;
mod pcnet;
mod rtl8139;
mod rtl8168;
mod vmxnet3;
mod bge;
mod eve_tls;
mod url;
mod pci;
mod ports;
mod ps2;
mod settings;
mod ehci;
mod ohci;
mod uhci;
mod usb_common;
mod usb_hid;
mod virtio_blk;
mod virtio_net;
mod xhci;

use bootloader_api::config::Mapping;
use bootloader_api::{entry_point, BootInfo};
use gfx::{CursorEngine, SettingsTextFocus, UiState, MAX_CURSORS};
use net::{NetPhase, NetStack};
use ps2::{scancode_set1_to_ascii, Ps2Event};
use settings::{DiskInstallPhase, NicChoice, Screen};

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
static mut DISK_SRC: MaybeUninit<virtio_blk::VirtioBlk> = MaybeUninit::uninit();
#[allow(static_mut_refs)]
static mut DISK_DST: MaybeUninit<virtio_blk::VirtioBlk> = MaybeUninit::uninit();
static mut INSTALL_SECTOR_BUF: [u8; 512] = [0u8; 512];

fn browser_scroll(state: &mut UiState, lines: i32) {
    if lines == 0 {
        return;
    }
    if lines < 0 {
        let d = (-lines) as usize;
        state.page_scroll_line = state.page_scroll_line.saturating_sub(d);
    } else {
        state.page_scroll_line = state
            .page_scroll_line
            .saturating_add(lines as usize)
            .min(4096);
    }
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
    }
    true
}

fn start_browser_fetch(inet: &mut NetStack, state: &mut UiState, inet_on: bool) {
    if state.url_len == 0 || !inet_on {
        return;
    }
    inet.start_fetch(&state.url[..state.url_len]);
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
    let pci_eth = unsafe { pci::scan_ethernet_count() };
    let pci_mm_audio = unsafe { pci::scan_mm_audio_present() };

    let mut net = unsafe { nic::AnyNic::probe(boot_info) };
    #[allow(static_mut_refs)]
    let inet = unsafe { &mut NET_STACK };
    if net.is_some() {
        if let Some(ref n) = net {
            inet.seed_from_mac(n.mac());
        }
    }

    let mut bfds = [(0u8, 0u8, 0u8); 8];
    let n_blk = unsafe { virtio_blk::enumerate(&mut bfds) };
    let disk_pair_ready = unsafe {
        if n_blk >= 2 {
            if let Some(src) = virtio_blk::VirtioBlk::init(
                bfds[0].0,
                bfds[0].1,
                bfds[0].2,
                boot_info,
                0,
                false,
            ) {
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
        } else {
            false
        }
    };

    if let Some(framebuffer) = boot_info.framebuffer.as_mut() {
        let info = framebuffer.info();
        let buf = framebuffer.buffer_mut();
        let state = {
            #[allow(static_mut_refs)]
            unsafe {
                UI_STATE.write(UiState::new(
                    info.width as i32,
                    info.height as i32,
                    pci_wlan,
                    wlan_pci_count,
                    wlan_first_vid,
                    wlan_first_did,
                    pci_eth,
                    pci_mm_audio,
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
        let mut boot_home_fetch_pending = net.is_some();

        loop {
            let inet_on = net.is_some()
                && state.settings.nic != NicChoice::Off
                && state.settings.internet_stack_enabled;
            if boot_home_fetch_pending
                && state.screen == Screen::Browser
                && inet_on
                && state.url_len > 0
                && state.inet_phase != NetPhase::Off
            {
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
                            if state.screen == Screen::Browser {
                                browser_scroll(state, lines);
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
                                0x40 if state.screen == Screen::Browser => {
                                    state.bios_fullpage_browser = !state.bios_fullpage_browser;
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
                    if usb_hid::usb_mouse_active() {
                        for i in 0..n_usb {
                            if let Some((btn, dx, dy)) = usb_hid::poll_hid_slot(i) {
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
                                0x3F if state.screen == Screen::Browser => {
                                    state.bios_fullpage_browser = !state.bios_fullpage_browser;
                                    state.content_dirty = true;
                                }
                                0x51 if state.screen == Screen::Browser => {
                                    browser_scroll(state, 3);
                                }
                                0x52 if state.screen == Screen::Browser => {
                                    browser_scroll(state, -3);
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
                    power::system_reboot();
                    power::halt_forever();
                }
                if state.power_shutdown_request {
                    state.power_shutdown_request = false;
                    power::system_shutdown();
                    power::halt_forever();
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
                        state.inet_phase = inet.phase;
                        state.inet_bytes = inet.http_bytes;
                        if state.screen == Screen::Browser {
                            let pl = inet.page_len.min(inet.page.len());
                            if pl != state.last_rendered_raw_len
                                || inet.fetch_err_len != state.fetch_err_len
                                || inet.page_truncated != state.page_truncated
                            {
                                state.last_rendered_raw_len = pl;
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
                                    );
                                    state.page_truncated =
                                        inet.page_truncated || html_trunc;
                                }
                                state.browser_body_dirty = true;
                            }
                        }
                    }
                } else {
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
                {
                    last_rx_drawn = state.net_rx;
                    last_inet_phase = state.inet_phase;
                    last_inet_bytes = state.inet_bytes;
                    state.status_dirty = true;
                }
            }
            gfx::render_frame(buf, &info, state, &font::FONT_5X7, cursor_eng);
            unsafe {
                core::arch::asm!("pause", options(nomem, nostack, preserves_flags));
            }
        }
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
fn panic(_: &core::panic::PanicInfo) -> ! {
    idle_forever()
}
