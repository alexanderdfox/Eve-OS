// SPDX-License-Identifier: MIT OR Apache-2.0
//
//! Native xHCI HID (boot keyboard + one boot mouse) for xHCI-only firmware (no OHCI/UHCI companion).
//! Phase 2: command/event rings, root-port reset, EP0 enumeration, interrupt-IN polling.

use crate::diag_log;
use crate::usb_common::find_hid_boot_eps;

const TRB_NORMAL: u32 = 1;
const TRB_SETUP: u32 = 2;
const TRB_DATA: u32 = 3;
const TRB_STATUS: u32 = 4;

const TRB_CMD_COMPLETION: u32 = 33;
const TRB_TRANSFER: u32 = 32;

const TRB_TYPE_SHIFT: u32 = 10;

const TRB_IOC: u32 = 1 << 5;
const TRB_IDT: u32 = 1 << 6;
const TRB_DIR_IN: u32 = 1 << 16;

const USBCMD_HCRST: u32 = 1 << 1;
const USBCMD_RUN: u32 = 1 << 0;
const USBSTS_HCH: u32 = 1 << 0;

const PORTSC_CCS: u32 = 1 << 0;
const PORTSC_PED: u32 = 1 << 1;
const PORTSC_PR: u32 = 1 << 4;
const PORTSC_WRC: u32 = 1 << 17;

const CMD_ENABLE_SLOT: u32 = 9;
const CMD_ADDRESS_DEVICE: u32 = 11;
const CMD_CONFIGURE_EP: u32 = 12;

const RING_CAP: usize = 64;

#[repr(C, align(64))]
#[derive(Clone, Copy)]
struct Trb {
    param: u64,
    status: u32,
    control: u32,
}

#[repr(C, align(64))]
struct ErstEntry {
    ring_base: u64,
    ring_size: u32,
    _rsvd: u32,
}

static mut MMIO: usize = 0;
static mut OP_OFF: u32 = 0;
static mut DB_OFF: u32 = 0;
static mut PHYS_SKEW: u64 = 0;
static mut MAX_SLOTS: u8 = 8;
static mut MAX_PORTS: u8 = 8;

static mut CMD_RING: [Trb; RING_CAP] = [Trb {
    param: 0,
    status: 0,
    control: 0,
}; RING_CAP];
static mut EVT_RING: [Trb; RING_CAP] = [Trb {
    param: 0,
    status: 0,
    control: 0,
}; RING_CAP];
static mut ERST: ErstEntry = ErstEntry {
    ring_base: 0,
    ring_size: 0,
    _rsvd: 0,
};
static mut DCBAAP: [u64; 64] = [0; 64];
static mut DEV_CTX: [u8; 2048] = [0; 2048];
static mut DEV_CTX2: [u8; 2048] = [0; 2048];
static mut INPUT_CTX: [u8; 512] = [0; 512];
static mut SETUP: [u8; 8] = [0; 8];
static mut DATA: [u8; 512] = [0; 512];
static mut IRQ_KBD: [u8; 8] = [0; 8];
static mut IRQ_MOUSE: [u8; 8] = [0; 8];

static mut CMD_ENQ: usize = 0;
static mut CMD_CYCLE: bool = true;
static mut EVT_DEQ: usize = 0;
static mut EVT_CYCLE: bool = true;

static mut EP0_RING: [Trb; RING_CAP] = [Trb {
    param: 0,
    status: 0,
    control: 0,
}; RING_CAP];
static mut EP0_ENQ: usize = 0;
static mut EP0_CYCLE: bool = true;

static mut KBD_RING: [Trb; RING_CAP] = [Trb {
    param: 0,
    status: 0,
    control: 0,
}; RING_CAP];
static mut KBD_ENQ: usize = 0;
static mut KBD_CYCLE: bool = true;

static mut MOUSE_RING: [Trb; RING_CAP] = [Trb {
    param: 0,
    status: 0,
    control: 0,
}; RING_CAP];
static mut MOUSE_ENQ: usize = 0;
static mut MOUSE_CYCLE: bool = true;

