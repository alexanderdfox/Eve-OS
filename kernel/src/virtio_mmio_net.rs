// SPDX-License-Identifier: MIT OR Apache-2.0

//! VirtIO 1.0 **MMIO** network (QEMU `virt` + `virtio-net-device`). Poll-based RX/TX like `virtio_net`.

use core::sync::atomic::{fence, Ordering};

const MM_MAGIC: usize = 0x000;
const MM_VERSION: usize = 0x004;
const MM_DEVICE_ID: usize = 0x008;
const MM_VENDOR_ID: usize = 0x00c;
const MM_DEVICE_FEATURES: usize = 0x010;
const MM_DEVICE_FEATURES_SEL: usize = 0x014;
const MM_DRIVER_FEATURES: usize = 0x020;
const MM_DRIVER_FEATURES_SEL: usize = 0x024;
const MM_QUEUE_SEL: usize = 0x030;
const MM_QUEUE_NUM_MAX: usize = 0x034;
const MM_QUEUE_NUM: usize = 0x038;
const MM_QUEUE_READY: usize = 0x044;
const MM_QUEUE_NOTIFY: usize = 0x050;
const MM_INTERRUPT_STATUS: usize = 0x060;
const MM_INTERRUPT_ACK: usize = 0x064;
const MM_STATUS: usize = 0x070;
const MM_QUEUE_DESC_LOW: usize = 0x080;
const MM_QUEUE_DESC_HIGH: usize = 0x084;
const MM_QUEUE_DRIVER_LOW: usize = 0x090;
const MM_QUEUE_DRIVER_HIGH: usize = 0x094;
const MM_QUEUE_DEVICE_LOW: usize = 0x0a0;
const MM_QUEUE_DEVICE_HIGH: usize = 0x0a4;
const MM_CONFIG0: usize = 0x100;

const VIRTIO_VENDOR: u32 = 0x1af4;
const MAGIC: u32 = 0x7472_6976;

const STATUS_ACK: u32 = 1;
const STATUS_DRIVER: u32 = 2;
const STATUS_DRIVER_OK: u32 = 4;
const STATUS_FEATURES_OK: u32 = 8;

const VIRTQ_DESC_F_WRITE: u16 = 2;

const RX_QSZ: usize = 256;
const TX_QSZ: usize = 8;
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

pub struct VirtioMmioNet {
    base: usize,
    rx_mask: usize,
    tx_mask: usize,
    pub mac: [u8; 6],
    pub rx_packets: u64,
    rx_last_used: u16,
    tx_last_used: u16,
    tx_inflight: bool,
}

unsafe fn mm_r32(base: usize, o: usize) -> u32 {
    ((base + o) as *const u32).read_volatile()
}

unsafe fn mm_w32(base: usize, o: usize, v: u32) {
    ((base + o) as *mut u32).write_volatile(v);
}

impl VirtioMmioNet {
    fn phys(virt: usize) -> u64 {
        virt as u64
    }

    unsafe fn status_write(base: usize, v: u32) {
        mm_w32(base, MM_STATUS, v);
    }

    unsafe fn status_read(base: usize) -> u32 {
        mm_r32(base, MM_STATUS)
    }

    unsafe fn notify_queue(base: usize, q: u32) {
        mm_w32(base, MM_QUEUE_NOTIFY, q);
        fence(Ordering::SeqCst);
    }

    unsafe fn ack_interrupts(base: usize) {
        let st = mm_r32(base, MM_INTERRUPT_STATUS);
        mm_w32(base, MM_INTERRUPT_ACK, st);
    }

    /// Scan QEMU `virt` virtio-mmio slots (512-byte stride from `0x0a00_0000`).
    pub unsafe fn probe() -> Option<Self> {
        const SCAN0: usize = 0x0a00_0000;
        const SCAN1: usize = 0x0a01_0000;
        const STEP: usize = 0x200;
        let mut b = SCAN0;
        while b < SCAN1 {
            if mm_r32(b, MM_MAGIC) == MAGIC
                && mm_r32(b, MM_VERSION) == 2
                && mm_r32(b, MM_DEVICE_ID) == 1
                && mm_r32(b, MM_VENDOR_ID) == VIRTIO_VENDOR
            {
                return Self::init_at(b);
            }
            b += STEP;
        }
        None
    }

