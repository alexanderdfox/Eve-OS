// SPDX-License-Identifier: MIT OR Apache-2.0

//! PCI configuration space access (ports 0xCF8 / 0xCFC). Buses 0–7 scanned.

use crate::ports::{inl, outl};

const CONFIG_ADDRESS: u16 = 0xCF8;
const CONFIG_DATA: u16 = 0xCFC;

#[inline]
fn cfg_addr(bus: u8, slot: u8, func: u8, offset: u8) -> u32 {
    0x8000_0000
        | (u32::from(bus) << 16)
        | (u32::from(slot) << 11)
        | (u32::from(func) << 8)
        | u32::from(offset & 0xFC)
}

pub unsafe fn read_u32(bus: u8, slot: u8, func: u8, offset: u8) -> u32 {
    outl(CONFIG_ADDRESS, cfg_addr(bus, slot, func, offset));
    inl(CONFIG_DATA)
}

pub unsafe fn write_u32(bus: u8, slot: u8, func: u8, offset: u8, v: u32) {
    outl(CONFIG_ADDRESS, cfg_addr(bus, slot, func, offset));
    outl(CONFIG_DATA, v);
}

pub unsafe fn read_u16(bus: u8, slot: u8, func: u8, offset: u8) -> u16 {
    let w = read_u32(bus, slot, func, offset);
    if offset & 3 == 0 {
        (w & 0xFFFF) as u16
    } else {
        (w >> 16) as u16
    }
}

pub unsafe fn read_u8(bus: u8, slot: u8, func: u8, offset: u8) -> u8 {
    let w = read_u32(bus, slot, func, offset & !3);
    let shift = 8 * (usize::from(offset) & 3);
    ((w >> shift) & 0xFF) as u8
}

pub unsafe fn write_u16(bus: u8, slot: u8, func: u8, offset: u8, v: u16) {
    let base = offset & !3;
    let mut w = read_u32(bus, slot, func, base);
    if offset & 3 == 0 {
        w = (w & !0xFFFF) | u32::from(v);
    } else {
        w = (w & 0xFFFF) | (u32::from(v) << 16);
    }
    write_u32(bus, slot, func, base, w);
}

/// Base class (offset `0x0B`) and subclass (`0x0A`), function 0 only.
pub unsafe fn class_subclass(bus: u8, slot: u8) -> Option<(u8, u8)> {
    let vid_did = read_u32(bus, slot, 0, 0);
    if vid_did == 0xFFFF_FFFF || (vid_did & 0xFFFF) == 0xFFFF {
        return None;
    }
    let sub = read_u8(bus, slot, 0, 0x0A);
    let base = read_u8(bus, slot, 0, 0x0B);
    Some((base, sub))
}

/// Any PCI device that looks like 802.11 (network class, subclass 0x80).
pub unsafe fn scan_wlan_present() -> bool {
    for bus in 0u8..=7 {
        for slot in 0u8..32 {
            if let Some((0x02, 0x80)) = class_subclass(bus, slot) {
                return true;
            }
        }
    }
    false
}

/// Count Ethernet controllers (class 0x02, subclass 0x00).
pub unsafe fn scan_ethernet_count() -> u8 {
    let mut n = 0u8;
    for bus in 0u8..=7 {
        for slot in 0u8..32 {
            if let Some((0x02, 0x00)) = class_subclass(bus, slot) {
                n = n.saturating_add(1);
            }
        }
    }
    n
}

/// Multimedia audio (PCI class 0x04: AC97 0x01, legacy 0x02, HDA 0x03).
pub unsafe fn scan_mm_audio_present() -> bool {
    for bus in 0u8..=7 {
        for slot in 0u8..32 {
            if let Some((0x04, sub)) = class_subclass(bus, slot) {
                if matches!(sub, 0x01 | 0x02 | 0x03) {
                    return true;
                }
            }
        }
    }
    false
}

/// Class, subclass, programming interface (header type 0, offset 0x09 = PI).
pub unsafe fn class_subclass_prog(bus: u8, slot: u8) -> Option<(u8, u8, u8)> {
    let cs = class_subclass(bus, slot)?;
    let pi = read_u8(bus, slot, 0, 0x09);
    Some((cs.0, cs.1, pi))
}

/// First USB host controller on bus 0–1: returns PI (`0x00` UHCI, `0x10` OHCI, `0x20` EHCI, `0x30` xHCI).
pub unsafe fn scan_usb_host_prog_if() -> Option<u8> {
    for bus in 0u8..=7 {
        for slot in 0u8..32 {
            if let Some((0x0C, 0x03, pi)) = class_subclass_prog(bus, slot) {
                return Some(pi);
            }
        }
    }
    None
}

/// First function only; buses 0–7 (Q35 / bridges may place virtio-net past bus 1).
pub unsafe fn find_device(vendor: u16, device: u16) -> Option<(u8, u8, u8)> {
    for bus in 0u8..=7 {
        for slot in 0u8..32 {
            let vid_did = read_u32(bus, slot, 0, 0);
            if vid_did == 0xFFFF_FFFF || (vid_did & 0xFFFF) == 0xFFFF {
                continue;
            }
            let vid = (vid_did & 0xFFFF) as u16;
            let did = (vid_did >> 16) as u16;
            if vid == vendor && did == device {
                return Some((bus, slot, 0));
            }
        }
    }
    None
}
