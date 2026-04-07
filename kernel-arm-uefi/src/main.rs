// SPDX-License-Identifier: MIT OR Apache-2.0
//
//! AArch64 UEFI: **same on-screen Eve UI** as x86 (`gfx::render_frame`, epilepsy notice, SHRINE/SYS
//! tabs, browser, cursors) via a CPU shadow buffer + GOP blit. **Simple Pointer** + **Simple Text
//! Input** feed [`kernel::arm_input`]. Networking uses **VirtIO-MMIO** when present (QEMU `virt`);
//! bare-metal Asahi has no MMIO NIC in-tree — UI still matches; **NET** stays off until a driver
//! exists. PS/2, PCI Ethernet, USB HID host, and disk install remain x86-only.

#![no_main]
#![no_std]

use core::time::Duration;

use kernel::arm_input::{self, ArmKeyEvent};
use kernel::arm_run;
use kernel::fb_info::{FrameBufferInfo, PixelFormat};
use uefi::boot::{self, MemoryType};
use uefi::prelude::*;
use uefi::proto::console::gop::{BltOp, BltPixel, BltRegion, GraphicsOutput};
use uefi::proto::console::pointer::Pointer;
use uefi::proto::console::text::{Input, Key, ScanCode};
use uefi::system;

fn map_uefi_key(k: Key) -> Option<ArmKeyEvent> {
    match k {
        Key::Printable(c) => {
            let ch = char::from(c);
            let code = u32::from(ch);
            match code {
                8 => Some(ArmKeyEvent::Backspace),
                10 | 13 => Some(ArmKeyEvent::Enter),
                32..=126 => Some(ArmKeyEvent::Char(code as u8)),
                _ => None,
            }
        }
        Key::Special(s) => match s {
            ScanCode::ESCAPE => Some(ArmKeyEvent::Escape),
            ScanCode::PAGE_UP => Some(ArmKeyEvent::PageUp),
            ScanCode::PAGE_DOWN => Some(ArmKeyEvent::PageDown),
            ScanCode::UP => Some(ArmKeyEvent::ArrowUp),
            ScanCode::DOWN => Some(ArmKeyEvent::ArrowDown),
            ScanCode::DELETE => Some(ArmKeyEvent::Backspace),
            ScanCode::FUNCTION_1 => Some(ArmKeyEvent::Func(1)),
            ScanCode::FUNCTION_2 => Some(ArmKeyEvent::Func(2)),
            ScanCode::FUNCTION_3 => Some(ArmKeyEvent::Func(3)),
            ScanCode::FUNCTION_4 => Some(ArmKeyEvent::Func(4)),
            ScanCode::FUNCTION_5 => Some(ArmKeyEvent::Func(5)),
            ScanCode::FUNCTION_6 => Some(ArmKeyEvent::Func(6)),
            ScanCode::FUNCTION_7 => Some(ArmKeyEvent::Func(7)),
            ScanCode::FUNCTION_8 => Some(ArmKeyEvent::Func(8)),
            ScanCode::FUNCTION_9 => Some(ArmKeyEvent::Func(9)),
            ScanCode::FUNCTION_10 => Some(ArmKeyEvent::Func(10)),
            ScanCode::FUNCTION_11 => Some(ArmKeyEvent::Func(11)),
            ScanCode::FUNCTION_12 => Some(ArmKeyEvent::Func(12)),
            _ => None,
        },
    }
}

/// Pixels per `BufferToVideo` chunk (stack). Any horizontal resolution works; previously a single
/// 1920-wide row buffer caused a **black screen** on panels wider than 1920 (common on laptops).
const BLIT_CHUNK: usize = 640;

fn blit_eve_to_gop(gop: &mut GraphicsOutput, src: &[u8], info: &FrameBufferInfo) {
    let w = info.width;
    let h = info.height;
    let stride_px = info.stride;
    let bpp = info.bytes_per_pixel;
    let mut chunk = [BltPixel::new(0, 0, 0); BLIT_CHUNK];

    for y in 0..h {
        let mut x0 = 0;
        while x0 < w {
            let n = (w - x0).min(BLIT_CHUNK);
            for i in 0..n {
                let px = x0 + i;
                let idx = y * stride_px * bpp + px * bpp;
                let (r, g, b) = if idx + 2 < src.len() {
                    match info.pixel_format {
                        PixelFormat::Bgr => (src[idx + 2], src[idx + 1], src[idx]),
                        PixelFormat::Rgb => (src[idx], src[idx + 1], src[idx + 2]),
                        _ => (0, 0, 0),
                    }
                } else {
                    (0, 0, 0)
                };
                chunk[i] = BltPixel::new(r, g, b);
            }
            let _ = gop.blt(BltOp::BufferToVideo {
                buffer: &chunk[..n],
                src: BltRegion::Full,
                dest: (x0, y),
                dims: (n, 1),
            });
            x0 += n;
        }
    }
}

