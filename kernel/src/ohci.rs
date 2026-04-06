// SPDX-License-Identifier: MIT OR Apache-2.0
//
//! OHCI 1.1 (MMIO): HID boot keyboard + mice on root ports and hubs.
//! Tested layout targets QEMU `usb-ohci-pci`; real hardware may need tuning.

use crate::pci::USB_PI_OHCI;
use crate::usb_common::{config_has_hub_interface, find_hid_boot_eps};

pub const MAX_USB_MICE: usize = 12;

const BUF_DATA_LEN: usize = 512;
static mut BUF_DATA: [u8; BUF_DATA_LEN] = [0u8; BUF_DATA_LEN];
static mut BUF_SETUP: [u8; 8] = [0u8; 8];
static mut BUF_IRQ_MOUSE: [u8; 8] = [0u8; 8];
static mut BUF_IRQ_KBD: [u8; 8] = [0u8; 8];
static mut BUF_PORT: [u8; 4] = [0u8; 4];

static mut PHYS_SKEW: u64 = 0;
static mut MMIO: usize = 0;

#[derive(Clone, Copy)]
struct MouseSlot {
    addr: u8,
    ep: u8,
    mps: u16,
    toggle: bool,
    present: bool,
}

static mut MICE: [MouseSlot; MAX_USB_MICE] = [MouseSlot {
    addr: 0,
    ep: 0,
    mps: 0,
    toggle: false,
    present: false,
}; MAX_USB_MICE];
static mut MOUSE_COUNT: usize = 0;

static mut KBD_ADDR: u8 = 0;
static mut KBD_EP: u8 = 0;
static mut KBD_MPS: u16 = 0;
static mut KBD_TOGGLE: bool = false;
static mut KBD_READY: bool = false;

static mut HID_KBD_XFER_OK: bool = false;
static mut HID_MOUSE_XFER_OK: bool = false;
static mut KBD_USB_FAILS: u16 = 0;
static mut MOUSE_USB_FAILS: u16 = 0;
const USB_STALL_BEFORE_PS2: u16 = 96;

const HUB_PORT_RESET: u16 = 4;
const HUB_C_PORT_RESET: u16 = 20;

#[repr(C, align(256))]
struct Hcca {
    int_table: [u32; 32],
    frame: u16,
    pad_82: u16,
    done_head: u32,
}

#[repr(C, align(16))]
#[derive(Clone, Copy)]
struct Ed {
    dw0: u32,
    tail: u32,
    head: u32,
    next: u32,
}

#[repr(C, align(16))]
#[derive(Clone, Copy)]
struct Td {
    dw0: u32,
    cbp: u32,
    next: u32,
    be: u32,
}

static mut HCCA: Hcca = Hcca {
    int_table: [0; 32],
    frame: 0,
    pad_82: 0,
    done_head: 0,
};

static mut ED_CTL: Ed = Ed {
    dw0: 0,
    tail: 0,
    head: 0,
    next: 0,
};
static mut ED_INT: Ed = Ed {
    dw0: 0,
    tail: 0,
    head: 0,
    next: 0,
};
static mut TD: [Td; 6] = [Td {
    dw0: 0,
    cbp: 0,
    next: 0,
    be: 0,
}; 6];

#[inline]
fn virt_to_phys(va: usize) -> u32 {
    (va as u64).wrapping_add(unsafe { PHYS_SKEW }) as u32
}

#[inline]
fn pause() {
    unsafe {
        core::arch::asm!("pause", options(nomem, nostack, preserves_flags));
    }
}

unsafe fn r32(off: usize) -> u32 {
    core::ptr::read_volatile((MMIO + off) as *const u32)
}

unsafe fn w32(off: usize, v: u32) {
    core::ptr::write_volatile((MMIO + off) as *mut u32, v);
}

