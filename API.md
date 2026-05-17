# API

All public REST endpoints are versioned under `/api/v1`.

## System

- `GET /api/v1/overview`
- `GET /api/v1/hardware/targets`
- `GET /api/v1/hardware/identity`
- `GET /api/v1/hardware/safety`
- `GET /api/v1/hardware/readiness`
- `GET /api/v1/hardware/probe`
- `GET /api/v1/firmware/deployment`
- `GET /api/v1/firmware/anti-brick`
- `GET /api/v1/system/info`
- `GET /api/v1/system/health`
- `GET /api/v1/runtime/control`
- `GET /api/v1/events`
- `GET /api/v1/ws`
- `GET /api/v1/support/bundle`

Overview response:

```json
{
  "schema_version": 1,
  "system": {
    "model": "s19j-pro",
    "board_family": "xilinx",
    "backend": "simulated"
  },
  "identity": {},
  "safety": {},
  "readiness": {},
  "anti_brick": {},
  "control": {},
  "health": {},
  "miner": {},
  "job_pipeline": {},
  "stratum": {},
  "chains": [],
  "pools": {},
  "pool_runtime": {},
  "pool_strategy": {},
  "profiles": {},
  "contribution": {},
  "update": {},
  "events": {}
}
```

The overview endpoint is the primary dashboard read model. It combines the
same redacted runtime state exposed by the narrower endpoints and does not
include pool passwords, session tokens, private keys, or hidden contribution
targets.

`system.backend` is `simulated` by default. The optional `hardware-probe`
backend is a read-only bring-up scaffold: it preserves the API contract, reports
zero hashrate, marks hashboards as unprobed, and does not start mining.

Hardware target catalog:

```json
{
  "schema_version": 1,
  "boards": [
    {
      "family": "xilinx",
      "board_id": "s19-xil",
      "display_name": "Xilinx / Zynq",
      "soc": "Zynq",
      "recovery": "external microSD",
      "capabilities": {
        "flags": ["install.sd", "install.commander", "update.ab"]
      }
    }
  ],
  "targets": [
    {
      "model": "s19j-pro",
      "board": "xilinx",
      "support": "mvp-stable"
    }
  ],
  "notes": []
}
```

Build `0.1.0` treats S19j Pro on Xilinx, BeagleBone Black, and Amlogic as
MVP-stable. Other S19-class Xilinx, BeagleBone Black, and Amlogic targets are
listed as experimental until hardware validation is complete. CVitek is listed
for operator identification but is not supported in build `0.1.0`.

Hardware identity report:

```json
{
  "schema_version": 1,
  "backend": "hardware-probe",
  "safe_read_only": true,
  "state": "inferred",
  "confidence": "high",
  "configured_board": "xilinx",
  "configured_model": "s19j-pro",
  "detected_board": "xilinx",
  "detected_model": "s19j-pro",
  "evidence": [
    {
      "source": "device-tree",
      "key": "device tree model",
      "value": "Antminer S19j Pro Xilinx Zynq",
      "matched_board": "xilinx",
      "matched_model": "s19j-pro",
      "detail": "matched s19-xil and s19j-pro"
    }
  ],
  "notes": []
}
```

Build `0.1.0` never auto-switches the configured target. The identity endpoint
only classifies read-only evidence as `configured_only`, `inferred`,
`conflict`, or `unknown`. A conflict means the configured board/model stays
active and hardware actions remain guarded until an operator reviews it.

Hardware safety gate:

```json
{
  "state": "hardware_probe_read_only",
  "configured_target_accepted": true,
  "identity_confirmed": false,
  "simulated_mining_allowed": false,
  "hardware_mining_allowed": false,
  "asic_bus_writes_allowed": false,
  "tuning_writes_allowed": false,
  "flashing_allowed": false,
  "reasons": ["hardware-probe backend is read-only"]
}
```

The safety gate is the central contract for hardware actions. Build `0.1.0`
allows synthetic mining state only in the simulated backend. Hardware mining,
ASIC bus writes, tuning writes, and flashing remain disabled on real/probe
targets. Identity conflicts and unsupported targets block all actions.

Hardware readiness report:

```json
{
  "schema_version": 1,
  "backend": "hardware-probe",
  "model": "s19j-pro",
  "board_family": "xilinx",
  "support": "mvp-stable",
  "recovery": "external microSD",
  "capabilities": {
    "flags": ["install.sd", "install.commander", "update.ab"]
  },
  "state": "read_only_needs_identity",
  "actions": [
    {
      "action": "hardware_mining",
      "allowed": false,
      "reason": "real mining remains disabled until a hardware backend is implemented and gated"
    }
  ],
  "notes": []
}
```

