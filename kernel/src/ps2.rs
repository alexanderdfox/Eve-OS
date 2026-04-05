// SPDX-License-Identifier: MIT OR Apache-2.0

//! PS/2 keyboard + mouse, polling. QEMU’s default i8042 usually delivers **scan code set 1**
//! (translation on), so mapping below is set 1 make codes with `0xF0` break prefix.
//!
//! **Mouse:** after `Set Defaults` / `Enable`, we read the device ID (`0xF2`). Scroll / 5-button
//! mice use **4-byte** movement packets; treating them as 3-byte desynchronizes the queue and
//! the pointer stops moving (common on QEMU `ps/2` and real hardware).

use crate::ports::{inb, outb};

const PS2_DATA: u16 = 0x60;
const PS2_STATUS: u16 = 0x64;
const PS2_CMD: u16 = 0x64;

const STATUS_OUT_FULL: u8 = 1;
const STATUS_IN_FULL: u8 = 2;

fn wait_write_ready() {
    for _ in 0..100_000 {
        unsafe {
            if inb(PS2_STATUS) & STATUS_IN_FULL == 0 {
                return;
            }
        }
        pause();
    }
}

fn wait_read_ready() {
    for _ in 0..100_000 {
        unsafe {
            if inb(PS2_STATUS) & STATUS_OUT_FULL != 0 {
                return;
            }
        }
        pause();
    }
}

#[inline]
fn pause() {
    unsafe {
        core::arch::asm!("pause", options(nomem, nostack, preserves_flags));
    }
}

unsafe fn write_cmd(v: u8) {
    wait_write_ready();
    outb(PS2_CMD, v);
}

unsafe fn write_data(v: u8) {
    wait_write_ready();
    outb(PS2_DATA, v);
}

unsafe fn read_data() -> u8 {
    wait_read_ready();
    inb(PS2_DATA)
}

/// Enable auxiliary port and streaming mouse; keyboard left as firmware configured it.
pub unsafe fn init() {
    for _ in 0..16 {
        if inb(PS2_STATUS) & STATUS_OUT_FULL != 0 {
            inb(PS2_DATA);
        } else {
            break;
        }
    }

    write_cmd(0xA8);

    write_cmd(0x20);
    let mut cfg = read_data();
    cfg |= 0x03;
    write_cmd(0x60);
    write_data(cfg);

    write_cmd(0xD4);
    write_data(0xF6);
    let _ = read_data();

    write_cmd(0xD4);
    write_data(0xF4);
    let _ = read_data();

    // Identify packet format: 0x00 → 3 bytes; 0x03 / 0x04 → wheel / 5-button, 4 bytes.
    write_cmd(0xD4);
    write_data(0xF2);
    let _ = read_data();
    let id = read_data();
    MOUSE_PKT_LEN = match id {
        0x03 | 0x04 => 4,
        _ => 3,
    };

    for _ in 0..8 {
        if inb(PS2_STATUS) & STATUS_OUT_FULL != 0 {
            inb(PS2_DATA);
        } else {
            break;
        }
    }
}

pub enum Ps2Event {
    Key { code: u8, shift: bool },
    /// Page scroll in Browser (negative = up).
    BrowserScroll { lines: i32 },
    Mouse { buttons: u8, dx: i16, dy: i16 },
}

static mut KBD_RELEASE: bool = false;
static mut KBD_E0_PREFIX: bool = false;
static mut KBD_EXT_RELEASE: bool = false;
static mut SHIFT_L: bool = false;
static mut SHIFT_R: bool = false;
static mut MOUSE_PHASE: u8 = 0;
static mut MOUSE_BUF: [u8; 4] = [0; 4];
static mut MOUSE_PKT_LEN: u8 = 3;

/// Pull one PS/2 event if the output buffer has data.
pub unsafe fn poll_event() -> Option<Ps2Event> {
    if inb(PS2_STATUS) & STATUS_OUT_FULL == 0 {
        return None;
    }
    let st = inb(PS2_STATUS);
    let b = inb(PS2_DATA);

    if st & 0x20 == 0 {
        if b == 0xE0 {
            KBD_E0_PREFIX = true;
            return None;
        }
        if KBD_E0_PREFIX {
            KBD_E0_PREFIX = false;
            if b == 0xF0 {
                KBD_EXT_RELEASE = true;
                return None;
            }
            return match b {
                0x48 => Some(Ps2Event::BrowserScroll { lines: -3 }),
                0x50 => Some(Ps2Event::BrowserScroll { lines: 3 }),
                _ => None,
            };
        }
        if b == 0xF0 {
            KBD_RELEASE = true;
            return None;
        }
        if KBD_RELEASE {
            KBD_RELEASE = false;
            if KBD_EXT_RELEASE {
                KBD_EXT_RELEASE = false;
                return None;
            }
            match b {
                0x2A => SHIFT_L = false,
                0x36 => SHIFT_R = false,
                _ => {}
            }
            return None;
        }
        if KBD_EXT_RELEASE {
            KBD_EXT_RELEASE = false;
            return None;
        }
        match b {
            0x2A => {
                SHIFT_L = true;
                return None;
            }
            0x36 => {
                SHIFT_R = true;
                return None;
            }
            _ => {}
        }
        let shift = SHIFT_L || SHIFT_R;
        return Some(Ps2Event::Key { code: b, shift });
    }

    let phase = MOUSE_PHASE;
    let pkt_len = MOUSE_PKT_LEN.max(3).min(4);
    if phase == 0 {
        if b & 0x08 == 0 {
            return None;
        }
        MOUSE_BUF[0] = b;
        MOUSE_PHASE = 1;
        None
    } else if phase == 1 {
        MOUSE_BUF[1] = b;
        MOUSE_PHASE = 2;
        None
    } else if phase == 2 {
        MOUSE_BUF[2] = b;
        if pkt_len > 3 {
            MOUSE_PHASE = 3;
            None
        } else {
            MOUSE_PHASE = 0;
            Some(finish_ps2_mouse_packet())
        }
    } else {
        // pkt_len == 4: wheel / Z byte (ignored for cursor; could map to scroll later).
        MOUSE_BUF[3] = b;
        MOUSE_PHASE = 0;
        Some(finish_ps2_mouse_packet())
    }
}

