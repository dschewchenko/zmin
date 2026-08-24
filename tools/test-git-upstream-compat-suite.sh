#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
archive="${ZMIN_TEST_UPSTREAM_ARCHIVE:?set ZMIN_TEST_UPSTREAM_ARCHIVE to the pinned v2.55.0 archive}"
archive_sha="${ZMIN_TEST_UPSTREAM_ARCHIVE_SHA256:?set ZMIN_TEST_UPSTREAM_ARCHIVE_SHA256 to the pinned archive SHA-256}"
zmin_bin="${ZMIN_TEST_BIN:?set ZMIN_TEST_BIN to an existing absolute zmin binary}"
perl_bin="${ZMIN_TEST_PERL:?set ZMIN_TEST_PERL to an existing absolute Perl interpreter}"
contract_git_bin="${ZMIN_TEST_CONTRACT_GIT:?set ZMIN_TEST_CONTRACT_GIT to the pinned absolute Git binary}"
contract_python_bin="${ZMIN_TEST_CONTRACT_PYTHON:?set ZMIN_TEST_CONTRACT_PYTHON to the trusted absolute Python binary}"
make_bin="${ZMIN_TEST_CONTRACT_MAKE:?set ZMIN_TEST_CONTRACT_MAKE to the trusted absolute Make binary}"
phase_timeout_seconds="${ZMIN_UPSTREAM_PHASE_TIMEOUT_SECONDS:-30}"
if [[ ! "$phase_timeout_seconds" =~ ^[1-9][0-9]*$ ]] ||
  (( phase_timeout_seconds > 45 )); then
  echo "ZMIN_UPSTREAM_PHASE_TIMEOUT_SECONDS must be an integer from 1 through 45" >&2
  exit 2
fi

