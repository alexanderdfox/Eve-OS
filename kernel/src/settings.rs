// SPDX-License-Identifier: MIT OR Apache-2.0

//! In-memory device preferences.
//!
//! - **Wi‑Fi** SSID/PSK/security: stored for UI only — no 802.11 driver or WPA (use VirtIO NAT in QEMU).
//! - **Bluetooth**: toggle only — no Bluetooth stack or HCI driver in Eve.
//! - **NIC** `Virtio` / `RTL8139` / `E1000` / `Pcnet` / `Off`: labels for SYS (probe order is fixed); **Off** disables the stack UI path (hardware is still probed).
//! - **IP MODE** `Dhcp` / `Slirp` / `Static`: guest/DNS/gateway for `net.rs` (boot default is
//!   **DHCP**; static defaults `192.168.1.100` / `.1` / `8.8.8.8`; octet editing not in UI yet).
//! - **USB HOST (USB poll)** on by default so QEMU/UTM `usb-kbd` and `usb-mouse` work immediately;
//!   turn off for PS/2-only fallback on problematic hosts.

use crate::cursor_emoji;
use crate::theme::UiPalette;

/// **SYS** → **DISPLAY**: light (classic Eve) vs dark chrome.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DisplayTheme {
    Light,
    Dark,
}

impl DisplayTheme {
    pub fn next(self) -> Self {
        match self {
            Self::Light => Self::Dark,
            Self::Dark => Self::Light,
        }
    }

    pub fn label(self) -> &'static [u8] {
        match self {
            Self::Light => b"LIGHT",
            Self::Dark => b"DARK",
        }
    }

    pub fn palette(self) -> UiPalette {
        match self {
            Self::Light => UiPalette::LIGHT,
            Self::Dark => UiPalette::DARK,
        }
    }
}

/// **SYS** tab: main settings vs mouse/keyboard (USB HID + PS/2) sub-panel.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SettingsSubtab {
    General,
    Input,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    /// Photosensitivity notice; dismiss to reach `screen_after_epilepsy_notice` (see `UiState`).
    EpilepsyWarning,
    Browser,
    Settings,
    /// Clone first VirtIO disk → second (QEMU / VMs with two `virtio-blk` drives).
    DiskInstall,
    /// In-guest view of `[EVE]` diagnostic lines (`diag_log` + `log_buffer`).
    Log,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DiskInstallPhase {
    Idle,
    Running,
    Done,
    Failed,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NicChoice {
    Virtio,
    Rtl8139,
    E1000,
    Pcnet,
    Off,
}

impl NicChoice {
    pub fn next(self) -> Self {
        match self {
            NicChoice::Virtio => NicChoice::Rtl8139,
            NicChoice::Rtl8139 => NicChoice::E1000,
            NicChoice::E1000 => NicChoice::Pcnet,
            NicChoice::Pcnet => NicChoice::Off,
            NicChoice::Off => NicChoice::Virtio,
        }
    }
}

/// How the guest picks IPv4 addresses for ARP/DNS/TCP (`net.rs`).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum IpConfig {
    /// QEMU user NAT defaults: 10.0.2.15 / .2 / .3.
    Slirp,
    /// Minimal DHCP client (DISCOVER → REQUEST); needs a DHCP server on the LAN or SLIRP.
    Dhcp,
    /// Fixed addresses from `static_*` (defaults suit a typical home LAN).
    Static,
}

impl IpConfig {
    pub fn next(self) -> Self {
        match self {
            Self::Dhcp => Self::Slirp,
            Self::Slirp => Self::Static,
            Self::Static => Self::Dhcp,
        }
    }

    pub fn label(self) -> &'static [u8] {
        match self {
            Self::Slirp => b"SLIRP",
            Self::Dhcp => b"DHCP",
            Self::Static => b"STATIC",
        }
    }
}

/// Preferred association security (preference only; not applied without a WLAN driver).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum WifiSecurity {
    Open,
    Wpa2Psk,
    Wpa3Sae,
}

impl WifiSecurity {
    pub fn next(self) -> Self {
        match self {
            Self::Open => Self::Wpa2Psk,
            Self::Wpa2Psk => Self::Wpa3Sae,
            Self::Wpa3Sae => Self::Open,
        }
    }

    pub fn label(self) -> &'static [u8] {
        match self {
            Self::Open => b"OPEN",
            Self::Wpa2Psk => b"WPA2 PSK",
            Self::Wpa3Sae => b"WPA3 SAE",
        }
    }
}

