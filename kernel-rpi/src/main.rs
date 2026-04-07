// SPDX-License-Identifier: MIT OR Apache-2.0
//
//! AArch64 bring-up for Raspberry Pi: PL011 serial + optional VideoCore framebuffer.
//! Build with `--features soc_pi3` or `--features soc_pi4` for peripheral base.

#![no_std]
#![no_main]

mod fb;
mod font;

use core::arch::global_asm;
use core::panic::PanicInfo;
use core::ptr::{read_volatile, write_volatile};
use kernel::arm_input::{self, ArmKeyEvent};
use kernel::arm_run;
use kernel::fb_info::{FrameBufferInfo, PixelFormat};

#[cfg(all(feature = "soc_pi3", feature = "soc_pi4"))]
compile_error!("enable only one of soc_pi3 or soc_pi4");

#[cfg(not(any(feature = "soc_pi3", feature = "soc_pi4")))]
compile_error!("enable soc_pi3 or soc_pi4");

/// BCM peripheral base (physical).
#[cfg(feature = "soc_pi3")]
const PERI_BASE: usize = 0x3F00_0000;

#[cfg(feature = "soc_pi4")]
const PERI_BASE: usize = 0xFE00_0000;

const GPIO_BASE: usize = PERI_BASE + 0x20_0000;
const UART0_BASE: usize = PERI_BASE + 0x20_1000;

const UART0_DR: usize = UART0_BASE;
const UART0_FR: usize = UART0_BASE + 0x18;
const UART0_IBRD: usize = UART0_BASE + 0x24;
const UART0_FBRD: usize = UART0_BASE + 0x28;
const UART0_LCRH: usize = UART0_BASE + 0x2C;
const UART0_CR: usize = UART0_BASE + 0x30;
const UART0_RSR: usize = UART0_BASE + 0x04;

const GPFSEL1: usize = GPIO_BASE + 0x04;

const MBOX_BASE: usize = PERI_BASE + 0xB880;

#[derive(Clone, Copy)]
struct InputState {
    ptr_x: i32,
    ptr_y: i32,
    ptr_btn: u8,
    term_cols: i32,
    term_rows: i32,
}

impl InputState {
    const fn new() -> Self {
        Self {
            ptr_x: 0,
            ptr_y: 0,
            ptr_btn: 0,
            term_cols: 80,
            term_rows: 24,
        }
    }
}

global_asm!(
    r#"
.section .text.boot
.global _start
_start:
    /* Stack grows down from __stack_top (link.ld). */
    ldr     x1, =__stack_top
    mov     sp, x1
    /* Clear .bss */
    ldr     x1, =__bss_start
    ldr     x2, =__bss_end
0:
    cmp     x1, x2
    b.ge    1f
    str     xzr, [x1], #8
    b       0b
1:
    bl      rust_entry
2:
    wfi
    b       2b
"#
);

#[no_mangle]
pub extern "C" fn rust_entry() -> ! {
    unsafe {
        gpio_uart_pins();
        uart0_init();
        uart_puts(b"\r\n=== EVE / Raspberry Pi AArch64 ===\r\n");
        #[cfg(feature = "soc_pi3")]
        uart_puts(b"SoC profile: BCM2837 family (Pi 3 / Zero 2 / 3B+)\r\n");
        #[cfg(feature = "soc_pi4")]
        uart_puts(b"SoC profile: BCM2711 (Pi 4 / 400)\r\n");
        match fb::init(MBOX_BASE, 640, 480) {
            Some(fbuf) => {
                uart_puts(b"Framebuffer: 32 bpp. Running shared arm_run UI loop.\r\n");
                arm_run::set_bootstrap_platform_caps(kernel::settings::PlatformCaps::rpi());
                let fb_info = FrameBufferInfo {
                    width: fbuf.width as usize,
                    height: fbuf.height as usize,
                    stride: (fbuf.pitch_bytes as usize) / core::mem::size_of::<u32>(),
                    pixel_format: PixelFormat::Rgb,
                    bytes_per_pixel: core::mem::size_of::<u32>(),
                };
                let fb_len = (fbuf.pitch_bytes as usize).saturating_mul(fbuf.height as usize);
                let fb_bytes = core::slice::from_raw_parts_mut(fbuf.ptr as *mut u8, fb_len);
                let cx = (fb_info.width / 2) as i32;
                let cy = (fb_info.height / 2) as i32;
                let mut input = InputState::new();
                input.ptr_x = cx;
                input.ptr_y = cy;
                // xterm mouse tracking so serial terminals can forward pointer clicks/moves.
                uart_puts(b"\x1b[?1000h\x1b[?1002h\x1b[?1006h");
                loop {
                    arm_input::key_queue_reset();
                    drain_uart_input_to_arm_queue(&mut input, &fb_info);
                    arm_input::set_pointer_abs(input.ptr_x, input.ptr_y, input.ptr_btn);
                    arm_run::main_step(fb_bytes, &fb_info);
                }
            }
            None => {
                uart_puts(
                    b"No framebuffer (mailbox failed) - serial only 115200 8N1 on GPIO14/15.\r\n",
                );
                loop {
                    core::arch::asm!("wfi", options(nomem, nostack));
                }
            }
        }
    }
}

