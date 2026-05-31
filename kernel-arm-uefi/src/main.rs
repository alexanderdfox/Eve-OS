// SPDX-License-Identifier: MIT OR Apache-2.0
//
//! AArch64 UEFI: **same on-screen Eve UI** as x86 (`gfx::render_frame`, epilepsy notice, SHRINE/SYS
//! tabs, browser, cursors) via a CPU shadow buffer + GOP blit. **Simple Pointer** (relative
//! movement) is preferred for the cursor — this matches the **built-in MacBook trackpad** and
//! typical UTM/QEMU setups — with **Absolute Pointer** as fallback when no simple-pointer event is
//! available (and only when absolute min/max are sane). **Keyboard:** drain **ConIn** from the
//! system table first (QEMU), then any extra [`Input`] handle (UTM / Apple quirks).
//! **VirtIO-MMIO** NIC: VM vendor heuristics or NVRAM
//! **`EveVirtioMmioScan`** = **`1`** / **`Y`** forces MMIO scan when the firmware vendor string is
//! minimal (some VMs); **never** enabled on bare-metal Apple firmware (unsafe MMIO probe).
//! Custom [`panic_uefi`] avoids `ResetSystem(SHUTDOWN)`, which often **reboots** Apple machines and
//! looks like a GRUB boot loop. PS/2, PCI Ethernet, USB HID host, and disk install remain x86-only.

#![no_main]
#![no_std]

mod abs_pointer;

use core::panic::PanicInfo;
use core::ptr::NonNull;
use core::time::Duration;

use kernel::arm_input::{self, ArmKeyEvent};
use kernel::arm_run;
use kernel::fb_info::{FrameBufferInfo, PixelFormat};
use abs_pointer::AbsolutePointer;
use uefi::boot::{self, MemoryType, OpenProtocolAttributes, OpenProtocolParams, SearchType};
use uefi::prelude::*;
use uefi::runtime::{get_variable, reset, set_variable, ResetType, VariableAttributes, VariableVendor};
use uefi::table;
use uefi::proto::console::gop::{
    BltOp, BltPixel, BltRegion, GraphicsOutput, Mode, ModeInfo, PixelFormat as GopPixelFormat,
};
use uefi::proto::console::pointer::{Pointer, PointerMode};
use uefi::proto::console::text::{Input, Key, ScanCode};
use uefi::proto::ProtocolPointer;
use uefi::system;

/// Stack UTF-8 sink for [`CStr16::as_str_in_buf`].
struct VendorUtf8 {
    data: [u8; 96],
    len: usize,
}

impl VendorUtf8 {
    const fn new() -> Self {
        Self {
            data: [0; 96],
            len: 0,
        }
    }
}

impl core::fmt::Write for VendorUtf8 {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let b = s.as_bytes();
        if self.len + b.len() > self.data.len() {
            return Err(core::fmt::Error);
        }
        self.data[self.len..self.len + b.len()].copy_from_slice(b);
        self.len += b.len();
        Ok(())
    }
}

fn contains_ascii_case_insensitive(haystack: &str, needle: &[u8]) -> bool {
    let hay = haystack.as_bytes();
    if needle.is_empty() {
        return true;
    }
    hay.windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle))
}

/// Heuristic only: MMIO at `0x0a00_0000` exists on QEMU `virt` and similar VMs. Apple and most bare
/// metal do not — reads can fault. Use NVRAM [`nvram_forces_virtio_mmio_scan`] when the vendor
/// string does not match (some ARM QEMU images report a minimal vendor name).
fn firmware_allows_virtio_mmio_scan() -> bool {
    let mut buf = VendorUtf8::new();
    if system::firmware_vendor().as_str_in_buf(&mut buf).is_err() {
        return false;
    }
    let s = core::str::from_utf8(&buf.data[..buf.len]).unwrap_or("");
    contains_ascii_case_insensitive(s, b"qemu")
        || contains_ascii_case_insensitive(s, b"edk")
        || contains_ascii_case_insensitive(s, b"tianocore")
        || contains_ascii_case_insensitive(s, b"ovmf")
        || contains_ascii_case_insensitive(s, b"bochs")
        || contains_ascii_case_insensitive(s, b"kvm")
        || contains_ascii_case_insensitive(s, b"vmware")
        || contains_ascii_case_insensitive(s, b"parallels")
        || contains_ascii_case_insensitive(s, b"virtualbox")
        || contains_ascii_case_insensitive(s, b"vbox")
        || contains_ascii_case_insensitive(s, b"hyper-v")
        || contains_ascii_case_insensitive(s, b"hyperv")
        || contains_ascii_case_insensitive(s, b"virt")
        // Do **not** match bare-metal Apple firmware here: probing virtio-mmio at 0x0a00_0000 can fault.
        // UTM / QEMU still match "qemu", "edk", "utm", "tianocore", etc. Use NVRAM `EveVirtioMmioScan=1` if needed.
        || contains_ascii_case_insensitive(s, b"utm")
        || contains_ascii_case_insensitive(s, b"foundation")
        || contains_ascii_case_insensitive(s, b"linaro")
}

