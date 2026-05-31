// SPDX-License-Identifier: MIT OR Apache-2.0
//
//! AArch64 / non-x86 power: optional hooks from the UEFI app (`ResetSystem`); spin if unset.

type PowerFn = unsafe fn();

static mut HOOK_REBOOT: Option<PowerFn> = None;
static mut HOOK_SHUTDOWN: Option<PowerFn> = None;

/// UEFI payload registers firmware reset handlers; `None` clears.
pub unsafe fn register_hooks(reboot: Option<PowerFn>, shutdown: Option<PowerFn>) {
    HOOK_REBOOT = reboot;
    HOOK_SHUTDOWN = shutdown;
}

pub unsafe fn system_reboot() {
    if let Some(f) = HOOK_REBOOT {
        f();
    }
    halt_forever();
}

pub unsafe fn system_shutdown() {
    if let Some(f) = HOOK_SHUTDOWN {
        f();
    }
    halt_forever();
}

pub fn halt_forever() -> ! {
    loop {
        core::hint::spin_loop();
    }
}
