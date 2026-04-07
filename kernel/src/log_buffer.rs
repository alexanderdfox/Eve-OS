// SPDX-License-Identifier: MIT OR Apache-2.0

//! Ring buffer of recent `[EVE] …` lines for the **LOG** tab (`gfx.rs`). Single-threaded kernel.

const CAP: usize = 512;
const LINE: usize = 120;

#[allow(static_mut_refs)]
static mut LINES: [[u8; LINE]; CAP] = [[0u8; LINE]; CAP];
#[allow(static_mut_refs)]
static mut LENS: [u8; CAP] = [0u8; CAP];
static mut BEGIN: usize = 0;
static mut LEN: usize = 0;
static mut DIRTY: bool = false;

/// Append one line (no CR/LF). Truncates to `LINE` bytes; control chars become spaces.
pub fn push_line(msg: &[u8]) {
    let mut buf = [0u8; LINE];
    let n = msg.len().min(LINE);
    for i in 0..n {
        let c = msg[i];
        buf[i] = if c == b'\n' || c == b'\r' || c < 32 { b' ' } else { c };
    }
    unsafe {
        let pos = if LEN < CAP {
            let p = (BEGIN + LEN) % CAP;
            LEN += 1;
            p
        } else {
            let p = BEGIN;
            BEGIN = (BEGIN + 1) % CAP;
            p
        };
        LINES[pos] = buf;
        LENS[pos] = n as u8;
        DIRTY = true;
    }
}

pub fn count() -> usize {
    unsafe { LEN }
}

/// `i` in `0..count()`, 0 = oldest line currently retained.
pub fn line_at(i: usize) -> Option<&'static [u8]> {
    unsafe {
        if i >= LEN {
            return None;
        }
        let idx = (BEGIN + i) % CAP;
        let n = LENS[idx] as usize;
        Some(&LINES[idx][..n])
    }
}

pub fn take_dirty() -> bool {
    unsafe {
        let d = DIRTY;
        DIRTY = false;
        d
    }
}
