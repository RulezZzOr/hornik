# Optional Development Contribution

OpenMinerOS does not have a mandatory devfee.

The contribution model is opt-in, transparent, and auditable:

- default is `0.0 %`,
- allowed range is `0.0 %` to `3.0 %`,
- activation must be explicit,
- the beneficiary and pool endpoints are public,
- the beneficiary and pool endpoints are locked in official builds,
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

## Mutability

Official OpenMinerOS builds allow the operator to change only:

- `enabled`
- `rate_percent`

The beneficiary address and contribution endpoints are not config values in the
official build. A fork can change source code because this is GPL open source,
but it must publish those changes and it will not match official signed release
artefacts.

The config parser uses a closed schema. Adding keys such as `beneficiary`,
`endpoint`, or `endpoints` under `[contribution]` is rejected.

## Prohibited Behavior

- hidden wallet addresses,
- obfuscated pool URLs,
- automatic activation,
- fallback contribution pools not shown in UI/API,
- runtime config overrides for beneficiary or endpoint in official builds,
- contribution mining when the operator selected `0.0 %`.
