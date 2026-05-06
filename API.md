# API

All public REST endpoints are versioned under `/api/v1`.

## System

- `GET /api/v1/overview`
- `GET /api/v1/hardware/targets`
- `GET /api/v1/hardware/probe`
- `GET /api/v1/system/info`
- `GET /api/v1/system/health`
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
  "health": {},
  "miner": {},
  "job_pipeline": {},
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

Hardware probe report:

```json
{
  "schema_version": 1,
  "backend": "hardware-probe",
  "safe_read_only": true,
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
  "health": {},
  "miner": {},
  "job_pipeline": {},
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
- `GET /api/v1/profiles`
- `GET /api/v1/tuning/plan`

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

Build `0.1.0` does not connect to pools yet, so latency and job-processing
values are `null` and counters are zero. The policy is intentionally strict:
low latency, fast job handling, and reconnect suppression are first-class
requirements for the miner engine.

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

Build `0.1.0` exposes the deterministic connection plan before networking is
enabled. Future Stratum code must hold the active pool persistently, suppress
reconnect loops inside the configured minimum interval, and follow enabled pool
priority order for failover.

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

Build `0.1.0` does not parse or dispatch real Stratum jobs yet. This endpoint
sets the runtime contract: keep the pending job queue short, prefer the newest
pool notify, retire stale work quickly, and reset nonce search when a new
`prev_hash` arrives.

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
omo_job_notify_to_dispatch_budget_ms
omo_job_stale_retirement_ms
omo_job_max_pending_jobs
omo_job_prefer_newest
omo_job_drop_stale
omo_job_reset_nonce_on_new_prev_hash
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
