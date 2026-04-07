// SPDX-License-Identifier: MIT OR Apache-2.0

//! Platform NIC: PCI drivers on x86_64; VirtIO-MMIO on AArch64.

#[cfg(target_arch = "x86_64")]
#[path = "x86.rs"]
mod imp;
#[cfg(target_arch = "x86_64")]
pub use imp::*;

#[cfg(target_arch = "aarch64")]
#[path = "aarch64.rs"]
mod imp_a64;
#[cfg(target_arch = "aarch64")]
pub use imp_a64::*;
