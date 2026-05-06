# Configuration

Build `0.1.0` keeps configuration schema work small and explicit.

Default config:

```toml
[contribution]
enabled = false
rate_percent = 0.0
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
