# API

All public REST endpoints are versioned under `/api/v1`.

## System

- `GET /api/v1/system/info`
- `GET /api/v1/system/health`

## Mining

- `GET /api/v1/miner/status`
- `GET /api/v1/chains`

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
