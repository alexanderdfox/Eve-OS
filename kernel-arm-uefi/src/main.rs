// SPDX-License-Identifier: MIT OR Apache-2.0
//
//! EVE AArch64 UEFI payload: serial banner + GOP fill (QEMU `virt` / Apple Silicon HVF).

#![no_main]
#![no_std]

use core::time::Duration;
use uefi::boot;
use uefi::prelude::*;
use uefi::proto::console::gop::{BltOp, BltPixel, GraphicsOutput};
use uefi::system;

#[entry]
fn main() -> Status {
    if let Err(e) = uefi::helpers::init() {
        return e.status();
    }

    let _ = system::with_stdout(|stdout| {
        let _ = stdout.reset(false);
        let _ = stdout.output_string(cstr16!("=== EVE AArch64 UEFI ===\r\n"));
        let _ = stdout.output_string(cstr16!("QEMU virt/UTM EDK2; Asahi Linux: utm/ASAHI-M1-UEFI-SETUP.txt\r\n"));
        let _ = stdout.output_string(cstr16!("Also: utm/ARM-UEFI-SETUP.txt\r\n"));
    });

    if let Ok(handle) = boot::get_handle_for_protocol::<GraphicsOutput>() {
        if let Ok(mut gop) = boot::open_protocol_exclusive::<GraphicsOutput>(handle) {
            let mi = gop.current_mode_info();
            let (w, h) = mi.resolution();
            let _ = gop.blt(BltOp::VideoFill {
                color: BltPixel::new(0x18, 0x30, 0x48),
                dest: (0, 0),
                dims: (w, h),
            });
        }
    }

    boot::stall(Duration::from_secs(3));
    Status::SUCCESS
}
