// SPDX-License-Identifier: MIT OR Apache-2.0

//! Absolute pointer state fed by the AArch64 UEFI app (GOP + Simple Pointer). Atomic so the
//! firmware side can update without Rust `static mut` races.
//!
//! **Keyboard:** the UEFI app resets and fills [`key_events`] each frame (same thread as
//! [`crate::arm_run::main_step`]); no locks required.

use core::sync::atomic::{AtomicI32, AtomicU8, Ordering};

use crate::gfx::UiState;

/// Normalized key for the shared AArch64 UI path (UEFI Simple Text Input → Eve).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArmKeyEvent {
    /// Printable ASCII (0x20–0x7E).
    Char(u8),
    Backspace,
    Enter,
    /// F1 ..= F12
    Func(u8),
    PageUp,
    PageDown,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    /// Scroll / view to start (browser top, log top).
    Home,
    /// Scroll / view to end (browser end, log tail).
    End,
    Escape,
}

const KEY_CAP: usize = 32;
static mut KEY_BUF: [ArmKeyEvent; KEY_CAP] = [ArmKeyEvent::Char(0); KEY_CAP];
static mut KEY_LEN: usize = 0;

/// Clear queued keys; call once per frame **before** pushing new strokes from firmware.
pub fn key_queue_reset() {
    unsafe {
        KEY_LEN = 0;
    }
}

/// Append a key; drops if the queue is full.
pub fn key_queue_push(ev: ArmKeyEvent) {
    unsafe {
        if KEY_LEN < KEY_CAP {
            KEY_BUF[KEY_LEN] = ev;
            KEY_LEN += 1;
        }
    }
}

/// Keys queued for the current frame (valid until the next [`key_queue_reset`]).
#[must_use]
pub fn key_events() -> &'static [ArmKeyEvent] {
    unsafe { &KEY_BUF[..KEY_LEN] }
}

static ARM_X: AtomicI32 = AtomicI32::new(0);
static ARM_Y: AtomicI32 = AtomicI32::new(0);
static ARM_BTN: AtomicU8 = AtomicU8::new(0);

/// Latest pointer position (pixels) and button bitmask (bit0 = left), from UEFI each poll.
pub fn set_pointer_abs(x: i32, y: i32, buttons: u8) {
    ARM_X.store(x, Ordering::Relaxed);
    ARM_Y.store(y, Ordering::Relaxed);
    ARM_BTN.store(buttons, Ordering::Relaxed);
}

/// Map UEFI pointer into cursor slot 0 before click / draw merge logic.
pub fn sync_primary_cursor(state: &mut UiState, width: i32, height: i32) {
    let x = ARM_X.load(Ordering::Relaxed).clamp(0, width.saturating_sub(1).max(0));
    let y = ARM_Y.load(Ordering::Relaxed).clamp(0, height.saturating_sub(1).max(0));
    state.cursor_active[0] = true;
    state.cursor_x[0] = x;
    state.cursor_y[0] = y;
    state.cursor_btn[0] = ARM_BTN.load(Ordering::Relaxed);
    for i in 1..crate::gfx::MAX_CURSORS {
        state.cursor_active[i] = false;
    }
    state.mx = x;
    state.my = y;
}
