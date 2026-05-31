// SPDX-License-Identifier: MIT OR Apache-2.0
//
//! x86 guest settings persistence on the **last 512-byte sector** of the first VirtIO block device.
//! Uses the same EVS2 blob as UEFI NVRAM (`settings_persist`). Safe when `capacity >= 1`.

use crate::settings::DeviceSettings;
use crate::settings_persist;
use crate::virtio_blk::VirtioBlk;

/// Load settings from LBA `capacity - 1`. Returns true when a valid EVS2 blob was merged.
pub unsafe fn load(blk: &mut VirtioBlk, settings: &mut DeviceSettings) -> bool {
    if blk.capacity == 0 {
        return false;
    }
    let lba = blk.capacity - 1;
    let mut sector = [0u8; 512];
    if !blk.read_sector(lba, &mut sector) {
        return false;
    }
    settings_persist::decode_merge(settings, &sector)
}

/// Write current settings to LBA `capacity - 1`.
pub unsafe fn save(blk: &mut VirtioBlk, settings: &DeviceSettings) -> bool {
    if blk.capacity == 0 {
        return false;
    }
    let lba = blk.capacity - 1;
    let mut sector = [0u8; 512];
    settings_persist::encode(settings, &mut sector);
    blk.write_sector(lba, &sector)
}
