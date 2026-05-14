// SPDX-License-Identifier: MIT OR Apache-2.0

//! Platform NIC: PCI drivers on x86 (32/64-bit); VirtIO-MMIO on AArch64.

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[path = "x86.rs"]
mod imp;
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
pub use imp::*;

#[cfg(target_arch = "aarch64")]
#[path = "aarch64.rs"]
mod imp_a64;
#[cfg(target_arch = "aarch64")]
pub use imp_a64::*;
