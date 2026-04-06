// SPDX-License-Identifier: MIT OR Apache-2.0

//! AMD PCnet-PCI II (`QEMU -device pcnet`, PCI **1022:2000**): SWSTYLE 3 init, 8× RX / 4× TX, polling.
//! I/O BAR0: APROM at `base+0`..`base+0x0F`, CSR at `base+0x10` / `base+0x12` / `base+0x16` (16‑bit).

use crate::pci;
use crate::ports::{inw, outw};
use bootloader_api::BootInfo;
use core::sync::atomic::{fence, Ordering};

const AMD_VENDOR: u16 = 0x1022;
const AMD_PCNET: u16 = 0x2000;

const BCR_SWS: u8 = 20;

const CSR0: u8 = 0;
const CSR1: u8 = 1;
const CSR2: u8 = 2;

const CSR0_INIT: u16 = 0x0001;
const CSR0_STRT: u16 = 0x0002;
const CSR0_STOP: u16 = 0x0004;
const CSR0_TDMD: u16 = 0x0008;
const CSR0_IDON: u16 = 0x0100;

const TMD_OWN: u16 = 0x8000;
const TMD_STP: u16 = 0x0200;
const TMD_ENP: u16 = 0x0100;

const RMD_OWN: u16 = 0x8000;

const NUM_RX: usize = 8;
const NUM_TX: usize = 4;
const PKT_MAX: usize = 1544;

