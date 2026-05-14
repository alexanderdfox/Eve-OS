// SPDX-License-Identifier: MIT OR Apache-2.0
//
//! UEFI NVRAM blob for **DeviceSettings** (display, network toggles, Wi‑Fi strings, browser home,
//! bookmarks). **EVS1** (32 B) is legacy display-only; **EVS2** (512 B) is the current format.
//! x86 has no saver yet; AArch64 UEFI registers `SetVariable` via `arm_run::register_settings_blob_saver`.

use crate::settings::{
    DeviceSettings, DisplayTheme, IpConfig, NicChoice, WifiSecurity, DEFAULT_HOME_URL,
    PERSIST_BOOKMARK_SLOTS, PERSIST_BOOKMARK_URL_CAP, PERSIST_HOME_URL_CAP,
};

pub const BLOB_LEN: usize = 512;

const MAGIC_V1: &[u8; 4] = b"EVS1";
const MAGIC_V2: &[u8; 4] = b"EVS2";
const VERSION_V1: u8 = 1;
const VERSION_V2: u8 = 2;

fn checksum(payload: &[u8]) -> u32 {
    let mut c: u32 = 0x5F37_0E41;
    for &b in payload {
        c = c.wrapping_mul(31).wrapping_add(u32::from(b));
    }
    c
}

/// Serialize settings into `out` (fixed 512 bytes, EVS2).
pub fn encode(settings: &DeviceSettings, out: &mut [u8; BLOB_LEN]) {
    out.fill(0);
    out[0..4].copy_from_slice(MAGIC_V2);
    out[4] = VERSION_V2;
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
    out[11] = settings.nic as u8;
    out[12] = settings.ip_config as u8;
    out[13..17].copy_from_slice(&settings.static_ip);
    out[17..21].copy_from_slice(&settings.static_gw);
    out[21..25].copy_from_slice(&settings.static_dns);
    let mut f: u8 = 0;
    if settings.internet_stack_enabled {
        f |= 1;
    }
    if settings.wifi_enabled {
        f |= 2;
    }
    if settings.usb_polling_enabled {
        f |= 4;
    }
    if settings.bluetooth_enabled {
        f |= 8;
    }
    if settings.midi_enabled {
        f |= 16;
    }
    if settings.midi_usb_enabled {
        f |= 32;
    }
    if settings.browser_script_runtime_enabled {
        f |= 64;
    }
    out[25] = f;
    out[26] = settings.midi_channel.min(16).max(1);
    out[27] = settings.cursor_emoji_preset;
    out[28] = settings.wifi_ssid_len.min(32) as u8;
    out[29] = settings.wifi_psk_len.min(64) as u8;
    out[30] = settings.wifi_sec as u8;
    out[31] = 0;
    out[32..64].copy_from_slice(&settings.wifi_ssid);
    out[64..128].copy_from_slice(&settings.wifi_psk[..64]);
    let hl = (settings.home_url_len as usize).min(PERSIST_HOME_URL_CAP);
    out[128] = hl as u8;
    out[129..129 + PERSIST_HOME_URL_CAP].copy_from_slice(&settings.home_url);
    for i in 0..PERSIST_BOOKMARK_SLOTS {
        out[249 + i] = settings.bookmark_len[i].min(PERSIST_BOOKMARK_URL_CAP as u8);
    }
    let bm0 = 257usize;
    for i in 0..PERSIST_BOOKMARK_SLOTS {
        let o = bm0 + i * PERSIST_BOOKMARK_URL_CAP;
        out[o..o + PERSIST_BOOKMARK_URL_CAP].copy_from_slice(&settings.bookmark_url[i]);
    }
    let c = checksum(&out[0..508]);
    out[508..512].copy_from_slice(&c.to_le_bytes());
}

