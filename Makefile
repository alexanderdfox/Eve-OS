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

ROOT := $(abspath $(dir $(lastword $(MAKEFILE_LIST))))
export PATH := $(HOME)/.cargo/bin:$(PATH)

.PHONY: default help all build \
	qemu-x86 qemu-x86-uefi qemu-rpi3 qemu-rpi4 qemu-arm-uefi \
	qemu run-everything

default: help

help:
	@echo "Eve Makefile (ROOT=$(ROOT))"
	@echo ""
	@echo "  make build           Full release build → utm/* (see scripts/build-all-images.sh)"
	@echo "  make all             Alias for build"
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

all: build

build:
	cd "$(ROOT)" && ./scripts/build-all-images.sh

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
