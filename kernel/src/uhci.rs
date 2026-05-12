// SPDX-License-Identifier: MIT OR Apache-2.0
//
//! UHCI + USB: HID boot keyboard, up to **12** HID boot mice (multi-pointer), and **USB hub**
//! enumeration (QEMU: chained `usb-hub` max 8 ports each — e.g. 7 mice + hub + 5 mice on root port 1).

use crate::pci;
use crate::ports::{inw, outw};
use crate::usb_common::{config_has_hub_interface, find_hid_boot_eps};

pub const MAX_USB_MICE: usize = 12;

const USBCMD: u16 = 0x00;
const USBSTS: u16 = 0x02;
const USBINTR: u16 = 0x04;
const FRNUM: u16 = 0x06;
const FLBASEADD: u16 = 0x08;
const PORTSC0: u16 = 0x10;

const TD_CTRL_ACTIVE: u32 = 1 << 23;
const TD_CTRL_IOC: u32 = 1 << 24;
const TD_CTRL_C_ERR_SHIFT: u32 = 27;

const PID_SETUP: u8 = 0x2D;
const PID_IN: u8 = 0x69;
const PID_OUT: u8 = 0xE1;

// Hub class feature selectors (USB 2.0 hub)
const HUB_PORT_RESET: u16 = 4;
const HUB_C_PORT_RESET: u16 = 20;

#[repr(C, align(4096))]
struct FrameList {
    entries: [u32; 1024],
}

#[repr(C, align(16))]
struct Td {
    link: u32,
    ctrl: u32,
    token: u32,
    buffer: u32,
}

static mut FRAME_LIST: FrameList = FrameList {
    entries: [1u32; 1024],
};
static mut TD_SETUP: Td = Td {
    link: 0,
    ctrl: 0,
    token: 0,
    buffer: 0,
};
static mut TD_DATA: Td = Td {
    link: 0,
    ctrl: 0,
    token: 0,
    buffer: 0,
};
static mut TD_STAT: Td = Td {
    link: 0,
    ctrl: 0,
    token: 0,
    buffer: 0,
};
static mut TD_IRQ: Td = Td {
    link: 0,
    ctrl: 0,
    token: 0,
    buffer: 0,
};
static mut BUF_SETUP: [u8; 8] = [0; 8];
const BUF_DATA_LEN: usize = 512;
static mut BUF_DATA: [u8; BUF_DATA_LEN] = [0; BUF_DATA_LEN];
static mut BUF_IRQ_MOUSE: [u8; 8] = [0; 8];
static mut BUF_IRQ_KBD: [u8; 8] = [0; 8];
static mut BUF_PORT: [u8; 4] = [0; 4];

static mut PHYS_SKEW: u64 = 0;
static mut IOBASE: u16 = 0;

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

/// True after at least one successful keyboard interrupt IN (boot report). PS/2 is only
/// ignored once this is set, so a bogus enumeration that cannot transfer still leaves i8042 usable.
static mut HID_KBD_XFER_OK: bool = false;
/// Same for HID mice: UTM/TCG often enumerates `usb-mouse` but UHCI IRQ IN can fail forever.
static mut HID_MOUSE_XFER_OK: bool = false;

/// Consecutive failed USB keyboard INs after a success; above threshold we stop suppressing PS/2.
static mut KBD_USB_FAILS: u16 = 0;
/// Same for USB mouse vs PS/2 mouse.
static mut MOUSE_USB_FAILS: u16 = 0;
const USB_STALL_BEFORE_PS2: u16 = 96;

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

fn td_token(pid: u8, addr: u8, endpoint: u8, toggle: bool, len: usize) -> u32 {
    let mut t = u32::from(pid);
    t |= u32::from(addr & 0x7F) << 8;
    t |= u32::from(endpoint & 0x0F) << 15;
    if toggle {
        t |= 1 << 19;
    }
    let explen = if len == 0 {
        0x7FFu32
    } else {
        ((len - 1) & 0x7FF) as u32
    };
    t |= explen << 21;
    t
}

fn td_ctrl_active_ioc() -> u32 {
    TD_CTRL_ACTIVE | TD_CTRL_IOC | (3 << TD_CTRL_C_ERR_SHIFT)
}

