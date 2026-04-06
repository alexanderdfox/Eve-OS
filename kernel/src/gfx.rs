// SPDX-License-Identifier: MIT OR Apache-2.0

//! TempleOS-style palette and chrome: Web Shrine tab, SYS settings, multi-pointer USB HUD,
//! MIDI flags, VirtIO status. URL typing uses `chrome_only_dirty` (no full clear) so save-under
//! cursors stay consistent; full `content_dirty` for tab/screen changes.

use crate::cursor_emoji;
use crate::html::{self, BROWSER_LINE_CAP, BROWSER_MAX_LINES};
use crate::net::NetPhase;
use crate::settings::{DeviceSettings, DiskInstallPhase, NicChoice, Screen};
use crate::usb_hid;
use bootloader_api::info::{FrameBufferInfo, PixelFormat};

pub const TAB_EVE_X: usize = 12;
pub const TAB_EVE_W: usize = 130;
pub const TAB_SET_X: usize = 148;
pub const TAB_SET_W: usize = 90;
pub const TAB_INS_X: usize = 246;
pub const TAB_INS_W: usize = 100;

pub const MAX_CURSORS: usize = 12;

/// Home page when VirtIO + the internet stack are on (`https://www.google.com/` over TLS 1.3).
pub const DEFAULT_HOME_URL: &[u8] = b"https://www.google.com/";

#[inline]
pub fn browser_bios_fullpage(state: &UiState) -> bool {
    state.screen == Screen::Browser && state.bios_fullpage_browser
}

/// Browser URL bar: `<` `>` `R` `HOME` `GO`.
const URL_BAR_BTN_COUNT: usize = 5;

#[inline]
fn url_bar_btn_width(w: usize) -> usize {
    // "HOME" needs ≥24px width at 6px/char; keep a floor so narrow windows still fit labels.
    (32.min(w / (URL_BAR_BTN_COUNT + 8))).max(24)
}

#[inline]
fn url_bar_url_x0(w: usize) -> usize {
    let btn = url_bar_btn_width(w);
    12 + URL_BAR_BTN_COUNT * (btn + 8) + 8
}

/// Text field focus on SYS settings (URL bar uses separate `url` buffers).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SettingsTextFocus {
    None,
    WifiSsid,
    WifiPsk,
}

pub struct UiState {
    pub url: [u8; 192],
    pub url_len: usize,
    /// Primary pointer (cursor 0) after input merge; each active cursor has its own `cursor_btn`.
    pub mx: i32,
    pub my: i32,
    pub cursor_x: [i32; MAX_CURSORS],
    pub cursor_y: [i32; MAX_CURSORS],
    pub cursor_active: [bool; MAX_CURSORS],
    /// Per-pointer buttons (USB slots 0..N-1; PS/2 on slot N when USB poll + USB mice are active).
    pub cursor_btn: [u8; MAX_CURSORS],
    pub prev_cursor_btn: [u8; MAX_CURSORS],
    pub net_rx: u64,
    pub net_ok: bool,
    pub mac: [u8; 6],
    pub screen: Screen,
    pub settings: DeviceSettings,
    pub pci_wlan: bool,
    /// PCI 802.11 functions found (enumeration only — no MAC/PHY driver).
    pub wlan_pci_count: u8,
    pub wlan_first_vid: u16,
    pub wlan_first_did: u16,
    pub pci_eth_count: u8,
    pub pci_mm_audio: bool,
    /// QEMU user-net stack phase (VirtIO + Wi-Fi or Ethernet link).
    pub inet_phase: NetPhase,
    pub inet_bytes: u32,
    /// Browser chrome “R” queued a reload of the current URL (handled in `main`).
    pub inet_reload_request: bool,
    /// How many entries in `html::browser_line(0..count)` are valid after `format_document`.
    pub browser_line_count: usize,
    /// Last `inet.page_len` we passed through `html::format_document` (avoids redundant work).
    pub last_rendered_raw_len: usize,
    pub page_scroll_line: usize,
    pub page_truncated: bool,
    pub fetch_err: [u8; 80],
    pub fetch_err_len: usize,
    pub settings_text_focus: SettingsTextFocus,
    /// Demo “scan” hits (no 802.11 driver); tap a row to copy into SSID.
    pub wifi_scan_count: u8,
    pub wifi_scan_demo: bool,
    pub wifi_scan_names: [[u8; 32]; 3],
    pub wifi_scan_lens: [u8; 3],
    /// Two VirtIO block disks present (`virtio_blk` probe in `main`).
    pub disk_install_available: bool,
    pub disk_install_phase: DiskInstallPhase,
    pub disk_install_cur: u64,
    pub disk_install_total: u64,
    pub disk_install_err: [u8; 56],
    pub disk_install_err_len: usize,
    /// SYS / install UI requested starting the clone (handled in `main`).
    pub disk_install_start_request: bool,
    /// Browser: hide title/tabs/URL bar/status — full framebuffer is the page (firmware-like boot).
    /// F6 toggles chrome back on.
    pub bios_fullpage_browser: bool,
    /// Full UI repaint (clear + chrome + body + status text).
    pub content_dirty: bool,
    /// Browser: URL / tab strip / nav bar / status — no full-screen clear (keeps cursor save/restore stable while typing).
    pub chrome_only_dirty: bool,
    /// Browser body area only (page text / errors).
    pub browser_body_dirty: bool,
    /// Status strip only (RX counter etc.).
    pub status_dirty: bool,
    /// SYS: user clicked **Reboot** (handled in `main` via `power::system_reboot`).
    pub power_reboot_request: bool,
    /// SYS: user clicked **Shutdown** (handled in `main` via `power::system_shutdown`).
    pub power_shutdown_request: bool,
    /// After `Screen::EpilepsyWarning` is dismissed: **Browser** or **DiskInstall** (two-disk QEMU).
    pub screen_after_epilepsy_notice: Screen,
}

pub struct Layout {
    pub w: usize,
    pub h: usize,
    pub chrome_h: usize,
    pub tab_y: usize,
    pub tab_h: usize,
    pub bar_y: usize,
    pub bar_h: usize,
    pub content_top: usize,
}

impl UiState {
    pub fn new(
        width: i32,
        height: i32,
        pci_wlan: bool,
        wlan_pci_count: u8,
        wlan_first_vid: u16,
        wlan_first_did: u16,
        pci_eth_count: u8,
        pci_mm_audio: bool,
    ) -> Self {
        let mut url = [0u8; 192];
        let s = DEFAULT_HOME_URL;
        debug_assert!(s.len() < url.len(), "DEFAULT_HOME_URL fits in url buffer");
        url[..s.len()].copy_from_slice(s);
        let mut cursor_x = [0i32; MAX_CURSORS];
        let mut cursor_y = [0i32; MAX_CURSORS];
        let mut cursor_active = [false; MAX_CURSORS];
        cursor_active[0] = true;
        for i in 0..MAX_CURSORS {
            let col = (i % 4) as i32;
            let row = (i / 4) as i32;
            cursor_x[i] = (width * (6 + col * 22)) / 100;
            cursor_y[i] = (height * (18 + row * 20)) / 100;
            cursor_x[i] = cursor_x[i].clamp(0, width.saturating_sub(1));
            cursor_y[i] = cursor_y[i].clamp(0, height.saturating_sub(1));
        }
        let mx = cursor_x[0];
        let my = cursor_y[0];
        Self {
            url,
            url_len: s.len(),
            mx,
            my,
            cursor_x,
            cursor_y,
            cursor_active,
            cursor_btn: [0; MAX_CURSORS],
            prev_cursor_btn: [0; MAX_CURSORS],
            net_rx: 0,
            net_ok: false,
            mac: [0; 6],
            screen: Screen::EpilepsyWarning,
            screen_after_epilepsy_notice: Screen::Browser,
            settings: DeviceSettings::new(),
            pci_wlan: pci_wlan,
            wlan_pci_count,
            wlan_first_vid,
            wlan_first_did,
            pci_eth_count: pci_eth_count,
            pci_mm_audio: pci_mm_audio,
            inet_phase: NetPhase::Off,
            inet_bytes: 0,
            inet_reload_request: false,
            browser_line_count: 0,
            last_rendered_raw_len: usize::MAX,
            page_scroll_line: 0,
            page_truncated: false,
            fetch_err: [0; 80],
            fetch_err_len: 0,
            settings_text_focus: SettingsTextFocus::None,
            wifi_scan_count: 0,
            wifi_scan_demo: false,
            wifi_scan_names: [[0; 32]; 3],
            wifi_scan_lens: [0; 3],
            disk_install_available: false,
            disk_install_phase: DiskInstallPhase::Idle,
            disk_install_cur: 0,
            disk_install_total: 0,
            disk_install_err: [0; 56],
            disk_install_err_len: 0,
            disk_install_start_request: false,
            bios_fullpage_browser: true,
            content_dirty: true,
            chrome_only_dirty: false,
            browser_body_dirty: false,
            status_dirty: true,
            power_reboot_request: false,
            power_shutdown_request: false,
        }
    }

    pub fn layout(&self, info: &FrameBufferInfo) -> Layout {
        let w = info.width;
        let h = info.height;
        if self.screen == Screen::EpilepsyWarning {
            return Layout {
                w,
                h,
                chrome_h: 0,
                tab_y: 0,
                tab_h: 0,
                bar_y: 0,
                bar_h: 0,
                content_top: 0,
            };
        }
        if browser_bios_fullpage(self) {
            return Layout {
                w,
                h,
                chrome_h: 0,
                tab_y: 0,
                tab_h: 0,
                bar_y: 0,
                bar_h: 0,
                content_top: 0,
            };
        }
        let chrome_h = (56usize).min(h / 8).max(24);
        let tab_y = chrome_h + 4;
        let tab_h = (36usize).min((h.saturating_sub(chrome_h)) / 6).max(20);
        let bar_y = tab_y + tab_h + 6;
        let bar_h = (40usize).min((h.saturating_sub(bar_y)) / 5).max(28);
        let content_top = bar_y + bar_h + 12;
        Layout {
            w,
            h,
            chrome_h,
            tab_y,
            tab_h,
            bar_y,
            bar_h,
            content_top,
        }
    }
}