The readiness report is the operator-facing rollup for S19 variability. It
combines configured board/model, support level, recovery path, board
capabilities, and the hardware safety gate. It does not grant additional
permissions beyond `/api/v1/hardware/safety`.

Runtime control report:

```json
{
  "schema_version": 1,
  "board_family": "xilinx",
  "model": "s19j-pro",
  "board_state": "running",
  "board_paused": false,
  "pause_reason": null,
  "tuning_lock_state": "searching",
  "tuning_locked": false,
  "locked_profile": null,
  "notes": [
    "board pause suspends ASIC dispatch without restarting the miner process",
    "tuning lock freezes the discovered profile until the operator unlocks it"
  ]
}
```

`POST /api/v1/board/pause` switches the board into paused state without
restarting the miner process. `POST /api/v1/board/resume` restores normal
dispatch. `POST /api/v1/tuning/lock` freezes the current tuning profile, and
`POST /api/v1/tuning/unlock` allows changes again.

Hardware probe report:

```json
{
  "schema_version": 1,
  "backend": "hardware-probe",
  "safe_read_only": true,
  "probe_root": "/mnt/s19-root",
  "summary": {
    "total": 4,
    "detected": 0,
    "missing_required": 3,
    "skipped": 0
  },
  "checks": [
    {
      "name": "control UART",
      "interface": "uart",
      "path": "/dev/ttyPS0",
      "required": true,
      "status": "missing",
      "detail": "expected path is not present on this host"
    }
  ],
  "notes": []
}
```

The probe endpoint is read-only. It checks expected OS paths only and does not
issue GPIO, UART, I2C, SPI, fan, voltage, clock, pool, or ASIC commands.
Set `OPENMINEROS_PROBE_ROOT=/path/to/target-root` when you want to scan a
mounted S19 root filesystem instead of the live host root.
`probe_root` shows which filesystem was scanned.

Firmware deployment report:

```json
{
  "schema_version": 1,
  "backend": "hardware-probe",
  "board_family": "xilinx",
  "model": "s19j-pro",
  "probe_root": "/mnt/s19-root",
  "deployable": false,
  "gaps": [
    {
      "key": "asic_transport",
      "title": "Real ASIC transport",
      "state": "missing",
      "detail": "The backend still writes framed bytes to a UART path; there is no device-side ASIC driver or chip scheduler yet."
    }
  ],
  "notes": [
    "build 0.1.0 is still missing the real device-side firmware runtime"
  ]
}
```

The report is the current checklist for what still blocks a bootable S19
firmware image.

Firmware anti-brick report:

```json
{
  "schema_version": 1,
  "board_family": "xilinx",
  "model": "s19j-pro",
  "backend": "hardware-probe",
  "state": "safe_first_boot",
  "safe_to_first_boot": true,
  "production_flashable": false,
  "nand_writes_allowed": false,
  "asic_writes_allowed": false,
  "tuning_writes_allowed": false,
  "flashing_allowed": false,
  "checks": [
    {
      "key": "read_only_backend",
      "passed": true,
      "detail": "first S19 boot must use hardware-probe so ASIC dispatch stays disabled"
    }
  ],
  "notes": [
    "safe_to_first_boot means read-only bring-up only; it does not mean mining is production-ready"
  ]
}
```

This endpoint is the operator gate before first S19 testing. Do not attempt
NAND or ASIC write paths unless it reports the expected locked state and the
deployment report is still non-flashable.

Events response:

```json
{
  "events": [
    {
      "seq": 1,
      "uptime_seconds": 12,
      "severity": "info",
      "event_type": "boot.completed",
      "component": "supervisor",
      "message": "supervisor initialized",
      "details": {
        "model": "s19j-pro",
        "board_family": "xilinx"
      }
    }
  ]
}
```

Build `0.1.0` returns a deterministic in-memory event snapshot. Persistent
SQLite-backed events come later.

WebSocket endpoint:

```text
ws://<host>/api/v1/ws
```

On connect, the server sends the current event snapshot as event envelopes and
then sends `system.heartbeat` messages. Build `0.1.0` does not yet stream
database-backed live events.

Envelope:

```json
{
  "type": "boot.completed",
  "seq": 1,
  "uptime_seconds": 12,
  "payload": {}
}
```

Support bundle response:

