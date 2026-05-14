// SPDX-License-Identifier: MIT OR Apache-2.0

//! Eve kernel library: shared with the **x86_64** `kernel` binary and (AArch64) UEFI payloads.
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

#[cfg(target_arch = "x86_64")]
pub mod bge;
#[cfg(target_arch = "x86_64")]
pub mod e1000;
#[cfg(target_arch = "x86_64")]
pub mod ehci;
#[cfg(target_arch = "x86_64")]
pub mod ohci;
#[cfg(target_arch = "x86_64")]
pub mod pcnet;
#[cfg(target_arch = "x86_64")]
pub mod pci;
#[cfg(target_arch = "x86_64")]
pub mod ports;
#[cfg(target_arch = "x86_64")]
pub mod ps2;
#[cfg(target_arch = "x86_64")]
pub mod rtl8139;
#[cfg(target_arch = "x86_64")]
pub mod rtl8168;
#[cfg(target_arch = "x86_64")]
pub mod uhci;
#[cfg(target_arch = "x86_64")]
pub mod usb_common;
#[cfg(target_arch = "x86_64")]
pub mod gpt_boot_patch;
#[cfg(target_arch = "x86_64")]
pub mod virtio_blk;
#[cfg(target_arch = "x86_64")]
pub mod virtio_net;
#[cfg(target_arch = "x86_64")]
pub mod vmxnet3;
#[cfg(target_arch = "x86_64")]
pub mod xhci;
