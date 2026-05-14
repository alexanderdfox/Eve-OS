// SPDX-License-Identifier: MIT OR Apache-2.0

//! VirtIO 1.0 PCI network (QEMU `virtio-net-pci`): RX queue 0, TX queue 1.

use crate::pci;
use bootloader_api::BootInfo;
use core::sync::atomic::{fence, Ordering};

const VIRTIO_VENDOR: u16 = 0x1AF4;
const VIRTIO_NET: u16 = 0x1041;

const VIRTIO_PCI_CAP: u8 = 0x09;
const CFG_COMMON: u8 = 1;
const CFG_NOTIFY: u8 = 2;
const CFG_DEVICE: u8 = 4;

const STATUS_ACK: u8 = 1;
const STATUS_DRIVER: u8 = 2;
const STATUS_FEATURES_OK: u8 = 8;
const STATUS_DRIVER_OK: u8 = 4;

const VIRTQ_DESC_F_WRITE: u16 = 2;

/// RX virtqueue length (power of two, ≤ device max).
const RX_QSZ: usize = 256;
/// TX virtqueue length (small; single in-flight packet).
const TX_QSZ: usize = 8;

/// Matches `RxPages::rx` / `TxPages::tx` buffer sizes (avoid `static mut` borrows for `.len()`).
const RX_BUFFER_BYTES: usize = 4096;
const TX_BUFFER_BYTES: usize = 4096;

#[repr(C)]
#[derive(Clone, Copy)]
struct VirtqDesc {
    addr: u64,
    len: u32,
    flags: u16,
    next: u16,
}

#[repr(C)]
struct VirtqAvail {
    flags: u16,
    idx: u16,
    ring: [u16; RX_QSZ],
    used_event: u16,
}

#[repr(C)]
struct VirtqAvailTx {
    flags: u16,
    idx: u16,
    ring: [u16; TX_QSZ],
    used_event: u16,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct VirtqUsedElem {
    id: u32,
    len: u32,
}

#[repr(C)]
struct VirtqUsed {
    flags: u16,
    idx: u16,
    ring: [VirtqUsedElem; RX_QSZ],
    avail_event: u16,
}

#[repr(C)]
struct VirtqUsedTx {
    flags: u16,
    idx: u16,
    ring: [VirtqUsedElem; TX_QSZ],
    avail_event: u16,
}

#[repr(C, align(4096))]
struct RxPages {
    desc: [VirtqDesc; RX_QSZ],
    avail: VirtqAvail,
    used: VirtqUsed,
    rx: [u8; 4096],
}

#[repr(C, align(4096))]
struct TxPages {
    desc: [VirtqDesc; TX_QSZ],
    avail: VirtqAvailTx,
    used: VirtqUsedTx,
    tx: [u8; 4096],
}

static mut RXQ: RxPages = RxPages {
    desc: [VirtqDesc {
        addr: 0,
        len: 0,
        flags: 0,
        next: 0,
    }; RX_QSZ],
    avail: VirtqAvail {
        flags: 0,
        idx: 0,
        ring: [0; RX_QSZ],
        used_event: 0,
    },
    used: VirtqUsed {
        flags: 0,
        idx: 0,
        ring: [VirtqUsedElem { id: 0, len: 0 }; RX_QSZ],
        avail_event: 0,
    },
    rx: [0; 4096],
};

static mut TXQ: TxPages = TxPages {
    desc: [VirtqDesc {
        addr: 0,
        len: 0,
        flags: 0,
        next: 0,
    }; TX_QSZ],
    avail: VirtqAvailTx {
        flags: 0,
        idx: 0,
        ring: [0; TX_QSZ],
        used_event: 0,
    },
    used: VirtqUsedTx {
        flags: 0,
        idx: 0,
        ring: [VirtqUsedElem { id: 0, len: 0 }; TX_QSZ],
        avail_event: 0,
    },
    tx: [0; 4096],
};

pub struct VirtioNet {
    common: usize,
    notify: usize,
    notify_mul: u32,
    /// Device config MMIO (MAC read in `probe`; kept for future MTU / status caps).
    #[allow(dead_code)]
    device_cfg: usize,
    phys_skew: Option<u64>,
    rx_mask: usize,
    tx_mask: usize,
    pub mac: [u8; 6],
    pub rx_packets: u64,
    rx_last_used: u16,
    tx_last_used: u16,
    tx_inflight: bool,
}

unsafe fn cfg32(bus: u8, slot: u8, func: u8, off: u8) -> u32 {
    let mut v = 0u32;
    for i in 0..4 {
        v |= u32::from(pci::read_u8(bus, slot, func, off.wrapping_add(i))) << (8 * i);
    }
    v
}

unsafe fn bar_mem(bus: u8, slot: u8, func: u8, idx: u8) -> Option<usize> {
    let raw = pci::read_u32(bus, slot, func, 0x10 + idx * 4);
    if raw == 0 || raw == 0xFFFF_FFFF || (raw & 1) != 0 {
        return None;
    }
    Some((raw & 0xFFFF_FFF0) as usize)
}

unsafe fn mm_r8(b: usize, o: usize) -> u8 {
    ((b + o) as *const u8).read_volatile()
}

unsafe fn mm_w8(b: usize, o: usize, v: u8) {
    ((b + o) as *mut u8).write_volatile(v);
}

unsafe fn mm_r16(b: usize, o: usize) -> u16 {
    ((b + o) as *const u16).read_volatile()
}

unsafe fn mm_w16(b: usize, o: usize, v: u16) {
    ((b + o) as *mut u16).write_volatile(v);
}

unsafe fn mm_r32(b: usize, o: usize) -> u32 {
    ((b + o) as *const u32).read_volatile()
}

unsafe fn mm_w32(b: usize, o: usize, v: u32) {
    ((b + o) as *mut u32).write_volatile(v);
}

unsafe fn mm_w64(b: usize, o: usize, v: u64) {
    ((b + o) as *mut u64).write_volatile(v);
}

impl VirtioNet {
    fn phys(&self, virt: usize) -> u64 {
        match self.phys_skew {
            Some(off) => (virt as u64).wrapping_sub(off),
            None => virt as u64,
        }
    }