fn pixel(buf: &mut [u8], info: &FrameBufferInfo, x: usize, y: usize, r: u8, g: u8, b: u8) {
    if x >= info.width || y >= info.height {
        return;
    }
    let bpp = info.bytes_per_pixel;
    let stride = info.stride;
    let i = y * stride * bpp + x * bpp;
    if i + bpp > buf.len() {
        return;
    }
    match info.pixel_format {
        PixelFormat::Rgb => {
            buf[i] = r;
            buf[i + 1] = g;
            buf[i + 2] = b;
        }
        PixelFormat::Bgr => {
            buf[i] = b;
            buf[i + 1] = g;
            buf[i + 2] = r;
        }
        PixelFormat::U8 => {
            let v = ((r as u16 + g as u16 + b as u16) / 3) as u8;
            buf[i] = v;
        }
        _ => {}
    }
}

fn clear(buf: &mut [u8], info: &FrameBufferInfo, r: u8, g: u8, b: u8) {
    for y in 0..info.height {
        for x in 0..info.width {
            pixel(buf, info, x, y, r, g, b);
        }
    }
}

fn blit_save_rect(
    buf: &[u8],
    info: &FrameBufferInfo,
    x0: usize,
    y0: usize,
    w: usize,
    h: usize,
    dst: &mut [u8],
) -> usize {
    let bpp = info.bytes_per_pixel;
    if bpp == 0 || w == 0 || h == 0 {
        return 0;
    }
    let stride = info.stride;
    let mut o = 0;
    for row in 0..h {
        let y = y0 + row;
        if y >= info.height {
            break;
        }
        let base = y * stride * bpp + x0 * bpp;
        let row_bytes = w * bpp;
        if base + row_bytes > buf.len() || o + row_bytes > dst.len() {
            break;
        }
        dst[o..o + row_bytes].copy_from_slice(&buf[base..base + row_bytes]);
        o += row_bytes;
    }
    o
}

fn blit_restore_rect(
    buf: &mut [u8],
    info: &FrameBufferInfo,
    x0: usize,
    y0: usize,
    w: usize,
    h: usize,
    src: &[u8],
) {
    let bpp = info.bytes_per_pixel;
    if bpp == 0 || w == 0 || h == 0 {
        return;
    }
    let stride = info.stride;
    let mut o = 0;
    for row in 0..h {
        let y = y0 + row;
        if y >= info.height {
            break;
        }
        let base = y * stride * bpp + x0 * bpp;
        let row_bytes = w * bpp;
        if base + row_bytes > buf.len() || o + row_bytes > src.len() {
            break;
        }
        buf[base..base + row_bytes].copy_from_slice(&src[o..o + row_bytes]);
        o += row_bytes;
    }
}

const CUR_HALF: i32 = cursor_emoji::CUR_HALF;
const SLOT_BYTES: usize = 22 * 22 * 4;

fn cursor_clip_rect(
    cx: i32,
    cy: i32,
    screen_w: usize,
    screen_h: usize,
) -> (usize, usize, usize, usize) {
    let w = screen_w as i32;
    let h = screen_h as i32;
    let x0 = (cx - CUR_HALF).clamp(0, w.saturating_sub(1));
    let y0 = (cy - CUR_HALF).clamp(0, h.saturating_sub(1));
    let x1 = (cx + CUR_HALF + 1).clamp(0, w);
    let y1 = (cy + CUR_HALF + 1).clamp(0, h);
    (
        x0 as usize,
        y0 as usize,
        (x1 - x0) as usize,
        (y1 - y0) as usize,
    )
}

fn draw_cursor_indexed(
    buf: &mut [u8],
    info: &FrameBufferInfo,
    cx: i32,
    cy: i32,
    idx: usize,
    emoji_preset: u8,
) {
    cursor_emoji::draw_sprite(buf, info, cx, cy, emoji_preset, idx);
}

pub struct CursorEngine {
    last_x: [i32; MAX_CURSORS],
    last_y: [i32; MAX_CURSORS],
    last_active: [bool; MAX_CURSORS],
    sx: [usize; MAX_CURSORS],
    sy: [usize; MAX_CURSORS],
    sw: [usize; MAX_CURSORS],
    sh: [usize; MAX_CURSORS],
    saved: [u8; MAX_CURSORS * SLOT_BYTES],
    save_len: [usize; MAX_CURSORS],
    save_valid: [bool; MAX_CURSORS],
    pub initialized: bool,
}

impl CursorEngine {
    /// Same as `new()` but `const` for `static mut` BSS init (keeps ~23 KiB off the kernel stack at boot).
    pub const fn static_initial() -> Self {
        Self {
            last_x: [0; MAX_CURSORS],
            last_y: [0; MAX_CURSORS],
            last_active: [false; MAX_CURSORS],
            sx: [0; MAX_CURSORS],
            sy: [0; MAX_CURSORS],
            sw: [0; MAX_CURSORS],
            sh: [0; MAX_CURSORS],
            saved: [0; MAX_CURSORS * SLOT_BYTES],
            save_len: [0; MAX_CURSORS],
            save_valid: [false; MAX_CURSORS],
            initialized: false,
        }
    }

    fn invalidate_all_saves(&mut self) {
        for v in &mut self.save_valid {
            *v = false;
        }
    }

    pub fn any_cursor_moved(&self, state: &UiState) -> bool {
        for i in 0..MAX_CURSORS {
            if state.cursor_active[i] != self.last_active[i] {
                return true;
            }
            if state.cursor_active[i]
                && (state.cursor_x[i] != self.last_x[i] || state.cursor_y[i] != self.last_y[i])
            {
                return true;
            }
        }
        false
    }

    fn slot_mut(&mut self, i: usize) -> &mut [u8] {
        let s = i * SLOT_BYTES;
        &mut self.saved[s..s + SLOT_BYTES]
    }

    fn restore_one(&mut self, buf: &mut [u8], info: &FrameBufferInfo, i: usize) {
        if !self.save_valid[i] || self.save_len[i] == 0 {
            return;
        }
        let slot = &self.saved[i * SLOT_BYTES..i * SLOT_BYTES + self.save_len[i]];
        blit_restore_rect(
            buf, info, self.sx[i], self.sy[i], self.sw[i], self.sh[i], slot,
        );
        self.save_valid[i] = false;
    }

    fn save_one(&mut self, buf: &[u8], info: &FrameBufferInfo, i: usize, cx: i32, cy: i32) {
        let (x0, y0, rw, rh) = cursor_clip_rect(cx, cy, info.width, info.height);
        if rw == 0 || rh == 0 {
            self.save_valid[i] = false;
            self.save_len[i] = 0;
            return;
        }
        self.sx[i] = x0;
        self.sy[i] = y0;
        self.sw[i] = rw;
        self.sh[i] = rh;
        let slot = self.slot_mut(i);
        let n = blit_save_rect(buf, info, x0, y0, rw, rh, slot);
        self.save_len[i] = n;
        self.save_valid[i] = n > 0;
    }

    fn draw_all_cursors(&self, buf: &mut [u8], info: &FrameBufferInfo, state: &UiState) {
        let preset = state.settings.cursor_emoji_preset;
        for i in 0..MAX_CURSORS {
            if state.cursor_active[i] {
                draw_cursor_indexed(buf, info, state.cursor_x[i], state.cursor_y[i], i, preset);
            }
        }
    }

    fn sync_last_from_state(&mut self, state: &UiState) {
        for i in 0..MAX_CURSORS {
            self.last_active[i] = state.cursor_active[i];
            self.last_x[i] = state.cursor_x[i];
            self.last_y[i] = state.cursor_y[i];
        }
    }

    /// After a full UI paint (no cursors drawn yet): save-under and draw all active cursors.
    pub fn prime_cursors(&mut self, buf: &mut [u8], info: &FrameBufferInfo, state: &UiState) {
        for i in 0..MAX_CURSORS {
            if state.cursor_active[i] {
                self.save_one(buf, info, i, state.cursor_x[i], state.cursor_y[i]);
            }
        }
        self.draw_all_cursors(buf, info, state);
        self.sync_last_from_state(state);
    }

    /// Restore old cursor damage, update to new positions (save, draw).
    pub fn patch_cursors_only(&mut self, buf: &mut [u8], info: &FrameBufferInfo, state: &UiState) {
        // Highest index first: each save-under was captured after lower-index cursors were drawn.
        for i in (0..MAX_CURSORS).rev() {
            if self.last_active[i] {
                self.restore_one(buf, info, i);
            }
        }
        for i in 0..MAX_CURSORS {
            if state.cursor_active[i] {
                self.save_one(buf, info, i, state.cursor_x[i], state.cursor_y[i]);
            }
        }
        self.draw_all_cursors(buf, info, state);
        self.sync_last_from_state(state);
    }
}

fn fill_rect(
    buf: &mut [u8],
    info: &FrameBufferInfo,
    x0: usize,
    y0: usize,
    rw: usize,
    rh: usize,
    r: u8,
    g: u8,
    b: u8,
) {
    let x1 = (x0 + rw).min(info.width);
    let y1 = (y0 + rh).min(info.height);
    for y in y0.min(info.height)..y1 {
        for x in x0.min(info.width)..x1 {
            pixel(buf, info, x, y, r, g, b);
        }
    }
}

