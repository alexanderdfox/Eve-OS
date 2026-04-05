// SPDX-License-Identifier: MIT OR Apache-2.0

//! In-memory device preferences.
//!
//! - **Wi‑Fi** SSID/PSK/security: stored for UI only — no 802.11 driver or WPA (use VirtIO NAT in QEMU).
//! - **Bluetooth**: toggle only — no Bluetooth stack or HCI driver in Eve.
//! - **NIC** `E1000Stub` / `Off`: labels for future work; only **VirtIO** is driven for packets today.
//! - **USB HOST (USB poll)** off → PS/2 only (default); on → UHCI or OHCI HID when that controller drives the bus.

use crate::cursor_emoji;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Browser,
    Settings,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NicChoice {
    Virtio,
    E1000Stub,
    Off,
}

impl NicChoice {
    pub fn next(self) -> Self {
        match self {
            NicChoice::Virtio => NicChoice::E1000Stub,
            NicChoice::E1000Stub => NicChoice::Off,
            NicChoice::Off => NicChoice::Virtio,
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
    /// With VirtIO, also allows the minimal TCP/IP stack when no PCI Ethernet is detected.
    pub wifi_enabled: bool,
    pub nic: NicChoice,
    /// ARP/TCP HTTP demo on QEMU user NAT (10.0.2.x).
    pub internet_stack_enabled: bool,
    /// Poll UHCI USB HID (QEMU `-device usb-kbd` / `usb-mouse`). Off → use PS/2 only.
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
            wifi_enabled: true,
            nic: NicChoice::Virtio,
            internet_stack_enabled: true,
            // Off by default: PS/2 is reliable in QEMU/TCG; UHCI HID can enumerate then stall and
            // leave PS/2 suppressed. Turn ON in SYS for multi-USB-pointer demos with working UHCI.
            usb_polling_enabled: false,
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
}
