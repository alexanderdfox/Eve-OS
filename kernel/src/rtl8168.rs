// SPDX-License-Identifier: MIT OR Apache-2.0

//! Realtek **RTL8168 / RTL8169** family (`QEMU` uses `rtl8139`; this path is for **VirtualBox `virtio`-less**
//! templates, some bare boards, and `10EC:8168` / `10EC:8169` PCI functions). **C+** descriptor mode,
//! MMIO layout adapted from Redox `rtl8168d` (see `drivers/net/ethernet/realtek/r8169` in Linux).

use crate::pci;
use bootloader_api::BootInfo;
use core::sync::atomic::{fence, Ordering};

const REALTEK: u16 = 0x10EC;
const DIDS: &[u16] = &[
    0x8168, // RTL8168/8111 family (common)
    0x8169, // RTL8169
    0x8167, // RTL8167
];

const OWN: u32 = 1 << 31;
const EOR: u32 = 1 << 30;
const FS: u32 = 1 << 29;
const LS: u32 = 1 << 28;

const NUM_RX: usize = 64;
const NUM_TX: usize = 16;
const RX_PKT: usize = 0x1FF8;
const TX_PKT: usize = 7552;

/// Walk PCI BARs `0x10`…`0x24`, returning the first **32- or 64-bit memory** base (skips I/O and
/// the high dword of a 64-bit BAR).
unsafe fn first_pci_mem_bar(bus: u8, slot: u8, func: u8) -> Option<usize> {
    let mut reg: u8 = 0x10;
    while reg <= 0x24 {
        let raw = pci::read_u32(bus, slot, func, reg);
        if raw == 0 || raw == 0xFFFF_FFFF {
            reg = reg.wrapping_add(4);
            continue;
        }
        if raw & 1 != 0 {
            reg = reg.wrapping_add(4);
            continue;
        }
        let low = u64::from(raw & 0xFFFF_FFF0);
        let ty = (raw >> 1) & 3;
        let base = if ty == 2 {
            reg = reg.wrapping_add(4);
            if reg > 0x24 {
                return None;
            }
            let hi = pci::read_u32(bus, slot, func, reg);
            low | (u64::from(hi) << 32)
        } else if ty == 0 {
            low
        } else {
            reg = reg.wrapping_add(4);
            continue;
        };
        return usize::try_from(base).ok();
    }
    None
}

#[repr(C, align(4096))]
struct Pages {
    rx_ring: [[u8; 16]; NUM_RX],
    tx_ring: [[u8; 16]; NUM_TX],
    tx_ring_h: [[u8; 16]; 1],
    rx_buf: [[u8; RX_PKT]; NUM_RX],
    tx_buf: [[u8; TX_PKT]; NUM_TX],
    tx_buf_h: [[u8; TX_PKT]; 1],
}

static mut PAGES: Pages = Pages {
    rx_ring: [[0u8; 16]; NUM_RX],
    tx_ring: [[0u8; 16]; NUM_TX],
    tx_ring_h: [[0u8; 16]; 1],
    rx_buf: [[0u8; RX_PKT]; NUM_RX],
    tx_buf: [[0u8; TX_PKT]; NUM_TX],
    tx_buf_h: [[0u8; TX_PKT]; 1],
};

pub struct Rtl8168 {
    mmio: usize,
    phys_skew: Option<u64>,
    pub mac: [u8; 6],
    pub rx_packets: u64,
    rx_i: usize,
    tx_i: usize,
}

#[inline]
unsafe fn r8(b: usize, o: usize) -> u8 {
    ((b + o) as *const u8).read_volatile()
}

#[inline]
unsafe fn w8(b: usize, o: usize, v: u8) {
    ((b + o) as *mut u8).write_volatile(v);
}

#[inline]
unsafe fn r16(b: usize, o: usize) -> u16 {
    ((b + o) as *const u16).read_volatile()
}

#[inline]
unsafe fn w16(b: usize, o: usize, v: u16) {
    ((b + o) as *mut u16).write_volatile(v);
}

