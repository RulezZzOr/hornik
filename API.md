# API

All public REST endpoints are versioned under `/api/v1`.

## System

- `GET /api/v1/overview`
- `GET /api/v1/hardware/targets`
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
  "chains": [],
  "pools": {},
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
  "chains": [],
  "pools": {},
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
- `GET /api/v1/chains`
- `GET /api/v1/pools`
- `GET /api/v1/profiles`

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
omo_system_health_state{state="mining"}
omo_system_health_severity
omo_pool_configured_total
omo_pool_enabled_total
omo_pool_active_priority
omo_tuning_profile_active{profile="stock_like"}
omo_tuning_profile_available{profile="manual"}
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
