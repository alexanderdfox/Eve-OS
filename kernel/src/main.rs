// SPDX-License-Identifier: MIT OR Apache-2.0
//
//! Eve — ring-0 shell with PS/2 keyboard/mouse and VirtIO network (QEMU).

#![no_std]
#![no_main]

mod font;
mod gfx;
mod net;
mod pci;
mod ports;
mod ps2;
mod settings;
mod usb_hid;
mod virtio_net;

use bootloader_api::config::Mapping;
use bootloader_api::{entry_point, BootInfo};
use gfx::{CursorEngine, UiState};
use ps2::{scancode_set1_to_ascii, Ps2Event};
use net::{NetPhase, NetStack};
use settings::{NicChoice, Screen};

pub static BOOTLOADER_CONFIG: bootloader_api::BootloaderConfig = {
    let mut c = bootloader_api::BootloaderConfig::new_default();
    c.mappings.physical_memory = Some(Mapping::Dynamic);
    c
};

entry_point!(kernel_main, config = &BOOTLOADER_CONFIG);

fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
    unsafe {
        ps2::init();
        usb_hid::init();
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

        loop {
            unsafe {
                while let Some(ev) = ps2::poll_event() {
                    match ev {
                        Ps2Event::Key { code, shift } => {
                            match code {
                                0x3B => {
                                    state.screen = Screen::Settings;
                                    state.content_dirty = true;
                                    continue;
                                }
                                0x3C => {
                                    state.screen = Screen::Browser;
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
                                                state.content_dirty = true;
                                            }
                                        }
                                        b'\n' => {}
                                        c if state.url_len < state.url.len() - 1 => {
                                            if c >= 32 && c < 127 {
                                                state.url[state.url_len] = c;
                                                state.url_len += 1;
                                                state.content_dirty = true;
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                        Ps2Event::Mouse { buttons, dx, dy } => {
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
                    let _ = usb_hid::poll_hid();
                }

                let left_now = state.mouse_btn & 1;
                let left_prev = state.prev_mouse_btn & 1;
                if left_now != 0 && left_prev == 0 {
                    if gfx::handle_click(&mut state, &info) {
                        state.content_dirty = true;
                    }
                }
                state.prev_mouse_btn = state.mouse_btn;

                let inet_on = net.is_some()
                    && state.settings.nic == NicChoice::Virtio
                    && (state.settings.wifi_enabled || pci_eth > 0)
                    && state.settings.internet_stack_enabled;
                if inet_on {
                    if let Some(ref mut n) = net {
                        inet.drive(n, &state.mac, &mut inet_scratch);
                        state.inet_phase = inet.phase;
                        state.inet_bytes = inet.http_bytes;
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
            gfx::render_frame(
                buf,
                &info,
                &mut state,
                &font::FONT_5X7,
                &mut cursor_eng,
            );
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
