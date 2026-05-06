# Configuration

Build `0.1.0` keeps configuration schema work small and explicit.

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