#[inline]
fn finish_ps2_mouse_packet() -> Ps2Event {
    unsafe {
        let flags = MOUSE_BUF[0];
        let mut dx = i16::from(MOUSE_BUF[1]);
        let mut dy = i16::from(MOUSE_BUF[2]);
        if flags & 0x10 != 0 {
            dx |= 0xFF00u16 as i16;
        }
        if flags & 0x20 != 0 {
            dy |= 0xFF00u16 as i16;
        }
        Ps2Event::Mouse {
            buttons: flags & 0x07,
            dx,
            dy: -dy,
        }
    }
}

/// Scan code **set 1** make code → ASCII. `0x08` is backspace. Shift adjusts digits/symbols.
pub fn scancode_set1_to_ascii(code: u8, shift: bool) -> Option<u8> {
    match code {
        0x1C => Some(b'\n'),
        0x0E => Some(0x08),
        0x39 => Some(b' '),
        0x02..=0x0A => Some(if shift {
            [b'!', b'@', b'#', b'$', b'%', b'^', b'&', b'*', b'('][(code - 0x02) as usize]
        } else {
            b'1' + (code - 0x02)
        }),
        0x0B => Some(if shift { b')' } else { b'0' }),
        0x0C => Some(if shift { b'_' } else { b'-' }),
        0x0D => Some(if shift { b'+' } else { b'=' }),
        0x1A => Some(if shift { b'{' } else { b'[' }),
        0x1B => Some(if shift { b'}' } else { b']' }),
        0x27 => Some(if shift { b':' } else { b';' }),
        0x28 => Some(if shift { b'"' } else { b'\'' }),
        0x29 => Some(if shift { b'~' } else { b'`' }),
        0x33 => Some(if shift { b'<' } else { b',' }),
        0x34 => Some(if shift { b'>' } else { b'.' }),
        0x35 => Some(if shift { b'?' } else { b'/' }),
        0x2B => Some(if shift { b'|' } else { b'\\' }),
        0x10 => Some(if shift { b'Q' } else { b'q' }),
        0x11 => Some(if shift { b'W' } else { b'w' }),
        0x12 => Some(if shift { b'E' } else { b'e' }),
        0x13 => Some(if shift { b'R' } else { b'r' }),
        0x14 => Some(if shift { b'T' } else { b't' }),
        0x15 => Some(if shift { b'Y' } else { b'y' }),
        0x16 => Some(if shift { b'U' } else { b'u' }),
        0x17 => Some(if shift { b'I' } else { b'i' }),
        0x18 => Some(if shift { b'O' } else { b'o' }),
        0x19 => Some(if shift { b'P' } else { b'p' }),
        0x1E => Some(if shift { b'A' } else { b'a' }),
        0x1F => Some(if shift { b'S' } else { b's' }),
        0x20 => Some(if shift { b'D' } else { b'd' }),
        0x21 => Some(if shift { b'F' } else { b'f' }),
        0x22 => Some(if shift { b'G' } else { b'g' }),
        0x23 => Some(if shift { b'H' } else { b'h' }),
        0x24 => Some(if shift { b'J' } else { b'j' }),
        0x25 => Some(if shift { b'K' } else { b'k' }),
        0x26 => Some(if shift { b'L' } else { b'l' }),
        0x2C => Some(if shift { b'Z' } else { b'z' }),
        0x2D => Some(if shift { b'X' } else { b'x' }),
        0x2E => Some(if shift { b'C' } else { b'c' }),
        0x2F => Some(if shift { b'V' } else { b'v' }),
        0x30 => Some(if shift { b'B' } else { b'b' }),
        0x31 => Some(if shift { b'N' } else { b'n' }),
        0x32 => Some(if shift { b'M' } else { b'm' }),
        _ => None,
    }
}