/// Card and primary button placement for the photosensitivity notice (static art only).
fn epilepsy_notice_geometry(w: usize, h: usize) -> (usize, usize, usize, usize, usize, usize, usize, usize) {
    let card_w = (w * 3 / 4).clamp(320, 620);
    const LINE_H: usize = 13;
    let body_lines = 9usize;
    let btn_h = 34usize;
    let btn_pad = 22usize;
    let top_area = 52usize;
    let card_h = (top_area + body_lines.saturating_mul(LINE_H) + btn_pad + btn_h + 36)
        .min(h.saturating_sub(36))
        .max(230);
    let card_x = w.saturating_sub(card_w) / 2;
    let card_y = h.saturating_sub(card_h) / 2;
    let btn_w = (card_w.saturating_sub(56)).min(340).max(200);
    let btn_x = card_x + (card_w - btn_w) / 2;
    let btn_y = card_y + card_h - btn_h - btn_pad;
    (card_x, card_y, card_w, card_h, btn_x, btn_y, btn_w, btn_h)
}

fn draw_epilepsy_warning(
    buf: &mut [u8],
    info: &FrameBufferInfo,
    _state: &UiState,
    font: &[[u8; 5]; 59],
) {
    let w = info.width;
    let h = info.height;
    const LINE_H: usize = 13;
    let bands = 6usize;
    for b in 0..bands {
        let y0 = b * h / bands;
        let y1 = (b + 1) * h / bands;
        let t = (b as u32).saturating_mul(20) / bands as u32;
        let r = (26u32 + t) as u8;
        let g = (30u32 + t) as u8;
        let bl = (48u32 + t.saturating_mul(2)) as u8;
        fill_rect(buf, info, 0, y0, w, y1.saturating_sub(y0), r, g, bl);
    }
    let (cx, cy, cw, ch, bx, by, bw, bh) = epilepsy_notice_geometry(w, h);
    fill_rect(buf, info, cx + 5, cy + 6, cw, ch, 0x12, 0x16, 0x24);
    fill_rect(buf, info, cx, cy, cw, ch, 0xf4, 0xf6, 0xfc);
    let br = 3usize;
    fill_rect(buf, info, cx, cy, cw, br, 0xd4, 0xa8, 0x32);
    fill_rect(buf, info, cx, cy + ch.saturating_sub(br), cw, br, 0xd4, 0xa8, 0x32);
    fill_rect(buf, info, cx, cy, br, ch, 0xd4, 0xa8, 0x32);
    fill_rect(buf, info, cx + cw.saturating_sub(br), cy, br, ch, 0xd4, 0xa8, 0x32);
    let inb = 6usize;
    if cw > inb * 2 {
        fill_rect(buf, info, cx + inb, cy + inb, cw - inb * 2, 1, 0xe8, 0xdc, 0xb0);
    }
    let mut y = cy + 22;
    let tx = cx + 20;
    draw_str_rgb(
        buf,
        info,
        tx,
        y,
        b"PHOTOSENSITIVE  EPILEPSY  WARNING",
        font,
        0x8b,
        0x45,
        0x10,
    );
    y += 16;
    draw_str_rgb(
        buf,
        info,
        tx,
        y,
        b"READ  CAREFULLY  BEFORE  USE",
        font,
        0x55,
        0x55,
        0x66,
    );
    y += 22;
    draw_str_rgb(
        buf,
        info,
        tx,
        y,
        b"EVE  SHOWS  WEB  PAGES  BRIGHT  UI",
        font,
        0x22,
        0x22,
        0x33,
    );
    y += LINE_H;
    draw_str_rgb(
        buf,
        info,
        tx,
        y,
        b"SCROLLING  CURSORS  AND  MOTION  THAT",
        font,
        0x22,
        0x22,
        0x33,
    );
    y += LINE_H;
    draw_str_rgb(
        buf,
        info,
        tx,
        y,
        b"MAY  FLICKER  ON  SOME  DISPLAYS.",
        font,
        0x22,
        0x22,
        0x33,
    );
    y += LINE_H;
    draw_str_rgb(
        buf,
        info,
        tx,
        y,
        b"IF  YOU  HAVE  EPILEPSY  OR  ARE",
        font,
        0x22,
        0x22,
        0x33,
    );
    y += LINE_H;
    draw_str_rgb(
        buf,
        info,
        tx,
        y,
        b"PHOTOSENSITIVE  ASK  A  DOCTOR  FIRST.",
        font,
        0x22,
        0x22,
        0x33,
    );
    y += LINE_H;
    draw_str_rgb(
        buf,
        info,
        tx,
        y,
        b"BY  CONTINUING  YOU  ACCEPT  THIS  RISK.",
        font,
        0x44,
        0x33,
        0x33,
    );
    fill_rect(buf, info, bx, by, bw, bh, 0xc9, 0x7a, 0x1e);
    if bw > 4 && bh > 4 {
        fill_rect(
            buf,
            info,
            bx + 2,
            by + 2,
            bw - 4,
            bh - 4,
            0xe5,
            0xa0,
            0x38,
        );
    }
    let label = b"CONTINUE  TO  EVE  OS";
    let lw = label.len().saturating_mul(6);
    let lx = bx + bw.saturating_sub(lw) / 2;
    let ly = by + bh.saturating_sub(7) / 2;
    draw_str_rgb(buf, info, lx, ly, label, font, 0xff, 0xff, 0xf5);
    let hint = b"ENTER   SPACE   OR  CLICK  BUTTON";
    let hw = hint.len().saturating_mul(6);
    let hx = cx + cw.saturating_sub(hw) / 2;
    let hy = cy + ch.saturating_sub(14);
    draw_str_rgb(buf, info, hx, hy, hint, font, 0x77, 0x77, 0x88);
}

/// Leave `Screen::EpilepsyWarning` for `screen_after_epilepsy_notice` (browser or disk install).
pub fn dismiss_epilepsy_notice(state: &mut UiState) {
    if state.screen != Screen::EpilepsyWarning {
        return;
    }
    state.screen = state.screen_after_epilepsy_notice;
    state.content_dirty = true;
}

#[allow(dead_code)]
fn fill_circle(
    buf: &mut [u8],
    info: &FrameBufferInfo,
    cx: usize,
    cy: usize,
    rad: usize,
    r: u8,
    g: u8,
    b: u8,
) {
    let r2 = (rad * rad) as isize;
    let cx = cx as isize;
    let cy = cy as isize;
    let rr = rad as isize;
    for dy in -rr..=rr {
        for dx in -rr..=rr {
            if dx * dx + dy * dy > r2 {
                continue;
            }
            let nx = cx + dx;
            let ny = cy + dy;
            if nx < 0 || ny < 0 {
                continue;
            }
            pixel(buf, info, nx as usize, ny as usize, r, g, b);
        }
    }
}

fn font_index(ch: u8) -> Option<usize> {
    match ch {
        32..=90 => Some((ch - 32) as usize),
        b'a'..=b'z' => Some((ch.to_ascii_uppercase() - 32) as usize),
        _ => None,
    }
}

fn draw_str_rgb(
    buf: &mut [u8],
    info: &FrameBufferInfo,
    mut x: usize,
    y: usize,
    s: &[u8],
    font: &[[u8; 5]; 59],
    r: u8,
    g: u8,
    b: u8,
) {
    for &ch in s {
        if let Some(idx) = font_index(ch) {
            if idx >= font.len() {
                continue;
            }
            let glyph = &font[idx];
            for col in 0..5 {
                let bits = glyph[col];
                for row in 0..7 {
                    if bits & (1 << row) != 0 {
                        pixel(buf, info, x + col, y + row, r, g, b);
                    }
                }
            }
        }
        x = x.saturating_add(6);
        if x + 5 >= info.width {
            break;
        }
    }
}

pub fn draw_str(
    buf: &mut [u8],
    info: &FrameBufferInfo,
    x: usize,
    y: usize,
    s: &[u8],
    font: &[[u8; 5]; 59],
) {
    draw_str_rgb(buf, info, x, y, s, font, 0x22, 0x22, 0x22);
}

fn draw_hex_byte(
    buf: &mut [u8],
    info: &FrameBufferInfo,
    mut x: usize,
    y: usize,
    v: u8,
    font: &[[u8; 5]; 59],
    r: u8,
    g: u8,
    b: u8,
) {
    const H: &[u8; 16] = b"0123456789ABCDEF";
    let hi = H[(v >> 4) as usize];
    let lo = H[(v & 0x0F) as usize];
    draw_str_rgb(buf, info, x, y, &[hi], font, r, g, b);
    x = x.saturating_add(6);
    draw_str_rgb(buf, info, x, y, &[lo], font, r, g, b);
}

fn draw_hex_u16(
    buf: &mut [u8],
    info: &FrameBufferInfo,
    mut x: usize,
    y: usize,
    v: u16,
    font: &[[u8; 5]; 59],
    r: u8,
    g: u8,
    b: u8,
) {
    draw_hex_byte(buf, info, x, y, (v >> 8) as u8, font, r, g, b);
    x = x.saturating_add(12);
    draw_hex_byte(buf, info, x, y, v as u8, font, r, g, b);
}