const HC_CONTROL: usize = 0x04;
const HC_CMDSTATUS: usize = 0x08;
const HC_INTSTATUS: usize = 0x0C;
const HC_INTENABLE: usize = 0x10;
const HC_HCCA: usize = 0x14;
const HC_CTRL_HEAD: usize = 0x1C;
const HC_CTRL_CUR: usize = 0x20;
const HC_FM_INTERVAL: usize = 0x30;
const HC_PERIODIC_START: usize = 0x3C;
const HC_RH_DESC_A: usize = 0x44;
const HC_RH_STATUS: usize = 0x4C;
const HC_RH_PORT_BASE: usize = 0x50;

const TD_CC: u32 = 0xF << 28;
const TD_EC: u32 = 3 << 26;
const TD_DI: u32 = 7 << 21;
const TD_R: u32 = 1 << 18;
const TD_DP_SETUP: u32 = 0;
const TD_DP_OUT: u32 = 1 << 18;
const TD_DP_IN: u32 = 2 << 18;
const TD_T_DATA0: u32 = 2 << 24;
const TD_T_DATA1: u32 = 3 << 24;

const CTRL_CLE: u32 = 1 << 4;
const CTRL_PLE: u32 = 1 << 2;
const CTRL_HCFS: u32 = 3 << 0;
const USB_OPER: u32 = 2 << 0;

fn td_info(dp: u32, t: u32, buf_lo: u32, buf_hi: u32, next_phys: u32) -> Td {
    Td {
        dw0: TD_CC | TD_EC | TD_DI | TD_R | dp | t,
        cbp: buf_lo,
        next: next_phys,
        be: buf_hi,
    }
}

unsafe fn wait_td(td: *const Td) -> Result<(), ()> {
    for _ in 0..4_000_000 {
        let v = core::ptr::read_volatile(core::ptr::addr_of!((*td).dw0));
        let cc = (v >> 28) & 0xF;
        if cc != 0xF {
            return if cc == 0 { Ok(()) } else { Err(()) };
        }
        pause();
    }
    Err(())
}

unsafe fn hc_stop_sched() {
    let mut c = r32(HC_CONTROL);
    c &= !(CTRL_CLE | CTRL_PLE | (1 << 5));
    w32(HC_CONTROL, c);
    for _ in 0..200_000 {
        pause();
    }
}

unsafe fn kick_control() {
    w32(HC_CMDSTATUS, 1 << 1);
}

unsafe fn control_transfer(addr: u8, setup: [u8; 8], mut data_in: Option<&mut [u8]>) -> Result<usize, ()> {
    hc_stop_sched();
    w32(HC_CTRL_HEAD, 0);
    w32(HC_CTRL_CUR, 0);

    BUF_SETUP = setup;
    let setup_phys = virt_to_phys(core::ptr::addr_of!(BUF_SETUP) as usize);
    let setup_end = setup_phys + 7;

    let td0p = virt_to_phys(core::ptr::addr_of!(TD[0]) as usize);
    let td1p = virt_to_phys(core::ptr::addr_of!(TD[1]) as usize);
    let td2p = virt_to_phys(core::ptr::addr_of!(TD[2]) as usize);

    TD[0] = td_info(TD_DP_SETUP, TD_T_DATA0, setup_phys, setup_end, td1p);

    let tailp;
    let mut data_len = 0usize;
    if let Some(ref mut buf) = data_in {
        let n = buf.len();
        data_len = n;
        let dp = virt_to_phys(buf.as_mut_ptr() as usize);
        let dend = dp + n as u32 - 1;
        TD[1] = td_info(TD_DP_IN, TD_T_DATA1, dp, dend, td2p);
        TD[2] = td_info(TD_DP_OUT, TD_T_DATA1, 0, 0, 0);
        TD[2].next = 0;
        tailp = td2p;
    } else {
        // No data stage: SETUP then STATUS IN (zero length).
        TD[1] = td_info(TD_DP_IN, TD_T_DATA1, 0, 0, 0);
        TD[1].next = 0;
        tailp = td1p;
    }

    let fa = u32::from(addr & 0x7F);
    let ed_dw0 = fa | (0u32 << 7) | (3u32 << 11) | (0u32 << 13) | (0u32 << 15) | (8u32 << 16);
    let edp = virt_to_phys(core::ptr::addr_of!(ED_CTL) as usize);
    ED_CTL.dw0 = ed_dw0;
    ED_CTL.tail = tailp;
    ED_CTL.head = td0p;
    ED_CTL.next = 0;

    w32(HC_CTRL_HEAD, edp);
    w32(HC_CTRL_CUR, 0);
    w32(HC_INTSTATUS, 0xFFFF_FFFF);

    let mut c = r32(HC_CONTROL);
    c = (c & !CTRL_HCFS) | USB_OPER | CTRL_CLE;
    w32(HC_CONTROL, c);
    kick_control();

    if data_len > 0 {
        wait_td(core::ptr::addr_of!(TD[0]))?;
        wait_td(core::ptr::addr_of!(TD[1]))?;
        wait_td(core::ptr::addr_of!(TD[2]))?;
        hc_stop_sched();
        w32(HC_CTRL_HEAD, 0);
        Ok(data_len)
    } else {
        wait_td(core::ptr::addr_of!(TD[0]))?;
        wait_td(core::ptr::addr_of!(TD[1]))?;
        hc_stop_sched();
        w32(HC_CTRL_HEAD, 0);
        Ok(0)
    }
}

