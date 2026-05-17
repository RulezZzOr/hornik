#!/usr/bin/env sh
set -eu

board="${1:-s19-xil}"
model="${2:-s19j-pro}"
version="${3:-0.1.0}"
dist_dir="${4:-dist}"
script_dir="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
repo_dir="$(CDPATH= cd -- "$script_dir/.." && pwd)"
name="openmineros-${board}-${model}-${version}"
manifest="$dist_dir/${name}-manifest.json"
artifact="$dist_dir/${name}-sd-card.img.xz"

fail() {
  echo "verify-sd-test-bundle: $*" >&2
  exit 1
}

require_file() {
  [ -f "$1" ] || fail "missing file: $1"
}

archive_has() {
  path="$1"
  if ! xz -dc "$artifact" | tar -tf - | grep -qx "./$path"; then
    fail "sd artifact is missing ./$path"
  fi
}

archive_extract() {
  path="$1"
  xz -dc "$artifact" | tar -xOf - "./$path"
}

require_flag() {
  file="$1"
  pattern="$2"
  if ! grep -Eq "$pattern" "$file"; then
    fail "required safe flag not found in $file: $pattern"
  fi
}

case "$board" in
  s19-xil) ;;
  *) fail "safe SD first-boot package currently supports s19-xil only, got $board" ;;
esac

require_file "$manifest"
require_file "$artifact"

if grep -Eq '"flashable"[[:space:]]*:[[:space:]]*true' "$manifest"; then
  fail "manifest must not be flashable for first SD hardware test"
fi

cargo run -q -p openmineros-commander -- verify-manifest \
  --manifest "$manifest"
cargo run -q -p openmineros-commander -- check-manifest-budget \
  --manifest "$manifest"
cargo run -q -p openmineros-commander -- install-plan \
  --manifest "$manifest" \
  --artifact-dir "$dist_dir" >/dev/null

archive_has "etc/openmineros/runtime.env"
archive_has "etc/openmineros/release.json"
archive_has "etc/openmineros/ssh.json"
archive_has "etc/openmineros/install-media.json"
archive_has "etc/default/dropbear"
archive_has "etc/network/interfaces"
archive_has "root/.ssh/authorized_keys"
archive_has "usr/bin/openmineros-control-plane"
archive_has "usr/bin/openmineros-launch"
archive_has "usr/bin/openmineros-first-boot-report"
archive_has "usr/bin/openmineros-safe-self-test"
archive_has "usr/bin/openmineros-ssh-check"
archive_has "usr/bin/openmineros-nand-update"

tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/openmineros-sd-verify.XXXXXX")"
trap 'rm -rf "$tmp_dir"' EXIT INT TERM

archive_extract "etc/openmineros/runtime.env" > "$tmp_dir/runtime.env"
archive_extract "etc/openmineros/release.json" > "$tmp_dir/release.json"
archive_extract "etc/openmineros/ssh.json" > "$tmp_dir/ssh.json"
archive_extract "etc/openmineros/install-media.json" > "$tmp_dir/install-media.json"
archive_extract "etc/default/dropbear" > "$tmp_dir/dropbear"
archive_extract "etc/network/interfaces" > "$tmp_dir/interfaces"

require_flag "$tmp_dir/runtime.env" '^OPENMINEROS_BACKEND=hardware-probe$'
require_flag "$tmp_dir/runtime.env" '^OPENMINEROS_FIRST_BOOT_SAFE=1$'
require_flag "$tmp_dir/runtime.env" '^OPENMINEROS_DISABLE_NAND_WRITES=1$'
require_flag "$tmp_dir/runtime.env" '^OPENMINEROS_DISABLE_ASIC_WRITES=1$'
require_flag "$tmp_dir/runtime.env" '^OPENMINEROS_ALLOW_UNSAFE_HARDWARE=0$'
require_flag "$tmp_dir/release.json" '"first_boot_safe"[[:space:]]*:[[:space:]]*true'
require_flag "$tmp_dir/release.json" '"nand_writes_allowed"[[:space:]]*:[[:space:]]*false'
require_flag "$tmp_dir/release.json" '"asic_writes_allowed"[[:space:]]*:[[:space:]]*false'
require_flag "$tmp_dir/release.json" '"flashable"[[:space:]]*:[[:space:]]*false'
require_flag "$tmp_dir/ssh.json" '"daemon"[[:space:]]*:[[:space:]]*"dropbear"'
require_flag "$tmp_dir/ssh.json" '"port"[[:space:]]*:[[:space:]]*22'
require_flag "$tmp_dir/ssh.json" '"password_login"[[:space:]]*:[[:space:]]*false'
require_flag "$tmp_dir/ssh.json" '"authorized_keys_present"[[:space:]]*:[[:space:]]*true'
require_flag "$tmp_dir/dropbear" 'DROPBEAR_ARGS=.*-s'
require_flag "$tmp_dir/dropbear" 'DROPBEAR_ARGS=.*-p 22'
require_flag "$tmp_dir/interfaces" '^iface eth0 inet dhcp$'
require_flag "$tmp_dir/install-media.json" '"media"[[:space:]]*:[[:space:]]*"sd"'
require_flag "$tmp_dir/install-media.json" '"target"[[:space:]]*:[[:space:]]*"removable_sd"'

echo "safe SD test bundle verified: $repo_dir/$artifact"