/// Global UEFI variable: first byte `1`, `Y`, or `y` forces VirtIO-MMIO NIC scan (for VMs whose
/// firmware vendor string is not recognized).
fn nvram_forces_virtio_mmio_scan() -> bool {
    let mut v = [0u8; 8];
    let Ok((got, _)) = get_variable(
        cstr16!("EveVirtioMmioScan"),
        &VariableVendor::GLOBAL_VARIABLE,
        &mut v,
    ) else {
        return false;
    };
    matches!(
        got.first().copied(),
        Some(b'1' | b'y' | b'Y' | 0x01)
    )
}

/// Prefer **GetProtocol** first so we do not disconnect GOP/console drivers (`Exclusive` can `Stop`
/// other agents — some Apple firmwares react badly). Fall back to exclusive if needed.
fn save_eve_settings_nvram(data: &[u8]) {
    let full = VariableAttributes::NON_VOLATILE
        | VariableAttributes::BOOTSERVICE_ACCESS
        | VariableAttributes::RUNTIME_ACCESS;
    if set_variable(
        cstr16!("EveOsSettings"),
        &VariableVendor::GLOBAL_VARIABLE,
        full,
        data,
    )
    .is_err()
    {
        // Some Apple / U-Boot UEFI stacks reject RUNTIME_ACCESS for custom variables.
        let bs_nv = VariableAttributes::NON_VOLATILE | VariableAttributes::BOOTSERVICE_ACCESS;
        let _ = set_variable(
            cstr16!("EveOsSettings"),
            &VariableVendor::GLOBAL_VARIABLE,
            bs_nv,
            data,
        );
    }
}

unsafe fn uefi_cold_reboot() {
    reset(ResetType::COLD, Status::SUCCESS, None);
}

/// Some Apple firmware treats `Shutdown` as reboot; still preferred over spinning forever.
unsafe fn uefi_shutdown() {
    reset(ResetType::SHUTDOWN, Status::SUCCESS, None);
}

fn open_get_or_exclusive<P: ProtocolPointer + ?Sized>(
    handle: uefi::Handle,
) -> uefi::Result<boot::ScopedProtocol<P>> {
    unsafe {
        match boot::open_protocol::<P>(
            OpenProtocolParams {
                handle,
                agent: boot::image_handle(),
                controller: None,
            },
            OpenProtocolAttributes::GetProtocol,
        ) {
            Ok(p) => Ok(p),
            Err(_) => boot::open_protocol_exclusive::<P>(handle),
        }
    }
}

/// Apple / UTM may expose **Absolute Pointer**; some VMs advertise it with a degenerate mode — keep
/// **Simple Pointer** open too and use it when absolute min/max are unusable.
fn try_open_first_absolute_pointer() -> Option<boot::ScopedProtocol<AbsolutePointer>> {
    let Ok(handles) = boot::locate_handle_buffer(SearchType::from_proto::<AbsolutePointer>()) else {
        return None;
    };
    for &handle in handles.iter() {
        if let Ok(p) = open_get_or_exclusive::<AbsolutePointer>(handle) {
            return Some(p);
        }
    }
    None
}

/// Score a simple-pointer device: real trackpads/mice usually report non-zero X/Y resolution.
/// When several `EFI_SIMPLE_POINTER_PROTOCOL` instances exist, prefer the strongest match (ties
/// favor later registration — often USB after a dummy tablet entry).
fn simple_pointer_device_score(mode: &PointerMode) -> u32 {
    let mut s = 0u32;
    if mode.resolution[0] > 0 {
        s += 2;
    }
    if mode.resolution[1] > 0 {
        s += 2;
    }
    if mode.has_button[0] {
        s += 1;
    }
    if mode.has_button[1] {
        s += 1;
    }
    s
}

