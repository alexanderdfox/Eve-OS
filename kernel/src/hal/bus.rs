// SPDX-License-Identifier: MIT OR Apache-2.0
//
//! Bus / device discovery: x86 PCI today; Pi MMIO and UEFI protocol walks are future backends.

/// How the platform finds devices before drivers attach.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BusKind {
    #[default]
    Pci,
    Mmio,
    FirmwareProtocol,
}

/// Read-only snapshot for SYS / diag without pulling in driver types.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BusSnapshot {
    pub ethernet_count: u8,
    pub wlan_count: u8,
    pub mm_audio_present: bool,
    pub usb_host_present: bool,
}

pub trait BusDiscover {
    fn bus_kind(&self) -> BusKind;
    fn snapshot(&self) -> BusSnapshot;
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
pub struct X86PciDiscover;

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
impl BusDiscover for X86PciDiscover {
    fn bus_kind(&self) -> BusKind {
        BusKind::Pci
    }

    fn snapshot(&self) -> BusSnapshot {
        let mut wlan_pci = [crate::pci::PciFnId::default(); 8];
        let nwlan = unsafe { crate::pci::enumerate_pci_class(0x02, 0x80, &mut wlan_pci) };
        let usb = unsafe { crate::pci::scan_usb_host_prog_if().is_some() };
        BusSnapshot {
            ethernet_count: unsafe { crate::pci::scan_ethernet_count() },
            wlan_count: nwlan.min(255) as u8,
            mm_audio_present: unsafe { crate::pci::scan_mm_audio_present() },
            usb_host_present: usb,
        }
    }
}

/// ARM UEFI / Pi bring-up before full MMIO or PCI walks exist.
#[derive(Clone, Copy, Debug, Default)]
pub struct StaticBusSnapshot {
    pub snap: BusSnapshot,
    pub kind: BusKind,
}

impl BusDiscover for StaticBusSnapshot {
    fn bus_kind(&self) -> BusKind {
        self.kind
    }

    fn snapshot(&self) -> BusSnapshot {
        self.snap
    }
}
