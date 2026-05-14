// SPDX-License-Identifier: MIT OR Apache-2.0
//
//! Full-color raster emoji (Unicode scalars) for the framebuffer UI. Curated set only; unknown
//! scalars fall back to ASCII `?` in mixed text layout.

use crate::fb_info::{FrameBufferInfo, PixelFormat};

/// Horizontal advance for a raster emoji cell (matches browser line rhythm).
pub const CELL_W: usize = 14;
pub const CELL_H: usize = 12;

/// Variation selector (emoji presentation); consumes no layout width.
#[inline]
pub fn is_variation_selector(cp: u32) -> bool {
    cp == 0xFE0F
}

/// UTF-8 length of a valid leading byte, or `None` if not a lead byte.
pub fn utf8_lead_len(b0: u8) -> Option<usize> {
    match b0 {
        0x00..=0x7F => Some(1),
        0xC2..=0xDF => Some(2),
        0xE0..=0xEF => Some(3),
        0xF0..=0xF4 => Some(4),
        _ => None,
    }
}

pub fn utf8_seq_valid(s: &[u8]) -> bool {
    let n = match s.first().copied() {
        Some(b) => match utf8_lead_len(b) {
            Some(l) if l <= s.len() => l,
            _ => return false,
        },
        None => return false,
    };
    if n == 1 {
        return true;
    }
    for i in 1..n {
        if s[i] & 0xC0 != 0x80 {
            return false;
        }
    }
    true
}

fn put_pixel(buf: &mut [u8], info: &FrameBufferInfo, x: usize, y: usize, r: u8, g: u8, b: u8) {
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
            if bpp >= 4 {
                buf[i + 3] = 0xff;
            }
        }
        PixelFormat::Bgr => {
            buf[i] = b;
            buf[i + 1] = g;
            buf[i + 2] = r;
            if bpp >= 4 {
                buf[i + 3] = 0xff;
            }
        }
        PixelFormat::U8 => {
            let v = ((r as u16 + g as u16 + b as u16) / 3) as u8;
            buf[i] = v;
        }
        _ => {}
    }
}

fn fill_rect(
    buf: &mut [u8],
    info: &FrameBufferInfo,
    x0: usize,
    y0: usize,
    w: usize,
    h: usize,
    r: u8,
    g: u8,
    b: u8,
) {
    for yy in y0..y0.saturating_add(h) {
        for xx in x0..x0.saturating_add(w) {
            put_pixel(buf, info, xx, yy, r, g, b);
        }
    }
}

fn fill_circle(
    buf: &mut [u8],
    info: &FrameBufferInfo,
    cx: i32,
    cy: i32,
    rad: i32,
    r: u8,
    g: u8,
    b: u8,
) {
    let r2 = rad * rad;
    for dy in -rad..=rad {
        for dx in -rad..=rad {
            if dx * dx + dy * dy > r2 {
                continue;
            }
            let px = cx + dx;
            let py = cy + dy;
            if px >= 0 && py >= 0 {
                put_pixel(buf, info, px as usize, py as usize, r, g, b);
            }
        }
    }
}

fn draw_heart(buf: &mut [u8], info: &FrameBufferInfo, ox: usize, oy: usize) {
    fill_circle(buf, info, (ox + 4) as i32, (oy + 4) as i32, 3, 0xe8, 0x3a, 0x4a);
    fill_circle(buf, info, (ox + 9) as i32, (oy + 4) as i32, 3, 0xe8, 0x3a, 0x4a);
    for py in 6..12usize {
        let row = py - 6;
        let half = row + 1;
        let x1 = ox + 6usize.saturating_sub(half);
        let w = half * 2 + 1;
        fill_rect(buf, info, x1, oy + py, w, 1, 0xe8, 0x3a, 0x4a);
    }
}

fn draw_globe(buf: &mut [u8], info: &FrameBufferInfo, ox: usize, oy: usize, americas: bool) {
    fill_circle(buf, info, (ox + 6) as i32, (oy + 5) as i32, 5, 0x38, 0x7c, 0xd8);
    fill_rect(buf, info, ox + 3, oy + 4, 2, 2, 0x4c, 0xa8, 0x5c);
    fill_rect(buf, info, ox + 9, oy + 7, 3, 2, 0x4c, 0xa8, 0x5c);
    if americas {
        fill_rect(buf, info, ox + 5, oy + 6, 2, 3, 0x4c, 0xa8, 0x5c);
        fill_rect(buf, info, ox + 8, oy + 4, 2, 2, 0x4c, 0xa8, 0x5c);
    } else {
        fill_rect(buf, info, ox + 4, oy + 6, 3, 2, 0x4c, 0xa8, 0x5c);
        fill_rect(buf, info, ox + 8, oy + 5, 2, 3, 0x4c, 0xa8, 0x5c);
    }
    for dy in -5i32..=5i32 {
        for dx in -5i32..=5i32 {
            if dx * dx + dy * dy > 25 {
                continue;
            }
            if dx * dx + dy * dy < 20 {
                continue;
            }
            let px = ox as i32 + 6 + dx;
            let py = oy as i32 + 5 + dy;
            if px >= 0 && py >= 0 {
                put_pixel(buf, info, px as usize, py as usize, 0x1a, 0x4a, 0x9e);
            }
        }
    }
}

