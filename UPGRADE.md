# Upgrade

Build `0.1.0` exposes update status only. It does not yet write slots, flash
images, or perform an in-place upgrade on real hardware.

## Current Behavior

- The active slot is reported.
- The inactive slot is reported.
- Rollback availability is surfaced through API and UI.
- No slot mutation happens in this build.
- `s19-xil` NAND output is a staging bundle only; it is not a writer.
- `/usr/bin/openmineros-nand-update` is a guard stub and must refuse writes.

## Future Upgrade Flow

Once flashable releases exist, upgrades should follow this order:

1. verify the release manifest,
2. confirm the image hash and signature,
3. write the inactive slot,
4. reboot into the new slot,
5. confirm health,
6. keep rollback available until the new slot is stable.

## Recovery Rule

Never remove the last known-good slot until the new image has passed boot and
health checks.

## Operator Rule

If the hardware identity, readiness report, or safety gate disagree during an
upgrade, stop and recover before attempting another write.

For Xilinx/Zynq boards, validate the SD recovery path before any NAND attempt.