unsafe fn gpio_uart_pins() {
    let mut sel = read_volatile(GPFSEL1 as *const u32);
    sel &= !((7 << 12) | (7 << 15));
    sel |= (4 << 12) | (4 << 15);
    write_volatile(GPFSEL1 as *mut u32, sel);
}

unsafe fn uart0_init() {
    write_volatile(UART0_CR as *mut u32, 0);
    write_volatile(UART0_RSR as *mut u32, 0);
    write_volatile(UART0_IBRD as *mut u32, 26);
    write_volatile(UART0_FBRD as *mut u32, 3);
    write_volatile(UART0_LCRH as *mut u32, 0x70);
    write_volatile(UART0_CR as *mut u32, 0x301);
}

unsafe fn uart_putc(b: u8) {
    while read_volatile(UART0_FR as *const u32) & (1 << 5) != 0 {}
    write_volatile(UART0_DR as *mut u32, u32::from(b));
}

unsafe fn uart_puts(s: &[u8]) {
    for &b in s {
        if b == b'\n' {
            uart_putc(b'\r');
        }
        uart_putc(b);
    }
}

unsafe fn uart_try_getc() -> Option<u8> {
    if read_volatile(UART0_FR as *const u32) & (1 << 4) != 0 {
        None
    } else {
        Some((read_volatile(UART0_DR as *const u32) & 0xFF) as u8)
    }
}

fn map_csi_function_key(seq: &[u8]) -> Option<u8> {
    match seq {
        b"[11~" | b"[[A" => Some(1),
        b"[12~" | b"[[B" => Some(2),
        b"[13~" | b"[[C" => Some(3),
        b"[14~" | b"[[D" => Some(4),
        b"[15~" | b"[[E" => Some(5),
        b"[17~" => Some(6),
        b"[18~" => Some(7),
        b"[19~" => Some(8),
        b"[20~" => Some(9),
        b"[21~" => Some(10),
        b"[23~" => Some(11),
        b"[24~" => Some(12),
        _ => None,
    }
}

fn parse_u16_ascii(s: &[u8]) -> Option<u16> {
    if s.is_empty() {
        return None;
    }
    let mut v: u16 = 0;
    for &b in s {
        if !b.is_ascii_digit() {
            return None;
        }
        v = v.saturating_mul(10).saturating_add(u16::from(b - b'0'));
    }
    Some(v)
}

fn parse_sgr_mouse(seq: &[u8]) -> Option<(u16, u16, u8)> {
    // CSI <btn;col;row M/m
    if seq.len() < 7 || seq[0] != b'[' || seq[1] != b'<' {
        return None;
    }
    let fin = *seq.last()?;
    if fin != b'M' && fin != b'm' {
        return None;
    }
    let body = &seq[2..seq.len() - 1];
    let mut it = body.split(|&b| b == b';');
    let btn = parse_u16_ascii(it.next()?)?;
    let col = parse_u16_ascii(it.next()?)?;
    let row = parse_u16_ascii(it.next()?)?;
    if it.next().is_some() {
        return None;
    }
    let mut mask = 0u8;
    if fin == b'M' {
        match btn & 0x03 {
            0 => mask = 1, // left
            2 => mask = 2, // right
            _ => {}
        }
    }
    Some((col, row, mask))
}