fn td_ctrl_active() -> u32 {
    TD_CTRL_ACTIVE | (3 << TD_CTRL_C_ERR_SHIFT)
}

fn link_to_td_next(phys: u32) -> u32 {
    (phys & 0xFFFF_FFF0) | 4
}

fn uhci_readw(io: u16, reg: u16) -> u16 {
    unsafe { inw(io.wrapping_add(reg)) }
}

fn uhci_writew(io: u16, reg: u16, v: u16) {
    unsafe { outw(io.wrapping_add(reg), v) }
}

fn hc_stop(io: u16) {
    let mut cmd = uhci_readw(io, USBCMD);
    cmd &= !1;
    uhci_writew(io, USBCMD, cmd);
    for _ in 0..5000 {
        let sts = uhci_readw(io, USBSTS);
        if sts & 0x20 != 0 {
            break;
        }
        pause();
    }
}

fn hc_start(io: u16) {
    let mut cmd = uhci_readw(io, USBCMD);
    cmd |= 1;
    uhci_writew(io, USBCMD, cmd);
}

fn wait_td_done(td: *const Td) -> Result<u32, ()> {
    for _ in 0..1_200_000 {
        unsafe {
            let c = core::ptr::read_volatile(core::ptr::addr_of!((*td).ctrl));
            if c & TD_CTRL_ACTIVE == 0 {
                if c & (1 << 22) != 0 {
                    return Err(());
                }
                return Ok(c);
            }
        }
        pause();
    }
    Err(())
}

unsafe fn run_chain(io: u16, first_td_phys: u32) -> Result<(), ()> {
    let fl_phys = virt_to_phys(core::ptr::addr_of!(FRAME_LIST) as usize);
    let _ = fl_phys;
    hc_stop(io);
    FRAME_LIST.entries[0] = link_to_td_next(first_td_phys);
    uhci_writew(io, FRNUM, 0);
    uhci_writew(io, USBSTS, 0xFFFF);
    hc_start(io);
    Ok(())
}

unsafe fn wait_chain_last(last: *const Td, io: u16) -> Result<u32, ()> {
    let r = wait_td_done(last);
    hc_stop(io);
    FRAME_LIST.entries[0] = 1;
    hc_start(io);
    r
}

unsafe fn control_transfer(
    io: u16,
    addr: u8,
    setup: [u8; 8],
    data_in: Option<&mut [u8]>,
) -> Result<usize, ()> {
    BUF_SETUP = setup;
    let setup_phys = virt_to_phys(core::ptr::addr_of!(BUF_SETUP) as usize);

    if let Some(buf) = data_in {
        let data_phys = virt_to_phys(buf.as_mut_ptr() as usize);
        let n = buf.len();

        TD_SETUP.link = link_to_td_next(virt_to_phys(core::ptr::addr_of!(TD_DATA) as usize));
        TD_SETUP.ctrl = td_ctrl_active();
        TD_SETUP.token = td_token(PID_SETUP, addr, 0, false, 8);
        TD_SETUP.buffer = setup_phys;

        TD_DATA.link = link_to_td_next(virt_to_phys(core::ptr::addr_of!(TD_STAT) as usize));
        TD_DATA.ctrl = td_ctrl_active();
        TD_DATA.token = td_token(PID_IN, addr, 0, true, n);
        TD_DATA.buffer = data_phys;

        TD_STAT.link = 1;
        TD_STAT.ctrl = td_ctrl_active_ioc();
        TD_STAT.token = td_token(PID_OUT, addr, 0, false, 0);
        TD_STAT.buffer = 0;

        let first = virt_to_phys(core::ptr::addr_of!(TD_SETUP) as usize);
        run_chain(io, first)?;
        wait_chain_last(core::ptr::addr_of!(TD_STAT), io)?;

        let ctrl_d = core::ptr::read_volatile(core::ptr::addr_of!(TD_DATA.ctrl));
        if ctrl_d & TD_CTRL_ACTIVE != 0 || (ctrl_d & (1 << 22)) != 0 {
            return Err(());
        }
        let actual = (((ctrl_d & 0x7FF) as usize).wrapping_add(1)) & 0x7FF;
        Ok(actual.min(n))
    } else {
        TD_SETUP.link = link_to_td_next(virt_to_phys(core::ptr::addr_of!(TD_STAT) as usize));
        TD_SETUP.ctrl = td_ctrl_active();
        TD_SETUP.token = td_token(PID_SETUP, addr, 0, false, 8);
        TD_SETUP.buffer = setup_phys;

        TD_STAT.link = 1;
        TD_STAT.ctrl = td_ctrl_active_ioc();
        TD_STAT.token = td_token(PID_IN, addr, 0, true, 0);
        TD_STAT.buffer = 0;

        let first = virt_to_phys(core::ptr::addr_of!(TD_SETUP) as usize);
        run_chain(io, first)?;
        wait_chain_last(core::ptr::addr_of!(TD_STAT), io)?;
        Ok(0)
    }
}

