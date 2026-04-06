// SPDX-License-Identifier: MIT OR Apache-2.0

//! Realtek RTL8139 (`QEMU -device rtl8139`, PCI **10EC:8139**): ring RX, four legacy TX slots, port I/O.
//! Common on **VirtualBox** and older QEMU templates. Same **10.0.2.x** assumptions as `net.rs`.

use crate::pci;
use crate::ports::{inb, inl, inw, outb, outl, outw};
use bootloader_api::BootInfo;
use core::sync::atomic::{fence, Ordering};

const REALTEK_VENDOR: u16 = 0x10EC;
const RTL8139_DEVICE: u16 = 0x8139;

const REG_MAC0: u16 = 0x00;
const REG_TX_STATUS0: u16 = 0x10;
const REG_TX_ADDR0: u16 = 0x20;
const REG_RX_BUF: u16 = 0x30;
const REG_CHIP_CMD: u16 = 0x37;
const REG_RX_BUF_PTR: u16 = 0x38;
const REG_INTR_MASK: u16 = 0x3C;
const REG_INTR_STATUS: u16 = 0x3E;
const REG_TX_CONFIG: u16 = 0x40;
const REG_RX_CONFIG: u16 = 0x44;
const REG_CONFIG1: u16 = 0x52;

const CMD_RESET: u8 = 0x10;
const CMD_RX_EN: u8 = 0x08;
const CMD_TX_EN: u8 = 0x04;

const TX_HOST_OWNS: u32 = 1 << 13;

const ISR_RX_OK: u16 = 0x0001;

/// QEMU default ring; must be 8K, 16K, 32K, or 64K per `RxConfig` bits (we use 8K + margin for wrap).
const RX_BUF_SIZE: usize = 8192;

const TX_BUF: usize = 2048;
const NUM_TX: usize = 4;

#[repr(C, align(4096))]
struct RtlPages {
    rx: [u8; RX_BUF_SIZE],
    tx: [[u8; TX_BUF]; NUM_TX],
}

static mut PAGES: RtlPages = RtlPages {
    rx: [0u8; RX_BUF_SIZE],
    tx: [[0u8; TX_BUF]; NUM_TX],
};

pub struct Rtl8139 {
    io: u16,
    phys_skew: Option<u64>,
    pub mac: [u8; 6],
    pub rx_packets: u64,
    /// Ring read offset (Linux `tp->cur_rx`).
    cur_rx: usize,
    tx_slot: u8,
}

impl Rtl8139 {
    fn phys(&self, virt: usize) -> u32 {
        let p = match self.phys_skew {
            Some(off) => (virt as u64).wrapping_sub(off),
            None => virt as u64,
        };
        p as u32
    }

    unsafe fn bar0_io(bus: u8, slot: u8, func: u8) -> Option<u16> {
        let raw = pci::read_u32(bus, slot, func, 0x10);
        if raw == 0xFFFF_FFFF || (raw & 1) == 0 {
            return None;
        }
        let port = (raw & 0xFFFC) as u16;
        if port == 0 {
            return None;
        }
        Some(port)
    }

    /// First RTL8139 Ethernet function on the PCI bus.
    pub unsafe fn probe(boot_info: &BootInfo) -> Option<Self> {
        let mut locs = [(0u8, 0u8, 0u8); 8];
        let n = pci::find_device_any_fn(REALTEK_VENDOR, RTL8139_DEVICE, &mut locs);
        if n == 0 {
            return None;
        }
        let picked = (0..n).find_map(|i| {
            let (b, s, f) = locs[i];
            match pci::class_subclass_prog_fn(b, s, f) {
                Some((0x02, 0x00, _)) => Some((b, s, f)),
                _ => None,
            }
        });
        let (bus, slot, func) = picked.unwrap_or(locs[0]);

        pci::pci_enable_io_bm(bus, slot, func);
        let io = Self::bar0_io(bus, slot, func)?;

        let skew = boot_info.physical_memory_offset.into();

        // Wake from low-power (some boards).
        outb(io + REG_CONFIG1, 0x00);

        // Software reset.
        outb(io + REG_CHIP_CMD, CMD_RESET);
        for _ in 0..500_000 {
            if inb(io + REG_CHIP_CMD) & CMD_RESET == 0 {
                break;
            }
            core::hint::spin_loop();
        }

        let mut mac = [0u8; 6];
        for i in 0..6 {
            mac[i] = inb(io + REG_MAC0 + i as u16);
        }

        let p = &mut *core::ptr::addr_of_mut!(PAGES);
        p.rx.fill(0);
        for t in &mut p.tx {
            t.fill(0);
        }

        let dev = Rtl8139 {
            io,
            phys_skew: skew,
            mac,
            rx_packets: 0,
            cur_rx: 0,
            tx_slot: 0,
        };

        let rx_phys = dev.phys(core::ptr::addr_of!(p.rx) as usize);
        outl(io + REG_RX_BUF, rx_phys);

        // 8 KiB ring, accept broadcast + physical + multicast filter off; wrap enabled.
        outl(io + REG_RX_CONFIG, 0x0000_070F | (1 << 7));

        outl(io + REG_TX_CONFIG, 0x0300_0700);

        outw(io + REG_INTR_MASK, 0);
        outw(io + REG_INTR_STATUS, 0xFFFF);

        // Linux `RTL_W16(RxBufPtr, RX_BUF_LEN - 16)` — NIC read pointer starts at 0.
        outw(io + REG_RX_BUF_PTR, (RX_BUF_SIZE as u16).wrapping_sub(16));

        outb(io + REG_CHIP_CMD, CMD_RX_EN | CMD_TX_EN);
        fence(Ordering::SeqCst);

        Some(dev)
    }

