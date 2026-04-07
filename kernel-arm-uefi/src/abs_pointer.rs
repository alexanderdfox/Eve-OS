// SPDX-License-Identifier: MIT OR Apache-2.0

//! [`EFI_ABSOLUTE_POINTER_PROTOCOL`] — used by Apple (and some other) firmware for trackpads
//! where Simple Pointer is missing or ineffective.

use core::ptr;

use uefi::proto::unsafe_protocol;
use uefi::{Result, Status, StatusExt};
use uefi_raw::protocol::console::{
    AbsolutePointerMode, AbsolutePointerProtocol, AbsolutePointerState,
};

#[derive(Debug)]
#[repr(transparent)]
#[unsafe_protocol(AbsolutePointerProtocol::GUID)]
pub struct AbsolutePointer(AbsolutePointerProtocol);

impl AbsolutePointer {
    pub fn reset(&mut self, extended_verification: bool) -> Result {
        unsafe { (self.0.reset)(&mut self.0, extended_verification.into()) }.to_result()
    }

    /// New sample since last call, or `None` if [`Status::NOT_READY`].
    pub fn read_state(&self) -> Result<Option<AbsolutePointerState>> {
        let mut st = AbsolutePointerState::default();
        match unsafe { (self.0.get_state)(ptr::addr_of!(self.0), &mut st) } {
            Status::NOT_READY => Ok(None),
            other => other.to_result_with_val(|| Some(st)),
        }
    }

    #[must_use]
    pub fn mode(&self) -> Option<&AbsolutePointerMode> {
        unsafe { self.0.mode.as_ref() }
    }
}