fn draw_grin(buf: &mut [u8], info: &FrameBufferInfo, ox: usize, oy: usize) {
    fill_circle(buf, info, (ox + 6) as i32, (oy + 5) as i32, 5, 0xff, 0xd5, 0x3a);
    put_pixel(buf, info, ox + 4, oy + 4, 0x22, 0x22, 0x22);
    put_pixel(buf, info, ox + 8, oy + 4, 0x22, 0x22, 0x22);
    for px in 3..10usize {
        put_pixel(buf, info, ox + px, oy + 8, 0x22, 0x22, 0x22);
    }
}

fn draw_check(buf: &mut [u8], info: &FrameBufferInfo, ox: usize, oy: usize) {
    fill_rect(buf, info, ox + 2, oy + 2, 9, 9, 0x2e, 0xb6, 0x7a);
    for (dx, dy) in [(3i32, 6), (4, 7), (5, 8), (6, 7), (7, 6), (8, 5), (9, 4)] {
        let px = ox as i32 + dx;
        let py = oy as i32 + dy;
        if px >= 0 && py >= 0 {
            put_pixel(buf, info, px as usize, py as usize, 0xff, 0xff, 0xff);
            put_pixel(buf, info, (px + 1) as usize, py as usize, 0xff, 0xff, 0xff);
        }
    }
}

fn draw_warn(buf: &mut [u8], info: &FrameBufferInfo, ox: usize, oy: usize) {
    for py in 2..11usize {
        let row = py - 2;
        let half = row / 2;
        let x1 = ox + 6usize.saturating_sub(half);
        let w = half * 2 + 1;
        fill_rect(buf, info, x1, oy + py, w, 1, 0xf5, 0xc5, 0x2a);
    }
    put_pixel(buf, info, ox + 6, oy + 4, 0x1a, 0x1a, 0x1a);
    fill_rect(buf, info, ox + 5, oy + 7, 3, 2, 0x1a, 0x1a, 0x1a);
}

fn draw_gear(buf: &mut [u8], info: &FrameBufferInfo, ox: usize, oy: usize) {
    fill_circle(buf, info, (ox + 6) as i32, (oy + 5) as i32, 3, 0x9a, 0xa3, 0xad);
    for &(px, py) in [
        (6u8, 1u8),
        (9, 3),
        (10, 6),
        (9, 9),
        (6, 10),
        (3, 9),
        (2, 6),
        (3, 3),
    ]
    .iter()
    {
        fill_rect(buf, info, ox + px as usize, oy + py as usize, 2, 2, 0x7a, 0x82, 0x8e);
    }
    fill_circle(buf, info, (ox + 6) as i32, (oy + 5) as i32, 1, 0xe8, 0xec, 0xf0);
}

fn draw_wrench(buf: &mut [u8], info: &FrameBufferInfo, ox: usize, oy: usize) {
    for i in 0..7usize {
        put_pixel(buf, info, ox + 3 + i, oy + 2 + i / 2, 0xb0, 0xb8, 0xc4);
    }
    fill_rect(buf, info, ox + 8, oy + 5, 4, 3, 0xb0, 0xb8, 0xc4);
    fill_rect(buf, info, ox + 9, oy + 4, 2, 2, 0xd0, 0xd6, 0xde);
}

fn draw_note(buf: &mut [u8], info: &FrameBufferInfo, ox: usize, oy: usize) {
    fill_rect(buf, info, ox + 4, oy + 2, 3, 6, 0x6b, 0x46, 0xc1);
    fill_circle(buf, info, (ox + 5) as i32, (oy + 3) as i32, 2, 0x8b, 0x6e, 0xe0);
    fill_rect(buf, info, ox + 6, oy + 6, 1, 5, 0x1a, 0x12, 0x20);
    fill_rect(buf, info, ox + 7, oy + 9, 4, 2, 0x1a, 0x12, 0x20);
}