#[derive(Clone, Copy)]
pub struct DeviceSettings {
    pub display_theme: DisplayTheme,
    /// Preferred GOP / framebuffer width (UEFI next boot). `0` = use firmware default / largest fit.
    pub display_pref_width: u16,
    pub display_pref_height: u16,
    /// When set, UEFI boot picks a GOP mode matching `display_pref_*` when available.
    pub display_use_custom_resolution: bool,
    /// Wi‑Fi row in SYS (no WLAN driver); TCP/IP uses probed PCI Ethernet (VirtIO, RTL8139, Intel e1000-class, …).
    pub wifi_enabled: bool,
    pub nic: NicChoice,
    /// ARP/TCP HTTP demo (addressing from `ip_config` / DHCP).
    pub internet_stack_enabled: bool,
    pub ip_config: IpConfig,
    pub static_ip: [u8; 4],
    pub static_gw: [u8; 4],
    pub static_dns: [u8; 4],
    /// Poll UHCI/OHCI USB HID (`usb-kbd` / `usb-mouse`). Off → PS/2 only. On → each USB mouse gets a
    /// cursor; PS/2 mouse shares the screen on the next cursor index (up to 12 total).
    pub usb_polling_enabled: bool,
    /// Emoji-style pointer preset (0..7). SYS row cycles; each mouse index offsets the sprite.
    pub cursor_emoji_preset: u8,
    pub bluetooth_enabled: bool,
    /// Software MIDI routing flag (no USB/audio stack).
    pub midi_enabled: bool,
    /// Prefer USB MIDI when a driver exists (preference only).
    pub midi_usb_enabled: bool,
    /// MIDI channel 1-16 (preference only).
    pub midi_channel: u8,
    /// Saved network name (no driver uses this yet).
    pub wifi_ssid: [u8; 32],
    pub wifi_ssid_len: usize,
    /// Saved pre-shared key (no driver uses this yet).
    pub wifi_psk: [u8; 64],
    pub wifi_psk_len: usize,
    pub wifi_sec: WifiSecurity,
}

impl DeviceSettings {
    pub const fn new() -> Self {
        Self {
            display_theme: DisplayTheme::Light,
            display_pref_width: 0,
            display_pref_height: 0,
            display_use_custom_resolution: false,
            wifi_enabled: true,
            nic: NicChoice::Virtio,
            internet_stack_enabled: true,
            ip_config: IpConfig::Dhcp,
            static_ip: [192, 168, 1, 100],
            static_gw: [192, 168, 1, 1],
            static_dns: [8, 8, 8, 8],
            // ON by default so QEMU/UTM usb-kbd + usb-mouse are live without opening SYS first.
            usb_polling_enabled: true,
            cursor_emoji_preset: 0,
            bluetooth_enabled: false,
            midi_enabled: true,
            midi_usb_enabled: false,
            midi_channel: 1,
            wifi_ssid: [0; 32],
            wifi_ssid_len: 0,
            wifi_psk: [0; 64],
            wifi_psk_len: 0,
            wifi_sec: WifiSecurity::Wpa2Psk,
        }
    }

    #[inline]
    pub fn ui_palette(&self) -> UiPalette {
        self.display_theme.palette()
    }

    pub fn toggle_midi_channel(self) -> Self {
        let mut c = self;
        c.midi_channel = if c.midi_channel >= 16 {
            1
        } else {
            c.midi_channel + 1
        };
        c
    }

    pub fn next_cursor_emoji_preset(self) -> Self {
        let mut c = self;
        c.cursor_emoji_preset = cursor_emoji::next_preset(c.cursor_emoji_preset);
        c
    }

    /// Fingerprint for `NetStack::sync_ip_from_settings` (mode; static octets only if `Static`).
    pub fn ip_settings_tag(self) -> u32 {
        let mut h = self.ip_config as u32;
        if self.ip_config == IpConfig::Static {
            h ^= u32::from(self.static_ip[0])
                .wrapping_shl(8)
                .wrapping_add(u32::from(self.static_ip[1]))
                .wrapping_shl(8)
                .wrapping_add(u32::from(self.static_ip[2]))
                .wrapping_shl(8)
                .wrapping_add(u32::from(self.static_ip[3]));
            h ^= u32::from(self.static_gw[0])
                .wrapping_shl(8)
                .wrapping_add(u32::from(self.static_gw[1]))
                .wrapping_shl(8)
                .wrapping_add(u32::from(self.static_gw[2]))
                .wrapping_shl(8)
                .wrapping_add(u32::from(self.static_gw[3]))
                .wrapping_mul(0x9E37_79B1);
            h ^= u32::from(self.static_dns[0])
                .wrapping_shl(8)
                .wrapping_add(u32::from(self.static_dns[1]))
                .wrapping_shl(8)
                .wrapping_add(u32::from(self.static_dns[2]))
                .wrapping_shl(8)
                .wrapping_add(u32::from(self.static_dns[3]))
                .wrapping_mul(0x85EB_CA6B);
        }
        h
    }
}
