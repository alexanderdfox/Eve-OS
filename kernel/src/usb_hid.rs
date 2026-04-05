// SPDX-License-Identifier: MIT OR Apache-2.0
//
//! USB host controller presence (PCI). Full HID enumeration (UHCI/xHCI) is not wired yet;
//! PS/2 remains the active path. `-usb -device usb-kbd -device usb-mouse` in QEMU attaches
//! devices the guest could claim once this stack grows.

use crate::pci;

/// Programming interface from PCI class 0x0C03.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum UsbHostKind {
    None,
    Uhci,
    Ohci,
    Ehci,
    Xhci,
    Other(u8),
}

static mut DETECTED: UsbHostKind = UsbHostKind::None;

#[inline]
pub fn detected() -> UsbHostKind {
    unsafe { DETECTED }
}

pub unsafe fn init() {
    DETECTED = match pci::scan_usb_host_prog_if() {
        None => UsbHostKind::None,
        Some(0x00) => UsbHostKind::Uhci,
        Some(0x10) => UsbHostKind::Ohci,
        Some(0x20) => UsbHostKind::Ehci,
        Some(0x30) => UsbHostKind::Xhci,
        Some(pi) => UsbHostKind::Other(pi),
    };
}

/// Reserved for future UHCI/xHCI interrupt / bulk polling. Returns no events today.
pub unsafe fn poll_hid() -> Option<()> {
    None
}

/// Short status label for the settings UI.
pub fn host_label() -> &'static [u8] {
    match detected() {
        UsbHostKind::None => b"USB NO",
        UsbHostKind::Uhci => b"USB UHCI",
        UsbHostKind::Ohci => b"USB OHCI",
        UsbHostKind::Ehci => b"USB EHCI",
        UsbHostKind::Xhci => b"USB XHCI",
        UsbHostKind::Other(_) => b"USB PCI",
    }
}
