// SPDX-License-Identifier: MIT OR Apache-2.0
//
//! Shared USB configuration descriptor parsing (HID boot keyboard / mouse).

/// Walk configuration descriptor bytes for a HID boot interface (`bInterfaceProtocol`).
pub fn find_hid_boot_eps(cfg: &[u8], protocol: u8) -> Option<(u8, u8, u16)> {
    let mut iface = 0u8;
    let mut i = 0usize;
    while i + 2 <= cfg.len() {
        let len = cfg[i] as usize;
        if len < 2 || i + len > cfg.len() {
            break;
        }
        let ty = cfg[i + 1];
        match ty {
            4 => {
                if len >= 9 {
                    iface = cfg[i + 2];
                    let class = cfg[i + 5];
                    let sub = cfg[i + 6];
                    let proto = cfg[i + 7];
                    if class != 3 || sub != 1 || proto != protocol {
                        iface = 0xFF;
                    }
                }
            }
            5 => {
                if len >= 7 && iface != 0xFF {
                    let addr = cfg[i + 2];
                    let attr = cfg[i + 3];
                    let mps = u16::from_le_bytes([cfg[i + 4], cfg[i + 5]]);
                    if addr & 0x80 != 0 && (attr & 3) == 3 {
                        return Some((iface, addr, mps));
                    }
                }
            }
            _ => {}
        }
        i += len;
    }
    None
}

pub fn config_has_hub_interface(cfg: &[u8]) -> bool {
    let mut i = 0usize;
    while i + 2 <= cfg.len() {
        let len = cfg[i] as usize;
        if len < 2 || i + len > cfg.len() {
            break;
        }
        if cfg[i + 1] == 4 && len >= 9 && cfg[i + 5] == 9 {
            return true;
        }
        i += len;
    }
    false
}