run_bounded_phase() {
  local phase="$1"
  shift
  "$contract_python_bin" - "$phase" "$phase_timeout_seconds" "$@" <<'PY'
import os
import signal
import subprocess
import sys


phase = sys.argv[1]
timeout_seconds = int(sys.argv[2])
argv = sys.argv[3:]
if not argv:
    raise SystemExit("%s: missing child command" % phase)
child = None


def terminate_process_group():
    if child is None:
        return
    if child.poll() is None:
        try:
            os.killpg(child.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
    try:
        child.wait(timeout=2)
    except subprocess.TimeoutExpired:
        pass
    try:
        os.killpg(child.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    try:
        child.wait()
    except ChildProcessError:
        pass


def terminate(signum, _frame):
    terminate_process_group()
    raise SystemExit(128 + signum)


signal.signal(signal.SIGTERM, terminate)
signal.signal(signal.SIGINT, terminate)
child = subprocess.Popen(argv, start_new_session=True)
phase_pid_file = os.environ.get("ZMIN_UPSTREAM_PHASE_PID_FILE")
if phase_pid_file is None:
    for argument in argv:
        if argument.startswith("ZMIN_UPSTREAM_PHASE_PID_FILE="):
            phase_pid_file = argument.split("=", 1)[1]
            break
if phase_pid_file:
    with open(phase_pid_file, "x", encoding="ascii") as stream:
        stream.write(str(child.pid))
try:
    status = child.wait(timeout=timeout_seconds)
except subprocess.TimeoutExpired:
    print(
        "%s: timed out after %ss; terminating child process group"
        % (phase, timeout_seconds),
        file=sys.stderr,
    )
    terminate_process_group()
    raise SystemExit(124)
normalized_status = 128 + (-status) if status < 0 else status
if normalized_status != 0:
    print(
        "%s: child exited with status %d" % (phase, normalized_status),
        file=sys.stderr,
    )
raise SystemExit(normalized_status)
PY
}

if [[ "${ZMIN_TEST_WATCHDOG_ONLY:-0}" == "1" ]]; then
  watchdog_root="$(mktemp -d "${TMPDIR:-/tmp}/zmin-watchdog-test.XXXXXX")"
  watchdog_pid_file="$watchdog_root/grandchild.pid"
  watchdog_cleanup() {
    if [[ -f "$watchdog_pid_file" ]]; then
      kill "$(<"$watchdog_pid_file")" 2>/dev/null || true
    fi
    if [[ -d "$watchdog_root" && ! -L "$watchdog_root" ]]; then
      find "$watchdog_root" -depth -delete
    fi
  }
  trap watchdog_cleanup EXIT
  saved_phase_timeout_seconds="$phase_timeout_seconds"
  phase_timeout_seconds=1
  set +e
  run_bounded_phase "watchdog leader-exit" "$contract_python_bin" -c \
    'import signal,subprocess,sys,time; child=subprocess.Popen([sys.executable,"-c","import signal,time; signal.signal(signal.SIGTERM,signal.SIG_IGN); time.sleep(30)"]); open(sys.argv[1],"w").write(str(child.pid)); time.sleep(30)' \
    "$watchdog_pid_file" >/dev/null 2>"$watchdog_root/leader.out"
  watchdog_status="$?"
  set -e
  phase_timeout_seconds="$saved_phase_timeout_seconds"
  [[ "$watchdog_status" == "124" ]]
  watchdog_grandchild_pid="$(<"$watchdog_pid_file")"
  for _ in 1 2 3 4 5; do
    if ! kill -0 "$watchdog_grandchild_pid" 2>/dev/null; then
      break
    fi
    sleep 0.05
  done
  if kill -0 "$watchdog_grandchild_pid" 2>/dev/null; then
    echo 'watchdog left TERM-ignoring child alive' >&2
    exit 1
  fi

  set +e
  signal_output="$(run_bounded_phase "signal normalization" "$contract_python_bin" -c 'import os,signal; os.kill(os.getpid(),signal.SIGTERM)' 2>&1)"
  signal_status="$?"
  set -e
  [[ "$signal_status" == "143" ]]
  grep -Fqx 'signal normalization: child exited with status 143' <<<"$signal_output"
  printf 'watchdog-tests=pass\n'
  exit 0
fi

[[ "$archive" == /* && -f "$archive" ]]
[[ "$zmin_bin" == /* && -x "$zmin_bin" ]]
[[ "$perl_bin" == /* && -x "$perl_bin" ]]
for contract_tool in "$contract_git_bin" "$contract_python_bin" "$make_bin"; do
  [[ "$contract_tool" == /* && -x "$contract_tool" && ! -L "$contract_tool" ]]
  contract_canonical="$(cd -P "$(dirname "$contract_tool")" && pwd -P)/$(basename "$contract_tool")"
  [[ "$contract_canonical" == "$contract_tool" ]]
done
[[ "$("$contract_git_bin" --version)" == "git version 2.55.0" ]]
"$contract_python_bin" -c 'import sys; assert sys.version_info[0] == 3'
[[ -x "$make_bin" && ! -L "$make_bin" ]]
[[ "$(shasum -a 256 "$archive" | awk '{ print $1 }')" == "$archive_sha" ]]

tmp_parent="${TMPDIR:-/tmp}"
tmp_parent="${tmp_parent%/}"
tmp_root="$(mktemp -d "$tmp_parent/zmin-upstream-prereq-test.XXXXXX")"
tmp_root="$(cd -P "$tmp_root" && pwd -P)"
identity_source=""
identity_backup=""
probe_pids=""
cleanup() {
  for probe_pid in $probe_pids; do
    kill "$probe_pid" 2>/dev/null || true
    wait "$probe_pid" 2>/dev/null || true
  done
  if [[ -n "${identity_backup:-}" && -f "$identity_backup" &&
    -n "${identity_source:-}" ]]; then
    cp "$identity_backup" "$identity_source" 2>/dev/null || true
  fi
  if [[ -d "$tmp_root" && ! -L "$tmp_root" ]]; then
    chmod -R u+w "$tmp_root" 2>/dev/null || true
    find "$tmp_root" -depth -delete
  fi
}
trap cleanup EXIT

cache_root="$tmp_root/cache"
out_dir="$tmp_root/out"
mkdir -p "$cache_root" "$out_dir"
cache_root="$(cd -P "$cache_root" && pwd -P)"
cp "$archive" "$cache_root/v2.55.0.tar.gz"

run_prepare_with_bin() {
  local selected_bin="$1"
  local selected_perl="$2"
  local selected_cache="$3"
  local selected_tag="${4:-v2.55.0}"
  run_bounded_phase "upstream preparation" env \
    ZMIN_TEST_PERL="$selected_perl" \
    ZMIN_BIN="$selected_bin" \
    ZMIN_UPSTREAM_GIT_TAG="$selected_tag" \
    ZMIN_UPSTREAM_GIT_CACHE="$selected_cache" \
    ZMIN_UPSTREAM_CONTRACT_GIT="$contract_git_bin" \
    ZMIN_UPSTREAM_CONTRACT_PYTHON="$contract_python_bin" \
    ZMIN_UPSTREAM_CONTRACT_MAKE="$make_bin" \
    ZMIN_UPSTREAM_OUT_DIR="$out_dir" \
    ZMIN_UPSTREAM_JOBS=1 \
    ZMIN_UPSTREAM_PREPARE_ONLY=1 \
    "$repo_root/tools/git-upstream-compat-suite.sh" all-nondeprecated
}

run_prepare() {
  local selected_perl="$1"
  local selected_cache="$2"
  local selected_tag="${3:-v2.55.0}"
  run_prepare_with_bin "$zmin_bin" "$selected_perl" "$selected_cache" "$selected_tag"
}

wait_for_probe_path() {
  local path="$1"
  local attempt=0
  while [[ ! -e "$path" ]]; do
    attempt=$((attempt + 1))
    [[ "$attempt" -lt 400 ]] || {
      echo "lock probe timed out waiting for $path" >&2
      return 1
    }
    sleep 0.01
  done
}

lock_probe_root="$tmp_root/lock-probe"
lock_probe_events="$lock_probe_root/events"
lock_probe_directory="$lock_probe_root/locks"
lock_probe_path="$lock_probe_directory/rendezvous"
mkdir -p "$lock_probe_events" "$lock_probe_directory"
: >"$lock_probe_path"
chmod 500 "$lock_probe_directory"
lock_probe_identity="$("$contract_python_bin" - "$lock_probe_directory" "$lock_probe_path" <<'PY'
import os
import sys

directory = os.stat(sys.argv[1], follow_symlinks=False)
lock = os.stat(sys.argv[2], follow_symlinks=False)
print("%d:%d:%d/%d:%d" % (directory.st_dev, directory.st_ino, directory.st_nlink, lock.st_dev, lock.st_ino))
PY
)"
probe_pid_one=""
probe_pid_two=""

run_lock_probe_contender() {
  local role="$1"
  local child_pid_file="$lock_probe_events/child.$role.pid"
  run_bounded_phase "lock probe $role" env \
    ZMIN_UPSTREAM_LOCK_PROBE=1 \
    ZMIN_UPSTREAM_LOCK_PROBE_LOCK="$lock_probe_path" \
    ZMIN_UPSTREAM_LOCK_PROBE_ROLE="$role" \
    ZMIN_UPSTREAM_LOCK_PROBE_EVENTS="$lock_probe_events" \
    ZMIN_UPSTREAM_LOCK_PROBE_START="$lock_probe_events/start" \
    ZMIN_UPSTREAM_LOCK_PROBE_RELEASE="$lock_probe_events/release.$role" \
    ZMIN_UPSTREAM_LOCK_PROBE_IDENTITY="$lock_probe_identity" \
    ZMIN_UPSTREAM_CONTRACT_PYTHON="$contract_python_bin" \
    ZMIN_UPSTREAM_PHASE_PID_FILE="$child_pid_file" \
    ZMIN_UPSTREAM_GIT_CACHE="$lock_probe_root/cache-$role" \
    ZMIN_UPSTREAM_OUT_DIR="$lock_probe_events/out-$role" \
    "$repo_root/tools/git-upstream-compat-suite.sh" all-nondeprecated \
    >"$lock_probe_events/$role.log" 2>&1 &
  probe_pids="$probe_pids $!"
  if [[ "$role" == "one" ]]; then
    probe_pid_one="$!"
  else
    probe_pid_two="$!"
  fi
}

terminate_phase_child() {
  local child_pid="$1"
  "$contract_python_bin" - "$child_pid" <<'PY'
import os
import signal
import sys

os.killpg(int(sys.argv[1]), signal.SIGTERM)
PY
}

run_lock_probe_contender one
run_lock_probe_contender two
wait_for_probe_path "$lock_probe_events/ready.one"
wait_for_probe_path "$lock_probe_events/ready.two"
: >"$lock_probe_events/start"

first_role=""
attempt=0
while [[ "$attempt" -lt 400 ]]; do
  attempt=$((attempt + 1))
  if [[ -e "$lock_probe_events/entered.one" && -e "$lock_probe_events/entered.two" ]]; then
    echo "both lock contenders entered their critical sections" >&2
    exit 1
  fi
  if [[ -e "$lock_probe_events/entered.one" ]]; then
    first_role=one
    break
  fi
  if [[ -e "$lock_probe_events/entered.two" ]]; then
    first_role=two
    break
  fi
  sleep 0.01
done
[[ -n "$first_role" ]] || {
  echo "lock probe did not enter a critical section" >&2
  exit 1
}
[[ -f "$lock_probe_path" && ! -L "$lock_probe_path" ]] || {
  echo "stable lock file was replaced during contention" >&2
  exit 1
}
printf 'replacement\n' >"$lock_probe_root/replacement"
if mv "$lock_probe_root/replacement" "$lock_probe_path" 2>/dev/null; then
  echo "writable lock rendezvous replacement unexpectedly succeeded" >&2
  exit 1
fi
if ln -s "$lock_probe_root/external" "$lock_probe_path" 2>/dev/null; then
  echo "lock rendezvous symlink replacement unexpectedly succeeded" >&2
  exit 1
fi
second_role=two
[[ "$first_role" == two ]] && second_role=one
if [[ "$first_role" == "one" ]]; then
  terminate_phase_child "$(<"$lock_probe_events/child.one.pid")"
  wait "$probe_pid_one" 2>/dev/null || true
else
  terminate_phase_child "$(<"$lock_probe_events/child.two.pid")"
  wait "$probe_pid_two" 2>/dev/null || true
fi
wait_for_probe_path "$lock_probe_events/entered.$second_role"
: >"$lock_probe_events/release.$second_role"
wait_for_probe_path "$lock_probe_events/done.$second_role"
if [[ "$second_role" == "one" ]]; then wait "$probe_pid_one" 2>/dev/null || true; else wait "$probe_pid_two" 2>/dev/null || true; fi
probe_pids=""
[[ -f "$lock_probe_path" && ! -L "$lock_probe_path" ]] || {
  echo "lock probe removed its stable lock file" >&2
  exit 1
}
if find "$lock_probe_root" -maxdepth 1 -name 'lock.*' -o -name 'takeover' | grep -q .; then
  echo "lock probe left stale takeover artifacts" >&2
  exit 1
fi
live_lock_root="$tmp_root/live-lock"
live_lock_events="$live_lock_root/events"
live_lock_directory="$live_lock_root/locks"
live_lock_path="$live_lock_directory/rendezvous"
mkdir -p "$live_lock_events" "$live_lock_directory"
: >"$live_lock_path"
chmod 500 "$live_lock_directory"
live_lock_identity="$("$contract_python_bin" - "$live_lock_directory" "$live_lock_path" <<'PY'
import os
import sys

directory = os.stat(sys.argv[1], follow_symlinks=False)
lock = os.stat(sys.argv[2], follow_symlinks=False)
print("%d:%d:%d/%d:%d" % (directory.st_dev, directory.st_ino, directory.st_nlink, lock.st_dev, lock.st_ino))
PY
)"
"$contract_python_bin" - "$live_lock_path" "$live_lock_events/held" <<'PY' &
import fcntl
import signal
import sys

lock = open(sys.argv[1], "r+")
fcntl.flock(lock, fcntl.LOCK_EX)
open(sys.argv[2], "w").close()
signal.pause()
PY
live_lock_holder_pid="$!"
probe_pids="$probe_pids $live_lock_holder_pid"
wait_for_probe_path "$live_lock_events/held"
live_lock_output="$tmp_root/live-lock.out"
: >"$live_lock_events/start"
set +e
run_bounded_phase "live lock probe" env \
  ZMIN_UPSTREAM_LOCK_PROBE=1 \
  ZMIN_UPSTREAM_LOCK_PROBE_LOCK="$live_lock_path" \
  ZMIN_UPSTREAM_LOCK_PROBE_ROLE=live \
  ZMIN_UPSTREAM_LOCK_PROBE_EVENTS="$live_lock_events" \
  ZMIN_UPSTREAM_LOCK_PROBE_START="$live_lock_events/start" \
  ZMIN_UPSTREAM_LOCK_PROBE_RELEASE="$live_lock_events/release.live" \
  ZMIN_UPSTREAM_LOCK_PROBE_IDENTITY="$live_lock_identity" \
  ZMIN_UPSTREAM_LOCK_TIMEOUT_SECONDS=2 \
  ZMIN_UPSTREAM_CONTRACT_PYTHON="$contract_python_bin" \
  ZMIN_UPSTREAM_GIT_CACHE="$live_lock_root/cache" \
  ZMIN_UPSTREAM_OUT_DIR="$live_lock_root/out" \
  "$repo_root/tools/git-upstream-compat-suite.sh" all-nondeprecated \
  >"$live_lock_output" 2>&1
live_lock_status="$?"
set -e
kill "$live_lock_holder_pid" 2>/dev/null || true
wait "$live_lock_holder_pid" 2>/dev/null || true
[[ "$live_lock_status" != "0" ]] || {
  echo "live held lock was accepted" >&2
  cat "$live_lock_output" >&2
  exit 1
}
grep -Fqx 'lock probe: timed out waiting for live lock after 2s' "$live_lock_output"
first_start_cache="$tmp_root/first-start-cache"
mkdir -p "$first_start_cache"
first_start_pid_one=""
first_start_pid_two=""
for role in one two; do
  first_start_out="$tmp_root/first-start-out.$role"
  mkdir -p "$first_start_out"
  run_bounded_phase "first-start fixture $role" env \
    ZMIN_TEST_PERL="$perl_bin" \
    ZMIN_UPSTREAM_CONTRACT_PYTHON="$contract_python_bin" \
    ZMIN_UPSTREAM_CONTRACT_MAKE="$make_bin" \
    ZMIN_UPSTREAM_GIT_CACHE="$first_start_cache" \
    ZMIN_UPSTREAM_OUT_DIR="$first_start_out" \
    ZMIN_UPSTREAM_MANIFEST_FIXTURE=1 \
    "$repo_root/tools/git-upstream-compat-suite.sh" all-nondeprecated \
    >"$first_start_out/run.log" 2>&1 &
  if [[ "$role" == "one" ]]; then
    first_start_pid_one="$!"
  else
    first_start_pid_two="$!"
  fi
done
probe_pids="$first_start_pid_one $first_start_pid_two"
if ! wait "$first_start_pid_one" || ! wait "$first_start_pid_two"; then
  echo "first-start lock rendezvous contenders failed" >&2
  sed -n '1,40p' "$tmp_root"/first-start-out.*/run.log >&2 || true
  exit 1
fi
probe_pids=""
grep -Fqx 'manifest-fixture=pass' "$tmp_root/first-start-out.one/run.log"
grep -Fqx 'manifest-fixture=pass' "$tmp_root/first-start-out.two/run.log"
first_start_identity_one="$(sed -n 's/^lock-rendezvous=//p' "$tmp_root/first-start-out.one/run.log")"
first_start_identity_two="$(sed -n 's/^lock-rendezvous=//p' "$tmp_root/first-start-out.two/run.log")"
[[ -n "$first_start_identity_one" && "$first_start_identity_one" == "$first_start_identity_two" ]]
[[ "$first_start_identity_one" == */*/* ]]
first_start_actual_identity="$("$contract_python_bin" - "$first_start_cache/.zmin-locks" "$first_start_cache/.zmin-locks/cache.lock" "$first_start_cache/.zmin-locks/prepare.lock" <<'PY'
import os
import sys

directory = os.stat(sys.argv[1], follow_symlinks=False)
cache = os.stat(sys.argv[2], follow_symlinks=False)
prepare = os.stat(sys.argv[3], follow_symlinks=False)
print(
    "%d:%d:%d/%d:%d/%d:%d"
    % (
        directory.st_dev,
        directory.st_ino,
        directory.st_nlink,
        cache.st_dev,
        cache.st_ino,
        prepare.st_dev,
        prepare.st_ino,
    )
)
PY
)"
[[ "$first_start_identity_one" == "$first_start_actual_identity" ]]
if find "$first_start_cache" -maxdepth 1 -name '.zmin-locks.tmp.*' -print -quit | grep -q .; then
  echo "first-start lock rendezvous left a private temporary directory" >&2
  exit 1
fi
manifest_fixture_output="$(run_bounded_phase "manifest fixture" env \
  ZMIN_TEST_PERL="$perl_bin" \
  ZMIN_UPSTREAM_CONTRACT_PYTHON="$contract_python_bin" \
  ZMIN_UPSTREAM_CONTRACT_MAKE="$make_bin" \
  ZMIN_UPSTREAM_GIT_CACHE="$tmp_root/manifest-cache" \
  ZMIN_UPSTREAM_OUT_DIR="$tmp_root/manifest-out" \
  ZMIN_UPSTREAM_MANIFEST_FIXTURE=1 \
  "$repo_root/tools/git-upstream-compat-suite.sh" all-nondeprecated
)"
printf '%s\n' "$manifest_fixture_output" | grep -Fqx 'manifest-fixture=pass' || {
  echo "prepared-source manifest fixture failed: $manifest_fixture_output" >&2
  exit 1
}
runner_source="$repo_root/tools/git-upstream-compat-suite.sh"
grep -Fq 'memfd_create("zmin-pinned-make", allow_sealing | cloexec)' "$runner_source"
grep -Fq 'fcntl.fcntl(pin_fd, add_seals, seal_flags)' "$runner_source"
grep -Fq 'fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)' "$runner_source"
grep -Fq 'libc.execveat.restype' "$runner_source"
grep -Fq 'libc.execveat(exec_fd, b"", argv_array, env_array, 0x1000)' "$runner_source"
grep -Fq 'phase_timeout_seconds = int(sys.argv[4])' "$runner_source"
grep -Fq 'deadline = time.monotonic() + phase_timeout_seconds' "$runner_source"
if grep -Fq '1800.0' "$runner_source"; then
  echo "unbounded authoritative Make child timeout remains" >&2
  exit 1
fi
grep -Fq 'prepare_lock="$lock_root/prepare.lock"' "$runner_source"
grep -Fq '"prepare.lock" "$label" "$lock_timeout_seconds"' "$runner_source"
if rg -n 'prepare-\$harness_fingerprint' "$runner_source" >/dev/null; then
  echo "fingerprint-specific lock path remains" >&2
  exit 1
fi
grep -Fq 'require_descriptor_bound_make_platform || return 1' "$runner_source"
grep -Fq '"/proc/self/fd/%d" % pin_fd' "$runner_source"
grep -Fq 'header != b"\x7fELF"' "$runner_source"
if grep -Fq 'header != b"\\x7fELF"' "$runner_source"; then
  echo "ELF magic is still represented as literal backslashes" >&2
  exit 1
fi
if grep -Fq 'open_readonly(pin_path)' "$runner_source"; then
  echo "Linux pinned Make runner reopens pin_path" >&2
  exit 1
fi
grep -Fq 'digest_fd(pin_fd)' "$runner_source"
grep -Fq 'sys.platform == "darwin"' "$runner_source"
grep -Fq 'Darwin authoritative Make execution is unsupported' "$runner_source"
if [[ "$(uname -s)" == "Linux" ]]; then
  grep -Fq 'native ELF executable' "$runner_source"
fi
if rg -n 'takeover|stale-lock|lock\.stale|lock\.release' "$runner_source" >/dev/null; then
  echo "obsolete stale-lock takeover code remains" >&2
  exit 1
fi
if [[ "${ZMIN_TEST_LOCK_PROBE_ONLY:-0}" == "1" ]]; then
  printf 'lock-race-probe=pass\n'
  exit 0
fi

if [[ "$(uname -s 2>/dev/null || printf '%s' unknown)" == "Darwin" ||
  "${RUNNER_OS:-}" == "Windows" || "${OS:-}" == "Windows_NT" ]]; then
  printf 'descriptor-bound-make-platform=fail-closed\n'
  exit 0
fi

if run_prepare perl "$cache_root" >/dev/null 2>&1; then
  echo "relative ZMIN_TEST_PERL was accepted" >&2
  exit 1
fi

linked_cache="$tmp_root/linked-cache"
ln -s "$cache_root" "$linked_cache"
if run_prepare "$perl_bin" "$linked_cache" >/dev/null 2>&1; then
  echo "symlinked upstream cache was accepted" >&2
  exit 1
fi
escaped_archive="$tmp_root/escaped.tar.gz"
if run_prepare "$perl_bin" "$cache_root" "../escaped" >/dev/null 2>&1; then
  echo "unsafe upstream tag was accepted" >&2
  exit 1
fi
[[ ! -e "$escaped_archive" ]] || { echo "unsafe tag touched an archive path" >&2; exit 1; }

prepared_output="$(run_prepare "$perl_bin" "$cache_root")"

source_dir="$(printf '%s\n' "$prepared_output" | sed -n 's/^prepared_source=//p')"
reported_perl="$(printf '%s\n' "$prepared_output" | sed -n 's/^perl=//p')"
perl_module="$(printf '%s\n' "$prepared_output" | sed -n 's/^perl_module=//p')"
test_tool="$(printf '%s\n' "$prepared_output" | sed -n 's/^test_tool=//p')"
test_tool_real="$(printf '%s\n' "$prepared_output" | sed -n 's/^test_tool_real=//p')"
test_tool_exec_sha256="$(printf '%s\n' "$prepared_output" | sed -n 's/^test_tool_exec_sha256=//p')"
archive_commit_binding="$(printf '%s\n' "$prepared_output" | sed -n 's/^archive_commit_binding=//p')"

[[ -n "$source_dir" && -d "$source_dir" ]]
[[ "$reported_perl" == "$perl_bin" ]]
[[ "$perl_module" == "$source_dir/perl/build/lib/Git.pm" && -f "$perl_module" ]]
if [[ "${RUNNER_OS:-}" == "Windows" || "${OS:-}" == "Windows_NT" ]]; then
  expected_test_tool="$source_dir/t/helper/test-tool.exe"
else
  expected_test_tool="$source_dir/t/helper/test-tool"
fi
[[ "$test_tool" == "$expected_test_tool" && -x "$test_tool" ]]
if [[ "${RUNNER_OS:-}" == "Windows" || "${OS:-}" == "Windows_NT" ]]; then
  expected_test_tool_real="$source_dir/t/helper/test-tool.exe"
else
  expected_test_tool_real="$source_dir/t/helper/test-tool-real"
fi
[[ "$test_tool_real" == "$expected_test_tool_real" && -x "$test_tool_real" ]]
[[ "$test_tool_exec_sha256" == "$(shasum -a 256 "$test_tool" | awk '{ print $1 }')" ]]
[[ "$archive_commit_binding" == "archive_sha256+tag+DEF_VER;commit_not_embedded_in_archive" ]]
[[ "$(printf '%s\n' "$prepared_output" | grep -E '^(archive_commit_binding|perl|perl_module|prepared_source|test_tool|test_tool_real)=' | sed 's/=.*//' | sort | tr '\n' ' ')" == \
  "archive_commit_binding perl perl_module prepared_source test_tool test_tool_real " ]]
! grep -qs "upstream test-tool helper is not available" "$test_tool_real"
PERL5LIB="$source_dir/perl/build/lib" "$perl_bin" -MGit -e 1
platform_helper="$(printf '%s\n' "$prepared_output" | sed -n 's/^platform_helper=//p')"
[[ -x "$platform_helper" && ! -L "$platform_helper" ]]

identity_source="$zmin_bin.identity.json"
[[ -f "$identity_source" && ! -L "$identity_source" ]] || {
  echo "current-contract positive requires a schema-v3 release identity sidecar" >&2
  exit 1
}
"$perl_bin" -MJSON::PP -MDigest::SHA=sha256_hex - \
  "$identity_source" <<'PERL'
use strict;
use warnings;
my ($path) = @ARGV;
open my $in, '<', $path or die "cannot read sidecar: $!\n";
local $/;
my $value = decode_json(<$in>);
close $in or die "cannot close sidecar: $!\n";
my $payload = $value->{payload_sha256};
delete $value->{payload_sha256};
my $json = JSON::PP->new->canonical(1)->ascii(1)->space_before(0)->space_after(0);
die "current sidecar payload is not freshly recomputable\n"
  unless defined($payload) && $payload eq sha256_hex($json->encode($value));
PERL
current_output="$(
  ZMIN_UPSTREAM_CONTRACT_GIT="$contract_git_bin" \
  ZMIN_UPSTREAM_CONTRACT_PYTHON="$contract_python_bin" \
  ZMIN_UPSTREAM_CONTRACT_MAKE="$make_bin" \
  run_prepare_with_bin "$zmin_bin" "$perl_bin" "$cache_root"
)"
printf '%s\n' "$current_output" | grep -qx 'zmin_binary_trust=trusted'
printf '%s\n' "$current_output" | grep -qx 'zmin_trust_binding=release-identity-sidecar-v3+current-checkout'
printf '%s\n' "$current_output" | grep -qx \
  'zmin_current_contract_detail=accepted by shared performance_contract.py'

sidecar_sentinel="$tmp_root/sidecar-sentinel"
sidecar_expected="$tmp_root/sidecar-expected"
printf 'sentinel\n' >"$sidecar_sentinel"
cp "$sidecar_sentinel" "$sidecar_expected"
sidecar_path="$source_dir/.zmin-test-tool.provenance.tsv"
rm -f "$sidecar_path"
ln -s "$sidecar_sentinel" "$sidecar_path"
if run_prepare "$perl_bin" "$cache_root" >/dev/null 2>&1; then
  echo "symlinked provenance sidecar was accepted" >&2
  exit 1
fi
cmp -s "$sidecar_sentinel" "$sidecar_expected"
rm -f "$sidecar_path"
run_prepare "$perl_bin" "$cache_root" >/dev/null
ln "$sidecar_sentinel" "$sidecar_path"
run_prepare "$perl_bin" "$cache_root" >/dev/null
cmp -s "$sidecar_sentinel" "$sidecar_expected"
[[ -f "$sidecar_path" && ! -L "$sidecar_path" ]]
rm -f "$sidecar_path"
run_prepare "$perl_bin" "$cache_root" >/dev/null
provenance_path="$source_dir/.zmin-test-tool.provenance.tsv"
grep -q $'perl_module_manifest_entry\tG\tperl/build/lib/' "$provenance_path"
grep -q $'perl_modules_manifest_sha256\t' "$provenance_path"

no_git_path="$tmp_root/no-git-bin"
mkdir -p "$no_git_path"
for tool in bash sh awk sed cat mkdir cp chmod mv find tar curl shasum dirname basename pwd tr rm sleep mktemp; do
  tool_path="$(command -v "$tool")"
  [[ -x "$tool_path" ]] || { echo "missing test utility: $tool" >&2; exit 1; }
  ln -s "$tool_path" "$no_git_path/$tool"
done
if ! PATH="$no_git_path" run_prepare "$perl_bin" "$cache_root" >/dev/null; then
  echo "prepare used ambient git or failed without it" >&2
  exit 1
fi
one_manifest="$tmp_root/one-test.tsv"
printf 'mode\ttest\treason\nall-nondeprecated\tt0000-basic.sh\tfocused harness negative\n' >"$one_manifest"
compat_dir="$tmp_root/compat-bin"
mkdir -p "$compat_dir"
compat_bin="$compat_dir/zmin"
cp "$zmin_bin" "$compat_bin"
source_helper="$(dirname "$zmin_bin")/zmin-git-remote-http"
if [[ ! -x "$source_helper" && -x "${source_helper}.exe" ]]; then source_helper="${source_helper}.exe"; fi
[[ -x "$source_helper" ]] || { echo "missing sibling remote HTTP helper for compat probe" >&2; exit 1; }
cp "$source_helper" "$compat_dir/$(basename "$source_helper")"
chmod +x "$compat_bin" "$compat_dir/$(basename "$source_helper")"
compat_out="$tmp_root/compat-out"
mkdir -p "$compat_out"
compat_output="$(run_bounded_phase "bounded compatibility probe" env \
  ZMIN_TEST_PERL="$perl_bin" ZMIN_BIN="$compat_bin" \
  ZMIN_UPSTREAM_GIT_TAG=v2.55.0 ZMIN_UPSTREAM_GIT_CACHE="$cache_root" \
  ZMIN_UPSTREAM_CONTRACT_MAKE="$make_bin" ZMIN_UPSTREAM_OUT_DIR="$compat_out" \
  ZMIN_UPSTREAM_TEST_LIST="$one_manifest" ZMIN_UPSTREAM_ALLOW_FAILURES=1 \
  ZMIN_UPSTREAM_CARGO_PROFILE=compat ZMIN_UPSTREAM_JOBS=1 \
  "$repo_root/tools/git-upstream-compat-suite.sh" all-nondeprecated
)"
printf '%s\n' "$compat_output" | grep -qx 'scope=exploratory-bounded'
printf '%s\n' "$compat_output" | grep -qx 'evidence_scope=exploratory'
grep -qx $'zmin_binary_trust\tuntrusted' "$compat_out/run-metadata.tsv"

identity_backup="$tmp_root/current-identity.backup.json"
cp "$identity_source" "$identity_backup"
"$perl_bin" -MJSON::PP -MDigest::SHA=sha256_hex - \
  "$identity_source" "$zmin_bin" "$tmp_root/stale-source" <<'PERL'
use strict;
use warnings;
my ($path, $binary, $stale_root) = @ARGV;
open my $in, '<', $path or die "cannot read sidecar: $!\n";
local $/;
my $value = decode_json(<$in>);
close $in or die "cannot close sidecar: $!\n";
$value->{repo_root} = $stale_root;
$value->{code_commit} = 'stale-current-checkout';
$value->{build_manifest}->{repo_root} = $stale_root;
$value->{source_snapshot}->{commit} = 'stale-current-checkout';
$value->{binary}->{path} = $binary;
$value->{sidecar_path} = "$binary.identity.json";
delete $value->{payload_sha256};
my $json = JSON::PP->new->canonical(1)->ascii(1)->space_before(0)->space_after(0);
$value->{payload_sha256} = sha256_hex($json->encode($value));
open my $out, '>', $path or die "cannot write sidecar: $!\n";
print {$out} $json->encode($value), "\n";
close $out or die "cannot close sidecar: $!\n";
PERL
stale_output="$(
  ZMIN_UPSTREAM_CONTRACT_GIT="$contract_git_bin" \
  ZMIN_UPSTREAM_CONTRACT_PYTHON="$contract_python_bin" \
  run_prepare_with_bin "$zmin_bin" "$perl_bin" "$cache_root"
)"
printf '%s\n' "$stale_output" | grep -qx 'zmin_binary_trust=untrusted'
printf '%s\n' "$stale_output" | grep -qx \
  'zmin_trust_reason=release sidecar is not bound to the current checkout/build contract'
printf '%s\n' "$stale_output" | grep -qx \
  'zmin_current_contract_detail=binary identity sidecar mismatch for repo_root'
cp "$identity_backup" "$identity_source"
identity_backup=""

trash_dir="$source_dir/t/trash directory.t0000-basic"
if [[ -e "$trash_dir" || -L "$trash_dir" ]]; then
  [[ ! -L "$trash_dir" ]] || { echo "unexpected pre-existing trash symlink" >&2; exit 1; }
  rm -rf "$trash_dir"
fi
trash_sentinel="$tmp_root/trash-sentinel"
trash_expected="$tmp_root/trash-expected"
printf 'trash sentinel\n' >"$trash_sentinel"
cp "$trash_sentinel" "$trash_expected"
ln -s "$trash_sentinel" "$trash_dir"
trash_out="$tmp_root/trash-out"
mkdir -p "$trash_out"
if run_bounded_phase "trash symlink probe" env \
  ZMIN_TEST_PERL="$perl_bin" ZMIN_BIN="$zmin_bin" \
  ZMIN_UPSTREAM_GIT_TAG=v2.55.0 ZMIN_UPSTREAM_GIT_CACHE="$cache_root" \
  ZMIN_UPSTREAM_CONTRACT_MAKE="$make_bin" ZMIN_UPSTREAM_OUT_DIR="$trash_out" \
  ZMIN_UPSTREAM_TEST_LIST="$one_manifest" ZMIN_UPSTREAM_ALLOW_FAILURES=1 \
  ZMIN_UPSTREAM_JOBS=1 "$repo_root/tools/git-upstream-compat-suite.sh" all-nondeprecated \
  >"$tmp_root/trash-run.out" 2>&1; then
  echo "trash symlink was accepted" >&2
  exit 1
fi
cmp -s "$trash_sentinel" "$trash_expected"
rm -f "$trash_dir"

if [[ "$test_tool" != *.exe ]]; then
  executed_helper_hash_before="$(shasum -a 256 "$test_tool" | awk '{ print $1 }')"
  printf '\n# deliberate executed-wrapper mutation\n' >>"$test_tool"
  run_prepare "$perl_bin" "$cache_root" >/dev/null
  executed_helper_hash_after="$(shasum -a 256 "$test_tool" | awk '{ print $1 }')"
  [[ "$executed_helper_hash_after" != "$executed_helper_hash_before" ]]
fi

helper_hash_before="$(shasum -a 256 "$test_tool_real" | awk '{ print $1 }')"
cp "$zmin_bin" "$test_tool_real"
run_prepare "$perl_bin" "$cache_root" >/dev/null
helper_hash_after="$(shasum -a 256 "$test_tool_real" | awk '{ print $1 }')"
[[ "$helper_hash_after" != "$helper_hash_before" ]]
! cmp -s "$zmin_bin" "$test_tool_real"

printf '\n# deliberate prepared-source mutation\n' >>"$source_dir/t/t9700-perl-git.sh"
run_prepare "$perl_bin" "$cache_root" >/dev/null
! grep -q 'deliberate prepared-source mutation' "$source_dir/t/t9700-perl-git.sh"

generated_perl_module="$source_dir/perl/build/lib/Git.pm"
printf '\n# deliberate generated-module mutation\n' >>"$generated_perl_module"
run_prepare "$perl_bin" "$cache_root" >/dev/null
! grep -q 'deliberate generated-module mutation' "$generated_perl_module"

generated_extra="$source_dir/perl/build/lib/ZminInjectedExtra.pm"
printf 'package ZminInjectedExtra;\n1;\n' >"$generated_extra"
run_prepare "$perl_bin" "$cache_root" >/dev/null
[[ ! -e "$generated_extra" && ! -L "$generated_extra" ]]

source_real="$tmp_root/source-real"
mv "$source_dir" "$source_real"
ln -s "$source_real" "$source_dir"
if run_prepare "$perl_bin" "$cache_root" >/dev/null 2>&1; then
  echo "symlinked prepared source was accepted" >&2
  exit 1
fi
rm "$source_dir"
mv "$source_real" "$source_dir"

pristine_source_dir="$cache_root/git-v2.55.0"
pristine_real="$tmp_root/pristine-real"
mv "$pristine_source_dir" "$pristine_real"
ln -s "$pristine_real" "$pristine_source_dir"
if run_prepare "$perl_bin" "$cache_root" >/dev/null 2>&1; then
  echo "symlinked pristine source was accepted" >&2
  exit 1
fi
rm "$pristine_source_dir"
mv "$pristine_real" "$pristine_source_dir"

printf 'upstream-prerequisites=valid\n'
