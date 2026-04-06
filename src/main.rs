// SPDX-License-Identifier: MIT OR Apache-2.0

use std::process::Command;

/// `pc` (i440FX): KVM on Linux x86_64 hosts, WHPX on Windows, TCG elsewhere.
/// (HVF is only for AArch64 guests; it is invalid for `qemu-system-x86_64` on Apple Silicon.)
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const MACHINE_PC: &str = "pc,accel=kvm:tcg";
#[cfg(all(target_os = "linux", not(target_arch = "x86_64")))]
const MACHINE_PC: &str = "pc,accel=tcg";
#[cfg(target_os = "windows")]
const MACHINE_PC: &str = "pc,accel=whpx:tcg";
#[cfg(not(any(target_os = "linux", target_os = "windows")))]
const MACHINE_PC: &str = "pc,accel=tcg";

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const MACHINE_Q35: &str = "q35,accel=kvm:tcg";
#[cfg(all(target_os = "linux", not(target_arch = "x86_64")))]
const MACHINE_Q35: &str = "q35,accel=tcg";
#[cfg(target_os = "windows")]
const MACHINE_Q35: &str = "q35,accel=whpx:tcg";
#[cfg(not(any(target_os = "linux", target_os = "windows")))]
const MACHINE_Q35: &str = "q35,accel=tcg";

fn append_qemu_audio(cmd: &mut Command, q35: bool) {
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
    // ICH9 HDA matches Q35; ICH6 `intel-hda` is the usual pairing for i440FX `pc`.
    let hda = if q35 {
        "ich9-intel-hda"
    } else {
        "intel-hda"
    };
    // Playback only: `hda-duplex` opens an ADC voice; CoreAudio often warns/fails on `adc`.
    cmd.args(["-device", hda, "-device", "hda-output,audiodev=eve0"]);
}

fn main() {
    let uefi_path = env!("UEFI_PATH");
    let bios_path = env!("BIOS_PATH");

    let use_uefi = std::env::args()
        .nth(1)
        .map(|a| a == "--uefi" || a == "-u")
        .unwrap_or(false);

    let mut cmd = Command::new("qemu-system-x86_64");
    // Networking: virtio-net-pci + user NAT (guest 10.0.2.15, gateway .2 — matches kernel net stack).
    // Input: PS/2 default; optional UHCI usb-kbd / usb-mice when USB poll is ON in SYS (see kernel).
    // `EVE_QEMU_M` overrides RAM (default 512M; e.g. 1024M helps TCG on Apple Silicon hosts).
    let guest_ram = std::env::var("EVE_QEMU_M").unwrap_or_else(|_| "512M".to_string());
    cmd.arg("-m").arg(&guest_ram);
    cmd.args(["-vga", "std", "-name", "eve-os"]);
    if use_uefi {
        cmd.arg("-machine").arg(MACHINE_Q35);
    } else {
        cmd.arg("-machine").arg(MACHINE_PC);
    }
    cmd.arg("-device").arg("virtio-net-pci,netdev=n0");
    // SLIRP user NAT must match `kernel/src/net.rs` (10.0.2.15 / .2 / .3). Explicit `net=` keeps
    // QEMU and UTM “Shared”/extra-args setups aligned. Override with full `-netdev` *value* via
    // `EVE_QEMU_NETDEV` (e.g. same string without the `-netdev` prefix) only if you know it is compatible.
    let net_backend = std::env::var_os("EVE_QEMU_NETDEV").filter(|s| !s.is_empty());
    if let Some(ref v) = net_backend {
        cmd.arg("-netdev").arg(v);
    } else {
        cmd.arg("-netdev").arg(
            "user,id=n0,ipv6=off,net=10.0.2.0/24,host=10.0.2.2,restrict=off",
        );
    }
    append_qemu_audio(&mut cmd, use_uefi);
    cmd.arg("-usb");
    // Root port 1: 8-port hub — mice on 1.1..1.7, second hub on 1.8 with five more mice (12 total).
    cmd.arg("-device").arg("usb-hub,bus=usb-bus.0,port=1,ports=8");
    for p in 1..=7 {
        cmd.arg("-device")
            .arg(format!("usb-mouse,bus=usb-bus.0,port=1.{p}"));
    }
    cmd.arg("-device").arg("usb-hub,bus=usb-bus.0,port=1.8,ports=8");
    for p in 1..=5 {
        cmd.arg("-device")
            .arg(format!("usb-mouse,bus=usb-bus.0,port=1.8.{p}"));
    }
    cmd.arg("-device").arg("usb-kbd,bus=usb-bus.0,port=2");
    // Two VirtIO disks: boot image + empty target — enables in-guest "INSTALL" tab (see install/pc-x86-64-disk-install/).
    let install_target = std::env::var_os("EVE_QEMU_INSTALL_TARGET").filter(|s| !s.is_empty());
    if let Some(ref tgt) = install_target {
        let path = tgt.to_str().expect("EVE_QEMU_INSTALL_TARGET must be valid UTF-8");
        if use_uefi {
            cmd.arg("-bios").arg(ovmf_prebuilt::ovmf_pure_efi());
            cmd.arg("-drive")
                .arg(format!("if=virtio,index=0,format=raw,file={uefi_path}"));
            cmd.arg("-drive")
                .arg(format!("if=virtio,index=1,format=raw,file={path}"));
        } else {
            cmd.arg("-drive")
                .arg(format!("if=virtio,index=0,format=raw,file={bios_path}"));
            cmd.arg("-drive")
                .arg(format!("if=virtio,index=1,format=raw,file={path}"));
        }
    } else if use_uefi {
        // OVMF expects a chipset with proper PCI hierarchy; i440fx often breaks GOP / boot.
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
