// SPDX-License-Identifier: MIT OR Apache-2.0

//! VirtIO-MMIO Ethernet (AArch64 QEMU `virt`).

use crate::virtio_mmio_net::VirtioMmioNet;

pub enum AnyNic {
    VirtioMmio(VirtioMmioNet),
}

impl AnyNic {
    pub unsafe fn probe() -> Option<Self> {
        VirtioMmioNet::probe().map(AnyNic::VirtioMmio)
    }

    pub fn driver_tag(&self) -> &'static [u8] {
        match self {
            AnyNic::VirtioMmio(_) => b"virtio-net-mmio",
        }
    }

    pub fn mac(&self) -> &[u8; 6] {
        match self {
            AnyNic::VirtioMmio(n) => &n.mac,
        }
    }

    pub fn rx_packets(&self) -> u64 {
        match self {
            AnyNic::VirtioMmio(n) => n.rx_packets,
        }
    }

    pub unsafe fn poll_rx_packet(&mut self, out: &mut [u8]) -> Option<usize> {
        match self {
            AnyNic::VirtioMmio(n) => n.poll_rx_packet(out),
        }
    }

    pub unsafe fn transmit(&mut self, pkt: &[u8]) -> bool {
        match self {
            AnyNic::VirtioMmio(n) => n.transmit(pkt),
        }
    }
}
