#!/usr/bin/env sh
set -eu

artifact="${1:-}"
mount_dir="${2:-}"

fail() {
  echo "extract-sd-test-rootfs: $*" >&2
  exit 1
}

[ -n "$artifact" ] || fail "usage: $0 dist/openmineros-s19-xil-s19j-pro-0.1.0-sd-card.img.xz /mounted/sd/rootfs"
[ -n "$mount_dir" ] || fail "usage: $0 dist/openmineros-s19-xil-s19j-pro-0.1.0-sd-card.img.xz /mounted/sd/rootfs"
[ -f "$artifact" ] || fail "missing artifact: $artifact"
[ -d "$mount_dir" ] || fail "mount directory does not exist: $mount_dir"

if [ "${OPENMINEROS_ALLOW_SD_ROOTFS_EXTRACT:-0}" != "1" ]; then
  echo "This artifact is a compressed rootfs tar model, not a raw dd image."
  echo "Refusing to extract without OPENMINEROS_ALLOW_SD_ROOTFS_EXTRACT=1."
  echo
  echo "Example:"
  echo "  OPENMINEROS_ALLOW_SD_ROOTFS_EXTRACT=1 $0 '$artifact' '$mount_dir'"
  exit 78
fi

entries="$(find "$mount_dir" -mindepth 1 -maxdepth 1 2>/dev/null | wc -l | tr -d ' ')"
if [ "$entries" != "0" ] && [ "${OPENMINEROS_ALLOW_NONEMPTY_SD:-0}" != "1" ]; then
  fail "$mount_dir is not empty; set OPENMINEROS_ALLOW_NONEMPTY_SD=1 only if this is intentional"
fi

archive_list="$(mktemp "${TMPDIR:-/tmp}/openmineros-sd-list.XXXXXX")"
trap 'rm -f "$archive_list"' EXIT INT TERM
xz -dc "$artifact" | tar -tf - > "$archive_list"

for required in \
  ./etc/openmineros/runtime.env \
  ./etc/openmineros/release.json \
  ./etc/openmineros/install-media.json \
  ./usr/bin/openmineros-control-plane \
  ./usr/bin/openmineros-launch \
  ./usr/bin/openmineros-first-boot-report \
  ./usr/bin/openmineros-safe-self-test \
  ./usr/bin/openmineros-nand-update
do
  grep -qx "$required" "$archive_list" || fail "artifact missing $required"
done

xz -dc "$artifact" | (
  cd "$mount_dir"
  tar -xpf -
)

sync
echo "rootfs extracted to $mount_dir"
