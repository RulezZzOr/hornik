#!/usr/bin/env sh
set -eu

board="${1:?board is required}"
model="${2:?model is required}"
version="${3:?version is required}"
dist_dir="${4:-dist}"

case "$board" in
  s19-xil|s19-bb|s19-aml) ;;
  *)
    echo "unsupported MVP board: $board" >&2
    exit 2
    ;;
esac

case "$model" in
  s19|s19-pro|s19j|s19j-pro|s19-xp|t19) ;;
  *)
    echo "unsupported S19-class model: $model" >&2
    exit 2
    ;;
esac

mkdir -p "$dist_dir"
name="openmineros-${board}-${model}-${version}"
manifest="$dist_dir/${name}-manifest.json"
image="$dist_dir/${name}-install.img.xz"

cat > "$manifest" <<EOF_MANIFEST
{
  "name": "$name",
  "version": "$version",
  "board": "$board",
  "model": "$model",
  "kind": "development-placeholder",
  "flashable": false,
  "warning": "This is not a real firmware image. It is a reproducible build pipeline placeholder for OpenMinerOS 0.1.0."
}
EOF_MANIFEST

printf '%s\n' "OpenMinerOS $version placeholder artefact" \
  "board=$board" \
  "model=$model" \
  "flashable=false" > "$image"

(
  cd "$dist_dir"
  sha256sum "$(basename "$manifest")" "$(basename "$image")" > "${name}-sha256sum.txt"
)

echo "$manifest"
echo "$image"

