// SPDX-License-Identifier: MIT OR Apache-2.0
//
// 32-bit **Multiboot v1** entry (QEMU: `qemu-system-i386 -kernel target/.../kernel-i686`).
// The full Eve UI remains on the **x86_64** `kernel` binary (`bootloader_api`). Grow this path
// for i686 framebuffer + PCI parity with the 64-bit bring-up.

#![no_std]
#![no_main]

use core::arch::asm;

const MULTIBOOT_MAGIC: u32 = 0x1BADB002;
const MULTIBOOT_FLAGS: u32 = 0;
const MULTIBOOT_CHECKSUM: u32 = 0u32.wrapping_sub(MULTIBOOT_MAGIC.wrapping_add(MULTIBOOT_FLAGS));

#[repr(C, packed)]
struct MultibootHeader {
    magic: u32,
    flags: u32,
    checksum: u32,
}

#[used]
#[link_section = ".multiboot"]
static MULTIBOOT: MultibootHeader = MultibootHeader {
    magic: MULTIBOOT_MAGIC,
    flags: MULTIBOOT_FLAGS,
    checksum: MULTIBOOT_CHECKSUM,
};

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {
        unsafe {
            asm!("cli; hlt", options(nomem, nostack));
        }
    }
}

const BOOT_STACK_LEN: usize = 32 * 1024;

static mut BOOT_STACK: [u8; BOOT_STACK_LEN] = [0u8; BOOT_STACK_LEN];

#[no_mangle]
pub extern "C" fn _start() -> ! {
    unsafe {
        let sp = (core::ptr::addr_of_mut!(BOOT_STACK) as *mut u8).byte_add(BOOT_STACK_LEN) as u32;
        asm!("mov esp, {0:e}", in(reg) sp, options(nostack, nomem));
        kernel::serial::init();
        kernel::serial::puts(b"\r\n=== EVE OS (i686) ===\r\n");
        kernel::serial::puts(
            b"32-bit Multiboot kernel OK. Full UI is x86_64 + bootloader_api; extend this entry.\r\n",
        );
        kernel::power::halt_forever()
    }
}
