// SPDX-License-Identifier: MIT OR Apache-2.0

use super::UsbHostKind;

pub unsafe fn init(_phys_skew: u64) {}

#[inline]
pub fn detected() -> UsbHostKind {
    UsbHostKind::None
}

pub unsafe fn poll_hid_slot(_idx: usize) -> Option<(u8, i16, i16)> {
    None
}

pub fn usb_mouse_count() -> usize {
    0
}

pub fn usb_mouse_active() -> bool {
    false
}

pub fn usb_keyboard_active() -> bool {
    false
}

#[inline]
pub fn usb_ps2_kbd_should_ignore() -> bool {
    false
}

pub unsafe fn poll_usb_key_press() -> Option<(u8, bool)> {
    None
}

pub fn usb_midi_status_label() -> &'static [u8] {
    b"NO USB-MIDI"
}

pub fn host_label() -> &'static [u8] {
    b"USB N/A (ARM UEFI)"
}
