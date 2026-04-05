// SPDX-License-Identifier: MIT OR Apache-2.0
//
//! USB host detection + UHCI **HID boot keyboard and mouse** (QEMU `pc` / `q35`).
//! PS/2 stays live until USB proves interrupt IN works (enumeration alone does not block i8042).

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

static mut PREV_KBD_REPORT: [u8; 8] = [0; 8];
const KBD_Q_CAP: usize = 8;
static mut KBD_QUEUE: [(u8, bool); KBD_Q_CAP] = [(0, false); KBD_Q_CAP];
static mut KBD_Q_HEAD: usize = 0;
static mut KBD_Q_TAIL: usize = 0;

#[inline]
pub fn detected() -> UsbHostKind {
    unsafe { DETECTED }
}

/// `phys_skew` = `BootInfo::physical_memory_offset` as `u64` (same as VirtIO DMA skew).
pub unsafe fn init(phys_skew: u64) {
    PREV_KBD_REPORT = [0; 8];
    KBD_Q_HEAD = 0;
    KBD_Q_TAIL = 0;
    // Prefer UHCI when present (QEMU `pc` / `q35` + PIIX/ICH9 companions). Must scan PCI func 1–7;
    // UHCI is often not function 0, which broke BIOS/`pc` and some UTM configs while UEFI/q35 worked.
    if pci::find_usb_uhci_io().is_some() && crate::uhci::init(phys_skew) {
        DETECTED = UsbHostKind::Uhci;
        return;
    }
    DETECTED = match pci::scan_usb_host_prog_if() {
        None => UsbHostKind::None,
        Some(0x00) => UsbHostKind::Uhci,
        Some(0x10) => UsbHostKind::Ohci,
        Some(0x20) => UsbHostKind::Ehci,
        Some(0x30) => UsbHostKind::Xhci,
        Some(pi) => UsbHostKind::Other(pi),
    };
}

/// Boot-protocol mouse for USB HID slot `idx` (0 .. [`usb_mouse_count()`]).
pub unsafe fn poll_hid_slot(idx: usize) -> Option<(u8, i16, i16)> {
    if matches!(detected(), UsbHostKind::Uhci) {
        crate::uhci::poll_mouse_slot(idx)
    } else {
        None
    }
}

/// Number of enumerated USB boot mice (max 12; see `uhci::MAX_USB_MICE`).
pub fn usb_mouse_count() -> usize {
    if matches!(detected(), UsbHostKind::Uhci) {
        crate::uhci::usb_mouse_count()
    } else {
        0
    }
}

/// HID mice enumerated (poll even before the first successful IN). Only when UHCI is in use.
pub fn usb_mouse_active() -> bool {
    matches!(detected(), UsbHostKind::Uhci) && crate::uhci::mouse_ready()
}

/// HID boot keyboard enumerated (endpoint configured). Only when UHCI is in use.
pub fn usb_keyboard_active() -> bool {
    matches!(detected(), UsbHostKind::Uhci) && crate::uhci::keyboard_ready()
}

/// When USB polling is on, ignore PS/2 keyboard only after a real HID boot report IN succeeds.
#[inline]
pub fn usb_ps2_kbd_should_ignore() -> bool {
    matches!(detected(), UsbHostKind::Uhci) && crate::uhci::hid_kbd_suppresses_ps2()
}

/// When USB polling is on, ignore PS/2 mouse only after a real HID mouse IN succeeds.
#[inline]
pub fn usb_ps2_mouse_should_ignore() -> bool {
    matches!(detected(), UsbHostKind::Uhci) && crate::uhci::hid_mouse_suppresses_ps2()
}

/// Next key **press** (make) from the USB boot keyboard, or `None`.
/// Shift is derived from the modifier byte of the report that contained the new key.
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

pub unsafe fn poll_usb_key_press() -> Option<(u8, bool)> {
    if !matches!(detected(), UsbHostKind::Uhci) || !crate::uhci::keyboard_ready() {
        return None;
    }
    if let Some(v) = kbd_q_pop() {
        return Some(v);
    }
    let rep = crate::uhci::poll_keyboard_report()?;
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

/// Map HID keyboard page usage ID to ASCII (US layout). Usage IDs follow USB HID Usage Tables §Keyboard.
pub fn hid_usage_to_ascii(usage: u8, shift: bool) -> Option<u8> {
    match usage {
        0x04..=0x1D => {
            let base = if shift {
                b'A' + (usage - 0x04)
            } else {
                b'a' + (usage - 0x04)
            };
            Some(base)
        }
        0x28 => Some(b'\n'),
        0x2A => Some(0x08),
        0x2C => Some(b' '),
        0x1E..=0x26 => Some(if shift {
            [b'!', b'@', b'#', b'$', b'%', b'^', b'&', b'*', b'('][(usage - 0x1E) as usize]
        } else {
            b'1' + (usage - 0x1E)
        }),
        0x27 => Some(if shift { b')' } else { b'0' }),
        0x2D => Some(if shift { b'_' } else { b'-' }),
        0x2E => Some(if shift { b'+' } else { b'=' }),
        0x2F => Some(if shift { b'{' } else { b'[' }),
        0x30 => Some(if shift { b'}' } else { b']' }),
        0x33 => Some(if shift { b':' } else { b';' }),
        0x34 => Some(if shift { b'"' } else { b'\'' }),
        0x35 => Some(if shift { b'~' } else { b'`' }),
        0x36 => Some(if shift { b'<' } else { b',' }),
        0x37 => Some(if shift { b'>' } else { b'.' }),
        0x38 => Some(if shift { b'?' } else { b'/' }),
        0x31 => Some(if shift { b'|' } else { b'\\' }),
        _ => None,
    }
}

/// QEMU exposes `usb-audio` (PCM), not `usb-midi`; there is no USB MIDI class device for the guest.
pub fn usb_midi_status_label() -> &'static [u8] {
    b"NO USB-MIDI"
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