static mut KBD_SLOT: u8 = 0;
static mut MOUSE_SLOT: u8 = 0;
static mut KBD_EP: u8 = 0;
static mut MOUSE_EP: u8 = 0;
static mut KBD_MPS: u16 = 8;
static mut MOUSE_MPS: u16 = 8;
static mut KBD_READY: bool = false;
static mut MOUSE_READY: bool = false;
static mut HID_KBD_XFER_OK: bool = false;
static mut HID_MOUSE_XFER_OK: bool = false;
static mut KBD_USB_FAILS: u16 = 0;
static mut MOUSE_USB_FAILS: u16 = 0;
static mut DEV_CTX2_USED: bool = false;
const USB_STALL_BEFORE_PS2: u16 = 96;

#[inline]
fn pause() {
    for _ in 0..128 {
        core::hint::spin_loop();
    }
}

#[inline]
unsafe fn phys(va: usize) -> u64 {
    (va as u64).wrapping_add(PHYS_SKEW)
}

#[inline]
unsafe fn cap_read32(off: u32) -> u32 {
    core::ptr::read_volatile((MMIO + off as usize) as *const u32)
}

#[inline]
unsafe fn op_read32(off: u32) -> u32 {
    core::ptr::read_volatile((MMIO + OP_OFF as usize + off as usize) as *const u32)
}

#[inline]
unsafe fn op_write32(off: u32, v: u32) {
    core::ptr::write_volatile((MMIO + OP_OFF as usize + off as usize) as *mut u32, v);
}

#[inline]
unsafe fn op_write64(off: u32, v: u64) {
    op_write32(off, v as u32);
    op_write32(off + 4, (v >> 32) as u32);
}

#[inline]
unsafe fn doorbell(slot: u32, target: u32) {
    let db = MMIO + DB_OFF as usize + (slot as usize * 4);
    core::ptr::write_volatile(db as *mut u32, target);
}

unsafe fn trb_cycle(c: bool) -> u32 {
    if c {
        1
    } else {
        0
    }
}

unsafe fn push_cmd(trb_type: u32, param: u64, status: u32, extra: u32) -> Result<(), ()> {
    let i = CMD_ENQ;
    if i >= RING_CAP {
        return Err(());
    }
    let ctrl = trb_cycle(CMD_CYCLE)
        | (trb_type << TRB_TYPE_SHIFT)
        | extra
        | TRB_IOC;
    let ring = core::ptr::addr_of_mut!(CMD_RING).cast::<Trb>();
    ring.add(i).write(Trb {
        param,
        status,
        control: ctrl,
    });
    CMD_ENQ += 1;
    if CMD_ENQ >= RING_CAP {
        CMD_ENQ = 0;
        CMD_CYCLE = !CMD_CYCLE;
    }
    doorbell(0, 0);
    Ok(())
}

unsafe fn event_ready() -> bool {
    let ring = core::ptr::addr_of!(EVT_RING).cast::<Trb>();
    let trb = ring.add(EVT_DEQ).read();
    let c = (trb.control & 1) != 0;
    c == EVT_CYCLE
}

unsafe fn pop_event() -> Trb {
    let ring = core::ptr::addr_of!(EVT_RING).cast::<Trb>();
    let trb = ring.add(EVT_DEQ).read();
    EVT_DEQ += 1;
    if EVT_DEQ >= RING_CAP {
        EVT_DEQ = 0;
        EVT_CYCLE = !EVT_CYCLE;
    }
    let erdp = phys(core::ptr::addr_of!(EVT_RING) as usize + EVT_DEQ * 16);
    op_write64(0x38, erdp & !0xF);
    trb
}

unsafe fn wait_event(trb_type: u32, timeout: u32) -> Result<Trb, ()> {
    for _ in 0..timeout {
        if event_ready() {
            let ev = pop_event();
            let ty = (ev.control >> TRB_TYPE_SHIFT) & 0x3F;
            if ty == trb_type {
                return Ok(ev);
            }
        }
        pause();
    }
    Err(())
}

unsafe fn hc_reset() -> Result<(), ()> {
    op_write32(0x00, op_read32(0x00) | USBCMD_HCRST);
    for _ in 0..500_000 {
        if op_read32(0x00) & USBCMD_HCRST == 0 {
            break;
        }
        pause();
    }
    for _ in 0..500_000 {
        if op_read32(0x04) & USBSTS_HCH != 0 {
            return Ok(());
        }
        pause();
    }
    Err(())
}

