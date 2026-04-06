// SPDX-License-Identifier: MIT OR Apache-2.0
//
//! EHCI (USB 2.0) host — **not implemented** for HID yet.
//!
//! Full-speed / low-speed HID behind EHCI normally uses **split transactions** to a **companion**
//! UHCI/OHCI controller, or the OS routes ports to the companion. Implementing reliable FS interrupt
//! IN through EHCI alone is a large project; use **xHCI** or **OHCI/UHCI** companions today.

/// `skew` = `BootInfo::physical_memory_offset` (unused).
pub unsafe fn init(_skew: u64) -> bool {
    false
}

pub fn keyboard_ready() -> bool {
    false
}

pub fn mouse_ready() -> bool {
    false
}

pub fn usb_mouse_count() -> usize {
    0
}

pub unsafe fn poll_mouse_slot(_idx: usize) -> Option<(u8, i16, i16)> {
    None
}

pub unsafe fn poll_keyboard_report() -> Option<[u8; 8]> {
    None
}

pub fn hid_kbd_suppresses_ps2() -> bool {
    false
}

