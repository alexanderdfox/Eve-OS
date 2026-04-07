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
                loop {
                    arm_input::key_queue_reset();
                    drain_uart_keys_to_arm_queue();
                    arm_input::set_pointer_abs(cx, cy, 0);
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

unsafe fn drain_uart_keys_to_arm_queue() {
    // ANSI escape parser for serial terminals:
    // ESC [ A/B/C/D (arrows), ESC [ 5~ / 6~ (PageUp/Down)
    static mut ESC_STATE: u8 = 0;
    while let Some(c) = uart_try_getc() {
        match ESC_STATE {
            0 => match c {
                0x1B => ESC_STATE = 1,
                b'\r' | b'\n' => arm_input::key_queue_push(ArmKeyEvent::Enter),
                0x7F | 0x08 => arm_input::key_queue_push(ArmKeyEvent::Backspace),
                0x20..=0x7E => arm_input::key_queue_push(ArmKeyEvent::Char(c)),
                _ => {}
            },
            1 => {
                if c == b'[' {
                    ESC_STATE = 2;
                } else {
                    ESC_STATE = 0;
                    if c == 0x1B {
                        ESC_STATE = 1;
                    } else {
                        arm_input::key_queue_push(ArmKeyEvent::Escape);
                    }
                }
            }
            _ => {
                ESC_STATE = 0;
                match c {
                    b'A' => arm_input::key_queue_push(ArmKeyEvent::ArrowUp),
                    b'B' => arm_input::key_queue_push(ArmKeyEvent::ArrowDown),
                    b'5' => {
                        if let Some(b'~') = uart_try_getc() {
                            arm_input::key_queue_push(ArmKeyEvent::PageUp);
                        }
                    }
                    b'6' => {
                        if let Some(b'~') = uart_try_getc() {
                            arm_input::key_queue_push(ArmKeyEvent::PageDown);
                        }
                    }
                    _ => arm_input::key_queue_push(ArmKeyEvent::Escape),
                }
            }
        }
    }
}

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {
        unsafe {
            core::arch::asm!("wfi", options(nomem, nostack));
        }
    }
}
