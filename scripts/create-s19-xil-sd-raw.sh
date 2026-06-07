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

resolve_tool() {
  for tool in "$@"; do
    if command -v "$tool" >/dev/null 2>&1; then
      command -v "$tool"
      return 0
    fi
  done

  if command -v brew >/dev/null 2>&1; then
    for tool in "$@"; do
      case "$tool" in
        mkfs.ext4|mke2fs)
          prefix="$(brew --prefix e2fsprogs 2>/dev/null || true)"
          [ -n "$prefix" ] && [ -x "$prefix/sbin/$tool" ] && {
            printf '%s\n' "$prefix/sbin/$tool"
            return 0
          }
          ;;
        mkfs.vfat|mkfs.fat)
          prefix="$(brew --prefix dosfstools 2>/dev/null || true)"
          [ -n "$prefix" ] && [ -x "$prefix/sbin/$tool" ] && {
            printf '%s\n' "$prefix/sbin/$tool"
            return 0
          }
          ;;
        mcopy|mmd|mdir|mdelete|mlabel|mmove|mren|mtype|mread|mwrite)
          prefix="$(brew --prefix mtools 2>/dev/null || true)"
          [ -n "$prefix" ] && [ -x "$prefix/bin/$tool" ] && {
            printf '%s\n' "$prefix/bin/$tool"
            return 0
          }
          ;;
      esac
    done
  fi

  return 1
}

require_tool() {
  tool_path="$(resolve_tool "$@")" || fail "missing required tool, need one of: $*"
  printf '%s\n' "$tool_path"
}

find_nonempty_by_name() {
  root="$1"
  name="$2"
  find "$root" -type f -name "$name" -print 2>/dev/null | while IFS= read -r candidate; do
    if [ -s "$candidate" ]; then
      printf '%s\n' "$candidate"
      break
    fi
  done
}

copy_first_named_as() {
  root="$1"
  destination="$2"
  target_name="$3"
  shift 3
  for name in "$@"; do
    found="$(find_nonempty_by_name "$root" "$name" | sed -n '1p')"
    if [ -n "$found" ]; then
      cp "$found" "$destination/$target_name"
      printf '%s\n' "$destination/$target_name"
      return 0
    fi
  done
  return 1
}

copy_optional_named() {
  root="$1"
  destination="$2"
  shift 2
  for name in "$@"; do
    found="$(find_nonempty_by_name "$root" "$name" | sed -n '1p')"
    [ -n "$found" ] || continue
    cp "$found" "$destination/$(basename "$found")"
  done
}

write_mbr() {
  image="$1"
  boot_start="$2"
  boot_sectors="$3"
  root_start="$4"
  root_sectors="$5"
  python3 - "$image" "$boot_start" "$boot_sectors" "$root_start" "$root_sectors" <<'PY'
import struct
import sys
from pathlib import Path

image = Path(sys.argv[1])
boot_start = int(sys.argv[2])
boot_sectors = int(sys.argv[3])
root_start = int(sys.argv[4])
root_sectors = int(sys.argv[5])

def entry(bootable, part_type, start, size):
    return struct.pack("<B3sB3sII", bootable, b"\x00\x00\x00", part_type, b"\x00\x00\x00", start, size)

mbr = bytearray(512)
mbr[446:462] = entry(0x80, 0x0C, boot_start, boot_sectors)
mbr[462:478] = entry(0x00, 0x83, root_start, root_sectors)
mbr[510:512] = b"\x55\xAA"

with image.open("r+b") as fh:
    fh.seek(0)
    fh.write(mbr)
PY
}

case "$board" in
  s19-xil) ;;
  *) fail "raw SD builder currently supports s19-xil only, got $board" ;;
esac

[ -n "$boot_assets" ] || fail "BOOT_ASSETS=/path/to/s19-xil-boot-files is required"
[ -d "$boot_assets" ] || fail "boot assets directory does not exist: $boot_assets"

if [ "$image_mib" -le "$boot_mib" ]; then
  fail "OPENMINEROS_SD_IMAGE_MIB must be larger than OPENMINEROS_SD_BOOT_MIB"
fi

cd "$repo_dir"
"$script_dir/prepare-sd-test.sh" "$board" "$model" "$version" "$dist_dir" >/dev/null
[ -f "$rootfs_artifact" ] || fail "missing rootfs artifact after package build: $rootfs_artifact"

tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/openmineros-sd-raw.XXXXXX")"
trap 'rm -rf "$tmp_dir"' EXIT INT TERM
rootfs_dir="$tmp_dir/rootfs"
boot_img="$tmp_dir/boot.vfat"
root_img="$tmp_dir/root.ext4"
boot_stage="$tmp_dir/boot-stage"
patched_update_image="$tmp_dir/update.image.gz"
mkdir -p "$rootfs_dir" "$boot_stage"

xz -dc "$rootfs_artifact" | (
  cd "$rootfs_dir"
  tar -xpf -
)

boot_start=2048
boot_sectors=$((boot_mib * 2048))
root_start=$((boot_start + boot_sectors))
root_guard_sectors=2048
image_sectors=$((image_mib * 2048))
root_sectors=$((image_sectors - root_start - root_guard_sectors))

if [ "$root_sectors" -le 0 ]; then
  fail "computed root partition is empty; increase OPENMINEROS_SD_IMAGE_MIB"
fi

boot_bytes=$((boot_sectors * 512))
root_bytes=$((root_sectors * 512))
image_bytes=$((image_sectors * 512))

kernel_source="$(copy_first_named_as "$boot_assets" "$boot_stage" uImage uImage image.ub || true)"
dtb_source="$(copy_first_named_as "$boot_assets" "$boot_stage" devicetree.dtb devicetree.dtb '*.dtb' || true)"

if [ -z "$kernel_source" ] || [ -z "$dtb_source" ]; then
  if [ "${OPENMINEROS_ALLOW_INCOMPLETE_BOOT_ASSETS:-0}" != "1" ]; then
    fail "boot assets must provide a bootable kernel (uImage/image.ub) and devicetree.dtb"
  fi
fi

if [ -f "$boot_assets/vendor-ramdisk.ext2" ]; then
  patched_update_image="$tmp_dir/update.image.gz"
  "$script_dir/patch-s19-xil-vendor-ramdisk.sh" \
    "$boot_assets/vendor-ramdisk.ext2" \
    "$rootfs_dir" \
    "$patched_update_image" >/dev/null
  cp "$patched_update_image" "$boot_stage/update.image.gz"
elif [ -f "$boot_assets/update.image.gz" ]; then
  cp "$boot_assets/update.image.gz" "$boot_stage/update.image.gz"
else
  if [ "${OPENMINEROS_ALLOW_INCOMPLETE_BOOT_ASSETS:-0}" != "1" ]; then
    fail "boot assets must include vendor-ramdisk.ext2 or update.image.gz"
  fi
fi

copy_optional_named "$boot_assets" "$boot_stage" uEnv.txt boot.scr system.bit.bin BOOT.BIN boot.bin

if [ ! -f "$boot_stage/update.image.gz" ]; then
  fail "boot stage missing update.image.gz after asset selection"
fi

mkfs_fat="$(require_tool mkfs.vfat mkfs.fat)"
mkfs_ext4="$(require_tool mkfs.ext4 mke2fs)"
mcopy_tool="$(require_tool mcopy)"

rm -f "$raw_img" "$raw_xz" "$metadata"
truncate -s "$boot_bytes" "$boot_img"
"$mkfs_fat" -n OMO_BOOT "$boot_img" >/dev/null
"$mcopy_tool" -i "$boot_img" -sp "$boot_stage"/* ::/

truncate -s "$root_bytes" "$root_img"
if [ "$(basename "$mkfs_ext4")" = "mke2fs" ]; then
  "$mkfs_ext4" -q -t ext4 -F -L OMO_ROOT -d "$rootfs_dir" "$root_img"
else
  "$mkfs_ext4" -q -F -L OMO_ROOT -d "$rootfs_dir" "$root_img"
fi

truncate -s "$image_bytes" "$raw_img"
write_mbr "$raw_img" "$boot_start" "$boot_sectors" "$root_start" "$root_sectors"

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
  "boot_chain": "sdboot -> uImage + devicetree.dtb + update.image.gz",
  "vendor_ramdisk_patched": $( [ -f "$boot_assets/vendor-ramdisk.ext2" ] && echo true || echo false ),
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
    "vendor ramdisk is patched with the OpenMinerOS overlay when vendor-ramdisk.ext2 is available",
    "verify on one sacrificial SD boot before any NAND or ASIC write path"
  ]
}
EOF_METADATA

echo "$raw_xz"
echo "$metadata"