unsafe fn interrupt_in(addr: u8, ep: u8, mps: u16, buf: &mut [u8], toggle: &mut bool) -> Result<usize, ()> {
    hc_stop_sched();
    for i in 0..32 {
        HCCA.int_table[i] = 0;
    }

    let n = buf.len().min(usize::from(mps)).max(1);
    let dp = virt_to_phys(buf.as_mut_ptr() as usize);
    let dend = dp + n as u32 - 1;
    let td3p = virt_to_phys(core::ptr::addr_of!(TD[3]) as usize);
    let t = if *toggle { TD_T_DATA1 } else { TD_T_DATA0 };
    *toggle = !*toggle;

    TD[3] = td_info(TD_DP_IN, t, dp, dend, 0);
    TD[3].next = 0;

    let fa = u32::from(addr & 0x7F);
    let epn = u32::from(ep & 0xF);
    let mpsz = u32::from(mps).min(0x7FF);
    let ed_dw0 = fa | (epn << 7) | (2u32 << 11) | (0u32 << 13) | (0u32 << 15) | (mpsz << 16);
    let edp = virt_to_phys(core::ptr::addr_of!(ED_INT) as usize);
    ED_INT.dw0 = ed_dw0;
    ED_INT.tail = td3p;
    ED_INT.head = td3p;
    ED_INT.next = 0;

    for i in 0..32 {
        HCCA.int_table[i] = edp;
    }

    w32(HC_INTSTATUS, 0xFFFF_FFFF);
    let mut c = r32(HC_CONTROL);
    c = (c & !CTRL_HCFS) | USB_OPER | CTRL_PLE;
    w32(HC_CONTROL, c);

    wait_td(core::ptr::addr_of!(TD[3]))?;
    let ctrl = core::ptr::read_volatile(core::ptr::addr_of!(TD[3].dw0));
    let cc = (ctrl >> 28) & 0xF;
    hc_stop_sched();
    for i in 0..32 {
        HCCA.int_table[i] = 0;
    }
    if cc != 0 {
        *toggle = !*toggle;
        return Err(());
    }
    Ok(n)
}

unsafe fn hub_get_port_status(hub: u8, port: u8) -> Result<u32, ()> {
    let setup = [0xA3, 0, 0, 0, port, 0, 4, 0];
    let n = control_transfer(hub, setup, Some(&mut BUF_PORT[..]))?;
    if n < 4 {
        return Err(());
    }
    Ok(u32::from_le_bytes([
        BUF_PORT[0], BUF_PORT[1], BUF_PORT[2], BUF_PORT[3],
    ]))
}

unsafe fn hub_set_port_feature(hub: u8, port: u8, feature: u16) -> Result<(), ()> {
    let setup = [
        0x23,
        0x03,
        (feature & 0xFF) as u8,
        (feature >> 8) as u8,
        port,
        0,
        0,
        0,
    ];
    control_transfer(hub, setup, None)?;
    Ok(())
}

