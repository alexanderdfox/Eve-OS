Eve — archived disk images and ISOs (versioned)
================================================

Each release is stored under its own directory:

  utm/archive/<label>/

`<label>` is normally the crate semver with an optional Git suffix, for example:

  v0.1.0
  v0.1.0+a1b2c3d4

Inside that folder, file names match the live `utm/` tree so you can compare or
swap artifacts without renaming:

  eve-bios.img          — x86 BIOS boot disk
  eve-uefi.img          — x86 UEFI boot disk (if present)
  eve-x86_64.iso        — hybrid ISO (if present)
  rpi/kernel8-pi3.img   — Raspberry Pi 3 family (if present)
  rpi/kernel8-pi4.img   — Raspberry Pi 4 / 400 (if present)
  arm-uefi/bootaa64.efi — AArch64 UEFI payload (if present)
  arm-uefi/eve-arm-uefi-fat.img — AArch64 UEFI FAT image (if present)

  MANIFEST.txt          — version, optional git revision, archive time

Create a new archive from whatever is currently under `utm/`:

  ./scripts/archive-utm-release.sh

Validate that Rust crates and scripts are sane:

  ./scripts/verify-repo.sh

Override the directory label (default = v + version from workspace Cargo.toml,
optional +<git> suffix):

  EVE_ARCHIVE_LABEL=v0.2.0-rc1 ./scripts/archive-utm-release.sh
  EVE_ARCHIVE_APPEND_GIT=0 ./scripts/archive-utm-release.sh   # no +hash

Large binaries here are gitignored like `utm/*.img`; keep archives locally or
publish them from your own release storage.
