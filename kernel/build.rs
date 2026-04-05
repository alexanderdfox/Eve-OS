// SPDX-License-Identifier: MIT OR Apache-2.0

//! Embeds an FNV-1a 64-bit hash of `integrity.anchor` so the running kernel can detect
//! unexpected changes to that blob at runtime (memory corruption). Rebuilding after edits
//! updates the expected hash.

use std::env;
use std::fs;
use std::path::Path;

fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 14695981039346656037;
    for b in data {
        h ^= u64::from(*b);
        h = h.wrapping_mul(1099511628211);
    }
    h
}

fn main() {
    let dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let path = Path::new(&dir).join("integrity.anchor");
    let data = fs::read(&path).unwrap_or_else(|e| panic!("read integrity.anchor: {e}"));
    let h = fnv1a64(&data);
    println!("cargo:rustc-env=EVE_INTEGRITY_FN64={h}");
    println!("cargo:rerun-if-changed=integrity.anchor");
}
