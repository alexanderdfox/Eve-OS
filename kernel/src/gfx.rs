// SPDX-License-Identifier: MIT OR Apache-2.0

//! Framebuffer UI: browser chrome, settings (Wi-Fi / NIC / BT / MIDI), status line.
//! Dirty updates: full repaint only when content changes; cursors use save-under to avoid flicker.

use bootloader_api::info::{FrameBufferInfo, PixelFormat};
use crate::net::NetPhase;
use crate::settings::{DeviceSettings, NicChoice, Screen};
use crate::usb_hid;

pub const TAB_EVE_X: usize = 12;
pub const TAB_EVE_W: usize = 130;
pub const TAB_SET_X: usize = 148;
pub const TAB_SET_W: usize = 90;

pub const MAX_CURSORS: usize = 10;

/// Default “home page” (open in a host browser; Eve has no HTML engine).
pub const DEFAULT_HOME_URL: &[u8] = b"https://alexanderdfox.github.io/TempleOSWebShrine/";

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
    /// Full UI repaint (clear + chrome + body + status text).
    pub content_dirty: bool,
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
        let cursor_active = [true; MAX_CURSORS];
        for i in 0..MAX_CURSORS {
            let col = (i % 5) as i32;
            let row = (i / 5) as i32;
            cursor_x[i] = (width * (8 + col * 17)) / 100;
            cursor_y[i] = (height * (22 + row * 18)) / 100;
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
            content_dirty: true,
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
];

