// SPDX-License-Identifier: MIT OR Apache-2.0
//
//! Guest time source for TCP timeouts, USB poll cadence, and TLS wall-clock approximation.
//! There is no RTC in most guests; the main loop advances a monotonic tick counter.

use core::sync::atomic::{AtomicU32, Ordering};

/// Assumed main-loop cadence (~100–120 Hz in QEMU). Used for ms conversion and TLS uptime.
pub const MAIN_LOOP_TICK_HZ: u64 = 100;

static MAIN_LOOP_TICK: AtomicU32 = AtomicU32::new(0);

/// Called once per [`crate::net::NetStack::poll`] (or equivalent platform loop).
pub fn note_main_loop_tick(tick: u32) {
    MAIN_LOOP_TICK.store(tick, Ordering::Relaxed);
}

pub fn main_loop_tick() -> u32 {
    MAIN_LOOP_TICK.load(Ordering::Relaxed)
}

/// Monotonic guest timer backed by the shared main-loop tick counter.
pub trait GuestTimer {
    fn now_tick(&self) -> u32;
    fn tick_hz(&self) -> u32;

    fn elapsed_ticks_since(&self, prev: u32) -> u32 {
        self.now_tick().wrapping_sub(prev)
    }

    fn elapsed_ms_since(&self, prev: u32) -> u32 {
        let hz = self.tick_hz().max(1) as u64;
        let ticks = u64::from(self.elapsed_ticks_since(prev));
        ((ticks * 1000) / hz).min(u32::MAX as u64) as u32
    }
}

pub struct MainLoopTimer;

impl GuestTimer for MainLoopTimer {
    fn now_tick(&self) -> u32 {
        main_loop_tick()
    }

    fn tick_hz(&self) -> u32 {
        MAIN_LOOP_TICK_HZ as u32
    }
}
