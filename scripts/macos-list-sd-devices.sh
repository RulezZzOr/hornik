#!/usr/bin/env sh
set -eu

if [ "$(uname -s)" != "Darwin" ]; then
  echo "macos-list-sd-devices: this helper is for macOS only" >&2
  exit 2
fi

diskutil list external physical
echo
echo "Inspect a candidate disk with:"
echo "  diskutil info /dev/diskN"
echo
echo "Use only the whole external SD disk, for example /dev/disk4, never a partition like /dev/disk4s1."