fn draw_lock(buf: &mut [u8], info: &FrameBufferInfo, ox: usize, oy: usize) {
    fill_rect(buf, info, ox + 4, oy + 6, 5, 5, 0xc9, 0xa2, 0x27);
    fill_rect(buf, info, ox + 5, oy + 7, 3, 3, 0xe8, 0xd4, 0x6a);
    fill_rect(buf, info, ox + 5, oy + 3, 3, 3, 0x8a, 0x8a, 0x92);
    put_pixel(buf, info, ox + 6, oy + 2, 0x6a, 0x6a, 0x72);
}

fn draw_thumbs(buf: &mut [u8], info: &FrameBufferInfo, ox: usize, oy: usize) {
    fill_rect(buf, info, ox + 4, oy + 3, 4, 7, 0xff, 0xd5, 0x3a);
    fill_rect(buf, info, ox + 3, oy + 8, 6, 3, 0xff, 0xd5, 0x3a);
    fill_rect(buf, info, ox + 2, oy + 9, 3, 2, 0xff, 0xd5, 0x3a);
}

fn draw_laptop(buf: &mut [u8], info: &FrameBufferInfo, ox: usize, oy: usize) {
    fill_rect(buf, info, ox + 2, oy + 3, 9, 6, 0x5c, 0x6a, 0x7e);
    fill_rect(buf, info, ox + 3, oy + 4, 7, 4, 0x38, 0x7c, 0xd8);
    fill_rect(buf, info, ox + 1, oy + 9, 11, 2, 0x8a, 0x92, 0x9e);
}

fn draw_floppy(buf: &mut [u8], info: &FrameBufferInfo, ox: usize, oy: usize) {
    fill_rect(buf, info, ox + 3, oy + 2, 8, 9, 0x3b, 0x82, 0xf6);
    fill_rect(buf, info, ox + 4, oy + 3, 6, 1, 0xe8, 0xec, 0xf0);
    fill_rect(buf, info, ox + 4, oy + 5, 6, 4, 0xf8, 0xfa, 0xfc);
    fill_rect(buf, info, ox + 5, oy + 6, 2, 2, 0x22, 0x22, 0x33);
}

fn draw_clipboard(buf: &mut [u8], info: &FrameBufferInfo, ox: usize, oy: usize) {
    fill_rect(buf, info, ox + 4, oy + 2, 6, 2, 0x9a, 0xa3, 0xad);
    fill_rect(buf, info, ox + 3, oy + 3, 8, 9, 0xe8, 0xec, 0xf0);
    fill_rect(buf, info, ox + 4, oy + 5, 5, 1, 0x22, 0x44, 0x66);
    fill_rect(buf, info, ox + 4, oy + 7, 5, 1, 0x22, 0x44, 0x66);
    fill_rect(buf, info, ox + 4, oy + 9, 5, 1, 0x22, 0x44, 0x66);
}

/// Draw a supported emoji at `(ox, oy)`; returns `false` if `cp` is not in the atlas (caller may skip).
pub fn draw_if_supported(buf: &mut [u8], info: &FrameBufferInfo, ox: usize, oy: usize, cp: u32) -> bool {
    match cp {
        0x2764 => {
            draw_heart(buf, info, ox, oy);
            true
        }
        0x1F310 => {
            draw_globe(buf, info, ox, oy, false);
            true
        }
        0x1F30D => {
            draw_globe(buf, info, ox, oy, true);
            true
        }
        0x1F600 => {
            draw_grin(buf, info, ox, oy);
            true
        }
        0x2705 => {
            draw_check(buf, info, ox, oy);
            true
        }
        0x26A0 => {
            draw_warn(buf, info, ox, oy);
            true
        }
        0x2699 => {
            draw_gear(buf, info, ox, oy);
            true
        }
        0x1F527 => {
            draw_wrench(buf, info, ox, oy);
            true
        }
        0x1F3B5 => {
            draw_note(buf, info, ox, oy);
            true
        }
        0x1F512 => {
            draw_lock(buf, info, ox, oy);
            true
        }
        0x1F44D => {
            draw_thumbs(buf, info, ox, oy);
            true
        }
        0x1F4BB => {
            draw_laptop(buf, info, ox, oy);
            true
        }
        0x1F4BE => {
            draw_floppy(buf, info, ox, oy);
            true
        }
        0x1F4CB => {
            draw_clipboard(buf, info, ox, oy);
            true
        }
        _ => false,
    }
}
