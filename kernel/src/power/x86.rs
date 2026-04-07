// SPDX-License-Identifier: MIT OR Apache-2.0

//! ACPI + PS/2 reset (x86_64).

use crate::ports::{inb, outb, outw};

const PS2_CTL: u16 = 0x64;

unsafe fn ps2_wait_input_clear() {
    for _ in 0..200_000 {
        if inb(PS2_CTL) & 2 == 0 {
            return;
        }
        core::hint::spin_loop();
    }
}

pub unsafe fn system_reboot() {
    ps2_wait_input_clear();
    outb(PS2_CTL, 0xFE);
    for _ in 0..5_000_000 {
        core::hint::spin_loop();
    }
}

pub unsafe fn system_shutdown() {
    outw(0x604, 0x2000);
    outw(0xB004, 0x2000);
    for _ in 0..50_000 {
        core::hint::spin_loop();
    }
}

pub fn halt_forever() -> ! {
    unsafe {
        core::arch::asm!("cli", options(nomem, nostack, preserves_flags));
    }
    loop {
        unsafe {
            core::arch::asm!("hlt", options(nomem, nostack, preserves_flags));
        }
    }
}
