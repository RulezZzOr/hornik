# Release

Build `0.1.0` establishes the release safety contract before real SD images
exist.

## Manifest Policy

Each release bundle must include a manifest with:

- schema version,
- board and model,
- artefact list,
- byte lengths,
- SHA-256 hashes,
- signature metadata.

The verifier command is:

```bash
cargo run -p openmineros-commander -- verify-manifest \
  --manifest dist/openmineros-s19-xil-s19j-pro-0.1.0-manifest.json
```

## Signature Policy

Non-flashable development placeholders may be unsigned.

Flashable artefacts must be signed. The verifier rejects flashable unsigned
manifests by default. `--allow-flashable-unsigned` exists only for isolated lab
testing and must not be used for production release gates.

## Size Budget

Release candidates must pass:

```bash
make size-budget
```

Default `0.1.0` budgets:

- `openmineros-control-plane`: 12 MiB
- `openmineros-commander`: 8 MiB
- `web/`: 512 KiB

Budget increases require an explicit release note because Antminer control
boards have limited storage and RAM headroom.

## Contribution Target Integrity

Official builds lock the optional contribution beneficiary and endpoints in
source. Runtime configuration can only change whether contribution is enabled
and what percentage is used. Official release signatures are the mechanism used
to detect source or artefact changes before installation.
