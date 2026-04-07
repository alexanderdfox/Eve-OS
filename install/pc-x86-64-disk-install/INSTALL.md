Eve OS — install to a second VirtIO disk (QEMU / VM)
====================================================

The x86_64 kernel can **clone the boot disk onto a second `virtio-blk` disk**
when both are present. This is meant for **virtual machines** (two VirtIO
disks). Typical PCs do **not** expose VirtIO block devices; use USB images and
`scripts/x86-usb-write.sh` on another machine for bare metal (see
`install/REAL-HARDWARE.md`).

What you get
------------
  • After boot, the **INSTALL** tab is selected automatically when two VirtIO
    disks are detected.
  • One click on the green **INSTALL** button copies sector **0 .. min(capacity)**
    from the **first** enumerated VirtIO disk to the **second** (full overwrite
    of the overlapping range on disk 2).
  • When finished, point the VM at **only** the target image (or swap boot
    order) and boot Eve from the clone.

Requirements
------------
  • **512-byte logical sectors** on both VirtIO disks (QEMU’s default).
  • Target disk must **not** be VirtIO read-only.
  • Guest must have been started with **two** VirtIO disks; the **boot** disk
    must be VirtIO as well so the kernel can read the source (there is no IDE
    block driver in-tree).

Quick start (repo host QEMU)
----------------------------
1) Build once:

     ./scripts/build-all-images.sh
   or `cargo build --release -p eve-os`

2) Create an empty raw target (size ≥ boot image; 96 MiB is usually enough):

     qemu-img create -f raw utm/eve-install-target.raw 96M

3) Run QEMU with the install layout:

     EVE_QEMU_INSTALL_TARGET="$(pwd)/utm/eve-install-target.raw" \
       cargo run --release -p eve-os

   This boots `bios.img` from **VirtIO disk 0** and attaches **VirtIO disk 1**
   as the install target (`src/main.rs`).

4) In the guest, click **INSTALL** once. Wait for **DONE**, then quit QEMU and
   boot with **only** `eve-install-target.raw` as the VirtIO disk (or set that
   disk first in firmware), e.g.:

     qemu-system-x86_64 -m 512M -vga std -machine pc,accel=tcg \
       -drive if=virtio,format=raw,file=utm/eve-install-target.raw

Keyboard shortcuts (optional)
-----------------------------
  • **F4** (PS/2 scancode path) or USB **F4** (usage 0x3D): open **INSTALL** tab
    when two disks are present.

See also: `utm/UTM-SETUP.md`, `Makefile` target `qemu-x86-install`.
