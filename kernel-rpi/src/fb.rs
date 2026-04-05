// SPDX-License-Identifier: MIT OR Apache-2.0
//
//! Framebuffer via VideoCore mailbox (property channel). Works on Pi 3 / 4 family
//! firmware and QEMU `raspi3b` / `raspi4b` when the mailbox is emulated.

use core::ptr::{read_volatile, write_volatile};

const MBOX_READ: usize = 0x00;
const MBOX_STATUS: usize = 0x18;
const MBOX_WRITE: usize = 0x20;

const MBOX_FULL: u32 = 1 << 31;
const MBOX_EMPTY: u32 = 1 << 30;
const MBOX_CH_PROP: u32 = 8;

// IDs match Linux `raspberrypi-firmware.h` / QEMU `raspberrypi-fw-defs.h` (not older BCM docs).
const TAG_SET_PHYS: u32 = 0x0004_8003;
const TAG_SET_VIRT: u32 = 0x0004_8004;
const TAG_SET_DEPTH: u32 = 0x0004_8005;
const TAG_SET_ORDER: u32 = 0x0004_8006;
const TAG_ALLOC: u32 = 0x0004_0001;
const TAG_GET_PITCH: u32 = 0x0004_0008;

const MBOX_SUCCESS: u32 = 0x8000_0000;

/// GPU-reported framebuffer base -> ARM physical (VC bus alias or plain phys).
#[inline]
fn gpu_fb_base_to_phys(gpu: u32) -> usize {
    if gpu == 0 {
        return 0;
    }
    // Property channel often returns ARM-visible SDRAM; VC bus uses 0x4xxxxxxx / 0xCxxxxxxx aliases.
    (gpu & 0x3FFF_FFFF) as usize
}

unsafe fn mbox_call(mbox_base: usize, buf: *mut u32, channel: u32) -> bool {
    // Property tags (channel 8): mailbox carries the *physical* buffer pointer (RPi wiki). Other
    // channels use VC bus addresses; we only use channel 8 here.
    let lo = (buf as usize as u32) & !0xF;
    let msg = lo | (channel & 0xF);

    while read_volatile((mbox_base + MBOX_STATUS) as *const u32) & MBOX_FULL != 0 {
        core::hint::spin_loop();
    }
    write_volatile((mbox_base + MBOX_WRITE) as *mut u32, msg);

    loop {
        while read_volatile((mbox_base + MBOX_STATUS) as *const u32) & MBOX_EMPTY != 0 {
            core::hint::spin_loop();
        }
        let raw = read_volatile((mbox_base + MBOX_READ) as *const u32);
        if raw & 0xF == channel {
            return read_volatile(buf.add(1)) == MBOX_SUCCESS;
        }
    }
}

pub struct Framebuffer {
    pub ptr: *mut u32,
    pub width: u32,
    pub height: u32,
    pub pitch_bytes: u32,
}

/// Try to set a 32 bpp framebuffer and return CPU pointer + pitch. On failure returns `None`.
pub unsafe fn init(mbox_base: usize, width: u32, height: u32) -> Option<Framebuffer> {
    #[repr(C, align(16))]
    struct MboxBuf {
        words: [u32; 32],
    }
    let mut mb = MboxBuf { words: [0; 32] };

    // Layout matches common Pi bare-metal examples (property tags in one request).
    mb.words[0] = 0; // size filled below
    mb.words[1] = 0; // request
    mb.words[2] = TAG_SET_PHYS;
    mb.words[3] = 8;
    mb.words[4] = 8;
    mb.words[5] = width;
    mb.words[6] = height;
    mb.words[7] = TAG_SET_VIRT;
    mb.words[8] = 8;
    mb.words[9] = 8;
    mb.words[10] = width;
    mb.words[11] = height;
    mb.words[12] = TAG_SET_DEPTH;
    mb.words[13] = 4;
    mb.words[14] = 4;
    mb.words[15] = 32;
    mb.words[16] = TAG_SET_ORDER;
    mb.words[17] = 4;
    mb.words[18] = 4;
    mb.words[19] = 1; // RGB (byte order R, G, R in LE low bytes)
    mb.words[20] = TAG_ALLOC;
    mb.words[21] = 8;
    mb.words[22] = 4;
    mb.words[23] = 4096;
    // Words 23–24 are the alloc value buffer (base + size after response).
    mb.words[25] = TAG_GET_PITCH;
    mb.words[26] = 4;
    mb.words[27] = 0;
    mb.words[28] = 0; // pitch (u32) written here by VC
    mb.words[29] = 0; // end tag
    mb.words[0] = 30 * 4;

    let p = mb.words.as_mut_ptr();
    if !mbox_call(mbox_base, p, MBOX_CH_PROP) {
        return None;
    }

    let pitch = read_volatile(p.add(28));
    let gpu_base = read_volatile(p.add(23));
    if pitch == 0 || gpu_base == 0 {
        return None;
    }

    let phys = gpu_fb_base_to_phys(gpu_base);
    if phys == 0 {
        return None;
    }

    Some(Framebuffer {
        ptr: phys as *mut u32,
        width: read_volatile(p.add(5)),
        height: read_volatile(p.add(6)),
        pitch_bytes: pitch,
    })
}

/// Clear the framebuffer to a single 32-bit color (see `TAG_SET_ORDER` in `init`).
#[allow(dead_code)]
pub unsafe fn fill32(fb: &Framebuffer, color: u32) {
    let w = fb.width as usize;
    let h = fb.height as usize;
    let pitch_u32 = fb.pitch_bytes as usize / core::mem::size_of::<u32>();

    for y in 0..h {
        let row = fb.ptr.add(y * pitch_u32);
        for x in 0..w {
            write_volatile(row.add(x), color);
        }
    }
}

/// Simple horizontal gradient + two horizontal bars so output is visible even if color depth differs.
pub unsafe fn draw_splash(fb: &Framebuffer) {
    let w = fb.width as usize;
    let h = fb.height as usize;
    let pitch_u32 = fb.pitch_bytes as usize / core::mem::size_of::<u32>();

    for y in 0..h {
        let row = fb.ptr.add(y * pitch_u32);
        let g = ((y * 255) / h.max(1)) as u32;
        let bg = 0x0010_2040u32 | (g << 8);
        for x in 0..w {
            let t = ((x * 80) / w.max(1)) as u32;
            let c = bg.wrapping_add(t << 16);
            write_volatile(row.add(x), c);
        }
    }
    let bar = |y0: usize, y1: usize, c: u32| {
        for y in y0..y1.min(h) {
            let row = fb.ptr.add(y * pitch_u32);
            for x in 0..w {
                write_volatile(row.add(x), c);
            }
        }
    };
    bar(h / 8, h / 8 + 24, 0x00_F0_A0_40);
    bar(h - 48, h - 24, 0x00_40_A0_F0);
}
