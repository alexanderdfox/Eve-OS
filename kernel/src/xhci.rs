// SPDX-License-Identifier: MIT OR Apache-2.0
//
//! xHCI (USB 3.x) host.
//!
//! When an xHCI PCI function is present, Eve tries **companion** OHCI/UHCI/EHCI first (legacy routing).
//! If none attach, **`xhci_native`** brings up a minimal native path (boot keyboard on root port).

use crate::diag_log;
use crate::pci::{self, USB_PI_XHCI};

#[path = "xhci_native.rs"]
mod xhci_native;

#[derive(Clone, Copy, PartialEq, Eq)]
enum XhciBackend {
    None,
    Ohci,
    Uhci,
    Ehci,
    Native,
}

static mut BACKEND: XhciBackend = XhciBackend::None;

fn log_backend(kind: &[u8]) {
    diag_log::line2(b"xhci: ", kind);
}

/// `skew` = `BootInfo::physical_memory_offset`.
pub unsafe fn init(skew: u64) -> bool {
    BACKEND = XhciBackend::None;
    let Some((bus, slot, func, bar)) = pci::find_usb_host_mmio_bar0(USB_PI_XHCI) else {
        return false;
    };
    let _ = (bus, slot, func);
    log_backend(b"pci fn found");

    if crate::ohci::init(skew) {
        BACKEND = XhciBackend::Ohci;
        log_backend(b"companion OHCI");
        return true;
    }
    if crate::uhci::init(skew) {
        BACKEND = XhciBackend::Uhci;
        log_backend(b"companion UHCI");
        return true;
    }
    if crate::ehci::init(skew) {
        BACKEND = XhciBackend::Ehci;
        log_backend(b"companion EHCI");
        return true;
    }

    log_backend(b"no companion; try native");
    pci::pci_enable_mmio_bm(bus, slot, func);
    let mmio = pci::pci_mmio_kernel_addr(Some(skew), bar as usize);
    if xhci_native::init(skew, mmio) {
        BACKEND = XhciBackend::Native;
        return true;
    }
    false
}

pub fn backend_label() -> &'static [u8] {
    match unsafe { BACKEND } {
        XhciBackend::Ohci => b"XHCI+OHCI",
        XhciBackend::Uhci => b"XHCI+UHCI",
        XhciBackend::Ehci => b"XHCI+EHCI",
        XhciBackend::Native => b"XHCI NAT",
        XhciBackend::None => b"XHCI ---",
    }
}

pub fn keyboard_ready() -> bool {
    unsafe {
        match BACKEND {
            XhciBackend::Ohci => crate::ohci::keyboard_ready(),
            XhciBackend::Uhci => crate::uhci::keyboard_ready(),
            XhciBackend::Ehci => crate::ehci::keyboard_ready(),
            XhciBackend::Native => xhci_native::keyboard_ready(),
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
            XhciBackend::Native => xhci_native::mouse_ready(),
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
            XhciBackend::Native => xhci_native::usb_mouse_count(),
            XhciBackend::None => 0,
        }
    }
}

pub unsafe fn poll_mouse_slot(idx: usize) -> Option<(u8, i16, i16)> {
    match BACKEND {
        XhciBackend::Ohci => crate::ohci::poll_mouse_slot(idx),
        XhciBackend::Uhci => crate::uhci::poll_mouse_slot(idx),
        XhciBackend::Ehci => crate::ehci::poll_mouse_slot(idx),
        XhciBackend::Native => xhci_native::poll_mouse_slot(idx),
        XhciBackend::None => None,
    }
}

pub unsafe fn poll_keyboard_report() -> Option<[u8; 8]> {
    match BACKEND {
        XhciBackend::Ohci => crate::ohci::poll_keyboard_report(),
        XhciBackend::Uhci => crate::uhci::poll_keyboard_report(),
        XhciBackend::Ehci => crate::ehci::poll_keyboard_report(),
        XhciBackend::Native => xhci_native::poll_keyboard_report(),
        XhciBackend::None => None,
    }
}

pub fn hid_kbd_suppresses_ps2() -> bool {
    unsafe {
        match BACKEND {
            XhciBackend::Ohci => crate::ohci::hid_kbd_suppresses_ps2(),
            XhciBackend::Uhci => crate::uhci::hid_kbd_suppresses_ps2(),
            XhciBackend::Ehci => crate::ehci::hid_kbd_suppresses_ps2(),
            XhciBackend::Native => xhci_native::hid_kbd_suppresses_ps2(),
            XhciBackend::None => false,
        }
    }
}
