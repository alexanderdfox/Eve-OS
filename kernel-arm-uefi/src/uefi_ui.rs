// SPDX-License-Identifier: MIT OR Apache-2.0
//
//! GOP splash + UEFI **Simple Pointer** (mouse/trackpad) and **Simple Text Input** (keyboard)
//! when the firmware exposes them — intended for GRUB chainload on Asahi Linux / Apple Silicon UEFI.

use core::time::Duration;
use uefi::boot::ScopedProtocol;
use uefi::boot;
use uefi::proto::console::gop::{BltOp, BltPixel, BltRegion, GraphicsOutput};
use uefi::proto::console::pointer::Pointer;
use uefi::proto::console::text::Input;

const CELL_W: usize = 6;
const CELL_H: usize = 8;
pub const MAX_SCAN: usize = 1600;
/// Cursor save-under / compose (fits stack; keep ≤ 32 for small stacks).
const CUR: usize = 26;

fn fill_rect(gop: &mut GraphicsOutput, x: usize, y: usize, w: usize, h: usize, c: BltPixel) {
    if w == 0 || h == 0 {
        return;
    }
    let _ = gop.blt(BltOp::VideoFill {
        color: c,
        dest: (x, y),
        dims: (w, h),
    });
}

fn draw_text_line(
    gop: &mut GraphicsOutput,
    text: &[u8],
    x0: usize,
    y0: usize,
    scale: usize,
    fg: BltPixel,
    bg: BltPixel,
    screen_w: usize,
    screen_h: usize,
    buf: &mut [BltPixel; MAX_SCAN],
) {
    use crate::font::{font_index, FONT_5X7};
    let s = scale.max(1);
    let mut px_w = text.len().saturating_mul(CELL_W).saturating_mul(s);
    px_w = px_w.min(MAX_SCAN).min(screen_w.saturating_sub(x0));
    if px_w == 0 || y0 >= screen_h {
        return;
    }

    let max_chars = (px_w / (CELL_W * s)).min(text.len());
    let rows = CELL_H.saturating_mul(s).min(screen_h.saturating_sub(y0));

    for ry in 0..rows {
        let font_row = (ry / s).min(CELL_H - 1);
        let bit_row = font_row.min(6);

        for x in 0..px_w {
            buf[x] = bg;
        }

        for (ci, &ch) in text.iter().enumerate().take(max_chars) {
            let cx = ci.saturating_mul(CELL_W).saturating_mul(s);
            if cx >= px_w {
                break;
            }
            let Some(idx) = font_index(ch).filter(|&i| i < FONT_5X7.len()) else {
                continue;
            };
            let glyph = &FONT_5X7[idx];
            for col in 0..5usize {
                let on = glyph[col] & (1 << bit_row) != 0;
                let c = if on { fg } else { bg };
                for sx in 0..s {
                    let lx = cx.saturating_add(col.saturating_mul(s).saturating_add(sx));
                    if lx < px_w {
                        buf[lx] = c;
                    }
                }
            }
        }

        let dest_y = y0 + ry;
        if dest_y >= screen_h {
            break;
        }
        let _ = gop.blt(BltOp::BufferToVideo {
            buffer: &buf[..px_w],
            src: BltRegion::Full,
            dest: (x0, dest_y),
            dims: (px_w, 1),
        });
    }
}

fn draw_splash(
    gop: &mut GraphicsOutput,
    w: usize,
    h: usize,
    buf: &mut [BltPixel; MAX_SCAN],
    pointer_ok: bool,
    input_ok: bool,
) {
    fill_rect(
        gop,
        0,
        0,
        w,
        h,
        BltPixel::new(0x14, 0x15, 0x18),
    );

    let accent_top = BltPixel::new(0xE8, 0x78, 0x38);
    let accent_bot = BltPixel::new(0x38, 0xB8, 0xE8);
    let bar_h = (h / 32).clamp(6, 48);
    fill_rect(gop, 0, h / 10, w, bar_h, accent_top);
    fill_rect(gop, 0, h.saturating_sub(h / 8), w, bar_h, accent_bot);

    let scale = (w / 400).clamp(2, 6);
    let fg = BltPixel::new(0xF2, 0xF4, 0xF8);
    let bg_cell = BltPixel::new(0x14, 0x15, 0x18);

    let pstr: &[u8] = if pointer_ok {
        b"UEFI POINTER OK  MOVE MOUSE OR TRACKPAD"
    } else {
        b"UEFI POINTER N/A  FIRMWARE HAS NO SIMPLE POINTER"
    };
    let kstr: &[u8] = if input_ok {
        b"UEFI KEYBOARD OK  SIMPLE TEXT INPUT"
    } else {
        b"UEFI KEYBOARD N/A  NO SIMPLE TEXT INPUT"
    };

    let lines: [&[u8]; 6] = [
        b"EVE OS  AARCH64 UEFI",
        b"DEMO ON ASAHI  GRUB CHAINLOAD",
        pstr,
        kstr,
        b"FULL DESKTOP EVE IS X86  USE QEMU OR UTM",
        b"GRUB DEFAULT  LINUX  THIS  DEMO",
    ];

    let line_step = CELL_H.saturating_mul(scale).saturating_add(scale.saturating_mul(2));
    let block_h = lines.len().saturating_mul(line_step);
    let mut y = h.saturating_sub(block_h).saturating_sub(h / 8);
    if y < h / 10 {
        y = h / 10;
    }

    for line in lines {
        let tw = line.len().saturating_mul(CELL_W).saturating_mul(scale);
        let x0 = w.saturating_sub(tw.min(w)) / 2;
        draw_text_line(
            gop,
            line,
            x0,
            y,
            scale,
            fg,
            bg_cell,
            w,
            h,
            buf,
        );
        y = y.saturating_add(line_step);
        if y >= h.saturating_sub(line_step) {
            break;
        }
    }
}

