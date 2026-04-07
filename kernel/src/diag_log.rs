// SPDX-License-Identifier: MIT OR Apache-2.0

//! Prefixed COM1 lines (115200 8N1) for bring-up — see `serial.rs`, `eve-os` `-serial stdio`.
//! Each line is also stored in `log_buffer` for the **LOG** tab.

use crate::log_buffer;
use crate::serial;

const PREFIX: &[u8] = b"[EVE] ";

fn put_u8_dec(mut v: u8) {
    let mut buf = [0u8; 3];
    let mut n = 3usize;
    loop {
        n -= 1;
        buf[n] = b'0' + (v % 10);
        v /= 10;
        if v == 0 {
            break;
        }
    }
    serial::puts(&buf[n..]);
}

fn put_u32_dec(mut v: u32) {
    if v == 0 {
        serial::put_byte(b'0');
        return;
    }
    let mut buf = [0u8; 10];
    let mut i = 10usize;
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    serial::puts(&buf[i..]);
}

fn append_store(store: &mut [u8], n: &mut usize, b: u8) {
    if *n < store.len() {
        store[*n] = b;
        *n += 1;
    }
}

fn append_store_slice(store: &mut [u8], n: &mut usize, s: &[u8]) {
    for &b in s {
        let c = if b == b'\n' || b == b'\r' || b < 32 { b' ' } else { b };
        append_store(store, n, c);
    }
}

fn append_u8_dec(store: &mut [u8], n: &mut usize, mut v: u8) {
    let mut buf = [0u8; 3];
    let mut k = 3usize;
    loop {
        k -= 1;
        buf[k] = b'0' + (v % 10);
        v /= 10;
        if v == 0 {
            break;
        }
    }
    append_store_slice(store, n, &buf[k..]);
}

fn append_u32_dec(store: &mut [u8], n: &mut usize, mut v: u32) {
    if v == 0 {
        append_store(store, n, b'0');
        return;
    }
    let mut buf = [0u8; 10];
    let mut i = 10usize;
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    append_store_slice(store, n, &buf[i..]);
}

/// One log line: `[EVE] ` + `msg` + newline.
pub fn line(msg: &[u8]) {
    serial::puts(PREFIX);
    serial::puts(msg);
    serial::puts(b"\n");
    let mut s = [0u8; 120];
    let mut n = 0usize;
    append_store_slice(&mut s, &mut n, PREFIX);
    append_store_slice(&mut s, &mut n, msg);
    log_buffer::push_line(&s[..n]);
}

/// `[EVE] ` + `a` + `b` + newline (no extra spaces).
pub fn line2(a: &[u8], b: &[u8]) {
    serial::puts(PREFIX);
    serial::puts(a);
    serial::puts(b);
    serial::puts(b"\n");
    let mut s = [0u8; 120];
    let mut n = 0usize;
    append_store_slice(&mut s, &mut n, PREFIX);
    append_store_slice(&mut s, &mut n, a);
    append_store_slice(&mut s, &mut n, b);
    log_buffer::push_line(&s[..n]);
}

/// Short error line for `net` / panic detail (bounded).
pub fn err_msg(msg: &[u8]) {
    serial::puts(PREFIX);
    serial::puts(b"err ");
    let n = msg.len().min(120);
    serial::puts(&msg[..n]);
    serial::puts(b"\n");
    let mut s = [0u8; 120];
    let mut w = 0usize;
    append_store_slice(&mut s, &mut w, PREFIX);
    append_store_slice(&mut s, &mut w, b"err ");
    append_store_slice(&mut s, &mut w, &msg[..n]);
    log_buffer::push_line(&s[..w]);
}

pub fn mac(m: &[u8; 6]) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    serial::puts(PREFIX);
    serial::puts(b"mac ");
    let mut s = [0u8; 120];
    let mut n = 0usize;
    append_store_slice(&mut s, &mut n, PREFIX);
    append_store_slice(&mut s, &mut n, b"mac ");
    for (i, &b) in m.iter().enumerate() {
        if i > 0 {
            serial::put_byte(b':');
            append_store(&mut s, &mut n, b':');
        }
        let hi = HEX[(b >> 4) as usize];
        let lo = HEX[(b & 0xf) as usize];
        serial::put_byte(hi);
        serial::put_byte(lo);
        append_store(&mut s, &mut n, hi);
        append_store(&mut s, &mut n, lo);
    }
    serial::puts(b"\n");
    log_buffer::push_line(&s[..n]);
}

pub fn ipv4(label: &[u8], a: [u8; 4]) {
    serial::puts(PREFIX);
    serial::puts(label);
    put_u8_dec(a[0]);
    serial::put_byte(b'.');
    put_u8_dec(a[1]);
    serial::put_byte(b'.');
    put_u8_dec(a[2]);
    serial::put_byte(b'.');
    put_u8_dec(a[3]);
    serial::puts(b"\n");
    let mut s = [0u8; 120];
    let mut n = 0usize;
    append_store_slice(&mut s, &mut n, PREFIX);
    append_store_slice(&mut s, &mut n, label);
    for (i, &oct) in a.iter().enumerate() {
        if i > 0 {
            append_store(&mut s, &mut n, b'.');
        }
        append_u8_dec(&mut s, &mut n, oct);
    }
    log_buffer::push_line(&s[..n]);
}

pub fn fb_wh(w: u32, h: u32) {
    serial::puts(PREFIX);
    serial::puts(b"fb ");
    put_u32_dec(w);
    serial::put_byte(b'x');
    put_u32_dec(h);
    serial::puts(b"\n");
    let mut s = [0u8; 120];
    let mut n = 0usize;
    append_store_slice(&mut s, &mut n, PREFIX);
    append_store_slice(&mut s, &mut n, b"fb ");
    append_u32_dec(&mut s, &mut n, w);
    append_store(&mut s, &mut n, b'x');
    append_u32_dec(&mut s, &mut n, h);
    log_buffer::push_line(&s[..n]);
}

/// Log resolved fetch host (truncated) and scheme.
pub fn fetch_host(host: &[u8], https: bool) {
    serial::puts(PREFIX);
    serial::puts(if https { b"fetch https://" } else { b"fetch http://" });
    let n = host.len().min(80);
    serial::puts(&host[..n]);
    serial::puts(b"\n");
    let mut s = [0u8; 120];
    let mut w = 0usize;
    append_store_slice(&mut s, &mut w, PREFIX);
    append_store_slice(
        &mut s,
        &mut w,
        if https {
            b"fetch https://"
        } else {
            b"fetch http://"
        },
    );
    append_store_slice(&mut s, &mut w, &host[..n]);
    log_buffer::push_line(&s[..w]);
}