#[entry]
fn main() -> Status {
    if let Err(e) = uefi::helpers::init() {
        return e.status();
    }

    let _ = system::with_stdout(|stdout| {
        let _ = stdout.reset(false);
        let _ = stdout.output_string(cstr16!("Eve OS (AArch64 UEFI) — full UI + virtio-net-mmio\r\n"));
    });

    let gh = boot::get_handle_for_protocol::<GraphicsOutput>().ok();
    let ph = boot::get_handle_for_protocol::<Pointer>().ok();
    let kh = boot::get_handle_for_protocol::<Input>().ok();

    let Some(h) = gh else {
        let _ = system::with_stdout(|s| {
            let _ = s.output_string(cstr16!("No GOP — cannot start Eve UI.\r\n"));
        });
        loop {
            let _ = boot::stall(Duration::from_secs(3600));
        }
    };

    let Ok(mut gop) = boot::open_protocol_exclusive::<GraphicsOutput>(h) else {
        return Status::ABORTED;
    };

    let mut ptr = ph.and_then(|p| boot::open_protocol_exclusive::<Pointer>(p).ok());
    let mut stdin = kh.and_then(|k| boot::open_protocol_exclusive::<Input>(k).ok());

    let best = gop.modes().max_by_key(|m| {
        let (w, h) = m.info().resolution();
        w.saturating_mul(h)
    });
    if let Some(mode) = best {
        let _ = gop.set_mode(&mode);
    }

    let mi = gop.current_mode_info();
    let (gw, gh) = mi.resolution();
    let stride_px = mi.stride();
    let bpp = 4usize;
    let fb_info = FrameBufferInfo {
        width: gw,
        height: gh,
        stride: stride_px,
        pixel_format: PixelFormat::Bgr,
        bytes_per_pixel: bpp,
    };

    let need = stride_px
        .saturating_mul(gh)
        .saturating_mul(bpp)
        .max(1);

    let pool = match boot::allocate_pool(MemoryType::LOADER_DATA, need) {
        Ok(p) => p,
        Err(e) => {
            let _ = system::with_stdout(|s| {
                let _ = s.output_string(cstr16!("allocate_pool failed\r\n"));
            });
            return e.status();
        }
    };

    let buf = unsafe { core::slice::from_raw_parts_mut(pool.as_ptr(), need) };

    if let Some(p) = ptr.as_mut() {
        let _ = p.reset(false);
    }
    if let Some(s) = stdin.as_mut() {
        let _ = s.reset(false);
    }

    let mut acc_x: i64 = (gw / 2) as i64;
    let mut acc_y: i64 = (gh / 2) as i64;
    let max_x = (gw.saturating_sub(1)) as i64;
    let max_y = (gh.saturating_sub(1)) as i64;

    loop {
        arm_input::key_queue_reset();
        if let Some(s) = stdin.as_mut() {
            while let Ok(Some(k)) = s.read_key() {
                if let Some(ev) = map_uefi_key(k) {
                    arm_input::key_queue_push(ev);
                }
            }
        }

        if let Some(p) = ptr.as_mut() {
            if let Ok(Some(st)) = p.read_state() {
                acc_x += st.relative_movement[0] as i64;
                acc_y += st.relative_movement[1] as i64;
                acc_x = acc_x.clamp(0, max_x);
                acc_y = acc_y.clamp(0, max_y);
                let mut btn = 0u8;
                if st.button[0] {
                    btn |= 1;
                }
                arm_input::set_pointer_abs(acc_x as i32, acc_y as i32, btn);
            }
        } else {
            arm_input::set_pointer_abs(acc_x as i32, acc_y as i32, 0);
        }

        unsafe {
            arm_run::main_step(buf, &fb_info);
        }
        blit_eve_to_gop(&mut gop, buf, &fb_info);
        let _ = boot::stall(Duration::from_millis(8));
    }
}
