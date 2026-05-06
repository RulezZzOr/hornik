#!/usr/bin/env sh
set -eu

board="${1:?board is required}"
model="${2:?model is required}"
version="${3:?version is required}"

tmp_a="$(mktemp -d)"
tmp_b="$(mktemp -d)"
trap 'rm -rf "$tmp_a" "$tmp_b"' EXIT

./scripts/build-image.sh "$board" "$model" "$version" "$tmp_a" >/dev/null
./scripts/build-image.sh "$board" "$model" "$version" "$tmp_b" >/dev/null

diff -ru "$tmp_a" "$tmp_b"
echo "reproducible placeholder artefacts verified for $board/$model $version"