/// Open the best **Simple Pointer** handle when firmware publishes more than one (common on Apple
/// Silicon guests). `get_handle_for_protocol` alone often returns the wrong instance for the
/// laptop trackpad.
fn try_open_best_simple_pointer() -> Option<boot::ScopedProtocol<Pointer>> {
    let Ok(handles) = boot::locate_handle_buffer(SearchType::from_proto::<Pointer>()) else {
        return boot::get_handle_for_protocol::<Pointer>()
            .ok()
            .and_then(|h| open_get_or_exclusive::<Pointer>(h).ok());
    };
    let mut best_handle: Option<uefi::Handle> = None;
    let mut best_score = 0u32;
    for &handle in handles.iter() {
        let Ok(p) = open_get_or_exclusive::<Pointer>(handle) else {
            continue;
        };
        let sc = simple_pointer_device_score(p.mode());
        if sc >= best_score {
            best_score = sc;
            best_handle = Some(handle);
        }
    }
    if let Some(h) = best_handle {
        return open_get_or_exclusive::<Pointer>(h).ok();
    }
    boot::get_handle_for_protocol::<Pointer>()
        .ok()
        .and_then(|h| open_get_or_exclusive::<Pointer>(h).ok())
}

fn map_abs_to_screen(v: u64, vmin: u64, vmax: u64, smax: i64) -> i64 {
    if smax <= 0 {
        return 0;
    }
    if vmax <= vmin {
        return smax / 2;
    }
    let v = v.clamp(vmin, vmax);
    let num = u128::from(v - vmin) * u128::from(smax as u64);
    let den = u128::from(vmax - vmin);
    let q = (num / den).min(smax as u128);
    q as i64
}

fn absolute_pointer_buttons(active: u32) -> u8 {
    let mut b = 0u8;
    if active & 0x1 != 0 {
        b |= 1;
    }
    if active & 0x2 != 0 {
        b |= 2;
    }
    b
}

fn conin_stdin_ptr() -> Option<*mut Input> {
    let st = table::system_table_raw()?;
    let st = unsafe { st.as_ref() };
    if st.stdin.is_null() {
        return None;
    }
    Some(st.stdin.cast::<Input>())
}

/// Firmware’s **ConIn** (`SystemTable.ConIn`) — on QEMU `virt` this is usually where USB/VGA
/// keyboard events arrive; `BootServices::LocateProtocol` can return a different `Input` instance
/// that never sees keys.
unsafe fn reset_conin_input() {
    if let Some(p) = conin_stdin_ptr() {
        let input = &mut *p;
        let _ = input.reset(false);
    }
}

unsafe fn drain_conin_keys() {
    if let Some(p) = conin_stdin_ptr() {
        let input = &mut *p;
        while let Ok(Some(k)) = input.read_key() {
            if let Some(ev) = map_uefi_key(k) {
                arm_input::key_queue_push(ev);
            }
        }
    }
}

fn map_uefi_key(k: Key) -> Option<ArmKeyEvent> {
    match k {
        Key::Printable(c) => {
            let ch = char::from(c);
            let code = u32::from(ch);
            match code {
                8 => Some(ArmKeyEvent::Backspace),
                10 | 13 => Some(ArmKeyEvent::Enter),
                32..=126 => Some(ArmKeyEvent::Char(code as u8)),
                _ => None,
            }
        }
        Key::Special(s) => match s {
            ScanCode::ESCAPE => Some(ArmKeyEvent::Escape),
            ScanCode::HOME => Some(ArmKeyEvent::Home),
            ScanCode::END => Some(ArmKeyEvent::End),
            ScanCode::LEFT => Some(ArmKeyEvent::ArrowLeft),
            ScanCode::RIGHT => Some(ArmKeyEvent::ArrowRight),
            ScanCode::PAGE_UP => Some(ArmKeyEvent::PageUp),
            ScanCode::PAGE_DOWN => Some(ArmKeyEvent::PageDown),
            ScanCode::UP => Some(ArmKeyEvent::ArrowUp),
            ScanCode::DOWN => Some(ArmKeyEvent::ArrowDown),
            ScanCode::DELETE => Some(ArmKeyEvent::Backspace),
            ScanCode::FUNCTION_1 => Some(ArmKeyEvent::Func(1)),
            ScanCode::FUNCTION_2 => Some(ArmKeyEvent::Func(2)),
            ScanCode::FUNCTION_3 => Some(ArmKeyEvent::Func(3)),
            ScanCode::FUNCTION_4 => Some(ArmKeyEvent::Func(4)),
            ScanCode::FUNCTION_5 => Some(ArmKeyEvent::Func(5)),
            ScanCode::FUNCTION_6 => Some(ArmKeyEvent::Func(6)),
            ScanCode::FUNCTION_7 => Some(ArmKeyEvent::Func(7)),
            ScanCode::FUNCTION_8 => Some(ArmKeyEvent::Func(8)),
            ScanCode::FUNCTION_9 => Some(ArmKeyEvent::Func(9)),
            ScanCode::FUNCTION_10 => Some(ArmKeyEvent::Func(10)),
            ScanCode::FUNCTION_11 => Some(ArmKeyEvent::Func(11)),
            ScanCode::FUNCTION_12 => Some(ArmKeyEvent::Func(12)),
            _ => None,
        },
    }
}

