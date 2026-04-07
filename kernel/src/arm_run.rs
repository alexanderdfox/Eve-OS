// SPDX-License-Identifier: MIT OR Apache-2.0

//! Full Eve UI + VirtIO-MMIO networking on **AArch64** (UEFI GOP + Simple Pointer + Simple Text
//! Input via [`crate::arm_input`]). No PS/2, PCI Ethernet, USB HID stack, or disk install.

#![cfg(target_arch = "aarch64")]
#![allow(static_mut_refs)]

use core::mem::MaybeUninit;

use crate::arm_input::{self, ArmKeyEvent};
use crate::diag_log;
use crate::fb_info::FrameBufferInfo;
use crate::font;
use crate::gfx::{self, CursorEngine, SettingsTextFocus, UiState, MAX_CURSORS};
use crate::html;
use crate::log_buffer;
use crate::net::{NetPhase, NetStack};
use crate::nic;
use crate::power;
use crate::settings::{DeviceSettings, NicChoice, PlatformCaps, Screen};
use crate::settings_persist;
use crate::usb_hid;

pub type SettingsBlobSaveFn = fn(&[u8]);

#[allow(static_mut_refs)]
static mut SETTINGS_SAVER: Option<SettingsBlobSaveFn> = None;

#[allow(static_mut_refs)]
static mut BOOTSTRAP_SETTINGS: MaybeUninit<DeviceSettings> = MaybeUninit::uninit();
static mut HAVE_BOOTSTRAP_SETTINGS: bool = false;
static mut BOOTSTRAP_CAPS: PlatformCaps = PlatformCaps::arm_uefi(true);

/// Call from UEFI before the first [`main_step`] to apply NVRAM-loaded prefs.
pub fn set_bootstrap_device_settings(s: DeviceSettings) {
    unsafe {
        BOOTSTRAP_SETTINGS.write(s);
        HAVE_BOOTSTRAP_SETTINGS = true;
    }
}

/// Optional platform capability override from target entrypoint.
pub fn set_bootstrap_platform_caps(c: PlatformCaps) {
    unsafe {
        BOOTSTRAP_CAPS = c;
    }
}

/// Register `None` to disable; UEFI sets a function that writes the blob with `SetVariable`.
pub unsafe fn register_settings_blob_saver(f: Option<SettingsBlobSaveFn>) {
    SETTINGS_SAVER = f;
}

#[allow(static_mut_refs)]
static mut NET_STACK: NetStack = NetStack::static_initial();
#[allow(static_mut_refs)]
static mut UI_STATE: MaybeUninit<UiState> = MaybeUninit::uninit();
#[allow(static_mut_refs)]
static mut CURSOR_ENG: CursorEngine = CursorEngine::static_initial();
static mut INET_SCRATCH: [u8; 2048] = [0u8; 2048];
static mut NET_NIC: Option<nic::AnyNic> = None;
static mut ARM_INIT: bool = false;
static mut LAST_RX_DRAWN: u64 = 0;
static mut LAST_INET_PHASE: NetPhase = NetPhase::Off;
static mut LAST_INET_BYTES: u32 = 0;
static mut BOOT_HOME_FETCH_PENDING: bool = false;

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

fn scroll_arm(state: &mut UiState, info: &FrameBufferInfo, lines: i32) {
    if lines == 0 {
        return;
    }
    let lay = state.layout(info);
    match state.screen {
        Screen::Browser => browser_scroll(state, lines),
        Screen::Log => gfx::log_scroll_by_wheel(state, &lay, lines),
        Screen::Settings => gfx::settings_scroll_by_wheel(state, &lay, lines.saturating_mul(40)),
        Screen::DiskInstall => {
            gfx::disk_install_scroll_by_wheel(state, &lay, lines.saturating_mul(32));
        }
        _ => {}
    }
}

