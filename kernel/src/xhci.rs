// SPDX-License-Identifier: MIT OR Apache-2.0
//
//! xHCI (USB 3.x) host — **placeholder** (enumeration + HID IN not wired yet).
//!
//! Laptops typically expose **only xHCI**; a full driver needs command/event rings, slot contexts,
//! and endpoint transfer rings (large). Prefer **OHCI/UHCI** companions in QEMU until this is
//! completed, or use PS/2 input.

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

pub fn hid_mouse_suppresses_ps2() -> bool {
    false
}