unsafe fn hub_clear_port_feature(hub: u8, port: u8, feature: u16) -> Result<(), ()> {
    let setup = [
        0x23,
        0x01,
        (feature & 0xFF) as u8,
        (feature >> 8) as u8,
        port,
        0,
        0,
        0,
    ];
    control_transfer(hub, setup, None)?;
    Ok(())
}

unsafe fn hub_wait_port_reset(hub: u8, port: u8) -> Result<(), ()> {
    for _ in 0..500_000 {
        let st = hub_get_port_status(hub, port)?;
        let change = (st >> 16) as u16;
        if change & (1 << 4) != 0 {
            let _ = hub_clear_port_feature(hub, port, HUB_C_PORT_RESET);
            for _ in 0..200_000 {
                pause();
            }
            return Ok(());
        }
        pause();
    }
    Err(())
}

unsafe fn hub_read_nports(hub_addr: u8) -> Result<u8, ()> {
    let setup = [0xA0, 0x06, 0, 0x29, 0, 0, 9, 0];
    let mut d = [0u8; 9];
    let n = control_transfer(hub_addr, setup, Some(&mut d))?;
    if n < 3 {
        return Err(());
    }
    Ok(d[2])
}

#[derive(Clone, Copy)]
enum HidKind {
    Keyboard,
    Mouse,
}

unsafe fn add_mouse_slot(addr: u8, ep: u8, mps: u16) {
    let i = MOUSE_COUNT;
    if i >= MAX_USB_MICE {
        return;
    }
    MICE[i] = MouseSlot {
        addr,
        ep,
        mps,
        toggle: false,
        present: true,
    };
    MOUSE_COUNT = i + 1;
}

unsafe fn configure_hid_boot(
    addr: u8,
    iface: u8,
    ep_addr: u8,
    mps: u16,
    kind: HidKind,
) -> Result<(), ()> {
    let cfg_val = BUF_DATA[5];
    let set_cfg = [0x00, 0x09, cfg_val, 0, 0, 0, 0, 0];
    control_transfer(addr, set_cfg, None)?;
    let set_proto = [0x21, 0x0B, 0, 0, iface, 0, 0, 0];
    control_transfer(addr, set_proto, None)?;
    match kind {
        HidKind::Mouse => add_mouse_slot(addr, ep_addr, mps.clamp(4, 64)),
        HidKind::Keyboard => {
            if !KBD_READY {
                KBD_ADDR = addr;
                KBD_EP = ep_addr;
                KBD_MPS = mps.max(8);
                KBD_TOGGLE = false;
                KBD_READY = true;
            }
        }
    }
    Ok(())
}

unsafe fn load_config_tuple(addr: u8) -> Result<(bool, Option<(HidKind, u8, u8, u16)>), ()> {
    let get_dev = [0x80, 0x06, 0, 0x01, 0, 0, 18, 0];
    let n = control_transfer(addr, get_dev, Some(&mut BUF_DATA[..18]))?;
    if n < 18 {
        return Err(());
    }
    let dev_class = BUF_DATA[4];

    let get_cfg9 = [0x80, 0x06, 0, 0x02, 0, 0, 9, 0];
    let n9 = control_transfer(addr, get_cfg9, Some(&mut BUF_DATA[..9]))?;
    if n9 < 9 {
        return Err(());
    }
    let total = u16::from_le_bytes([BUF_DATA[2], BUF_DATA[3]]) as usize;
    let total = total.clamp(9, BUF_DATA_LEN);
    let get_cfg = [
        0x80,
        0x06,
        0,
        0x02,
        0,
        0,
        (total & 0xFF) as u8,
        ((total >> 8) & 0xFF) as u8,
    ];
    let got = control_transfer(addr, get_cfg, Some(&mut BUF_DATA[..total]))?;
    if got < 9 {
        return Err(());
    }
    let cfg = &BUF_DATA[..got];

    let is_hub = dev_class == 9 || config_has_hub_interface(cfg);

    let hid = if let Some((iface, ep, mps)) = find_hid_boot_eps(cfg, 1) {
        Some((HidKind::Keyboard, iface, ep, mps))
    } else if let Some((iface, ep, mps)) = find_hid_boot_eps(cfg, 2) {
        Some((HidKind::Mouse, iface, ep, mps))
    } else {
        None
    };

    Ok((is_hub, hid))
}

