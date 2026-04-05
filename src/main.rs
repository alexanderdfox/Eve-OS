// SPDX-License-Identifier: MIT OR Apache-2.0

use std::process::Command;

fn append_qemu_audio(cmd: &mut Command) {
    #[cfg(target_os = "macos")]
    cmd.args(["-audiodev", "coreaudio,id=eve0"]);
    #[cfg(target_os = "linux")]
    cmd.args(["-audiodev", "alsa,id=eve0"]);
    #[cfg(target_os = "windows")]
    cmd.args(["-audiodev", "dsound,id=eve0"]);
    #[cfg(not(any(
        target_os = "macos",
        target_os = "linux",
        target_os = "windows"
    )))]
    cmd.args(["-audiodev", "none,id=eve0"]);
    cmd.args([
        "-device",
        "intel-hda",
        "-device",
        "hda-duplex,audiodev=eve0",
    ]);
}

fn main() {
    let uefi_path = env!("UEFI_PATH");
    let bios_path = env!("BIOS_PATH");

    let use_uefi = std::env::args()
        .nth(1)
        .map(|a| a == "--uefi" || a == "-u")
        .unwrap_or(false);

    let mut cmd = Command::new("qemu-system-x86_64");
    cmd.arg("-device").arg("virtio-net-pci,netdev=n0");
    cmd.arg("-netdev").arg("user,id=n0");
    append_qemu_audio(&mut cmd);
    cmd.arg("-usb");
    cmd.arg("-device").arg("usb-kbd");
    cmd.arg("-device").arg("usb-mouse");
    if use_uefi {
        // OVMF expects a chipset with proper PCI hierarchy; i440fx often breaks GOP / boot.
        cmd.arg("-machine").arg("q35");
        cmd.arg("-bios").arg(ovmf_prebuilt::ovmf_pure_efi());
        cmd.arg("-drive")
            .arg(format!("format=raw,file={uefi_path}"));
    } else {
        cmd.arg("-drive")
            .arg(format!("format=raw,file={bios_path}"));
    }
    let status = cmd.status().expect("spawn qemu-system-x86_64");
    std::process::exit(status.code().unwrap_or(1));
}
