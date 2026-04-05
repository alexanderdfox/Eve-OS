// SPDX-License-Identifier: MIT OR Apache-2.0

//! In-memory device preferences (no real Wi-Fi / BT / MIDI drivers yet).

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

#[derive(Clone, Copy)]
pub struct DeviceSettings {
    /// With VirtIO, also allows the minimal TCP/IP stack when no PCI Ethernet is detected.
    pub wifi_enabled: bool,
    pub nic: NicChoice,
    /// ARP/TCP HTTP demo on QEMU user NAT (10.0.2.x).
    pub internet_stack_enabled: bool,
    /// Poll UHCI USB HID (QEMU `-device usb-kbd` / `usb-mouse`). Off → use PS/2 only.
    pub usb_polling_enabled: bool,
    pub bluetooth_enabled: bool,
    /// Software MIDI routing flag (no USB/audio stack).
    pub midi_enabled: bool,
    /// Prefer USB MIDI when a driver exists (preference only).
    pub midi_usb_enabled: bool,
    /// MIDI channel 1-16 (preference only).
    pub midi_channel: u8,
}

impl DeviceSettings {
    pub const fn new() -> Self {
        Self {
            // VirtIO “WAN” path when no discrete PCI Ethernet (0x02/0x00) is enumerated.
            wifi_enabled: true,
            nic: NicChoice::Virtio,
            internet_stack_enabled: true,
            usb_polling_enabled: true,
            bluetooth_enabled: false,
            midi_enabled: true,
            midi_usb_enabled: false,
            midi_channel: 1,
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
}
