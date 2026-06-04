# OpenMinerOS Project Ledger

This file is the working status and forward-plan document for OpenMinerOS.
`hornik.md` is the original design and market rationale note. `project.md`
is the current state ledger: what is built, what is still missing, and what
the next development steps are.

Last updated: 2026-06-04

## 1. What OpenMinerOS Is

OpenMinerOS is an open-source mining OS for Antminer S19-class hardware.
The project is built around these non-negotiable goals:

- fully public runtime and build chain,
- no mandatory devfee,
- local-first API and UI,
- deterministic and reproducible build artifacts,
- safe first boot and recovery-first installation,
- conservative defaults before any aggressive tuning,
- no hidden pool redirects or secret fallback wallets.

The current codebase is intentionally a safe development baseline, not a
final flashable miner firmware.

## 2. Current State Snapshot

| Area | Status | Notes |
| --- | --- | --- |
| Rust workspace | Done | `common`, `supervisor`, `asic-backend`, `control-plane`, `commander`, board crates |
| Runtime identity/config wiring | Done | Rendered per board/model, no hardcoded runtime defaults |
| Side-effect-free observability | Done | `overview` and `metrics` use snapshot reads, not polling |
| Subscribe response handling | Done | rejected subscribe does not mark the session subscribed |
| First-boot SD package | Done | safe first boot, SSH key access, health collection, anti-brick checks |
| Raw SD candidate workflow | Done for operator-supplied boot assets | Not vendor-shipped; depends on external boot files |
| Readiness model | Done | `MiningReady` is distinct from read-only identification |
| macOS raw SD writer | Done | pipefail plus SHA-256 guard before writing |
| Pool strategy and Stratum V1 contract | Partial | Present as runtime scaffold and read-only/live bridge |
| ASIC transport and chip scheduler | Partial | Real BM1398 VIL codec (`asic-backend::bm1398`), mmap FPGA register transport (`asic-backend::axi`, `/dev/axi_fpga_dev`), and a chain driver tying them together (`asic-backend::bm1398_driver`: enumerate, set-frequency, submit-work, poll-nonces) all host-unit-tested; register map/opcodes/FIFO handshake documented from the stock `.bmu` but `NEEDS-HW-CONFIRM` and not yet driven on a board |
| Thermal and fan control | Missing | No real sensor loop or thermal shutdown path yet |
| Signed updates and flashable image | Partial | Update metadata and install bundles exist, but not a production flash flow |
| Production NAND firmware | Missing | The repo does not yet ship a final flashable image |

Validation status: workspace tests and clippy are currently clean.

## 3. What Is Already Implemented

### Runtime and control plane

- local HTTP API for system, hardware, firmware, runtime control, Stratum, and metrics,
- `hardware-probe` safe read-only backend for bring-up,
- `hardware-mining` scaffold with live session handling,
- board pause/resume without restarting the miner process,
- tuning lock/unlock without tearing down the runtime,
- health, readiness, anti-brick, and deployment reports,
- Prometheus metrics without secret-bearing labels.

### Stratum and mining state

- deterministic pool selection and failover policy,
- share precheck before submit,
- live stratum status model,
- nonce candidate parsing and share submission bridge,
- side-effect-free dashboard and metrics reads.

### Build and install tooling

- Buildroot-based image generation,
- reproducible install bundle and manifest checks,
- SD test package generation,
- raw SD candidate builder for blank media when operator boot assets are supplied,
- macOS raw SD writer with explicit arming and integrity validation.

### Safety and recovery

- anti-brick report for first boot gating,
- safe-mode first boot defaults,
- NAND writes disabled in the current build,
- ASIC writes disabled in the safe test path,
- SSH access is key-based in the SD test bundle,
- operator-visible first boot collection script.

## 4. What Is Still Missing

These are the real blockers between the current codebase and a usable miner
firmware release:

