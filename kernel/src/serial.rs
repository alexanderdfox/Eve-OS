// SPDX-License-Identifier: MIT OR Apache-2.0
//
//! PC COM1 (0x3F8) polled UART — for bare-metal bring-up when there is no GOP/VESA framebuffer.
//! Many laptops do not route the internal UART to an external jack; use USB–serial on a desktop
//! board or QEMU `-serial stdio` when testing.

use crate::ports::{inb, outb};

const COM1: u16 = 0x3F8;
const LSR: u16 = COM1 + 5;

/// Initialize COM1 at **115200 8N1**. Safe to call on unknown hardware (port I/O only).
pub unsafe fn init() {
    outb(COM1 + 1, 0x00);
    outb(COM1 + 3, 0x80);
    outb(COM1 + 0, 0x01);
    outb(COM1 + 1, 0x00);
    outb(COM1 + 3, 0x03);
    outb(COM1 + 2, 0xC7);
    outb(COM1 + 4, 0x0B);
}

#[inline]
fn tx_empty() -> bool {
    unsafe { (inb(LSR) & 0x20) != 0 }
}

pub fn put_byte(b: u8) {
    let mut spins = 0u32;
    while !tx_empty() {
        spins = spins.wrapping_add(1);
        if spins > 10_000_000 {
            return;
        }
        core::hint::spin_loop();
    }
    unsafe {
        outb(COM1, b);
    }
}

pub fn puts(s: &[u8]) {
    for &b in s {
        if b == b'\n' {
            put_byte(b'\r');
        }
        put_byte(b);
    }
}
