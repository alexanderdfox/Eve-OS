// SPDX-License-Identifier: MIT OR Apache-2.0

//! TempleOS-style palette and chrome: Web Shrine tab, SYS settings, multi-pointer USB HUD,
//! MIDI flags, VirtIO status. URL typing uses `chrome_only_dirty` (no full clear) so save-under
//! cursors stay consistent; full `content_dirty` for tab/screen changes.

use crate::html::{self, BROWSER_LINE_CAP, BROWSER_MAX_LINES};
use crate::net::NetPhase;
use crate::settings::{DeviceSettings, NicChoice, Screen};
use crate::usb_hid;
use bootloader_api::info::{FrameBufferInfo, PixelFormat};

pub const TAB_EVE_X: usize = 12;
pub const TAB_EVE_W: usize = 130;
pub const TAB_SET_X: usize = 148;
pub const TAB_SET_W: usize = 90;

pub const MAX_CURSORS: usize = 12;

/// Home page loaded at boot when VirtIO + the internet stack are on (`http://` only).
/// `example.com` serves small plain HTTP without TLS; GitHub Pages and most sites redirect to HTTPS.
pub const DEFAULT_HOME_URL: &[u8] = b"http://example.com/";

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
    /// Primary pointer (cursor 0) for clicks and URL typing.
    pub mx: i32,
    pub my: i32,
    pub cursor_x: [i32; MAX_CURSORS],
    pub cursor_y: [i32; MAX_CURSORS],
    pub cursor_active: [bool; MAX_CURSORS],
    pub mouse_btn: u8,
    pub prev_mouse_btn: u8,
    pub net_rx: u64,
    pub net_ok: bool,
    pub mac: [u8; 6],
    pub screen: Screen,
    pub settings: DeviceSettings,
    pub pci_wlan: bool,
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
    /// Full UI repaint (clear + chrome + body + status text).
    pub content_dirty: bool,
    /// Browser: URL / tab strip / nav bar / status — no full-screen clear (keeps cursor save/restore stable while typing).
    pub chrome_only_dirty: bool,
    /// Browser body area only (page text / errors).
    pub browser_body_dirty: bool,
    /// Status strip only (RX counter etc.).
    pub status_dirty: bool,
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
            mouse_btn: 0,
            prev_mouse_btn: 0,
            net_rx: 0,
            net_ok: false,
            mac: [0; 6],
            screen: Screen::Browser,
            settings: DeviceSettings::new(),
            pci_wlan: pci_wlan,
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
            content_dirty: true,
            chrome_only_dirty: false,
            browser_body_dirty: false,
            status_dirty: true,
        }
    }

    pub fn layout(&self, info: &FrameBufferInfo) -> Layout {
        let w = info.width;
        let h = info.height;
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

const CUR_HALF: i32 = 9;
const SLOT_BYTES: usize = 22 * 22 * 4;

const CURSOR_RGB: [(u8, u8, u8); MAX_CURSORS] = [
    (0, 0, 0),
    (220, 60, 60),
    (60, 180, 80),
    (70, 130, 220),
    (200, 140, 60),
    (180, 80, 200),
    (80, 200, 200),
    (240, 200, 60),
    (120, 120, 220),
    (200, 100, 140),
    (100, 200, 120),
    (220, 120, 200),
];

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

fn draw_cursor_indexed(buf: &mut [u8], info: &FrameBufferInfo, cx: i32, cy: i32, idx: usize) {
    let w = info.width as i32;
    let h = info.height as i32;
    if cx < 0 || cy < 0 || cx >= w || cy >= h {
        return;
    }
    let (r, g, b) = CURSOR_RGB[idx.min(MAX_CURSORS - 1)];
    let cx = cx as usize;
    let cy = cy as usize;
    for d in 0..8usize {
        pixel(buf, info, cx.saturating_sub(d), cy, r, g, b);
        pixel(buf, info, cx.saturating_add(d), cy, r, g, b);
        pixel(buf, info, cx, cy.saturating_sub(d), r, g, b);
        pixel(buf, info, cx, cy.saturating_add(d), r, g, b);
    }
    pixel(buf, info, cx, cy, 0xFF, 0xFF, 0xFF);
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
    pub fn new() -> Self {
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
        for i in 0..MAX_CURSORS {
            if state.cursor_active[i] {
                draw_cursor_indexed(buf, info, state.cursor_x[i], state.cursor_y[i], i);
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
    let btn = 32.min(w / 12).max(24);
    for (i, label) in [(0usize, b"<"), (1, b">"), (2, b"R")].iter() {
        let x0 = 12 + i * (btn + 8);
        fill_rect(buf, info, x0, bar_y + 6, btn, bar_h - 12, 0xff, 0xff, 0xff);
        draw_str(
            buf,
            info,
            x0 + btn / 2 - 3,
            bar_y + (bar_h / 2).saturating_sub(4),
            *label,
            font,
        );
    }
    let url_x = 12 + 3 * (btn + 8) + 8;
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

/// First clickable settings row top Y (must match `draw_settings_body`).
fn settings_first_row_y(content_top: usize) -> usize {
    content_top + 12 + 20 + 14
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

fn draw_on_off(
    buf: &mut [u8],
    info: &FrameBufferInfo,
    x: usize,
    y: usize,
    on: bool,
    font: &[[u8; 5]; 59],
) {
    if on {
        draw_str(buf, info, x, y, b"ON", font);
    } else {
        draw_str(buf, info, x, y, b"OFF", font);
    }
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
    if content_top + 520 >= h {
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
        0xf2,
        0xff,
    );

    let mut y = content_top + 12;
    draw_str(buf, info, 40, y, b"SYS CONFIG", font);
    y += 20;
    draw_str(buf, info, 40, y, b"NETWORK", font);
    y += 14;

    const ROW_H: usize = 22;
    const GAP: usize = 4;
    let row_bg = |buf: &mut [u8], ry: usize| {
        fill_rect(
            buf,
            info,
            36,
            ry,
            w.saturating_sub(72),
            ROW_H,
            0xef,
            0xef,
            0xef,
        );
    };
    let right_x = w.saturating_sub(80).min(420);

    // 0: Wi‑Fi
    row_bg(buf, y);
    draw_str(buf, info, 44, y + 6, b"WIFI", font);
    let hx = 188.min(w.saturating_sub(140));
    if state.pci_wlan {
        draw_str(buf, info, hx, y + 6, b"HW YES", font);
    } else {
        draw_str(buf, info, hx, y + 6, b"HW NO", font);
    }
    draw_on_off(buf, info, right_x, y + 6, state.settings.wifi_enabled, font);
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
            b"SAMPLES ONLY  NO 802.11 DRIVER IN EVE",
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
        NicChoice::E1000Stub => draw_str(buf, info, nx, y + 6, b"E1000", font),
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

    // 2: Internet stack (ARP/HTTP demo)
    row_bg(buf, y);
    draw_str(buf, info, 44, y + 6, b"INTERNET", font);
    let ix = 200.min(w.saturating_sub(160));
    if state.settings.internet_stack_enabled {
        draw_str(buf, info, ix, y + 6, b"TCP HTTP", font);
    } else {
        draw_str(buf, info, ix, y + 6, b"PAUSED", font);
    }
    draw_on_off(
        buf,
        info,
        right_x,
        y + 6,
        state.settings.internet_stack_enabled,
        font,
    );
    y += ROW_H + GAP;

    y += 4;
    draw_str(buf, info, 40, y, b"USB", font);
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
    draw_on_off(
        buf,
        info,
        right_x,
        y + 6,
        state.settings.usb_polling_enabled,
        font,
    );
    y += ROW_H + GAP;

    y += 4;
    draw_str(buf, info, 40, y, b"WIRELESS", font);
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
    draw_on_off(
        buf,
        info,
        right_x,
        y + 6,
        state.settings.bluetooth_enabled,
        font,
    );
    y += ROW_H + GAP;

    y += 4;
    draw_str(buf, info, 40, y, b"MIDI AND AUDIO", font);
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
    draw_on_off(buf, info, right_x, y + 6, state.settings.midi_enabled, font);
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
    draw_on_off(
        buf,
        info,
        right_x,
        y + 6,
        state.settings.midi_usb_enabled,
        font,
    );
    y += ROW_H + 10;

    draw_str(
        buf,
        info,
        36,
        y,
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
    if content_top + 40 >= h {
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
        0xff,
    );

    let x0 = 48usize;
    let status_h = 28usize;
    let y_max = h.saturating_sub(status_h + 8);
    let mut y = content_top + 12;
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
        draw_str(
            buf,
            info,
            x0,
            y,
            b"TYPE HTTP://HOST/PATH  ENTER GO  R RELOAD  ARROWS SCROLL",
            font,
        );
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
    clear(buf, info, 0xd0, 0xe4, 0xff);
    draw_chrome_and_tabs(buf, info, lay, state, font);
    draw_url_bar(buf, info, lay, state, font);
    match state.screen {
        Screen::Browser => draw_browser_body(buf, info, lay, state, font),
        Screen::Settings => draw_settings_body(buf, info, lay, state, font),
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
        draw_chrome_and_tabs(buf, info, &lay, state, font);
        draw_url_bar(buf, info, &lay, state, font);
        draw_status_line(buf, info, &lay, state, font);
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

    eng.patch_cursors_only(buf, info, state);
}

/// True if `(mx, my)` hits the browser chrome reload button (“R”).
fn chrome_reload_hit(lay: &Layout, mx: usize, my: usize) -> bool {
    let w = lay.w;
    let bar_y = lay.bar_y;
    let bar_h = lay.bar_h;
    if bar_y + bar_h >= lay.h {
        return false;
    }
    let btn = 32.min(w / 12).max(24);
    let x0 = 12 + 2 * (btn + 8);
    mx >= x0 && mx < x0 + btn && my >= bar_y + 6 && my < bar_y + bar_h.saturating_sub(6)
}

/// Left button down edge; returns true if a setting was toggled or tab switched.
pub fn handle_click(state: &mut UiState, info: &FrameBufferInfo) -> bool {
    let lay = state.layout(info);
    let mx = state.mx as usize;
    let my = state.my as usize;

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
    }

    if state.screen == Screen::Browser && chrome_reload_hit(&lay, mx, my) {
        state.inet_reload_request = true;
        state.status_dirty = true;
        return true;
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
        state.settings.internet_stack_enabled = !state.settings.internet_stack_enabled;
        return true;
    }
    y += ROW_H + GAP + SEC_SKIP;

    if in_row(mx, my, y) {
        state.settings.usb_polling_enabled = !state.settings.usb_polling_enabled;
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

    false
}
