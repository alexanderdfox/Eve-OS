// SPDX-License-Identifier: MIT OR Apache-2.0
//
//! Eve — TempleOS-inspired ring-0 guest (x86_64): browser chrome, VirtIO user-NAT HTTP, SYS prefs.
//!
//! # Device drivers actually in this tree (x86_64)
//!
//! - **Keyboard / mouse (implemented):** PS/2 (i8042); USB HID boot keyboard + up to 12 boot mice via PCI **UHCI** only — see `ps2.rs`, `uhci.rs`, `usb_hid.rs`.
//! - **Keyboard / mouse (not implemented):** OHCI, EHCI, xHCI; full hub topologies; non-boot HID, touchpads.
//! - **Networking (implemented):** VirtIO net PCI — ARP, DNS (`10.0.2.3`), TCP, HTTP/1.0 — see `virtio_net.rs`, `net.rs`, `url.rs`.
//! - **Networking (not implemented):** e1000, Realtek, other NICs; Wi‑Fi / 802.11; TLS; IPv6.
//! - **Bluetooth (not implemented):** SYS toggle is a placeholder — no HCI or stack.
//!
//! **QEMU / UTM / PC:** same guest code; USB vs PS/2 depends on VM devices and the **USB HOST** SYS toggle.
//! **Raspberry Pi** (`kernel-rpi/`): UART + mailbox framebuffer only — no USB or Eve UI there yet.
//!
//! “Add all drivers” is not a single feature: pick one concrete next target (e.g. e1000 for QEMU `-device e1000`).

#![no_std]
#![no_main]

mod font;
mod gfx;
mod net;
mod url;
mod pci;
mod ports;
mod ps2;
mod settings;
mod uhci;
mod usb_hid;
mod virtio_net;

use bootloader_api::config::Mapping;
use bootloader_api::{entry_point, BootInfo};
use gfx::{CursorEngine, SettingsTextFocus, UiState, MAX_CURSORS};
use net::{NetPhase, NetStack};
use ps2::{scancode_set1_to_ascii, Ps2Event};
use settings::{NicChoice, Screen};

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
    c
};

entry_point!(kernel_main, config = &BOOTLOADER_CONFIG);

fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
    let phys_skew: u64 = boot_info.physical_memory_offset.into_option().unwrap_or(0);
    unsafe {
        ps2::init();
        usb_hid::init(phys_skew);
    }

    let pci_wlan = unsafe { pci::scan_wlan_present() };
    let pci_eth = unsafe { pci::scan_ethernet_count() };
    let pci_mm_audio = unsafe { pci::scan_mm_audio_present() };

    let mut net = unsafe { virtio_net::VirtioNet::probe(boot_info) };
    let mut inet = NetStack::new();
    let mut inet_scratch = [0u8; 2048];
    if net.is_some() {
        if let Some(ref n) = net {
            inet.seed_from_mac(&n.mac);
        }
    }

    if let Some(framebuffer) = boot_info.framebuffer.as_mut() {
        let info = framebuffer.info();
        let buf = framebuffer.buffer_mut();
        let mut state = UiState::new(
            info.width as i32,
            info.height as i32,
            pci_wlan,
            pci_eth,
            pci_mm_audio,
        );
        if net.is_some() {
            state.net_ok = true;
            if let Some(ref n) = net {
                state.mac = n.mac;
            }
        }

        let mut cursor_eng = CursorEngine::new();
        let mut last_rx_drawn: u64 = state.net_rx;
        let mut last_inet_phase = state.inet_phase;
        let mut last_inet_bytes = state.inet_bytes;
        let mut boot_home_fetch_pending = net.is_some();

        loop {
            let inet_on = net.is_some()
                && state.settings.nic == NicChoice::Virtio
                && state.settings.internet_stack_enabled;
            if boot_home_fetch_pending && inet_on && state.url_len > 0 {
                boot_home_fetch_pending = false;
                start_browser_fetch(&mut inet, &mut state, inet_on);
            }
            unsafe {
                while let Some(ev) = ps2::poll_event() {
                    match ev {
                        Ps2Event::BrowserScroll { lines } => {
                            if state.screen == Screen::Browser {
                                browser_scroll(&mut state, lines);
                            }
                        }
                        Ps2Event::Key { code, shift } => {
                            if usb_hid::usb_ps2_kbd_should_ignore()
                                && state.settings.usb_polling_enabled
                            {
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
                                            start_browser_fetch(&mut inet, &mut state, inet_on);
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
                                    if settings_text_key(&mut state, ch) {
                                        state.content_dirty = true;
                                    }
                                }
                            }
                        }
                        Ps2Event::Mouse { buttons, dx, dy } => {
                            if usb_hid::usb_ps2_mouse_should_ignore()
                                && state.settings.usb_polling_enabled
                            {
                                continue;
                            }
                            state.mouse_btn = buttons;
                            state.cursor_x[0] += i32::from(dx);
                            state.cursor_y[0] += i32::from(dy);
                            state.cursor_x[0] = state.cursor_x[0].clamp(0, info.width as i32 - 1);
                            state.cursor_y[0] = state.cursor_y[0].clamp(0, info.height as i32 - 1);
                            state.mx = state.cursor_x[0];
                            state.my = state.cursor_y[0];
                        }
                    }
                }

                if state.settings.usb_polling_enabled {
                    if usb_hid::usb_mouse_active() {
                        let n = usb_hid::usb_mouse_count().min(MAX_CURSORS);
                        for i in 0..MAX_CURSORS {
                            state.cursor_active[i] = i < n;
                        }
                        for i in 0..n {
                            if let Some((btn, dx, dy)) = usb_hid::poll_hid_slot(i) {
                                state.cursor_x[i] += i32::from(dx);
                                state.cursor_y[i] += i32::from(dy);
                                state.cursor_x[i] =
                                    state.cursor_x[i].clamp(0, info.width as i32 - 1);
                                state.cursor_y[i] =
                                    state.cursor_y[i].clamp(0, info.height as i32 - 1);
                                if i == 0 {
                                    state.mouse_btn = btn;
                                    state.mx = state.cursor_x[0];
                                    state.my = state.cursor_y[0];
                                }
                            }
                        }
                    } else {
                        state.cursor_active[0] = true;
                        for i in 1..MAX_CURSORS {
                            state.cursor_active[i] = false;
                        }
                    }
                    if usb_hid::usb_keyboard_active() {
                        while let Some((usage, shift)) = usb_hid::poll_usb_key_press() {
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
                                0x51 if state.screen == Screen::Browser => {
                                    browser_scroll(&mut state, 3);
                                }
                                0x52 if state.screen == Screen::Browser => {
                                    browser_scroll(&mut state, -3);
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
                                                        &mut inet,
                                                        &mut state,
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
                                            if settings_text_key(&mut state, ch) {
                                                state.content_dirty = true;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                let left_now = state.mouse_btn & 1;
                let left_prev = state.prev_mouse_btn & 1;
                if left_now != 0 && left_prev == 0 {
                    if gfx::handle_click(&mut state, &info) {
                        state.content_dirty = true;
                    }
                }
                state.prev_mouse_btn = state.mouse_btn;

                if state.inet_reload_request {
                    state.inet_reload_request = false;
                    if state.url_len > 0 {
                        start_browser_fetch(&mut inet, &mut state, inet_on);
                    } else {
                        inet.reset_demo();
                        if let Some(ref n) = net {
                            inet.seed_from_mac(&n.mac);
                        }
                        state.page_body_len = 0;
                        state.fetch_err_len = 0;
                        state.page_truncated = false;
                        state.browser_body_dirty = true;
                    }
                    state.status_dirty = true;
                }

                // QEMU `virtio-net-pci` + `-netdev user` is always “linked”; no Wi‑Fi PHY.
                if inet_on {
                    if let Some(ref mut n) = net {
                        inet.drive(n, &state.mac, &mut inet_scratch);
                        state.inet_phase = inet.phase;
                        state.inet_bytes = inet.http_bytes;
                        if state.screen == Screen::Browser {
                            let pl = inet.page_len.min(state.page_body.len());
                            if pl != state.page_body_len
                                || inet.fetch_err_len != state.fetch_err_len
                                || inet.page_truncated != state.page_truncated
                            {
                                state.page_body[..pl].copy_from_slice(&inet.page[..pl]);
                                state.page_body_len = pl;
                                let fe = inet.fetch_err_len.min(state.fetch_err.len());
                                state.fetch_err[..fe].copy_from_slice(&inet.fetch_err[..fe]);
                                state.fetch_err_len = fe;
                                state.page_truncated = inet.page_truncated;
                                state.browser_body_dirty = true;
                            }
                        }
                    }
                } else {
                    state.inet_phase = NetPhase::Off;
                    state.inet_bytes = 0;
                }

                if let Some(ref n) = net {
                    if state.settings.nic == NicChoice::Virtio {
                        state.net_rx = n.rx_packets;
                        state.mac = n.mac;
                    }
                }
                state.net_ok = net.is_some() && state.settings.nic == NicChoice::Virtio;
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
            gfx::render_frame(buf, &info, &mut state, &font::FONT_5X7, &mut cursor_eng);
            unsafe {
                core::arch::asm!("pause", options(nomem, nostack, preserves_flags));
            }
        }
    }

    idle_forever();
}

fn idle_forever() -> ! {
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
