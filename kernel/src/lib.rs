// SPDX-License-Identifier: MIT OR Apache-2.0

//! Eve kernel library: shared with the **x86_64** `kernel` binary, the **i686** `kernel-i686`
//! Multiboot binary, and (AArch64) UEFI payloads.
//! Hardware drivers are `target_arch`-gated; networking and UI code are common.

#![no_std]

pub mod cursor_emoji;
pub mod emoji_glyph;
pub mod diag_log;
pub mod dom;
pub mod eve_tls;
pub mod fb_info;
pub mod font;
pub mod gfx;
pub mod html;
pub mod log_buffer;
pub mod net;
pub mod net_ipv4;
pub mod nic;
pub mod power;
pub mod serial;
pub mod settings;
pub mod settings_persist;
pub mod script_runtime;
pub mod theme;
pub mod url;
pub mod usb_hid;

#[cfg(target_arch = "aarch64")]
pub mod arm_input;
#[cfg(target_arch = "aarch64")]
pub mod arm_run;
#[cfg(target_arch = "aarch64")]
pub mod virtio_mmio_net;

pub use settings::{DeviceSettings, DisplayTheme};
pub use settings_persist::BLOB_LEN;

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
pub mod bge;
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
pub mod e1000;
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
pub mod ehci;
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
pub mod ohci;
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
pub mod pcnet;
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
pub mod pci;
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
pub mod ports;
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
pub mod ps2;
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
pub mod rtl8139;
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
pub mod rtl8168;
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
pub mod uhci;
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
pub mod usb_common;
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
pub mod gpt_boot_patch;
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
pub mod virtio_blk;
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
pub mod virtio_net;
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
pub mod settings_persist_disk;
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
pub mod vmxnet3;
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
pub mod xhci;