    unsafe fn notify_queue(&self, queue_sel: u16) {
        mm_w16(self.common, 22, queue_sel);
        fence(Ordering::SeqCst);
        let qn = mm_r16(self.common, 30);
        mm_w16(
            self.notify
                .wrapping_add(u32::from(qn).wrapping_mul(self.notify_mul) as usize),
            0,
            0,
        );
    }

    unsafe fn setup_one_queue(
        &self,
        queue_index: u16,
        queue_size: u16,
        p_desc: u64,
        p_avail: u64,
        p_used: u64,
    ) {
        mm_w16(self.common, 22, queue_index);
        mm_w16(self.common, 24, queue_size);
        mm_w16(self.common, 26, 0xFFFF);
        mm_w16(self.common, 28, 0);
        mm_w64(self.common, 32, p_desc);
        mm_w64(self.common, 40, p_avail);
        mm_w64(self.common, 48, p_used);
        mm_w16(self.common, 28, 1);
    }

    pub unsafe fn probe(boot_info: &BootInfo) -> Option<Self> {
        let mut locs = [(0u8, 0u8, 0u8); 8];
        let n = pci::find_device_any_fn(VIRTIO_VENDOR, VIRTIO_NET, &mut locs);
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

        let mmio_off = boot_info.physical_memory_offset.into_option();

        let cmd = pci::read_u16(bus, slot, func, 0x04);
        pci::write_u16(bus, slot, func, 0x04, cmd | 0x0006);

        let mut bars = [None; 6];
        for i in 0..6u8 {
            bars[i as usize] = bar_mem(bus, slot, func, i).map(|p| pci::pci_mmio_kernel_addr(mmio_off, p));
        }

        let mut cap = pci::read_u8(bus, slot, func, 0x34) & 0xFC;
        let mut common = None::<(u8, u32)>;
        let mut notify = None::<(u8, u32, u32)>;
        let mut devcfg = None::<(u8, u32)>;
        for _ in 0..64 {
            if cap == 0 {
                break;
            }
            if pci::read_u8(bus, slot, func, cap) == VIRTIO_PCI_CAP {
                let ty = pci::read_u8(bus, slot, func, cap.wrapping_add(3));
                let bar = pci::read_u8(bus, slot, func, cap.wrapping_add(4));
                let off = cfg32(bus, slot, func, cap.wrapping_add(8));
                match ty {
                    CFG_COMMON => common = Some((bar, off)),
                    CFG_NOTIFY => {
                        let mul = cfg32(bus, slot, func, cap.wrapping_add(16));
                        notify = Some((bar, off, mul));
                    }
                    CFG_DEVICE => devcfg = Some((bar, off)),
                    _ => {}
                }
            }
            cap = pci::read_u8(bus, slot, func, cap.wrapping_add(1));
            if cap != 0 {
                cap &= 0xFC;
            }
        }

        let (cbar, coff) = common?;
        let (nbar, noff, nmul) = notify?;
        let (dbar, doff) = devcfg?;
        let common_base = bars[cbar as usize]?.wrapping_add(coff as usize);
        let notify_base = bars[nbar as usize]?.wrapping_add(noff as usize);
        let device_cfg = bars[dbar as usize]?.wrapping_add(doff as usize);

        mm_w8(common_base, 20, 0);
        for _ in 0..50_000 {
            core::hint::spin_loop();
        }

        mm_w8(common_base, 20, STATUS_ACK);
        mm_w8(common_base, 20, STATUS_ACK | STATUS_DRIVER);

        mm_w32(common_base, 0, 1);
        let dev_f1 = mm_r32(common_base, 4);
        mm_w32(common_base, 8, 1);
        mm_w32(common_base, 12, dev_f1 & 1);

        mm_w32(common_base, 0, 0);
        let dev_f0 = mm_r32(common_base, 4);
        mm_w32(common_base, 8, 0);
        mm_w32(common_base, 12, dev_f0 & (1 << 5));

        mm_w8(
            common_base,
            20,
            STATUS_ACK | STATUS_DRIVER | STATUS_FEATURES_OK,
        );
        if mm_r8(common_base, 20) & STATUS_FEATURES_OK == 0 {
            return None;
        }

        mm_w16(common_base, 22, 0);
        let q0max = mm_r16(common_base, 24) as usize;
        if q0max < 2 || !q0max.is_power_of_two() {
            return None;
        }
        let rx_use = core::cmp::min(RX_QSZ, q0max);

        let skew = boot_info.physical_memory_offset;
        let mut net = VirtioNet {
            common: common_base,
            notify: notify_base,
            notify_mul: nmul,
            device_cfg,
            phys_skew: skew.into(),
            rx_mask: rx_use - 1,
            tx_mask: TX_QSZ - 1,
            mac: [0; 6],
            rx_packets: 0,
            rx_last_used: 0,
            tx_last_used: 0,
            tx_inflight: false,
        };

        let p_desc_rx = net.phys(core::ptr::addr_of!(RXQ.desc) as usize);
        let p_avail_rx = net.phys(core::ptr::addr_of!(RXQ.avail) as usize);
        let p_used_rx = net.phys(core::ptr::addr_of!(RXQ.used) as usize);
        let p_rx = net.phys(core::ptr::addr_of!(RXQ.rx) as usize);

        RXQ.desc[0].addr = p_rx;
        RXQ.desc[0].len = RX_BUFFER_BYTES as u32;
        RXQ.desc[0].flags = VIRTQ_DESC_F_WRITE;
        RXQ.desc[0].next = 0;
        RXQ.avail.flags = 0;
        RXQ.avail.idx = 0;
        RXQ.avail.ring[0] = 0;
        RXQ.avail.idx = 1;
        RXQ.used.flags = 0;
        RXQ.used.idx = 0;

        net.setup_one_queue(0, rx_use as u16, p_desc_rx, p_avail_rx, p_used_rx);

        mm_w16(common_base, 22, 1);
        let q1max = mm_r16(common_base, 24) as usize;
        if q1max < 2 || !q1max.is_power_of_two() {
            return None;
        }
        let tx_use = core::cmp::min(TX_QSZ, q1max);
        net.tx_mask = tx_use - 1;

        let p_desc_tx = net.phys(core::ptr::addr_of!(TXQ.desc) as usize);
        let p_avail_tx = net.phys(core::ptr::addr_of!(TXQ.avail) as usize);
        let p_used_tx = net.phys(core::ptr::addr_of!(TXQ.used) as usize);

        TXQ.desc[0].addr = 0;
        TXQ.desc[0].len = 0;
        TXQ.desc[0].flags = 0;
        TXQ.desc[0].next = 0;
        TXQ.avail.flags = 0;
        TXQ.avail.idx = 0;
        TXQ.used.flags = 0;
        TXQ.used.idx = 0;

        net.setup_one_queue(1, tx_use as u16, p_desc_tx, p_avail_tx, p_used_tx);

        mm_w8(
            common_base,
            20,
            STATUS_ACK | STATUS_DRIVER | STATUS_FEATURES_OK | STATUS_DRIVER_OK,
        );

        net.notify_queue(0);

        for i in 0..6 {
            net.mac[i] = mm_r8(device_cfg, i);
        }

        Some(net)
    }

