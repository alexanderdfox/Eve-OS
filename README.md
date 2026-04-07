# Eve OS Downloads

This repo includes three ready-to-use x86 images under `build/`:

| File | Size (bytes) | Purpose |
|---|---:|---|
| `build/eve-bios.img` | `3638272` | Legacy BIOS disk image |
| `build/eve-uefi.img` | `3211264` | UEFI disk image |
| `build/eve-x86_64.iso` | `8388608` | Hybrid boot ISO (UEFI + BIOS) |

## Download (latest `main`)

```bash
curl -L -o eve-bios.img https://github.com/alexanderdfox/Eve-OS/raw/main/eve-bios.img
curl -L -o eve-uefi.img https://github.com/alexanderdfox/Eve-OS/raw/main/eve-uefi.img
curl -L -o eve-x86_64.iso https://github.com/alexanderdfox/Eve-OS/raw/main/eve-x86_64.iso
```

Current tracked artifacts in this repo are stored in `build/`, so if you are using a clone directly:

```bash
ls -l build/eve-bios.img build/eve-uefi.img build/eve-x86_64.iso
```

## Verify sizes

```bash
ls -l build/eve-bios.img build/eve-uefi.img build/eve-x86_64.iso
```

Expected:

- `build/eve-bios.img` -> `3638272`
- `build/eve-uefi.img` -> `3211264`
- `build/eve-x86_64.iso` -> `8388608`

## Optional: build locally instead of downloading

```bash
make build
```

Build output paths:

- `utm/eve-bios.img`
- `utm/eve-uefi.img`
- `utm/eve-x86_64.iso`
- `build/eve-bios.img`
- `build/eve-uefi.img`
- `build/eve-x86_64.iso`
- `build/kernel8.img`
- `build/kernel8-pi3.img`
- `build/kernel8-pi4.img`