unsafe fn interrupt_in(
    io: u16,
    addr: u8,
    ep: u8,
    mps: u16,
    buf: &mut [u8],
    toggle: &mut bool,
) -> Result<usize, ()> {
    let ep_num = ep & 0x0F;
    let phys = virt_to_phys(buf.as_mut_ptr() as usize);
    let len = buf.len().min(usize::from(mps)).max(1);
    let t = *toggle;
    TD_IRQ.link = 1;
    TD_IRQ.ctrl = td_ctrl_active_ioc();
    TD_IRQ.token = td_token(PID_IN, addr, ep_num, t, len);
    TD_IRQ.buffer = phys;
    *toggle = !*toggle;

    let first = virt_to_phys(core::ptr::addr_of!(TD_IRQ) as usize);
    run_chain(io, first)?;
    let ctrl = wait_chain_last(core::ptr::addr_of!(TD_IRQ), io)?;
    if ctrl & (1 << 22) != 0 {
        *toggle = !*toggle;
        return Err(());
    }
    let actual = (((ctrl & 0x7FF) as usize).wrapping_add(1)) & 0x7FF;
    Ok(actual.min(len))
}

fn port_connected(io: u16, portsc: u16) -> bool {
    uhci_readw(io, portsc) & 1 != 0
}

unsafe fn port_reset_enable(io: u16, portsc: u16) -> Result<(), ()> {
    let mut sc = uhci_readw(io, portsc);
    if sc & 1 == 0 {
        return Err(());
    }
    sc |= 0x0200;
    uhci_writew(io, portsc, sc);
    for _ in 0..500_000 {
        pause();
    }
    sc = uhci_readw(io, portsc);
    sc &= !0x0200u16;
    uhci_writew(io, portsc, sc);
    for _ in 0..200_000 {
        pause();
    }
    sc = uhci_readw(io, portsc);
    if sc & 1 == 0 {
        return Err(());
    }
    sc |= 0x0004;
    uhci_writew(io, portsc, sc);
    for _ in 0..100_000 {
        pause();
    }
    Ok(())
}

unsafe fn hub_get_port_status(io: u16, hub: u8, port: u8) -> Result<u32, ()> {
    let setup = [0xA3, 0, 0, 0, port, 0, 4, 0];
    let n = control_transfer(io, hub, setup, Some(&mut BUF_PORT[..]))?;
    if n < 4 {
        return Err(());
    }
    Ok(u32::from_le_bytes([
        BUF_PORT[0], BUF_PORT[1], BUF_PORT[2], BUF_PORT[3],
    ]))
}

unsafe fn hub_set_port_feature(io: u16, hub: u8, port: u8, feature: u16) -> Result<(), ()> {
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
    control_transfer(io, hub, setup, None)?;
    Ok(())
}

unsafe fn hub_clear_port_feature(io: u16, hub: u8, port: u8, feature: u16) -> Result<(), ()> {
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
    control_transfer(io, hub, setup, None)?;
    Ok(())
}

unsafe fn hub_wait_port_reset(io: u16, hub: u8, port: u8) -> Result<(), ()> {
    for _ in 0..500_000 {
        let st = hub_get_port_status(io, hub, port)?;
        let change = (st >> 16) as u16;
        if change & (1 << 4) != 0 {
            let _ = hub_clear_port_feature(io, hub, port, HUB_C_PORT_RESET);
            for _ in 0..200_000 {
                pause();
            }
            return Ok(());
        }
        pause();
    }
    Err(())
}

