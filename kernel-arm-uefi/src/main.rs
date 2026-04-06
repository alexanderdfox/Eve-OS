// SPDX-License-Identifier: MIT OR Apache-2.0
//
//! EVE AArch64 UEFI payload for GRUB chainload on **Asahi Linux** (and QEMU `virt` / UTM).
//! GOP splash + **UEFI Simple Pointer** (mouse/trackpad) and **Simple Text Input** when the
//! firmware exposes them. This is **not** the x86_64 Eve desktop — that OS only runs under QEMU/UTM
//! or a PC; see `utm/MAC-M1-PRO.txt`.

#![no_main]
#![no_std]

mod font;
mod uefi_ui;

use core::time::Duration;
use uefi::boot;
use uefi::prelude::*;
use uefi::proto::console::gop::{BltPixel, GraphicsOutput};
use uefi::proto::console::pointer::Pointer;
use uefi::proto::console::text::Input;
use uefi::system;

#[entry]
fn main() -> Status {
    if let Err(e) = uefi::helpers::init() {
        return e.status();
    }

    let _ = system::with_stdout(|stdout| {
        let _ = stdout.reset(false);
        let _ = stdout.output_string(cstr16!("=== EVE AArch64 UEFI ===\r\n"));
        let _ = stdout.output_string(cstr16!("GOP + UEFI pointer/keyboard when firmware allows.\r\n"));
        let _ = stdout.output_string(cstr16!("Asahi: utm/ASAHI-M1-UEFI-SETUP.txt\r\n"));
    });

    let mut line_buf = [BltPixel::new(0, 0, 0); uefi_ui::MAX_SCAN];

    let gh = boot::get_handle_for_protocol::<GraphicsOutput>().ok();
    let ph = boot::get_handle_for_protocol::<Pointer>().ok();
    let ih = boot::get_handle_for_protocol::<Input>().ok();

    if let Some(h) = gh {
        if let Ok(mut gop) = boot::open_protocol_exclusive::<GraphicsOutput>(h) {
            let mut ptr = ph.and_then(|p| boot::open_protocol_exclusive::<Pointer>(p).ok());
            let mut inp = ih.and_then(|i| boot::open_protocol_exclusive::<Input>(i).ok());
            uefi_ui::run_interactive_demo(&mut gop, &mut line_buf, &mut ptr, &mut inp);
        }
    }

    loop {
        let _ = boot::stall(Duration::from_secs(3600));
    }
}
