// SPDX-License-Identifier: MIT OR Apache-2.0

fn main() {
    let epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    println!("cargo:rustc-env=EVE_BUILD_UNIX_EPOCH={epoch}");
}
