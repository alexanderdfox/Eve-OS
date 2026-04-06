// SPDX-License-Identifier: MIT OR Apache-2.0

//! Guest **power off** and **reset** for QEMU / typical PC VMs.
//!
//! - **Shutdown:** ACPI sleep control on common QEMU PIIX/ICH layouts (`0x604`, `0xB004`).
//! - **Reboot:** PS/2 keyboard controller CPU reset pulse (`0x64` ← `0xFE`).
//!
//! Real hardware may need firmware-specific ACPI; if nothing works we fall back to **HLT**.

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

/// Cold reset via legacy keyboard controller (works in QEMU, many PCs).
pub unsafe fn system_reboot() {
    ps2_wait_input_clear();
    outb(PS2_CTL, 0xFE);
    for _ in 0..5_000_000 {
        core::hint::spin_loop();
    }
}

/// Try ACPI-style shutdown ports used by QEMU `pc` / `q35`.
pub unsafe fn system_shutdown() {
    // PM1a_CNT.SLP_EN | SLP_TYP — common QEMU defaults (see OSDev / QEMU ACPI).
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
