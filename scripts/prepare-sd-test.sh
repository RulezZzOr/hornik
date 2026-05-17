#!/usr/bin/env sh
set -eu

board="${1:-s19-xil}"
model="${2:-s19j-pro}"
version="${3:-0.1.0}"
dist_dir="${4:-dist}"
script_dir="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
repo_dir="$(CDPATH= cd -- "$script_dir/.." && pwd)"
name="openmineros-${board}-${model}-${version}"
manifest="$dist_dir/${name}-manifest.json"
artifact="$dist_dir/${name}-sd-card.img.xz"
package_dir="$dist_dir/${name}-sd-test"

fail() {
  echo "prepare-sd-test: $*" >&2
  exit 1
}

case "$board" in
  s19-xil) ;;
  *) fail "safe SD first-boot package currently supports s19-xil only, got $board" ;;
esac

cd "$repo_dir"

"$script_dir/build-image.sh" "$board" "$model" "$version" "$dist_dir" sd >/dev/null
"$script_dir/verify-sd-test-bundle.sh" "$board" "$model" "$version" "$dist_dir" >/dev/null

rm -rf "$package_dir"
mkdir -p "$package_dir"

cp "$manifest" "$package_dir/"
cp "$artifact" "$package_dir/"
cp "$dist_dir/${name}-sha256sum.txt" "$package_dir/"
cp "$script_dir/collect-s19-first-boot.sh" "$package_dir/"
cp "$script_dir/extract-sd-test-rootfs.sh" "$package_dir/"

cat > "$package_dir/README.md" <<EOF_README
# OpenMinerOS S19 Xilinx SD First-Boot Test Package

This package is for read-only first boot validation only.

It is not a production flashable firmware image and must not be written to NAND.

## Contents

- ${name}-sd-card.img.xz
- ${name}-manifest.json
- ${name}-sha256sum.txt
- collect-s19-first-boot.sh
- extract-sd-test-rootfs.sh

## Local Verification

\`\`\`bash
./scripts/verify-sd-test-bundle.sh "$board" "$model" "$version" "$dist_dir"
\`\`\`

## Rootfs Staging

The current SD artifact is a compressed rootfs tar model, not a raw partitioned
disk image. Extract it only to an already prepared and mounted SD rootfs:

\`\`\`bash
OPENMINEROS_ALLOW_SD_ROOTFS_EXTRACT=1 \\
  ./scripts/extract-sd-test-rootfs.sh \\
  "$artifact" \\
  /Volumes/OPENMINEROS_ROOT
\`\`\`

## First Boot Collection

After the miner boots and gets an IP address:

\`\`\`bash
MINER_HOST=<miner-ip> MINER_USER=root \\
  ./scripts/collect-s19-first-boot.sh
\`\`\`

Required first-boot state:

- backend: hardware-probe
- safe_to_first_boot: true
- nand_writes_allowed: false
- asic_writes_allowed: false
- flashing_allowed: false
EOF_README

echo "$package_dir"
