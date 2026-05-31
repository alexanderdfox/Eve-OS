// SPDX-License-Identifier: MIT OR Apache-2.0
//
//! EHCI (USB 2.0) host — **HID not implemented** (explicit companion failure mode).
//!
//! Full-speed / low-speed HID behind EHCI normally uses **split transactions** to a **companion**
//! UHCI/OHCI controller, or the OS routes ports to the companion. Eve does **not** implement FS
//! interrupt IN through EHCI; when firmware exposes only EHCI behind xHCI, **`xhci_native`**
//! handles HID on the xHCI root ports instead. See `install/REAL-HARDWARE.md`.

use crate::diag_log;
use crate::pci::{self, USB_PI_EHCI};

/// `skew` = `BootInfo::physical_memory_offset` (unused for bring-up).
pub unsafe fn init(_skew: u64) -> bool {
    if pci::find_usb_host_mmio_bar0(USB_PI_EHCI).is_some() {
        diag_log::line(b"ehci: companion present; hid N/I");
    }
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

