# Build

Build `0.1.0` provides local development builds, tests, and non-flashable board artefacts.

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
make run-control-plane BOARD=s19-xil MODEL=s19j-pro
cargo run -p openmineros-commander -- validate-config --config config/default.toml
make image BOARD=s19-xil MODEL=s19j-pro
make verify-repro BOARD=s19-xil MODEL=s19j-pro
cargo run -p openmineros-commander -- verify-manifest \
  --manifest dist/openmineros-s19-xil-s19j-pro-0.1.0-manifest.json
```

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
- artefact byte length,
- artefact SHA-256 hash,
- signature metadata.

Build `0.1.0` artefacts are not flashable and intentionally unsigned. The
verifier rejects any flashable manifest without a signature unless explicitly
run with `--allow-flashable-unsigned` for lab-only testing.

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