fn cursor_clip_rect(cx: i32, cy: i32, screen_w: usize, screen_h: usize) -> (usize, usize, usize, usize) {
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
        pixel(
            buf,
            info,
            cx.saturating_sub(d),
            cy,
            r,
            g,
            b,
        );
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
            buf,
            info,
            self.sx[i],
            self.sy[i],
            self.sw[i],
            self.sh[i],
            slot,
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

fn fill_circle(buf: &mut [u8], info: &FrameBufferInfo, cx: usize, cy: usize, rad: usize, r: u8, g: u8, b: u8) {
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

pub fn draw_str(buf: &mut [u8], info: &FrameBufferInfo, mut x: usize, y: usize, s: &[u8], font: &[[u8; 5]; 59]) {
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
                        pixel(buf, info, x + col, y + row, 0x22, 0x22, 0x22);
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

fn draw_hex_byte(buf: &mut [u8], info: &FrameBufferInfo, mut x: usize, y: usize, v: u8, font: &[[u8; 5]; 59]) {
    const H: &[u8; 16] = b"0123456789ABCDEF";
    let hi = H[(v >> 4) as usize];
    let lo = H[(v & 0x0F) as usize];
    draw_str(buf, info, x, y, &[hi], font);
    x = x.saturating_add(6);
    draw_str(buf, info, x, y, &[lo], font);
}

fn draw_decimal(buf: &mut [u8], info: &FrameBufferInfo, x: usize, y: usize, mut n: u32, font: &[[u8; 5]; 59]) {
    if n == 0 {
        draw_str(buf, info, x, y, b"0", font);
        return;
    }
    let mut tmp = [0u8; 10];
    let mut i = tmp.len();
    while n > 0 && i > 0 {
        i -= 1;
        tmp[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    draw_str(buf, info, x, y, &tmp[i..], font);
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
    fill_rect(buf, info, 0, 0, w, chrome_h, 0x3c, 0x3c, 0x3c);
    let dot_y = chrome_h / 2;
    for (i, (r, g, b)) in [(0xFF, 0x5F, 0x57), (0xFF, 0xBD, 0x2E), (0x28, 0xC8, 0x3F)]
        .into_iter()
        .enumerate()
    {
        let cx = 20 + i * 22;
        fill_circle(buf, info, cx, dot_y, 6, r, g, b);
    }

    let tab_y = lay.tab_y;
    let tab_h = lay.tab_h;
    if tab_y + tab_h < lay.h {
        fill_rect(buf, info, 0, tab_y, w, tab_h, 0xd0, 0xd0, 0xd0);
        let eve_on = state.screen == Screen::Browser;
        let set_on = state.screen == Screen::Settings;
        fill_rect(
            buf,
            info,
            TAB_EVE_X,
            tab_y + 4,
            TAB_EVE_W,
            tab_h - 8,
            if eve_on {
                0xf5
            } else {
                0xe0
            },
            if eve_on {
                0xf5
            } else {
                0xe0
            },
            if eve_on {
                0xf5
            } else {
                0xe0
            },
        );
        fill_rect(
            buf,
            info,
            TAB_SET_X,
            tab_y + 4,
            TAB_SET_W,
            tab_h - 8,
            if set_on {
                0xf5
            } else {
                0xe0
            },
            if set_on {
                0xf5
            } else {
                0xe0
            },
            if set_on {
                0xf5
            } else {
                0xe0
            },
        );
        draw_str(
            buf,
            info,
            24,
            tab_y + (tab_h / 2).saturating_sub(4),
            b"EVE",
            font,
        );
        draw_str(
            buf,
            info,
            TAB_SET_X + 12,
            tab_y + (tab_h / 2).saturating_sub(4),
            b"SET",
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
    fill_rect(buf, info, 0, bar_y, w, bar_h, 0xe8, 0xe8, 0xe8);
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
        fill_rect(buf, info, url_x, bar_y + 6, w - url_x - 12, bar_h - 12, 0xff, 0xff, 0xff);
        let text_y = bar_y + (bar_h / 2).saturating_sub(4);
        if state.url_len > 0 {
            draw_str(buf, info, url_x + 8, text_y, &state.url[..state.url_len], font);
        }
    }
}

/// First clickable settings row top Y (must match `draw_settings_body`).
fn settings_first_row_y(content_top: usize) -> usize {
    content_top + 12 + 20 + 14
}

fn draw_on_off(buf: &mut [u8], info: &FrameBufferInfo, x: usize, y: usize, on: bool, font: &[[u8; 5]; 59]) {
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
    if content_top + 320 >= h {
        return;
    }
    fill_rect(
        buf,
        info,
        24,
        content_top,
        w.saturating_sub(48),
        h - content_top - 48,
        0xfa,
        0xfa,
        0xfa,
    );

    let mut y = content_top + 12;
    draw_str(buf, info, 40, y, b"SETTINGS", font);
    y += 20;
    draw_str(buf, info, 40, y, b"NETWORK", font);
    y += 14;

    const ROW_H: usize = 22;
    const GAP: usize = 4;
    let row_bg = |buf: &mut [u8], ry: usize| {
        fill_rect(buf, info, 36, ry, w.saturating_sub(72), ROW_H, 0xef, 0xef, 0xef);
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
    draw_decimal(buf, info, ex, y + 6, u32::from(state.pci_eth_count), font);
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
    draw_str(buf, info, 200.min(w.saturating_sub(140)), y + 6, b"CLASSIC", font);
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
    draw_str(buf, info, 280.min(w.saturating_sub(140)), y + 6, b"CH", font);
    draw_decimal(
        buf,
        info,
        302.min(w.saturating_sub(120)),
        y + 6,
        u32::from(state.settings.midi_channel),
        font,
    );
    draw_on_off(buf, info, right_x, y + 6, state.settings.midi_enabled, font);
    y += ROW_H + GAP;

    // 6: USB MIDI preference
    row_bg(buf, y);
    draw_str(buf, info, 44, y + 6, b"USB MIDI", font);
    draw_str(buf, info, 200.min(w.saturating_sub(160)), y + 6, b"DRIVER TBD", font);
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
        b"F1 SET  F2 WEB  F3 MIDICH  CLICK ROW TO TOGGLE",
        font,
    );
}

fn draw_browser_body(
    buf: &mut [u8],
    info: &FrameBufferInfo,
    lay: &Layout,
    font: &[[u8; 5]; 59],
) {
    let w = lay.w;
    let h = lay.h;
    let content_top = lay.content_top;
    if content_top + 120 >= h {
        return;
    }
    fill_rect(
        buf,
        info,
        24,
        content_top,
        w.saturating_sub(48),
        h - content_top - 48,
        0xff,
        0xff,
        0xff,
    );
    draw_str(
        buf,
        info,
        48,
        content_top + 16,
        b"HOME: TEMPLEOS WEB SHRINE (HOST BROWSER FOR HTML+AUDIO).",
        font,
    );
    draw_str(
        buf,
        info,
        48,
        content_top + 36,
        b"INET: QEMU 10.0.2.X USER NET. STATUS WWW=HTTP BYTES FROM EXAMPLE.COM.",
        font,
    );
}

fn draw_status_line(buf: &mut [u8], info: &FrameBufferInfo, lay: &Layout, state: &UiState, font: &[[u8; 5]; 59]) {
    let h = lay.h;
    let status_y = h.saturating_sub(28);
    draw_str(buf, info, 8, status_y, b"NET", font);
    let mut sx = 8 + 4 * 6;
    if state.net_ok {
        draw_str(buf, info, sx, status_y, b"MAC ", font);
        sx += 5 * 6;
        for (i, oct) in state.mac.iter().enumerate() {
            if i > 0 {
                draw_str(buf, info, sx, status_y, b":", font);
                sx += 6;
            }
            draw_hex_byte(buf, info, sx, status_y, *oct, font);
            sx += 12;
        }
        draw_str(buf, info, sx, status_y, b" RX", font);
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
        draw_str(buf, info, sx, status_y, &tmp[i..], font);
        sx += (tmp.len() - i) * 6;
        draw_str(buf, info, sx, status_y, b" I", font);
        let ix = sx + 2 * 6;
        let lab: &[u8] = match state.inet_phase {
            NetPhase::Off => b"--",
            NetPhase::Arp => b"ARP",
            NetPhase::Tcp => b"TCP",
            NetPhase::Http => b"GET",
            NetPhase::Done => b"WWW",
        };
        draw_str(buf, info, ix, status_y, lab, font);
        let ix2 = ix + lab.len() * 6;
        draw_str(buf, info, ix2, status_y, b" ", font);
        draw_decimal(buf, info, ix2 + 6, status_y, state.inet_bytes, font);
    } else {
        draw_str(buf, info, sx, status_y, b": OFF", font);
    }
}

fn paint_ui(buf: &mut [u8], info: &FrameBufferInfo, lay: &Layout, state: &UiState, font: &[[u8; 5]; 59]) {
    clear(buf, info, 0xee, 0xee, 0xee);
    draw_chrome_and_tabs(buf, info, lay, state, font);
    draw_url_bar(buf, info, lay, state, font);
    match state.screen {
        Screen::Browser => draw_browser_body(buf, info, lay, font),
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
    fill_rect(buf, info, 0, status_y, lay.w, 28, 0xee, 0xee, 0xee);
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
        state.status_dirty = false;
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
            return true;
        }
        if mx >= TAB_SET_X && mx < TAB_SET_X + TAB_SET_W {
            state.screen = Screen::Settings;
            return true;
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

    let in_row = |mx: usize, my: usize, y: usize| {
        my >= y && my < y + ROW_H && mx >= rx && mx < rx + rw
    };

    if in_row(mx, my, y) {
        state.settings.wifi_enabled = !state.settings.wifi_enabled;
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