fn process_arm_keys(
    state: &mut UiState,
    inet: &mut NetStack,
    info: &FrameBufferInfo,
    inet_on: bool,
) {
    for ev in arm_input::key_events().iter().copied() {
        if state.screen == Screen::EpilepsyWarning {
            match ev {
                ArmKeyEvent::Char(b' ') | ArmKeyEvent::Char(b'\n') | ArmKeyEvent::Enter => {
                    gfx::dismiss_epilepsy_notice(state);
                    state.content_dirty = true;
                }
                _ => {}
            }
            continue;
        }
        if state.screen == Screen::CaliforniaAgeNotice {
            match ev {
                ArmKeyEvent::Char(b' ') | ArmKeyEvent::Char(b'\n') | ArmKeyEvent::Enter => {
                    gfx::dismiss_california_age_notice(state);
                    state.content_dirty = true;
                }
                _ => {}
            }
            continue;
        }

        match ev {
            ArmKeyEvent::Func(1) => {
                state.screen = Screen::Settings;
                state.content_dirty = true;
            }
            ArmKeyEvent::Func(2) => {
                state.screen = Screen::Browser;
                state.settings_text_focus = SettingsTextFocus::None;
                state.content_dirty = true;
            }
            ArmKeyEvent::Func(3) => {
                if state.screen == Screen::Settings {
                    state.settings = state.settings.toggle_midi_channel();
                    state.content_dirty = true;
                }
            }
            ArmKeyEvent::Func(4) if state.disk_install_available => {
                state.screen = Screen::DiskInstall;
                state.content_dirty = true;
            }
            ArmKeyEvent::Func(5) => {
                state.screen = Screen::Log;
                state.content_dirty = true;
            }
            ArmKeyEvent::Func(6) => {
                if state.screen == Screen::Browser {
                    state.bios_fullpage_browser = !state.bios_fullpage_browser;
                    state.content_dirty = true;
                }
            }
            ArmKeyEvent::PageDown => scroll_arm(state, info, 3),
            ArmKeyEvent::PageUp => scroll_arm(state, info, -3),
            ArmKeyEvent::ArrowDown => scroll_arm(state, info, 3),
            ArmKeyEvent::ArrowUp => scroll_arm(state, info, -3),
            ArmKeyEvent::Escape => {
                if state.screen == Screen::Settings {
                    state.settings_text_focus = SettingsTextFocus::None;
                    state.content_dirty = true;
                }
            }
            ArmKeyEvent::Backspace => {
                if state.screen == Screen::Browser {
                    if state.url_len > 0 {
                        state.url_len -= 1;
                        state.chrome_only_dirty = true;
                    }
                } else if state.screen == Screen::Settings {
                    if settings_text_key(state, 0x08) {
                        state.content_dirty = true;
                    }
                }
            }
            ArmKeyEvent::Enter => {
                if state.screen == Screen::Browser {
                    start_browser_fetch(inet, state, inet_on);
                }
            }
            ArmKeyEvent::Char(ch) => {
                if state.screen == Screen::Browser {
                    if ch >= 32 && ch < 127 && state.url_len < state.url.len() - 1 {
                        state.url[state.url_len] = ch;
                        state.url_len += 1;
                        state.chrome_only_dirty = true;
                    }
                } else if state.screen == Screen::Settings {
                    if settings_text_key(state, ch) {
                        state.content_dirty = true;
                    }
                }
            }
            _ => {}
        }
    }
}