#[repr(C, align(16))]
struct InitBlock {
    mode: u16,
    rlen_tlen: u16,
    mac01: u16,
    mac23: u16,
    mac45: u16,
    _res: u16,
    ladrf: [u16; 4],
    rdra: u32,
    tdra: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Rmd {
    rbadr: u32,
    buf_length: u16,
    status: u16,
    msg_length: u32,
    res: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Tmd {
    tbadr: u32,
    length: u16,
    status: u16,
    misc: u32,
    res: u32,
}

#[repr(C, align(4096))]
struct PcnetPages {
    init: InitBlock,
    rmd: [Rmd; NUM_RX],
    tmd: [Tmd; NUM_TX],
    rx: [[u8; PKT_MAX]; NUM_RX],
    tx: [[u8; PKT_MAX]; NUM_TX],
}

static mut PAGES: PcnetPages = PcnetPages {
    init: InitBlock {
        mode: 0,
        rlen_tlen: 0,
        mac01: 0,
        mac23: 0,
        mac45: 0,
        _res: 0,
        ladrf: [0; 4],
        rdra: 0,
        tdra: 0,
    },
    rmd: [Rmd {
        rbadr: 0,
        buf_length: 0,
        status: 0,
        msg_length: 0,
        res: 0,
    }; NUM_RX],
    tmd: [Tmd {
        tbadr: 0,
        length: 0,
        status: 0,
        misc: 0,
        res: 0,
    }; NUM_TX],
    rx: [[0u8; PKT_MAX]; NUM_RX],
    tx: [[0u8; PKT_MAX]; NUM_TX],
};

pub struct Pcnet {
    io: u16,
    phys_skew: Option<u64>,
    pub mac: [u8; 6],
    pub rx_packets: u64,
    rx_i: usize,
    tx_i: usize,
}

impl Pcnet {
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

    unsafe fn csr_write(io: u16, reg: u8, val: u16) {
        outw(io + 0x12, u16::from(reg));
        outw(io + 0x10, val);
    }

    unsafe fn csr_read(io: u16, reg: u8) -> u16 {
        outw(io + 0x12, u16::from(reg));
        inw(io + 0x10)
    }

    unsafe fn bcr_write(io: u16, reg: u8, val: u16) {
        outw(io + 0x12, u16::from(reg));
        outw(io + 0x16, val);
    }

    /// First PCnet-PCI Ethernet function (class 0x02 / 0x00).
    pub unsafe fn probe(boot_info: &BootInfo) -> Option<Self> {
        let mut locs = [(0u8, 0u8, 0u8); 4];
        let n = pci::find_device_any_fn(AMD_VENDOR, AMD_PCNET, &mut locs);
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

        let _ = Self::csr_read(io, CSR0);
        let _ = inw(io + 0x14);

        Self::bcr_write(io, BCR_SWS, 0x0103);

        let mut mac = [0u8; 6];
        for i in 0..6usize {
            mac[i] = crate::ports::inb(io + i as u16);
        }

        let p = &mut *core::ptr::addr_of_mut!(PAGES);
        for row in &mut p.rx {
            row.fill(0);
        }
        for row in &mut p.tx {
            row.fill(0);
        }

        let rlog = 3u8;
        let tlog = 2u8;
        p.init.mode = 0;
        p.init.rlen_tlen = u16::from(rlog) | (u16::from(tlog) << 8);
        p.init.mac01 = u16::from_le_bytes([mac[0], mac[1]]);
        p.init.mac23 = u16::from_le_bytes([mac[2], mac[3]]);
        p.init.mac45 = u16::from_le_bytes([mac[4], mac[5]]);
        p.init._res = 0;
        p.init.ladrf = [0; 4];

        let dev = Pcnet {
            io,
            phys_skew: skew,
            mac,
            rx_packets: 0,
            rx_i: 0,
            tx_i: 0,
        };

        let rd_phys = dev.phys(core::ptr::addr_of!(p.rmd) as usize);
        let td_phys = dev.phys(core::ptr::addr_of!(p.tmd) as usize);
        p.init.rdra = rd_phys.to_le();
        p.init.tdra = td_phys.to_le();

        let init_phys = dev.phys(core::ptr::addr_of!(p.init) as usize);
        for i in 0..NUM_RX {
            let bp = dev.phys(core::ptr::addr_of!(p.rx[i]) as usize);
            let bl: u16 = 0xf000u16 | (((-(PKT_MAX as i16)) as u16) & 0x0fff);
            p.rmd[i] = Rmd {
                rbadr: bp.to_le(),
                buf_length: bl.to_le(),
                status: RMD_OWN.to_le(),
                msg_length: 0,
                res: 0,
            };
        }
        for i in 0..NUM_TX {
            p.tmd[i] = Tmd {
                tbadr: 0,
                length: 0,
                status: 0,
                misc: 0,
                res: 0,
            };
        }

        Self::csr_write(io, CSR1, (init_phys & 0xffff) as u16);
        Self::csr_write(io, CSR2, (init_phys >> 16) as u16);

        let mut c0 = Self::csr_read(io, CSR0);
        c0 |= CSR0_INIT;
        Self::csr_write(io, CSR0, c0);

        for _ in 0..500_000 {
            c0 = Self::csr_read(io, CSR0);
            if c0 & CSR0_IDON != 0 {
                break;
            }
            core::hint::spin_loop();
        }
        c0 = Self::csr_read(io, CSR0);
        Self::csr_write(io, CSR0, (c0 | CSR0_STRT) & !CSR0_INIT & !CSR0_IDON);

        fence(Ordering::SeqCst);
        Some(dev)
    }

    pub unsafe fn poll_rx_packet(&mut self, out: &mut [u8]) -> Option<usize> {
        let p = &mut *core::ptr::addr_of_mut!(PAGES);
        let i = self.rx_i % NUM_RX;
        let st = u16::from_le(p.rmd[i].status);
        if (st & RMD_OWN) != 0 {
            return None;
        }
        let err = st & 0x0400;
        let ml = u32::from_le(p.rmd[i].msg_length) & 0xfff;
        let n = ml as usize;
        self.rx_packets = self.rx_packets.wrapping_add(1);
        let take = n.min(out.len()).min(PKT_MAX);
        if err == 0 && take > 0 {
            out[..take].copy_from_slice(&p.rx[i][..take]);
        } else {
            out[..0].copy_from_slice(&[]);
        }
        let bl: u16 = 0xf000u16 | (((-(PKT_MAX as i16)) as u16) & 0x0fff);
        p.rmd[i].buf_length = bl.to_le();
        p.rmd[i].status = RMD_OWN.to_le();
        p.rmd[i].msg_length = 0;
        self.rx_i += 1;
        if err != 0 || n == 0 {
            return None;
        }
        Some(take)
    }

    pub unsafe fn transmit(&mut self, pkt: &[u8]) -> bool {
        if pkt.is_empty() || pkt.len() > PKT_MAX {
            return false;
        }
        let p = &mut *core::ptr::addr_of_mut!(PAGES);
        let i = self.tx_i % NUM_TX;
        let st = u16::from_le(p.tmd[i].status);
        if (st & TMD_OWN) != 0 {
            return false;
        }
        p.tx[i][..pkt.len()].copy_from_slice(pkt);
        let tb = self.phys(core::ptr::addr_of!(p.tx[i]) as usize);
        let tlen: u16 = 0xf000u16 | (((-(pkt.len() as i16)) as u16) & 0x0fff);
        p.tmd[i].tbadr = tb.to_le();
        p.tmd[i].length = tlen.to_le();
        p.tmd[i].status = (TMD_OWN | TMD_STP | TMD_ENP).to_le();
        p.tmd[i].misc = 0;
        fence(Ordering::SeqCst);
        let mut c0 = Self::csr_read(self.io, CSR0);
        c0 &= !CSR0_STOP;
        c0 |= CSR0_TDMD;
        Self::csr_write(self.io, CSR0, c0);
        self.tx_i += 1;
        true
    }
}