1. **Verified ASIC transport on real hardware**
   - architecture (recovered from the stock `.bmu`): S19 XIL reaches the BM1398
     chain through an FPGA in the Zynq PL, mmap'd at `/dev/axi_fpga_dev`
     (`bitmain_axi.ko`); the work TX FIFO and nonce RX FIFO live in the FPGA.
     The earlier `/dev/ttyPS0` text protocol was a placeholder, not the real bus,
   - the BM1398 VIL command codec now exists (`asic-backend::bm1398`): CRC5/CRC16,
     register read/write framing, PLL divider solver, chain-address enumeration,
     and nonce-word decode, all unit-tested on the host,
   - the FPGA AXI mmap transport now exists (`asic-backend::axi`): volatile
     word-addressed register access over `/dev/axi_fpga_dev`, command-buffer
     write, work-FIFO push, nonce-FIFO pop, host-tested via an anonymous mapping,
   - a chain driver ties protocol and transport together
     (`asic-backend::bm1398_driver`: enumerate / set-frequency / submit-work /
     poll-nonces),
   - still missing before real hashing:
     - CONFIRMED against the stock firmware (Ghidra decompile of `bmminer` +
       `bitmain_axi.ko`): FPGA window base `0x4000_0000` / size `0x1400`; the
       CRC-5 routine (matches our `crc5` exactly); the VIL register-write frame
       (header `0x41`/`0x51`, length 9, big-endian value, CRC-5 over the 8 bytes,
       and **no** `0x55 0xAA` preamble on the CPU side — the FPGA adds it); the
       FPGA version register (offset 0); and the command path (data words at
       offsets `0x31/0x32/0x33`, control/trigger at `0x30` = `0x8080_0000 |
       chain<<16`, busy = bit31); the nonce RX FIFO (data regs 4/5 read
       alternately, status reg 6 with entry count `(v & 0x7fff) >> 1`, control
       reg 7 enable bit `0x1_0000`); and the work TX FIFO (first word -> reg
       0x10, rest -> reg 0x11, per-chain ready bits in reg 3). The code was
       corrected to match all of these,
     - the nonce-entry field decode is now CONFIRMED from bmminer
       (`FUN_00034810`): metadata word `byte0 & 0x40` = CRC error, `byte3 & 0x60`
       = register-response flag, `byte1` = work id, `byte2`+`byte3[4:0]` = a
       chip/core selector, with the nonce in the second word,
     - the work-frame wire layout is now CONFIRMED from bmminer (`FUN_0002591c`
       + builder `FUN_000420e8`): a fixed 148-byte frame = a 5-word (20-byte)
       header plus exactly four 32-byte midstates, all big-endian, no preamble
       and no CRC; the first word streams to work FIFO reg 0x10, the rest to 0x11,
     - still NEEDS-HW-CONFIRM: the exact chip-vs-core split inside the nonce
       selector, the exact nbits/ntime/merkle assignment of header words 2-4, and
       the full chip register map,
     - real on-board chip discovery (validate the enumeration walk returns chips),
     - stable init sequence (baud, ticket mask, version rolling, core config),
     - full 8-byte nonce-frame reassembly from the RX FIFO,
     - nonce/share capture from real hashboard traffic,
     - wire the driver into the live dispatch path behind the safety gate.

2. **Thermal and power control**
   - fan feedback and control,
   - board and chip temperature reads,
   - overtemp shutdown,
   - safe board isolation when a chain misbehaves.

3. **Tuning executor**
   - chip-by-chip downclock/upclock,
   - voltage trim,
   - discover-then-lock tuning profile flow,
   - safe rollback to last known-good profile.

4. **Flashable production image**
   - signed release manifest,
   - bootable install artifact that can actually be flashed,
   - A/B or rescue flow that survives a bad update,
   - recovery instructions that do not depend on a live web UI.

5. **Board parity**
   - XIL first, then BBB, then AML,
   - board-specific recovery stories for each family,
   - validation of the install path on each target.

## 5. Hardware Strategy

### Board priority

1. **XIL first**
   - best recovery story,
   - safest lab bring-up path,
   - current safe SD and raw SD workflow is centered here.

2. **BBB second**
   - internal SD access and different recovery handling,
   - should follow only after XIL proves the runtime contract.

3. **AML third**
   - OTG/USB recovery path,
   - should be treated as a separate install story, not a copy of XIL.

4. **CVitek**
   - operator identification only for now,
   - not part of the current MVP support target.

### Recovery philosophy

- recovery first, not web-upgrade first,
- no assumption that a blank SD card can boot without vendor boot assets,
- no NAND writes until the anti-brick and install path are proven,
- no power-on mining before the thermal path exists.

## 6. Repository Map

- `crates/common` - shared types, config parsing, identity/safety/readiness,
  Stratum message classification, tuning and update models.
- `crates/supervisor` - runtime state aggregation, dashboard read model,
  pool runtime, tuning execution, firmware deployment report.
- `crates/asic-backend` - backend scaffold, UART transport, frame building,
  probe and identity reporting, nonce candidate plumbing.
- `crates/control-plane` - HTTP API, request handlers, startup wiring.
- `crates/commander` - CLI and verification helpers for manifests and reports.
- `buildroot/` - image build system, overlays, board-specific runtime hooks.
- `scripts/` - build, verify, first-boot collection, SD package, raw SD tools.
- `README.md`, `BUILD.md`, `INSTALL.md`, `API.md` - operator-facing quick docs.
- `S19_SD_TEST.md`, `S19_RAW_SD.md` - safe first-boot and raw SD procedures.
- `hornik.md` - design rationale and market/background note.

## 7. Release and Security Policy

- no mandatory devfee,
- optional contribution only, public and auditable,
- no hidden pool redirects,
- no silent wallet substitution,
- no secret recovery backdoors,
- no secret-bearing metrics or dashboard endpoints,
- no flashable release without manifest verification and rollback story,
- no hardware-mining writes until the safety gate says the path is ready.

