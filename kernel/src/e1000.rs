// SPDX-License-Identifier: MIT OR Apache-2.0

//! Intel legacy PCI gigabit (`QEMU -device e1000`, `e1000-82545em`, …): shared RX/TX descriptor path, polling only.
//! Avoids global `CTRL_RST` during init — on QEMU that has been observed to leave the NIC in a
//! state that DMAs with stale ring pointers and reboots the guest.
//! Same guest IP assumptions as `net.rs` (QEMU `-netdev user`).

use crate::pci;
use bootloader_api::BootInfo;
use core::sync::atomic::{fence, Ordering};

const INTEL_VENDOR: u16 = 0x8086;
/// Legacy PCI Intel gigabit devices that share the same descriptor layout as QEMU's `e1000` (82540EM).
const E1000_LEGACY_DIDS: &[u16] = &[
    0x100E, // 82540EM — `qemu -device e1000`
    0x100F, // 82545EM — `e1000-82545em`
    0x101E, // 82540EP
    0x1008, // 82544EI (copper)
    0x1004, // 82543GC
    // QEMU `e1000e` / 82574L — emulated core accepts **legacy** RX/TX rings like `e1000`.
    0x10D3, // 82574L
    0x15A0, // I218-V / related ICH
    0x15A2,
    0x15A3,
];

const REG_CTRL: usize = 0x0000;
const REG_STATUS: usize = 0x0008;
const REG_IMC: usize = 0x00D8;
const REG_RCTL: usize = 0x0100;
const REG_TCTL: usize = 0x0400;
const REG_TIPG: usize = 0x0410;
const REG_RDBAL: usize = 0x2800;
const REG_RDBAH: usize = 0x2804;
const REG_RDLEN: usize = 0x2808;
const REG_RDH: usize = 0x2810;
const REG_RDT: usize = 0x2818;
const REG_TDBAL: usize = 0x3800;
const REG_TDBAH: usize = 0x3804;
const REG_TDLEN: usize = 0x3808;
const REG_TDH: usize = 0x3810;
const REG_TDT: usize = 0x3818;
const REG_RAL0: usize = 0x5400;
const REG_RAH0: usize = 0x5404;

const CTRL_SLU: u32 = 1 << 5;

const RCTL_EN: u32 = 1 << 1;
const RCTL_SBP: u32 = 1 << 2;
const RCTL_MPE: u32 = 1 << 4;
const RCTL_BAM: u32 = 1 << 15;
const RCTL_SECRC: u32 = 1 << 26;

const TCTL_EN: u32 = 1 << 1;
const TCTL_PSP: u32 = 1 << 3;
const TCTL_CT: u32 = 0x10 << 4;
const TCTL_COLD: u32 = 0x40 << 12;

const TX_CMD_EOP: u8 = 1 << 0;
const TX_CMD_IFCS: u8 = 1 << 1;
const TX_CMD_RS: u8 = 1 << 3;

const RX_DD: u8 = 1;

const TX_DD: u8 = 1;

const NUM_RX: usize = 32;
const NUM_TX: usize = 8;
const RX_BUF: usize = 2048;
const TX_BUF: usize = 2048;

#[repr(C, align(16))]
#[derive(Clone, Copy)]
struct LegacyRxDesc {
    addr: u64,
    length: u16,
    checksum: u16,
    status: u8,
    errors: u8,
    special: u16,
}

#[repr(C, align(16))]
#[derive(Clone, Copy)]
struct LegacyTxDesc {
    addr: u64,
    length: u16,
    cso: u8,
    cmd: u8,
    status: u8,
    css: u8,
    special: u16,
}

#[repr(C, align(4096))]
struct E1000Pages {
    rx_desc: [LegacyRxDesc; NUM_RX],
    tx_desc: [LegacyTxDesc; NUM_TX],
    rx_buf: [[u8; RX_BUF]; NUM_RX],
    tx_buf: [[u8; TX_BUF]; NUM_TX],
}

static mut PAGES: E1000Pages = E1000Pages {
    rx_desc: [LegacyRxDesc {
        addr: 0,
        length: 0,
        checksum: 0,
        status: 0,
        errors: 0,
        special: 0,
    }; NUM_RX],
    tx_desc: [LegacyTxDesc {
        addr: 0,
        length: 0,
        cso: 0,
        cmd: 0,
        status: 0,
        css: 0,
        special: 0,
    }; NUM_TX],
    rx_buf: [[0u8; RX_BUF]; NUM_RX],
    tx_buf: [[0u8; TX_BUF]; NUM_TX],
};

pub struct E1000 {
    mmio: usize,
    phys_skew: Option<u64>,
    pub mac: [u8; 6],
    pub rx_packets: u64,
    rx_cur: usize,
    tx_wait: Option<usize>,
}