```json
{
  "schema_version": 1,
  "privacy": {
    "pool_passwords_redacted": true,
    "session_tokens_included": false,
    "private_keys_included": false,
    "raw_logs_included": false
  },
  "system": {},
  "identity": {},
  "safety": {},
  "readiness": {},
  "health": {},
  "miner": {},
  "job_pipeline": {},
  "stratum": {},
  "chains": [],
  "pools": {},
  "pool_runtime": {},
  "pool_strategy": {},
  "profiles": {},
  "contribution": {},
  "events": {}
}
```

Build `0.1.0` support bundles include only structured in-memory state and never
include pool passwords, session tokens, private keys, or raw logs.

## Updates

- `GET /api/v1/update/status`

Update status response:

```json
{
  "update_model": "a_b",
  "active_slot": "slot_a",
  "inactive_slot": "slot_b",
  "rollback_available": true,
  "boot_once_pending": false,
  "slots": [
    {
      "name": "slot_a",
      "state": "active",
      "bootable": true,
      "version": "0.1.0",
      "last_boot_successful": true
    }
  ],
  "notes": []
}
```

Build `0.1.0` exposes A/B status only. It does not write boot targets, install
bundles, or reboot the device.

## Mining

- `GET /api/v1/miner/status`
- `GET /api/v1/miner/job-pipeline`
- `GET /api/v1/chains`
- `GET /api/v1/pools`
- `GET /api/v1/pools/strategy`
- `GET /api/v1/stratum/status`
- `GET /api/v1/stratum/submit-policy`
- `POST /api/v1/stratum/submit-share`
- `GET /api/v1/runtime/control`
- `POST /api/v1/board/pause`
- `POST /api/v1/board/resume`
- `POST /api/v1/tuning/lock`
- `POST /api/v1/tuning/unlock`
- `GET /api/v1/profiles`
- `GET /api/v1/firmware/deployment`
- `GET /api/v1/firmware/anti-brick`
- `GET /api/v1/tuning/plan`
- `GET /api/v1/tuning/execution`
- `GET /api/v1/tuning/transcript`

Pool responses are redacted. The API reports whether a password is set, but it
does not return the password value.

Example:

```json
{
  "configured": 1,
  "enabled": 1,
  "active_priority": 0,
  "pools": [
    {
      "priority": 0,
      "url": "stratum+tcp://pool.example:3333",
      "user": "account.worker",
      "enabled": true,
      "active": true,
      "password_set": true
    }
  ]
}
```

The overview response also includes `pool_runtime`, which is the performance
contract for the future stratum engine:

```json
{
  "state": "ready_no_connection",
  "active_priority": 0,
  "active_latency_ms": null,
  "job_processing_p50_ms": null,
  "job_processing_p99_ms": null,
  "reconnects_total": 0,
  "reconnect_suppressed_total": 0,
  "stale_jobs_total": 0,
  "policy": {
    "latency_warning_ms": 500,
    "job_processing_budget_ms": 50,
    "reconnect_min_interval_seconds": 15,
    "failover_cooldown_seconds": 60,
    "keepalive_interval_seconds": 30
  }
}
```

`simulated` and `hardware-probe` backends keep pool runtime counters at zero.
`hardware-mining` enables real Stratum socket activity and updates these fields
in-memory at runtime.

Pool strategy response:

```json
{
  "state": "planned_no_connection",
  "active_priority": 0,
  "failover_priority_order": [0, 1],
  "persistent_connection_required": true,
  "reconnect_jitter_allowed": false,
  "plans": [
    {
      "priority": 0,
      "role": "active",
      "url": "stratum+tcp://pool.example:3333",
      "user": "account.worker",
      "keepalive_interval_seconds": 30,
      "reconnect_min_interval_seconds": 15,
      "failover_cooldown_seconds": 60,
      "latency_warning_ms": 500,
      "job_processing_budget_ms": 50
    }
  ]
}
```

The connection plan is deterministic: active pool is the lowest enabled
priority, reconnect attempts are rate-limited by `reconnect_min_interval`, and
failover ordering follows enabled priority order.

Job pipeline response:

```json
{
  "state": "planned_read_only",
  "notify_to_dispatch_budget_ms": 50,
  "stale_job_retirement_ms": 500,
  "max_pending_jobs": 2,
  "prefer_newest_job": true,
  "drop_stale_jobs": true,
  "reset_nonce_on_new_prev_hash": true
}
```

Job pipeline policy remains strict in all backends: keep pending queue short,
prefer newest notify, retire stale work quickly, and reset nonce search when a
new `prev_hash` arrives.

Stratum status response:

