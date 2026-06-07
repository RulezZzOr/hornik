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
  *.bmu)
    command -v python3 >/dev/null 2>&1 || fail "python3 is required to extract .bmu boot assets"
    mkdir -p "$tmp_dir/extract"
    python3 - "$source_path" "$tmp_dir/extract" <<'PY'
import gzip
import json
import struct
import sys
from pathlib import Path

source = Path(sys.argv[1]).read_bytes()
out_dir = Path(sys.argv[2])

UIMAGE_MAGIC = b"\x27\x05\x19\x56"
FDT_MAGIC = b"\xd0\x0d\xfe\xed"

def write(path, data):
    path.write_bytes(data)

uimages = []
offset = 0
while True:
    found = source.find(UIMAGE_MAGIC, offset)
    if found < 0 or found + 64 > len(source):
        break
    size = struct.unpack(">I", source[found + 12 : found + 16])[0]
    end = found + 64 + size
    if end > len(source):
        offset = found + 4
        continue
    header = source[found : found + 64]
    payload = source[found + 64 : end]
    name = header[32:64].split(b"\0", 1)[0].decode("ascii", "ignore")
    uimages.append(
        {
            "offset": found,
            "size": size,
            "end": end,
            "name": name,
            "type": header[30],
            "payload": payload,
            "blob": source[found:end],
        }
    )
    offset = found + 4

if not uimages:
    raise SystemExit("no uImage blobs found in .bmu")

kernel_candidates = [item for item in uimages if item["type"] == 2]
ramdisk_candidates = [item for item in uimages if item["type"] == 3]

if not kernel_candidates:
    raise SystemExit("no kernel uImage candidate found in .bmu")
if not ramdisk_candidates:
    raise SystemExit("no ramdisk uImage candidate found in .bmu")

def pick_best_kernel(items):
    return sorted(
        items,
        key=lambda item: (
            1 if "Linux" in item["name"] else 0,
            item["size"],
            -item["offset"],
        ),
        reverse=True,
    )[0]

def pick_best_ramdisk(items):
    return sorted(
        items,
        key=lambda item: (
            item["size"],
            1 if "ramdisk" in item["name"].lower() else 0,
            -item["offset"],
        ),
        reverse=True,
    )[0]

def dtb_score(blob):
    text = blob.decode("latin1", "ignore")
    score = 0
    for token in ("xlnx,zynq", "has-power", "has-modem", "has-wp", "has-ecc"):
        if token in text:
            score += 10
    if "microzed" in text:
        score -= 30
    if "zynq-7000" in text:
        score += 5
    return score

dtb_candidates = []
offset = 0
while True:
    found = source.find(FDT_MAGIC, offset)
    if found < 0 or found + 8 > len(source):
        break
    size = struct.unpack(">I", source[found + 4 : found + 8])[0]
    end = found + size
    if end > len(source):
        offset = found + 4
        continue
    blob = source[found:end]
    dtb_candidates.append(
        {
            "offset": found,
            "size": size,
            "end": end,
            "blob": blob,
            "score": dtb_score(blob),
        }
    )
    offset = found + 4

if not dtb_candidates:
    raise SystemExit("no devicetree blob candidate found in .bmu")

kernel = pick_best_kernel(kernel_candidates)
ramdisk = pick_best_ramdisk(ramdisk_candidates)
dtb = sorted(dtb_candidates, key=lambda item: (item["score"], item["size"], -item["offset"]), reverse=True)[0]

write(out_dir / "uImage", kernel["blob"])
write(out_dir / "update.image.gz", ramdisk["blob"])
write(out_dir / "vendor-ramdisk.ext2", gzip.decompress(ramdisk["payload"]))
write(out_dir / "devicetree.dtb", dtb["blob"])

(out_dir / "bmu-assets.json").write_text(
    json.dumps(
        {
            "schema_version": 1,
            "source": str(Path(sys.argv[1])),
            "kernel_offset": kernel["offset"],
            "ramdisk_offset": ramdisk["offset"],
            "devicetree_offset": dtb["offset"],
            "vendor_ramdisk_bytes": len(gzip.decompress(ramdisk["payload"])),
        },
        indent=2,
    )
    + "\n"
)
PY
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

bootloader="$(copy_first_named "$search_root" "$output_dir" BOOT.BIN boot.bin uImage image.ub || true)"
kernel="$(copy_first_named "$search_root" "$output_dir" uImage image.ub zImage Image fit.itb || true)"
ramdisk="$(copy_first_named "$search_root" "$output_dir" update.image.gz || true)"
copy_optional_matches "$search_root" "$output_dir" '*.dtb' '*.dtbo' 'uEnv.txt' 'boot.scr' 'extlinux.conf' '*.bit' 'vendor-ramdisk.ext2' 'bmu-assets.json'

[ -n "$bootloader" ] || bootloader="$kernel"

[ -n "$bootloader" ] || fail "missing non-empty BOOT.BIN or boot.bin in source"
[ -n "$kernel" ] || fail "missing non-empty kernel image: image.ub, uImage, zImage, Image, or fit.itb"
[ -n "$ramdisk" ] || fail "missing non-empty update.image.gz in source"

manifest="$output_dir/boot-assets.json"
{
  echo "{"
  echo "  \"schema_version\": 1,"
  echo "  \"board\": \"s19-xil\","
  echo "  \"source\": \"$source_path\","
  echo "  \"bootloader\": \"$(basename "$bootloader")\","
  echo "  \"kernel\": \"$(basename "$kernel")\","
  echo "  \"ramdisk\": \"$(basename "$ramdisk")\","
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
