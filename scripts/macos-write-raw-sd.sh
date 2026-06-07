#!/usr/bin/env sh
set -eu
set -o pipefail

image="${1:-}"
disk="${2:-}"

fail() {
  echo "macos-write-raw-sd: $*" >&2
  exit 1
}

[ "$(uname -s)" = "Darwin" ] || fail "this helper is for macOS only"
[ -n "$image" ] || fail "usage: $0 dist/openmineros-...-sd-raw-candidate.img.xz /dev/diskN"
[ -n "$disk" ] || fail "usage: $0 dist/openmineros-...-sd-raw-candidate.img.xz /dev/diskN"
[ -f "$image" ] || fail "image not found: $image"

case "$image" in
  *-sd-raw-candidate.img.xz) ;;
  *) fail "refusing non raw candidate image: $image" ;;
esac

case "$disk" in
  /dev/disk[0-9]*) ;;
  *) fail "disk must look like /dev/diskN" ;;
esac

case "$disk" in
  *s[0-9]*) fail "use the whole disk, not a partition: $disk" ;;
esac

metadata="${image%.img.xz}.json"
[ -f "$metadata" ] || fail "metadata not found for raw image: $metadata"

expected_sha256="$(
  python3 - "$metadata" <<'PY'
import json
import sys
from pathlib import Path

metadata = Path(sys.argv[1])
with metadata.open("r", encoding="utf-8") as handle:
    data = json.load(handle)

try:
    print(data["compressed_sha256"])
except KeyError as exc:
    raise SystemExit(f"missing {exc.args[0]} in {metadata}") from exc
PY
)"
actual_sha256="$(shasum -a 256 "$image" | awk '{print $1}')"
[ "$actual_sha256" = "$expected_sha256" ] || fail "image hash mismatch for $image"

info="$(diskutil info "$disk")"
printf '%s\n' "$info"

if ! printf '%s\n' "$info" | grep -Eq 'External:[[:space:]]+Yes|Removable Media:[[:space:]]+Removable'; then
  fail "disk does not look external/removable; refusing"
fi

if [ "${OPENMINEROS_ALLOW_RAW_SD_WRITE:-0}" != "1" ]; then
  echo
  echo "Refusing to write until explicitly armed."
  echo "This will overwrite the whole disk: $disk"
  echo
  echo "Run:"
  echo "  OPENMINEROS_ALLOW_RAW_SD_WRITE=1 $0 '$image' '$disk'"
  exit 78
fi

raw_disk="/dev/r$(basename "$disk")"
diskutil unmountDisk "$disk"
xz -dc "$image" | sudo dd of="$raw_disk" bs=4m conv=sync
sync
diskutil eject "$disk"
echo "wrote and ejected $disk"
