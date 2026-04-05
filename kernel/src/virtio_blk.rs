// SPDX-License-Identifier: MIT OR Apache-2.0

//! VirtIO 1.0 PCI block device (`virtio-blk`, 0x1AF4/0x1042): queue 0, 512-byte sectors.
//! Used to clone the boot disk onto a second VirtIO disk from the Disk Install screen (QEMU / VMs).

use crate::pci;
use bootloader_api::BootInfo;
use core::sync::atomic::{fence, Ordering};

const VIRTIO_VENDOR: u16 = 0x1AF4;
pub const VIRTIO_BLK_DEVICE: u16 = 0x1042;

const VIRTIO_PCI_CAP: u8 = 0x09;
const CFG_COMMON: u8 = 1;
const CFG_NOTIFY: u8 = 2;
const CFG_DEVICE: u8 = 4;

const STATUS_ACK: u8 = 1;
const STATUS_DRIVER: u8 = 2;
const STATUS_FEATURES_OK: u8 = 8;
const STATUS_DRIVER_OK: u8 = 4;

const VIRTQ_DESC_F_NEXT: u16 = 1;
const VIRTQ_DESC_F_WRITE: u16 = 2;

const VIRTIO_BLK_F_RO: u32 = 1 << 5;
const VIRTIO_BLK_F_BLK_SIZE: u32 = 1 << 6;

const VIRTIO_BLK_T_IN: u32 = 0;
const VIRTIO_BLK_T_OUT: u32 = 1;

const VIRTIO_BLK_S_OK: u8 = 0;

const BLK_QSZ: usize = 64;
const SECTOR: usize = 512;

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
    ring: [u16; BLK_QSZ],
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
    ring: [VirtqUsedElem; BLK_QSZ],
    avail_event: u16,
}

/// Request header + one sector + status byte (three-descriptor chain).
#[repr(C, align(16))]
struct BlkPages {
    desc: [VirtqDesc; 4],
    avail: VirtqAvail,
    used: VirtqUsed,
    req: [u8; 16],
    data: [u8; SECTOR],
    status: u8,
    _pad: [u8; 15],
    /// Last seen `used.idx` for queue 0 (per-device; lives with queue pages).
    vq_last_seen: u16,
}

static mut BLK0: BlkPages = BlkPages {
    desc: [VirtqDesc {
        addr: 0,
        len: 0,
        flags: 0,
        next: 0,
    }; 4],
    avail: VirtqAvail {
        flags: 0,
        idx: 0,
        ring: [0; BLK_QSZ],
        used_event: 0,
    },
    used: VirtqUsed {
        flags: 0,
        idx: 0,
        ring: [VirtqUsedElem { id: 0, len: 0 }; BLK_QSZ],
        avail_event: 0,
    },
    req: [0; 16],
    data: [0; SECTOR],
    status: 0,
    _pad: [0; 15],
    vq_last_seen: 0,
};

static mut BLK1: BlkPages = BlkPages {
    desc: [VirtqDesc {
        addr: 0,
        len: 0,
        flags: 0,
        next: 0,
    }; 4],
    avail: VirtqAvail {
        flags: 0,
        idx: 0,
        ring: [0; BLK_QSZ],
        used_event: 0,
    },
    used: VirtqUsed {
        flags: 0,
        idx: 0,
        ring: [VirtqUsedElem { id: 0, len: 0 }; BLK_QSZ],
        avail_event: 0,
    },
    req: [0; 16],
    data: [0; SECTOR],
    status: 0,
    _pad: [0; 15],
    vq_last_seen: 0,
};

pub struct VirtioBlk {
    common: usize,
    notify: usize,
    notify_mul: u32,
    #[allow(dead_code)]
    device_cfg: usize,
    phys_skew: Option<u64>,
    qmask: usize,
    pages: *mut BlkPages,
    /// Capacity in **logical** sectors (`sector_size` bytes each).
    pub capacity: u64,
    pub sector_size: u32,
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

impl VirtioBlk {
    fn phys(&self, virt: usize) -> u64 {
        match self.phys_skew {
            Some(off) => (virt as u64).wrapping_sub(off),
            None => virt as u64,
        }
    }

    unsafe fn setup_queue(&self, queue_size: u16, p_desc: u64, p_avail: u64, p_used: u64) {
        mm_w16(self.common, 22, 0);
        mm_w16(self.common, 24, queue_size);
        mm_w16(self.common, 26, 0xFFFF);
        mm_w16(self.common, 28, 0);
        mm_w64(self.common, 32, p_desc);
        mm_w64(self.common, 40, p_avail);
        mm_w64(self.common, 48, p_used);
        mm_w16(self.common, 28, 1);
    }