unsafe fn rings_init() -> Result<(), ()> {
    CMD_ENQ = 0;
    CMD_CYCLE = true;
    EVT_DEQ = 0;
    EVT_CYCLE = true;

    let cmd_phys = phys(core::ptr::addr_of!(CMD_RING) as usize);
    op_write64(0x18, cmd_phys | 1);

    ERST.ring_base = phys(core::ptr::addr_of!(EVT_RING) as usize);
    ERST.ring_size = RING_CAP as u32;
    let erst_phys = phys(core::ptr::addr_of!(ERST) as usize);
    op_write64(0x20, erst_phys);
    op_write32(0x28, 1);
    op_write64(0x38, ERST.ring_base);

    let dcba_phys = phys(core::ptr::addr_of!(DCBAAP) as usize);
    op_write64(0x30, dcba_phys);

    op_write32(0x3C, u32::from(MAX_SLOTS) << 8);
    op_write32(0x00, op_read32(0x00) | USBCMD_RUN);
    for _ in 0..200_000 {
        if op_read32(0x04) & USBSTS_HCH == 0 {
            return Ok(());
        }
        pause();
    }
    Err(())
}

unsafe fn port_reset(port: u8) -> Result<(), ()> {
    let off = 0x400 + u32::from(port) * 0x10;
    let mut sc = op_read32(off);
    if sc & PORTSC_CCS == 0 {
        return Err(());
    }
    sc |= PORTSC_PR;
    op_write32(off, sc);
    for _ in 0..500_000 {
        sc = op_read32(off);
        if sc & PORTSC_PR == 0 {
            break;
        }
        pause();
    }
    sc = op_read32(off);
    sc |= PORTSC_WRC | PORTSC_PED;
    op_write32(off, sc);
    for _ in 0..100_000 {
        pause();
    }
    if op_read32(off) & PORTSC_PED == 0 {
        return Err(());
    }
    Ok(())
}

unsafe fn enable_slot() -> Result<u8, ()> {
    push_cmd(CMD_ENABLE_SLOT, 0, 0, 0)?;
    let ev = wait_event(TRB_CMD_COMPLETION, 400_000)?;
    let slot = ((ev.status >> 24) & 0xFF) as u8;
    if slot == 0 {
        return Err(());
    }
    Ok(slot)
}

unsafe fn set_ep_ring(ctx: &mut [u8], ep_off: usize, ring_va: usize) {
    let ring_phys = phys(ring_va);
    ctx[ep_off + 8..ep_off + 16].copy_from_slice(&ring_phys.to_le_bytes());
}

unsafe fn build_ep0_context(ctx: &mut [u8], max_packet: u16) {
    let ep0 = &mut ctx[0x20..0x40];
    ep0.fill(0);
    let max = max_packet.min(64);
    ep0[4] = 0;
    ep0[6] = (max & 0xFF) as u8;
    ep0[7] = ((max >> 8) & 0xFF) as u8;
    set_ep_ring(ctx, 0x20, core::ptr::addr_of!(EP0_RING) as usize);
}

unsafe fn ctx_for_slot(slot: u8) -> &'static mut [u8] {
    if !KBD_READY || slot == KBD_SLOT {
        #[allow(static_mut_refs)]
        unsafe {
            &mut *core::ptr::addr_of_mut!(DEV_CTX)
        }
    } else {
        DEV_CTX2_USED = true;
        #[allow(static_mut_refs)]
        unsafe {
            &mut *core::ptr::addr_of_mut!(DEV_CTX2)
        }
    }
}

unsafe fn address_device(slot: u8, port: u8, ctx: &mut [u8]) -> Result<(), ()> {
    let dev_phys = phys(ctx.as_ptr() as usize);
    DCBAAP[slot as usize] = dev_phys;
    build_ep0_context(ctx, 8);

    INPUT_CTX.fill(0);
    INPUT_CTX[0] = 0x03;
    INPUT_CTX[0x20..0x40].copy_from_slice(&ctx[0..0x20]);
    INPUT_CTX[0x40..0x60].copy_from_slice(&ctx[0x20..0x40]);

    let in_phys = phys(core::ptr::addr_of!(INPUT_CTX) as usize);
    let param = in_phys | (u64::from(port) << 16);
    push_cmd(CMD_ADDRESS_DEVICE, param, 0, 0)?;
    let ev = wait_event(TRB_CMD_COMPLETION, 400_000)?;
    if (ev.status & 0xFF) != 1 {
        return Err(());
    }
    Ok(())
}

