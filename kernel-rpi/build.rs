// SPDX-License-Identifier: MIT OR Apache-2.0

use std::path::PathBuf;

fn main() {
    let ld = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("link.ld");
    println!("cargo:rustc-link-arg=-T{}", ld.display());
    println!("cargo:rerun-if-changed=link.ld");
}
