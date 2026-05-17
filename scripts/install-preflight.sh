#!/usr/bin/env sh
set -eu

board="${1:-s19-xil}"
model="${2:-s19j-pro}"
version="${3:-0.1.0}"
dist_dir="${4:-dist}"
media="${5:-all}"

./scripts/build-image.sh "$board" "$model" "$version" "$dist_dir" "$media" >/dev/null
manifest="$dist_dir/openmineros-${board}-${model}-${version}-manifest.json"

cargo run -q -p openmineros-commander -- install-plan \
  --manifest "$manifest" \
  --artifact-dir "$dist_dir"