unsafe fn push_transfer(
    ring: *mut Trb,
    enq: &mut usize,
    cycle: &mut bool,
    trb_type: u32,
    param: u64,
    status: u32,
    extra: u32,
) {
    let i = *enq;
    let ctrl = trb_cycle(*cycle) | (trb_type << TRB_TYPE_SHIFT) | extra;
    ring.add(i).write(Trb {
        param,
        status,
        control: ctrl,
    });
    *enq += 1;
    if *enq >= RING_CAP {
        *enq = 0;
        *cycle = !*cycle;
    }
}

unsafe fn ep0_transfer(slot: u8, setup: [u8; 8], data_in: Option<&mut [u8]>) -> Result<usize, ()> {
    EP0_ENQ = 0;
    EP0_CYCLE = true;
    let ring = core::ptr::addr_of_mut!(EP0_RING).cast::<Trb>();
    let has_in = data_in.is_some();

    let mut setup_param = 0u64;
    for (i, &b) in setup.iter().enumerate() {
        setup_param |= u64::from(b) << (i * 8);
    }
    let trt = if has_in { 2u32 } else { 3u32 };
    push_transfer(
        ring,
        &mut EP0_ENQ,
        &mut EP0_CYCLE,
        TRB_SETUP,
        setup_param,
        8,
        TRB_IDT | (trt << 16),
    );

    let mut total = 0usize;
    if let Some(buf) = data_in {
        let n = buf.len().min(512);
        let data_phys = phys(buf.as_mut_ptr() as usize);
        push_transfer(
            ring,
            &mut EP0_ENQ,
            &mut EP0_CYCLE,
            TRB_DATA,
            data_phys,
            n as u32,
            TRB_DIR_IN,
        );
        total = n;
    }

    let dir = if has_in { 1u32 } else { 2u32 };
    push_transfer(
        ring,
        &mut EP0_ENQ,
        &mut EP0_CYCLE,
        TRB_STATUS,
        0,
        0,
        dir << 16 | TRB_IOC,
    );

    doorbell(u32::from(slot), 0);
    let ev = wait_event(TRB_TRANSFER, 800_000)?;
    if (ev.status & 0xFFFF) != 1 {
        return Err(());
    }
    Ok(total)
}

unsafe fn set_address(slot: u8, addr: u8) -> Result<(), ()> {
    SETUP = [0x00, 0x05, addr, 0, 0, 0, 0, 0];
    ep0_transfer(slot, SETUP, None)?;
    Ok(())
}

unsafe fn fetch_config(slot: u8) -> Result<usize, ()> {
    SETUP = [0x80, 0x06, 0, 0x01, 0, 0, 9, 0];
    ep0_transfer(slot, SETUP, Some(&mut DATA[..9]))?;
    let total = u16::from_le_bytes([DATA[2], DATA[3]]) as usize;
    SETUP = [0x80, 0x06, 0, 0x02, 0, 0, (total & 0xFF) as u8, (total >> 8) as u8];
    let got = ep0_transfer(slot, SETUP, Some(&mut DATA[..total.min(512)]))?;
    Ok(got)
}

unsafe fn set_configuration(slot: u8, cfg: u8) -> Result<(), ()> {
    SETUP = [0x00, 0x09, cfg, 0, 0, 0, 0, 0];
    ep0_transfer(slot, SETUP, None)?;
    let _ = slot;
    Ok(())
}