```json
{
  "state": "planned_no_socket",
  "protocol": "v1",
  "connection": "not_started",
  "socket_open": false,
  "active_pool_priority": 0,
  "subscribed": false,
  "authorized": false,
  "current_difficulty": null,
  "active_job": null,
  "pending_jobs": 0,
  "shares_submitted": 0,
  "shares_accepted": 0,
  "shares_rejected": 0,
  "share_validation": "planned_local_precheck",
  "submit_policy": {
    "enabled_in_build": false,
    "local_precheck_required": true,
    "require_socket_open": true,
    "require_subscribed": true,
    "require_authorized": true,
    "require_active_job": true,
    "require_current_difficulty": true,
    "require_matching_job_id": true,
    "require_hex_extranonce2": true,
    "require_hex_ntime": true,
    "require_hex_nonce": true,
    "ntime_hex_len": 8,
    "nonce_hex_len": 8,
    "max_submit_queue_depth": 2
  }
}
```

`hardware-mining` backend opens a real Stratum V1 socket, sends
`mining.subscribe`/`mining.authorize`, classifies `mining.notify` and
`mining.set_difficulty`, dispatches notify jobs to ASIC backend IO, and accepts
`POST /api/v1/stratum/submit-share` when the safety gate allows live mining.
`simulated` and `hardware-probe` backends keep submit disabled.

Share submit request example:

```json
{
  "worker": "account.worker",
  "job_id": "job-1",
  "extranonce2": "00000002",
  "ntime": "5f5e1000",
  "nonce": "00000001"
}
```

Submit policy response:

```json
{
  "enabled_in_build": false,
  "local_precheck_required": true,
  "require_socket_open": true,
  "require_subscribed": true,
  "require_authorized": true,
  "require_active_job": true,
  "require_current_difficulty": true,
  "require_matching_job_id": true,
  "require_hex_extranonce2": true,
  "require_hex_ntime": true,
  "require_hex_nonce": true,
  "ntime_hex_len": 8,
  "nonce_hex_len": 8,
  "max_submit_queue_depth": 2
}
```

The local share precheck rejects invalid hex fields, closed sockets,
unsubscribed or unauthorized sessions, missing difficulty, missing active jobs,
and stale `job_id` values before a future `mining.submit` call can happen.

Profiles response:

```json
{
  "active": "stock_like",
  "target_type": "watts",
  "target_value": null,
  "autotune": false,
  "profiles": [
    {
      "name": "stock_like",
      "available": true,
      "reason": null
    },
    {
      "name": "manual",
      "available": false,
      "reason": "manual tuning is disabled in build 0.1.0"
    }
  ]
}
```

Build `0.1.0` exposes profile state and validation only. It does not write
frequency or voltage settings to hardware.

Tuning plan response:

```json
{
  "state": "planned_read_only",
  "active_phase": "baseline",
  "writable": false,
  "guardrails": {
    "chip_frequency_step_mhz": 5,
    "voltage_step_mv": 5,
    "min_step_duration_seconds": 300,
    "max_chip_temp_c": 85.0,
    "max_hw_error_rate_percent": 0.03,
    "rollback_on_rejected_shares": true,
    "voltage_trim_requires_stable_upclock": true
  },
  "steps": [
    { "order": 1, "phase": "baseline", "scope": "chain" },
    { "order": 2, "phase": "downclock_efficiency", "scope": "chip" },
    { "order": 3, "phase": "upclock_stability", "scope": "chip" },
    { "order": 4, "phase": "voltage_trim", "scope": "chip" }
  ]
}
```

Build `0.1.0` exposes this as a read-only plan. Future autotune must move
slowly chip-by-chip, run downclock efficiency before upclock stability, and
touch voltage only after stable frequency results.

Tuning transcript response:

```json
{
  "schema_version": 1,
  "board_family": "xilinx",
  "chip_id": 0,
  "base_frequency_mhz": 725,
  "base_voltage_mv": 800,
  "frequency_step_mhz": 5,
  "voltage_step_mv": 5,
  "frames": [
    {
      "order": 1,
      "phase": "baseline",
      "command": "set_frequency",
      "target_frequency_mhz": 725,
      "target_voltage_mv": 800,
      "min_duration_seconds": 300,
      "frame": "omo-asic/xilinx/v1|board=xilinx|cmd=set_frequency|..."
    }
  ]
}
```

The transcript is read-only. It shows how the planned chip-by-chip sequence
would be framed for the active board family.

Tuning execution status:

