// SPDX-License-Identifier: MIT OR Apache-2.0

//! VMware **vmxnet3** (PCI **15AD:07B0**). UPT requires a large shared `Vmxnet3_DriverShared` block,
//! separate TX **data** rings, and completion parsing per `hw/net/vmxnet3.h` in QEMU — a full port is
//! tracked as follow-up work. Prefer **`-device virtio-net-pci`** or **`-device e1000e`** in UTM/QEMU
//! on Apple Silicon until this driver is completed.

use bootloader_api::BootInfo;

pub struct Vmxnet3 {
    pub mac: [u8; 6],
    pub rx_packets: u64,
}

impl Vmxnet3 {
    pub unsafe fn probe(_boot_info: &BootInfo) -> Option<Self> {
        None
    }

    pub unsafe fn poll_rx_packet(&mut self, _out: &mut [u8]) -> Option<usize> {
        None
    }

    pub unsafe fn transmit(&mut self, _pkt: &[u8]) -> bool {
        false
    }
}
