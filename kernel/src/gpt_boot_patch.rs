// SPDX-License-Identifier: MIT OR Apache-2.0

//! After a raw **sector clone** to the install target, make the disk more likely to boot from
//! USB / firmware menus: same intent as `scripts/x86-uefi-gpt-boot-flags.sh` (GPT ESP attributes
//! bits **0** and **2**), or **MBR active** on the first primary partition when the layout is not
//! GPT.

#![cfg(target_arch = "x86_64")]

use crate::virtio_blk::VirtioBlk;

const SIG_EFI_PART: &[u8; 8] = b"EFI PART";

/// Bit 0: required for platform; bit 2: legacy BIOS bootable (see `sgdisk -A 1:set:0` / `:set:2`).
const GPT_ATTR_BOOT_MASK: u64 = (1u64 << 0) | (1u64 << 2);

fn crc32_update(mut crc: u32, data: &[u8]) -> u32 {
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB88320;
            } else {
                crc >>= 1;
            }
        }
    }
    crc
}

fn crc32_finish(crc: u32) -> u32 {
    !crc
}

fn read_u32_le(buf: &[u8], off: usize) -> Option<u32> {
    let s = buf.get(off..off + 4)?;
    Some(u32::from_le_bytes(s.try_into().ok()?))
}

fn read_u64_le(buf: &[u8], off: usize) -> Option<u64> {
    let s = buf.get(off..off + 8)?;
    Some(u64::from_le_bytes(s.try_into().ok()?))
}

fn write_u32_le(buf: &mut [u8], off: usize, v: u32) {
    if let Some(s) = buf.get_mut(off..off + 4) {
        s.copy_from_slice(&v.to_le_bytes());
    }
}

fn write_u64_le(buf: &mut [u8], off: usize, v: u64) {
    if let Some(s) = buf.get_mut(off..off + 8) {
        s.copy_from_slice(&v.to_le_bytes());
    }
}

unsafe fn crc_partition_bytes(
    blk: &mut VirtioBlk,
    entry_lba: u64,
    total_bytes: usize,
    buf: &mut [u8; 512],
) -> Option<u32> {
    let mut crc = 0xFFFF_FFFFu32;
    let mut pos = 0usize;
    while pos < total_bytes {
        let lba = entry_lba + (pos / 512) as u64;
        let o = pos % 512;
        if !blk.read_sector(lba, buf) {
            return None;
        }
        let chunk = (512 - o).min(total_bytes - pos);
        crc = crc32_update(crc, &buf[o..o + chunk]);
        pos += chunk;
    }
    Some(crc32_finish(crc))
}

unsafe fn try_patch_gpt(blk: &mut VirtioBlk, hdr: &mut [u8; 512], ent: &mut [u8; 512]) -> bool {
    if hdr.get(0..8) != Some(SIG_EFI_PART.as_slice()) {
        return false;
    }
    let Some(hdr_size) = read_u32_le(hdr, 0x0C).map(|v| v as usize) else {
        return false;
    };
    if hdr_size < 92 || hdr_size > 512 {
        return false;
    }
    let Some(my_lba) = read_u64_le(hdr, 0x18) else {
        return false;
    };
    if my_lba != 1 {
        return false;
    }
    let Some(entry_lba) = read_u64_le(hdr, 0x48) else {
        return false;
    };
    let Some(num_entries) = read_u32_le(hdr, 0x50) else {
        return false;
    };
    let Some(entry_size) = read_u32_le(hdr, 0x54) else {
        return false;
    };
    if num_entries == 0 || entry_size < 128 || entry_size > 4096 {
        return false;
    }
    let Some(total_bytes) = (num_entries as usize).checked_mul(entry_size as usize) else {
        return false;
    };
    if total_bytes > 2 * 1024 * 1024 {
        return false;
    }

    if !blk.read_sector(entry_lba, ent) {
        return false;
    }
    if ent[0..16].iter().all(|&b| b == 0) {
        return false;
    }

    let attrs_off = 48usize;
    let cur = read_u64_le(ent, attrs_off).unwrap_or(0);
    let new_attrs = cur | GPT_ATTR_BOOT_MASK;
    if new_attrs == cur {
        return true;
    }
    write_u64_le(ent, attrs_off, new_attrs);
    if !blk.write_sector(entry_lba, ent) {
        return false;
    }

    let Some(part_crc) = crc_partition_bytes(blk, entry_lba, total_bytes, ent) else {
        return false;
    };
    if !blk.read_sector(1, hdr) {
        return false;
    }
    write_u32_le(hdr, 0x58, part_crc);
    write_u32_le(hdr, 0x10, 0);

    let mut tmp = [0u8; 128];
    if hdr_size > tmp.len() {
        return false;
    }
    tmp[..hdr_size].copy_from_slice(&hdr[..hdr_size]);
    tmp[0x10..0x14].fill(0);
    let hc = crc32_finish(crc32_update(0xFFFF_FFFF, &tmp[..hdr_size]));
    write_u32_le(hdr, 0x10, hc);

    blk.write_sector(1, hdr)
}

unsafe fn try_patch_mbr(blk: &mut VirtioBlk, buf: &mut [u8; 512]) -> bool {
    if !blk.read_sector(0, buf) {
        return false;
    }
    if buf[510] != 0x55 || buf[511] != 0xAA {
        return false;
    }
    if buf[0x1C2] == 0xEE {
        return false;
    }
    for i in 0..4 {
        buf[0x1BE + i * 16] = 0;
    }
    buf[0x1BE] = 0x80;
    blk.write_sector(0, buf)
}

/// Best-effort boot flags on the **destination** disk after a full clone.
///
/// Returns `true` if GPT was recognized (patched or already had bits set) or MBR active was set.
pub fn patch_install_target_boot(blk: &mut VirtioBlk) -> bool {
    if blk.sector_size != 512 {
        return false;
    }
    let mut hdr = [0u8; 512];
    let mut ent = [0u8; 512];
    if !unsafe { blk.read_sector(1, &mut hdr) } {
        return false;
    }
    if hdr.get(0..8) == Some(SIG_EFI_PART.as_slice()) {
        return unsafe { try_patch_gpt(blk, &mut hdr, &mut ent) };
    }
    unsafe { try_patch_mbr(blk, &mut hdr) }
}
