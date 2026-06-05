# S19 XIL Bring-Up and Validation Runbook

This is the staged, safety-first procedure for validating the OpenMinerOS BM1398
ASIC path on a real Antminer S19 XIL control board. Every `NEEDS-HW-CONFIRM`
item in the code is tied to a concrete step here.

> The reference board (`192.168.2.41`) currently runs VNish. Stages 0–2 are
> **read-only** and safe to run while it mines. Stages 3+ write to the FPGA and
> must only run after the existing miner is stopped, on a board you are willing
> to treat as sacrificial.

## Ground rules

- **Recovery first.** VNish stays on NAND. Do all OpenMinerOS work from SD or a
  scratch rootfs so a power cycle returns the board to a known-good firmware.
- **Disarmed by default.** The runtime never touches `/dev/axi_fpga_dev` unless
  `OPENMINEROS_ALLOW_FPGA=1` is set. Keep it unset until a stage explicitly arms.
- **Thermal watch.** Keep `GET /api/v1/thermal` (or the stock telemetry) open
  during any armed stage. `ThermalAction::Shutdown` means stop immediately.
- **One change at a time.** Validate each stage before proceeding.

## Stage 0 — Read-only FPGA probe (safe while mining)

Goal: confirm the FPGA register window is where we think it is, with no writes.

```bash
# On the board (or any host with the device mapped):
omo-commander fpga-probe --device /dev/axi_fpga_dev
```

Expected: a JSON report with a plausible `fpga_version` (compare against what the
stock `bmminer` logs as `FPGA Version = 0x%04X`). The probe maps the device
`PROT_READ` only — it physically cannot write — so it is safe against a running
miner.

Confirms: window base/size (`0x4000_0000` / `0x1400`) and the version register
(offset 0).

## Stage 1 — Get the runtime onto the board

- Cross-compile for the board target (armv7 hard-float):
  `OPENMINEROS_RUNTIME_TARGET=armv7-unknown-linux-gnueabihf make image BOARD=s19-xil MODEL=s19j-pro`
- Copy `omo-commander` and `openmineros-control-plane` to the board (SD/scp).
- Boot the SD test package (see `S19_SD_TEST.md`) so VNish on NAND is untouched.

## Stage 2 — Read-only identity and probe

```bash
openmineros-control-plane --board s19-xil --model s19j-pro --backend hardware-probe
# then, from the board or over the LAN:
curl http://127.0.0.1:8080/api/v1/hardware/identity
curl http://127.0.0.1:8080/api/v1/hardware/probe
curl http://127.0.0.1:8080/api/v1/firmware/anti-brick   # must report safe_to_first_boot=true
omo-commander fpga-probe
```

Confirms: device-tree identity, expected OS paths, and the FPGA probe again. No
writes are issued in `hardware-probe` mode.

## Stage 3 — Armed chain bring-up (writes; miner must be stopped)

Only after the existing miner is stopped and the board is sacrificial:

```bash
export OPENMINEROS_ALLOW_FPGA=1
openmineros-control-plane --board s19-xil --model s19j-pro --backend hardware-mining
```

The first armed call to `bring_up_chain` will: read the FPGA version, enable
nonce RX, enumerate the chain, and set the chain frequency.

Validate, in order:

1. **Enumeration** — `enumerated_chips` matches the real per-chain chip count.
   `NEEDS-HW-CONFIRM`: the address stride (`DEFAULT_ADDRESS_INTERVAL`).
2. **Command path** — the chain ACKs register writes (no bus errors). Confirms
   command data regs `0x31/0x32/0x33`, control `0x30`, trigger `0x80800000`.
3. **Frequency** — chips report the configured PLL. Confirms the PLL divider
   solver and `PLL0_PARAMETER` register.

## Stage 4 — Work dispatch and nonce capture

With a pool configured and `--backend hardware-mining`, the live Stratum engine
captures the extranonce at subscribe and dispatches real work on each notify.

Validate, in order:

1. **Nonce return** — `fpga_poll_nonces` yields entries; `crc_error` is false for
   good nonces. Confirms nonce FIFO regs `4/5/6/7` and `decode_nonce_entry`.
2. **Header byte order** — the pool **accepts** a share. This is the end-to-end
   proof of the `NEEDS-HW-CONFIRM` swaps in `common::mining` and
   `stratum::to_mining_job` (version / prev-hash / ntime / nbits) and of the work
   header word order. A rejected share means a byte-order field is wrong.
3. **Version rolling** — once shares are accepted, replace the placeholder
   `[version; 4]` rolls with a real roll mask and re-validate acceptance.

## Stage 5 — Thermal and tuning under load

- Watch `GET /api/v1/thermal`. Confirm fan ramp and that an induced overtemp
  drives `Throttle` then `Shutdown` per `common::thermal`.
- Drive `TuningExecutor` from real per-step measurements (hashrate, HW error
  rate, chip temp). Confirm a guardrail breach rolls back to the last-known-good
  profile.

## Abort and recovery

- Unset `OPENMINEROS_ALLOW_FPGA` and restart the control plane to stop all FPGA
  writes; the runtime returns to the safe, read-only posture.
- Power-cycle to boot VNish from NAND (the SD path leaves NAND untouched).

## NEEDS-HW-CONFIRM checklist

| Item | Validated by |
| --- | --- |
| FPGA window base/size, version reg | Stage 0 probe |
| Address stride / enumeration | Stage 3.1 |
| Command path regs + trigger | Stage 3.2 |
| PLL register / divider solver | Stage 3.3 |
| Nonce FIFO regs + decode | Stage 4.1 |
| Header byte order (mining/stratum) | Stage 4.2 (accepted share) |
| Work header word 2–4 order | Stage 4.2 (accepted share) |
| Version-roll mask | Stage 4.3 |
| Chip/core split in nonce selector | Stage 4 (per-chip nonce attribution) |
| Thermal sensor + fan I/O | Stage 5 |