unsafe fn hub_read_nports(io: u16, hub_addr: u8) -> Result<u8, ()> {
    let setup = [0xA0, 0x06, 0, 0x29, 0, 0, 9, 0];
    let mut d = [0u8; 9];
    let n = control_transfer(io, hub_addr, setup, Some(&mut d))?;
    if n < 3 {
        return Err(());
    }
    Ok(d[2])
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

#[derive(Clone, Copy)]
enum HidKind {
    Keyboard,
    Mouse,
}

unsafe fn configure_hid_boot(
    io: u16,
    addr: u8,
    iface: u8,
    ep_addr: u8,
    mps: u16,
    kind: HidKind,
) -> Result<(), ()> {
    let cfg_val = BUF_DATA[5];
    let set_cfg = [0x00, 0x09, cfg_val, 0, 0, 0, 0, 0];
    control_transfer(io, addr, set_cfg, None)?;
    let set_proto = [0x21, 0x0B, 0, 0, iface, 0, 0, 0];
    control_transfer(io, addr, set_proto, None)?;
    match kind {
        HidKind::Mouse => {
            add_mouse_slot(addr, ep_addr, mps.clamp(4, 64));
        }
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

unsafe fn load_config_tuple(
    io: u16,
    addr: u8,
) -> Result<(bool, Option<(HidKind, u8, u8, u16)>), ()> {
    let get_dev = [0x80, 0x06, 0, 0x01, 0, 0, 18, 0];
    let n = control_transfer(io, addr, get_dev, Some(&mut BUF_DATA[..18]))?;
    if n < 18 {
        return Err(());
    }
    let dev_class = BUF_DATA[4];

    let get_cfg9 = [0x80, 0x06, 0, 0x02, 0, 0, 9, 0];
    let n9 = control_transfer(io, addr, get_cfg9, Some(&mut BUF_DATA[..9]))?;
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
    let got = control_transfer(io, addr, get_cfg, Some(&mut BUF_DATA[..total]))?;
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

unsafe fn enumerate_device_at_zero(io: u16, assign_addr: u8, next_free: &mut u8) -> Result<(), ()> {
    if assign_addr == 0 || assign_addr > 127 {
        return Err(());
    }

    let set_addr = [0x00, 0x05, assign_addr, 0, 0, 0, 0, 0];
    control_transfer(io, 0, set_addr, None)?;
    for _ in 0..50_000 {
        pause();
    }

    let addr = assign_addr;
    let (is_hub, hid) = load_config_tuple(io, addr)?;

    if is_hub {
        let cfg_val = BUF_DATA[5];
        let set_cfg = [0x00, 0x09, cfg_val, 0, 0, 0, 0, 0];
        control_transfer(io, addr, set_cfg, None)?;
        for _ in 0..100_000 {
            pause();
        }

        let nports = hub_read_nports(io, addr)?;
        if nports == 0 || nports > 16 {
            return Ok(());
        }

        for port in 1..=nports {
            let st = hub_get_port_status(io, addr, port).unwrap_or(0);
            if st & 1 == 0 {
                continue;
            }
            let _ = hub_set_port_feature(io, addr, port, HUB_PORT_RESET);
            if hub_wait_port_reset(io, addr, port).is_err() {
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
            let _ = enumerate_device_at_zero(io, child, next_free);
        }
        return Ok(());
    }

    if let Some((kind, iface, ep, mps)) = hid {
        configure_hid_boot(io, addr, iface, ep, mps, kind)?;
    }
    Ok(())
}

/// Call after PCI scan. `skew` is `BootInfo::physical_memory_offset` as `u64`.
pub unsafe fn init(skew: u64) -> bool {
    PHYS_SKEW = skew;
    MOUSE_COUNT = 0;
    let mice = core::ptr::addr_of_mut!(MICE).cast::<MouseSlot>();
    for i in 0..MAX_USB_MICE {
        unsafe {
            mice.add(i).write(MouseSlot {
                addr: 0,
                ep: 0,
                mps: 0,
                toggle: false,
                present: false,
            });
        }
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

    let Some((bus, slot, func, io)) = pci::find_usb_uhci_io() else {
        return false;
    };
    pci::pci_enable_io_bm(bus, slot, func);
    IOBASE = io;

    let fl = core::ptr::addr_of_mut!(FRAME_LIST.entries).cast::<u32>();
    for i in 0..1024 {
        unsafe {
            fl.add(i).write(1);
        }
    }

    uhci_writew(io, USBCMD, 1 << 1);
    for _ in 0..2000 {
        pause();
    }
    uhci_writew(io, USBCMD, 0);

    let fl_phys = virt_to_phys(core::ptr::addr_of!(FRAME_LIST) as usize);
    uhci_writew(io, USBINTR, 0);
    outw(io.wrapping_add(FLBASEADD), fl_phys as u16);
    outw(io.wrapping_add(FLBASEADD + 2), (fl_phys >> 16) as u16);
    uhci_writew(io, FRNUM, 0);
    uhci_writew(io, USBSTS, 0xFFFF);

    let mut cmd = uhci_readw(io, USBCMD);
    cmd |= 1;
    uhci_writew(io, USBCMD, cmd);

    let mut next_free: u8 = 1;
    for port_idx in 0..2u16 {
        let portsc = PORTSC0.wrapping_add(port_idx.wrapping_mul(2));
        if !port_connected(io, portsc) {
            continue;
        }
        if next_free > 127 {
            break;
        }
        if port_reset_enable(io, portsc).is_err() {
            continue;
        }
        let assign = next_free;
        next_free = next_free.saturating_add(1);
        let _ = enumerate_device_at_zero(io, assign, &mut next_free);
    }

    if MOUSE_COUNT > 0 || KBD_READY {
        return true;
    }

    hc_stop(io);
    IOBASE = 0;
    false
}

/// Poll USB HID mouse slot `idx` (0 .. [`usb_mouse_count()`]).
pub unsafe fn poll_mouse_slot(idx: usize) -> Option<(u8, i16, i16)> {
    if IOBASE == 0 || idx >= MOUSE_COUNT {
        return None;
    }
    let slot = &mut MICE[idx];
    if !slot.present {
        return None;
    }
    let io = IOBASE;
    let buf = &mut BUF_IRQ_MOUSE[..];
    buf.fill(0);
    let mut toggle = slot.toggle;
    let n = match interrupt_in(io, slot.addr, slot.ep, slot.mps, buf, &mut toggle) {
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
    // Match PS/2 cursor Y convention in `ps2.rs` (`dy: -raw`).
    let dy = -(buf[2] as i8 as i16);
    Some((buttons, dx, dy))
}

pub unsafe fn poll_keyboard_report() -> Option<[u8; 8]> {
    if !KBD_READY || IOBASE == 0 {
        return None;
    }
    let io = IOBASE;
    let buf = &mut BUF_IRQ_KBD[..];
    buf.fill(0);
    let n = match interrupt_in(
        io,
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

/// True only after at least one good HID boot IN and while stalls have not exceeded the PS/2
/// fallback threshold — same idea as [`hid_kbd_suppresses_ps2`], so a ghost-enumerated mouse does
/// not reserve cursor slot 0 and strand PS/2 on a secondary slot (see main loop `state.mx`).
pub fn mouse_ready() -> bool {
    unsafe {
        MOUSE_COUNT > 0
            && HID_MOUSE_XFER_OK
            && MOUSE_USB_FAILS < USB_STALL_BEFORE_PS2
    }
}

pub fn usb_mouse_count() -> usize {
    unsafe { MOUSE_COUNT }
}

pub fn keyboard_ready() -> bool {
    unsafe { KBD_READY }
}

/// PS/2 keyboard may be skipped only when USB HID IN is healthy (not stalled).
pub fn hid_kbd_suppresses_ps2() -> bool {
    unsafe {
        KBD_READY
            && HID_KBD_XFER_OK
            && KBD_USB_FAILS < USB_STALL_BEFORE_PS2
    }
}

