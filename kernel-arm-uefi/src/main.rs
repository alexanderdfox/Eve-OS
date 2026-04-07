// SPDX-License-Identifier: MIT OR Apache-2.0
//
//! AArch64 UEFI: **same on-screen Eve UI** as x86 (`gfx::render_frame`, epilepsy notice, SHRINE/SYS
//! tabs, browser, cursors) via a CPU shadow buffer + GOP blit. **Simple Pointer** + **Simple Text
//! Input** feed [`kernel::arm_input`]. **VirtIO-MMIO** NIC is probed only when firmware is not Apple
//! (QEMU EDK2, etc.); probing QEMU MMIO on Apple UEFI can fault and boot-loop. **NET** stays off on
//! bare Asahi. PS/2, PCI Ethernet, USB HID host, and disk install remain x86-only.

#![no_main]
#![no_std]

use core::ptr::NonNull;
use core::time::Duration;

use kernel::arm_input::{self, ArmKeyEvent};
use kernel::arm_run;
use kernel::fb_info::{FrameBufferInfo, PixelFormat};
use uefi::boot::{self, MemoryType, OpenProtocolAttributes, OpenProtocolParams};
use uefi::prelude::*;
use uefi::proto::console::gop::{BltOp, BltPixel, BltRegion, GraphicsOutput, Mode};
use uefi::proto::console::pointer::Pointer;
use uefi::proto::console::text::{Input, Key, ScanCode};
use uefi::proto::ProtocolPointer;
use uefi::system;

/// Stack UTF-8 sink for [`CStr16::as_str_in_buf`].
struct VendorUtf8 {
    data: [u8; 96],
    len: usize,
}

impl VendorUtf8 {
    const fn new() -> Self {
        Self {
            data: [0; 96],
            len: 0,
        }
    }
}

impl core::fmt::Write for VendorUtf8 {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let b = s.as_bytes();
        if self.len + b.len() > self.data.len() {
            return Err(core::fmt::Error);
        }
        self.data[self.len..self.len + b.len()].copy_from_slice(b);
        self.len += b.len();
        Ok(())
    }
}

fn contains_ascii_case_insensitive(haystack: &str, needle: &[u8]) -> bool {
    let hay = haystack.as_bytes();
    if needle.is_empty() {
        return true;
    }
    hay.windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle))
}

/// Apple (and similar) UEFI: MMIO at QEMU virtio addresses is not mapped; probing faults → panic →
/// apparent GRUB boot loop. Non-Apple (QEMU EDK2, etc.): allow scan.
fn firmware_vendor_likely_apple() -> bool {
    let mut buf = VendorUtf8::new();
    if system::firmware_vendor().as_str_in_buf(&mut buf).is_err() {
        return false;
    }
    let s = core::str::from_utf8(&buf.data[..buf.len]).unwrap_or("");
    contains_ascii_case_insensitive(s, b"apple")
}

fn open_exclusive_or_get<P: ProtocolPointer + ?Sized>(
    handle: uefi::Handle,
) -> uefi::Result<boot::ScopedProtocol<P>> {
    match boot::open_protocol_exclusive::<P>(handle) {
        Ok(p) => Ok(p),
        Err(_) => unsafe {
            boot::open_protocol::<P>(
                OpenProtocolParams {
                    handle,
                    agent: boot::image_handle(),
                    controller: None,
                },
                OpenProtocolAttributes::GetProtocol,
            )
        },
    }
}

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

    kernel::nic::set_allow_virtio_mmio_scan(!firmware_vendor_likely_apple());

    let _ = system::with_stdout(|stdout| {
        let _ = stdout.reset(false);
        let _ = stdout.output_string(cstr16!(
            "Eve OS (AArch64 UEFI) — full UI; virtio-mmio NIC only when firmware allows scan\r\n"
        ));
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

    let mut gop = match open_exclusive_or_get::<GraphicsOutput>(h) {
        Ok(g) => g,
        Err(e) => {
            let _ = system::with_stdout(|s| {
                let _ = s.output_string(cstr16!("open GOP failed (exclusive + get)\r\n"));
            });
            return e.status();
        }
    };

    let mut ptr = ph.and_then(|p| open_exclusive_or_get::<Pointer>(p).ok());
    let mut stdin = kh.and_then(|k| open_exclusive_or_get::<Input>(k).ok());

    const MAX_MODES: usize = 128;
    let mut mode_list: [Option<Mode>; MAX_MODES] = [None; MAX_MODES];
    let mut n_modes = 0usize;
    for m in gop.modes() {
        if n_modes < MAX_MODES {
            mode_list[n_modes] = Some(m);
            n_modes += 1;
        }
    }
    for i in 0..n_modes {
        let mut best = i;
        for j in (i + 1)..n_modes {
            let ra = mode_list[j].unwrap().info().resolution();
            let rb = mode_list[best].unwrap().info().resolution();
            if ra.0.saturating_mul(ra.1) > rb.0.saturating_mul(rb.1) {
                best = j;
            }
        }
        if best != i {
            mode_list.swap(i, best);
        }
    }

    let bpp = 4usize;
    let mut picked: Option<(NonNull<u8>, usize, FrameBufferInfo)> = None;

    if n_modes == 0 {
        let mi = gop.current_mode_info();
        let (gw, gheight) = mi.resolution();
        if let Some(need) = gw
            .checked_mul(gheight)
            .and_then(|n| n.checked_mul(bpp))
        {
            if need > 0 {
                if let Ok(pool) = boot::allocate_pool(MemoryType::LOADER_DATA, need) {
                    picked = Some((
                        pool,
                        need,
                        FrameBufferInfo {
                            width: gw,
                            height: gheight,
                            stride: gw,
                            pixel_format: PixelFormat::Bgr,
                            bytes_per_pixel: bpp,
                        },
                    ));
                }
            }
        }
    }

    if picked.is_none() {
        for i in 0..n_modes {
            let Some(mode) = mode_list[i] else {
                continue;
            };
            if gop.set_mode(&mode).is_err() {
                continue;
            }
            let mi = gop.current_mode_info();
            let (gw, gheight) = mi.resolution();
            let Some(need) = gw
                .checked_mul(gheight)
                .and_then(|n| n.checked_mul(bpp))
            else {
                continue;
            };
            if need == 0 {
                continue;
            }
            if let Ok(pool) = boot::allocate_pool(MemoryType::LOADER_DATA, need) {
                let fb_info = FrameBufferInfo {
                    width: gw,
                    height: gheight,
                    stride: gw,
                    pixel_format: PixelFormat::Bgr,
                    bytes_per_pixel: bpp,
                };
                picked = Some((pool, need, fb_info));
                break;
            }
        }
    }

    let Some((pool, need, fb_info)) = picked else {
        let _ = system::with_stdout(|s| {
            let _ = s.output_string(cstr16!(
                "allocate_pool failed for GOP shadow (tried all modes).\r\n"
            ));
        });
        loop {
            let _ = boot::stall(Duration::from_secs(3600));
        }
    };

    let buf = unsafe { core::slice::from_raw_parts_mut(pool.as_ptr(), need) };

    let (gw, gh) = (fb_info.width, fb_info.height);

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