fn draw_decimal(
    buf: &mut [u8],
    info: &FrameBufferInfo,
    x: usize,
    y: usize,
    mut n: u32,
    font: &[[u8; 5]; 59],
    r: u8,
    g: u8,
    b: u8,
) {
    if n == 0 {
        draw_str_rgb(buf, info, x, y, b"0", font, r, g, b);
        return;
    }
    let mut tmp = [0u8; 10];
    let mut i = tmp.len();
    while n > 0 && i > 0 {
        i -= 1;
        tmp[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    draw_str_rgb(buf, info, x, y, &tmp[i..], font, r, g, b);
}

fn draw_chrome_and_tabs(
    buf: &mut [u8],
    info: &FrameBufferInfo,
    lay: &Layout,
    state: &UiState,
    font: &[[u8; 5]; 59],
) {
    let w = lay.w;
    let chrome_h = lay.chrome_h;
    // TempleOS-like title bar: bright blue + yellow caption.
    fill_rect(buf, info, 0, 0, w, chrome_h, 0x44, 0x88, 0xff);
    let title_y = chrome_h / 2 + 4;
    draw_str_rgb(
        buf,
        info,
        12,
        title_y.saturating_sub(8),
        b"EVE OS   TEMPLE STYLE RING0",
        font,
        0xff,
        0xee,
        0x44,
    );

    let tab_y = lay.tab_y;
    let tab_h = lay.tab_h;
    if tab_y + tab_h < lay.h {
        fill_rect(buf, info, 0, tab_y, w, tab_h, 0xa8, 0xcc, 0xff);
        let eve_on = state.screen == Screen::Browser;
        let set_on = state.screen == Screen::Settings;
        let ins_on = state.screen == Screen::DiskInstall;
        fill_rect(
            buf,
            info,
            TAB_EVE_X,
            tab_y + 4,
            TAB_EVE_W,
            tab_h - 8,
            if eve_on { 0xff } else { 0xd0 },
            if eve_on { 0xff } else { 0xe4 },
            if eve_on { 0xff } else { 0xf8 },
        );
        fill_rect(
            buf,
            info,
            TAB_SET_X,
            tab_y + 4,
            TAB_SET_W,
            tab_h - 8,
            if set_on { 0xff } else { 0xd0 },
            if set_on { 0xff } else { 0xe4 },
            if set_on { 0xff } else { 0xf8 },
        );
        draw_str(
            buf,
            info,
            24,
            tab_y + (tab_h / 2).saturating_sub(4),
            b"SHRINE",
            font,
        );
        draw_str(
            buf,
            info,
            TAB_SET_X + 12,
            tab_y + (tab_h / 2).saturating_sub(4),
            b"SYS",
            font,
        );
        if state.disk_install_available {
            fill_rect(
                buf,
                info,
                TAB_INS_X,
                tab_y + 4,
                TAB_INS_W,
                tab_h - 8,
                if ins_on { 0xff } else { 0xd0 },
                if ins_on { 0xff } else { 0xe4 },
                if ins_on { 0xff } else { 0xf8 },
            );
            draw_str(
                buf,
                info,
                TAB_INS_X + 8,
                tab_y + (tab_h / 2).saturating_sub(4),
                b"INSTALL",
                font,
            );
        }
    }
}

fn draw_url_bar(
    buf: &mut [u8],
    info: &FrameBufferInfo,
    lay: &Layout,
    state: &UiState,
    font: &[[u8; 5]; 59],
) {
    let w = lay.w;
    let bar_y = lay.bar_y;
    let bar_h = lay.bar_h;
    if bar_y + bar_h >= lay.h {
        return;
    }
    fill_rect(buf, info, 0, bar_y, w, bar_h, 0xc8, 0xe0, 0xff);
    let btn = url_bar_btn_width(w);
    let text_y = bar_y + (bar_h / 2).saturating_sub(4);
    for (i, label) in [(0usize, b"<"), (1, b">"), (2, b"R")].iter() {
        let x0 = 12 + i * (btn + 8);
        fill_rect(buf, info, x0, bar_y + 6, btn, bar_h - 12, 0xff, 0xff, 0xff);
        draw_str(
            buf,
            info,
            x0 + btn / 2 - 3,
            text_y,
            *label,
            font,
        );
    }
    for (i, label) in [(3usize, &b"HOME"[..]), (4, &b"GO"[..])] {
        let x0 = 12 + i * (btn + 8);
        fill_rect(buf, info, x0, bar_y + 6, btn, bar_h - 12, 0xff, 0xff, 0xff);
        let tw = label.len().saturating_mul(6);
        let tx = x0 + btn.saturating_sub(tw) / 2;
        draw_str(buf, info, tx, text_y, label, font);
    }
    let url_x = url_bar_url_x0(w);
    if url_x + 40 < w {
        fill_rect(
            buf,
            info,
            url_x,
            bar_y + 6,
            w - url_x - 12,
            bar_h - 12,
            0xff,
            0xff,
            0xff,
        );
        let text_y = bar_y + (bar_h / 2).saturating_sub(4);
        if state.url_len > 0 {
            draw_str(
                buf,
                info,
                url_x + 8,
                text_y,
                &state.url[..state.url_len],
                font,
            );
        }
    }
}

fn draw_install_top_strip(
    buf: &mut [u8],
    info: &FrameBufferInfo,
    lay: &Layout,
    font: &[[u8; 5]; 59],
) {
    let w = lay.w;
    let bar_y = lay.bar_y;
    let bar_h = lay.bar_h;
    if bar_y + bar_h >= lay.h {
        return;
    }
    fill_rect(buf, info, 0, bar_y, w, bar_h, 0xc8, 0xe8, 0xd8);
    let text_y = bar_y + (bar_h / 2).saturating_sub(4);
    draw_str(
        buf,
        info,
        12,
        text_y,
        b"VIRTIO DISK 1  ->  DISK 2  (ONE CLICK INSTALL)",
        font,
    );
}

fn disk_install_button_rect(lay: &Layout) -> (usize, usize, usize, usize) {
    let w = lay.w;
    let bw = w.saturating_sub(80).min(440);
    let bh = 52usize;
    let bx = w.saturating_sub(bw) / 2;
    let by = lay.content_top + 168;
    (bx, by, bw, bh)
}

fn draw_disk_install_body(
    buf: &mut [u8],
    info: &FrameBufferInfo,
    lay: &Layout,
    state: &UiState,
    font: &[[u8; 5]; 59],
) {
    let w = lay.w;
    let h = lay.h;
    let content_top = lay.content_top;
    if content_top + 280 >= h {
        return;
    }
    fill_rect(
        buf,
        info,
        24,
        content_top,
        w.saturating_sub(48),
        h - content_top - 48,
        0xe8,
        0xf4,
        0xe8,
    );
    let mut y = content_top + 16;
    draw_str(
        buf,
        info,
        40,
        y,
        b"INSTALL EVE TO INTERNAL DISK",
        font,
    );
    y += 22;
    draw_str(
        buf,
        info,
        40,
        y,
        b"COPIES BOOT DISK SECTORS TO 2ND VIRTIO DISK",
        font,
    );
    y += 22;
    draw_str(buf, info, 40, y, b"TARGET DISK IS FULLY ERASED", font);
    y += 28;
    draw_str(buf, info, 40, y, b"SECTORS TO COPY:", font);
    let tot_show = (state.disk_install_total.min(u32::MAX as u64)) as u32;
    draw_decimal(
        buf,
        info,
        200,
        y,
        tot_show,
        font,
        0x22,
        0x22,
        0x22,
    );
    y += 22;
    match state.disk_install_phase {
        DiskInstallPhase::Idle | DiskInstallPhase::Running => {
            if state.disk_install_phase == DiskInstallPhase::Running {
                draw_str(buf, info, 40, y, b" COPYING...", font);
                y += 20;
                let bar_w = w.saturating_sub(120).min(520);
                let bx = 60usize;
                fill_rect(buf, info, bx, y, bar_w, 14, 0xcc, 0xcc, 0xcc);
                if state.disk_install_total > 0 {
                    let num = state
                        .disk_install_cur
                        .saturating_mul(bar_w as u64)
                        / state.disk_install_total;
                    let fw = (num as usize).min(bar_w);
                    if fw > 0 {
                        fill_rect(buf, info, bx, y, fw, 14, 0x22, 0xaa, 0x44);
                    }
                }
            }
            let (bx, by, bw, bh) = disk_install_button_rect(lay);
            if state.disk_install_phase == DiskInstallPhase::Idle {
                fill_rect(buf, info, bx, by, bw, bh, 0x44, 0xcc, 0x44);
                let tw = 7 * 6;
                draw_str(
                    buf,
                    info,
                    bx + bw / 2 - tw / 2,
                    by + bh / 2 - 4,
                    b"INSTALL",
                    font,
                );
            }
        }
        DiskInstallPhase::Done => {
            draw_str(
                buf,
                info,
                40,
                y,
                b"DONE  REBOOT AND BOOT FROM DISK 2",
                font,
            );
        }
        DiskInstallPhase::Failed => {
            draw_str(buf, info, 40, y, b"FAILED", font);
            y += 20;
            let n = state.disk_install_err_len.min(state.disk_install_err.len());
            if n > 0 {
                draw_str(buf, info, 40, y, &state.disk_install_err[..n], font);
            }
        }
    }
}

/// First clickable settings row top Y (must match `draw_settings_body`).
fn settings_first_row_y(content_top: usize) -> usize {
    content_top + 12 + 20 + 14
}

const SETTINGS_ROW_H: usize = 22;
const SETTINGS_GAP: usize = 4;
const SETTINGS_SEC_SKIP: usize = 18;
const POWER_BTN_H: usize = 46;
const POWER_BTN_GAP: usize = 18;
const POWER_STRIP_PAD_TOP: usize = 20;

/// Y immediately below the last SYS settings row (USB MIDI), before the power strip.
fn settings_y_after_all_rows(state: &UiState, content_top: usize) -> usize {
    let mut y = settings_first_row_y(content_top);
    y += SETTINGS_ROW_H + SETTINGS_GAP;
    y += SETTINGS_ROW_H + SETTINGS_GAP;
    y += (SETTINGS_ROW_H + SETTINGS_GAP) * 3;
    if state.wifi_scan_demo {
        y += 12;
    }
    y += SETTINGS_ROW_H + SETTINGS_GAP;
    y += SETTINGS_ROW_H + SETTINGS_GAP;
    y += SETTINGS_ROW_H + SETTINGS_GAP;
    y += SETTINGS_ROW_H + SETTINGS_GAP;
    y += SETTINGS_ROW_H + SETTINGS_GAP;
    y += SETTINGS_SEC_SKIP;
    y += SETTINGS_ROW_H + SETTINGS_GAP;
    y += SETTINGS_ROW_H + SETTINGS_GAP;
    y += SETTINGS_SEC_SKIP;
    y += SETTINGS_ROW_H + SETTINGS_GAP;
    y += SETTINGS_SEC_SKIP;
    y += SETTINGS_ROW_H + SETTINGS_GAP;
    y += SETTINGS_ROW_H + 10;
    y
}

/// `(reboot_rect, shutdown_rect)` as `(x, y, w, h)`.
pub fn settings_power_button_rects(
    state: &UiState,
    lay: &Layout,
) -> ((usize, usize, usize, usize), (usize, usize, usize, usize)) {
    let w = lay.w;
    let y_rows_end = settings_y_after_all_rows(state, lay.content_top);
    let y_btn = y_rows_end + POWER_STRIP_PAD_TOP + 16;
    let margin = 40usize;
    let inner = w.saturating_sub(margin * 2);
    let btn_w = inner.saturating_sub(POWER_BTN_GAP) / 2;
    let btn_w = btn_w.max(120);
    let bx0 = margin;
    let bx1 = margin + btn_w + POWER_BTN_GAP;
    (
        (bx0, y_btn, btn_w, POWER_BTN_H),
        (bx1, y_btn, btn_w, POWER_BTN_H),
    )
}

#[inline]
fn lerp_chan(a: u8, b: u8, i: u8, n: u8) -> u8 {
    if n == 0 {
        return a;
    }
    let a = i32::from(a);
    let b = i32::from(b);
    let v = a + (b - a) * i32::from(i) / i32::from(n);
    v.clamp(0, 255) as u8
}

fn draw_luxe_button(
    buf: &mut [u8],
    info: &FrameBufferInfo,
    x: usize,
    y: usize,
    bw: usize,
    bh: usize,
    label: &[u8],
    font: &[[u8; 5]; 59],
    r_top: u8,
    g_top: u8,
    b_top: u8,
    r_bot: u8,
    g_bot: u8,
    b_bot: u8,
    accent_r: u8,
    accent_g: u8,
    accent_b: u8,
) {
    if bw < 8 || bh < 12 {
        return;
    }
    fill_rect(
        buf,
        info,
        x + 3,
        y + 4,
        bw,
        bh,
        0x1a,
        0x1e,
        0x28,
    );
    let bands = 10usize.max(bh / 4);
    let n = (bands as u8).saturating_sub(1).max(1);
    for i in 0..bands {
        let yy = y + (i * bh) / bands;
        let next = y + ((i + 1) * bh) / bands;
        let hh = next.saturating_sub(yy).max(1);
        let ii = i as u8;
        let rr = lerp_chan(r_top, r_bot, ii, n);
        let gg = lerp_chan(g_top, g_bot, ii, n);
        let bb = lerp_chan(b_top, b_bot, ii, n);
        fill_rect(buf, info, x, yy, bw, hh, rr, gg, bb);
    }
    let accent_w = 5.min(bw);
    fill_rect(buf, info, x, y, accent_w, bh, accent_r, accent_g, accent_b);
    let inset = accent_w + 1;
    if bw > inset + 2 {
        fill_rect(
            buf,
            info,
            x + inset,
            y + 1,
            bw.saturating_sub(inset),
            1,
            0xff,
            0xff,
            0xff,
        );
    }
    if bh > 2 && bw > inset + 2 {
        fill_rect(
            buf,
            info,
            x + inset,
            y + bh - 1,
            bw.saturating_sub(inset),
            1,
            lerp_chan(r_bot, 0, 1, 3),
            lerp_chan(g_bot, 0, 1, 3),
            lerp_chan(b_bot, 0, 1, 3),
        );
    }
    let lw = label.len().saturating_mul(6);
    let lx = x + bw / 2 - lw / 2;
    let ly = y + bh / 2 - 4;
    draw_str_rgb(buf, info, lx, ly, label, font, 0xf8, 0xfc, 0xff);
}

/// Stub “scan”: no radio driver; fills sample SSIDs the user can tap to copy.
pub fn wifi_demo_scan(state: &mut UiState) {
    state.wifi_scan_demo = true;
    state.wifi_scan_count = 3;
    const NAMES: [&[u8]; 3] = [b"HOMENET", b"OFFICE-AP", b"GUEST-WIFI"];
    for i in 0..3 {
        let n = NAMES[i].len().min(32);
        state.wifi_scan_names[i][..n].copy_from_slice(&NAMES[i][..n]);
        state.wifi_scan_lens[i] = n as u8;
    }
}

/// Pill-style ON/OFF for SYS rows (right-aligned).
fn draw_settings_toggle(
    buf: &mut [u8],
    info: &FrameBufferInfo,
    x: usize,
    y: usize,
    on: bool,
    font: &[[u8; 5]; 59],
) {
    const TW: usize = 44;
    const TH: usize = 18;
    let (br, bg, bb) = if on {
        (0x16u8, 0xa3u8, 0x4au8)
    } else {
        (0x64u8, 0x74u8, 0x8bu8)
    };
    fill_rect(
        buf,
        info,
        x,
        y,
        TW,
        TH,
        br.saturating_sub(0x12),
        bg.saturating_sub(0x12),
        bb.saturating_sub(0x12),
    );
    fill_rect(buf, info, x + 1, y + 1, TW - 2, TH - 2, br, bg, bb);
    fill_rect(buf, info, x + 2, y + 2, TW - 4, 1, 0xee, 0xf8, 0xf0);
    let text: &[u8] = if on { b"ON" } else { b"OFF" };
    let lx = x + (TW - text.len().saturating_mul(6)) / 2;
    let ly = y + TH / 2 - 3;
    draw_str_rgb(buf, info, lx, ly, text, font, 0xff, 0xff, 0xff);
}

#[inline]
fn draw_section_tag(
    buf: &mut [u8],
    info: &FrameBufferInfo,
    x: usize,
    y: usize,
    label: &[u8],
    font: &[[u8; 5]; 59],
    r: u8,
    g: u8,
    b: u8,
) {
    fill_rect(buf, info, x, y + 3, 3, 10, r, g, b);
    draw_str_rgb(buf, info, x + 8, y, label, font, r, g, b);
}

fn draw_settings_body(
    buf: &mut [u8],
    info: &FrameBufferInfo,
    lay: &Layout,
    state: &UiState,
    font: &[[u8; 5]; 59],
) {
    let w = lay.w;
    let h = lay.h;
    let content_top = lay.content_top;
    if content_top + 72 >= h {
        return;
    }
    let panel_w = w.saturating_sub(48);
    let panel_h = h.saturating_sub(content_top).saturating_sub(48);
    fill_rect(buf, info, 24, content_top, panel_w, panel_h, 0xec, 0xf2, 0xfa);
    fill_rect(buf, info, 24, content_top, 4, panel_h, 0x3b, 0x82, 0xf6);
    fill_rect(
        buf,
        info,
        28,
        content_top,
        panel_w.saturating_sub(4),
        1,
        0xfe,
        0xfc,
        0xff,
    );

    let mut y = content_top + 12;
    draw_str_rgb(buf, info, 36, y, b"EVE SETTINGS", font, 0x0f, 0x17, 0x2e);
    fill_rect(
        buf,
        info,
        36,
        y + 14,
        (panel_w - 24).min(340),
        2,
        0x3b,
        0x82,
        0xf6,
    );
    y += 20;
    draw_section_tag(buf, info, 32, y, b"NETWORK", font, 0x1e, 0x40, 0xad);
    y += 14;

    const ROW_H: usize = 22;
    const GAP: usize = 4;
    let mut row_idx = 0u32;
    let mut row_bg = |buf: &mut [u8], ry: usize| {
        let alt = row_idx % 2 == 1;
        row_idx += 1;
        let (r, g, b) = if alt {
            (0xf1, 0xf5, 0xf9)
        } else {
            (0xf8, 0xfa, 0xfc)
        };
        fill_rect(buf, info, 36, ry, w.saturating_sub(72), ROW_H, r, g, b);
        fill_rect(
            buf,
            info,
            36,
            ry + ROW_H - 1,
            w.saturating_sub(72),
            1,
            0xe2,
            0xe8,
            0xf0,
        );
    };
    let right_x = w.saturating_sub(92).min(400);

    // 0: Wi‑Fi (PCI 802.11 probe; no iwlwifi/ath/rtl stack in-tree)
    row_bg(buf, y);
    draw_str(buf, info, 44, y + 6, b"WIFI", font);
    let hx = 120.min(w.saturating_sub(220));
    const HR: u8 = 0x22;
    const HG: u8 = 0x55;
    const HB: u8 = 0x22;
    if state.pci_wlan {
        draw_hex_u16(
            buf,
            info,
            hx,
            y + 6,
            state.wlan_first_vid,
            font,
            HR,
            HG,
            HB,
        );
        let mut x2 = hx + 24;
        draw_str_rgb(buf, info, x2, y + 6, b":", font, HR, HG, HB);
        x2 += 6;
        draw_hex_u16(
            buf,
            info,
            x2,
            y + 6,
            state.wlan_first_did,
            font,
            HR,
            HG,
            HB,
        );
        if state.wlan_pci_count > 1 {
            x2 += 24;
            draw_str_rgb(buf, info, x2, y + 6, b"+", font, HR, HG, HB);
        }
    } else {
        draw_str(buf, info, hx, y + 6, b"NO PCI 802.11", font);
    }
    draw_settings_toggle(buf, info, right_x, y + 2, state.settings.wifi_enabled, font);
    y += ROW_H + GAP;

    // Wi‑Fi scan (stub: no driver; fills sample SSIDs to tap)
    row_bg(buf, y);
    draw_str(buf, info, 44, y + 6, b"WIFI SCAN", font);
    draw_str(
        buf,
        info,
        200.min(w.saturating_sub(160)),
        y + 6,
        b"TAP TO RUN",
        font,
    );
    y += ROW_H + GAP;

    for slot in 0..3usize {
        row_bg(buf, y);
        draw_decimal(buf, info, 44, y + 6, (slot + 1) as u32, font, 0x22, 0x22, 0x22);
        draw_str(buf, info, 56, y + 6, b":", font);
        let n = state.wifi_scan_lens[slot] as usize;
        if n > 0 && n <= 32 {
            draw_str(buf, info, 68, y + 6, &state.wifi_scan_names[slot][..n], font);
        } else {
            draw_str(buf, info, 68, y + 6, b"--", font);
        }
        y += ROW_H + GAP;
    }
    if state.wifi_scan_demo {
        draw_str_rgb(
            buf,
            info,
            44,
            y,
            b"SAMPLES ONLY  NO 802.11 MAC DRIVER  USE VIRTIO NET IN QEMU",
            font,
            0xaa,
            0x33,
            0x22,
        );
        y += 12;
    }

    let ssid_focus = state.settings_text_focus == SettingsTextFocus::WifiSsid;
    let psk_focus = state.settings_text_focus == SettingsTextFocus::WifiPsk;
    if ssid_focus {
        fill_rect(
            buf,
            info,
            36,
            y,
            w.saturating_sub(72),
            ROW_H,
            0xe0,
            0xf8,
            0xff,
        );
    } else {
        row_bg(buf, y);
    }
    draw_str(buf, info, 44, y + 6, b"SSID", font);
    let sx = 120.min(w.saturating_sub(200));
    if state.settings.wifi_ssid_len > 0 {
        let n = state.settings.wifi_ssid_len.min(state.settings.wifi_ssid.len());
        draw_str(buf, info, sx, y + 6, &state.settings.wifi_ssid[..n], font);
    } else {
        draw_str(buf, info, sx, y + 6, b"(TYPE)", font);
    }
    y += ROW_H + GAP;

    row_bg(buf, y);
    draw_str(buf, info, 44, y + 6, b"SEC", font);
    draw_str(
        buf,
        info,
        120.min(w.saturating_sub(200)),
        y + 6,
        state.settings.wifi_sec.label(),
        font,
    );
    draw_str(buf, info, 260.min(w.saturating_sub(120)), y + 6, b"TAP", font);
    y += ROW_H + GAP;

    if psk_focus {
        fill_rect(
            buf,
            info,
            36,
            y,
            w.saturating_sub(72),
            ROW_H,
            0xe0,
            0xf8,
            0xff,
        );
    } else {
        row_bg(buf, y);
    }
    draw_str(buf, info, 44, y + 6, b"PSK", font);
    let px = 120.min(w.saturating_sub(200));
    let stars = state.settings.wifi_psk_len.min(24);
    for i in 0..stars {
        draw_str(buf, info, px + i * 6, y + 6, b"*", font);
    }
    if state.settings.wifi_psk_len == 0 {
        draw_str(buf, info, px, y + 6, b"(TYPE)", font);
    }
    y += ROW_H + GAP;

    // 1: Ethernet / NIC driver
    row_bg(buf, y);
    draw_str(buf, info, 44, y + 6, b"ETHERNET", font);
    let nx = 200.min(w.saturating_sub(200));
    match state.settings.nic {
        NicChoice::Virtio => draw_str(buf, info, nx, y + 6, b"VIRTIO", font),
        NicChoice::Rtl8139 => draw_str(buf, info, nx, y + 6, b"RTL8139", font),
        NicChoice::E1000 => draw_str(buf, info, nx, y + 6, b"E1000", font),
        NicChoice::Pcnet => draw_str(buf, info, nx, y + 6, b"PCNET", font),
        NicChoice::Off => draw_str(buf, info, nx, y + 6, b"OFF", font),
    }
    let mut ex = 300.min(w.saturating_sub(120));
    draw_str(buf, info, ex, y + 6, b"PCI", font);
    ex += 4 * 6;
    draw_decimal(
        buf,
        info,
        ex,
        y + 6,
        u32::from(state.pci_eth_count),
        font,
        0x22,
        0x22,
        0x22,
    );
    y += ROW_H + GAP;

    row_bg(buf, y);
    draw_str(buf, info, 44, y + 6, b"IP MODE", font);
    let pxm = 200.min(w.saturating_sub(160));
    draw_str(
        buf,
        info,
        pxm,
        y + 6,
        state.settings.ip_config.label(),
        font,
    );
    y += ROW_H + GAP;

    // 2: Internet stack (ARP/HTTP demo)
    row_bg(buf, y);
    draw_str(buf, info, 44, y + 6, b"INTERNET", font);
    let ix = 200.min(w.saturating_sub(160));
    if state.settings.internet_stack_enabled {
        draw_str(buf, info, ix, y + 6, b"TCP HTTP", font);
    } else {
        draw_str(buf, info, ix, y + 6, b"PAUSED", font);
    }
    draw_settings_toggle(
        buf,
        info,
        right_x,
        y + 2,
        state.settings.internet_stack_enabled,
        font,
    );
    y += ROW_H + GAP;

    y += 4;
    draw_section_tag(buf, info, 32, y, b"USB", font, 0x6d, 0x28, 0xd9);
    y += 14;

    // 3: USB host + polling stub
    row_bg(buf, y);
    draw_str(buf, info, 44, y + 6, b"USB HOST", font);
    draw_str(
        buf,
        info,
        188.min(w.saturating_sub(120)),
        y + 6,
        usb_hid::host_label(),
        font,
    );
    let mix = 268.min(w.saturating_sub(200));
    draw_str(buf, info, mix, y + 6, b"MICE", font);
    draw_decimal(
        buf,
        info,
        mix + 5 * 6,
        y + 6,
        usb_hid::usb_mouse_count() as u32,
        font,
        0x22,
        0x22,
        0x22,
    );
    draw_settings_toggle(
        buf,
        info,
        right_x,
        y + 2,
        state.settings.usb_polling_enabled,
        font,
    );
    y += ROW_H + GAP;

    row_bg(buf, y);
    draw_str(buf, info, 44, y + 6, b"EMOJI PTR", font);
    draw_str(
        buf,
        info,
        188.min(w.saturating_sub(120)),
        y + 6,
        cursor_emoji::label(state.settings.cursor_emoji_preset),
        font,
    );
    y += ROW_H + GAP;

    y += 4;
    draw_section_tag(buf, info, 32, y, b"WIRELESS", font, 0xc0, 0x25, 0x8a);
    y += 14;

    // 4: Bluetooth
    row_bg(buf, y);
    draw_str(buf, info, 44, y + 6, b"BLUETOOTH", font);
    draw_str(
        buf,
        info,
        200.min(w.saturating_sub(140)),
        y + 6,
        b"CLASSIC",
        font,
    );
    draw_settings_toggle(
        buf,
        info,
        right_x,
        y + 2,
        state.settings.bluetooth_enabled,
        font,
    );
    y += ROW_H + GAP;

    y += 4;
    draw_section_tag(
        buf,
        info,
        32,
        y,
        b"MIDI AND AUDIO",
        font,
        0x0d,
        0x94,
        0x88,
    );
    y += 14;

    // 5: MIDI core
    row_bg(buf, y);
    draw_str(buf, info, 44, y + 6, b"MIDI", font);
    let ax = 188.min(w.saturating_sub(160));
    if state.pci_mm_audio {
        draw_str(buf, info, ax, y + 6, b"HDA YES", font);
    } else {
        draw_str(buf, info, ax, y + 6, b"HDA NO", font);
    }
    draw_str(
        buf,
        info,
        280.min(w.saturating_sub(140)),
        y + 6,
        b"CH",
        font,
    );
    draw_decimal(
        buf,
        info,
        302.min(w.saturating_sub(120)),
        y + 6,
        u32::from(state.settings.midi_channel),
        font,
        0x22,
        0x22,
        0x22,
    );
    draw_settings_toggle(buf, info, right_x, y + 2, state.settings.midi_enabled, font);
    y += ROW_H + GAP;

    // 6: USB MIDI preference
    row_bg(buf, y);
    draw_str(buf, info, 44, y + 6, b"USB MIDI", font);
    draw_str(
        buf,
        info,
        200.min(w.saturating_sub(160)),
        y + 6,
        crate::usb_hid::usb_midi_status_label(),
        font,
    );
    draw_settings_toggle(
        buf,
        info,
        right_x,
        y + 2,
        state.settings.midi_usb_enabled,
        font,
    );
    y += ROW_H + 10;

    let y_power = y + POWER_STRIP_PAD_TOP;
    draw_str_rgb(
        buf,
        info,
        36,
        y_power,
        b"SYSTEM POWER",
        font,
        0x0f,
        0x17,
        0x2e,
    );
    fill_rect(
        buf,
        info,
        36,
        y_power + 14,
        (panel_w - 24).min(280),
        2,
        0xf4,
        0x3f,
        0x5e,
    );
    let ((rx, ry, rw, rh), (sx, sy, sw, sh)) = settings_power_button_rects(state, lay);
    draw_luxe_button(
        buf,
        info,
        rx,
        ry,
        rw,
        rh,
        b"REBOOT",
        font,
        0x0e,
        0xa5,
        0xa4,
        0x05,
        0x69,
        0x63,
        0x2d,
        0xd4,
        0xbf,
    );
    draw_luxe_button(
        buf,
        info,
        sx,
        sy,
        sw,
        sh,
        b"SHUTDOWN",
        font,
        0x52,
        0x62,
        0x7a,
        0x33,
        0x41,
        0x55,
        0xf4,
        0x3f,
        0x5e,
    );
    let y_hint = sy + sh + 20;
    draw_str_rgb(
        buf,
        info,
        40,
        y_hint,
        b"QEMU ACPI OFF  PS2 RESET  HARDWARE MAY VARY",
        font,
        0x64,
        0x74,
        0x8b,
    );
    let y_foot = y_hint + 14;
    draw_str(
        buf,
        info,
        36,
        y_foot,
        b"F1 SYS  F2 SHRINE  F3 MIDICH  CLICK ROW TO TOGGLE",
        font,
    );
}

fn page_glyph(ch: u8) -> u8 {
    match ch {
        b'\n' | b'\r' => b' ',
        b'\t' => b' ',
        32..=90 => ch,
        b'a'..=b'z' => ch.to_ascii_uppercase(),
        _ => b'.',
    }
}

fn draw_line_mapped_rgb(
    buf: &mut [u8],
    info: &FrameBufferInfo,
    x0: usize,
    y: usize,
    raw: &[u8],
    font: &[[u8; 5]; 59],
    r: u8,
    g: u8,
    b: u8,
) {
    let mut t = [0u8; 128];
    let n = raw.len().min(t.len());
    for i in 0..n {
        t[i] = page_glyph(raw[i]);
    }
    draw_str_rgb(buf, info, x0, y, &t[..n], font, r, g, b);
}

const BROWSER_LINE_H: usize = 10;

fn draw_browser_body(
    buf: &mut [u8],
    info: &FrameBufferInfo,
    lay: &Layout,
    state: &UiState,
    font: &[[u8; 5]; 59],
) {
    let w = lay.w;
    let h = lay.h;
    let content_top = lay.content_top;
    let bios = browser_bios_fullpage(state);
    if content_top + 40 >= h && !bios {
        return;
    }
    if bios {
        fill_rect(buf, info, 0, 0, w, h, 0xff, 0xff, 0xff);
    } else {
        fill_rect(
            buf,
            info,
            24,
            content_top,
            w.saturating_sub(48),
            h - content_top - 48,
            0xe8,
            0xf4,
            0xff,
        );
    }

    let x0 = if bios { 20usize } else { 48usize };
    let bottom_pad = if bios { 12usize } else { 48usize };
    let status_h = if bios { 0usize } else { 28usize };
    let y_max = h.saturating_sub(status_h + bottom_pad);
    let mut y = content_top + if bios { 16 } else { 12 };
    let scroll = state.page_scroll_line;

    if state.fetch_err_len > 0 {
        draw_str_rgb(
            buf,
            info,
            x0,
            y,
            &state.fetch_err[..state.fetch_err_len],
            font,
            0xcc,
            0x22,
            0x22,
        );
        y = y.saturating_add(BROWSER_LINE_H + 4);
    }

    let mut line_no = 0usize;
    for li in 0..state.browser_line_count.min(BROWSER_MAX_LINES) {
        if line_no >= scroll {
            if y + BROWSER_LINE_H > y_max {
                break;
            }
            if let Some(line) = html::browser_line(li) {
                if line.len > 0 {
                    let n = line.len.min(BROWSER_LINE_CAP);
                    draw_line_mapped_rgb(
                        buf,
                        info,
                        x0,
                        y,
                        &line.data[..n],
                        font,
                        line.r,
                        line.g,
                        line.b,
                    );
                }
            }
            y = y.saturating_add(BROWSER_LINE_H);
        }
        line_no += 1;
    }

    if state.page_truncated && y + BROWSER_LINE_H <= y_max {
        draw_str_rgb(
            buf,
            info,
            x0,
            y,
            b"[PAGE TRUNCATED]",
            font,
            0x88,
            0x44,
            0x22,
        );
    }

    if state.fetch_err_len == 0 && state.browser_line_count == 0 && y + BROWSER_LINE_H <= y_max {
        let hint: &[u8] = if bios {
            b"LOADING GOOGLE   F1 SETTINGS   F6 SHOW CHROME"
        } else {
            b"TYPE URL  ENTER  HOME  GO  R  ARROWS SCROLL"
        };
        draw_str(buf, info, x0, y, hint, font);
    }
}

fn draw_status_line(
    buf: &mut [u8],
    info: &FrameBufferInfo,
    lay: &Layout,
    state: &UiState,
    font: &[[u8; 5]; 59],
) {
    const W: u8 = 0xff;
    let dec_width = |n: u32| -> usize {
        if n == 0 {
            return 1;
        }
        let mut c = 0usize;
        let mut x = n;
        while x > 0 {
            c += 1;
            x /= 10;
        }
        c
    };

    let h = lay.h;
    let status_y = h.saturating_sub(28);
    let mut sx = 8usize;
    draw_str_rgb(buf, info, sx, status_y, b"NET", font, W, W, W);
    sx += 4 * 6;

    if state.net_ok {
        draw_str_rgb(buf, info, sx, status_y, b"MAC ", font, W, W, W);
        sx += 5 * 6;
        for (i, oct) in state.mac.iter().enumerate() {
            if i > 0 {
                draw_str_rgb(buf, info, sx, status_y, b":", font, W, W, W);
                sx += 6;
            }
            draw_hex_byte(buf, info, sx, status_y, *oct, font, W, W, W);
            sx += 12;
        }
        draw_str_rgb(buf, info, sx, status_y, b" RX", font, W, W, W);
        sx += 4 * 6;
        let mut n = state.net_rx;
        let mut tmp = [0u8; 20];
        let mut i = tmp.len();
        if n == 0 {
            i -= 1;
            tmp[i] = b'0';
        } else {
            while n > 0 && i > 0 {
                i -= 1;
                tmp[i] = b'0' + (n % 10) as u8;
                n /= 10;
            }
        }
        draw_str_rgb(buf, info, sx, status_y, &tmp[i..], font, W, W, W);
        sx += (tmp.len() - i) * 6;
        draw_str_rgb(buf, info, sx, status_y, b" I", font, W, W, W);
        sx += 2 * 6;
        let lab: &[u8] = match state.inet_phase {
            NetPhase::Off => b"--",
            NetPhase::Arp => b"ARP",
            NetPhase::Dns => b"DNS",
            NetPhase::Tcp => b"TCP",
            NetPhase::Http => b"GET",
            NetPhase::Done => b"WWW",
        };
        draw_str_rgb(buf, info, sx, status_y, lab, font, W, W, W);
        sx += lab.len() * 6;
        draw_str_rgb(buf, info, sx, status_y, b" ", font, W, W, W);
        sx += 6;
        draw_decimal(
            buf,
            info,
            sx,
            status_y,
            state.inet_bytes,
            font,
            W,
            W,
            W,
        );
        sx += dec_width(state.inet_bytes) * 6;
    } else if state.pci_eth_count > 0 {
        // PCI Ethernet class device present, but only VirtIO net is driven — typical on bare metal.
        draw_str_rgb(buf, info, sx, status_y, b": NODRV", font, W, W, W);
        sx += 7 * 6;
    } else {
        draw_str_rgb(buf, info, sx, status_y, b": OFF", font, W, W, W);
        sx += 5 * 6;
    }

    draw_str_rgb(buf, info, sx, status_y, b"  M", font, W, W, W);
    sx += 4 * 6;
    if state.settings.usb_polling_enabled {
        let mc = usb_hid::usb_mouse_count() as u32;
        draw_decimal(buf, info, sx, status_y, mc, font, W, W, W);
        sx += dec_width(mc) * 6;
    } else {
        draw_str_rgb(buf, info, sx, status_y, b"--", font, W, W, W);
        sx += 2 * 6;
    }

    draw_str_rgb(buf, info, sx, status_y, b" MID", font, W, W, W);
    sx += 4 * 6;
    if state.settings.midi_enabled {
        draw_str_rgb(buf, info, sx, status_y, b"ON", font, W, W, W);
    } else {
        draw_str_rgb(buf, info, sx, status_y, b"OFF", font, W, W, W);
    }
}

fn paint_ui(
    buf: &mut [u8],
    info: &FrameBufferInfo,
    lay: &Layout,
    state: &UiState,
    font: &[[u8; 5]; 59],
) {
    if state.screen == Screen::EpilepsyWarning {
        draw_epilepsy_warning(buf, info, state, font);
        return;
    }
    if browser_bios_fullpage(state) {
        clear(buf, info, 0xff, 0xff, 0xff);
        draw_browser_body(buf, info, lay, state, font);
        return;
    }
    clear(buf, info, 0xd0, 0xe4, 0xff);
    draw_chrome_and_tabs(buf, info, lay, state, font);
    match state.screen {
        Screen::EpilepsyWarning => {}
        Screen::DiskInstall => draw_install_top_strip(buf, info, lay, font),
        _ => draw_url_bar(buf, info, lay, state, font),
    }
    match state.screen {
        Screen::EpilepsyWarning => {}
        Screen::Browser => draw_browser_body(buf, info, lay, state, font),
        Screen::Settings => draw_settings_body(buf, info, lay, state, font),
        Screen::DiskInstall => draw_disk_install_body(buf, info, lay, state, font),
    }
    draw_status_line(buf, info, lay, state, font);
}

fn redraw_status_strip(
    buf: &mut [u8],
    info: &FrameBufferInfo,
    lay: &Layout,
    state: &UiState,
    font: &[[u8; 5]; 59],
) {
    let h = lay.h;
    let status_y = h.saturating_sub(28);
    fill_rect(buf, info, 0, status_y, lay.w, 28, 0x38, 0x6c, 0xc8);
    draw_status_line(buf, info, lay, state, font);
}

/// Full / partial compositor: avoids clearing the whole framebuffer every frame (reduces flicker).
pub fn render_frame(
    buf: &mut [u8],
    info: &FrameBufferInfo,
    state: &mut UiState,
    font: &[[u8; 5]; 59],
    eng: &mut CursorEngine,
) {
    if eng.initialized
        && !state.content_dirty
        && !state.chrome_only_dirty
        && !state.browser_body_dirty
        && !state.status_dirty
        && !eng.any_cursor_moved(state)
    {
        return;
    }

    let lay = state.layout(info);

    if state.content_dirty {
        eng.invalidate_all_saves();
        paint_ui(buf, info, &lay, state, font);
        eng.prime_cursors(buf, info, state);
        eng.initialized = true;
        state.content_dirty = false;
        state.chrome_only_dirty = false;
        state.browser_body_dirty = false;
        state.status_dirty = false;
        return;
    }

    if state.chrome_only_dirty {
        if eng.initialized {
            for i in (0..MAX_CURSORS).rev() {
                if eng.last_active[i] {
                    eng.restore_one(buf, info, i);
                }
            }
        }
        eng.invalidate_all_saves();
        if state.screen == Screen::EpilepsyWarning {
            draw_epilepsy_warning(buf, info, state, font);
        } else if browser_bios_fullpage(state) {
            clear(buf, info, 0xff, 0xff, 0xff);
            draw_browser_body(buf, info, &lay, state, font);
        } else {
            draw_chrome_and_tabs(buf, info, &lay, state, font);
            match state.screen {
                Screen::EpilepsyWarning => {}
                Screen::DiskInstall => draw_install_top_strip(buf, info, &lay, font),
                _ => draw_url_bar(buf, info, &lay, state, font),
            }
            draw_status_line(buf, info, &lay, state, font);
        }
        eng.prime_cursors(buf, info, state);
        eng.initialized = true;
        state.chrome_only_dirty = false;
        state.browser_body_dirty = false;
        state.status_dirty = false;
        return;
    }

    if state.browser_body_dirty {
        if eng.initialized {
            for i in (0..MAX_CURSORS).rev() {
                if eng.last_active[i] {
                    eng.restore_one(buf, info, i);
                }
            }
        }
        eng.invalidate_all_saves();
        draw_browser_body(buf, info, &lay, state, font);
        eng.prime_cursors(buf, info, state);
        eng.initialized = true;
        state.browser_body_dirty = false;
        return;
    }

    if state.status_dirty {
        if browser_bios_fullpage(state) || state.screen == Screen::EpilepsyWarning {
            state.status_dirty = false;
        } else {
            for i in (0..MAX_CURSORS).rev() {
                if eng.last_active[i] {
                    eng.restore_one(buf, info, i);
                }
            }
            redraw_status_strip(buf, info, &lay, state, font);
            eng.invalidate_all_saves();
            eng.prime_cursors(buf, info, state);
            state.status_dirty = false;
            return;
        }
    }

    eng.patch_cursors_only(buf, info, state);
}

/// Index of URL bar button hit (`<` `>` `R` `HOME` `GO`), or `None`.
fn url_bar_button_hit(lay: &Layout, mx: usize, my: usize) -> Option<usize> {
    let w = lay.w;
    let bar_y = lay.bar_y;
    let bar_h = lay.bar_h;
    if bar_y + bar_h >= lay.h {
        return None;
    }
    let btn = url_bar_btn_width(w);
    if my < bar_y + 6 || my >= bar_y + bar_h.saturating_sub(6) {
        return None;
    }
    for i in 0..URL_BAR_BTN_COUNT {
        let x0 = 12 + i * (btn + 8);
        if mx >= x0 && mx < x0 + btn {
            return Some(i);
        }
    }
    None
}

/// Left button down at `(mx, my)`; returns true if a setting was toggled or tab switched.
pub fn handle_click_at(state: &mut UiState, info: &FrameBufferInfo, mx: usize, my: usize) -> bool {
    let lay = state.layout(info);

    if state.screen == Screen::EpilepsyWarning {
        let w = lay.w;
        let h = lay.h;
        let (_, _, _, _, bx, by, bw, bh) = epilepsy_notice_geometry(w, h);
        if mx >= bx && mx < bx + bw && my >= by && my < by + bh {
            dismiss_epilepsy_notice(state);
            return true;
        }
        return false;
    }

    let tab_y = lay.tab_y + 4;
    let tab_h = lay.tab_h - 8;
    if my >= tab_y && my < tab_y + tab_h {
        if mx >= TAB_EVE_X && mx < TAB_EVE_X + TAB_EVE_W {
            state.screen = Screen::Browser;
            state.settings_text_focus = SettingsTextFocus::None;
            return true;
        }
        if mx >= TAB_SET_X && mx < TAB_SET_X + TAB_SET_W {
            state.screen = Screen::Settings;
            return true;
        }
        if state.disk_install_available && mx >= TAB_INS_X && mx < TAB_INS_X + TAB_INS_W {
            state.screen = Screen::DiskInstall;
            return true;
        }
    }

    if state.screen == Screen::DiskInstall {
        if state.disk_install_phase == DiskInstallPhase::Idle {
            let (bx, by, bw, bh) = disk_install_button_rect(&lay);
            if mx >= bx && mx < bx + bw && my >= by && my < by + bh {
                state.disk_install_start_request = true;
                return true;
            }
        }
        return false;
    }

    if state.screen == Screen::Browser {
        if let Some(i) = url_bar_button_hit(&lay, mx, my) {
            match i {
                0 | 1 => {
                    // Back / forward — not implemented; consume click.
                    return true;
                }
                2 => {
                    state.inet_reload_request = true;
                    state.status_dirty = true;
                    return true;
                }
                3 => {
                    let h = DEFAULT_HOME_URL;
                    let n = h.len().min(state.url.len());
                    state.url[..n].copy_from_slice(&h[..n]);
                    state.url_len = n;
                    state.inet_reload_request = true;
                    state.chrome_only_dirty = true;
                    state.status_dirty = true;
                    return true;
                }
                4 => {
                    state.inet_reload_request = true;
                    state.status_dirty = true;
                    return true;
                }
                _ => {}
            }
        }
    }

    if state.screen != Screen::Settings {
        return false;
    }

    let w = lay.w;
    let rx = 36usize;
    let rw = w.saturating_sub(72);
    let mut y = settings_first_row_y(lay.content_top);
    const ROW_H: usize = 22;
    const GAP: usize = 4;
    const SEC_SKIP: usize = 4 + 14;

    let in_row =
        |mx: usize, my: usize, y: usize| my >= y && my < y + ROW_H && mx >= rx && mx < rx + rw;

    if in_row(mx, my, y) {
        state.settings.wifi_enabled = !state.settings.wifi_enabled;
        return true;
    }
    y += ROW_H + GAP;

    if in_row(mx, my, y) {
        wifi_demo_scan(state);
        return true;
    }
    y += ROW_H + GAP;

    for slot in 0..3usize {
        if in_row(mx, my, y) {
            let n = state.wifi_scan_lens[slot] as usize;
            if slot < usize::from(state.wifi_scan_count) && n > 0 && n <= 32 {
                state.settings.wifi_ssid[..n]
                    .copy_from_slice(&state.wifi_scan_names[slot][..n]);
                state.settings.wifi_ssid_len = n;
                state.settings_text_focus = SettingsTextFocus::WifiSsid;
            }
            return true;
        }
        y += ROW_H + GAP;
    }
    if state.wifi_scan_demo {
        y += 12;
    }

    if in_row(mx, my, y) {
        state.settings_text_focus = SettingsTextFocus::WifiSsid;
        return true;
    }
    y += ROW_H + GAP;

    if in_row(mx, my, y) {
        state.settings.wifi_sec = state.settings.wifi_sec.next();
        return true;
    }
    y += ROW_H + GAP;

    if in_row(mx, my, y) {
        state.settings_text_focus = SettingsTextFocus::WifiPsk;
        return true;
    }
    y += ROW_H + GAP;

    if in_row(mx, my, y) {
        state.settings.nic = state.settings.nic.next();
        return true;
    }
    y += ROW_H + GAP;

    if in_row(mx, my, y) {
        state.settings.ip_config = state.settings.ip_config.next();
        return true;
    }
    y += ROW_H + GAP;

    if in_row(mx, my, y) {
        state.settings.internet_stack_enabled = !state.settings.internet_stack_enabled;
        return true;
    }
    y += ROW_H + GAP + SEC_SKIP;

    if in_row(mx, my, y) {
        state.settings.usb_polling_enabled = !state.settings.usb_polling_enabled;
        return true;
    }
    y += ROW_H + GAP;

    if in_row(mx, my, y) {
        state.settings = state.settings.next_cursor_emoji_preset();
        return true;
    }
    y += ROW_H + GAP + SEC_SKIP;

    if in_row(mx, my, y) {
        state.settings.bluetooth_enabled = !state.settings.bluetooth_enabled;
        return true;
    }
    y += ROW_H + GAP + SEC_SKIP;

    if in_row(mx, my, y) {
        state.settings.midi_enabled = !state.settings.midi_enabled;
        return true;
    }
    y += ROW_H + GAP;

    if in_row(mx, my, y) {
        state.settings.midi_usb_enabled = !state.settings.midi_usb_enabled;
        return true;
    }

    let ((rx, ry, rw, rh), (sx, sy, sw, sh)) = settings_power_button_rects(state, &lay);
    if mx >= rx && mx < rx + rw && my >= ry && my < ry + rh {
        state.power_reboot_request = true;
        return true;
    }
    if mx >= sx && mx < sx + sw && my >= sy && my < sy + sh {
        state.power_shutdown_request = true;
        return true;
    }

    false
}
