# OpenMinerOS

OpenMinerOS is an open mining OS prototype for Antminer S19-class hardware.

The project is published as GPL-3.0-only open source.

Build `0.1.0` is a safe development baseline:

- Rust workspace bootstrapped for runtime modules.
- Board identity support for Xilinx/Zynq, BeagleBone Black, Amlogic, and CVitek.
- S19-class model matrix with explicit support levels.
- Simulated ASIC backend for local API and UI development.
- REST endpoints for dashboard overview, system info, health, miner status, chains, contribution status, and Prometheus metrics.
- Validated user pool config with redacted pool status API.
- Validated tuning profile config with read-only profile API.
- Deterministic in-memory event snapshot API.
- WebSocket event snapshot and heartbeat API.
- Redacted support bundle API.
- Read-only A/B update status API.
- Expanded Prometheus metrics without secret-bearing labels.
- Placeholder reproducible image artefact flow for `s19-xil`, `s19-bb`, and `s19-aml`.
- Optional, transparent development contribution defaults to `0.0 %` and is capped at `3.0 %`.

This build does **not** flash hardware or ship low-level ASIC drivers yet.

## Quick Start

```bash
make test
make run-control-plane BOARD=s19-xil MODEL=s19j-pro
cargo run -p openmineros-commander -- validate-config --config config/default.toml
```

Open `http://127.0.0.1:8080` for the local status UI.

## Board Builds

```bash
make image BOARD=s19-xil MODEL=s19j-pro
make image BOARD=s19-bb MODEL=s19j-pro
make image BOARD=s19-aml MODEL=s19j-pro
make verify-repro BOARD=s19-xil MODEL=s19j-pro
cargo run -p openmineros-commander -- verify-manifest \
  --manifest dist/openmineros-s19-xil-s19j-pro-0.1.0-manifest.json
```

The generated files are metadata-only development artefacts in `dist/`.
They are intentionally not flashable firmware images.

## API

- `GET /api/v1/system/info`
- `GET /api/v1/overview`
- `GET /api/v1/hardware/targets`
- `GET /api/v1/system/health`
- `GET /api/v1/miner/status`
- `GET /api/v1/chains`
- `GET /api/v1/contribution/status`
- `GET /metrics`

`omo-commander matrix` returns the same hardware catalog used by
`GET /api/v1/hardware/targets`.

## Config

Build `0.1.0` includes a minimal TOML config parser. In official builds,
contribution config can only change `enabled` and `rate_percent`; beneficiary
and pool endpoint overrides are rejected.

User mining pools are validated under `[[pools]]` and exposed through
`GET /api/v1/pools` with passwords redacted.

Tuning profiles are validated under `[tuning]` and exposed through
`GET /api/v1/profiles`. Build `0.1.0` does not touch hardware clocks or
voltages.

## Development Contribution

OpenMinerOS does not ship a mandatory devfee. The optional contribution target
is public, auditable, and locked in official builds as described in
[DEVFEE.md](DEVFEE.md). Hidden wallet addresses, obfuscated pool URLs, and
silent redirects are out of scope for this project.
