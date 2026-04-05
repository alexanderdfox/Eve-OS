// SPDX-License-Identifier: MIT OR Apache-2.0

//! Runtime check: FNV-1a of `integrity.anchor` must match the value `build.rs` embedded at
//! compile time. Mismatch → [`core::panic!`] → halt loop (see `panic_handler`).
//!
//! This catches some RAM corruption or accidental overwrite of the included bytes; it does **not**
//! prove the boot image was unmodified on disk (an attacker could patch the binary and rebuild).

const ANCHOR: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/integrity.anchor"));

fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 14695981039346656037;
    for b in data {
        h ^= u64::from(*b);
        h = h.wrapping_mul(1099511628211);
    }
    h
}

#[inline]
pub fn verify_anchor() {
    let expected: u64 = env!("EVE_INTEGRITY_FN64")
        .parse()
        .expect("EVE_INTEGRITY_FN64 must be a decimal u64 from build.rs");
    let got = fnv1a64(ANCHOR);
    if got != expected {
        panic!("integrity anchor mismatch");
    }
}
