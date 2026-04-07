// SPDX-License-Identifier: MIT OR Apache-2.0

//! Compact binary blob for **display** preferences (theme + optional GOP WxH).
//! Carried on UEFI NVRAM by `kernel-arm-uefi`; ignored on x86 until a store exists.

use crate::settings::{DeviceSettings, DisplayTheme};

pub const BLOB_LEN: usize = 32;
const MAGIC: &[u8; 4] = b"EVS1";
const VERSION: u8 = 1;

fn checksum(payload: &[u8]) -> u32 {
    let mut c: u32 = 0x5F37_0E41;
    for &b in payload {
        c = c.wrapping_mul(31).wrapping_add(u32::from(b));
    }
    c
}

/// Serialize display-related fields into `out` (fixed 32 bytes).
pub fn encode(settings: &DeviceSettings, out: &mut [u8; BLOB_LEN]) {
    out.fill(0);
    out[0..4].copy_from_slice(MAGIC);
    out[4] = VERSION;
    out[5] = match settings.display_theme {
        DisplayTheme::Light => 0,
        DisplayTheme::Dark => 1,
    };
    out[6..8].copy_from_slice(&settings.display_pref_width.to_le_bytes());
    out[8..10].copy_from_slice(&settings.display_pref_height.to_le_bytes());
    out[10] = if settings.display_use_custom_resolution {
        1
    } else {
        0
    };
    let c = checksum(&out[0..24]);
    out[24..28].copy_from_slice(&c.to_le_bytes());
}

/// Parse blob; on success merge into `settings` (only display fields).
pub fn decode_merge(settings: &mut DeviceSettings, data: &[u8]) -> bool {
    if data.len() < 28 {
        return false;
    }
    if &data[0..4] != MAGIC {
        return false;
    }
    if data[4] != VERSION {
        return false;
    }
    let expect = u32::from_le_bytes([data[24], data[25], data[26], data[27]]);
    if checksum(&data[0..24]) != expect {
        return false;
    }
    settings.display_theme = match data[5] {
        1 => DisplayTheme::Dark,
        _ => DisplayTheme::Light,
    };
    settings.display_pref_width = u16::from_le_bytes([data[6], data[7]]);
    settings.display_pref_height = u16::from_le_bytes([data[8], data[9]]);
    settings.display_use_custom_resolution = data[10] & 1 != 0;
    true
}