fn decode_v1_display_only(settings: &mut DeviceSettings, data: &[u8]) -> bool {
    if data.len() < 28 {
        return false;
    }
    if &data[0..4] != MAGIC_V1 {
        return false;
    }
    if data[4] != VERSION_V1 {
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

fn decode_v2(settings: &mut DeviceSettings, data: &[u8]) -> bool {
    if data.len() < BLOB_LEN {
        return false;
    }
    if &data[0..4] != MAGIC_V2 {
        return false;
    }
    if data[4] != VERSION_V2 {
        return false;
    }
    let expect = u32::from_le_bytes([data[508], data[509], data[510], data[511]]);
    if checksum(&data[0..508]) != expect {
        return false;
    }
    settings.display_theme = match data[5] {
        1 => DisplayTheme::Dark,
        _ => DisplayTheme::Light,
    };
    settings.display_pref_width = u16::from_le_bytes([data[6], data[7]]);
    settings.display_pref_height = u16::from_le_bytes([data[8], data[9]]);
    settings.display_use_custom_resolution = data[10] & 1 != 0;
    settings.nic = match data[11] {
        0 => NicChoice::Virtio,
        1 => NicChoice::Rtl8139,
        2 => NicChoice::E1000,
        3 => NicChoice::Pcnet,
        _ => NicChoice::Off,
    };
    settings.ip_config = match data[12] {
        0 => IpConfig::Slirp,
        1 => IpConfig::Dhcp,
        _ => IpConfig::Static,
    };
    settings.static_ip.copy_from_slice(&data[13..17]);
    settings.static_gw.copy_from_slice(&data[17..21]);
    settings.static_dns.copy_from_slice(&data[21..25]);
    let f = data[25];
    settings.internet_stack_enabled = f & 1 != 0;
    settings.wifi_enabled = f & 2 != 0;
    settings.usb_polling_enabled = f & 4 != 0;
    settings.bluetooth_enabled = f & 8 != 0;
    settings.midi_enabled = f & 16 != 0;
    settings.midi_usb_enabled = f & 32 != 0;
    settings.browser_script_runtime_enabled = f & 64 != 0;
    settings.midi_channel = data[26].clamp(1, 16);
    settings.cursor_emoji_preset = data[27];
    settings.wifi_ssid_len = (data[28] as usize).min(32);
    settings.wifi_psk_len = (data[29] as usize).min(64);
    settings.wifi_sec = match data[30] {
        1 => WifiSecurity::Wpa2Psk,
        2 => WifiSecurity::Wpa3Sae,
        _ => WifiSecurity::Open,
    };
    settings.wifi_ssid.copy_from_slice(&data[32..64]);
    settings.wifi_psk.copy_from_slice(&data[64..128]);
    let hl = (data[128] as usize).min(PERSIST_HOME_URL_CAP);
    settings.home_url.copy_from_slice(&data[129..129 + PERSIST_HOME_URL_CAP]);
    settings.home_url_len = hl as u8;
    if hl == 0 {
        let n = DEFAULT_HOME_URL.len().min(PERSIST_HOME_URL_CAP);
        settings.home_url[..n].copy_from_slice(&DEFAULT_HOME_URL[..n]);
        settings.home_url_len = n as u8;
    }
    for i in 0..PERSIST_BOOKMARK_SLOTS {
        settings.bookmark_len[i] = data[249 + i].min(PERSIST_BOOKMARK_URL_CAP as u8);
    }
    let bm0 = 257usize;
    for i in 0..PERSIST_BOOKMARK_SLOTS {
        let o = bm0 + i * PERSIST_BOOKMARK_URL_CAP;
        settings.bookmark_url[i].copy_from_slice(&data[o..o + PERSIST_BOOKMARK_URL_CAP]);
    }
    true
}

/// Parse blob; on success merge into `settings` (full merge for EVS2, display-only for EVS1).
pub fn decode_merge(settings: &mut DeviceSettings, data: &[u8]) -> bool {
    if data.len() >= 4 && &data[0..4] == MAGIC_V2 {
        return decode_v2(settings, data);
    }
    decode_v1_display_only(settings, data)
}