unsafe fn configure_interrupt_ep(
    ctx: &mut [u8],
    slot: u8,
    ep: u8,
    mps: u16,
    irq_ring: *mut Trb,
    enq: &mut usize,
    cycle: &mut bool,
) -> Result<(), ()> {
    let _ = slot;
    let ep_idx = (ep & 0x0F) * 2 + 1;
    let off = 0x20 + (ep_idx as usize) * 0x20;
    if off + 0x20 > ctx.len() {
        return Err(());
    }
    {
        let ep_ctx = &mut ctx[off..off + 0x20];
        ep_ctx.fill(0);
        ep_ctx[1] = 3;
        ep_ctx[4] = ep;
        ep_ctx[6] = (mps & 0xFF) as u8;
        ep_ctx[7] = (mps >> 8) as u8;
        ep_ctx[11]  = 8;
    }
    set_ep_ring(ctx, off, irq_ring as usize);
    *enq = 0;
    *cycle = true;

    INPUT_CTX.fill(0);
    INPUT_CTX[0] = 1 << ep_idx;
    INPUT_CTX[0x20..0x40].copy_from_slice(&ctx[0..0x20]);
    INPUT_CTX[off..off + 0x20].copy_from_slice(&ctx[off..off + 0x20]);

    let in_phys = phys(core::ptr::addr_of!(INPUT_CTX) as usize);
    push_cmd(CMD_CONFIGURE_EP, in_phys, 0, 0)?;
    let ev = wait_event(TRB_CMD_COMPLETION, 400_000)?;
    if (ev.status & 0xFF) != 1 {
        return Err(());
    }
    Ok(())
}

unsafe fn poll_interrupt(
    slot: u8,
    ep: u8,
    mps: u16,
    ring: *mut Trb,
    enq: &mut usize,
    cycle: &mut bool,
    buf: &mut [u8],
) -> Result<usize, ()> {
    let ep_doorbell = u32::from(ep & 0x0F);
    let buf_phys = phys(buf.as_mut_ptr() as usize);
    push_transfer(
        ring,
        enq,
        cycle,
        TRB_NORMAL,
        buf_phys,
        mps as u32,
        TRB_DIR_IN | TRB_IOC,
    );
    doorbell(u32::from(slot), ep_doorbell);
    let ev = wait_event(TRB_TRANSFER, 50_000)?;
    if (ev.status & 0xFFFF) != 1 {
        return Err(());
    }
    Ok(mps as usize)
}

unsafe fn try_hid_on_port(port: u8) -> (bool, bool) {
    let mut got_kbd = false;
    let mut got_mouse = false;

    if port_reset(port).is_err() {
        return (false, false);
    }
    let slot = match enable_slot() {
        Ok(s) => s,
        Err(()) => return (false, false),
    };
    let ctx = ctx_for_slot(slot);
    if address_device(slot, port, ctx).is_err() {
        return (false, false);
    }
    if set_address(slot, 1).is_err() {
        return (false, false);
    }
    let n = match fetch_config(slot) {
        Ok(n) => n,
        Err(()) => return (false, false),
    };
    if n < 9 {
        return (false, false);
    }
    let cfg = DATA[5];
    if set_configuration(slot, cfg).is_err() {
        return (false, false);
    }

    if !KBD_READY {
        if let Some((_, ep, mps)) = find_hid_boot_eps(&DATA[..n], 1) {
            let ring = core::ptr::addr_of_mut!(KBD_RING).cast::<Trb>();
            if configure_interrupt_ep(ctx, slot, ep, mps, ring, &mut KBD_ENQ, &mut KBD_CYCLE).is_ok()
            {
                KBD_EP = ep;
                KBD_MPS = mps.max(8);
                KBD_SLOT = slot;
                KBD_READY = true;
                got_kbd = true;
                diag_log::line(b"xhci nat: kbd ok");
            }
        }
    }

    if !MOUSE_READY {
        if let Some((_, ep, mps)) = find_hid_boot_eps(&DATA[..n], 2) {
            let mouse_ctx = if slot == KBD_SLOT {
                ctx
            } else {
                ctx_for_slot(slot)
            };
            let ring = core::ptr::addr_of_mut!(MOUSE_RING).cast::<Trb>();
            if configure_interrupt_ep(
                mouse_ctx,
                slot,
                ep,
                mps,
                ring,
                &mut MOUSE_ENQ,
                &mut MOUSE_CYCLE,
            )
            .is_ok()
            {
                MOUSE_EP = ep;
                MOUSE_MPS = mps.clamp(4, 64);
                MOUSE_SLOT = slot;
                MOUSE_READY = true;
                got_mouse = true;
                diag_log::line(b"xhci nat: mouse ok");
            }
        }
    }

    (got_kbd, got_mouse)
}