#[inline]
unsafe fn r32(b: usize, o: usize) -> u32 {
    ((b + o) as *const u32).read_volatile()
}

#[inline]
unsafe fn w32(b: usize, o: usize, v: u32) {
    ((b + o) as *mut u32).write_volatile(v);
}

fn rd_ctrl(d: &[u8; 16]) -> u32 {
    u32::from_le_bytes([d[0], d[1], d[2], d[3]])
}

fn rd_set_ctrl(d: &mut [u8; 16], v: u32) {
    d[0..4].copy_from_slice(&v.to_le_bytes());
}

fn rd_set_buf(d: &mut [u8; 16], pa: u64) {
    d[8..12].copy_from_slice(&(pa as u32).to_le_bytes());
    d[12..16].copy_from_slice(&((pa >> 32) as u32).to_le_bytes());
}

impl Rtl8168 {
    fn phys(&self, virt: usize) -> u64 {
        match self.phys_skew {
            Some(off) => (virt as u64).wrapping_sub(off),
            None => virt as u64,
        }
    }

    /// First matching RTL8168-class Ethernet function. Picks the first **memory** BAR (skips I/O).
    pub unsafe fn probe(boot_info: &BootInfo) -> Option<Self> {
        for &did in DIDS {
            let mut locs = [(0u8, 0u8, 0u8); 8];
            let n = pci::find_device_any_fn(REALTEK, did, &mut locs);
            if n == 0 {
                continue;
            }
            let picked = (0..n).find_map(|i| {
                let (b, s, f) = locs[i];
                match pci::class_subclass_prog_fn(b, s, f) {
                    Some((0x02, 0x00, _)) => Some((b, s, f)),
                    _ => None,
                }
            });
            let (bus, slot, func) = picked.unwrap_or(locs[0]);
            pci::pci_enable_mmio_bm(bus, slot, func);
            let mmio = first_pci_mem_bar(bus, slot, func)?;
            if let Some(dev) = Self::try_init(mmio, boot_info) {
                return Some(dev);
            }
        }
        None
    }

