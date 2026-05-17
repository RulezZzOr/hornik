# Install

Build `0.1.0` is install-prep ready, but it does **not** ship a flashable S19
firmware image yet. The current release flow prepares and verifies a
reproducible install bundle with the compiled device-side runtime binary,
launch script, and recovery hooks for each board family.

## Current Install Prep

```bash
make install-preflight BOARD=s19-xil MODEL=s19j-pro
make install-preflight BOARD=s19-xil MODEL=s19j-pro MEDIA=sd
make install-preflight BOARD=s19-xil MODEL=s19j-pro MEDIA=nand
OPENMINEROS_RUNTIME_TARGET=native make install-preflight BOARD=s19-xil MODEL=s19j-pro
```

That command:

1. builds the install bundle,
2. verifies the manifest,
3. checks the artefact budget,
4. prints the install plan for the generated bundle.

For a real board build, set `OPENMINEROS_RUNTIME_TARGET` to the Rust target
triple used by the board image. Leave it unset or set it to `native` for local
development bundles.

For `s19-xil`, `MEDIA=all` emits both removable SD and onboard NAND artefacts:

- `openmineros-s19-xil-s19j-pro-0.1.0-sd-card.img.xz`
- `openmineros-s19-xil-s19j-pro-0.1.0-nand-update.tar.xz`

## Release Checks

Before installation, always verify the bundle:

```bash
make image BOARD=s19-xil MODEL=s19j-pro
cargo run -p openmineros-commander -- verify-manifest \
  --manifest dist/openmineros-s19-xil-s19j-pro-0.1.0-manifest.json
cargo run -p openmineros-commander -- check-manifest-budget \
  --manifest dist/openmineros-s19-xil-s19j-pro-0.1.0-manifest.json
cargo run -p openmineros-commander -- install-plan \
  --manifest dist/openmineros-s19-xil-s19j-pro-0.1.0-manifest.json
```

## Board Recovery Paths

### Xilinx / Zynq

- Preferred recovery path: external SD image.
- Use this path first for bring-up and rollback.
- NAND staging is modelled as a separate bundle and must not be written until
  the partition map, bootloader handoff, and recovery path are verified.

### BeagleBone Black

- Preferred recovery path: removable SD media.
- Keep the original recovery media with the board label.

### Amlogic

- Preferred recovery path: OTG / USB recovery.
- Do not rely on a web-only installer as the only recovery path.

## What Will Be Needed For Real Installation

When the flashable image lands, installation will require:

- a signed release manifest,
- a flashable install image,
- a verified recovery medium or update slot,
- a post-boot health check,
- rollback instructions for the active/inactive slot pair.

The development bundle already carries the runtime binary and boot hooks, so
the remaining gap is the flashable image and update/rollback path.

## Post-Install Check

After a successful boot, confirm:

```bash
curl -s http://127.0.0.1:8080/api/v1/hardware/readiness | jq
curl -s http://127.0.0.1:8080/api/v1/firmware/deployment | jq
```

The target is not considered ready until the safety gate, hardware identity,
and deployment report all agree on the same board family and model.
