// SPDX-License-Identifier: MIT OR Apache-2.0

//! No PC ACPI from AArch64 UEFI — spin.

pub unsafe fn system_reboot() {}

pub unsafe fn system_shutdown() {}

pub fn halt_forever() -> ! {
    loop {
        core::hint::spin_loop();
    }
}