    /// Copy one received L2 frame into `out` (virtio net hdr stripped). Returns length.
    pub unsafe fn poll_rx_packet(&mut self, out: &mut [u8]) -> Option<usize> {
        fence(Ordering::Acquire);
        let idx = core::ptr::read_volatile(core::ptr::addr_of!(RXQ.used.idx));
        if self.rx_last_used == idx {
            return None;
        }

        let slot = (self.rx_last_used as usize) & self.rx_mask;
        let elem = core::ptr::read_volatile(core::ptr::addr_of!(RXQ.used.ring[slot]));
        let total = elem.len as usize;
        self.rx_last_used = self.rx_last_used.wrapping_add(1);
        self.rx_packets = self.rx_packets.wrapping_add(1);

        if total == 0 || total > RX_BUFFER_BYTES {
            self.requeue_rx_only();
            return None;
        }

        let hdr = virtio_net_hdr_len(&RXQ.rx[..total]);
        let body = total.saturating_sub(hdr);
        if body == 0 || body > out.len() {
            self.requeue_rx_only();
            return None;
        }

        out[..body].copy_from_slice(&RXQ.rx[hdr..total]);

        self.requeue_rx_only();

        Some(body)
    }

    unsafe fn requeue_rx_only(&mut self) {
        RXQ.desc[0].addr = self.phys(core::ptr::addr_of!(RXQ.rx) as usize);
        RXQ.desc[0].len = RX_BUFFER_BYTES as u32;
        RXQ.desc[0].flags = VIRTQ_DESC_F_WRITE;

        let ai = core::ptr::read_volatile(core::ptr::addr_of!(RXQ.avail.idx));
        let slot = (ai as usize) & self.rx_mask;
        let ring = core::ptr::addr_of_mut!(RXQ.avail.ring).cast::<u16>();
        ring.add(slot).write_volatile(0);
        core::ptr::write_volatile(core::ptr::addr_of_mut!(RXQ.avail.idx), ai.wrapping_add(1));

        self.notify_queue(0);
        fence(Ordering::Release);
    }

