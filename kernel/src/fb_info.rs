// SPDX-License-Identifier: MIT OR Apache-2.0

//! Framebuffer layout used by `gfx` — on x86 re-exported from `bootloader_api`; on AArch64 a
//! local copy so the kernel library builds without the PC bootloader crate.

#[cfg(target_arch = "x86_64")]
pub use bootloader_api::info::{FrameBufferInfo, PixelFormat};

#[cfg(not(target_arch = "x86_64"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelFormat {
    Rgb,
    Bgr,
    U8,
    Unknown,
}

#[cfg(not(target_arch = "x86_64"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameBufferInfo {
    pub width: usize,
    pub height: usize,
    pub stride: usize,
    pub pixel_format: PixelFormat,
    pub bytes_per_pixel: usize,
}
