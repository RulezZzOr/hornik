#!/usr/bin/env sh
set -eu

source_path="${1:-${BOOT_SOURCE:-}}"
output_dir="${2:-${BOOT_ASSETS:-boot-assets/s19-xil}}"

fail() {
  echo "import-s19-xil-boot-assets: $*" >&2
  exit 1
}

sha256_file() {
  file="$1"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$file" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$file" | awk '{print $1}'
  else
    echo "unavailable"
  fi
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

copy_first_named() {
  root="$1"
  destination="$2"
  shift 2
  for name in "$@"; do
    found="$(find_nonempty_by_name "$root" "$name" | sed -n '1p')"
    if [ -n "$found" ]; then
      cp "$found" "$destination/$(basename "$found")"
      printf '%s\n' "$destination/$(basename "$found")"
      return 0
    fi
  done
  return 1
}

copy_optional_matches() {
  root="$1"
  destination="$2"
  shift 2
  for pattern in "$@"; do
    find "$root" -type f -name "$pattern" -print 2>/dev/null | while IFS= read -r found; do
      if [ -s "$found" ]; then
        cp "$found" "$destination/$(basename "$found")"
      fi
    done
  done
}

[ -n "$source_path" ] || fail "usage: $0 /path/to/stock-recovery-dir-or-archive [boot-assets/s19-xil]"
[ -e "$source_path" ] || fail "source does not exist: $source_path"

tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/openmineros-boot-assets.XXXXXX")"
trap 'rm -rf "$tmp_dir"' EXIT INT TERM

case "$source_path" in
  *.zip)
    command -v unzip >/dev/null 2>&1 || fail "unzip is required for zip archives"
    mkdir -p "$tmp_dir/extract"
    unzip -q "$source_path" -d "$tmp_dir/extract"
    search_root="$tmp_dir/extract"
    ;;
  *.tar|*.tar.gz|*.tgz|*.tar.xz|*.txz|*.tar.bz2|*.tbz2)
    mkdir -p "$tmp_dir/extract"
    tar -xf "$source_path" -C "$tmp_dir/extract"
    search_root="$tmp_dir/extract"
    ;;
  *.img|*.img.xz|*.dmg)
    fail "mount raw images first, then pass the mounted boot partition or extracted directory"
    ;;
  *)
    if [ -d "$source_path" ]; then
      search_root="$source_path"
    else
      fail "unsupported source type; pass a directory, .zip, or tar archive"
    fi
    ;;
esac

rm -rf "$output_dir"
mkdir -p "$output_dir"

bootloader="$(copy_first_named "$search_root" "$output_dir" BOOT.BIN boot.bin || true)"
kernel="$(copy_first_named "$search_root" "$output_dir" image.ub uImage zImage Image fit.itb || true)"
copy_optional_matches "$search_root" "$output_dir" '*.dtb' '*.dtbo' 'uEnv.txt' 'boot.scr' 'extlinux.conf' '*.bit'

[ -n "$bootloader" ] || fail "missing non-empty BOOT.BIN or boot.bin in source"
[ -n "$kernel" ] || fail "missing non-empty kernel image: image.ub, uImage, zImage, Image, or fit.itb"

manifest="$output_dir/boot-assets.json"
{
  echo "{"
  echo "  \"schema_version\": 1,"
  echo "  \"board\": \"s19-xil\","
  echo "  \"source\": \"$source_path\","
  echo "  \"bootloader\": \"$(basename "$bootloader")\","
  echo "  \"kernel\": \"$(basename "$kernel")\","
  echo "  \"files\": ["
  first=1
  find "$output_dir" -type f ! -name 'boot-assets.json' -print | LC_ALL=C sort | while IFS= read -r file; do
    if [ "$first" -eq 0 ]; then
      echo "    ,"
    fi
    first=0
    file_name="$(basename "$file")"
    file_sha="$(sha256_file "$file")"
    file_bytes="$(wc -c < "$file" | tr -d ' ')"
    printf '    {"path":"%s","sha256":"%s","bytes":%s}\n' "$file_name" "$file_sha" "$file_bytes"
  done
  echo "  ],"
  echo "  \"notes\": ["
  echo "    \"operator-supplied vendor boot assets; not committed into the repo\","
  echo "    \"use as BOOT_ASSETS input for make sd-raw-image\""
  echo "  ]"
  echo "}"
} > "$manifest"

echo "$output_dir"
echo "$manifest"