unsafe fn bar0_mem(bus: u8, slot: u8, func: u8) -> Option<usize> {
    let raw = pci::read_u32(bus, slot, func, 0x10);
    if raw == 0 || raw == 0xFFFF_FFFF || (raw & 1) != 0 {
        return None;
    }
    let low = u64::from(raw & 0xFFFF_FFF0);
    let ty = (raw >> 1) & 3;
    let base = if ty == 2 {
        let hi = pci::read_u32(bus, slot, func, 0x14);
        low | (u64::from(hi) << 32)
    } else if ty == 0 {
        low
    } else {
        return None;
    };
    usize::try_from(base).ok()
}

unsafe fn mm_r32(base: usize, reg: usize) -> u32 {
    ((base + reg) as *const u32).read_volatile()
}

unsafe fn mm_w32(base: usize, reg: usize, v: u32) {
    ((base + reg) as *mut u32).write_volatile(v);
}

impl E1000 {
    fn phys(&self, virt: usize) -> u64 {
        match self.phys_skew {
            Some(off) => (virt as u64).wrapping_sub(off),
            None => virt as u64,
        }
    }

    pub unsafe fn probe(boot_info: &BootInfo) -> Option<Self> {
        for &did in E1000_LEGACY_DIDS {
            let mut locs = [(0u8, 0u8, 0u8); 8];
            let n = pci::find_device_any_fn(INTEL_VENDOR, did, &mut locs);
            if n == 0 {
                continue;
            }
            // Prefer PCI class 0x02/0x00 (ethernet).
            let picked = (0..n).find_map(|i| {
                let (b, s, f) = locs[i];
                match pci::class_subclass_prog_fn(b, s, f) {
                    Some((0x02, 0x00, _)) => Some((b, s, f)),
                    _ => None,
                }
            });
            let (bus, slot, func) = picked.unwrap_or(locs[0]);

            if let Some(dev) = Self::try_init(bus, slot, func, boot_info) {
                return Some(dev);
            }
        }
        None
    }

    unsafe fn try_init(bus: u8, slot: u8, func: u8, boot_info: &BootInfo) -> Option<Self> {
        pci::pci_enable_mmio_bm(bus, slot, func);
        let bar_phys = bar0_mem(bus, slot, func)?;
        let mmio = pci::pci_mmio_kernel_addr(boot_info.physical_memory_offset.into_option(), bar_phys);

        let skew = boot_info.physical_memory_offset.into();

        let ctrl_probe = mm_r32(mmio, REG_CTRL);
        let status_probe = mm_r32(mmio, REG_STATUS);
        if ctrl_probe == 0xFFFF_FFFF || status_probe == 0xFFFF_FFFF {
            return None;
        }

        // Stop RX/TX before touching descriptor rings (no global CTRL_RST — see module comment).
        mm_w32(mmio, REG_RCTL, 0);
        mm_w32(mmio, REG_TCTL, 0);
        for _ in 0..10_000 {
            core::hint::spin_loop();
        }

        pci::pci_enable_mmio_bm(bus, slot, func);

        mm_w32(mmio, REG_IMC, 0xFFFF_FFFF);

        let ral = mm_r32(mmio, REG_RAL0);
        let rah = mm_r32(mmio, REG_RAH0);
        let mut mac = [0u8; 6];
        mac[0] = (ral & 0xFF) as u8;
        mac[1] = ((ral >> 8) & 0xFF) as u8;
        mac[2] = ((ral >> 16) & 0xFF) as u8;
        mac[3] = ((ral >> 24) & 0xFF) as u8;
        mac[4] = (rah & 0xFF) as u8;
        mac[5] = ((rah >> 8) & 0xFF) as u8;

        let p = &mut *core::ptr::addr_of_mut!(PAGES);
        for i in 0..NUM_RX {
            p.rx_desc[i].addr = 0;
            p.rx_desc[i].length = 0;
            p.rx_desc[i].checksum = 0;
            p.rx_desc[i].status = 0;
            p.rx_desc[i].errors = 0;
            p.rx_desc[i].special = 0;
        }
        for i in 0..NUM_TX {
            p.tx_desc[i].addr = 0;
            p.tx_desc[i].length = 0;
            p.tx_desc[i].cso = 0;
            p.tx_desc[i].cmd = 0;
            p.tx_desc[i].status = 0;
            p.tx_desc[i].css = 0;
            p.tx_desc[i].special = 0;
        }

        let dev = E1000 {
            mmio,
            phys_skew: skew,
            mac,
            rx_packets: 0,
            rx_cur: 0,
            tx_wait: None,
        };

        for i in 0..NUM_RX {
            let pa = dev.phys(core::ptr::addr_of!(p.rx_buf[i]) as usize);
            p.rx_desc[i].addr = pa;
            p.rx_desc[i].length = 0;
        }

        let rdbal = dev.phys(core::ptr::addr_of!(p.rx_desc) as usize);
        mm_w32(mmio, REG_RDBAL, rdbal as u32);
        mm_w32(mmio, REG_RDBAH, (rdbal >> 32) as u32);
        mm_w32(mmio, REG_RDLEN, (NUM_RX * core::mem::size_of::<LegacyRxDesc>()) as u32);
        mm_w32(mmio, REG_RDH, 0);
        mm_w32(mmio, REG_RDT, (NUM_RX - 1) as u32);

        for i in 0..NUM_TX {
            let pa = dev.phys(core::ptr::addr_of!(p.tx_buf[i]) as usize);
            p.tx_desc[i].addr = pa;
        }

        let tdbal = dev.phys(core::ptr::addr_of!(p.tx_desc) as usize);
        mm_w32(mmio, REG_TDBAL, tdbal as u32);
        mm_w32(mmio, REG_TDBAH, (tdbal >> 32) as u32);
        mm_w32(mmio, REG_TDLEN, (NUM_TX * core::mem::size_of::<LegacyTxDesc>()) as u32);
        mm_w32(mmio, REG_TDH, 0);
        mm_w32(mmio, REG_TDT, 0);

        mm_w32(mmio, REG_TIPG, 0x0060_200A);

        // Bring link up (PHY); STATUS.LU (bit 1) is set by QEMU user-net quickly.
        mm_w32(mmio, REG_CTRL, mm_r32(mmio, REG_CTRL) | CTRL_SLU);
        for _ in 0..500_000 {
            if mm_r32(mmio, REG_STATUS) & (1 << 1) != 0 {
                break;
            }
            core::hint::spin_loop();
        }

        mm_w32(
            mmio,
            REG_RCTL,
            RCTL_EN | RCTL_SBP | RCTL_MPE | RCTL_BAM | RCTL_SECRC,
        );
        mm_w32(mmio, REG_TCTL, TCTL_EN | TCTL_PSP | TCTL_CT | TCTL_COLD);

        fence(Ordering::SeqCst);
        let _ = mm_r32(mmio, REG_STATUS);

        Some(dev)
    }

