// SPDX-License-Identifier: MIT OR Apache-2.0

use super::UsbHostKind;
use crate::pci;

#[derive(Clone, Copy, PartialEq, Eq)]
enum UsbActive {
    None,
    Uhci,
    Ohci,
    Ehci,
    Xhci,
}

static mut DETECTED: UsbHostKind = UsbHostKind::None;
static mut ACTIVE: UsbActive = UsbActive::None;

static mut PREV_KBD_REPORT: [u8; 8] = [0; 8];
const KBD_Q_CAP: usize = 8;
static mut KBD_QUEUE: [(u8, bool); KBD_Q_CAP] = [(0, false); KBD_Q_CAP];
static mut KBD_Q_HEAD: usize = 0;
static mut KBD_Q_TAIL: usize = 0;

#[inline]
pub fn detected() -> UsbHostKind {
    unsafe { DETECTED }
}

fn pci_prog_to_kind(pi: u8) -> UsbHostKind {
    match pi {
        0x00 => UsbHostKind::Uhci,
        0x10 => UsbHostKind::Ohci,
        0x20 => UsbHostKind::Ehci,
        0x30 => UsbHostKind::Xhci,
        _ => UsbHostKind::Other(pi),
    }
}

/// `phys_skew` = `BootInfo::physical_memory_offset` as `u64` (same as VirtIO DMA skew).
pub unsafe fn init(phys_skew: u64) {
    PREV_KBD_REPORT = [0; 8];
    KBD_Q_HEAD = 0;
    KBD_Q_TAIL = 0;
    ACTIVE = UsbActive::None;

    DETECTED = match pci::scan_usb_host_prog_if() {
        Some(pi) => pci_prog_to_kind(pi),
        None => UsbHostKind::None,
    };

    if pci::find_usb_uhci_io().is_some() && crate::uhci::init(phys_skew) {
        ACTIVE = UsbActive::Uhci;
        DETECTED = UsbHostKind::Uhci;
        return;
    }
    if crate::xhci::init(phys_skew) {
        ACTIVE = UsbActive::Xhci;
        DETECTED = UsbHostKind::Xhci;
        return;
    }
    if crate::ohci::init(phys_skew) {
        ACTIVE = UsbActive::Ohci;
        DETECTED = UsbHostKind::Ohci;
        return;
    }
    if crate::ehci::init(phys_skew) {
        ACTIVE = UsbActive::Ehci;
        DETECTED = UsbHostKind::Ehci;
        return;
    }
}

pub unsafe fn poll_hid_slot(idx: usize) -> Option<(u8, i16, i16)> {
    match ACTIVE {
        UsbActive::Uhci => crate::uhci::poll_mouse_slot(idx),
        UsbActive::Ohci => crate::ohci::poll_mouse_slot(idx),
        UsbActive::Xhci => crate::xhci::poll_mouse_slot(idx),
        UsbActive::Ehci => crate::ehci::poll_mouse_slot(idx),
        UsbActive::None => None,
    }
}

pub fn usb_mouse_count() -> usize {
    match unsafe { ACTIVE } {
        UsbActive::Uhci => crate::uhci::usb_mouse_count(),
        UsbActive::Ohci => crate::ohci::usb_mouse_count(),
        UsbActive::Xhci => crate::xhci::usb_mouse_count(),
        UsbActive::Ehci => crate::ehci::usb_mouse_count(),
        UsbActive::None => 0,
    }
}

pub fn usb_mouse_active() -> bool {
    match unsafe { ACTIVE } {
        UsbActive::Uhci => crate::uhci::mouse_ready(),
        UsbActive::Ohci => crate::ohci::mouse_ready(),
        UsbActive::Xhci => crate::xhci::mouse_ready(),
        UsbActive::Ehci => crate::ehci::mouse_ready(),
        UsbActive::None => false,
    }
}

pub fn usb_keyboard_active() -> bool {
    match unsafe { ACTIVE } {
        UsbActive::Uhci => crate::uhci::keyboard_ready(),
        UsbActive::Ohci => crate::ohci::keyboard_ready(),
        UsbActive::Xhci => crate::xhci::keyboard_ready(),
        UsbActive::Ehci => crate::ehci::keyboard_ready(),
        UsbActive::None => false,
    }
}

#[inline]
pub fn usb_ps2_kbd_should_ignore() -> bool {
    match unsafe { ACTIVE } {
        UsbActive::Uhci => crate::uhci::hid_kbd_suppresses_ps2(),
        UsbActive::Ohci => crate::ohci::hid_kbd_suppresses_ps2(),
        UsbActive::Xhci => crate::xhci::hid_kbd_suppresses_ps2(),
        UsbActive::Ehci => crate::ehci::hid_kbd_suppresses_ps2(),
        UsbActive::None => false,
    }
}

unsafe fn kbd_q_push(usage: u8, shift: bool) {
    let n = (KBD_Q_TAIL + 1) % KBD_Q_CAP;
    if n == KBD_Q_HEAD {
        return;
    }
    KBD_QUEUE[KBD_Q_TAIL] = (usage, shift);
    KBD_Q_TAIL = n;
}

unsafe fn kbd_q_pop() -> Option<(u8, bool)> {
    if KBD_Q_HEAD == KBD_Q_TAIL {
        return None;
    }
    let v = KBD_QUEUE[KBD_Q_HEAD];
    KBD_Q_HEAD = (KBD_Q_HEAD + 1) % KBD_Q_CAP;
    Some(v)
}

fn poll_keyboard_report_any() -> Option<[u8; 8]> {
    unsafe {
        match ACTIVE {
            UsbActive::Uhci => crate::uhci::poll_keyboard_report(),
            UsbActive::Ohci => crate::ohci::poll_keyboard_report(),
            UsbActive::Xhci => crate::xhci::poll_keyboard_report(),
            UsbActive::Ehci => crate::ehci::poll_keyboard_report(),
            UsbActive::None => None,
        }
    }
}

pub unsafe fn poll_usb_key_press() -> Option<(u8, bool)> {
    if !usb_keyboard_active() {
        return None;
    }
    if let Some(v) = kbd_q_pop() {
        return Some(v);
    }
    let rep = poll_keyboard_report_any()?;
    let shift = rep[0] & 0x22 != 0;
    let old = &PREV_KBD_REPORT[2..8];
    for &k in rep[2..8].iter() {
        if k == 0 {
            continue;
        }
        let mut seen = false;
        for &o in old.iter() {
            if o == k {
                seen = true;
                break;
            }
        }
        if !seen {
            kbd_q_push(k, shift);
        }
    }
    PREV_KBD_REPORT = rep;
    kbd_q_pop()
}

pub fn usb_midi_status_label() -> &'static [u8] {
    b"NO USB-MIDI"
}

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
