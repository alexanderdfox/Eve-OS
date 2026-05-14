// SPDX-License-Identifier: MIT OR Apache-2.0
//
//! 16×16 **Noto Emoji** (Apache-2.0) cursor sprites: RGBA assets in `../assets/cursor_noto/`.
//! Each USB pointer uses `(preset + pointer_index) % SPRITE_STYLES` so multiple mice differ.

use crate::fb_info::{FrameBufferInfo, PixelFormat};

pub const CUR_HALF: i32 = 8;
pub const SPRITE_STYLES: usize = 8;
pub const SPRITE: i32 = 16;

const CUR: usize = 16;
const BYTES_PER_CELL: usize = CUR * CUR * 4;

macro_rules! preset_bytes {
    ($n:literal) => {
        include_bytes!(concat!("../assets/cursor_noto/preset_", $n, ".rgba"))
    };
}

const P0: &[u8] = preset_bytes!("00");
const P1: &[u8] = preset_bytes!("01");
const P2: &[u8] = preset_bytes!("02");
const P3: &[u8] = preset_bytes!("03");
const P4: &[u8] = preset_bytes!("04");
const P5: &[u8] = preset_bytes!("05");
const P6: &[u8] = preset_bytes!("06");
const P7: &[u8] = preset_bytes!("07");

const PRESETS: [&[u8]; SPRITE_STYLES] = [P0, P1, P2, P3, P4, P5, P6, P7];

const _: () = assert!(
    P0.len() == BYTES_PER_CELL
        && P1.len() == BYTES_PER_CELL
        && P2.len() == BYTES_PER_CELL
        && P3.len() == BYTES_PER_CELL
        && P4.len() == BYTES_PER_CELL
        && P5.len() == BYTES_PER_CELL
        && P6.len() == BYTES_PER_CELL
        && P7.len() == BYTES_PER_CELL
);

#[inline]
pub fn next_preset(p: u8) -> u8 {
    ((p as usize + 1) % SPRITE_STYLES) as u8
}

pub fn label(preset: u8) -> &'static [u8] {
    let i = (preset as usize) % SPRITE_STYLES;
    match i {
        0 => b"POINTER",
        1 => b"SMILE",
        2 => b"HEART",
        3 => b"STAR",
        4 => b"ROCKET",
        5 => b"CAT",
        6 => b"THUMBS",
        _ => b"RAINBOW",
    }
}

fn read_rgb(buf: &[u8], info: &FrameBufferInfo, x: usize, y: usize) -> Option<(u8, u8, u8)> {
    if x >= info.width || y >= info.height {
        return None;
    }
    let bpp = info.bytes_per_pixel;
    let stride = info.stride;
    let i = y * stride * bpp + x * bpp;
    if i + bpp.min(3) > buf.len() {
        return None;
    }
    match info.pixel_format {
        PixelFormat::Rgb => {
            if bpp >= 3 {
                Some((buf[i], buf[i + 1], buf[i + 2]))
            } else {
                None
            }
        }
        PixelFormat::Bgr => {
            if bpp >= 3 {
                Some((buf[i + 2], buf[i + 1], buf[i]))
            } else {
                None
            }
        }
        PixelFormat::U8 => Some((buf[i], buf[i], buf[i])),
        _ => None,
    }
}

fn write_rgb(buf: &mut [u8], info: &FrameBufferInfo, x: usize, y: usize, r: u8, g: u8, b: u8) {
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

#[inline]
fn blend_channel(s: u8, d: u8, sa: u16) -> u8 {
    let inv = 255u16 - sa;
    ((s as u16 * sa + d as u16 * inv) / 255) as u8
}

fn blend_over(
    buf: &mut [u8],
    info: &FrameBufferInfo,
    x: usize,
    y: usize,
    sr: u8,
    sg: u8,
    sb: u8,
    sa: u8,
) {
    if sa == 0 {
        return;
    }
    if sa == 255 {
        write_rgb(buf, info, x, y, sr, sg, sb);
        return;
    }
    let Some((dr, dg, db)) = read_rgb(buf, info, x, y) else {
        return;
    };
    let sa16 = sa as u16;
    let r = blend_channel(sr, dr, sa16);
    let g = blend_channel(sg, dg, sa16);
    let b = blend_channel(sb, db, sa16);
    write_rgb(buf, info, x, y, r, g, b);
}

/// Draw preset sprite centered on `(cx, cy)`. `pointer_idx` shifts which of the eight glyphs is used.
pub fn draw_sprite(
    buf: &mut [u8],
    info: &FrameBufferInfo,
    cx: i32,
    cy: i32,
    preset: u8,
    pointer_idx: usize,
) {
    let w = info.width as i32;
    let h = info.height as i32;
    if cx < 0 || cy < 0 || cx >= w || cy >= h {
        return;
    }
    let style = (preset as usize + pointer_idx) % SPRITE_STYLES;
    let data = PRESETS[style];
    debug_assert_eq!(data.len(), BYTES_PER_CELL);
    let ox = cx - SPRITE / 2;
    let oy = cy - SPRITE / 2;
    for py in 0..CUR as i32 {
        for px in 0..CUR as i32 {
            let o = ((py * CUR as i32 + px) as usize) * 4;
            if o + 4 > data.len() {
                break;
            }
            let sr = data[o];
            let sg = data[o + 1];
            let sb = data[o + 2];
            let sa = data[o + 3];
            if sa == 0 {
                continue;
            }
            let sx = ox + px;
            let sy = oy + py;
            if sx >= 0 && sy >= 0 && sx < w && sy < h {
                blend_over(
                    buf,
                    info,
                    sx as usize,
                    sy as usize,
                    sr,
                    sg,
                    sb,
                    sa,
                );
            }
        }
    }
}