fn kernel_pixel_from_gop(pf: GopPixelFormat) -> PixelFormat {
    match pf {
        GopPixelFormat::Rgb => PixelFormat::Rgb,
        GopPixelFormat::Bgr => PixelFormat::Bgr,
        // Blt-only / bitmask GOPs still accept BufferToVideo blits; treat shadow as 32-bit BGRx.
        GopPixelFormat::Bitmask | GopPixelFormat::BltOnly => PixelFormat::Bgr,
    }
}

/// CPU shadow buffer sized to **GOP stride × height** (not width × height) so row padding matches
/// firmware (common on built-in panels); pixel order follows the active UEFI mode.
fn try_alloc_shadow_for_mode_info(mi: ModeInfo) -> Option<(NonNull<u8>, usize, FrameBufferInfo)> {
    let (gw, gh) = mi.resolution();
    let stride = mi.stride();
    let bpp = 4usize;
    let kpf = kernel_pixel_from_gop(mi.pixel_format());
    let need = stride.checked_mul(gh)?.checked_mul(bpp)?;
    if need == 0 {
        return None;
    }
    let pool = boot::allocate_pool(MemoryType::LOADER_DATA, need).ok()?;
    Some((
        pool,
        need,
        FrameBufferInfo {
            width: gw,
            height: gh,
            stride,
            pixel_format: kpf,
            bytes_per_pixel: bpp,
        },
    ))
}

fn try_alloc_shadow_for_current_mode(gop: &GraphicsOutput) -> Option<(NonNull<u8>, usize, FrameBufferInfo)> {
    try_alloc_shadow_for_mode_info(gop.current_mode_info())
}

/// Pixels per `BufferToVideo` chunk (stack). Any horizontal resolution works; previously a single
/// 1920-wide row buffer caused a **black screen** on panels wider than 1920 (common on laptops).
const BLIT_CHUNK: usize = 640;

fn blit_eve_to_gop(gop: &mut GraphicsOutput, src: &[u8], info: &FrameBufferInfo) {
    let w = info.width;
    let h = info.height;
    let stride_px = info.stride;
    let bpp = info.bytes_per_pixel;
    let mut chunk = [BltPixel::new(0, 0, 0); BLIT_CHUNK];

    for y in 0..h {
        let mut x0 = 0;
        while x0 < w {
            let n = (w - x0).min(BLIT_CHUNK);
            for i in 0..n {
                let px = x0 + i;
                let idx = y * stride_px * bpp + px * bpp;
                let (r, g, b) = if idx + 2 < src.len() {
                    match info.pixel_format {
                        PixelFormat::Bgr => (src[idx + 2], src[idx + 1], src[idx]),
                        PixelFormat::Rgb => (src[idx], src[idx + 1], src[idx + 2]),
                        PixelFormat::U8 | PixelFormat::Unknown => {
                            (src[idx + 2], src[idx + 1], src[idx])
                        }
                    }
                } else {
                    (0, 0, 0)
                };
                chunk[i] = BltPixel::new(r, g, b);
            }
            let _ = gop.blt(BltOp::BufferToVideo {
                buffer: &chunk[..n],
                src: BltRegion::Full,
                dest: (x0, y),
                dims: (n, 1),
            });
            x0 += n;
        }
    }
}

