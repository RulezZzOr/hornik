# S19 XIL Raw SD Image Path

The safe SD package in `S19_SD_TEST.md` is a rootfs artifact. A blank SD card
needs a full raw image with S19 XIL boot assets.

This repo does not ship Bitmain bootloader or kernel files. To create a raw SD
candidate, provide a directory with known-good S19 XIL boot assets from your own
recovery media.

Expected boot asset examples:

- `BOOT.BIN` or `boot.bin`
- one of `image.ub`, `uImage`, `zImage`, `Image`, or `fit.itb`
- board device-tree files if your kernel format needs separate `.dtb` files

## Import Boot Assets

If you have a stock/recovery directory or archive, first import only the boot
files:

```bash
make boot-assets-import \
  BOOT_SOURCE=/path/to/stock-or-recovery-dir-or-archive \
  BOOT_ASSETS=boot-assets/s19-xil
```

Supported input forms:

- extracted directory,
- `.zip`,
- `.tar`, `.tar.gz`, `.tgz`, `.tar.xz`, `.txz`, `.tar.bz2`, `.tbz2`.

For raw `.img` files, mount or extract the boot partition first and pass that
mounted/extracted directory.

## Build Raw Candidate

Run this on Linux or in a Linux VM/container because it needs `sfdisk`,
`mkfs.vfat`, `mcopy`, and `mkfs.ext4`.

```bash
make sd-raw-image \
  BOARD=s19-xil \
  MODEL=s19 \
  BOOT_ASSETS=boot-assets/s19-xil
```

Output:

```text
dist/openmineros-s19-xil-s19-0.1.0-sd-raw-candidate.img
dist/openmineros-s19-xil-s19-0.1.0-sd-raw-candidate.img.xz
dist/openmineros-s19-xil-s19-0.1.0-sd-raw-candidate.json
```

The image still boots in safe first-boot mode:

- `hardware-probe`
- NAND writes disabled
- ASIC writes disabled
- Dropbear SSH on TCP `22`
- generated SSH key in the SD test package

## Write On macOS

List SD devices:

```bash
make macos-list-sd
```

Write only the whole external disk, never a partition:

```bash
OPENMINEROS_ALLOW_RAW_SD_WRITE=1 \
  make macos-write-sd \
  SD_RAW_IMAGE=dist/openmineros-s19-xil-s19-0.1.0-sd-raw-candidate.img.xz \
  SD_DISK=/dev/diskN
```

The script refuses non-raw-candidate images and refuses to run unless explicitly
armed with `OPENMINEROS_ALLOW_RAW_SD_WRITE=1`.

## First Boot Collection

```bash
MINER_HOST=<miner-ip> \
MINER_USER=root \
SSH_KEY=dist/openmineros-s19-xil-s19-0.1.0-sd-test/ssh/id_ed25519 \
make first-boot-collect
```

Do not use NAND update or mining mode until the first-boot report passes.
