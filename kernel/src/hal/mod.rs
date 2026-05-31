// SPDX-License-Identifier: MIT OR Apache-2.0
//
//! Hardware abstraction layer: platform backends implement these traits; `gfx`, `net`, and
//! `arm_run` consume them via slices and shared types. See `utm/TODO-PLATFORMS.md` §1 (ADR).

pub mod bus;
pub mod framebuffer;
pub mod timer;

pub use bus::{BusDiscover, BusKind, BusSnapshot, StaticBusSnapshot};
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
pub use bus::X86PciDiscover;
pub use framebuffer::{FramebufferSurface, SliceFramebuffer, info_32bpp, info_rgb32};
pub use timer::{GuestTimer, MainLoopTimer, MAIN_LOOP_TICK_HZ, main_loop_tick, note_main_loop_tick};