/// Never call `RuntimeServices::ResetSystem`: on Apple firmware **Shutdown** often reboots into the
/// boot chain (GRUB → chainload → panic → repeat). Avoid `hlt #imm` loops (may UNDEF at EL1).
#[cfg(not(test))]
#[panic_handler]
fn panic_uefi(_info: &PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}

#[entry]
fn main() -> Status {
    if let Err(e) = uefi::helpers::init() {
        return e.status();
    }

    let allow_mmio_scan = firmware_allows_virtio_mmio_scan() || nvram_forces_virtio_mmio_scan();
    kernel::nic::set_allow_virtio_mmio_scan(allow_mmio_scan);

    let mut boot_settings = kernel::DeviceSettings::new();
    let mut nv_buf = [0u8; kernel::settings_persist::BLOB_LEN];
    if let Ok((got, _)) = get_variable(
        cstr16!("EveOsSettings"),
        &VariableVendor::GLOBAL_VARIABLE,
        &mut nv_buf,
    ) {
        let _ = kernel::settings_persist::decode_merge(&mut boot_settings, got);
    }

    let _ = system::with_stdout(|stdout| {
        let _ = stdout.reset(false);
        let _ = stdout.output_string(cstr16!(
            "Eve OS (AArch64 UEFI) — virtio-mmio NIC: VM vendor match or NVRAM EveVirtioMmioScan=1/Y (required on some hosts)\r\n"
        ));
    });

    let gh = boot::get_handle_for_protocol::<GraphicsOutput>().ok();
    let kh = boot::get_handle_for_protocol::<Input>().ok();

    let Some(h) = gh else {
        let _ = system::with_stdout(|s| {
            let _ = s.output_string(cstr16!("No GOP — cannot start Eve UI.\r\n"));
        });
        loop {
            let _ = boot::stall(Duration::from_secs(3600));
        }
    };

    let mut gop = match open_get_or_exclusive::<GraphicsOutput>(h) {
        Ok(g) => g,
        Err(e) => {
            let _ = system::with_stdout(|s| {
                let _ = s.output_string(cstr16!("open GOP failed (exclusive + get)\r\n"));
            });
            return e.status();
        }
    };

    let mut ptr_abs = try_open_first_absolute_pointer();
    let mut ptr = try_open_best_simple_pointer();
    let mut stdin_alt = kh.and_then(|k| open_get_or_exclusive::<Input>(k).ok());

    const MAX_MODES: usize = 128;
    let mut mode_list: [Option<Mode>; MAX_MODES] = [None; MAX_MODES];
    let mut n_modes = 0usize;
    for m in gop.modes() {
        if n_modes < MAX_MODES {
            mode_list[n_modes] = Some(m);
            n_modes += 1;
        }
    }
    for i in 0..n_modes {
        let mut best = i;
        for j in (i + 1)..n_modes {
            let ra = mode_list[j].unwrap().info().resolution();
            let rb = mode_list[best].unwrap().info().resolution();
            if ra.0.saturating_mul(ra.1) > rb.0.saturating_mul(rb.1) {
                best = j;
            }
        }
        if best != i {
            mode_list.swap(i, best);
        }
    }

    let mut picked: Option<(NonNull<u8>, usize, FrameBufferInfo)> = None;

    let want_custom = boot_settings.display_use_custom_resolution
        && boot_settings.display_pref_width > 0
        && boot_settings.display_pref_height > 0;
    let tw = boot_settings.display_pref_width as usize;
    let th = boot_settings.display_pref_height as usize;

    if want_custom {
        for i in 0..n_modes {
            let Some(mode) = mode_list[i] else {
                continue;
            };
            let (mw, mh) = mode.info().resolution();
            if mw != tw || mh != th {
                continue;
            }
            if gop.set_mode(&mode).is_err() {
                continue;
            }
            if let Some(triple) = try_alloc_shadow_for_current_mode(&gop) {
                picked = Some(triple);
                break;
            }
        }
    }

    // Prefer **current** GOP mode without `set_mode` first: forcing max resolution on Apple/GRUB
    // handoff can reset the display stack or fail allocations.
    if picked.is_none() {
        picked = try_alloc_shadow_for_current_mode(&gop);
    }

    if picked.is_none() {
        for i in 0..n_modes {
            let Some(mode) = mode_list[i] else {
                continue;
            };
            if gop.set_mode(&mode).is_err() {
                continue;
            }
            if let Some(triple) = try_alloc_shadow_for_current_mode(&gop) {
                picked = Some(triple);
                break;
            }
        }
    }

    let Some((pool, need, fb_info)) = picked else {
        let _ = system::with_stdout(|s| {
            let _ = s.output_string(cstr16!(
                "allocate_pool failed for GOP shadow (tried all modes).\r\n"
            ));
        });
        loop {
            let _ = boot::stall(Duration::from_secs(3600));
        }
    };

    let buf = unsafe { core::slice::from_raw_parts_mut(pool.as_ptr(), need) };

    let (gw, gh) = (fb_info.width, fb_info.height);

    kernel::arm_run::set_bootstrap_device_settings(boot_settings);
    kernel::arm_run::set_bootstrap_platform_caps(kernel::settings::PlatformCaps::arm_uefi(
        !allow_mmio_scan,
    ));
    unsafe {
        kernel::arm_run::register_settings_blob_saver(Some(save_eve_settings_nvram));
        kernel::power::register_hooks(
            Some(uefi_cold_reboot),
            Some(uefi_shutdown),
        );
    }

    if let Some(p) = ptr_abs.as_mut() {
        let _ = p.reset(false);
    }
    if let Some(p) = ptr.as_mut() {
        let _ = p.reset(false);
    }
    unsafe {
        reset_conin_input();
    }
    if let Some(s) = stdin_alt.as_mut() {
        let _ = s.reset(false);
    }

    let mut acc_x: i64 = (gw / 2) as i64;
    let mut acc_y: i64 = (gh / 2) as i64;
    let max_x = (gw.saturating_sub(1)) as i64;
    let max_y = (gh.saturating_sub(1)) as i64;
    let mut abs_btn = 0u8;

    loop {
        arm_input::key_queue_reset();
        unsafe {
            drain_conin_keys();
        }
        if let Some(s) = stdin_alt.as_mut() {
            while let Ok(Some(k)) = s.read_key() {
                if let Some(ev) = map_uefi_key(k) {
                    arm_input::key_queue_push(ev);
                }
            }
        }

        let mut pos_ok = false;
        let mut btn_out = abs_btn;

        // Prefer **Simple Pointer** (MacBook / UTM trackpad) over **Absolute Pointer** so a
        // tablet-style absolute device cannot mask relative trackpad motion.
        if let Some(p) = ptr.as_mut() {
            if let Ok(Some(st)) = p.read_state() {
                acc_x += st.relative_movement[0] as i64;
                acc_y += st.relative_movement[1] as i64;
                acc_x = acc_x.clamp(0, max_x);
                acc_y = acc_y.clamp(0, max_y);
                let mut btn = 0u8;
                if st.button[0] {
                    btn |= 1;
                }
                if st.button[1] {
                    btn |= 2;
                }
                btn_out = btn;
                abs_btn = btn;
                pos_ok = true;
            }
        }

        if !pos_ok {
            if let Some(p) = ptr_abs.as_mut() {
                match p.read_state() {
                    Ok(Some(st)) => {
                        abs_btn = absolute_pointer_buttons(st.active_buttons);
                        btn_out = abs_btn;
                        if let Some(mode) = p.mode() {
                            let range_ok = mode.absolute_max_x > mode.absolute_min_x
                                && mode.absolute_max_y > mode.absolute_min_y;
                            if range_ok {
                                acc_x = map_abs_to_screen(
                                    st.current_x,
                                    mode.absolute_min_x,
                                    mode.absolute_max_x,
                                    max_x,
                                );
                                acc_y = map_abs_to_screen(
                                    st.current_y,
                                    mode.absolute_min_y,
                                    mode.absolute_max_y,
                                    max_y,
                                );
                                pos_ok = true;
                            }
                        }
                    }
                    Ok(None) | Err(_) => {
                        btn_out = abs_btn;
                    }
                }
            }
        }

        if pos_ok || ptr_abs.is_some() || ptr.is_some() {
            arm_input::set_pointer_abs(acc_x as i32, acc_y as i32, btn_out);
        } else {
            arm_input::set_pointer_abs(acc_x as i32, acc_y as i32, 0);
        }

        unsafe {
            arm_run::main_step(buf, &fb_info);
        }
        blit_eve_to_gop(&mut gop, buf, &fb_info);
        let _ = boot::stall(Duration::from_millis(8));
    }
}
