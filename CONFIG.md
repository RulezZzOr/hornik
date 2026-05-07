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

[pool_policy]
latency_warning_ms = 500
job_processing_budget_ms = 50
reconnect_min_interval_seconds = 15
failover_cooldown_seconds = 60
keepalive_interval_seconds = 30
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

## Hardware Identity

`board` and `model` are still explicit runtime inputs in build `0.1.0`.
The hardware identity report can infer likely S19 board/model evidence from
read-only sources, but it does not automatically change the configured target.

Possible identity states:

- `configured_only`: simulated backend or no probe evidence is used,
- `inferred`: read-only evidence matches a supported board or model,
- `conflict`: detected evidence disagrees with configured board/model,
- `unknown`: no supported S19 identity evidence was found.

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

Pool configuration affects event output:

- no enabled pools emits `pool.unconfigured`,
- at least one enabled pool emits `pool.strategy_loaded`,
- at least one enabled pool emits `pool.active_selected`.

Support bundles expose pool metadata with `password_set`, but never include
pool passwords.

## Pool Policy

The pool policy is the first contract for the future stratum engine. It
prioritizes low pool latency, fast job processing, and stable persistent
connections over aggressive reconnect loops.

```toml
[pool_policy]
latency_warning_ms = 500
job_processing_budget_ms = 50
reconnect_min_interval_seconds = 15
failover_cooldown_seconds = 60
keepalive_interval_seconds = 30
```

Validation rules:

- `latency_warning_ms` must be greater than zero,
- `job_processing_budget_ms` must be greater than zero,
- `job_processing_budget_ms` must not exceed `latency_warning_ms`,
- `reconnect_min_interval_seconds` must be greater than zero,
- `failover_cooldown_seconds` must be at least `reconnect_min_interval_seconds`,
- `keepalive_interval_seconds` must be greater than zero.

Build `0.1.0` exposes policy and zeroed runtime counters only. Real stratum
latency, job processing, stale-job, and reconnect counters come with the network
engine.

The derived pool strategy is also exposed through API/UI. It sorts enabled
pools by priority, marks the lowest priority as active, keeps failover order
deterministic, requires persistent connections, and disallows reconnect jitter
in build `0.1.0`.

The same policy drives the read-only job pipeline contract:

- `job_processing_budget_ms` becomes the notify-to-dispatch budget,
- `latency_warning_ms` becomes the stale-job retirement window,
- pending job queue depth is capped at `2`,
- newest jobs are preferred,
- stale jobs are dropped after a new `prev_hash`.

## Stratum

Build `0.1.0` exposes a Stratum V1 status model and message classifier only.
It does not open sockets, subscribe, authorize, dispatch jobs, or submit
shares.

`mining.submit` is disabled in this build, but the local submit policy is
already explicit. Future submits must pass local prechecks for socket state,
subscription, authorization, active job, current difficulty, matching `job_id`,
and hex-encoded `extranonce2`, `ntime`, and `nonce` fields.

The future Stratum engine must follow the pool policy and job pipeline:

- keep the active pool connection persistent,
- suppress reconnect loops,
- process `mining.notify` inside the job budget,
- prefer the newest job,
- retire stale work quickly,
- locally precheck shares before `mining.submit`.

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

Autotune planning is intentionally conservative. The public plan order is:

1. baseline stock-like stability,
2. downclock efficiency test,
3. slow chip-by-chip upclock stability test,
4. voltage trim last, only after stable frequency results.

Build `0.1.0` exposes this plan through API/UI only. It does not write clocks
or voltages.

Validation rules:

- `target_value`, when present, must be positive and finite,
- `autotune = true` is rejected with `safe_mode`,
- `manual` is rejected in build `0.1.0`.
