#!/usr/bin/env sh
set -eu

miner_host="${MINER_HOST:-${1:-}}"
miner_user="${MINER_USER:-root}"
miner_port="${MINER_PORT:-22}"
report_root="${REPORT_DIR:-dist/first-boot-reports}"
ssh_batch_mode="${SSH_BATCH_MODE:-no}"
ssh_connect_timeout="${SSH_CONNECT_TIMEOUT:-10}"
ssh_strict_host_key_checking="${SSH_STRICT_HOST_KEY_CHECKING:-accept-new}"
ssh_key="${SSH_KEY:-}"

fail() {
  echo "collect-s19-first-boot: $*" >&2
  exit 1
}

[ -n "$miner_host" ] || fail "set MINER_HOST or pass host as first argument"

safe_host="$(printf '%s' "$miner_host" | tr '/:.' '___')"
stamp="$(date -u '+%Y%m%dT%H%M%SZ')"
out_dir="$report_root/${safe_host}-${stamp}"
mkdir -p "$out_dir"

remote() {
  if [ -n "$ssh_key" ]; then
    ssh \
      -i "$ssh_key" \
      -p "$miner_port" \
      -o "BatchMode=$ssh_batch_mode" \
      -o "ConnectTimeout=$ssh_connect_timeout" \
      -o "StrictHostKeyChecking=$ssh_strict_host_key_checking" \
      "$miner_user@$miner_host" \
      "$@"
  else
    ssh \
      -p "$miner_port" \
      -o "BatchMode=$ssh_batch_mode" \
      -o "ConnectTimeout=$ssh_connect_timeout" \
      -o "StrictHostKeyChecking=$ssh_strict_host_key_checking" \
      "$miner_user@$miner_host" \
      "$@"
  fi
}

collect_command() {
  name="$1"
  command="$2"
  if remote "$command" > "$out_dir/$name" 2> "$out_dir/$name.stderr"; then
    :
  else
    echo "warning: failed to collect $name" >&2
  fi
}

collect_api() {
  name="$1"
  path="$2"
  collect_command "$name" "if command -v curl >/dev/null 2>&1; then if ! curl -fsS 'http://127.0.0.1:8080$path'; then echo 'failed to fetch $path from control plane' >&2; exit 1; fi; elif command -v wget >/dev/null 2>&1; then if ! wget -qO- 'http://127.0.0.1:8080$path'; then echo 'failed to fetch $path from control plane via wget' >&2; exit 1; fi; else echo 'curl/wget missing' >&2; exit 127; fi"
}

require_json_bool() {
  file="$1"
  key="$2"
  value="$3"
  if ! grep -Eq "\"$key\"[[:space:]]*:[[:space:]]*$value" "$file"; then
    echo "FAIL $key expected $value in $file" >> "$out_dir/verdict.txt"
    return 1
  fi
  echo "PASS $key=$value" >> "$out_dir/verdict.txt"
  return 0
}

echo "miner_host=$miner_host" > "$out_dir/metadata.txt"
echo "miner_user=$miner_user" >> "$out_dir/metadata.txt"
echo "miner_port=$miner_port" >> "$out_dir/metadata.txt"
echo "ssh_key=${ssh_key:-not-set}" >> "$out_dir/metadata.txt"
echo "generated_utc=$stamp" >> "$out_dir/metadata.txt"
: > "$out_dir/verdict.txt"

collect_api "api-system-info.json" "/api/v1/system/info"
collect_api "api-health.json" "/api/v1/system/health"
collect_api "api-runtime-control.json" "/api/v1/runtime/control"
collect_api "api-hardware-identity.json" "/api/v1/hardware/identity"
collect_api "api-hardware-readiness.json" "/api/v1/hardware/readiness"
collect_api "api-firmware-deployment.json" "/api/v1/firmware/deployment"
collect_api "api-firmware-anti-brick.json" "/api/v1/firmware/anti-brick"
collect_api "api-stratum-status.json" "/api/v1/stratum/status"
collect_api "metrics.prom" "/metrics"

