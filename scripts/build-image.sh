#!/usr/bin/env sh
set -eu

board="${1:?board is required}"
model="${2:?model is required}"
version="${3:?version is required}"
dist_dir="${4:-dist}"
media="${5:-all}"
script_dir="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
repo_dir="$(CDPATH= cd -- "$script_dir/.." && pwd)"
runtime_target="${OPENMINEROS_RUNTIME_TARGET:-native}"
cargo_target_root="${CARGO_TARGET_DIR:-$repo_dir/target}"

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
rootfs_base="$dist_abs/${name}-rootfs-base"
overlay="$script_dir/../buildroot/board/common/overlay"
runtime_binary_root="$cargo_target_root"
runtime_binary_label="$runtime_target"
runtime_binary_source=""
artifacts_json="$dist_abs/${name}-artifacts.json"

rm -f "$dist_abs/${name}-manifest.json" "$dist_abs/${name}-sha256sum.txt" "$dist_abs/${name}-"*.xz

if ! command -v xz >/dev/null 2>&1; then
  echo "xz is required to build install images" >&2
  exit 2
fi

case "$runtime_target" in
  native|host)
    runtime_binary_label="native"
    cargo build --manifest-path "$repo_dir/Cargo.toml" --release -p openmineros-control-plane
    runtime_binary_source="$runtime_binary_root/release/openmineros-control-plane"
    ;;
  *)
    cargo build --manifest-path "$repo_dir/Cargo.toml" --release -p openmineros-control-plane --target "$runtime_target"
    runtime_binary_source="$runtime_binary_root/$runtime_target/release/openmineros-control-plane"
    ;;
esac

if [ ! -x "$runtime_binary_source" ]; then
  echo "missing runtime binary after build: $runtime_binary_source" >&2
  exit 1
fi

case "$board:$media" in
  s19-xil:all) media_list="sd nand" ;;
  s19-xil:sd) media_list="sd" ;;
  s19-xil:nand) media_list="nand" ;;
  s19-bb:all|s19-bb:sd) media_list="sd" ;;
  s19-aml:all|s19-aml:otg) media_list="otg" ;;
  *)
    echo "unsupported install media '$media' for $board" >&2
    exit 2
    ;;
esac

rm -rf "$rootfs_base" "$dist_abs/${name}-"*"-rootfs"
mkdir -p "$rootfs_base"

if [ -d "$overlay" ]; then
  cp -R "$overlay"/. "$rootfs_base"/
fi

mkdir -p "$rootfs_base/etc/openmineros" "$rootfs_base/usr/share/openmineros" "$rootfs_base/var/log/openmineros"
mkdir -p "$rootfs_base/usr/bin"
cp "$runtime_binary_source" "$rootfs_base/usr/bin/openmineros-control-plane"
chmod 0755 "$rootfs_base/usr/bin/openmineros-control-plane"

cat > "$rootfs_base/etc/openmineros/release.json" <<EOF_RELEASE
{
  "name": "OpenMinerOS",
  "version": "$version",
  "board": "$board",
  "model": "$model",
  "runtime_backend": "hardware-probe",
  "runtime_binary": "/usr/bin/openmineros-control-plane",
  "runtime_target": "$runtime_binary_label",
  "install_media": "$media",
  "flashable": false
}
EOF_RELEASE

cat > "$rootfs_base/usr/share/openmineros/install-readme.txt" <<EOF_README
OpenMinerOS $version install bundle
board=$board
model=$model
runtime_target=$runtime_binary_label
runtime_binary=/usr/bin/openmineros-control-plane
install_media=$media
flashable=false
This bundle contains the compiled device-side runtime, safe boot hooks, and
the install-time launch script.
EOF_README

first_artifact=1
: > "$artifacts_json"

