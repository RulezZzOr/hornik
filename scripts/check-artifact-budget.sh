#!/usr/bin/env sh
set -eu

version="${1:-0.1.0}"
dist_dir="${2:-dist}"
model="${3:-s19j-pro}"

boards="s19-xil s19-bb s19-aml"
max_install_image_bytes="${MAX_INSTALL_IMAGE_BYTES:-67108864}"
max_total_artifact_bytes="${MAX_TOTAL_ARTIFACT_BYTES:-83886080}"

rm -rf "$dist_dir"
mkdir -p "$dist_dir"

for board in $boards; do
  ./scripts/build-image.sh "$board" "$model" "$version" "$dist_dir" >/dev/null
  manifest="$dist_dir/openmineros-${board}-${model}-${version}-manifest.json"

  cargo run -q -p openmineros-commander -- check-manifest-budget \
    --manifest "$manifest" \
    --max-install-image-bytes "$max_install_image_bytes" \
    --max-total-artifact-bytes "$max_total_artifact_bytes" >/dev/null

  printf 'artifact-budget: %-8s install media artifacts under %s bytes each\n' "$board" "$max_install_image_bytes"
done
