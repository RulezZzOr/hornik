#!/usr/bin/env sh
set -eu

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
