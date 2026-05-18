# Build

Build `0.1.0` provides local development builds, tests, and non-flashable board
artefacts with a packaged device-side runtime binary, launch script, and boot
hooks.

## Requirements

- Rust toolchain
- `make`
- POSIX shell

## Commands

```bash
make bootstrap
make fmt
make clippy
make test
make size-budget
make artifact-budget
make run-control-plane BOARD=s19-xil MODEL=s19j-pro
cargo run -p openmineros-commander -- validate-config --config config/default.toml
make image BOARD=s19-xil MODEL=s19j-pro
make image BOARD=s19-xil MODEL=s19j-pro MEDIA=sd
make image BOARD=s19-xil MODEL=s19j-pro MEDIA=nand
OPENMINEROS_RUNTIME_TARGET=native make image BOARD=s19-xil MODEL=s19j-pro
make install-preflight BOARD=s19-xil MODEL=s19j-pro
make verify-repro BOARD=s19-xil MODEL=s19j-pro
make sd-test-package BOARD=s19-xil MODEL=s19j-pro
make sd-test-verify BOARD=s19-xil MODEL=s19j-pro
make sd-raw-image BOARD=s19-xil MODEL=s19 BOOT_ASSETS=/path/to/s19-xil-boot-assets
cargo run -p openmineros-commander -- verify-manifest \
  --manifest dist/openmineros-s19-xil-s19j-pro-0.1.0-manifest.json
```

`make sd-test-package` generates a local test SSH key by default and embeds its
public key into the SD rootfs for first-boot collection. Set
`OPENMINEROS_SSH_AUTHORIZED_KEYS=/path/to/id_ed25519.pub` to use an existing
public key instead.

`make sd-raw-image` requires caller-provided S19 XIL bootloader/kernel assets.
See `S19_RAW_SD.md`; the repo does not ship vendor boot files.

## Supported Board Values

- `s19-xil`
- `s19-bb`
- `s19-aml`
- `s19-cvitek`

Only `s19-xil`, `s19-bb`, and `s19-aml` are part of the `0.1.0` MVP artefact flow.

## Manifest Verification

Every generated artefact manifest uses schema version `1` and records:

- board and model,
- flashable status,
- artefact kind,
- install media,
- install target,
- artefact byte length,
- artefact SHA-256 hash,
- signature metadata.

Build `0.1.0` artefacts are not flashable and intentionally unsigned. The
verifier rejects any flashable manifest without a signature unless explicitly
run with `--allow-flashable-unsigned` for lab-only testing.

`make image` always embeds a compiled `openmineros-control-plane` binary into
the install bundle. Set `OPENMINEROS_RUNTIME_TARGET` to the Rust target triple
for a real board build; leave it unset or `native` for local development.

`MEDIA=all` is the default. For `s19-xil`, it produces both a removable SD
model and an onboard NAND staging bundle. Use `MEDIA=sd` or `MEDIA=nand` when
you need only one target.

## Size Budget

CI builds the Rust workspace in release mode and runs:

```bash
./scripts/check-size-budget.sh
```

Default budgets:

- `openmineros-control-plane`: 12 MiB
- `openmineros-commander`: 8 MiB
- `web/`: 512 KiB

Override with `CONTROL_PLANE_MAX_BYTES`, `COMMANDER_MAX_BYTES`, or
`WEB_MAX_BYTES` when intentionally changing the budget.

## Artefact Budget

CI also generates install bundles for `s19-xil`, `s19-bb`, and `s19-aml`, then
checks each manifest with:

```bash
./scripts/check-artifact-budget.sh
```

Default artefact budgets:

- each `install-image`, `sd-card-image`, `nand-update-bundle`, or `otg-recovery-bundle`: 64 MiB
- total artefacts per manifest: 80 MiB

Override with `MAX_INSTALL_IMAGE_BYTES` or `MAX_TOTAL_ARTIFACT_BYTES` when an
image format change intentionally changes the budget.