collect_command "runtime.env" "cat /etc/openmineros/runtime.env 2>/dev/null || true"
collect_command "release.json" "cat /etc/openmineros/release.json 2>/dev/null || true"
collect_command "install-media.json" "cat /etc/openmineros/install-media.json 2>/dev/null || true"
collect_command "first-boot-report.path" "/usr/bin/openmineros-first-boot-report 2>&1 || true"
collect_command "first-boot-report.txt" "cat /var/log/openmineros/first-boot-report.txt 2>/dev/null || true"
collect_command "safe-self-test.txt" "/usr/bin/openmineros-safe-self-test 2>&1 || true"
collect_command "ssh-check.txt" "/usr/bin/openmineros-ssh-check 2>&1 || true"
collect_command "nand-guard.txt" "rc=0; /usr/bin/openmineros-nand-update >/tmp/openmineros-nand-guard.out 2>&1 || rc=\$?; cat /tmp/openmineros-nand-guard.out; echo exit_code=\$rc; [ \"\$rc\" = 78 ]"
collect_command "processes.txt" "ps w 2>/dev/null || ps 2>/dev/null || true"
collect_command "mounts.txt" "cat /proc/mounts 2>/dev/null || true"
collect_command "mtd.txt" "cat /proc/mtd 2>/dev/null || true"
collect_command "partitions.txt" "cat /proc/partitions 2>/dev/null || true"
collect_command "serial-devices.txt" "ls -l /dev/ttyPS* /dev/ttyS* /dev/ttyO* 2>/dev/null || true"
collect_command "dmesg.txt" "dmesg 2>/dev/null | tail -n 300 || true"
collect_command "openmineros-log.txt" "cat /var/log/openmineros/control-plane.log 2>/dev/null || true"

failed=0
require_json_bool "$out_dir/api-firmware-anti-brick.json" safe_to_first_boot true || failed=1
require_json_bool "$out_dir/api-firmware-anti-brick.json" nand_writes_allowed false || failed=1
require_json_bool "$out_dir/api-firmware-anti-brick.json" asic_writes_allowed false || failed=1
require_json_bool "$out_dir/api-firmware-anti-brick.json" flashing_allowed false || failed=1

for flag in \
  '^OPENMINEROS_BACKEND=hardware-probe$' \
  '^OPENMINEROS_FIRST_BOOT_SAFE=1$' \
  '^OPENMINEROS_DISABLE_NAND_WRITES=1$' \
  '^OPENMINEROS_DISABLE_ASIC_WRITES=1$' \
  '^OPENMINEROS_ALLOW_UNSAFE_HARDWARE=0$'
do
  if grep -Eq "$flag" "$out_dir/runtime.env"; then
    echo "PASS runtime.env $flag" >> "$out_dir/verdict.txt"
  else
    echo "FAIL runtime.env missing $flag" >> "$out_dir/verdict.txt"
    failed=1
  fi
done

if grep -Eq 'exit_code=78' "$out_dir/nand-guard.txt"; then
  echo "PASS nand guard refused writes with exit 78" >> "$out_dir/verdict.txt"
else
  echo "FAIL nand guard did not return exit 78" >> "$out_dir/verdict.txt"
  failed=1
fi

if grep -Eq 'PASS tcp port 22 listening|SKIP netstat unavailable' "$out_dir/ssh-check.txt" \
  && grep -Eq 'PASS dropbear process running' "$out_dir/ssh-check.txt" \
  && grep -Eq 'PASS root authorized_keys present' "$out_dir/ssh-check.txt"; then
  echo "PASS ssh dropbear key-based access configured" >> "$out_dir/verdict.txt"
else
  echo "FAIL ssh dropbear key-based access is not fully confirmed" >> "$out_dir/verdict.txt"
  failed=1
fi

tar -czf "$out_dir.tar.gz" -C "$report_root" "$(basename "$out_dir")"

echo "$out_dir"
echo "$out_dir.tar.gz"

if [ "$failed" -ne 0 ]; then
  echo "first boot safety checks failed; inspect $out_dir/verdict.txt" >&2
  exit 1
fi

echo "first boot safety checks passed"