The current codebase already enforces the safe default path at runtime.

## 8. Current Operational Flow

### Safe first boot

1. build the SD test package,
2. boot from SD only,
3. collect health and readiness through SSH or API,
4. verify anti-brick reports,
5. keep NAND and ASIC writes disabled.

### Raw SD candidate

1. supply operator-owned S19 XIL boot assets,
2. build the raw SD candidate,
3. verify its hash against the sidecar JSON,
4. write it to the whole removable disk on macOS only after explicit arming.

### What not to do yet

- do not assume the repo ships vendor bootloader or kernel files,
- do not flash NAND from the current development path,
- do not switch to live mining until the ASIC and thermal paths are verified.

## 9. Next Development Timeline

The critical path is not the UI. It is ASIC bring-up, thermal safety, and a
flashable release path.

### Phase 0 - current baseline, already done

Goal:
- keep the runtime safe, deterministic, and testable.

Exit criteria already met:
- workspace tests pass,
- clippy passes,
- read-only observability has no side effects,
- safe first-boot path exists,
- raw SD writer is guarded by hash verification.

### Phase 1 - ASIC bring-up on XIL

Estimated duration:
- 1 to 2 weeks if a usable reference implementation or protocol dump exists,
- much longer if the ASIC protocol must be recovered from scratch.

Deliverables:
- real ASIC transport on one XIL board,
- chip enumeration and chain inventory,
- stable job dispatch path,
- nonce/share capture from hardware,
- basic failure handling without reboot loops.

Exit criteria:
- one sacrificial XIL board boots,
- real ASIC traffic is visible,
- shares can be produced in a controlled lab run,
- the board can be stopped safely.

### Phase 2 - Thermal and power safety

Estimated duration:
- 1 to 2 weeks after Phase 1.

Deliverables:
- fan control,
- board and chip temperature reads,
- overtemperature protection,
- chain isolation for faults,
- safe pause/resume behavior under load.

Exit criteria:
- 24 hour soak without thermal runaway,
- no uncontrolled reset loop,
- fault handling is deterministic.

### Phase 3 - Tuning executor and profile lock

Estimated duration:
- 1 to 2 weeks after Phase 2.

Deliverables:
- downclock then upclock tuning flow,
- voltage trim,
- discover-then-lock profile persistence,
- rollback to the last safe profile,
- operator-visible tuning transcript.

Exit criteria:
- tuning is repeatable,
- tuning state survives restarts,
- the runtime can always fall back to a safe profile.

### Phase 4 - Flashable release path

Estimated duration:
- 2 to 3 weeks after the hardware path is stable.

Deliverables:
- signed manifest and release metadata,
- actual flashable image,
- A/B or rescue update story,
- post-boot verification flow,
- install docs that are not lab-only.

Exit criteria:
- a bad update can be recovered without JTAG,
- the release is reproducible,
- install and rollback are documented and tested.

### Phase 5 - Board parity and public beta

Estimated duration:
- ongoing after the first stable XIL path.

Deliverables:
- BBB support stabilized,
- AML support stabilized,
- board-specific recovery docs,
- longer soak tests,
- public beta release candidate.

Exit criteria:
- each target board has a tested recovery path,
- support matrix is explicit,
- release notes explain the remaining limits honestly.

## 10. Biggest Risks

1. **No verified ASIC reference path**
   - This is the largest schedule risk.
   - Without a working reference, ASIC work becomes reverse engineering, not
     straightforward porting.

2. **Recovery assets not available**
   - The repo does not ship Bitmain bootloader or kernel files.
   - Blank SD flow depends on operator-supplied boot assets.

3. **Thermal safety not validated early enough**
   - This is the fastest way to damage hardware.
   - Thermal work must come before any aggressive mining mode.

4. **Feature creep before bring-up**
   - UI work, extra APIs, and new tuning ideas must not delay the hardware
     bring-up path.

## 11. Definition of Done for a Real Release

OpenMinerOS is not a finished miner firmware until all of these are true:

- a board boots from a documented recovery path,
- the ASIC backend talks to real hardware,
- thermal and power control are live,
- the runtime can mine without manual babysitting,
- the release is reproducible and signed,
- recovery from a failed update is documented and tested,
- the project still has no mandatory devfee and no hidden redirect logic.

## 12. Reference Docs

- `README.md` - project overview and quick start
- `BUILD.md` - build, budget, and verification commands
- `INSTALL.md` - install prep and recovery flow
- `UPGRADE.md` - update and rollback policy
- `API.md` - runtime API contract
- `CONFIG.md` - config schema and validation rules
- `DEVFEE.md` - optional contribution model
- `S19_SD_TEST.md` - safe first boot checklist
- `S19_RAW_SD.md` - raw SD candidate path
- `hornik.md` - original design and market analysis