    unsafe fn sync_capr(&self) {
        // `REG_RX_BUF_PTR` write value W → NIC internal ptr `(W + 16) % RX_BUF_SIZE`.
        let w = self.cur_rx.wrapping_sub(16) & 0xFFFC;
        outw(self.io + REG_RX_BUF_PTR, w as u16);
    }

    /// One Ethernet frame (no virtio header). Returns `None` if no packet.
    pub unsafe fn poll_rx_packet(&mut self, out: &mut [u8]) -> Option<usize> {
        let st = inw(self.io + REG_INTR_STATUS);
        if st & ISR_RX_OK != 0 {
            outw(self.io + REG_INTR_STATUS, ISR_RX_OK);
        }

        let p = &mut *core::ptr::addr_of_mut!(PAGES);
        let ring_offset = self.cur_rx % RX_BUF_SIZE;
        if ring_offset + 8 > RX_BUF_SIZE {
            return None;
        }

        let hdr = u32::from_le_bytes([
            p.rx[ring_offset],
            p.rx[ring_offset + 1],
            p.rx[ring_offset + 2],
            p.rx[ring_offset + 3],
        ]);

        if hdr & 1 == 0 {
            return None;
        }

        // High 16 bits: length field from QEMU (includes CRC); Linux uses `(rx_size + 4 + 3) & ~3`.
        let rx_size = (hdr >> 16) as u16 as usize;
        if rx_size < 4 + 14 || rx_size > RX_BUF_SIZE {
            let skip = (rx_size + 4 + 3) & !3;
            self.cur_rx = (self.cur_rx + skip.min(RX_BUF_SIZE)) % RX_BUF_SIZE;
            self.sync_capr();
            return None;
        }

        let copy_len = rx_size - 4;
        if copy_len > out.len() || copy_len > TX_BUF {
            let received = (rx_size + 4 + 3) & !3;
            self.cur_rx = (self.cur_rx + received) % RX_BUF_SIZE;
            self.sync_capr();
            return None;
        }

        if ring_offset + 4 + copy_len > RX_BUF_SIZE {
            return None;
        }

        out[..copy_len].copy_from_slice(&p.rx[ring_offset + 4..ring_offset + 4 + copy_len]);

        let received = (rx_size + 4 + 3) & !3;
        self.cur_rx = (self.cur_rx + received) % RX_BUF_SIZE;
        self.sync_capr();

        self.rx_packets = self.rx_packets.wrapping_add(1);
        Some(copy_len)
    }

    pub unsafe fn transmit(&mut self, pkt: &[u8]) -> bool {
        if pkt.is_empty() || pkt.len() > TX_BUF {
            return false;
        }

        let p = &mut *core::ptr::addr_of_mut!(PAGES);
        let slot = usize::from(self.tx_slot % NUM_TX as u8);
        let ts = self.io + REG_TX_STATUS0 + (slot as u16) * 4;
        let ta = self.io + REG_TX_ADDR0 + (slot as u16) * 4;

        let mut status = inl(ts);
        if status & TX_HOST_OWNS == 0 {
            for _ in 0..500_000 {
                status = inl(ts);
                if status & TX_HOST_OWNS != 0 {
                    break;
                }
                core::hint::spin_loop();
            }
        }
        if status & TX_HOST_OWNS == 0 {
            return false;
        }

        p.tx[slot][..pkt.len()].copy_from_slice(pkt);
        fence(Ordering::SeqCst);

        let pa = self.phys(core::ptr::addr_of!(p.tx[slot]) as usize);
        outl(ta, pa);
        fence(Ordering::SeqCst);
        // Length in bits 0..12; clear bit 13 so NIC transmits (QEMU C-mode).
        outl(ts, pkt.len() as u32 & 0x1FFF);

        self.tx_slot = self.tx_slot.wrapping_add(1);
        true
    }
}
