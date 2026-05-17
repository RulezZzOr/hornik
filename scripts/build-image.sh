#!/usr/bin/env sh
set -eu

board="${1:?board is required}"
model="${2:?model is required}"
version="${3:?version is required}"
dist_dir="${4:-dist}"
script_dir="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"

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
dist_abs="$(cd "$dist_dir" && pwd)"
name="openmineros-${board}-${model}-${version}"
manifest="$dist_abs/${name}-manifest.json"
image="$dist_abs/${name}-install.img.xz"
rootfs="$dist_abs/${name}-rootfs"
overlay="$script_dir/../buildroot/board/common/overlay"

if ! command -v xz >/dev/null 2>&1; then
  echo "xz is required to build install images" >&2
  exit 2
fi

rm -rf "$rootfs"
mkdir -p "$rootfs"

if [ -d "$overlay" ]; then
  cp -R "$overlay"/. "$rootfs"/
fi

mkdir -p "$rootfs/etc/openmineros" "$rootfs/usr/share/openmineros" "$rootfs/var/log/openmineros"

cat > "$rootfs/etc/openmineros/release.json" <<EOF_RELEASE
{
  "name": "OpenMinerOS",
  "version": "$version",
  "board": "$board",
  "model": "$model",
  "runtime_backend": "hardware-probe",
  "flashable": false
}
EOF_RELEASE

cat > "$rootfs/usr/share/openmineros/install-readme.txt" <<EOF_README
OpenMinerOS $version install bundle
board=$board
model=$model
flashable=false
This bundle contains the device-side runtime layout and safe boot hooks.
EOF_README

find "$rootfs" -exec touch -t 197001010000 {} +
(
  cd "$rootfs"
  find . -type f | LC_ALL=C sort > "$dist_abs/${name}-filelist.txt"
  tar -cf - -T "$dist_abs/${name}-filelist.txt" | xz -c > "$image"
)
rm -f "$dist_abs/${name}-filelist.txt"

image_file="$(basename "$image")"
image_sha256="$(sha256sum "$image" | awk '{print $1}')"
image_bytes="$(wc -c < "$image" | tr -d ' ')"

cat > "$manifest" <<EOF_MANIFEST
{
  "schema_version": 1,
  "name": "$name",
  "version": "$version",
  "board": "$board",
  "model": "$model",
  "kind": "development-install-bundle",
  "flashable": false,
  "artifacts": [
    {
      "path": "$image_file",
      "kind": "install-image",
      "sha256": "$image_sha256",
      "bytes": $image_bytes
    }
  ],
  "signature": null,
  "warning": "This is not a flashable firmware image yet. It packages the device-side runtime layout and safe boot hooks for OpenMinerOS 0.1.0."
}
EOF_MANIFEST

(
  cd "$dist_dir"
  sha256sum "$(basename "$manifest")" "$(basename "$image")" > "${name}-sha256sum.txt"
)

echo "$manifest"
echo "$image"
