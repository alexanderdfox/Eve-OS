// SPDX-License-Identifier: MIT OR Apache-2.0
//
//! USB HID host: **UHCI/OHCI/…** on x86_64; stub on AArch64 (UEFI firmware / VirtIO input TBD).

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum UsbHostKind {
    None,
    Uhci,
    Ohci,
    Ehci,
    Xhci,
    Other(u8),
}

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

#[cfg(target_arch = "x86_64")]
mod x86;
#[cfg(target_arch = "x86_64")]
pub use x86::*;

#[cfg(not(target_arch = "x86_64"))]
mod stub;
#[cfg(not(target_arch = "x86_64"))]
pub use stub::*;
