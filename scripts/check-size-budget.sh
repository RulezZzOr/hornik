#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET_DIR="${TARGET_DIR:-$ROOT_DIR/target/release}"

CONTROL_PLANE="${TARGET_DIR}/openmineros-control-plane"
COMMANDER="${TARGET_DIR}/openmineros-commander"
WEB_DIR="${ROOT_DIR}/web"

MIB=$((1024 * 1024))
KIB=1024

CONTROL_PLANE_MAX_BYTES="${CONTROL_PLANE_MAX_BYTES:-$((12 * MIB))}"
COMMANDER_MAX_BYTES="${COMMANDER_MAX_BYTES:-$((8 * MIB))}"
WEB_MAX_BYTES="${WEB_MAX_BYTES:-$((512 * KIB))}"

failures=0

check_file() {
  local label="$1"
  local path="$2"
  local max_bytes="$3"

  if [[ ! -f "$path" ]]; then
    echo "size-budget: missing ${label}: ${path}" >&2
    failures=$((failures + 1))
    return
  fi

  local bytes
  bytes="$(wc -c < "$path" | tr -d '[:space:]')"
  print_result "$label" "$bytes" "$max_bytes"
}

check_dir() {
  local label="$1"
  local path="$2"
  local max_bytes="$3"

  if [[ ! -d "$path" ]]; then
    echo "size-budget: missing ${label}: ${path}" >&2
    failures=$((failures + 1))
    return
  fi

  local bytes
  bytes="$(find "$path" -type f -print0 | xargs -0 wc -c | awk 'END {print $1 + 0}')"
  print_result "$label" "$bytes" "$max_bytes"
}

print_result() {
  local label="$1"
  local bytes="$2"
  local max_bytes="$3"

  printf 'size-budget: %-28s %8s / %8s bytes\n' "$label" "$bytes" "$max_bytes"

  if (( bytes > max_bytes )); then
    echo "size-budget: ${label} exceeds budget" >&2
    failures=$((failures + 1))
  fi
}

check_file "control-plane release bin" "$CONTROL_PLANE" "$CONTROL_PLANE_MAX_BYTES"
check_file "commander release bin" "$COMMANDER" "$COMMANDER_MAX_BYTES"
check_dir "static web assets" "$WEB_DIR" "$WEB_MAX_BYTES"

if (( failures > 0 )); then
  exit 1
fi