/// Initialize native xHCI; enumerate boot keyboard (required) and one boot mouse when present.
pub unsafe fn init(skew: u64, mmio: usize) -> bool {
    MMIO = mmio;
    PHYS_SKEW = skew;
    OP_OFF = cap_read32(0) as u32;
    DB_OFF = cap_read32(0x14);
    let hcs1 = cap_read32(4);
    MAX_SLOTS = ((hcs1 & 0xFF) as u8).max(1).min(32);
    MAX_PORTS = (((hcs1 >> 24) & 0xFF) as u8).max(1).min(15);

    KBD_READY = false;
    MOUSE_READY = false;
    HID_KBD_XFER_OK = false;
    HID_MOUSE_XFER_OK = false;
    KBD_USB_FAILS = 0;
    MOUSE_USB_FAILS = 0;
    KBD_SLOT = 0;
    MOUSE_SLOT = 0;
    DEV_CTX2_USED = false;

    if hc_reset().is_err() {
        diag_log::line(b"xhci nat: reset fail");
        MMIO = 0;
        return false;
    }
    if rings_init().is_err() {
        diag_log::line(b"xhci nat: rings fail");
        MMIO = 0;
        return false;
    }

    for port in 0..MAX_PORTS {
        if KBD_READY && MOUSE_READY {
            break;
        }
        let _ = try_hid_on_port(port);
    }

    if !KBD_READY {
        diag_log::line(b"xhci nat: no kbd");
        MMIO = 0;
        return false;
    }
    true
}

pub fn keyboard_ready() -> bool {
    unsafe { KBD_READY }
}

pub unsafe fn poll_keyboard_report() -> Option<[u8; 8]> {
    if !KBD_READY || MMIO == 0 {
        return None;
    }
    IRQ_KBD.fill(0);
    let ring = core::ptr::addr_of_mut!(KBD_RING).cast::<Trb>();
    if poll_interrupt(
        KBD_SLOT,
        KBD_EP,
        KBD_MPS,
        ring,
        &mut KBD_ENQ,
        &mut KBD_CYCLE,
        &mut IRQ_KBD,
    )
    .is_err()
    {
        KBD_USB_FAILS = KBD_USB_FAILS.saturating_add(1);
        return None;
    }
    KBD_USB_FAILS = 0;
    HID_KBD_XFER_OK = true;
    Some([
        IRQ_KBD[0], IRQ_KBD[1], IRQ_KBD[2], IRQ_KBD[3], IRQ_KBD[4], IRQ_KBD[5], IRQ_KBD[6],
        IRQ_KBD[7],
    ])
}

pub fn hid_kbd_suppresses_ps2() -> bool {
    unsafe {
        KBD_READY && HID_KBD_XFER_OK && KBD_USB_FAILS < USB_STALL_BEFORE_PS2
    }
}

pub fn mouse_ready() -> bool {
    unsafe {
        MOUSE_READY && HID_MOUSE_XFER_OK && MOUSE_USB_FAILS < USB_STALL_BEFORE_PS2
    }
}

pub fn usb_mouse_count() -> usize {
    unsafe {
        if MOUSE_READY {
            1
        } else {
            0
        }
    }
}

pub unsafe fn poll_mouse_slot(idx: usize) -> Option<(u8, i16, i16)> {
    if idx != 0 || !MOUSE_READY || MMIO == 0 {
        return None;
    }
    IRQ_MOUSE.fill(0);
    let ring = core::ptr::addr_of_mut!(MOUSE_RING).cast::<Trb>();
    if poll_interrupt(
        MOUSE_SLOT,
        MOUSE_EP,
        MOUSE_MPS,
        ring,
        &mut MOUSE_ENQ,
        &mut MOUSE_CYCLE,
        &mut IRQ_MOUSE,
    )
    .is_err()
    {
        MOUSE_USB_FAILS = MOUSE_USB_FAILS.saturating_add(1);
        return None;
    }
    if IRQ_MOUSE.len() < 3 {
        MOUSE_USB_FAILS = MOUSE_USB_FAILS.saturating_add(1);
        return None;
    }
    MOUSE_USB_FAILS = 0;
    HID_MOUSE_XFER_OK = true;
    let buttons = IRQ_MOUSE[0] & 0x07;
    let dx = IRQ_MOUSE[1] as i8 as i16;
    let dy = -(IRQ_MOUSE[2] as i8 as i16);
    Some((buttons, dx, dy))
}

pub fn active() -> bool {
    unsafe { MMIO != 0 }
}