    /// One Ethernet frame (no virtio shim). Returns `None` if no packet.
    pub unsafe fn poll_rx_packet(&mut self, out: &mut [u8]) -> Option<usize> {
        let p = &mut *core::ptr::addr_of_mut!(PAGES);
        let i = self.rx_cur % NUM_RX;
        let d = &mut p.rx_desc[i];
        if d.status & RX_DD == 0 {
            return None;
        }

        let mut len = d.length as usize;
        if len > RX_BUF {
            len = RX_BUF;
        }
        if len == 0 || len > out.len() {
            d.status = 0;
            mm_w32(self.mmio, REG_RDT, i as u32);
            self.rx_cur = self.rx_cur.wrapping_add(1);
            return None;
        }

        out[..len].copy_from_slice(&p.rx_buf[i][..len]);
        d.status = 0;
        d.length = 0;
        fence(Ordering::SeqCst);
        mm_w32(self.mmio, REG_RDT, i as u32);
        self.rx_cur = self.rx_cur.wrapping_add(1);
        self.rx_packets = self.rx_packets.wrapping_add(1);
        Some(len)
    }

    pub unsafe fn transmit(&mut self, pkt: &[u8]) -> bool {
        if let Some(slot) = self.tx_wait {
            let p = &mut *core::ptr::addr_of_mut!(PAGES);
            if p.tx_desc[slot].status & TX_DD == 0 {
                for _ in 0..500_000 {
                    if p.tx_desc[slot].status & TX_DD != 0 {
                        break;
                    }
                    core::hint::spin_loop();
                }
            }
            p.tx_desc[slot].status = 0;
            self.tx_wait = None;
        }

        if pkt.is_empty() || pkt.len() > TX_BUF {
            return false;
        }

        let p = &mut *core::ptr::addr_of_mut!(PAGES);
        let tdt = mm_r32(self.mmio, REG_TDT) as usize % NUM_TX;

        if p.tx_desc[tdt].status & TX_DD == 0 && p.tx_desc[tdt].cmd != 0 {
            for _ in 0..500_000 {
                if p.tx_desc[tdt].status & TX_DD != 0 {
                    break;
                }
                core::hint::spin_loop();
            }
        }

        p.tx_buf[tdt][..pkt.len()].copy_from_slice(pkt);
        fence(Ordering::SeqCst);

        let td = &mut p.tx_desc[tdt];
        td.addr = self.phys(core::ptr::addr_of!(p.tx_buf[tdt]) as usize);
        td.length = pkt.len() as u16;
        td.cso = 0;
        td.cmd = TX_CMD_EOP | TX_CMD_IFCS | TX_CMD_RS;
        td.status = 0;
        td.css = 0;
        td.special = 0;
        fence(Ordering::SeqCst);

        let next = (tdt + 1) % NUM_TX;
        mm_w32(self.mmio, REG_TDT, next as u32);
        fence(Ordering::SeqCst);

        self.tx_wait = Some(tdt);
        true
    }
}