unsafe fn ensure_init(info: &FrameBufferInfo) {
    if ARM_INIT {
        return;
    }
    ARM_INIT = true;
    let base_settings = if HAVE_BOOTSTRAP_SETTINGS {
        HAVE_BOOTSTRAP_SETTINGS = false;
        BOOTSTRAP_SETTINGS.assume_init_read()
    } else {
        DeviceSettings::new()
    };
    let caps = BOOTSTRAP_CAPS;
    diag_log::line2(b"caps input ", caps.input_backend.label());
    diag_log::line2(b"caps usb ", caps.usb_parity.label());
    diag_log::line2(b"caps wifi ", caps.wifi_mode_label());
    diag_log::line2(b"caps net ", caps.net_mode_label());
    diag_log::line2(b"caps save ", caps.persist_label());
    usb_hid::init(0);
    let n = nic::AnyNic::probe();
    if let Some(ref ni) = n {
        diag_log::line2(b"nic ", ni.driver_tag());
        diag_log::mac(ni.mac());
        (&mut *core::ptr::addr_of_mut!(NET_STACK)).seed_from_mac(ni.mac());
    } else {
        diag_log::line(b"nic none");
    }
    NET_NIC = n;
    UI_STATE.write(UiState::new_with_settings(
        info.width as i32,
        info.height as i32,
        false,
        0,
        0,
        0,
        0,
        0,
        0,
        false,
        caps,
        base_settings,
    ));
    let s = UI_STATE.assume_init_mut();
    if NET_NIC.is_some() {
        s.net_ok = true;
        if let Some(ref ni) = NET_NIC {
            s.mac = *ni.mac();
        }
    }
    LAST_RX_DRAWN = s.net_rx;
    LAST_INET_PHASE = s.inet_phase;
    LAST_INET_BYTES = s.inet_bytes;
    BOOT_HOME_FETCH_PENDING = NET_NIC.is_some();
}

/// One frame: network stack, input merge, chrome, and render into `buf`. Call from UEFI after
/// updating [`crate::arm_input`] and blit `buf` to GOP.
pub unsafe fn main_step(buf: &mut [u8], info: &FrameBufferInfo) {
    ensure_init(info);
    let state = UI_STATE.assume_init_mut();
    let inet = &mut *core::ptr::addr_of_mut!(NET_STACK);
    let cursor_eng = &mut *core::ptr::addr_of_mut!(CURSOR_ENG);

    let inet_on = NET_NIC.is_some()
        && state.settings.nic != NicChoice::Off
        && state.settings.internet_stack_enabled;
    if BOOT_HOME_FETCH_PENDING
        && state.screen == Screen::Browser
        && inet_on
        && state.url_len > 0
        && state.inet_phase != NetPhase::Off
    {
        BOOT_HOME_FETCH_PENDING = false;
        start_browser_fetch(inet, state, inet_on);
    }

    process_arm_keys(state, inet, info, inet_on);

    arm_input::sync_primary_cursor(state, info.width as i32, info.height as i32);

    unsafe {
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
                    if gfx::handle_click_at(state, info, cx, cy) {
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
                    if let Some(ref n) = NET_NIC {
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

            if inet_on {
                if let Some(ref mut n) = NET_NIC {
                    inet.drive(
                        n,
                        &state.mac,
                        &mut INET_SCRATCH[..],
                        &state.settings,
                    );
                    state.inet_phase = inet.phase;
                    state.inet_bytes = inet.http_bytes;
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
                            if let Some(res) = crate::script_runtime::run_page_eve_script(
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
                state.inet_phase = NetPhase::Off;
                state.inet_bytes = 0;
            }

            if let Some(ref n) = NET_NIC {
                if state.settings.nic != NicChoice::Off {
                    state.net_rx = n.rx_packets();
                    state.mac = *n.mac();
                }
            }
            state.net_ok = NET_NIC.is_some() && state.settings.nic != NicChoice::Off;
            if state.net_rx != LAST_RX_DRAWN
                || state.inet_phase != LAST_INET_PHASE
                || state.inet_bytes != LAST_INET_BYTES
            {
                LAST_RX_DRAWN = state.net_rx;
                LAST_INET_PHASE = state.inet_phase;
                LAST_INET_BYTES = state.inet_bytes;
                state.status_dirty = true;
            }
    }

    if log_buffer::take_dirty() {
        if state.screen == Screen::Log {
            state.content_dirty = true;
        }
    }

    if state.settings_save_requested {
        state.settings_save_requested = false;
        let mut blob = [0u8; settings_persist::BLOB_LEN];
        settings_persist::encode(&state.settings, &mut blob);
        if let Some(f) = SETTINGS_SAVER {
            f(&blob);
        }
    }

    gfx::render_frame(buf, info, state, &font::FONT_5X7, cursor_eng);
}
