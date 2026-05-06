# Optional Development Contribution

OpenMinerOS does not have a mandatory devfee.

The contribution model is opt-in, transparent, and auditable:

- default is `0.0 %`,
- allowed range is `0.0 %` to `3.0 %`,
- activation must be explicit,
- the beneficiary and pool endpoints are public,
- no hidden fallback pool is allowed,
- if the contribution endpoint fails, mining returns to the operator's pools.

## Default Beneficiary

```text
bc1qp6d4vxmenug97ghcy027vsn3902yadcj77ka6j
```

## Default Endpoints

```text
stratum+tcp://ss.antpool.com:3333
stratum+tcp://ss.antpool.com:443
stratum+tcp://ss.antpool.com:25
```

These defaults are only used after the operator enables contribution.

Pool URL, worker naming, reward mode, and payout settings must be verified
against the chosen pool account before production use.

## Prohibited Behavior

- hidden wallet addresses,
- obfuscated pool URLs,
- automatic activation,
- fallback contribution pools not shown in UI/API,
- contribution mining when the operator selected `0.0 %`.

