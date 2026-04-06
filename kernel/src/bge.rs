// SPDX-License-Identifier: MIT OR Apache-2.0

//! **Broadcom NetXtreme** (`14e4:…` BCM57xx / BCM58xx). Linux **`tg3`** and **`bnxt`** drivers depend on
//! firmware blobs and long MMIO init sequences; there is no Broadcom NIC in default **QEMU** PC builds.
//! Use **virtio-net-pci** or **e1000e** for VMs. Native drivers for Apple’s integrated Ethernet (where
//! present) belong in a separate `tg3`/`bnxt` bring-up effort.

use bootloader_api::BootInfo;

pub struct Bge {
    pub mac: [u8; 6],
    pub rx_packets: u64,
}

impl Bge {
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