unsafe fn enumerate_device_at_zero(assign_addr: u8, next_free: &mut u8) -> Result<(), ()> {
    if assign_addr == 0 || assign_addr > 127 {
        return Err(());
    }
    control_transfer(0, [0x00, 0x05, assign_addr, 0, 0, 0, 0, 0], None)?;
    for _ in 0..50_000 {
        pause();
    }

    let addr = assign_addr;
    let (is_hub, hid) = load_config_tuple(addr)?;

    if is_hub {
        let cfg_val = BUF_DATA[5];
        control_transfer(addr, [0x00, 0x09, cfg_val, 0, 0, 0, 0, 0], None)?;
        for _ in 0..100_000 {
            pause();
        }

        let nports = hub_read_nports(addr)?;
        if nports == 0 || nports > 16 {
            return Ok(());
        }

        for port in 1..=nports {
            let st = hub_get_port_status(addr, port).unwrap_or(0);
            if st & 1 == 0 {
                continue;
            }
            let _ = hub_set_port_feature(addr, port, HUB_PORT_RESET);
            if hub_wait_port_reset(addr, port).is_err() {
                continue;
            }
            for _ in 0..300_000 {
                pause();
            }
            let child = *next_free;
            if child > 127 {
                break;
            }
            *next_free = next_free.saturating_add(1);
            let _ = enumerate_device_at_zero(child, next_free);
        }
        return Ok(());
    }

    if let Some((kind, iface, ep, mps)) = hid {
        configure_hid_boot(addr, iface, ep, mps, kind)?;
    }
    Ok(())
}

unsafe fn root_port_reset(port: u8) -> Result<(), ()> {
    let off = HC_RH_PORT_BASE + port as usize * 4;
    w32(off, 1 << (16 + 4));
    for _ in 0..500_000 {
        pause();
    }
    w32(off, 1 << (16 + 1));
    for _ in 0..200_000 {
        pause();
    }
    let st = r32(off);
    if st & 1 == 0 {
        return Err(());
    }
    Ok(())
}

unsafe fn root_port_connected(port: u8) -> bool {
    let off = HC_RH_PORT_BASE + port as usize * 4;
    r32(off) & 1 != 0
}

pub unsafe fn init(skew: u64) -> bool {
    PHYS_SKEW = skew;
    MOUSE_COUNT = 0;
    let mice = core::ptr::addr_of_mut!(MICE).cast::<MouseSlot>();
    for i in 0..MAX_USB_MICE {
        mice.add(i).write(MouseSlot {
            addr: 0,
            ep: 0,
            mps: 0,
            toggle: false,
            present: false,
        });
    }
    KBD_READY = false;
    KBD_ADDR = 0;
    KBD_EP = 0;
    KBD_MPS = 0;
    KBD_TOGGLE = false;
    HID_KBD_XFER_OK = false;
    HID_MOUSE_XFER_OK = false;
    KBD_USB_FAILS = 0;
    MOUSE_USB_FAILS = 0;
    MMIO = 0;

    let Some((bus, slot, func, bar)) = crate::pci::find_usb_host_mmio_bar0(USB_PI_OHCI) else {
        return false;
    };
    crate::pci::pci_enable_mmio_bm(bus, slot, func);
    MMIO = (bar as u64).wrapping_add(skew) as usize;

    for i in 0..32 {
        HCCA.int_table[i] = 0;
    }
    HCCA.done_head = 0;
    HCCA.frame = 0;
    HCCA.pad_82 = 0;

    let fm = r32(HC_FM_INTERVAL);
    w32(HC_CONTROL, 0);
    for _ in 0..50_000 {
        pause();
    }
    w32(HC_CMDSTATUS, 1);
    for _ in 0..800_000 {
        pause();
        if r32(HC_CMDSTATUS) & 1 == 0 {
            break;
        }
    }
    w32(HC_FM_INTERVAL, fm);
    let fit = fm & 0x3FFF;
    w32(HC_PERIODIC_START, (fit * 9 / 10).max(0x2A2F));

    let hcca_phys = virt_to_phys(core::ptr::addr_of!(HCCA) as usize);
    w32(HC_HCCA, hcca_phys);
    w32(HC_CTRL_HEAD, 0);
    w32(HC_CTRL_CUR, 0);
    w32(HC_INTSTATUS, 0xFFFF_FFFF);
    w32(HC_INTENABLE, 0);

    let rh_a = r32(HC_RH_DESC_A);
    w32(HC_RH_DESC_A, rh_a | (1 << 9));
    w32(HC_RH_STATUS, 1 << 16);

    let ndp = ((rh_a >> 24) & 0x7F) as u8;
    let nports = if ndp == 0 { 2 } else { ndp.min(15) };

    w32(HC_CONTROL, USB_OPER);

    let mut next_free: u8 = 1;
    for p in 0..nports {
        if !root_port_connected(p) {
            continue;
        }
        if root_port_reset(p).is_err() {
            continue;
        }
        if next_free > 127 {
            break;
        }
        let assign = next_free;
        next_free = next_free.saturating_add(1);
        let _ = enumerate_device_at_zero(assign, &mut next_free);
    }

    if MOUSE_COUNT > 0 || KBD_READY {
        return true;
    }

    w32(HC_CONTROL, 0);
    MMIO = 0;
    false
}

