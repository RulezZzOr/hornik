#!/usr/bin/env sh
set -eu

board="${1:-s19-xil}"
model="${2:-s19}"
version="${3:-0.1.0}"
dist_dir="${4:-dist}"
boot_assets="${5:-${BOOT_ASSETS:-}}"
script_dir="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
repo_dir="$(CDPATH= cd -- "$script_dir/.." && pwd)"
image_mib="${OPENMINEROS_SD_IMAGE_MIB:-512}"
boot_mib="${OPENMINEROS_SD_BOOT_MIB:-64}"
name="openmineros-${board}-${model}-${version}"
sd_package="$dist_dir/${name}-sd-test"
rootfs_artifact="$dist_dir/${name}-sd-card.img.xz"
raw_img="$dist_dir/${name}-sd-raw-candidate.img"
raw_xz="$raw_img.xz"
metadata="$dist_dir/${name}-sd-raw-candidate.json"

fail() {
  echo "create-s19-xil-sd-raw: $*" >&2
  exit 1
}

require_tool() {
  command -v "$1" >/dev/null 2>&1 || fail "missing required tool: $1"
}

require_one_tool() {
  for tool in "$@"; do
    if command -v "$tool" >/dev/null 2>&1; then
      printf '%s\n' "$tool"
      return 0
    fi
  done
  fail "missing required tool, need one of: $*"
}

case "$board" in
  s19-xil) ;;
  *) fail "raw SD builder currently supports s19-xil only, got $board" ;;
esac

[ -n "$boot_assets" ] || fail "BOOT_ASSETS=/path/to/s19-xil-boot-files is required"
[ -d "$boot_assets" ] || fail "boot assets directory does not exist: $boot_assets"

case "$(uname -s)" in
  Linux) ;;
  *)
    fail "raw SD composition requires Linux tools. On macOS, run this inside a Linux VM/container, then write the produced .img.xz from macOS."
    ;;
esac

if [ "$image_mib" -le "$boot_mib" ]; then
  fail "OPENMINEROS_SD_IMAGE_MIB must be larger than OPENMINEROS_SD_BOOT_MIB"
fi

has_bootloader=false
has_kernel=false
if [ -f "$boot_assets/BOOT.BIN" ] || [ -f "$boot_assets/boot.bin" ]; then
  has_bootloader=true
fi
for kernel in image.ub uImage zImage Image fit.itb; do
  if [ -f "$boot_assets/$kernel" ]; then
    has_kernel=true
  fi
done

if [ "$has_bootloader" != true ] || [ "$has_kernel" != true ]; then
  if [ "${OPENMINEROS_ALLOW_INCOMPLETE_BOOT_ASSETS:-0}" != "1" ]; then
    fail "boot assets must contain BOOT.BIN/boot.bin and one kernel image: image.ub, uImage, zImage, Image, or fit.itb"
  fi
fi

require_tool sfdisk
require_tool dd
require_tool truncate
require_tool tar
require_tool xz
require_tool mcopy
mkfs_fat="$(require_one_tool mkfs.vfat mkfs.fat)"
mkfs_ext4="$(require_one_tool mkfs.ext4 mke2fs)"

cd "$repo_dir"
"$script_dir/prepare-sd-test.sh" "$board" "$model" "$version" "$dist_dir" >/dev/null
[ -f "$rootfs_artifact" ] || fail "missing rootfs artifact after package build: $rootfs_artifact"

tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/openmineros-sd-raw.XXXXXX")"
trap 'rm -rf "$tmp_dir"' EXIT INT TERM
rootfs_dir="$tmp_dir/rootfs"
boot_img="$tmp_dir/boot.vfat"
root_img="$tmp_dir/root.ext4"
mkdir -p "$rootfs_dir"

xz -dc "$rootfs_artifact" | (
  cd "$rootfs_dir"
  tar -xpf -
)

image_sectors=$((image_mib * 2048))
boot_start=2048
boot_sectors=$((boot_mib * 2048))
root_start=$((boot_start + boot_sectors))
root_guard_sectors=2048
root_sectors=$((image_sectors - root_start - root_guard_sectors))

if [ "$root_sectors" -le 0 ]; then
  fail "computed root partition is empty; increase OPENMINEROS_SD_IMAGE_MIB"
fi

boot_bytes=$((boot_sectors * 512))
root_bytes=$((root_sectors * 512))
image_bytes=$((image_sectors * 512))

rm -f "$raw_img" "$raw_xz" "$metadata"
truncate -s "$boot_bytes" "$boot_img"
"$mkfs_fat" -n OMO_BOOT "$boot_img" >/dev/null
mcopy -i "$boot_img" -sp "$boot_assets"/* ::/

truncate -s "$root_bytes" "$root_img"
if [ "$(basename "$mkfs_ext4")" = "mke2fs" ]; then
  "$mkfs_ext4" -q -t ext4 -F -L OMO_ROOT -d "$rootfs_dir" "$root_img"
else
  "$mkfs_ext4" -q -F -L OMO_ROOT -d "$rootfs_dir" "$root_img"
fi

truncate -s "$image_bytes" "$raw_img"
sfdisk "$raw_img" >/dev/null <<EOF_SFDISK
label: dos
unit: sectors

start=$boot_start, size=$boot_sectors, type=c, bootable
start=$root_start, size=$root_sectors, type=83
EOF_SFDISK

dd if="$boot_img" of="$raw_img" bs=512 seek="$boot_start" conv=notrunc status=none
dd if="$root_img" of="$raw_img" bs=512 seek="$root_start" conv=notrunc status=none
xz -f -k "$raw_img"

raw_sha256="$(sha256sum "$raw_xz" | awk '{print $1}')"
raw_bytes="$(wc -c < "$raw_xz" | tr -d ' ')"

cat > "$metadata" <<EOF_METADATA
{
  "schema_version": 1,
  "board": "$board",
  "model": "$model",
  "version": "$version",
  "kind": "sd-raw-candidate",
  "raw_image": "$(basename "$raw_img")",
  "compressed_image": "$(basename "$raw_xz")",
  "compressed_sha256": "$raw_sha256",
  "compressed_bytes": $raw_bytes,
  "image_mib": $image_mib,
  "boot_partition_mib": $boot_mib,
  "root_partition_mib": $((root_sectors / 2048)),
  "boot_assets_source": "$boot_assets",
  "first_boot_safe": true,
  "nand_writes_allowed": false,
  "asic_writes_allowed": false,
  "ssh": {
    "daemon": "dropbear",
    "port": 22,
    "password_login": false,
    "test_key_package": "$sd_package/ssh/id_ed25519"
  },
  "notes": [
    "candidate image assembled from caller-provided S19 XIL boot assets",
    "verify on one sacrificial SD boot before any NAND or ASIC write path"
  ]
}
EOF_METADATA

echo "$raw_xz"
echo "$metadata"