    unsafe fn try_init(mmio: usize, boot_info: &BootInfo) -> Option<Self> {
        let skew = boot_info.physical_memory_offset.into();
        if r32(mmio, 0) == 0xFFFF_FFFF {
            return None;
        }

        let mac_lo = r32(mmio, 0);
        let mac_hi = r32(mmio, 4);
        let mac = [
            mac_lo as u8,
            (mac_lo >> 8) as u8,
            (mac_lo >> 16) as u8,
            (mac_lo >> 24) as u8,
            mac_hi as u8,
            (mac_hi >> 8) as u8,
        ];

        let p = &mut *core::ptr::addr_of_mut!(PAGES);
        for row in &mut p.rx_buf {
            row.fill(0);
        }
        for row in &mut p.tx_buf {
            row.fill(0);
        }
        p.tx_buf_h[0].fill(0);

        let dev = Rtl8168 {
            mmio,
            phys_skew: skew,
            mac,
            rx_packets: 0,
            rx_i: 0,
            tx_i: 0,
        };

        // Reset
        w8(mmio, 0x37, 1 << 4);
        for _ in 0..500_000 {
            if r8(mmio, 0x37) & (1 << 4) == 0 {
                break;
            }
            core::hint::spin_loop();
        }

        for i in 0..NUM_RX {
            let pa = dev.phys(core::ptr::addr_of!(p.rx_buf[i]) as usize);
            rd_set_buf(&mut p.rx_ring[i], pa);
            rd_set_ctrl(&mut p.rx_ring[i], OWN | (RX_PKT as u32));
        }
        let rx_last = rd_ctrl(&p.rx_ring[NUM_RX - 1]) | EOR;
        rd_set_ctrl(&mut p.rx_ring[NUM_RX - 1], rx_last);

        for i in 0..NUM_TX {
            let pa = dev.phys(core::ptr::addr_of!(p.tx_buf[i]) as usize);
            rd_set_buf(&mut p.tx_ring[i], pa);
            rd_set_ctrl(&mut p.tx_ring[i], 0);
        }
        let tx_last = rd_ctrl(&p.tx_ring[NUM_TX - 1]) | EOR;
        rd_set_ctrl(&mut p.tx_ring[NUM_TX - 1], tx_last);

        let pa_h = dev.phys(core::ptr::addr_of!(p.tx_buf_h[0]) as usize);
        rd_set_buf(&mut p.tx_ring_h[0], pa_h);
        rd_set_ctrl(&mut p.tx_ring_h[0], EOR);

        w8(mmio, 0x50, (1 << 7) | (1 << 6));
        w8(mmio, 0x37, (1 << 3) | (1 << 2));
        w16(mmio, 0xDA, 0x1FF8);
        w8(mmio, 0xEC, 0x3B);

        let tnp = dev.phys(core::ptr::addr_of!(p.tx_ring) as usize);
        w32(mmio, 0x20, tnp as u32);
        w32(mmio, 0x24, (tnp >> 32) as u32);

        let thp = dev.phys(core::ptr::addr_of!(p.tx_ring_h) as usize);
        w32(mmio, 0x28, thp as u32);
        w32(mmio, 0x2C, (thp >> 32) as u32);

        let rpa = dev.phys(core::ptr::addr_of!(p.rx_ring) as usize);
        w32(mmio, 0xE4, rpa as u32);
        w32(mmio, 0xE8, (rpa >> 32) as u32);

        w32(mmio, 0xF4, 0);
        let isr = r16(mmio, 0x3E);
        w16(mmio, 0x3E, isr);
        w16(
            mmio,
            0x3C,
            (1 << 15)
                | (1 << 14)
                | (1 << 7)
                | (1 << 6)
                | (1 << 5)
                | (1 << 4)
                | (1 << 3)
                | (1 << 2)
                | (1 << 1)
                | 1,
        );
        w32(mmio, 0x40, 0b11 << 24 | 0b111 << 8);
        w32(mmio, 0x44, 0xE70E);
        w8(mmio, 0x50, 0);

        fence(Ordering::SeqCst);
        Some(dev)
    }

    pub unsafe fn poll_rx_packet(&mut self, out: &mut [u8]) -> Option<usize> {
        let p = &mut *core::ptr::addr_of_mut!(PAGES);
        let i = self.rx_i % NUM_RX;
        let ctrl = rd_ctrl(&p.rx_ring[i]);
        if (ctrl & OWN) != 0 {
            return None;
        }
        let n = (ctrl & 0x3FFF) as usize;
        self.rx_packets = self.rx_packets.wrapping_add(1);
        let take = n.min(out.len()).min(RX_PKT);
        if take > 0 {
            out[..take].copy_from_slice(&p.rx_buf[i][..take]);
        }
        let eor = ctrl & EOR;
        let pa = self.phys(core::ptr::addr_of!(p.rx_buf[i]) as usize);
        rd_set_buf(&mut p.rx_ring[i], pa);
        rd_set_ctrl(&mut p.rx_ring[i], OWN | eor | (RX_PKT as u32));
        self.rx_i += 1;
        if n == 0 {
            return None;
        }
        Some(take)
    }

    pub unsafe fn transmit(&mut self, pkt: &[u8]) -> bool {
        if pkt.is_empty() || pkt.len() > TX_PKT {
            return false;
        }
        let p = &mut *core::ptr::addr_of_mut!(PAGES);
        let i = self.tx_i % NUM_TX;
        let td = &mut p.tx_ring[i];
        let c = rd_ctrl(td);
        if (c & OWN) != 0 {
            return false;
        }
        p.tx_buf[i][..pkt.len()].copy_from_slice(pkt);
        let eor = c & EOR;
        rd_set_ctrl(td, OWN | eor | FS | LS | (pkt.len() as u32));
        w8(self.mmio, 0x38, 1 << 6);
        for _ in 0..500_000 {
            if r8(self.mmio, 0x38) & (1 << 6) == 0 {
                break;
            }
            core::hint::spin_loop();
        }
        self.tx_i += 1;
        true
    }
}