    /// `pages_slot`: 0 → `BLK0`, 1 → `BLK1`. `reject_read_only` fails init if the device is RO (for install targets).
    pub unsafe fn init(
        bus: u8,
        slot: u8,
        func: u8,
        boot_info: &BootInfo,
        pages_slot: u8,
        reject_read_only: bool,
    ) -> Option<Self> {
        let pages = match pages_slot {
            0 => core::ptr::addr_of_mut!(BLK0),
            1 => core::ptr::addr_of_mut!(BLK1),
            _ => return None,
        };

        pci::pci_enable_mmio_bm(bus, slot, func);

        let mut bars = [None; 6];
        for i in 0..6u8 {
            bars[i as usize] = bar_mem(bus, slot, func, i);
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

        mm_w32(common_base, 0, 0);
        let dev_f0 = mm_r32(common_base, 4);
        if reject_read_only && (dev_f0 & VIRTIO_BLK_F_RO) != 0 {
            return None;
        }
        let mut driver_f0 = dev_f0 & !VIRTIO_BLK_F_RO;
        driver_f0 &= !VIRTIO_BLK_F_BLK_SIZE;
        mm_w32(common_base, 8, 0);
        mm_w32(common_base, 12, driver_f0);

        mm_w32(common_base, 0, 1);
        let dev_f1 = mm_r32(common_base, 4);
        mm_w32(common_base, 8, 1);
        mm_w32(common_base, 12, dev_f1 & 1);

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
        if q0max < 4 || !q0max.is_power_of_two() {
            return None;
        }
        let q_use = core::cmp::min(BLK_QSZ, q0max);

        let skew = boot_info.physical_memory_offset;
        let mut dev = VirtioBlk {
            common: common_base,
            notify: notify_base,
            notify_mul: nmul,
            device_cfg,
            phys_skew: skew.into(),
            qmask: q_use - 1,
            pages,
            capacity: 0,
            sector_size: SECTOR as u32,
        };

        let cap_lo = mm_r32(device_cfg, 0);
        let cap_hi = mm_r32(device_cfg, 4);
        dev.capacity = u64::from(cap_lo) | (u64::from(cap_hi) << 32);

        let p_desc = dev.phys(core::ptr::addr_of!((*pages).desc) as usize);
        let p_avail = dev.phys(core::ptr::addr_of!((*pages).avail) as usize);
        let p_used = dev.phys(core::ptr::addr_of!((*pages).used) as usize);

        (*pages).desc[0] = VirtqDesc {
            addr: 0,
            len: 0,
            flags: 0,
            next: 0,
        };
        (*pages).avail.flags = 0;
        (*pages).avail.idx = 0;
        (*pages).used.flags = 0;
        (*pages).used.idx = 0;
        (*pages).vq_last_seen = 0;

        dev.setup_queue(q_use as u16, p_desc, p_avail, p_used);

        mm_w8(
            common_base,
            20,
            STATUS_ACK | STATUS_DRIVER | STATUS_FEATURES_OK | STATUS_DRIVER_OK,
        );

        Some(dev)
    }

    fn fill_req_header(req: &mut [u8; 16], typ: u32, sector: u64) {
        req[0..4].copy_from_slice(&typ.to_le_bytes());
        req[4..8].copy_from_slice(&0u32.to_le_bytes());
        req[8..16].copy_from_slice(&sector.to_le_bytes());
    }

    /// Read one logical sector into `out` (must be `sector_size` bytes).
    pub unsafe fn read_sector(&mut self, sector: u64, out: &mut [u8]) -> bool {
        let ss = self.sector_size as usize;
        if out.len() < ss || sector >= self.capacity {
            return false;
        }
        let skew = self.phys_skew;
        let common = self.common;
        let notify = self.notify;
        let notify_mul = self.notify_mul;
        let qmask = self.qmask;
        let phys = |virt: usize| -> u64 {
            match skew {
                Some(off) => (virt as u64).wrapping_sub(off),
                None => virt as u64,
            }
        };

        let p = &mut *self.pages;
        let mut lu = p.vq_last_seen;
        Self::fill_req_header(&mut p.req, VIRTIO_BLK_T_IN, sector);
        p.status = 0xFF;

        let p_req = phys(core::ptr::addr_of!(p.req) as usize);
        let p_data = phys(core::ptr::addr_of!(p.data) as usize);
        let p_stat = phys(core::ptr::addr_of!(p.status) as usize);

        p.desc[0] = VirtqDesc {
            addr: p_req,
            len: 16,
            flags: VIRTQ_DESC_F_NEXT,
            next: 1,
        };
        p.desc[1] = VirtqDesc {
            addr: p_data,
            len: ss as u32,
            flags: VIRTQ_DESC_F_NEXT | VIRTQ_DESC_F_WRITE,
            next: 2,
        };
        p.desc[2] = VirtqDesc {
            addr: p_stat,
            len: 1,
            flags: VIRTQ_DESC_F_WRITE,
            next: 0,
        };

        let ai = core::ptr::read_volatile(core::ptr::addr_of!(p.avail.idx));
        let slot = (ai as usize) & qmask;
        let ring = core::ptr::addr_of_mut!(p.avail.ring).cast::<u16>();
        ring.add(slot).write_volatile(0);
        core::ptr::write_volatile(core::ptr::addr_of_mut!(p.avail.idx), ai.wrapping_add(1));

        mm_w16(common, 22, 0);
        fence(Ordering::SeqCst);
        let qn = mm_r16(common, 30);
        mm_w16(
            notify.wrapping_add(u32::from(qn).wrapping_mul(notify_mul) as usize),
            0,
            0,
        );
        fence(Ordering::SeqCst);

        for _ in 0..2_000_000 {
            let uidx = core::ptr::read_volatile(core::ptr::addr_of!(p.used.idx));
            if uidx != lu {
                lu = uidx;
                break;
            }
            core::hint::spin_loop();
        }
        p.vq_last_seen = lu;

        if p.status != VIRTIO_BLK_S_OK {
            return false;
        }
        out[..ss].copy_from_slice(&p.data[..ss]);
        true
    }

    /// Write one logical sector from `data` (must be `sector_size` bytes).
    pub unsafe fn write_sector(&mut self, sector: u64, data: &[u8]) -> bool {
        let ss = self.sector_size as usize;
        if data.len() < ss || sector >= self.capacity {
            return false;
        }
        let skew = self.phys_skew;
        let common = self.common;
        let notify = self.notify;
        let notify_mul = self.notify_mul;
        let qmask = self.qmask;
        let phys = |virt: usize| -> u64 {
            match skew {
                Some(off) => (virt as u64).wrapping_sub(off),
                None => virt as u64,
            }
        };

        let p = &mut *self.pages;
        let mut lu = p.vq_last_seen;
        Self::fill_req_header(&mut p.req, VIRTIO_BLK_T_OUT, sector);
        p.data[..ss].copy_from_slice(&data[..ss]);
        p.status = 0xFF;

        let p_req = phys(core::ptr::addr_of!(p.req) as usize);
        let p_data = phys(core::ptr::addr_of!(p.data) as usize);
        let p_stat = phys(core::ptr::addr_of!(p.status) as usize);

        p.desc[0] = VirtqDesc {
            addr: p_req,
            len: 16,
            flags: VIRTQ_DESC_F_NEXT,
            next: 1,
        };
        p.desc[1] = VirtqDesc {
            addr: p_data,
            len: ss as u32,
            flags: VIRTQ_DESC_F_NEXT,
            next: 2,
        };
        p.desc[2] = VirtqDesc {
            addr: p_stat,
            len: 1,
            flags: VIRTQ_DESC_F_WRITE,
            next: 0,
        };

        let ai = core::ptr::read_volatile(core::ptr::addr_of!(p.avail.idx));
        let slot = (ai as usize) & qmask;
        let ring = core::ptr::addr_of_mut!(p.avail.ring).cast::<u16>();
        ring.add(slot).write_volatile(0);
        core::ptr::write_volatile(core::ptr::addr_of_mut!(p.avail.idx), ai.wrapping_add(1));

        mm_w16(common, 22, 0);
        fence(Ordering::SeqCst);
        let qn = mm_r16(common, 30);
        mm_w16(
            notify.wrapping_add(u32::from(qn).wrapping_mul(notify_mul) as usize),
            0,
            0,
        );
        fence(Ordering::SeqCst);

        for _ in 0..2_000_000 {
            let uidx = core::ptr::read_volatile(core::ptr::addr_of!(p.used.idx));
            if uidx != lu {
                lu = uidx;
                break;
            }
            core::hint::spin_loop();
        }
        p.vq_last_seen = lu;

        p.status == VIRTIO_BLK_S_OK
    }
}

/// Enumerate VirtIO block PCI functions (up to `out.len()` devices).
pub unsafe fn enumerate(out: &mut [(u8, u8, u8)]) -> usize {
    pci::find_device_any_fn(VIRTIO_VENDOR, VIRTIO_BLK_DEVICE, out)
}
