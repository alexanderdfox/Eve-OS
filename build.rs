// SPDX-License-Identifier: MIT OR Apache-2.0

use std::path::PathBuf;

use bootloader::BootConfig;

/// Bootloader `boot.json`: modest minimum resolution so picky UEFI/VESA firmware is more likely
/// to yield *some* framebuffer; serial logging stays on for bare-metal consoles.
fn eve_boot_config() -> BootConfig {
    let mut c = BootConfig::default();
    c.frame_buffer.minimum_framebuffer_width = Some(640);
    c.frame_buffer.minimum_framebuffer_height = Some(480);
    c.serial_logging = true;
    c.frame_buffer_logging = true;
    c
}

fn main() {
    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());

    let kernel = std::env::vars_os()
        .find(|(k, _)| k.to_string_lossy().starts_with("CARGO_BIN_FILE_"))
        .map(|(_, v)| PathBuf::from(v))
        .expect("artifact dependency should set CARGO_BIN_FILE_*");

    let boot = eve_boot_config();

    let uefi_path = out_dir.join("uefi.img");
    bootloader::UefiBoot::new(&kernel)
        .set_boot_config(&boot)
        .create_disk_image(&uefi_path)
        .expect("create UEFI disk image");

    let bios_path = out_dir.join("bios.img");
    bootloader::BiosBoot::new(&kernel)
        .set_boot_config(&boot)
        .create_disk_image(&bios_path)
        .expect("create BIOS disk image");

    println!("cargo:rustc-env=UEFI_PATH={}", uefi_path.display());
    println!("cargo:rustc-env=BIOS_PATH={}", bios_path.display());
}
