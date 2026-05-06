# Configuration

Build `0.1.0` keeps configuration schema work small and explicit.

Default config:

```toml
[contribution]
enabled = false
rate_percent = 0.0

[tuning]
mode = "stock_like"
target_type = "watts"
autotune = false
```

Validate a config file:

```bash
cargo run -p openmineros-commander -- validate-config --config config/default.toml
```

Run the local control plane with a config file:

```bash
cargo run -p openmineros-control-plane -- \
  --board s19-xil \
  --model s19j-pro \
  --config config/default.toml
```

## Contribution

```toml
[contribution]
enabled = false
rate_percent = 0.0
```

Validation rules:

- `rate_percent` must be between `0.0` and `3.0`,
- `enabled = false` means no contribution mining,
- official builds must reject config keys that try to override beneficiary or endpoint values,
- endpoint and beneficiary values are locked in the official build and visible through UI/API,
- production mode must reject hidden or obfuscated contribution targets.

The only mutable contribution fields in the official build are:

- `enabled`
- `rate_percent`

Invalid example:

```toml
[contribution]
enabled = true
rate_percent = 1.0
beneficiary = "bc1qattacker"
```

The official parser rejects this because `beneficiary` is not a mutable config
field.

## Pools

User mining pools are configured with `[[pools]]`.

```toml
[[pools]]
priority = 0
url = "stratum+tcp://pool.example:3333"
user = "account.worker"
password = "x"
enabled = true

[[pools]]
priority = 1
url = "stratum+tcp://backup.example:3333"
user = "account.worker"
password = "x"
enabled = true
```

Validation rules:

- `priority` values must be unique,
- `url` must start with `stratum+tcp://` or `stratum+ssl://`,
- enabled pools must have a non-empty `user`,
- unknown keys are rejected,
- API output never returns pool passwords.

Build `0.1.0` validates and exposes pool state, but does not connect to pools
yet.

## Tuning

Tuning config selects the active high-level profile.

```toml
[tuning]
mode = "stock_like"
target_type = "watts"
target_value = 3000
autotune = false
```

Supported modes:

- `stock_like`
- `eco`
- `balanced`
- `performance`
- `safe_mode`

`manual` is part of the API catalog but rejected by config validation in build
`0.1.0`, because safe frequency and voltage bounds are not implemented yet.

Validation rules:

- `target_value`, when present, must be positive and finite,
- `autotune = true` is rejected with `safe_mode`,
- `manual` is rejected in build `0.1.0`.