fn apply_csi_sequence(input: &mut InputState, info: &FrameBufferInfo, seq: &[u8]) {
    match seq {
        b"[A" => arm_input::key_queue_push(ArmKeyEvent::ArrowUp),
        b"[B" => arm_input::key_queue_push(ArmKeyEvent::ArrowDown),
        b"[5~" => arm_input::key_queue_push(ArmKeyEvent::PageUp),
        b"[6~" => arm_input::key_queue_push(ArmKeyEvent::PageDown),
        b"[H" => arm_input::key_queue_push(ArmKeyEvent::Escape),
        b"[F" => arm_input::key_queue_push(ArmKeyEvent::Escape),
        _ => {
            if let Some(f) = map_csi_function_key(seq) {
                arm_input::key_queue_push(ArmKeyEvent::Func(f));
                return;
            }
            if let Some((col, row, btn)) = parse_sgr_mouse(seq) {
                input.term_cols = input.term_cols.max(i32::from(col));
                input.term_rows = input.term_rows.max(i32::from(row));
                let max_x = (info.width.saturating_sub(1)) as i32;
                let max_y = (info.height.saturating_sub(1)) as i32;
                let denom_x = (input.term_cols - 1).max(1);
                let denom_y = (input.term_rows - 1).max(1);
                input.ptr_x = (((i32::from(col).saturating_sub(1)).saturating_mul(max_x)) / denom_x)
                    .clamp(0, max_x);
                input.ptr_y = (((i32::from(row).saturating_sub(1)).saturating_mul(max_y)) / denom_y)
                    .clamp(0, max_y);
                input.ptr_btn = btn;
            }
        }
    }
}

unsafe fn drain_uart_input_to_arm_queue(input: &mut InputState, info: &FrameBufferInfo) {
    // ANSI parser for serial terminals (arrows/F-keys/PageUp/PageDown + xterm SGR mouse).
    const ESC_BUF_CAP: usize = 32;
    static mut ESC_ACTIVE: bool = false;
    static mut ESC_BUF: [u8; ESC_BUF_CAP] = [0; ESC_BUF_CAP];
    static mut ESC_LEN: usize = 0;
    while let Some(c) = uart_try_getc() {
        if ESC_ACTIVE {
            if ESC_LEN < ESC_BUF_CAP {
                ESC_BUF[ESC_LEN] = c;
                ESC_LEN += 1;
            } else {
                ESC_ACTIVE = false;
                ESC_LEN = 0;
                arm_input::key_queue_push(ArmKeyEvent::Escape);
                continue;
            }
            let final_byte = (0x40..=0x7E).contains(&c);
            if final_byte {
                let seq = &ESC_BUF[..ESC_LEN];
                apply_csi_sequence(input, info, seq);
                ESC_ACTIVE = false;
                ESC_LEN = 0;
            }
            continue;
        }
        match c {
            0x1B => {
                ESC_ACTIVE = true;
                ESC_LEN = 0;
            }
            b'\r' | b'\n' => arm_input::key_queue_push(ArmKeyEvent::Enter),
            0x7F | 0x08 => arm_input::key_queue_push(ArmKeyEvent::Backspace),
            b' '..=b'~' => arm_input::key_queue_push(ArmKeyEvent::Char(c)),
            _ => {}
        }
    }
    if ESC_ACTIVE && ESC_LEN == 0 {
        // Bare ESC key
        ESC_ACTIVE = false;
        arm_input::key_queue_push(ArmKeyEvent::Escape);
    }
}

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {
        unsafe {
            // Disable xterm mouse tracking before freezing, so host terminal is restored.
            uart_puts(b"\x1b[?1000l\x1b[?1002l\x1b[?1006l");
            core::arch::asm!("wfi", options(nomem, nostack));
        }
    }
}
