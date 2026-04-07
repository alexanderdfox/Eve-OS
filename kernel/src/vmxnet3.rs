// SPDX-License-Identifier: MIT OR Apache-2.0

//! VMware **vmxnet3** (PCI **15AD:07B0**).
//!
//! This module now performs real PCI/MMIO attach and MAC discovery so vmxnet3 devices are visible in
//! Eve's NIC abstraction. Full UPT queue activation (driver-shared block + RX/TX rings + completion
//! processing) is still incremental work.

use crate::pci;
use bootloader_api::BootInfo;
use core::sync::atomic::{fence, Ordering};

const VMWARE_VENDOR: u16 = 0x15AD;
const VMXNET3_DID: u16 = 0x07B0;

// BAR0 register window (subset).
const REG_MACL: usize = 0x28;
const REG_MACH: usize = 0x30;
const REG_DSAL: usize = 0x108;
const REG_DSAH: usize = 0x10C;
const REG_CMD: usize = 0x20;
const REG_TX_DOORBELL: usize = 0x600;
const TX_RING_SIZE: usize = 128;
const RX_RING_SIZE: usize = 128;
const TX_BUF_SIZE: usize = 2048;
const RX_BUF_SIZE: usize = 2048;
const VMXNET3_CMD_ACTIVATE_DEV: u32 = 0xCAFE_0001;
const DESC_OWN_DEV: u32 = 1 << 31;
const DESC_DONE: u32 = 1 << 30;
const DESC_LEN_MASK: u32 = 0x0000_3FFF;
const TX_STALL_TICKS: u32 = 4096;

#[repr(C, align(16))]
#[derive(Clone, Copy)]
struct TxDesc {
    addr: u64,
    len_flags: u32,
    gen: u32,
}

#[repr(C, align(16))]
#[derive(Clone, Copy)]
struct RxDesc {
    addr: u64,
    len_flags: u32,
    gen: u32,
}

const EMPTY_TX: TxDesc = TxDesc {
    addr: 0,
    len_flags: 0,
    gen: 0,
};
const EMPTY_RX: RxDesc = RxDesc {
    addr: 0,
    len_flags: 0,
    gen: 0,
};

#[repr(C, align(4096))]
struct Vmxnet3Shared {
    tx_ring: [TxDesc; TX_RING_SIZE],
    rx_ring: [RxDesc; RX_RING_SIZE],
    tx_buf: [[u8; TX_BUF_SIZE]; TX_RING_SIZE],
    rx_buf: [[u8; RX_BUF_SIZE]; RX_RING_SIZE],
}

static mut SHARED: Vmxnet3Shared = Vmxnet3Shared {
    tx_ring: [EMPTY_TX; TX_RING_SIZE],
    rx_ring: [EMPTY_RX; RX_RING_SIZE],
    tx_buf: [[0; TX_BUF_SIZE]; TX_RING_SIZE],
    rx_buf: [[0; RX_BUF_SIZE]; RX_RING_SIZE],
};

pub struct Vmxnet3 {
    mmio: usize,
    phys_skew: Option<u64>,
    pub mac: [u8; 6],
    pub rx_packets: u64,
    pub tx_packets: u64,
    tx_head: u16,
    tx_tail: u16,
    tx_stall_ticks: u32,
    rx_head: u16,
    queues_ready: bool,
}

unsafe fn bar0_mem(bus: u8, slot: u8, func: u8) -> Option<usize> {
    let raw = pci::read_u32(bus, slot, func, 0x10);
    if raw == 0 || raw == 0xFFFF_FFFF || (raw & 1) != 0 {
        return None;
    }
    Some((raw & 0xFFFF_FFF0) as usize)
}

#[inline]
unsafe fn mm_r32(base: usize, off: usize) -> u32 {
    ((base + off) as *const u32).read_volatile()
}

#[inline]
unsafe fn mm_w32(base: usize, off: usize, v: u32) {
    ((base + off) as *mut u32).write_volatile(v)
}

impl Vmxnet3 {
    pub unsafe fn probe(boot_info: &BootInfo) -> Option<Self> {
        let mut locs = [(0u8, 0u8, 0u8); 8];
        let n = pci::find_device_any_fn(VMWARE_VENDOR, VMXNET3_DID, &mut locs);
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

        pci::pci_enable_mmio_bm(bus, slot, func);
        let mmio = bar0_mem(bus, slot, func)?;
        let skew = boot_info.physical_memory_offset.into();

        let macl = mm_r32(mmio, REG_MACL);
        let mach = mm_r32(mmio, REG_MACH);
        if macl == 0xFFFF_FFFF || mach == 0xFFFF_FFFF {
            return None;
        }
        let mut mac = [0u8; 6];
        mac[0] = (macl & 0xFF) as u8;
        mac[1] = ((macl >> 8) & 0xFF) as u8;
        mac[2] = ((macl >> 16) & 0xFF) as u8;
        mac[3] = ((macl >> 24) & 0xFF) as u8;
        mac[4] = (mach & 0xFF) as u8;
        mac[5] = ((mach >> 8) & 0xFF) as u8;
        if mac == [0; 6] {
            return None;
        }
        Some(Self {
            mmio,
            phys_skew: skew,
            mac,
            rx_packets: 0,
            tx_packets: 0,
            tx_head: 0,
            tx_tail: 0,
            tx_stall_ticks: 0,
            rx_head: 0,
            queues_ready: false,
        })
    }

