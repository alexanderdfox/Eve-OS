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
                uart_puts(b"Framebuffer: 32 bpp, drawing splash (use QEMU display or HDMI).\r\n");
                fb::draw_splash(&fbuf);
                let fg = 0x00_FF_FF_FFu32;
                let bg = 0x00_14_1E_2Cu32;
                #[cfg(feature = "soc_pi3")]
                {
                    let lines: [&[u8]; 5] = [
                        b"EVE RASPBERRY PI",
                        b"PROFILE PI3 BCM2837",
                        b"FRAMEBUFFER TEXT GUI",
                        b"ETHERNET NOT IN KERNEL",
                        b"USB LAN ONBOARD TODO",
                    ];
                    fb::draw_text_block(&fbuf, 16, 28, &lines, fg, bg);
                }
                #[cfg(feature = "soc_pi4")]
                {
                    let lines: [&[u8]; 5] = [
                        b"EVE RASPBERRY PI",
                        b"PROFILE PI4 BCM2711",
                        b"FRAMEBUFFER TEXT GUI",
                        b"ETHERNET NOT IN KERNEL",
                        b"GENET DRIVER TODO",
                    ];
                    fb::draw_text_block(&fbuf, 16, 28, &lines, fg, bg);
                }
            }
            None => {
                uart_puts(
                    b"No framebuffer (mailbox failed) - serial only 115200 8N1 on GPIO14/15.\r\n",
                );
            }
        }
        uart_puts(
            b"Note: full Eve UI (browser, PS/2, USB HID, VirtIO net) is the x86_64 guest.\r\n",
        );
        uart_puts(
            b"This Pi image is AArch64 bring-up only - no USB keyboard/mouse stack here yet.\r\n",
        );
        uart_puts(b"====================================\r\n");
    }
    loop {
        unsafe {
            core::arch::asm!("wfi", options(nomem, nostack));
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

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {
        unsafe {
            core::arch::asm!("wfi", options(nomem, nostack));
        }
    }
}
