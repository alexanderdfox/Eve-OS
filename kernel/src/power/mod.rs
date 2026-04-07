// SPDX-License-Identifier: MIT OR Apache-2.0

//! Guest power: x86 ACPI/PS/2 reset vs AArch64 stub.

#[cfg(target_arch = "x86_64")]
mod x86;
#[cfg(target_arch = "x86_64")]
pub use x86::*;

#[cfg(not(target_arch = "x86_64"))]
mod stub;
#[cfg(not(target_arch = "x86_64"))]
pub use stub::*;
