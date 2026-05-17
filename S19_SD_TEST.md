# S19 SD First-Boot Test

This is the safe hardware test path for `0.1.0`. It is read-only first boot
validation, not NAND flashing and not full mining.

## Prepare Package

```bash
make sd-test-package BOARD=s19-xil MODEL=s19j-pro MEDIA=sd
```

The package is created under:

```text
dist/openmineros-s19-xil-s19j-pro-0.1.0-sd-test
```

## Verify Package

```bash
make sd-test-verify BOARD=s19-xil MODEL=s19j-pro
```

This checks:

- manifest verification,
- artifact budget,
- rootfs contents,
- `hardware-probe` backend,
- first boot safe mode,
- NAND writes disabled,
- ASIC writes disabled,
- flashable flag disabled.

## Stage Rootfs To SD

The current `sd-card.img.xz` artifact is a compressed rootfs tar model, not a
raw partitioned image. Extract it only onto an already prepared and mounted SD
rootfs.

```bash
OPENMINEROS_ALLOW_SD_ROOTFS_EXTRACT=1 \
  ./scripts/extract-sd-test-rootfs.sh \
  dist/openmineros-s19-xil-s19j-pro-0.1.0-sd-card.img.xz \
  /Volumes/OPENMINEROS_ROOT
```

Do not run `dd` with this artifact.

## First Boot

Boot the miner from SD. Do not run NAND update tools manually. The launcher
forces:

```text
OPENMINEROS_BACKEND=hardware-probe
OPENMINEROS_FIRST_BOOT_SAFE=1
OPENMINEROS_DISABLE_NAND_WRITES=1
OPENMINEROS_DISABLE_ASIC_WRITES=1
OPENMINEROS_ALLOW_UNSAFE_HARDWARE=0
```

On the miner, a local check is available:

```bash
/usr/bin/openmineros-safe-self-test
/usr/bin/openmineros-first-boot-report
```

## Collect Report From Host

```bash
MINER_HOST=<miner-ip> MINER_USER=root make first-boot-collect
```

The collector writes:

```text
dist/first-boot-reports/<host>-<timestamp>
dist/first-boot-reports/<host>-<timestamp>.tar.gz
```

## Pass Criteria

Required values from `/api/v1/firmware/anti-brick`:

- `safe_to_first_boot=true`
- `nand_writes_allowed=false`
- `asic_writes_allowed=false`
- `flashing_allowed=false`

Required NAND guard behavior:

- `/usr/bin/openmineros-nand-update` exits with code `78`

Stop testing immediately if any of those checks fail.
