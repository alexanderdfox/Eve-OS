# Eve — build all release artifacts and run QEMU targets.
# Requires: Rust nightly, targets, and tools per utm/BUILT-IMAGES.txt / scripts/build-all-images.sh
#
# Usage:
#   make build              # full image build (x86 BIOS/UEFI, RPi×2, AArch64 UEFI FAT, optional ISO)
#   make qemu-x86           # x86_64 PC + BIOS disk (same as cargo run --release)
#   make qemu-x86-uefi      # x86_64 Q35 + OVMF + UEFI disk
#   make qemu-rpi3          # AArch64 raspi3b + kernel-rpi (pi3 profile)
#   make qemu-rpi4          # AArch64 raspi4b + kernel-rpi (pi4 profile)
#   make qemu-arm-uefi      # AArch64 virt + EDK2 + FAT (rebuilds EFI via script)
#
# Default target prints help. Put ~/.cargo/bin on PATH or run from a shell where cargo works.
#
# USB install (whole disk — erases the stick; you will type YES and a sudo password):
#   make usb DISK=disk3              # hybrid ISO (UEFI + Syslinux/BIOS) — recommended
#   make usb-iso DISK=disk3          # same as usb
#   make usb-bios DISK=disk3         # legacy MBR image (utm/eve-bios.img)
#   make usb-uefi DISK=disk3        # GPT/ESP image (utm/eve-uefi.img)
# Use diskutil list (macOS) to pick the correct diskN. Linux: DISK=/dev/sdb
#
# Backup built images / ISOs under utm/archive/<label>/ (see utm/archive/README.txt):
#   make archive
#   make archive EVE_ARCHIVE_LABEL=v0.2.0-rc1

ROOT := $(abspath $(dir $(lastword $(MAKEFILE_LIST))))
export PATH := $(HOME)/.cargo/bin:$(PATH)

# /dev/disk3 stays; disk3 -> /dev/disk3
USB_DEVICE := $(if $(filter /dev/%,$(DISK)),$(DISK),/dev/$(DISK))

.PHONY: default help all build iso-x86 archive \
	qemu-x86 qemu-x86-uefi qemu-rpi3 qemu-rpi4 qemu-arm-uefi \
	qemu run-everything \
	usb usb-iso usb-bios usb-uefi

default: help

help:
	@echo "Eve Makefile (ROOT=$(ROOT))"
	@echo ""
	@echo "  make build           Full release build → utm/* (see scripts/build-all-images.sh)"
	@echo "  make all             Alias for build"
	@echo "  make iso-x86         Hybrid UEFI+BIOS ISO only (scripts/build-x86-iso.sh → utm/eve-x86_64.iso)"
	@echo "  make archive         Copy utm ship artifacts → utm/archive/<label>/ (see utm/archive/README.txt)"
	@echo "                       Optional: EVE_ARCHIVE_LABEL=…  EVE_ARCHIVE_APPEND_GIT=0"
	@echo ""
	@echo "  make qemu-x86        QEMU PC BIOS  (cargo run --release -p eve-os)"
	@echo "  make qemu-x86-uefi   QEMU Q35 UEFI (cargo run --release -p eve-os -- --uefi)"
	@echo "  make qemu-rpi3       QEMU raspi3b + scripts/run-raspi-qemu.sh pi3"
	@echo "  make qemu-rpi4       QEMU raspi4b + scripts/run-raspi-qemu.sh pi4"
	@echo "  make qemu-arm-uefi   QEMU virt + scripts/arm-uefi-run.sh"
	@echo ""
	@echo "  make run-everything  Print the qemu-* commands to run in separate terminals"
	@echo "  make qemu            Same as help"
	@echo ""
	@echo "Optional env (Pi QEMU): RPI_QEMU_NET=0  RPI_QEMU_USB_KBD=0"
	@echo ""
	@echo "Bare-metal PC: install/REAL-HARDWARE.txt (USB images + what works without QEMU)"
	@echo ""
	@echo "  make usb DISK=disk3       Flash hybrid ISO (sudo; whole disk — see utm/X86-USB-BOOT.txt)"
	@echo "  make usb-bios DISK=disk3  Flash utm/eve-bios.img (legacy)"
	@echo "  make usb-uefi DISK=disk3  Flash utm/eve-uefi.img (UEFI)"
	@echo "  (DISK=/dev/disk3 or DISK=disk3 on macOS; Linux e.g. DISK=/dev/sdb)"
	@echo "  See: install/pc-x86-64-unified-usb/INSTALL.txt"

all: build

build:
	cd "$(ROOT)" && ./scripts/build-all-images.sh

iso-x86:
	cd "$(ROOT)" && ./scripts/build-x86-iso.sh

archive:
	cd "$(ROOT)" && ./scripts/archive-utm-release.sh

# --- USB (requires DISK=…, runs x86-usb-write.sh under sudo) ---
usb: usb-iso

usb-iso:
	@test -n "$(DISK)" || (echo >&2 "error: set disk, e.g.  make usb DISK=disk3  or  DISK=/dev/disk3"; exit 1)
	cd "$(ROOT)" && sudo ./scripts/x86-usb-write.sh --iso "$(USB_DEVICE)"

usb-bios:
	@test -n "$(DISK)" || (echo >&2 "error: set disk, e.g.  make usb-bios DISK=disk3"; exit 1)
	cd "$(ROOT)" && sudo ./scripts/x86-usb-write.sh --bios "$(USB_DEVICE)"

usb-uefi:
	@test -n "$(DISK)" || (echo >&2 "error: set disk, e.g.  make usb-uefi DISK=disk3"; exit 1)
	cd "$(ROOT)" && sudo ./scripts/x86-usb-write.sh --uefi "$(USB_DEVICE)"

qemu-x86:
	cd "$(ROOT)" && cargo run --release -p eve-os

qemu-x86-uefi:
	cd "$(ROOT)" && cargo run --release -p eve-os -- --uefi

qemu-rpi3:
	cd "$(ROOT)" && ./scripts/run-raspi-qemu.sh pi3

qemu-rpi4:
	cd "$(ROOT)" && ./scripts/run-raspi-qemu.sh pi4

qemu-arm-uefi:
	cd "$(ROOT)" && ./scripts/arm-uefi-run.sh

qemu: help

run-everything:
	@echo "Build once:"
	@echo "  $(MAKE) build"
	@echo ""
	@echo "Then run each guest in its own terminal (QEMU blocks until you quit):"
	@echo "  $(MAKE) qemu-x86"
	@echo "  $(MAKE) qemu-x86-uefi"
	@echo "  $(MAKE) qemu-rpi3"
	@echo "  $(MAKE) qemu-rpi4"
	@echo "  $(MAKE) qemu-arm-uefi"
