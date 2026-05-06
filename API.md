# API

All public REST endpoints are versioned under `/api/v1`.

## System

- `GET /api/v1/system/info`
- `GET /api/v1/system/health`
- `GET /api/v1/events`

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