append_artifact() {
  artifact_path="$1"
  artifact_kind="$2"
  artifact_media="$3"
  artifact_target="$4"
  artifact_sha256="$(sha256sum "$dist_abs/$artifact_path" | awk '{print $1}')"
  artifact_bytes="$(wc -c < "$dist_abs/$artifact_path" | tr -d ' ')"

  if [ "$first_artifact" -eq 0 ]; then
    printf ',\n' >> "$artifacts_json"
  fi
  first_artifact=0

  cat >> "$artifacts_json" <<EOF_ARTIFACT
{
  "path": "$artifact_path",
  "kind": "$artifact_kind",
  "media": "$artifact_media",
  "install_target": "$artifact_target",
  "sha256": "$artifact_sha256",
  "bytes": $artifact_bytes
}
EOF_ARTIFACT
}

package_media() {
  package_media_name="$1"
  package_kind="$2"
  package_target="$3"
  package_suffix="$4"
  package_rootfs="$dist_abs/${name}-${package_media_name}-rootfs"
  package_file="${name}-${package_suffix}"
  package_path="$dist_abs/$package_file"
  package_filelist="$dist_abs/${name}-${package_media_name}-filelist.txt"

  rm -rf "$package_rootfs"
  mkdir -p "$package_rootfs"
  cp -R "$rootfs_base"/. "$package_rootfs"/

  cat > "$package_rootfs/etc/openmineros/release.json" <<EOF_RELEASE_MEDIA
{
  "name": "OpenMinerOS",
  "version": "$version",
  "board": "$board",
  "model": "$model",
  "runtime_backend": "hardware-probe",
  "runtime_binary": "/usr/bin/openmineros-control-plane",
  "runtime_target": "$runtime_binary_label",
  "install_media": "$package_media_name",
  "flashable": false
}
EOF_RELEASE_MEDIA

  cat > "$package_rootfs/etc/openmineros/install-media.json" <<EOF_MEDIA
{
  "schema_version": 1,
  "board": "$board",
  "model": "$model",
  "media": "$package_media_name",
  "target": "$package_target",
  "artifact_kind": "$package_kind",
  "runtime_binary": "/usr/bin/openmineros-control-plane",
  "runtime_target": "$runtime_binary_label",
  "flashable": false,
  "notes": [
    "development media model for S19 bring-up",
    "do not write NAND without a verified partition map and recovery path"
  ]
}
EOF_MEDIA

  cat > "$package_rootfs/usr/share/openmineros/install-plan-${package_media_name}.txt" <<EOF_PLAN
OpenMinerOS $version $package_media_name install model
board=$board
model=$model
target=$package_target
runtime=/usr/bin/openmineros-control-plane
backend=hardware-probe
flashable=false
EOF_PLAN

  find "$package_rootfs" -exec touch -t 197001010000 {} +
  (
    cd "$package_rootfs"
    find . -type f | LC_ALL=C sort > "$package_filelist"
    tar -cf - -T "$package_filelist" | xz -c > "$package_path"
  )
  rm -f "$package_filelist"

  append_artifact "$package_file" "$package_kind" "$package_media_name" "$package_target"
}

for target_media in $media_list; do
  case "$target_media" in
    sd)
      package_media "sd" "sd-card-image" "removable_sd" "sd-card.img.xz"
      ;;
    nand)
      package_media "nand" "nand-update-bundle" "onboard_nand" "nand-update.tar.xz"
      ;;
    otg)
      package_media "otg" "otg-recovery-bundle" "usb_otg" "otg-recovery.tar.xz"
      ;;
  esac
done

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
$(sed 's/^/    /' "$artifacts_json")
  ],
  "signature": null,
  "warning": "This is not a production-flashable firmware image yet. It packages the compiled device-side runtime, safe boot hooks, and S19 install media models for OpenMinerOS 0.1.0."
}
EOF_MANIFEST
rm -f "$artifacts_json"

(
  cd "$dist_dir"
  sha256sum "$(basename "$manifest")" ${name}-*.xz > "${name}-sha256sum.txt"
)

echo "$manifest"
for target_media in $media_list; do
  case "$target_media" in
    sd) echo "$dist_abs/${name}-sd-card.img.xz" ;;
    nand) echo "$dist_abs/${name}-nand-update.tar.xz" ;;
    otg) echo "$dist_abs/${name}-otg-recovery.tar.xz" ;;
  esac
done