    pub unsafe fn poll_rx_packet(&mut self, _out: &mut [u8]) -> Option<usize> {
        if !self.queues_ready {
            self.queues_ready = self.seed_queue_state();
            return None;
        }
        self.reap_tx_completions();
        let slot = usize::from(self.rx_head) % RX_RING_SIZE;
        let desc = &mut SHARED.rx_ring[slot];
        let flags = desc.len_flags;
        if (flags & DESC_DONE) == 0 {
            return None;
        }
        let got = (flags & DESC_LEN_MASK) as usize;
        if got == 0 {
            return None;
        }
        let n = got.min(_out.len()).min(RX_BUF_SIZE);
        _out[..n].copy_from_slice(&SHARED.rx_buf[slot][..n]);
        self.rx_packets = self.rx_packets.saturating_add(1);
        // Return descriptor to device ownership.
        desc.len_flags = (RX_BUF_SIZE as u32) | DESC_OWN_DEV;
        desc.gen ^= 1;
        self.rx_head = self.rx_head.wrapping_add(1);
        let repl = usize::from(self.rx_head) % RX_RING_SIZE;
        SHARED.rx_ring[repl].addr = self.phys(SHARED.rx_buf[repl].as_ptr() as usize);
        SHARED.rx_ring[repl].len_flags = (RX_BUF_SIZE as u32) | DESC_OWN_DEV;
        SHARED.rx_ring[repl].gen = 1;
        Some(n)
    }

    pub unsafe fn transmit(&mut self, _pkt: &[u8]) -> bool {
        if !self.queues_ready {
            self.queues_ready = self.seed_queue_state();
            return false;
        }
        self.reap_tx_completions();
        let used = usize::from(self.tx_tail.wrapping_sub(self.tx_head));
        if used >= TX_RING_SIZE.saturating_sub(1) {
            self.tx_stall_ticks = self.tx_stall_ticks.saturating_add(1);
            if self.tx_stall_ticks > TX_STALL_TICKS {
                self.recover_tx_ring();
            }
            return false;
        }
        self.tx_stall_ticks = 0;
        let slot = usize::from(self.tx_tail) % TX_RING_SIZE;
        let data = _pkt;
        if data.is_empty() || data.len() > TX_BUF_SIZE {
            return false;
        }
        let tx_buf = &mut SHARED.tx_buf[slot];
        tx_buf[..data.len()].copy_from_slice(data);
        let p = self.phys(tx_buf.as_ptr() as usize);
        SHARED.tx_ring[slot].addr = p;
        SHARED.tx_ring[slot].len_flags = (data.len() as u32) | DESC_OWN_DEV;
        // `gen` is a software-visible ownership hint in this scaffold:
        // driver submits with 1, completion path expects device to flip to 0.
        SHARED.tx_ring[slot].gen = 1;
        fence(Ordering::Release);
        mm_w32(self.mmio, REG_TX_DOORBELL, u32::from(self.tx_tail));
        self.tx_tail = self.tx_tail.wrapping_add(1);
        self.tx_packets = self.tx_packets.saturating_add(1);
        true
    }

    #[inline]
    unsafe fn seed_queue_state(&mut self) -> bool {
        self.tx_head = 0;
        self.tx_tail = 0;
        self.tx_stall_ticks = 0;
        self.rx_head = 0;
        for i in 0..TX_RING_SIZE {
            SHARED.tx_ring[i] = EMPTY_TX;
        }
        for i in 0..RX_RING_SIZE {
            SHARED.rx_ring[i] = EMPTY_RX;
            SHARED.rx_ring[i].addr = self.phys(SHARED.rx_buf[i].as_ptr() as usize);
            SHARED.rx_ring[i].len_flags = (RX_BUF_SIZE as u32) | DESC_OWN_DEV;
            SHARED.rx_ring[i].gen = 1;
        }
        let dsa = self.phys(core::ptr::addr_of!(SHARED) as usize);
        mm_w32(self.mmio, REG_DSAL, dsa as u32);
        mm_w32(self.mmio, REG_DSAH, (dsa >> 32) as u32);
        mm_w32(self.mmio, REG_CMD, VMXNET3_CMD_ACTIVATE_DEV);
        true
    }

    #[inline]
    unsafe fn reap_tx_completions(&mut self) {
        let mut progressed = false;
        while self.tx_head != self.tx_tail {
            let slot = usize::from(self.tx_head) % TX_RING_SIZE;
            let d = &mut SHARED.tx_ring[slot];
            if d.len_flags == 0 {
                self.tx_head = self.tx_head.wrapping_add(1);
                progressed = true;
                continue;
            }
            if (d.len_flags & DESC_OWN_DEV) != 0 {
                break;
            }
            d.addr = 0;
            d.len_flags = 0;
            self.tx_head = self.tx_head.wrapping_add(1);
            progressed = true;
        }
        if progressed {
            self.tx_stall_ticks = 0;
        }
    }

    #[inline]
    unsafe fn recover_tx_ring(&mut self) {
        for i in 0..TX_RING_SIZE {
            SHARED.tx_ring[i] = EMPTY_TX;
        }
        self.tx_head = 0;
        self.tx_tail = 0;
        self.tx_stall_ticks = 0;
        // Kick queue tail 0 to re-synchronize with device-side producer view.
        mm_w32(self.mmio, REG_TX_DOORBELL, 0);
    }

    #[inline]
    fn phys(&self, virt: usize) -> u64 {
        match self.phys_skew {
            Some(off) => (virt as u64).wrapping_sub(off),
            None => virt as u64,
        }
    }
}
