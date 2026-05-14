// SPDX-License-Identifier: MIT OR Apache-2.0

//! Guest power: x86 ACPI/PS/2 reset vs AArch64 stub.

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
mod x86;
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
pub use x86::*;

#[cfg(not(any(target_arch = "x86_64", target_arch = "x86")))]
mod stub;
#[cfg(not(any(target_arch = "x86_64", target_arch = "x86")))]
pub use stub::*;
