// SPDX-License-Identifier: MIT OR Apache-2.0
//
//! xHCI (USB 3.x) host.
//!
//! A native xHCI command/event/endpoint ring implementation is still in progress. Meanwhile, when an
//! xHCI PCI function is present, this module can route HID polling through initialized companion stacks
//! (OHCI/UHCI/EHCI) so input remains usable on more hosts while preserving the xHCI selection path.

use crate::pci::{self, USB_PI_XHCI};

#[derive(Clone, Copy, PartialEq, Eq)]
enum XhciBackend {
    None,
    Ohci,
    Uhci,
    Ehci,
}

static mut BACKEND: XhciBackend = XhciBackend::None;

/// `skew` = `BootInfo::physical_memory_offset`.
pub unsafe fn init(skew: u64) -> bool {
    BACKEND = XhciBackend::None;
    // Require an actual xHCI function to activate this path.
    if pci::find_usb_host_mmio_bar0(USB_PI_XHCI).is_none() {
        return false;
    }
    // Companion fallback path while native xHCI transfers are being completed.
    if crate::ohci::init(skew) {
        BACKEND = XhciBackend::Ohci;
        return true;
    }
    if crate::uhci::init(skew) {
        BACKEND = XhciBackend::Uhci;
        return true;
    }
    if crate::ehci::init(skew) {
        BACKEND = XhciBackend::Ehci;
        return true;
    }
    false
}

pub fn keyboard_ready() -> bool {
    unsafe {
        match BACKEND {
            XhciBackend::Ohci => crate::ohci::keyboard_ready(),
            XhciBackend::Uhci => crate::uhci::keyboard_ready(),
            XhciBackend::Ehci => crate::ehci::keyboard_ready(),
            XhciBackend::None => false,
        }
    }
}

pub fn mouse_ready() -> bool {
    unsafe {
        match BACKEND {
            XhciBackend::Ohci => crate::ohci::mouse_ready(),
            XhciBackend::Uhci => crate::uhci::mouse_ready(),
            XhciBackend::Ehci => crate::ehci::mouse_ready(),
            XhciBackend::None => false,
        }
    }
}

pub fn usb_mouse_count() -> usize {
    unsafe {
        match BACKEND {
            XhciBackend::Ohci => crate::ohci::usb_mouse_count(),
            XhciBackend::Uhci => crate::uhci::usb_mouse_count(),
            XhciBackend::Ehci => crate::ehci::usb_mouse_count(),
            XhciBackend::None => 0,
        }
    }
}

pub unsafe fn poll_mouse_slot(idx: usize) -> Option<(u8, i16, i16)> {
    match BACKEND {
        XhciBackend::Ohci => crate::ohci::poll_mouse_slot(idx),
        XhciBackend::Uhci => crate::uhci::poll_mouse_slot(idx),
        XhciBackend::Ehci => crate::ehci::poll_mouse_slot(idx),
        XhciBackend::None => None,
    }
}

pub unsafe fn poll_keyboard_report() -> Option<[u8; 8]> {
    match BACKEND {
        XhciBackend::Ohci => crate::ohci::poll_keyboard_report(),
        XhciBackend::Uhci => crate::uhci::poll_keyboard_report(),
        XhciBackend::Ehci => crate::ehci::poll_keyboard_report(),
        XhciBackend::None => None,
    }
}

pub fn hid_kbd_suppresses_ps2() -> bool {
    unsafe {
        match BACKEND {
            XhciBackend::Ohci => crate::ohci::hid_kbd_suppresses_ps2(),
            XhciBackend::Uhci => crate::uhci::hid_kbd_suppresses_ps2(),
            XhciBackend::Ehci => crate::ehci::hid_kbd_suppresses_ps2(),
            XhciBackend::None => false,
        }
    }
}

