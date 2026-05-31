// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::fb_info::{FrameBufferInfo, PixelFormat};

/// CPU-writable linear framebuffer used by `gfx` and `emoji_glyph`.
pub trait FramebufferSurface {
    fn info(&self) -> FrameBufferInfo;
    fn buffer_mut(&mut self) -> &mut [u8];

    fn width(&self) -> i32 {
        self.info().width as i32
    }

    fn height(&self) -> i32 {
        self.info().height as i32
    }
}

/// Borrowed pixel buffer + layout (x86 bootloader GOP, UEFI shadow, Pi mailbox mapping).
pub struct SliceFramebuffer<'a> {
    info: FrameBufferInfo,
    buf: &'a mut [u8],
}

impl<'a> SliceFramebuffer<'a> {
    pub fn new(info: FrameBufferInfo, buf: &'a mut [u8]) -> Self {
        Self { info, buf }
    }
}

impl FramebufferSurface for SliceFramebuffer<'_> {
    fn info(&self) -> FrameBufferInfo {
        self.info
    }

    fn buffer_mut(&mut self) -> &mut [u8] {
        self.buf
    }
}

/// Common 32 bpp layout for GOP, Pi mailbox, and UEFI shadow buffers.
/// `stride_pixels` matches [`bootloader_api::info::FrameBufferInfo::stride`] (pixels per line).
pub const fn info_32bpp(
    width: usize,
    height: usize,
    stride_pixels: usize,
    pixel_format: PixelFormat,
) -> FrameBufferInfo {
    let bytes_per_pixel = 4;
    #[cfg_attr(not(target_arch = "x86_64"), allow(unused_variables))]
    let byte_len = stride_pixels
        .saturating_mul(height)
        .saturating_mul(bytes_per_pixel);
    FrameBufferInfo {
        #[cfg(target_arch = "x86_64")]
        byte_len,
        width,
        height,
        stride: stride_pixels,
        pixel_format,
        bytes_per_pixel,
    }
}

/// Shorthand when the active mode is 32 bpp RGB (Pi mailbox `TAG_SET_ORDER`).
pub const fn info_rgb32(width: usize, height: usize, stride_pixels: usize) -> FrameBufferInfo {
    info_32bpp(width, height, stride_pixels, PixelFormat::Rgb)
}
