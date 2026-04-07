// SPDX-License-Identifier: MIT OR Apache-2.0

//! PCI Ethernet probe order: **VirtIO net** → **RTL8139** → **RTL8168/8169** → **Intel e1000/e1000e**
//! → **vmxnet3** → **Broadcom bge** → **AMD PCnet**.
//!
//! `net.rs` builds packets with a **12-byte VirtIO net header** (zeros) in front of the Ethernet frame.
//! **VirtIO** passes the full buffer to the device; all other drivers transmit **L2 only** (header stripped here).

use crate::bge::Bge;
use crate::e1000::E1000;
use crate::pcnet::Pcnet;
use crate::rtl8139::Rtl8139;
use crate::rtl8168::Rtl8168;
use crate::virtio_net::VirtioNet;
use crate::vmxnet3::Vmxnet3;
use bootloader_api::BootInfo;

pub enum AnyNic {
    Virtio(VirtioNet),
    Rtl8139(Rtl8139),
    Rtl8168(Rtl8168),
    E1000(E1000),
    Vmxnet3(Vmxnet3),
    Bge(Bge),
    Pcnet(Pcnet),
}

impl AnyNic {
    pub unsafe fn probe(boot_info: &BootInfo) -> Option<Self> {
        if let Some(v) = VirtioNet::probe(boot_info) {
            return Some(AnyNic::Virtio(v));
        }
        if let Some(r) = Rtl8139::probe(boot_info) {
            return Some(AnyNic::Rtl8139(r));
        }
        if let Some(r) = Rtl8168::probe(boot_info) {
            return Some(AnyNic::Rtl8168(r));
        }
        if let Some(e) = E1000::probe(boot_info) {
            return Some(AnyNic::E1000(e));
        }
        if let Some(v) = Vmxnet3::probe(boot_info) {
            return Some(AnyNic::Vmxnet3(v));
        }
        if let Some(b) = Bge::probe(boot_info) {
            return Some(AnyNic::Bge(b));
        }
        Pcnet::probe(boot_info).map(AnyNic::Pcnet)
    }

    pub fn driver_tag(&self) -> &'static [u8] {
        match self {
            AnyNic::Virtio(_) => b"virtio-net",
            AnyNic::Rtl8139(_) => b"rtl8139",
            AnyNic::Rtl8168(_) => b"rtl8168",
            AnyNic::E1000(_) => b"e1000",
            AnyNic::Vmxnet3(_) => b"vmxnet3",
            AnyNic::Bge(_) => b"bge",
            AnyNic::Pcnet(_) => b"pcnet",
        }
    }

    pub fn mac(&self) -> &[u8; 6] {
        match self {
            AnyNic::Virtio(n) => &n.mac,
            AnyNic::Rtl8139(n) => &n.mac,
            AnyNic::Rtl8168(n) => &n.mac,
            AnyNic::E1000(n) => &n.mac,
            AnyNic::Vmxnet3(n) => &n.mac,
            AnyNic::Bge(n) => &n.mac,
            AnyNic::Pcnet(n) => &n.mac,
        }
    }

    pub fn rx_packets(&self) -> u64 {
        match self {
            AnyNic::Virtio(n) => n.rx_packets,
            AnyNic::Rtl8139(n) => n.rx_packets,
            AnyNic::Rtl8168(n) => n.rx_packets,
            AnyNic::E1000(n) => n.rx_packets,
            AnyNic::Vmxnet3(n) => n.rx_packets,
            AnyNic::Bge(n) => n.rx_packets,
            AnyNic::Pcnet(n) => n.rx_packets,
        }
    }

    pub unsafe fn poll_rx_packet(&mut self, out: &mut [u8]) -> Option<usize> {
        match self {
            AnyNic::Virtio(n) => n.poll_rx_packet(out),
            AnyNic::Rtl8139(n) => n.poll_rx_packet(out),
            AnyNic::Rtl8168(n) => n.poll_rx_packet(out),
            AnyNic::E1000(n) => n.poll_rx_packet(out),
            AnyNic::Vmxnet3(n) => n.poll_rx_packet(out),
            AnyNic::Bge(n) => n.poll_rx_packet(out),
            AnyNic::Pcnet(n) => n.poll_rx_packet(out),
        }
    }

    pub unsafe fn transmit(&mut self, pkt: &[u8]) -> bool {
        const VH: usize = crate::net::VIRTIO_NET_HDR;
        match self {
            AnyNic::Virtio(n) => n.transmit(pkt),
            AnyNic::Rtl8139(n) => {
                if pkt.len() <= VH {
                    return false;
                }
                n.transmit(&pkt[VH..])
            }
            AnyNic::Rtl8168(n) => {
                if pkt.len() <= VH {
                    return false;
                }
                n.transmit(&pkt[VH..])
            }
            AnyNic::E1000(n) => {
                if pkt.len() <= VH {
                    return false;
                }
                n.transmit(&pkt[VH..])
            }
            AnyNic::Vmxnet3(n) => {
                if pkt.len() <= VH {
                    return false;
                }
                n.transmit(&pkt[VH..])
            }
            AnyNic::Bge(n) => {
                if pkt.len() <= VH {
                    return false;
                }
                n.transmit(&pkt[VH..])
            }
            AnyNic::Pcnet(n) => {
                if pkt.len() <= VH {
                    return false;
                }
                n.transmit(&pkt[VH..])
            }
        }
    }
}