fn overlay_cursor(work: &mut [BltPixel], side: usize) {
    let mid = side / 2;
    for y in 0..side {
        for x in 0..side {
            let i = y * side + x;
            let dx = (x as isize - mid as isize).abs();
            let dy = (y as isize - mid as isize).abs();
            let border = x == 0 || y == 0 || x + 1 == side || y + 1 == side;
            let cross = dx <= 1 || dy <= 1;
            let tip = x < 5 && y < 5;
            if tip {
                work[i] = BltPixel::new(255, 80, 80);
            } else if border || (cross && dx.max(dy) > 2) {
                work[i] = BltPixel::new(235, 235, 250);
            }
        }
    }
}

/// Interactive loop: polls UEFI pointer + optional keyboard; draws hardware cursor with save-under.
pub fn run_interactive_demo(
    gop: &mut GraphicsOutput,
    line_buf: &mut [BltPixel; MAX_SCAN],
    pointer: &mut Option<ScopedProtocol<Pointer>>,
    input: &mut Option<ScopedProtocol<Input>>,
) -> ! {
    let best = gop.modes().max_by_key(|m| {
        let (w, h) = m.info().resolution();
        w.saturating_mul(h)
    });
    if let Some(mode) = best {
        let _ = gop.set_mode(&mode);
    }
    let mi = gop.current_mode_info();
    let (w, h) = mi.resolution();

    let ptr_ok = pointer.is_some();
    let inp_ok = input.is_some();
    draw_splash(gop, w, h, line_buf, ptr_ok, inp_ok);

    let cs = CUR.min(w).min(h).max(8);
    if cs < 8 || w < cs || h < cs {
        loop {
            let _ = boot::stall(Duration::from_secs(3600));
        }
    }

    if let Some(p) = pointer.as_mut() {
        let _ = p.reset(false);
    }
    if let Some(i) = input.as_mut() {
        let _ = i.reset(false);
    }

    let mut acc_x: i64 = (w / 2).saturating_sub(cs / 2) as i64;
    let mut acc_y: i64 = (h / 2).saturating_sub(cs / 2) as i64;
    let max_x = (w.saturating_sub(cs)) as i64;
    let max_y = (h.saturating_sub(cs)) as i64;

    let mut under = [BltPixel::new(0, 0, 0); CUR * CUR];
    let mut work = [BltPixel::new(0, 0, 0); CUR * CUR];
    let mut prev: Option<(usize, usize)> = None;

    loop {
        let _ = boot::stall(Duration::from_millis(12));

        if let Some(p) = pointer.as_mut() {
            if let Ok(Some(st)) = p.read_state() {
                acc_x += st.relative_movement[0] as i64;
                acc_y += st.relative_movement[1] as i64;
                acc_x = acc_x.clamp(0, max_x);
                acc_y = acc_y.clamp(0, max_y);
            }
        }

        if let Some(i) = input.as_mut() {
            let _ = i.read_key();
        }

        let cx = acc_x as usize;
        let cy = acc_y as usize;

        if let Some((px, py)) = prev {
            let _ = gop.blt(BltOp::BufferToVideo {
                buffer: &under[..cs * cs],
                src: BltRegion::Full,
                dest: (px, py),
                dims: (cs, cs),
            });
        }

        let _ = gop.blt(BltOp::VideoToBltBuffer {
            buffer: &mut under[..cs * cs],
            src: (cx, cy),
            dest: BltRegion::Full,
            dims: (cs, cs),
        });

        work[..cs * cs].copy_from_slice(&under[..cs * cs]);
        overlay_cursor(&mut work[..cs * cs], cs);
        let _ = gop.blt(BltOp::BufferToVideo {
            buffer: &work[..cs * cs],
            src: BltRegion::Full,
            dest: (cx, cy),
            dims: (cs, cs),
        });

        prev = Some((cx, cy));
    }
}
