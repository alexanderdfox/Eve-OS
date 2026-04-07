After running scripts/populate-from-repo.sh from the Eve repository, this directory
will contain BOOTAA64.EFI (the AArch64 UEFI demo binary).

If you copied this bundle by hand, copy your built bootaa64.efi from:

  utm/arm-uefi/bootaa64.efi   (repo path after ./scripts/arm-uefi-sync.sh)

to:

  EFI/EVE/BOOTAA64.EFI        (name must match; FAT is case-insensitive)
