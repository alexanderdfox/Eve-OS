// SPDX-License-Identifier: MIT OR Apache-2.0

//! VirtIO-MMIO Ethernet (AArch64 QEMU `virt`).

use core::sync::atomic::{AtomicBool, Ordering};

use crate::virtio_mmio_net::VirtioMmioNet;

/// Whether [`VirtioMmioNet::probe`] may touch QEMU `virt` MMIO at `0x0a00_0000`.
/// **Default `false`:** on Apple Silicon UEFI those addresses are not a virtio device; reads can
/// fault and reset the machine (GRUB chainload boot loop). The AArch64 UEFI binary calls
/// [`set_allow_virtio_mmio_scan`] for non-Apple firmware before the
/// first [`crate::arm_run::main_step`].
static ALLOW_VIRTIO_MMIO_SCAN: AtomicBool = AtomicBool::new(false);

/// Call from AArch64 UEFI after firmware is known (e.g. after `uefi::helpers::init`).
#[inline]
pub fn set_allow_virtio_mmio_scan(allow: bool) {
    ALLOW_VIRTIO_MMIO_SCAN.store(allow, Ordering::SeqCst);
}

pub enum AnyNic {
    VirtioMmio(VirtioMmioNet),
}

impl AnyNic {
    pub unsafe fn probe() -> Option<Self> {
        if !ALLOW_VIRTIO_MMIO_SCAN.load(Ordering::SeqCst) {
            return None;
        }
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
