# OpenMinerOS

OpenMinerOS is an open mining OS prototype for Antminer S19-class hardware.

The project is published as GPL-3.0-only open source.

Build `0.1.0` is a safe development baseline:

- Rust workspace bootstrapped for runtime modules.
- Board identity support for Xilinx/Zynq, BeagleBone Black, Amlogic, and CVitek.
- S19-class model matrix with explicit support levels.
- Read-only hardware identity report for configured vs inferred S19 target.
- Central hardware safety gate for mining, flashing, tuning, and ASIC writes.
- Hardware readiness report for target support, recovery path, capabilities, and allowed actions.
- Simulated ASIC backend for local API and UI development.
- REST endpoints for dashboard overview, system info, health, miner status, chains, contribution status, and Prometheus metrics.
- Validated user pool config with redacted pool status API.
- Pool latency, job processing, and reconnect policy surfaced for the future stratum engine.
- Deterministic pool connection strategy for persistent active pool, stable failover order, and reconnect suppression.
- Read-only job pipeline policy for newest-job priority, short queues, and stale-work retirement.
- Read-only Stratum V1 engine contract with no sockets opened in build `0.1.0`.
- Validated tuning profile config with read-only profile API.
- Read-only autotune plan: baseline, downclock efficiency, upclock stability, then voltage trim.
- Read-only tuning transcript API for chip-by-chip command previews with structured targets.
- Read-only tuning execution status API that shows the current step, queue, and write gate.
- Runtime control API for board pause/resume and tuning profile lock without restarting the miner process.
- Anti-brick report API for read-only first boot gating.
- Deterministic in-memory event snapshot API.
- WebSocket event snapshot and heartbeat API.
- Redacted support bundle API.
- Read-only A/B update status API.
- Expanded Prometheus metrics without secret-bearing labels.
- Reproducible install bundle flow for `s19-xil`, `s19-bb`, and `s19-aml` that embeds the compiled control-plane runtime and boot hooks.
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
make image BOARD=s19-xil MODEL=s19j-pro MEDIA=sd
make image BOARD=s19-xil MODEL=s19j-pro MEDIA=nand
make image BOARD=s19-bb MODEL=s19j-pro
make image BOARD=s19-aml MODEL=s19j-pro
make install-preflight BOARD=s19-xil MODEL=s19j-pro
make verify-repro BOARD=s19-xil MODEL=s19j-pro
make sd-test-package BOARD=s19-xil MODEL=s19j-pro
make boot-assets-import BOOT_SOURCE=/path/to/stock-recovery BOOT_ASSETS=boot-assets/s19-xil
make sd-raw-image BOARD=s19-xil MODEL=s19 BOOT_ASSETS=/path/to/s19-xil-boot-assets
cargo run -p openmineros-commander -- verify-manifest \
  --manifest dist/openmineros-s19-xil-s19j-pro-0.1.0-manifest.json
```

The generated files are reproducible development artefacts in `dist/`.
They are intentionally not flashable firmware images yet, but the bundle now
contains the compiled control-plane runtime, device-side launch script, and
safe boot hooks.

`make sd-test-package` prepares the guarded first-boot SD package with DHCP on
`eth0`, Dropbear SSH on port `22`, generated key-based root access, and local
self-test scripts for collecting the first hardware report.
For a blank SD card, `make sd-raw-image` can assemble a raw SD candidate only
when you provide known-good S19 XIL bootloader/kernel assets. See
`S19_RAW_SD.md`.

For `s19-xil`, the default `MEDIA=all` build emits both:

- `sd-card-image` for removable SD bring-up,
- `nand-update-bundle` for onboard NAND staging.

See `INSTALL.md` for the current install-prep flow and `UPGRADE.md` for the
slot and rollback policy.

## API

- `GET /api/v1/system/info`
- `GET /api/v1/overview`
- `GET /api/v1/hardware/targets`
- `GET /api/v1/hardware/identity`
- `GET /api/v1/hardware/safety`
- `GET /api/v1/hardware/readiness`
- `GET /api/v1/hardware/probe`
- `GET /api/v1/firmware/deployment`
- `GET /api/v1/firmware/anti-brick`
- `GET /api/v1/system/health`
- `GET /api/v1/miner/status`
- `GET /api/v1/miner/job-pipeline`
- `GET /api/v1/chains`
- `GET /api/v1/thermal`
- `GET /api/v1/stratum/status`
- `GET /api/v1/stratum/submit-policy`
- `GET /api/v1/runtime/control`
- `POST /api/v1/board/pause`
- `POST /api/v1/board/resume`
- `POST /api/v1/tuning/lock`
- `POST /api/v1/tuning/unlock`
- `GET /api/v1/tuning/transcript`
- `GET /api/v1/contribution/status`
- `GET /api/v1/pools/strategy`
- `GET /metrics`

`omo-commander matrix` returns the same hardware catalog used by
`GET /api/v1/hardware/targets`.

`omo-commander fpga-probe --device /dev/axi_fpga_dev` does a **read-only** sample
of the FPGA register window (it maps `PROT_READ`, so it cannot write) for safe
on-board validation. See `S19_BRINGUP.md` for the staged bring-up runbook.

## Runtime Backends

The control plane defaults to the `simulated` backend for local development.
For board bring-up work, `hardware-probe` exposes the same API shape but stays
read-only and reports zero hashrate until ASIC bus probing is implemented.
`hardware-mining` enables live Stratum V1 socket/session handling plus ASIC job
dispatch. On S19 XIL it carries a real BM1398 FPGA chain driver
(`/dev/axi_fpga_dev`), but that path is **disarmed by default**: it only touches
the FPGA when an operator sets `OPENMINEROS_ALLOW_FPGA=1`, and stays inert on any
host where the device is absent.
`GET /api/v1/hardware/probe` checks expected OS paths only; it does not issue
GPIO, UART, fan, voltage, clock, pool, or ASIC commands.
Set `OPENMINEROS_PROBE_ROOT=/path/to/target-root` to scan a mounted S19 root
filesystem instead of the live host root.
The probe report includes the effective `probe_root` so you can confirm which
filesystem was scanned.
`GET /api/v1/firmware/deployment` lists the remaining blockers before the
project can become a bootable S19 firmware image.
`GET /api/v1/firmware/anti-brick` must report `safe_to_first_boot=true` before
first S19 testing. That only means read-only bring-up is guarded; it does not
mean mining or NAND writes are production-ready.
`make image` builds the control-plane runtime binary first and embeds it into
the install bundle. Set `OPENMINEROS_RUNTIME_TARGET` when cross-compiling for a
real board target; leave it unset for local native development bundles.
`GET /api/v1/hardware/identity` compares the configured board/model with
read-only evidence and never auto-switches the target in build `0.1.0`.
`GET /api/v1/hardware/safety` keeps real hardware mining, flashing, tuning
writes, and ASIC bus writes disabled until the backend implements those paths.
`GET /api/v1/hardware/readiness` rolls target support, recovery method,
capabilities, and the safety gate into one operator-facing status report.
`POST /api/v1/stratum/submit-share` performs local precheck and submits share
payloads to the active pool when the safety gate allows live mining.

```bash
make run-control-plane BOARD=s19-xil MODEL=s19j-pro
cargo run -p openmineros-control-plane -- \
  --board s19-xil \
  --model s19j-pro \
  --backend hardware-probe
```

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
