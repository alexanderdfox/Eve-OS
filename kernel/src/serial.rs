// SPDX-License-Identifier: MIT OR Apache-2.0
//
//! Polled serial: **COM1** on x86 (32/64-bit); no-op on AArch64 (use UEFI ConOut in the app).

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
mod x86 {
    use crate::ports::{inb, outb};

    const COM1: u16 = 0x3F8;
    const LSR: u16 = COM1 + 5;

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
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
pub use x86::*;

#[cfg(not(any(target_arch = "x86_64", target_arch = "x86")))]
mod stub {
    pub unsafe fn init() {}

    pub fn put_byte(_b: u8) {}

    pub fn puts(_s: &[u8]) {}
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "x86")))]
pub use stub::*;
