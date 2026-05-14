// SPDX-License-Identifier: MIT OR Apache-2.0

use std::path::PathBuf;

fn main() {
    let epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    println!("cargo:rustc-env=EVE_BUILD_UNIX_EPOCH={epoch}");

    if std::env::var("CARGO_BIN_NAME").as_deref() == Ok("kernel-i686") {
        let dir = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
        let script = dir.join("i686-link.x");
        println!(
            "cargo:rustc-link-arg=-T{}",
            script.to_string_lossy().replace('\\', "/")
        );
        println!("cargo:rerun-if-changed=i686-link.x");
    }
}
