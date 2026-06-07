#!/usr/bin/env sh
set -eu

board="${1:?board is required}"
model="${2:?model is required}"
version="${3:?version is required}"
media="${4:-all}"

tmp_a="$(mktemp -d)"
tmp_b="$(mktemp -d)"
trap 'rm -rf "$tmp_a" "$tmp_b"' EXIT

./scripts/build-image.sh "$board" "$model" "$version" "$tmp_a" "$media" >/dev/null
./scripts/build-image.sh "$board" "$model" "$version" "$tmp_b" "$media" >/dev/null

diff -ru "$tmp_a" "$tmp_b"
cargo run -q -p openmineros-commander -- verify-manifest \
  --manifest "$tmp_a/openmineros-${board}-${model}-${version}-manifest.json" >/dev/null

runtime_env="$tmp_a/openmineros-${board}-${model}-${version}-rootfs-base/etc/openmineros/runtime.env"
launch_script="$tmp_a/openmineros-${board}-${model}-${version}-rootfs-base/usr/bin/openmineros-launch"
config_file="$tmp_a/openmineros-${board}-${model}-${version}-rootfs-base/etc/openmineros/config.toml"

[ -f "$runtime_env" ] || {
  echo "missing rendered runtime env: $runtime_env" >&2
  exit 1
}
[ -f "$launch_script" ] || {
  echo "missing launch script: $launch_script" >&2
  exit 1
}
[ -f "$config_file" ] || {
  echo "missing control-plane config: $config_file" >&2
  exit 1
}

grep -Fqx "BOARD=$board" "$runtime_env"
grep -Fqx "MODEL=$model" "$runtime_env"
grep -Fq -- '--config /etc/openmineros/config.toml' "$launch_script"

if grep -Fq '${BOARD}' "$runtime_env" || grep -Fq '${MODEL}' "$runtime_env"; then
  echo "runtime.env was not rendered for $board/$model" >&2
  exit 1
fi

echo "reproducible install bundle verified for $board/$model $version media=$media"