pub unsafe fn poll_mouse_slot(idx: usize) -> Option<(u8, i16, i16)> {
    if MMIO == 0 || idx >= MOUSE_COUNT {
        return None;
    }
    let slot = &mut MICE[idx];
    if !slot.present {
        return None;
    }
    let buf = &mut BUF_IRQ_MOUSE[..];
    buf.fill(0);
    let mut toggle = slot.toggle;
    let n = match interrupt_in(slot.addr, slot.ep, slot.mps, buf, &mut toggle) {
        Ok(n) => n,
        Err(()) => {
            MOUSE_USB_FAILS = MOUSE_USB_FAILS.saturating_add(1);
            return None;
        }
    };
    slot.toggle = toggle;
    if n < 3 {
        MOUSE_USB_FAILS = MOUSE_USB_FAILS.saturating_add(1);
        return None;
    }
    MOUSE_USB_FAILS = 0;
    HID_MOUSE_XFER_OK = true;
    let buttons = buf[0] & 0x07;
    let dx = buf[1] as i8 as i16;
    let dy = -(buf[2] as i8 as i16);
    Some((buttons, dx, dy))
}

pub unsafe fn poll_keyboard_report() -> Option<[u8; 8]> {
    if !KBD_READY || MMIO == 0 {
        return None;
    }
    let buf = &mut BUF_IRQ_KBD[..];
    buf.fill(0);
    let n = match interrupt_in(
        KBD_ADDR,
        KBD_EP,
        KBD_MPS,
        buf,
        &mut *(&raw mut KBD_TOGGLE),
    ) {
        Ok(n) => n,
        Err(()) => {
            KBD_USB_FAILS = KBD_USB_FAILS.saturating_add(1);
            return None;
        }
    };
    if n < 8 {
        KBD_USB_FAILS = KBD_USB_FAILS.saturating_add(1);
        return None;
    }
    KBD_USB_FAILS = 0;
    HID_KBD_XFER_OK = true;
    Some([
        buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7],
    ])
}

pub fn mouse_ready() -> bool {
    unsafe { MOUSE_COUNT > 0 }
}

pub fn usb_mouse_count() -> usize {
    unsafe { MOUSE_COUNT }
}

pub fn keyboard_ready() -> bool {
    unsafe { KBD_READY }
}

pub fn hid_kbd_suppresses_ps2() -> bool {
    unsafe {
        KBD_READY
            && HID_KBD_XFER_OK
            && KBD_USB_FAILS < USB_STALL_BEFORE_PS2
    }
}