```json
{
  "schema_version": 1,
  "state": "planned_read_only",
  "autotune": true,
  "active_phase": "baseline",
  "current_step": {
    "order": 1,
    "phase": "baseline",
    "command": "set_frequency",
    "target_frequency_mhz": 725,
    "target_voltage_mv": 800,
    "min_duration_seconds": 300
  },
  "queued_steps": [
    {
      "order": 2,
      "phase": "downclock_efficiency",
      "command": "set_frequency",
      "target_frequency_mhz": 720,
      "target_voltage_mv": 800,
      "min_duration_seconds": 300
    }
  ],
  "write_allowed": false,
  "blocked_reason": "tuning executor is planned but write paths remain gated in build 0.1.0",
  "notes": [
    "execution status is derived from the tuning plan, transcript, and safety gate"
  ]
}
```

The execution status exposes the current step queue the runtime would use if
the write gate were enabled. In build `0.1.0` it remains read-only and tracks
the same baseline, downclock, upclock, then voltage-trim order.
When runtime control is locked, the board keeps the chosen tuning profile and
does not keep changing it between cycles.

## Contribution

- `GET /api/v1/contribution/status`

The default contribution rate is `0.0`. Operators may explicitly choose any
rate from `0.0` to `3.0`.

Contribution configuration is public by design. The project does not hide,
obfuscate, or silently redirect contribution mining.

Official builds expose the beneficiary and endpoints for audit, but do not allow
runtime config to override them. The mutable contribution fields are only
`enabled` and `rate_percent`.

Default opt-in contribution target:

```json
{
  "beneficiary": "bc1qp6d4vxmenug97ghcy027vsn3902yadcj77ka6j",
  "endpoints": [
    "stratum+tcp://ss.antpool.com:3333",
    "stratum+tcp://ss.antpool.com:443",
    "stratum+tcp://ss.antpool.com:25"
  ]
}
```

The endpoint list is inactive while contribution is disabled.

## Metrics

- `GET /metrics`

Metrics use the `omo_` prefix.

Build `0.1.0` exports miner, chain, health, pool, tuning, update, and
contribution gauges/counters. Metrics intentionally do not expose pool URLs,
pool users, pool passwords, or the contribution beneficiary address.

Core metrics:

```text
omo_miner_hashrate_ths
omo_miner_power_watts
omo_miner_efficiency_j_th
omo_miner_uptime_seconds
omo_hardware_identity_evidence_total
omo_hardware_identity_conflict
omo_hardware_safety_configured_target_accepted
omo_hardware_safety_hardware_mining_allowed
omo_hardware_safety_asic_bus_writes_allowed
omo_hardware_safety_flashing_allowed
omo_hardware_readiness_state{state="simulation_ready"}
omo_hardware_readiness_actions_allowed_total
omo_job_notify_to_dispatch_budget_ms
omo_job_stale_retirement_ms
omo_job_max_pending_jobs
omo_job_prefer_newest
omo_job_drop_stale
omo_job_reset_nonce_on_new_prev_hash
omo_stratum_socket_open
omo_stratum_subscribed
omo_stratum_authorized
omo_stratum_active_pool_priority
omo_stratum_pending_jobs
omo_stratum_shares_submitted_total
omo_stratum_shares_accepted_total
omo_stratum_shares_rejected_total
omo_stratum_submit_enabled_in_build
omo_stratum_submit_local_precheck_required
omo_stratum_submit_max_queue_depth
omo_system_health_state{state="mining"}
omo_system_health_severity
omo_pool_configured_total
omo_pool_enabled_total
omo_pool_active_priority
omo_pool_latency_warning_ms
omo_pool_job_processing_budget_ms
omo_pool_reconnect_min_interval_seconds
omo_pool_failover_cooldown_seconds
omo_pool_reconnects_total
omo_pool_reconnect_suppressed_total
omo_pool_stale_jobs_total
omo_pool_strategy_enabled_candidates_total
omo_pool_strategy_active_priority
omo_pool_strategy_persistent_connection_required
omo_pool_strategy_reconnect_jitter_allowed
omo_tuning_profile_active{profile="stock_like"}
omo_tuning_profile_available{profile="manual"}
omo_tuning_plan_writable
omo_tuning_frequency_step_mhz
omo_tuning_voltage_step_mv
omo_tuning_min_step_duration_seconds
omo_update_rollback_available
omo_update_boot_once_pending
omo_update_slot_bootable{slot="slot_a",state="active"}
omo_contribution_enabled
omo_contribution_rate_percent
omo_contribution_target_locked
omo_chain_up{chain="0"}
omo_chain_asic_detected{chain="0"}
omo_temp_board_celsius{chain="0"}
omo_temp_chip_max_celsius{chain="0"}
```