    /// Send one TX buffer (must include virtio net header + Ethernet frame). Blocks for completion.
    pub unsafe fn transmit(&mut self, pkt: &[u8]) -> bool {
        if self.tx_inflight {
            for _ in 0..500_000 {
                let uidx = core::ptr::read_volatile(core::ptr::addr_of!(TXQ.used.idx));
                if uidx != self.tx_last_used {
                    self.tx_last_used = uidx;
                    self.tx_inflight = false;
                    break;
                }
                core::hint::spin_loop();
            }
        }
        if pkt.is_empty() || pkt.len() > TX_BUFFER_BYTES {
            return false;
        }

        TXQ.tx[..pkt.len()].copy_from_slice(pkt);
        let p_tx = self.phys(core::ptr::addr_of!(TXQ.tx) as usize);

        TXQ.desc[0].addr = p_tx;
        TXQ.desc[0].len = pkt.len() as u32;
        TXQ.desc[0].flags = 0;
        TXQ.desc[0].next = 0;

        let ai = core::ptr::read_volatile(core::ptr::addr_of!(TXQ.avail.idx));
        let slot = (ai as usize) & self.tx_mask;
        let ring = core::ptr::addr_of_mut!(TXQ.avail.ring).cast::<u16>();
        ring.add(slot).write_volatile(0);
        core::ptr::write_volatile(core::ptr::addr_of_mut!(TXQ.avail.idx), ai.wrapping_add(1));

        self.tx_inflight = true;
        self.notify_queue(1);
        fence(Ordering::SeqCst);

        for _ in 0..500_000 {
            let uidx = core::ptr::read_volatile(core::ptr::addr_of!(TXQ.used.idx));
            if uidx != self.tx_last_used {
                self.tx_last_used = uidx;
                self.tx_inflight = false;
                return true;
            }
            core::hint::spin_loop();
        }
        self.tx_inflight = false;
        false
    }
}

fn virtio_net_hdr_len(pkt: &[u8]) -> usize {
    if pkt.len() >= 14 {
        let et = u16::from_be_bytes([pkt[12], pkt[13]]);
        if et == 0x0800 || et == 0x0806 {
            return 0;
        }
    }
    if pkt.len() >= 12 {
        let et = u16::from_be_bytes([pkt[10], pkt[11]]);
        if et == 0x0800 || et == 0x0806 {
            return 10;
        }
    }
    core::cmp::min(12, pkt.len())
}
