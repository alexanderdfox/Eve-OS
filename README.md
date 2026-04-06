# Eve OS Downloads

This repo includes three ready-to-use x86 images:

| File | Size (bytes) | Purpose |
|---|---:|---|
| `eve-bios.img` | `3638272` | Legacy BIOS disk image |
| `eve-uefi.img` | `3211264` | UEFI disk image |
| `eve-x86_64.iso` | `8388608` | Hybrid boot ISO (UEFI + BIOS) |

## Download (latest `main`)

```bash
curl -L -o eve-bios.img https://github.com/alexanderdfox/Eve-OS/raw/main/eve-bios.img
curl -L -o eve-uefi.img https://github.com/alexanderdfox/Eve-OS/raw/main/eve-uefi.img
curl -L -o eve-x86_64.iso https://github.com/alexanderdfox/Eve-OS/raw/main/eve-x86_64.iso
```

## Verify sizes

```bash
ls -l eve-bios.img eve-uefi.img eve-x86_64.iso
```

Expected:

- `eve-bios.img` -> `3638272`
- `eve-uefi.img` -> `3211264`
- `eve-x86_64.iso` -> `8388608`

## Optional: build locally instead of downloading

```bash
make build
```

Build output paths:

- `utm/eve-bios.img`
- `utm/eve-uefi.img`
- `utm/eve-x86_64.iso`
