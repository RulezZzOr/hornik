#!/usr/bin/env sh
set -eu

vendor_ramdisk="${1:-}"
overlay_dir="${2:-}"
output_image="${3:-}"

fail() {
  echo "patch-s19-xil-vendor-ramdisk: $*" >&2
  exit 1
}

require_tool() {
  command -v "$1" >/dev/null 2>&1 || fail "missing required tool: $1"
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
        mkfs.ext4|e2fsck|mke2fs|resize2fs|tune2fs)
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
        mcopy|mdelete|mdir|mlabel|mmove|mren|mtype|mread|mwrite|mmd)
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

overlay_path() {
  base="$1"
  path="$2"
  if [ "$path" = "." ]; then
    printf '%s\n' "$base"
  else
    printf '%s/%s\n' "$base" "$path"
  fi
}

copy_tree_to_ext2() {
  src="$1"
  fs="$2"

  find "$src" -type d -print | LC_ALL=C sort | while IFS= read -r dir; do
    rel="${dir#$src}"
    rel="${rel#/}"
    [ -z "$rel" ] && continue
    e2mkdir "$fs:$rel" >/dev/null 2>&1 || true
  done

  find "$src" -type f -print | LC_ALL=C sort | while IFS= read -r file; do
    rel="${file#$src}"
    rel="${rel#/}"
    parent="$(dirname "$rel")"
    [ "$parent" = "." ] || e2mkdir "$fs:$parent" >/dev/null 2>&1 || true
    e2rm "$fs:$rel" >/dev/null 2>&1 || true
    e2cp -p "$file" "$fs:$rel"
  done

  find "$src" -type l -print | LC_ALL=C sort | while IFS= read -r link; do
    rel="${link#$src}"
    rel="${rel#/}"
    parent="$(dirname "$rel")"
    [ "$parent" = "." ] || e2mkdir "$fs:$parent" >/dev/null 2>&1 || true
    e2rm "$fs:$rel" >/dev/null 2>&1 || true
    e2ln -s "$(readlink "$link")" "$fs:$rel"
  done
}

[ -n "$vendor_ramdisk" ] || fail "usage: $0 /path/to/vendor-ramdisk.ext2 /path/to/overlay-root /path/to/output/update.image.gz"
[ -n "$overlay_dir" ] || fail "usage: $0 /path/to/vendor-ramdisk.ext2 /path/to/overlay-root /path/to/output/update.image.gz"
[ -n "$output_image" ] || fail "usage: $0 /path/to/vendor-ramdisk.ext2 /path/to/overlay-root /path/to/output/update.image.gz"
[ -f "$vendor_ramdisk" ] || fail "missing vendor ramdisk image: $vendor_ramdisk"
[ -d "$overlay_dir" ] || fail "missing overlay directory: $overlay_dir"

require_tool e2cp
require_tool e2mkdir
require_tool e2rm
require_tool e2ln
require_tool gzip
require_tool mkimage

if e2fsck_tool="$(resolve_tool e2fsck 2>/dev/null)"; then
  :
else
  e2fsck_tool=""
fi

tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/openmineros-ramdisk.XXXXXX")"
trap 'rm -rf "$tmp_dir"' EXIT INT TERM

patched_ext2="$tmp_dir/vendor-ramdisk-patched.ext2"
gzip_payload="$tmp_dir/vendor-ramdisk-patched.ext2.gz"

cp "$vendor_ramdisk" "$patched_ext2"

if [ -n "$e2fsck_tool" ]; then
  "$e2fsck_tool" -fy "$patched_ext2" >/dev/null 2>&1 || true
fi

copy_tree_to_ext2 "$overlay_dir" "$patched_ext2"

if [ -n "$e2fsck_tool" ]; then
  "$e2fsck_tool" -fy "$patched_ext2" >/dev/null 2>&1 || true
fi

gzip -n -c "$patched_ext2" > "$gzip_payload"

SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-0}" \
  mkimage -A arm -O linux -T ramdisk -C gzip -a 0 -e 0 -n ramdisk -d "$gzip_payload" "$output_image" >/dev/null

echo "$output_image"