    unsafe fn init_at(base: usize) -> Option<Self> {
        Self::ack_interrupts(base);

        mm_w32(base, MM_STATUS, 0);
        for _ in 0..50_000 {
            core::hint::spin_loop();
        }

        Self::status_write(base, STATUS_ACK);
        Self::status_write(base, STATUS_ACK | STATUS_DRIVER);

        mm_w32(base, MM_DEVICE_FEATURES_SEL, 1);
        let dev_f1 = mm_r32(base, MM_DEVICE_FEATURES);
        mm_w32(base, MM_DRIVER_FEATURES_SEL, 1);
        mm_w32(base, MM_DRIVER_FEATURES, dev_f1 & 1);

        mm_w32(base, MM_DEVICE_FEATURES_SEL, 0);
        let dev_f0 = mm_r32(base, MM_DEVICE_FEATURES);
        mm_w32(base, MM_DRIVER_FEATURES_SEL, 0);
        mm_w32(base, MM_DRIVER_FEATURES, dev_f0 & (1 << 5));

        Self::status_write(
            base,
            STATUS_ACK | STATUS_DRIVER | STATUS_FEATURES_OK,
        );
        if Self::status_read(base) & STATUS_FEATURES_OK == 0 {
            return None;
        }

        mm_w32(base, MM_QUEUE_SEL, 0);
        let q0max = mm_r32(base, MM_QUEUE_NUM_MAX) as usize;
        if q0max < 2 || !q0max.is_power_of_two() {
            return None;
        }
        let rx_use = core::cmp::min(RX_QSZ, q0max);

        let mut net = VirtioMmioNet {
            base,
            rx_mask: rx_use - 1,
            tx_mask: TX_QSZ - 1,
            mac: [0; 6],
            rx_packets: 0,
            rx_last_used: 0,
            tx_last_used: 0,
            tx_inflight: false,
        };

        let p_desc_rx = Self::phys(core::ptr::addr_of!(RXQ.desc) as usize);
        let p_avail_rx = Self::phys(core::ptr::addr_of!(RXQ.avail) as usize);
        let p_used_rx = Self::phys(core::ptr::addr_of!(RXQ.used) as usize);
        let p_rx = Self::phys(core::ptr::addr_of!(RXQ.rx) as usize);

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

        mm_w32(base, MM_QUEUE_SEL, 0);
        mm_w32(base, MM_QUEUE_NUM, rx_use as u32);
        mm_w32(base, MM_QUEUE_DESC_LOW, p_desc_rx as u32);
        mm_w32(base, MM_QUEUE_DESC_HIGH, (p_desc_rx >> 32) as u32);
        mm_w32(base, MM_QUEUE_DRIVER_LOW, p_avail_rx as u32);
        mm_w32(base, MM_QUEUE_DRIVER_HIGH, (p_avail_rx >> 32) as u32);
        mm_w32(base, MM_QUEUE_DEVICE_LOW, p_used_rx as u32);
        mm_w32(base, MM_QUEUE_DEVICE_HIGH, (p_used_rx >> 32) as u32);
        mm_w32(base, MM_QUEUE_READY, 1);

        mm_w32(base, MM_QUEUE_SEL, 1);
        let q1max = mm_r32(base, MM_QUEUE_NUM_MAX) as usize;
        if q1max < 2 || !q1max.is_power_of_two() {
            return None;
        }
        let tx_use = core::cmp::min(TX_QSZ, q1max);
        net.tx_mask = tx_use - 1;

        let p_desc_tx = Self::phys(core::ptr::addr_of!(TXQ.desc) as usize);
        let p_avail_tx = Self::phys(core::ptr::addr_of!(TXQ.avail) as usize);
        let p_used_tx = Self::phys(core::ptr::addr_of!(TXQ.used) as usize);

        TXQ.desc[0].addr = 0;
        TXQ.desc[0].len = 0;
        TXQ.desc[0].flags = 0;
        TXQ.desc[0].next = 0;
        TXQ.avail.flags = 0;
        TXQ.avail.idx = 0;
        TXQ.used.flags = 0;
        TXQ.used.idx = 0;

        mm_w32(base, MM_QUEUE_NUM, tx_use as u32);
        mm_w32(base, MM_QUEUE_DESC_LOW, p_desc_tx as u32);
        mm_w32(base, MM_QUEUE_DESC_HIGH, (p_desc_tx >> 32) as u32);
        mm_w32(base, MM_QUEUE_DRIVER_LOW, p_avail_tx as u32);
        mm_w32(base, MM_QUEUE_DRIVER_HIGH, (p_avail_tx >> 32) as u32);
        mm_w32(base, MM_QUEUE_DEVICE_LOW, p_used_tx as u32);
        mm_w32(base, MM_QUEUE_DEVICE_HIGH, (p_used_tx >> 32) as u32);
        mm_w32(base, MM_QUEUE_READY, 1);

        Self::status_write(
            base,
            STATUS_ACK | STATUS_DRIVER | STATUS_FEATURES_OK | STATUS_DRIVER_OK,
        );

        Self::notify_queue(base, 0);

        for i in 0..6 {
            net.mac[i] = ((base + MM_CONFIG0 + i) as *const u8).read_volatile();
        }

        Some(net)
    }

    pub unsafe fn poll_rx_packet(&mut self, out: &mut [u8]) -> Option<usize> {
        Self::ack_interrupts(self.base);
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
        RXQ.desc[0].addr = Self::phys(core::ptr::addr_of!(RXQ.rx) as usize);
        RXQ.desc[0].len = RX_BUFFER_BYTES as u32;
        RXQ.desc[0].flags = VIRTQ_DESC_F_WRITE;

        let ai = core::ptr::read_volatile(core::ptr::addr_of!(RXQ.avail.idx));
        let slot = (ai as usize) & self.rx_mask;
        let ring = core::ptr::addr_of_mut!(RXQ.avail.ring).cast::<u16>();
        ring.add(slot).write_volatile(0);
        core::ptr::write_volatile(core::ptr::addr_of_mut!(RXQ.avail.idx), ai.wrapping_add(1));

        Self::notify_queue(self.base, 0);
        fence(Ordering::Release);
    }

    pub unsafe fn transmit(&mut self, pkt: &[u8]) -> bool {
        Self::ack_interrupts(self.base);
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
        let p_tx = Self::phys(core::ptr::addr_of!(TXQ.tx) as usize);

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
        Self::notify_queue(self.base, 1);
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
