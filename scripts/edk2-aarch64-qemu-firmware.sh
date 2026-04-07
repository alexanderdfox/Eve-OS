#!/usr/bin/env bash
# SPDX-License-Identifier: MIT OR Apache-2.0
#
# Resolve QEMU "virt" AArch64 UEFI pflash firmware (code + vars template).
# Sourced by arm-uefi-run.sh and arm-uefi-boot-img.sh.
#
# Override explicitly:
#   EDK2_AARCH64_CODE=/path/to/code.fd EDK2_AARCH64_VARS=/path/to/vars-template.fd
#
# Distros:
#   - Homebrew / many builds: .../share/qemu/edk2-aarch64-code.fd + edk2-*-vars.fd
#   - Debian/Ubuntu: qemu-efi-aarch64 → /usr/share/AAVMF/AAVMF_{CODE,VARS}.fd
#   - Fedora: edk2-aarch64 → /usr/share/edk2/aarch64/QEMU_EFI-pflash.raw + vars-template-pflash.raw
#
# shellcheck shell=bash

resolve_edk2_aarch64_qemu_firmware() {
    CODE=""
    VARS_SRC=""

    if [[ -n "${EDK2_AARCH64_CODE:-}" && -f "$EDK2_AARCH64_CODE" ]]; then
        CODE=$EDK2_AARCH64_CODE
        if [[ -n "${EDK2_AARCH64_VARS:-}" && -f "$EDK2_AARCH64_VARS" ]]; then
            VARS_SRC=$EDK2_AARCH64_VARS
            return 0
        fi
    fi

    local -a try_dirs=()
    if [[ -n "${QEMU_SHARE:-}" && -d "$QEMU_SHARE" ]]; then
        try_dirs+=("$QEMU_SHARE")
    fi
    try_dirs+=(
        /opt/homebrew/share/qemu
        /usr/local/share/qemu
        /usr/share/qemu
    )

    local d
    for d in "${try_dirs[@]}"; do
        [[ -d "$d" ]] || continue
        if [[ -f "$d/edk2-aarch64-code.fd" ]]; then
            CODE=$d/edk2-aarch64-code.fd
            if [[ -f "$d/edk2-aarch64-vars.fd" ]]; then
                VARS_SRC=$d/edk2-aarch64-vars.fd
                return 0
            fi
            if [[ -f "$d/edk2-arm-vars.fd" ]]; then
                VARS_SRC=$d/edk2-arm-vars.fd
                return 0
            fi
        fi
    done

    # Debian/Ubuntu: apt install qemu-efi-aarch64
    if [[ -f /usr/share/AAVMF/AAVMF_CODE.fd && -f /usr/share/AAVMF/AAVMF_VARS.fd ]]; then
        CODE=/usr/share/AAVMF/AAVMF_CODE.fd
        VARS_SRC=/usr/share/AAVMF/AAVMF_VARS.fd
        return 0
    fi

    # Fedora / RHEL: dnf install edk2-aarch64
    if [[ -f /usr/share/edk2/aarch64/QEMU_EFI-pflash.raw && -f /usr/share/edk2/aarch64/vars-template-pflash.raw ]]; then
        CODE=/usr/share/edk2/aarch64/QEMU_EFI-pflash.raw
        VARS_SRC=/usr/share/edk2/aarch64/vars-template-pflash.raw
        return 0
    fi

    return 1
}

edk2_aarch64_firmware_hint() {
    echo "AArch64 UEFI firmware for QEMU was not found." >&2
    echo "  Fedora / Asahi:  sudo dnf install edk2-aarch64 qemu-system-aarch64" >&2
    echo "  Debian/Ubuntu:   sudo apt install qemu-system-arm qemu-efi-aarch64" >&2
    echo "  macOS:           brew install qemu" >&2
    echo "  Or set EDK2_AARCH64_CODE and EDK2_AARCH64_VARS to the code and vars template files." >&2
}
