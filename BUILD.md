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
make run-control-plane BOARD=s19-xil MODEL=s19j-pro
make image BOARD=s19-xil MODEL=s19j-pro
make verify-repro BOARD=s19-xil MODEL=s19j-pro
```

## Supported Board Values

- `s19-xil`
- `s19-bb`
- `s19-aml`
- `s19-cvitek`

Only `s19-xil`, `s19-bb`, and `s19-aml` are part of the `0.1.0` MVP artefact flow.

