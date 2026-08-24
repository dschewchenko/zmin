#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
usage: tools/git-upstream-compat-suite.sh [quick|standard|exhaustive|all-nondeprecated|all-top-level]

Runs selected upstream Git t-suite tests against zmin (or ZMIN_BIN).

Environment:
  ZMIN_BIN                    Path to zmin. Builds release when omitted.
  ZMIN_TEST_PERL              Required absolute, non-symlink Perl interpreter
                              used for every upstream build/probe/timeout.
  ZMIN_UPSTREAM_CONTRACT_GIT  Required absolute, non-symlink Git binary used
                              to bind the current checkout for authoritative
                              Zmin identity validation.
  ZMIN_UPSTREAM_CONTRACT_PYTHON
                              Required absolute, non-symlink Python used to
                              invoke the shared schema-v3 release identity validator
                              for authoritative Zmin identity validation.
  ZMIN_UPSTREAM_CONTRACT_MAKE   Required absolute, non-symlink Make used for
                                every prepared-source graph/build invocation.
  ZMIN_UPSTREAM_GIT_TAG       Git tag to test against. Default: v2.55.0.
                              The frozen current-Git contract is v2.55.0;
                              other tags are exploratory only.
  ZMIN_UPSTREAM_GIT_CACHE     Cache dir for upstream Git source/build.
  ZMIN_GIT_HTTP_BUNDLE         Validated cache-local Git HTTP bundle selected
                               by the stock HTTP comparator tests.
  ZMIN_UPSTREAM_TEST_LIST     Test manifest TSV override. Defaults:
                              quick/standard -> tools/git-upstream-compat-tests-core.txt
                              exhaustive     -> auto-generated full upstream
                                                shell suite minus explicit
                                                legacy/external excludes
                              all-nondeprecated
                                              -> auto-generated full upstream
                                                 top-level shell suite minus
                                                 only whole-file deprecated
                                                 excludes
                              all-top-level   -> auto-generated complete
                                                 upstream top-level shell
                                                 suite
  ZMIN_UPSTREAM_OUT_DIR       Output dir for logs and summary.
  ZMIN_UPSTREAM_CARGO_PROFILE Cargo profile used when ZMIN_BIN is omitted.
                              Default: release. Use compat for faster
                              behavior-only iteration.
  ZMIN_UPSTREAM_MANIFEST_OFFSET
                              Skip this many selected top-level upstream shell
                              tests after mode resolution.
  ZMIN_UPSTREAM_MANIFEST_LIMIT
                              Run at most this many selected top-level
                              upstream shell tests after mode resolution.
  ZMIN_UPSTREAM_ALLOW_FAILURES=1  Report failures but exit 0.
  ZMIN_UPSTREAM_TEST_FLAGS    Flags passed to each upstream test. Default: -q.
  ZMIN_UPSTREAM_TEST_TIMEOUT  Per-file timeout in seconds. Default: 0 (disabled).
  ZMIN_UPSTREAM_STOCK_GIT_CONTROL=1
                              Run the selected upstream tests against stock git
                              from PATH instead of a zmin shim.
  ZMIN_UPSTREAM_BOUNDED_RUN=1
                              Stop an upstream test after the max numeric
                              --run selector. Use only for focused parity
                              slices where full skip-heavy Windows/MSYS loops
                              are not stable evidence.
  ZMIN_UPSTREAM_SKIP_UNSUPPORTED_REFTABLE=1
                               Skip upstream assertions that require reftable
                               ref storage, which Zmin does not support yet.
  ZMIN_UPSTREAM_PREPARE_ONLY=1
                               Prepare and validate the pinned upstream
                               prerequisites, then stop before test execution.
  ZMIN_UPSTREAM_PHASE_TIMEOUT_SECONDS
                               Maximum seconds for authoritative child phases;
                               default 30, hard maximum 45.
  ZMIN_UPSTREAM_LOCK_TIMEOUT_SECONDS
                               Maximum seconds waiting for an authenticated
                               cache/prepare lock; default follows phase timeout.
EOF
}

mode="${1:-quick}"
case "$mode" in
  quick|standard|exhaustive|all-nondeprecated|all-top-level) ;;
  -h|--help)
    usage
    exit 0
    ;;
  *)
    usage
    exit 2
    ;;
esac

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tag="${ZMIN_UPSTREAM_GIT_TAG:-v2.55.0}"
if [[ ! "$tag" =~ ^[A-Za-z0-9][A-Za-z0-9._-]*$ || "$tag" == *..* ]]; then
  echo "ZMIN_UPSTREAM_GIT_TAG must be a safe single archive path component" >&2
  exit 2
fi
cache_root="${ZMIN_UPSTREAM_GIT_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/zmin/git-upstream}"
contract_file="$repo_root/tools/git-upstream-compat-contract.tsv"
contract_tag="$(awk -F '\t' '$1 == "upstream_git_tag" { print $2; exit }' "$contract_file")"
contract_commit="$(awk -F '\t' '$1 == "upstream_git_commit" { print $2; exit }' "$contract_file")"
contract_archive_sha="$(awk -F '\t' '$1 == "upstream_archive_sha256" { print $2; exit }' "$contract_file")"
contract_denominator="$(awk -F '\t' '$1 == "authoritative_upstream_test_denominator" { print $2; exit }' "$contract_file")"
pristine_source_dir="$cache_root/git-$tag"
harness_fingerprint="$(shasum -a 256 "${BASH_SOURCE[0]}" | awk '{ print substr($1, 1, 12) }')"
source_dir="$cache_root/harness-$tag-$harness_fingerprint"
default_core_test_list="$repo_root/tools/git-upstream-compat-tests-core.txt"
test_list="${ZMIN_UPSTREAM_TEST_LIST:-}"
out_dir="${ZMIN_UPSTREAM_OUT_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/zmin-upstream-compat.XXXXXX")}"
jobs="${ZMIN_UPSTREAM_JOBS:-4}"
test_flags="${ZMIN_UPSTREAM_TEST_FLAGS:--q}"
custom_test_flags=0
if [[ -n "${ZMIN_UPSTREAM_TEST_FLAGS+x}" ]]; then
  custom_test_flags=1
fi
test_timeout="${ZMIN_UPSTREAM_TEST_TIMEOUT:-0}"
stock_git_control="${ZMIN_UPSTREAM_STOCK_GIT_CONTROL:-0}"
bounded_run="${ZMIN_UPSTREAM_BOUNDED_RUN:-0}"
allow_failures="${ZMIN_UPSTREAM_ALLOW_FAILURES:-0}"
skip_unsupported_reftable="${ZMIN_UPSTREAM_SKIP_UNSUPPORTED_REFTABLE:-0}"
prepare_only="${ZMIN_UPSTREAM_PREPARE_ONLY:-0}"
cargo_profile="${ZMIN_UPSTREAM_CARGO_PROFILE:-release}"
manifest_offset="${ZMIN_UPSTREAM_MANIFEST_OFFSET:-0}"
manifest_limit="${ZMIN_UPSTREAM_MANIFEST_LIMIT:-0}"
resolved_test_list=""
contract_scope="exploratory"
perl_bin="${ZMIN_TEST_PERL:-}"
verified_archive_sha=""
archive_commit_binding=""
reported_upstream_commit="not-embedded-in-archive"
zmin_bin_sha256=""
zmin_identity_path=""
zmin_identity_sha256=""
zmin_version=""
zmin_profile=""
zmin_binary_trust="untrusted"
zmin_trust_binding="none"
zmin_trust_reason="binary identity sidecar not validated"
zmin_current_contract_trust=0
zmin_current_contract_detail="not-run"
prepared_cleanup_python_trust=0
contract_git_bin="${ZMIN_UPSTREAM_CONTRACT_GIT:-}"
contract_python_bin="${ZMIN_UPSTREAM_CONTRACT_PYTHON:-}"
make_bin="${ZMIN_UPSTREAM_CONTRACT_MAKE:-}"
make_sha256=""
make_version=""
make_anchor_path=""
make_anchor_sha256=""
make_anchor_version=""
make_validation_in_progress=0
lock_root="$cache_root/.zmin-locks"
lock_rendezvous_identity=""
cache_lock="$lock_root/cache.lock"
cache_lock_held=0
cache_lock_fd=8
prepare_lock=""
prepare_lock_held=0
prepare_lock_fd=9
prepared_generated_graph=""
cache_temp_path=""
pristine_temp_path=""
source_temp_path=""
prepared_artifacts_manifest=""
prepared_artifacts_marker=""
prepared_artifacts_sha256=""
t5510_transform_id="t5510-fetch-reftable-skip-v1"
t5510_transform_sha256=""
manifest_fixture_mode="${ZMIN_UPSTREAM_MANIFEST_FIXTURE:-0}"
phase_timeout_seconds="${ZMIN_UPSTREAM_PHASE_TIMEOUT_SECONDS:-30}"
lock_timeout_seconds="${ZMIN_UPSTREAM_LOCK_TIMEOUT_SECONDS:-$phase_timeout_seconds}"
if [[ ! "$phase_timeout_seconds" =~ ^[1-9][0-9]*$ ]] ||
  (( phase_timeout_seconds > 45 )); then
  echo "ZMIN_UPSTREAM_PHASE_TIMEOUT_SECONDS must be an integer from 1 through 45" >&2
  exit 2
fi
if [[ ! "$lock_timeout_seconds" =~ ^[1-9][0-9]*$ ]] ||
  (( lock_timeout_seconds > 45 )); then
  echo "ZMIN_UPSTREAM_LOCK_TIMEOUT_SECONDS must be an integer from 1 through 45" >&2
  exit 2
fi

canonical_path() {
  local path="$1"
  local parent
  parent="$(cd -P "$(dirname "$path")" 2>/dev/null && pwd -P)" || return 1
  printf '%s/%s\n' "$parent" "$(basename "$path")"
}

acquire_owned_lock() {
  local lock="$1"
  local label="$2"
  local fd="$3"
  local canonical expected_identity
  [[ "$fd" == "8" || "$fd" == "9" ]] || {
    echo "$label uses an unsupported lock descriptor: $fd" >&2
    return 1
  }
  [[ -n "$contract_python_bin" && "$contract_python_bin" == /* &&
    -x "$contract_python_bin" && ! -L "$contract_python_bin" ]] || {
    echo "$label requires the trusted absolute Python runtime" >&2
    return 1
  }
  canonical="$(canonical_path "$contract_python_bin")" || return 1
  [[ "$canonical" == "$contract_python_bin" ]] || return 1
  expected_identity="${ZMIN_UPSTREAM_LOCK_PROBE_IDENTITY:-$lock_rendezvous_identity}"
  [[ -n "$expected_identity" ]] || {
    echo "$label has no authenticated rendezvous identity" >&2
    return 1
  }
  case "$fd" in
    8) exec 8>>"$lock" ;;
    9) exec 9>>"$lock" ;;
  esac
  if ! "$contract_python_bin" - "$fd" "$lock" "$expected_identity" \
    "$(basename "$cache_lock")" "prepare.lock" "$label" "$lock_timeout_seconds" <<'PY'
import errno
import fcntl
import os
import stat
import sys
import time


fd = int(sys.argv[1])
path = sys.argv[2]
identity_parts = sys.argv[3].split("/")
expected_parent = identity_parts[0]
if len(identity_parts) == 2:
    expected_lock = identity_parts[1]
elif len(identity_parts) == 3:
    if os.path.basename(path) == sys.argv[4]:
        expected_lock = identity_parts[1]
    elif os.path.basename(path) == sys.argv[5]:
        expected_lock = identity_parts[2]
    else:
        raise SystemExit("lock rendezvous name is not authenticated")
else:
    raise SystemExit("invalid lock rendezvous identity")
expected_parent = tuple(int(part) for part in expected_parent.split(":"))
expected_lock = tuple(int(part) for part in expected_lock.split(":"))
label = sys.argv[6]
timeout_seconds = int(sys.argv[7])
if os.name == "nt":
    raise SystemExit("descriptor advisory flock is unsupported on Windows")
directory_flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW
parent = os.path.dirname(path)
name = os.path.basename(path)
parent_fd = os.open(parent, directory_flags)
check_fd = None
try:
    parent_stat = os.fstat(parent_fd)
    parent_path_stat = os.stat(parent, follow_symlinks=False)
    if stat.S_ISLNK(parent_path_stat.st_mode) or not stat.S_ISDIR(parent_path_stat.st_mode):
        raise SystemExit("lock rendezvous parent is not a directory")
    if (parent_stat.st_dev, parent_stat.st_ino) != (parent_path_stat.st_dev, parent_path_stat.st_ino):
        raise SystemExit("lock rendezvous parent changed while opening")
    if (
        (parent_stat.st_dev, parent_stat.st_ino, parent_stat.st_nlink)
        != expected_parent
    ):
        raise SystemExit("lock rendezvous parent identity changed")
    if parent_stat.st_nlink != expected_parent[2] or parent_stat.st_mode & 0o222:
        raise SystemExit("lock rendezvous parent is writable or has unexpected links")
    check_fd = os.open(name, os.O_RDWR | os.O_NOFOLLOW, dir_fd=parent_fd)
    check_stat = os.fstat(check_fd)
    fd_stat = os.fstat(fd)
    if (check_stat.st_dev, check_stat.st_ino) != expected_lock:
        raise SystemExit("lock rendezvous file identity changed")
    if (fd_stat.st_dev, fd_stat.st_ino) != (check_stat.st_dev, check_stat.st_ino):
        raise SystemExit("lock descriptor is not the authenticated rendezvous file")
    current = os.stat(name, dir_fd=parent_fd, follow_symlinks=False)
    if stat.S_ISLNK(current.st_mode) or (current.st_dev, current.st_ino) != expected_lock:
        raise SystemExit("lock rendezvous path changed before flock")
    deadline = time.monotonic() + timeout_seconds
    while True:
        try:
            fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
            break
        except OSError as error:
            if error.errno not in (errno.EACCES, errno.EAGAIN):
                raise
            if time.monotonic() >= deadline:
                raise SystemExit(
                    "%s: timed out waiting for live lock after %ss"
                    % (label, timeout_seconds)
                )
            time.sleep(0.05)
    parent_after = os.fstat(parent_fd)
    path_after = os.stat(parent, follow_symlinks=False)
    lock_after = os.stat(name, dir_fd=parent_fd, follow_symlinks=False)
    if (
       (parent_after.st_dev, parent_after.st_ino, parent_after.st_nlink)
       != expected_parent or parent_after.st_mode & 0o222 or \
       (path_after.st_dev, path_after.st_ino, path_after.st_nlink) != expected_parent or \
       (lock_after.st_dev, lock_after.st_ino) != expected_lock
    ):
        raise SystemExit("lock rendezvous changed during flock")
finally:
    if check_fd is not None:
        os.close(check_fd)
    os.close(parent_fd)
PY
  then
    case "$fd" in
      8) exec 8>&- ;;
      9) exec 9>&- ;;
    esac
    return 1
  fi
}

ensure_lock_rendezvous() {
  local directory="$1"
  local cache_name="$2"
  local prepare_name="$3"
  "$contract_python_bin" - "$cache_root" "$directory" "$(basename "$cache_name")" "$(basename "$prepare_name")" <<'PY'
import ctypes
import errno
import os
import stat
import sys
import time


cache_root, directory, cache_name, prepare_name = sys.argv[1:]
if os.name == "nt":
    raise SystemExit("descriptor lock rendezvous is unsupported on Windows")
directory_flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW
if os.path.dirname(directory) != cache_root:
    raise SystemExit("lock directory is not directly contained by the authenticated cache")
if not cache_name or not prepare_name or any("/" in name or name in (".", "..") for name in (cache_name, prepare_name)):
    raise SystemExit("invalid lock rendezvous name")
parent_fd = os.open(cache_root, directory_flags)
directory_fd = None


def identity(value):
    return (value.st_dev, value.st_ino, stat.S_IFMT(value.st_mode))


def open_and_validate(directory_name):
    directory_fd = os.open(directory_name, directory_flags, dir_fd=parent_fd)
    try:
        directory_stat = os.fstat(directory_fd)
        path_stat = os.stat(directory_name, dir_fd=parent_fd, follow_symlinks=False)
        if (
            stat.S_ISLNK(path_stat.st_mode)
            or not stat.S_ISDIR(path_stat.st_mode)
            or identity(directory_stat) != identity(path_stat)
            or directory_stat.st_mode & 0o222
        ):
            raise SystemExit("lock rendezvous directory is writable, replaced, or not a directory")
        names = sorted(os.listdir(directory_fd))
        if names != sorted((cache_name, prepare_name)):
            raise SystemExit("lock rendezvous directory is incomplete or contains unexpected entries")
        identities = []
        for name in (cache_name, prepare_name):
            fd = os.open(name, os.O_RDWR | os.O_NOFOLLOW, dir_fd=directory_fd)
            try:
                value = os.fstat(fd)
                path_value = os.stat(name, dir_fd=directory_fd, follow_symlinks=False)
                if (
                    stat.S_ISLNK(path_value.st_mode)
                    or not stat.S_ISREG(path_value.st_mode)
                    or (value.st_dev, value.st_ino) != (path_value.st_dev, path_value.st_ino)
                ):
                    raise SystemExit("lock rendezvous file is not a stable regular file")
                identities.append("%d:%d" % (value.st_dev, value.st_ino))
            finally:
                os.close(fd)
        if directory_stat.st_nlink <= 0:
            raise SystemExit("lock rendezvous directory has invalid link count")
        return directory_fd, identities
    except BaseException:
        os.close(directory_fd)
        raise


def no_replace_rename(source_name, destination_name):
    source = source_name.encode()
    destination = destination_name.encode()
    libc = ctypes.CDLL(None, use_errno=True)
    if sys.platform == "linux":
        rename = getattr(libc, "renameat2", None)
        flags = 1
    elif sys.platform == "darwin":
        rename = getattr(libc, "renameatx_np", None)
        flags = 4
    else:
        rename = None
        flags = 0
    if rename is None:
        raise SystemExit("atomic no-replace lock-directory publication is unsupported")
    rename.argtypes = [ctypes.c_int, ctypes.c_char_p, ctypes.c_int, ctypes.c_char_p, ctypes.c_uint]
    rename.restype = ctypes.c_int
    if rename(parent_fd, source, parent_fd, destination, flags) != 0:
        error_number = ctypes.get_errno()
        if error_number == errno.EEXIST:
            raise FileExistsError(destination_name)
        raise OSError(error_number, os.strerror(error_number))


def create_private_rendezvous(temp_name):
    temp_fd = None
    created = []
    try:
        try:
            os.mkdir(temp_name, 0o700, dir_fd=parent_fd)
        except FileExistsError:
            raise SystemExit("lock rendezvous temporary name collided")
        temp_fd = os.open(temp_name, directory_flags, dir_fd=parent_fd)
        for name in (cache_name, prepare_name):
            fd = os.open(name, os.O_RDWR | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600, dir_fd=temp_fd)
            value = os.fstat(fd)
            created.append((name, (value.st_dev, value.st_ino)))
            os.fsync(fd)
            os.close(fd)
        os.fchmod(temp_fd, 0o500)
        validation_fd, _ = open_and_validate(temp_name)
        os.close(validation_fd)
        no_replace_rename(temp_name, os.path.basename(directory))
        os.fsync(parent_fd)
    except FileExistsError:
        if temp_fd is not None:
            os.fchmod(temp_fd, 0o700)
            temp_value = os.fstat(temp_fd)
            temp_path_value = os.stat(temp_name, dir_fd=parent_fd, follow_symlinks=False)
            if (
                stat.S_ISLNK(temp_path_value.st_mode)
                or not stat.S_ISDIR(temp_path_value.st_mode)
                or (temp_value.st_dev, temp_value.st_ino)
                != (temp_path_value.st_dev, temp_path_value.st_ino)
            ):
                raise SystemExit("private lock rendezvous loser directory changed")
            for name, expected in created:
                value = os.stat(name, dir_fd=temp_fd, follow_symlinks=False)
                if stat.S_ISLNK(value.st_mode) or (value.st_dev, value.st_ino) != expected:
                    raise SystemExit("private lock rendezvous loser entry changed")
                os.unlink(name, dir_fd=temp_fd)
            if os.listdir(temp_fd):
                raise SystemExit("private lock rendezvous loser directory is not empty")
            temp_path_value = os.stat(temp_name, dir_fd=parent_fd, follow_symlinks=False)
            if (temp_path_value.st_dev, temp_path_value.st_ino) != (temp_value.st_dev, temp_value.st_ino):
                raise SystemExit("private lock rendezvous loser directory was replaced")
            os.rmdir(temp_name, dir_fd=parent_fd)
            os.close(temp_fd)
            temp_fd = None
    finally:
        if temp_fd is not None:
            os.close(temp_fd)


try:
    parent_before = os.fstat(parent_fd)
    parent_path = os.stat(cache_root, follow_symlinks=False)
    if (parent_before.st_dev, parent_before.st_ino) != (parent_path.st_dev, parent_path.st_ino):
        raise SystemExit("cache parent changed while creating lock rendezvous")
    directory_name = os.path.basename(directory)
    try:
        directory_fd, identities = open_and_validate(directory_name)
    except FileNotFoundError:
        temp_name = ".zmin-locks.tmp.%d.%d" % (os.getpid(), time.time_ns())
        create_private_rendezvous(temp_name)
        directory_fd, identities = open_and_validate(directory_name)
    directory_after = os.fstat(directory_fd)
    print(
        "%d:%d:%d/%s/%s"
        % (
            directory_after.st_dev,
            directory_after.st_ino,
            directory_after.st_nlink,
            identities[0],
            identities[1],
        )
    )
finally:
    if directory_fd is not None:
        os.close(directory_fd)
    os.close(parent_fd)
PY
}

release_owned_lock() {
  local label="$1"
  local fd="$2"
  case "$fd" in
    8) exec 8>&- ;;
    9) exec 9>&- ;;
    *) echo "$label uses an unsupported lock descriptor: $fd" >&2; return 1 ;;
  esac
}

run_lock_probe() {
  local lock="${ZMIN_UPSTREAM_LOCK_PROBE_LOCK:?missing lock probe path}"
  local role="${ZMIN_UPSTREAM_LOCK_PROBE_ROLE:?missing lock probe role}"
  local events="${ZMIN_UPSTREAM_LOCK_PROBE_EVENTS:?missing lock probe events path}"
  local start="${ZMIN_UPSTREAM_LOCK_PROBE_START:?missing lock probe start path}"
  local release="${ZMIN_UPSTREAM_LOCK_PROBE_RELEASE:?missing lock probe release path}"
  local fd=8
  local attempt=0
  mkdir -p "$events"
  : >"$events/ready.$role"
  while [[ ! -e "$start" ]]; do
    attempt=$((attempt + 1))
    [[ "$attempt" -lt $((phase_timeout_seconds * 100)) ]] || {
      echo "lock probe $role: timed out waiting for start after ${phase_timeout_seconds}s" >&2
      return 124
    }
    sleep 0.01
  done
  acquire_owned_lock "$lock" "lock probe" "$fd" || return 1
  : >"$events/entered.$role"
  attempt=0
  while [[ ! -e "$release" ]]; do
    attempt=$((attempt + 1))
    [[ "$attempt" -lt $((phase_timeout_seconds * 100)) ]] || {
      echo "lock probe $role: timed out waiting for release after ${phase_timeout_seconds}s" >&2
      release_owned_lock "lock probe" "$fd" || true
      return 124
    }
    sleep 0.01
  done
  release_owned_lock "lock probe" "$fd" || return 1
  : >"$events/done.$role"
}

if [[ "${ZMIN_UPSTREAM_LOCK_PROBE:-0}" == "1" ]]; then
  run_lock_probe
  exit $?
fi

require_absolute_executable() {
  local name="$1"
  local path="$2"
  local canonical
  if [[ -z "$path" || "$path" != /* || ! -x "$path" || -L "$path" ]]; then
    echo "$name must be an existing absolute non-symlink executable" >&2
    exit 2
  fi
  canonical="$(canonical_path "$path")" || {
    echo "cannot canonicalize $name: $path" >&2
    exit 2
  }
  if [[ "$canonical" != "$path" ]]; then
    echo "$name must not contain symlinked components: $path" >&2
    exit 2
  fi
}

require_absolute_executable ZMIN_TEST_PERL "$perl_bin"
perl_version="$("$perl_bin" -e 'print $^V')"
export ZMIN_TEST_PERL="$perl_bin"
"$perl_bin" -MDigest::SHA -e 'Digest::SHA::sha256_hex("")' >/dev/null 2>&1 || {
  echo "ZMIN_TEST_PERL must provide Digest::SHA" >&2
  exit 2
}

sha256_file_with_validated_perl() {
  local path="$1"
  "$perl_bin" -MDigest::SHA -e '
    use strict;
    use warnings;
    my ($path) = @ARGV;
    open my $in, "<", $path or die "cannot read $path: $!\n";
    binmode $in;
    my $sha = Digest::SHA->new(256);
    my $buffer;
    while (read($in, $buffer, 1024 * 1024)) { $sha->add($buffer); }
    close $in or die "cannot close $path: $!\n";
    print $sha->hexdigest, "\n";
  ' "$path"
}

require_descriptor_bound_make_platform() {
  if [[ "$(uname -s 2>/dev/null || printf '%s' unknown)" != "Linux" ]]; then
    echo "authoritative Make execution requires Linux sealed-memfd/execveat support" >&2
    return 1
  fi
}

validate_make_identity() {
  local canonical version_line
  require_descriptor_bound_make_platform || return 1
  require_absolute_executable ZMIN_UPSTREAM_CONTRACT_MAKE "$make_bin"
  canonical="$(canonical_path "$make_bin")" || return 1
  [[ "$canonical" == "$make_bin" ]] || return 1
  make_sha256="$(shasum -a 256 "$make_bin" | awk '{ print $1 }')" || return 1
  [[ "$make_sha256" =~ ^[0-9a-f]{64}$ ]] || return 1
  if [[ "$make_validation_in_progress" == "1" ]]; then
    return 0
  fi
  if [[ -z "$make_version" ]]; then
    make_validation_in_progress=1
    if ! version_line="$(run_pinned_make --version 2>/dev/null)"; then
      make_validation_in_progress=0
      return 1
    fi
    make_validation_in_progress=0
    version_line="${version_line%%$'\n'*}"
    [[ -n "$version_line" && "$version_line" != *$'\t'* && "$version_line" != *$'\r'* ]] || return 1
    make_version="$version_line"
  fi
  if [[ -n "$make_anchor_path" ]]; then
    [[ "$make_bin" == "$make_anchor_path" &&
      "$make_sha256" == "$make_anchor_sha256" &&
      "$make_version" == "$make_anchor_version" ]] || {
      echo "authenticated Make identity changed during preparation" >&2
      return 1
    }
  else
    make_anchor_path="$make_bin"
    make_anchor_sha256="$make_sha256"
    make_anchor_version="$make_version"
  fi
}

run_pinned_make() {
  local status post_status
  [[ -n "$contract_python_bin" && -x "$contract_python_bin" && ! -L "$contract_python_bin" ]] || {
    echo "pinned Make execution requires the trusted contract Python" >&2
    return 1
  }
  validate_make_identity || return 1
  if "$contract_python_bin" - "$make_bin" "$make_sha256" "$perl_bin" \
    "$phase_timeout_seconds" "$@" <<'PY'
import ctypes
import hashlib
import os
import signal
import stat
import sys
import time


source_path, expected_sha, perl_path = sys.argv[1:4]
phase_timeout_seconds = int(sys.argv[4])
make_args = sys.argv[5:]
source_fd = None
pin_fd = None
exec_fd = None
source_identity = None
failure = None


if sys.platform == "darwin":
    raise SystemExit(
        "Darwin authoritative Make execution is unsupported: no descriptor-bound exec primitive"
    )


def fail(message):
    raise RuntimeError(message)


def digest_fd(fd):
    duplicate = os.dup(fd)
    try:
        os.lseek(duplicate, 0, os.SEEK_SET)
        digest = hashlib.sha256()
        while True:
            chunk = os.read(duplicate, 1024 * 1024)
            if not chunk:
                return digest.hexdigest()
            digest.update(chunk)
    finally:
        os.close(duplicate)


def open_readonly(path):
    flags = os.O_RDONLY | getattr(os, "O_BINARY", 0)
    if os.name != "nt":
        nofollow = getattr(os, "O_NOFOLLOW", None)
        if nofollow is None:
            fail("pinned Make execution requires O_NOFOLLOW on POSIX")
        flags |= nofollow
    return os.open(path, flags)


def copy_verified_source():
    global source_fd, pin_fd, source_identity
    path_stat = os.stat(source_path, follow_symlinks=False)
    if stat.S_ISLNK(path_stat.st_mode) or not stat.S_ISREG(path_stat.st_mode):
        fail("authenticated Make source path is not a regular non-symlink file")
    source_fd = open_readonly(source_path)
    source_stat = os.fstat(source_fd)
    if (source_stat.st_dev, source_stat.st_ino) != (path_stat.st_dev, path_stat.st_ino):
        fail("authenticated Make source changed while opening")
    source_identity = (source_stat.st_dev, source_stat.st_ino)
    if not stat.S_ISREG(source_stat.st_mode) or not (source_stat.st_mode & 0o111):
        fail("authenticated Make source is not an executable regular file")
    if digest_fd(source_fd) != expected_sha:
        fail("authenticated Make source changed before pinning")
    if sys.platform != "linux":
        fail("memfd descriptor-bound Make execution is supported only on Linux")
    memfd_create = getattr(os, "memfd_create", None)
    if memfd_create is None:
        fail("Linux Python lacks os.memfd_create")
    allow_sealing = getattr(os, "MFD_ALLOW_SEALING", None)
    cloexec = getattr(os, "MFD_CLOEXEC", None)
    if allow_sealing is None or cloexec is None:
        fail("Linux Python lacks sealed memfd primitives")
    if source_fd is None:
        fail("authenticated Make source descriptor is unavailable")
    pin_fd = memfd_create("zmin-pinned-make", allow_sealing | cloexec)
    os.fchmod(pin_fd, 0o500)
    os.lseek(source_fd, 0, os.SEEK_SET)
    while True:
        chunk = os.read(source_fd, 1024 * 1024)
        if not chunk:
            break
        view = memoryview(chunk)
        while view:
            written = os.write(pin_fd, view)
            if written <= 0:
                fail("sealed Make copy made no progress")
            view = view[written:]
    os.fsync(pin_fd)
    os.lseek(pin_fd, 0, os.SEEK_SET)
    if digest_fd(pin_fd) != expected_sha:
        fail("sealed Make copy hash mismatch")
    header = os.read(pin_fd, 4)
    os.lseek(pin_fd, 0, os.SEEK_SET)
    if header != b"\x7fELF":
        fail("Linux descriptor-bound Make requires a native ELF executable")
    import fcntl
    add_seals = getattr(fcntl, "F_ADD_SEALS", None)
    seal_flags = 0x01 | 0x02 | 0x04 | 0x08
    get_seals = getattr(fcntl, "F_GET_SEALS", None)
    if add_seals is None or get_seals is None:
        fail("Linux Python lacks F_ADD_SEALS")
    try:
        fcntl.fcntl(pin_fd, add_seals, seal_flags)
    except OSError as error:
        fail("cannot seal pinned Make memfd: %s" % error)
    if fcntl.fcntl(pin_fd, get_seals) & seal_flags != seal_flags:
        fail("sealed Make memfd did not retain all required seals")
    current_source = os.stat(source_path, follow_symlinks=False)
    if (current_source.st_dev, current_source.st_ino) != (source_stat.st_dev, source_stat.st_ino):
        fail("authenticated Make source changed while pinning")
    if digest_fd(pin_fd) != expected_sha:
        fail("sealed Make executable changed before launch")


def launch_pinned():
    global exec_fd
    environment = {
        "PATH": "/usr/bin:/bin:/usr/sbin:/sbin",
        "HOME": "/tmp",
        "TMPDIR": "/tmp",
        "LANG": "C",
        "LC_ALL": "C",
        "TZ": "UTC",
        "SHELL": "/bin/sh",
        "PERL_PATH": perl_path,
    }
    if sys.platform != "linux":
        fail("descriptor-bound Make execution is supported only on Linux")
    pin_stat = os.fstat(pin_fd)
    exec_fd = os.open(
        "/proc/self/fd/%d" % pin_fd,
        os.O_RDONLY | getattr(os, "O_CLOEXEC", 0),
    )
    exec_stat = os.fstat(exec_fd)
    if (
        (exec_stat.st_dev, exec_stat.st_ino, exec_stat.st_size)
        != (pin_stat.st_dev, pin_stat.st_ino, pin_stat.st_size)
        or not stat.S_ISREG(exec_stat.st_mode)
        or digest_fd(exec_fd) != expected_sha
    ):
        fail("sealed Make execution descriptor failed read-only identity validation")
    os.set_inheritable(exec_fd, True)
    error_read, error_write = os.pipe()
    pid = os.fork()
    if pid == 0:
        os.close(error_read)
        try:
            os.setsid()
        except OSError as error:
            os.write(
                error_write,
                ("setsid errno=%d (%s)" % (error.errno, error.strerror)).encode(),
            )
            os._exit(126)
        libc = ctypes.CDLL(None, use_errno=True)
        libc.execveat.restype = ctypes.c_int
        libc.execveat.argtypes = [ctypes.c_int, ctypes.c_char_p, ctypes.POINTER(ctypes.c_char_p), ctypes.POINTER(ctypes.c_char_p), ctypes.c_int]
        argv = [b"make"] + [argument.encode() for argument in make_args]
        envp = [f"{key}={value}".encode() for key, value in environment.items()]
        argv_array = (ctypes.c_char_p * (len(argv) + 1))(*argv, None)
        env_array = (ctypes.c_char_p * (len(envp) + 1))(*envp, None)
        libc.execveat(exec_fd, b"", argv_array, env_array, 0x1000)
        error_number = ctypes.get_errno()
        os.write(error_write, ("execveat errno=%d (%s)" % (error_number, os.strerror(error_number))).encode())
        os._exit(126)
    os.close(error_write)
    wait_status = None
    try:
        deadline = time.monotonic() + phase_timeout_seconds
        while True:
            waited, candidate_status = os.waitpid(pid, os.WNOHANG)
            if waited == pid:
                wait_status = candidate_status
                break
            if time.monotonic() >= deadline:
                try:
                    os.killpg(pid, signal.SIGTERM)
                except ProcessLookupError:
                    pass
                if wait_status is None:
                    try:
                        os.killpg(pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass
                    _, wait_status = os.waitpid(pid, 0)
                break
            time.sleep(0.05)
        exec_error = os.read(error_read, 4096)
        if exec_error:
            fail("pinned Make execveat failed: %s" % exec_error.decode("utf-8", "replace"))
        if wait_status is None:
            fail("pinned Make child did not report an exit status")
        if digest_fd(exec_fd) != expected_sha:
            fail("sealed Make execution descriptor changed during launch")
        return os.waitstatus_to_exitcode(wait_status)
    finally:
        os.close(error_read)
        os.close(exec_fd)
        exec_fd = None


try:
    copy_verified_source()
    child_status = launch_pinned()
    if digest_fd(pin_fd) != expected_sha:
        fail("sealed Make executable changed after launch")
    source_after = os.stat(source_path, follow_symlinks=False)
    if (source_after.st_dev, source_after.st_ino) != source_identity:
        fail("authenticated Make source inode changed after launch")
    if digest_fd(source_fd) != expected_sha:
        fail("authenticated Make source changed after launch")
except BaseException as error:
    failure = str(error)
    child_status = 1
finally:
    cleanup_failure = None
    if pin_fd is not None:
        os.close(pin_fd)
    if source_fd is not None:
        os.close(source_fd)
if cleanup_failure is not None:
    failure = cleanup_failure if failure is None else failure + "; " + cleanup_failure
if failure is not None:
    raise SystemExit(failure)
raise SystemExit(child_status)
PY
  then
    status=0
  else
    status=$?
  fi
  post_status=0
  validate_make_identity || post_status=1
  [[ "$post_status" == "0" ]] || return 1
  return "$status"
}

validate_zmin_identity() {
  local identity="$zmin_bin.identity.json"
  local canonical fields schema marker producer profile dirty binary_path binary_sha sidecar_path
  local python_path python_sha python_version
  [[ "$zmin_bin" == /* && -f "$zmin_bin" && -x "$zmin_bin" && ! -L "$zmin_bin" ]] || return 1
  canonical="$(canonical_path "$zmin_bin")" || return 1
  [[ "$canonical" == "$zmin_bin" ]] || return 1
  [[ -f "$identity" && ! -L "$identity" ]] || return 1
 fields="$("$perl_bin" -MJSON::PP -e '
   use strict;
   use warnings;
    use Digest::SHA qw(sha256_hex);
   my $value = decode_json(do { local $/; <> });
   die "invalid binary identity\n" unless ref($value) eq "HASH";
    my %expected = map { $_ => 1 } qw(
      schema_version marker producer repo_root code_commit code_dirty
      code_status_sha256 cargo_profile cargo_lock_sha256 git binary toolchain
      python make build_manifest cargo_config source_snapshot source_inputs build_command
      sidecar_path durability payload_sha256
    );
    die "unknown or missing identity fields\n"
      unless keys(%$value) == keys(%expected) && !grep { !$expected{$_} } keys(%$value);
   my $binary = $value->{binary};
   die "missing binary identity\n" unless ref($binary) eq "HASH";
   die "unknown binary fields\n"
     unless keys(%$binary) == 3 && !grep { !/^(?:path|sha256|version)$/ } keys(%$binary);
    die "invalid release build command\n"
      unless ref($value->{build_command}) eq "ARRAY";
    my $command = join("\x1f", @{$value->{build_command}});
    die "non-release build command\n"
      unless $command =~ /\x1fbuild\x1f/ && $command =~ /\x1f--release\x1f/ &&
        $command =~ /\x1f--locked\x1f/ && $command =~ /\x1f-p\x1fzmin-cli\x1f/ &&
        $command =~ /\x1f--bin\x1fzmin(?:\x1f|$)/;
   die "missing clean source identity\n"
      unless exists($value->{code_dirty}) && !$value->{code_dirty};
   my $python = $value->{python};
   die "missing Python identity\n" unless ref($python) eq "HASH" &&
     keys(%$python) == 3 && !grep { !/^(?:path|sha256|version)$/ } keys(%$python);
   my @python_values = values(%$python);
   die "invalid Python identity\n" unless !grep { !defined($_) || /[\t\r\n]/ } @python_values &&
     $python->{path} =~ m{^/} && $python->{sha256} =~ /^[0-9a-f]{64}$/;
   my $make = $value->{make};
   die "missing Make identity\n" unless ref($make) eq "HASH" &&
     keys(%$make) == 3 && !grep { !/^(?:path|sha256|version)$/ } keys(%$make);
   my @make_values = values(%$make);
   die "invalid Make identity\n" unless !grep { !defined($_) || /[\t\r\n]/ } @make_values &&
     $make->{path} =~ m{^/} && $make->{sha256} =~ /^[0-9a-f]{64}$/;
   my @fields = (
      $value->{schema_version},
      $value->{marker},
      $value->{producer},
      $value->{cargo_profile},
      ($value->{code_dirty} ? "true" : "false"),
     $binary->{path},
     $binary->{sha256},
      $binary->{version},
     $value->{sidecar_path},
      $python->{path}, $python->{sha256}, $python->{version},
      $make->{path}, $make->{sha256}, $make->{version},
    );
   die "incomplete binary identity\n" if grep { !defined($_) || /[\t\r\n]/ } @fields;
    my %payload = %$value;
    delete $payload{payload_sha256};
    my $canonical = JSON::PP->new->canonical(1)->ascii(1)->space_before(0)->space_after(0);
    push @fields, $value->{payload_sha256}, sha256_hex($canonical->encode(\%payload));
   print join("\t", @fields), "\n";
  ' "$identity")" || return 1
  IFS=$'\t' read -r schema marker producer profile dirty binary_path binary_sha binary_version sidecar_path python_path python_sha python_version make_path make_sha make_version payload_sha payload_expected <<<"$fields"
  [[ "$schema" == "3" ]] || return 1
  [[ "$marker" == "zmin-sanitized-release-build-v1" ]] || return 1
  [[ "$producer" == "tools/performance_contract.py build-release" ]] || return 1
  [[ "$profile" == "release" && "$dirty" == "false" ]] || return 1
 [[ "$binary_path" == "$zmin_bin" && "$binary_sha" == "$zmin_bin_sha256" && -n "$binary_version" ]] || return 1
 [[ "$("$zmin_bin" --version 2>&1)" == "$binary_version" ]] || return 1
  [[ "$payload_sha" =~ ^[0-9a-f]{64}$ && "$payload_sha" == "$payload_expected" ]] || return 1
 [[ "$sidecar_path" == "$identity" ]] || return 1
 [[ "$python_path" == /* && "$python_sha" =~ ^[0-9a-f]{64}$ &&
    -x "$python_path" && ! -L "$python_path" ]] || return 1
 [[ "$(canonical_path "$python_path")" == "$python_path" ]] || return 1
 [[ "$(shasum -a 256 "$python_path" | awk '{ print $1 }')" == "$python_sha" ]] || return 1
 [[ "$("$python_path" --version 2>&1)" == "$python_version" ]] || return 1
 [[ "$make_path" == /* && "$make_sha" =~ ^[0-9a-f]{64}$ && -n "$make_version" ]] || return 1
 [[ -z "$contract_python_bin" || "$contract_python_bin" == "$python_path" ]] || return 1
 contract_python_bin="$python_path"
 prepared_cleanup_python_trust=1
 zmin_identity_path="$identity"
 zmin_identity_sha256="$(shasum -a 256 "$identity" | awk '{ print $1 }')" || return 1
  zmin_version="$binary_version"
 zmin_profile="$profile"
  zmin_binary_trust="trusted"
  zmin_trust_binding="release-identity-sidecar-v3"
  zmin_trust_reason="matched sanitized release identity sidecar"
}

validate_current_zmin_identity() {
  [[ -n "$contract_git_bin" && -n "$contract_python_bin" ]] || return 1
  zmin_current_contract_detail="contract tool preflight rejected"
  local path canonical
  for path in "$contract_git_bin" "$contract_python_bin"; do
    [[ "$path" == /* && -x "$path" && ! -L "$path" ]] || return 1
    canonical="$(canonical_path "$path")" || return 1
    [[ "$canonical" == "$path" ]] || return 1
  done
  validate_make_identity || return 1
  if ! zmin_current_contract_detail="$(
    "$contract_python_bin" - "$repo_root" "$contract_git_bin" "$zmin_bin" \
    "$zmin_profile" "$zmin_identity_path" "$contract_python_bin" "$make_bin" 2>&1 <<'PY'
import importlib.util
import pathlib
import sys

repo_root, git_bin, zmin_bin, profile, sidecar_path, python_bin, make_bin = sys.argv[1:]
module_path = pathlib.Path(repo_root) / "tools" / "performance_contract.py"
spec = importlib.util.spec_from_file_location("zmin_performance_contract", module_path)
if spec is None or spec.loader is None:
    raise SystemExit("cannot load shared performance contract")
contract = importlib.util.module_from_spec(spec)
spec.loader.exec_module(contract)
repo = pathlib.Path(repo_root)
git = pathlib.Path(git_bin)
zmin = pathlib.Path(zmin_bin)
python = pathlib.Path(python_bin)
make = pathlib.Path(make_bin)
state = contract.repo_state(repo, git)
sidecar = contract.load_json(pathlib.Path(sidecar_path))
ok, detail = contract.sidecar_matches(
    sidecar,
    repo_root=repo,
    git_bin=git,
    binary=zmin,
    profile=profile,
    state=state,
    cargo_lock_sha256=contract.sha256_file(repo / "Cargo.lock"),
    python_bin=python,
    make_bin=make,
)
if not ok:
    print(detail, file=sys.stderr)
    raise SystemExit(1)
PY
  )"; then
    [[ -n "$zmin_current_contract_detail" ]] ||
      zmin_current_contract_detail="shared performance_contract.py rejected identity"
    return 1
  fi
  zmin_current_contract_detail="accepted by shared performance_contract.py"
}

resolve_cargo_target_dir() {
  if [[ -n "${CARGO_TARGET_DIR:-}" ]]; then
    printf '%s\n' "$CARGO_TARGET_DIR"
    return
  fi

  local metadata_json target_dir
  metadata_json="$(
    rustup run stable cargo metadata \
      --manifest-path "$repo_root/Cargo.toml" \
      --format-version 1 \
      --no-deps 2>/dev/null | tr -d '\n'
  )"
  target_dir="$(printf '%s' "$metadata_json" | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p')"
  target_dir="${target_dir//\\\\/\\}"

  if [[ -n "$target_dir" ]]; then
    printf '%s\n' "$target_dir"
    return
  fi

  printf '%s\n' "$repo_root/target"
}

if [[ "$cache_root" != /* ]]; then
  echo "ZMIN_UPSTREAM_GIT_CACHE must be an absolute path" >&2
  exit 2
fi
mkdir -p "$cache_root" "$out_dir"
cache_root_physical="$(cd -P "$cache_root" 2>/dev/null && pwd -P)" || {
  echo "cannot canonicalize upstream cache path: $cache_root" >&2
  exit 2
}
if [[ "$cache_root_physical" != "$cache_root" ]]; then
  echo "upstream cache path must not contain symlinked components: $cache_root" >&2
  exit 2
fi
cache_root="$cache_root_physical"
lock_root="$cache_root/.zmin-locks"
pristine_source_dir="$cache_root/git-$tag"
harness_fingerprint="$(shasum -a 256 "${BASH_SOURCE[0]}" | awk '{ print substr($1, 1, 12) }')"
source_dir="$cache_root/harness-$tag-$harness_fingerprint"
cache_lock="$lock_root/cache.lock"
prepare_lock="$lock_root/prepare.lock"
lock_rendezvous_identity="$(ensure_lock_rendezvous "$lock_root" "$cache_lock" "$prepare_lock")" || {
  echo "cannot establish authenticated non-writable lock rendezvous" >&2
  exit 2
}
prepared_artifacts_manifest="$cache_root/.zmin-prepared-artifacts-$tag-$harness_fingerprint.tsv"
prepared_artifacts_marker="$cache_root/.zmin-prepared-artifacts-$tag-$harness_fingerprint.sha256"
t5510_transform_sha256="$(printf '%s' "$t5510_transform_id" | shasum -a 256 | awk '{ print $1 }')"

stock_git=""
http_bundle="${ZMIN_GIT_HTTP_BUNDLE:-}"
http_git_relative=""
zmin_bin="${ZMIN_BIN:-}"
if [[ "$manifest_fixture_mode" != "1" ]]; then
  if [[ "$stock_git_control" != "1" && -z "$zmin_bin" ]]; then
    cargo_target_dir="$(resolve_cargo_target_dir)"
    cargo_args=(build --manifest-path "$repo_root/Cargo.toml" -p zmin-cli --bin zmin)
    if [[ "$cargo_profile" == "release" ]]; then
      cargo_args+=(--release)
    else
      cargo_args+=(--profile "$cargo_profile")
    fi
    rustup run stable cargo "${cargo_args[@]}" >/dev/null
    zmin_bin="$cargo_target_dir/$cargo_profile/zmin"
  elif [[ "$stock_git_control" != "1" && "$zmin_bin" != /* && "$zmin_bin" != [A-Za-z]:* ]]; then
    zmin_bin="$(cd "$repo_root" && pwd)/$zmin_bin"
  fi
  if [[ "$stock_git_control" != "1" ]]; then
    if [[ "${RUNNER_OS:-}" == "Windows" || "${OS:-}" == "Windows_NT" ]]; then
      if [[ ! -x "$zmin_bin" && -x "${zmin_bin}.exe" ]]; then
        zmin_bin="${zmin_bin}.exe"
      fi
    else
      if [[ ! -x "$zmin_bin" && -x "${zmin_bin}.exe" ]]; then
        zmin_bin="${zmin_bin}.exe"
      fi
    fi
  fi
  if [[ "$stock_git_control" != "1" && ! -x "$zmin_bin" ]]; then
    echo "missing executable ZMIN_BIN: $zmin_bin" >&2
    exit 2
  fi
  if [[ "$stock_git_control" != "1" ]]; then
    zmin_profile="$cargo_profile"
    zmin_identity_path="$zmin_bin.identity.json"
    if [[ -f "$zmin_bin" ]]; then
      zmin_bin_sha256="$(shasum -a 256 "$zmin_bin" | awk '{ print $1 }')"
    fi
    if ! validate_zmin_identity; then
      zmin_trust_reason="missing or mismatched sanitized release identity sidecar"
    elif validate_current_zmin_identity; then
      zmin_current_contract_trust=1
      zmin_trust_binding="release-identity-sidecar-v3+current-checkout"
      zmin_trust_reason="matched current checkout release identity contract"
    else
      zmin_binary_trust="untrusted"
      zmin_trust_binding="release-identity-sidecar-v3"
      zmin_trust_reason="release sidecar is not bound to the current checkout/build contract"
    fi
  fi
fi

resolve_zmin_remote_http_helper() {
  local helper_dir helper_name helper_path cargo_args
  helper_dir="$(dirname "$zmin_bin")"
  if [[ "${RUNNER_OS:-}" == "Windows" || "${OS:-}" == "Windows_NT" ]]; then
    helper_name="zmin-git-remote-http.exe"
  else
    helper_name="zmin-git-remote-http"
  fi
  helper_path="$helper_dir/$helper_name"
  if [[ -x "$helper_path" ]]; then
    printf '%s\n' "$helper_path"
    return
  fi

  cargo_args=(
    build
    --manifest-path "$repo_root/Cargo.toml"
    -p zmin-git-remote-http
  )
  if [[ "$cargo_profile" == "release" ]]; then
    cargo_args+=(--release)
  else
    cargo_args+=(--profile "$cargo_profile")
  fi
  rustup run stable cargo "${cargo_args[@]}" >/dev/null

  if [[ -x "$helper_path" ]]; then
    printf '%s\n' "$helper_path"
    return
  fi

  echo "missing zmin remote HTTP helper after build: $helper_path" >&2
  exit 2
}

zmin_remote_http_helper=""
zmin_remote_http_helper_name=""
if [[ "$manifest_fixture_mode" != "1" && "$stock_git_control" != "1" ]]; then
  zmin_remote_http_helper="$(resolve_zmin_remote_http_helper)"
  zmin_remote_http_helper_name="$(basename "$zmin_remote_http_helper")"
fi

ensure_git_http_backend() {
  local backend_dir
  backend_dir="$(dirname "$zmin_bin")"
  if [[ "${RUNNER_OS:-}" == "Windows" || "${OS:-}" == "Windows_NT" ]]; then
    local backend="$backend_dir/git-http-backend.exe"
    if [[ ! -x "$backend" ]]; then
      cp "$zmin_bin" "$backend"
    fi
  else
    local backend="$backend_dir/git-http-backend"
    if [[ ! -x "$backend" ]]; then
      cat >"$backend" <<EOF
#!/usr/bin/env sh
exec "$zmin_bin" http-backend "\$@"
EOF
      chmod +x "$backend"
    fi
  fi
}

require_cache_directory() {
  local path="$1"
  local label="$2"
  local canonical parent
  if [[ "$path" != "$cache_root/"* || -L "$path" ]]; then
    echo "$label is not a safe cache path: $path" >&2
    return 1
  fi
  if [[ -e "$path" ]]; then
    [[ -d "$path" ]] || {
      echo "$label is not a directory: $path" >&2
      return 1
    }
    canonical="$(cd -P "$path" 2>/dev/null && pwd -P)" || return 1
    [[ "$canonical" == "$path" ]] || {
      echo "$label contains a symlinked path component: $path" >&2
      return 1
    }
  else
    parent="$(cd -P "$(dirname "$path")" 2>/dev/null && pwd -P)" || return 1
    [[ "$parent/$(basename "$path")" == "$path" ]] || {
      echo "$label parent contains a symlinked path component: $path" >&2
      return 1
    }
  fi
}

write_source_manifest() {
  local root="$1"
  local output="$2"
  "$perl_bin" -MFile::Find -MDigest::SHA -e '
    use strict;
    use warnings;
    my ($root, $output, $artifact_manifest) = @ARGV;
    my %excluded = (
      "./.zmin-pristine-source.sha256" => 1,
      "./.zmin-test-tool.provenance.tsv" => 1,
    );
    if (defined($artifact_manifest) && length($artifact_manifest)) {
      open my $allow, "<", $artifact_manifest or die "cannot read prepared artifact manifest: $!\n";
      while (my $line = <$allow>) {
        chomp $line;
        my @fields = split /\t/, $line, -1;
        my ($kind, $relative) = @fields[0, 1];
        die "invalid prepared artifact manifest entry\n"
          unless defined($kind) && defined($relative) &&
            (($kind eq "F" && @fields == 3) ||
             ($kind eq "D" && @fields == 2) ||
             ($kind eq "P" && @fields == 5) ||
             ($kind eq "R" && @fields == 5)) &&
            $relative =~ m{^\./[^\t\r\n]+$};
        if ($kind eq "F" || $kind eq "P") {
          die "invalid prepared artifact digest\n"
            unless $fields[2] =~ /^[0-9a-f]{64}$/;
        }
        if ($kind eq "P") {
          die "invalid prepared Perl artifact source\n"
            unless $fields[3] =~ m{^\./perl/[^\t\r\n]+\.pm$} &&
              $fields[4] =~ /^[0-9a-f]{64}$/;
        }
        if ($kind eq "R") {
          die "invalid prepared reftable transform entry\n"
            unless $relative eq "./t/t5510-fetch.sh" &&
              $fields[2] =~ /^[0-9a-f]{64}$/ &&
              $fields[3] =~ /^[0-9a-f]{64}$/ &&
              $fields[4] =~ /^[0-9a-f]{64}$/;
        }
        $excluded{$relative} = 1;
      }
      close $allow or die "cannot close prepared artifact manifest: $!\n";
    }
    my @paths;
    find({ no_chdir => 1, follow => 0, wanted => sub { push @paths, $File::Find::name } }, $root);
    @paths = sort @paths;
    open my $out, ">", $output or die "cannot write source manifest: $!\n";
    binmode $out;
    for my $path (@paths) {
      (my $relative = $path) =~ s/^\Q$root\E\/?//;
      next if $relative eq "";
      $relative = "./$relative";
      next if $excluded{$relative};
      if (-l $path) {
        my $target = readlink $path;
        die "cannot read symlink $path: $!\n" unless defined $target;
        print {$out} "L\t$relative\t$target\n";
      } elsif (-f $path) {
        open my $in, "<", $path or die "cannot read $path: $!\n";
        binmode $in;
        if ($relative eq "./t/test-lib.sh") {
          local $/;
          my $data = <$in>;
          $data =~ s{
\n\tif test -n "\$ZMIN_UPSTREAM_STOP_AFTER_TEST" &&
\t   test "\$test_count" -ge "\$ZMIN_UPSTREAM_STOP_AFTER_TEST"
\tthen
\t\ttest_done
\tfi
}{\n}g;
          $data =~ s/\*MINGW\*\|\*MSYS\*\)/\*MINGW\*\)/g;
          $data =~ s/GIT_TEST_CMP="GIT_DIR=\/dev\/null git diff --no-index --ignore-cr-at-eol --"/GIT_TEST_CMP="diff -u"/g;
          $data =~ s/GIT_TEST_CMP="\$DIFF -u"/GIT_TEST_CMP="diff -u"/g;
          $data =~ s/GIT_TEST_CMP=" +-u"/GIT_TEST_CMP="diff -u"/g;
          my $sha = Digest::SHA::sha256_hex($data);
          print {$out} "F\t$relative\t$sha\n";
          close $in or die "cannot close $path: $!\n";
          next;
        }
        my $sha = Digest::SHA->new(256);
        my $buffer;
        while (read($in, $buffer, 1024 * 1024)) {
          $sha->add($buffer);
        }
        close $in or die "cannot close $path: $!\n";
        print {$out} "F\t$relative\t", $sha->hexdigest, "\n";
      } elsif (-d $path) {
        print {$out} "D\t$relative\n";
      } else {
        die "unsupported source entry: $path\n";
      }
    }
    close $out or die "cannot close source manifest: $!\n";
  ' "$root" "$output" "${3:-}"
}

source_manifest_sha256() {
  local root="$1"
  local artifact_manifest="${2:-}"
  local manifest digest
  manifest="$(mktemp "$cache_root/.zmin-source-manifest.XXXXXX")" || return 1
  if ! write_source_manifest "$root" "$manifest" "$artifact_manifest"; then
    rm -f "$manifest"
    return 1
  fi
  digest="$(shasum -a 256 "$manifest" | awk '{ print $1 }')" || {
    rm -f "$manifest"
    return 1
  }
  rm -f "$manifest"
  printf '%s\n' "$digest"
}

write_prepared_artifacts_manifest() {
  local root="$1"
  local pristine="$2"
  local output="${3:-}"
  "$contract_python_bin" - "$root" "$pristine" "$output" \
    "$skip_unsupported_reftable" "$t5510_transform_sha256" \
    "$prepared_generated_graph" "$manifest_fixture_mode" \
    "${ZMIN_UPSTREAM_MANIFEST_FIXTURE_LIMITS:-}" <<'PY'
import hashlib
import os
import stat
import sys


class SnapshotError(Exception):
    pass


def require(condition, message):
    if not condition:
        raise SnapshotError(message)


(root_path, pristine_path, output_path, skip_reftable, transform_sha,
 generated_graph, fixture_mode, fixture_limits) = sys.argv[1:]
skip_reftable = skip_reftable == "1"

require(os.path.isabs(root_path) and os.path.isabs(pristine_path),
        "prepared snapshot paths must be absolute")
require(hasattr(os, "O_DIRECTORY") and hasattr(os, "O_NOFOLLOW"),
        "descriptor-relative no-follow snapshot primitives are unavailable")
require(os.open in os.supports_dir_fd and os.stat in os.supports_dir_fd and
        os.readlink in os.supports_dir_fd,
        "descriptor-relative snapshot operations are unavailable")

DIR_FLAGS = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW
FILE_FLAGS = os.O_RDONLY | os.O_NOFOLLOW | getattr(os, "O_CLOEXEC", 0)


def identity(st):
    return (st.st_dev, st.st_ino, st.st_mode & stat.S_IFMT(st.st_mode),
            st.st_nlink, st.st_size, st.st_mtime_ns, st.st_ctime_ns)


def open_directory(path):
    fd = os.open(os.sep, DIR_FLAGS)
    try:
        for component in path.split(os.sep):
            if not component:
                continue
            before = os.stat(component, dir_fd=fd, follow_symlinks=False)
            require(stat.S_ISDIR(before.st_mode),
                    "retained root contains a non-directory component")
            child = os.open(component, DIR_FLAGS, dir_fd=fd)
            try:
                require(identity(before) == identity(os.fstat(child)),
                        "retained root component changed during open")
            except Exception:
                os.close(child)
                raise
            os.close(fd)
            fd = child
        return fd
    except Exception:
        os.close(fd)
        raise


class Snapshot:
    def __init__(self, path):
        self.path = path
        self.entries = {}
        self.root_fd = open_directory(path)
        self.root_identity = identity(os.fstat(self.root_fd))
        try:
            self.walk(self.root_fd, [])
            require(identity(os.fstat(self.root_fd)) == self.root_identity,
                    "retained root changed during snapshot")
        finally:
            os.close(self.root_fd)

    @staticmethod
    def relative(parts):
        return "./" + "/".join(parts)

    def read_file(self, fd, relative, opened):
        opened_identity = identity(os.fstat(fd))
        require(opened_identity == identity(opened),
                "retained file changed before hashing: " + relative)
        hasher = hashlib.sha256()
        total = 0
        while True:
            data = os.read(fd, 1024 * 1024)
            if not data:
                break
            total += len(data)
            hasher.update(data)
        require(identity(os.fstat(fd)) == opened_identity,
                "retained file changed while hashing: " + relative)
        return {"kind": "F", "size": total, "sha": hasher.hexdigest()}

    def walk(self, directory_fd, parts):
        before = identity(os.fstat(directory_fd))
        try:
            names = sorted(os.listdir(directory_fd))
            for name in names:
                current_parts = parts + [name]
                relative = self.relative(current_parts)
                entry = os.stat(name, dir_fd=directory_fd,
                                follow_symlinks=False)
                mode = entry.st_mode
                if stat.S_ISLNK(mode):
                    self.entries[relative] = {"kind": "L",
                                               "target": os.readlink(
                                                   name, dir_fd=directory_fd)}
                elif stat.S_ISDIR(mode):
                    child = os.open(name, DIR_FLAGS, dir_fd=directory_fd)
                    try:
                        require(identity(entry) == identity(os.fstat(child)),
                                "retained directory changed during open: " +
                                relative)
                        self.entries[relative] = {"kind": "D"}
                        self.walk(child, current_parts)
                    finally:
                        os.close(child)
                elif stat.S_ISREG(mode):
                    child = os.open(name, FILE_FLAGS, dir_fd=directory_fd)
                    try:
                        self.entries[relative] = self.read_file(
                            child, relative, entry)
                    finally:
                        os.close(child)
                else:
                    raise SnapshotError("unsupported retained entry: " +
                                        relative)
            require(identity(os.fstat(directory_fd)) == before,
                    "retained directory changed during enumeration")
        except OSError as error:
            raise SnapshotError("retained snapshot failed: " + str(error))


def parse_graph(raw):
    result = set()
    for line in raw.splitlines():
        if not line:
            continue
        if line.startswith("./"):
            line = line[2:]
        require(line == "po/build" or
                (line.endswith("/.depend") or line == ".depend"),
                "invalid evaluated generated directory")
        parts = line.split("/")
        require(all(part not in ("", ".", "..") for part in parts),
                "invalid evaluated generated directory")
        relative = "./" + line
        require(relative not in result,
                "duplicate evaluated generated directory")
        result.add(relative)
    return result


def generated_path(relative, directories):
    return any(relative == directory or
               relative.startswith(directory + "/")
               for directory in directories)


def emit(lines, output):
    payload = "".join(line + "\n" for line in lines)
    if not output:
        sys.stdout.write(payload)
        return
    flags = os.O_WRONLY | os.O_CREAT | os.O_TRUNC | os.O_NOFOLLOW
    fd = os.open(output, flags, 0o600)
    try:
        view = memoryview(payload.encode())
        while view:
            written = os.write(fd, view)
            require(written > 0, "prepared manifest write made no progress")
            view = view[written:]
        os.fsync(fd)
    finally:
        os.close(fd)


try:
    pristine = Snapshot(pristine_path)
    current = Snapshot(root_path)
    graph = parse_graph(generated_graph)
    node_limit = 65536
    byte_limit = 1024 * 1024 * 1024
    if fixture_mode == "1" and fixture_limits:
        fields = fixture_limits.split(":")
        require(len(fields) == 2 and all(field.isdigit() and int(field) > 0
                                         for field in fields),
                "invalid manifest fixture limits")
        node_limit, byte_limit = (int(field) for field in fields)

    generated_nodes = 0
    generated_bytes = 0
    for relative in sorted(current.entries):
        if relative in pristine.entries:
            continue
        entry = current.entries[relative]
        require(entry["kind"] != "L", "prepared generated symlink is forbidden: " +
                relative)
        require(entry["kind"] in ("D", "F"),
                "unsupported evaluated generated entry " + relative)
        generated_nodes += 1
        require(generated_nodes <= node_limit,
                "prepared generated node limit exceeded")
        if entry["kind"] == "F":
            generated_bytes += entry["size"]
            require(generated_bytes <= byte_limit,
                    "prepared generated byte limit exceeded")

    allowed_files = {"./" + item for item in (
        "Cargo.lock", "GIT-BUILD-OPTIONS", "GIT-CFLAGS", "GIT-LDFLAGS",
        "GIT-PERL-DEFINES", "GIT-PREFIX", "GIT-USER-AGENT",
        "GIT-VERSION-FILE", "command-list.h", "hook-list.h", "version-def.h",
        "git-sh-i18n--envsubst", "git-sh-i18n--envsubst.exe", "git",
        "git.exe", "git-http-backend", "git-http-backend.exe", "git-sh-i18n",
        "git-sh-i18n.exe", "git-sh-setup", "git-sh-setup.exe",
        "templates/boilerplates.made", "templates/blt/description",
        "templates/blt/hooks/applypatch-msg.sample",
        "templates/blt/hooks/commit-msg.sample",
        "templates/blt/hooks/fsmonitor-watchman.sample",
        "templates/blt/hooks/post-update.sample",
        "templates/blt/hooks/pre-applypatch.sample",
        "templates/blt/hooks/pre-commit.sample",
        "templates/blt/hooks/pre-merge-commit.sample",
        "templates/blt/hooks/prepare-commit-msg.sample",
        "templates/blt/hooks/pre-push.sample",
        "templates/blt/hooks/pre-rebase.sample",
        "templates/blt/hooks/pre-receive.sample",
        "templates/blt/hooks/push-to-checkout.sample",
        "templates/blt/hooks/sendemail-validate.sample",
        "templates/blt/hooks/update.sample", "templates/blt/info/exclude",
        "t/helper/test-tool", "t/helper/test-tool-real", "t/helper/test-tool.exe",
        "t/helper/test-tool-real.exe")}
    allowed_dirs = {"./" + item for item in (
        "perl/build", "perl/build/lib", "templates/blt", "templates/blt/hooks",
        "templates/blt/info")}
    lines = []
    reftable_count = 0
    for relative in sorted(current.entries):
        if relative in ("./t/test-lib.sh", "./.zmin-test-tool.provenance.tsv"):
            continue
        entry = current.entries[relative]
        source = pristine.entries.get(relative)
        if skip_reftable and relative == "./t/t5510-fetch.sh":
            require(entry["kind"] == "F" and source and source["kind"] == "F",
                    "prepared reftable transform must be a regular file")
            require(entry["sha"] != source["sha"],
                    "reftable transform did not change t5510-fetch.sh")
            require(len(transform_sha) == 64 and
                    all(char in "0123456789abcdef" for char in transform_sha),
                    "invalid reftable transform identity")
            lines.append("R\t%s\t%s\t%s\t%s" %
                         (relative, source["sha"], entry["sha"], transform_sha))
            reftable_count += 1
            continue
        if source is not None:
            require(entry["kind"] == source["kind"],
                    "prepared source changed static entry " + relative)
            if entry["kind"] == "F":
                require(entry["sha"] == source["sha"],
                        "prepared source changed static file " + relative)
            elif entry["kind"] == "L":
                require(entry.get("target") == source.get("target"),
                        "prepared source changed static symlink " + relative)
            continue
        if relative.startswith("./perl/build/lib/"):
            suffix = relative[len("./perl/build/lib/"):]
            source_relative = "./perl/" + suffix
            source_file = pristine.entries.get(source_relative)
            current_source = current.entries.get(source_relative)
            if entry["kind"] == "D" and source_file and current_source and \
                    source_file["kind"] == "D" and current_source["kind"] == "D":
                lines.append("D\t" + relative)
                continue
            if entry["kind"] == "F" and suffix.endswith(".pm"):
                require(source_file and source_file["kind"] == "F" and
                        current_source and current_source["kind"] == "F",
                        "generated Perl module has no canonical source: " + relative)
                require(current_source["sha"] == source_file["sha"],
                        "generated Perl source changed: " + source_relative)
                lines.append("P\t%s\t%s\t%s\t%s" %
                             (relative, entry["sha"], source_relative,
                              source_file["sha"]))
                continue
        if generated_path(relative, graph):
            require(entry["kind"] in ("D", "F"),
                    "unsupported evaluated generated entry " + relative)
            if entry["kind"] == "D":
                lines.append("D\t" + relative)
            else:
                lines.append("F\t%s\t%s" % (relative, entry["sha"]))
            continue
        require(relative in allowed_files or relative in allowed_dirs,
                "unexpected prepared source entry " + relative)
        require(entry["kind"] in ("D", "F"),
                "unsupported prepared source entry " + relative)
        if entry["kind"] == "F":
            require(relative in allowed_files,
                    "prepared generated file is not canonical: " + relative)
            lines.append("F\t%s\t%s" % (relative, entry["sha"]))
        else:
            require(relative in allowed_dirs,
                    "prepared generated directory is not canonical: " + relative)
            lines.append("D\t" + relative)
    require((not skip_reftable and reftable_count == 0) or
            (skip_reftable and reftable_count == 1),
            "required reftable transform artifact is missing or unexpected")
    emit(lines, output_path)
except (OSError, SnapshotError) as error:
    print(str(error), file=sys.stderr)
    sys.exit(1)
PY
}

compare_prepared_artifacts_manifest() {
  local root="$1"
  local pristine="$2"
  local expected="$3"
  local actual graph
  graph="$prepared_generated_graph"
  if [[ -z "$graph" && -f "$expected" ]]; then
    graph="$(awk -F '\t' '$1 == "D" && ($2 == "./po/build" || $2 ~ /\/\.depend$/) { sub(/^\.\//, "", $2); print $2 }' "$expected")"
  fi
  local saved_graph="$prepared_generated_graph"
  prepared_generated_graph="$graph"
  actual="$(mktemp "$cache_root/.zmin-prepared-artifacts-check.XXXXXX")" || return 1
  if ! write_prepared_artifacts_manifest "$root" "$pristine" "$actual"; then
    prepared_generated_graph="$saved_graph"
    rm -f "$actual"
    return 1
  fi
  if ! cmp -s "$expected" "$actual"; then
    prepared_generated_graph="$saved_graph"
    rm -f "$actual"
    return 1
  fi
  prepared_generated_graph="$saved_graph"
  rm -f "$actual"
}

publish_prepared_artifacts_manifest() {
  local payload digest
  payload="$(write_prepared_artifacts_manifest "$source_dir" "$pristine_source_dir")" || return 1
  digest="$(printf '%s\n' "$payload" | shasum -a 256 | awk '{ print $1 }')" || return 1
  publish_descriptor_file "$prepared_artifacts_manifest" "$payload" || return 1
  publish_descriptor_file "$prepared_artifacts_marker" "$digest" || return 1
  prepared_artifacts_sha256="$digest"
}

validate_prepared_artifacts_manifest() {
  local expected digest
  [[ -f "$prepared_artifacts_manifest" && ! -L "$prepared_artifacts_manifest" &&
    -f "$prepared_artifacts_marker" && ! -L "$prepared_artifacts_marker" ]] || return 1
  expected="$(cat "$prepared_artifacts_marker")" || return 1
  [[ "$expected" =~ ^[0-9a-f]{64}$ ]] || return 1
  digest="$(shasum -a 256 "$prepared_artifacts_manifest" | awk '{ print $1 }')" || return 1
  [[ "$digest" == "$expected" ]] || return 1
  prepared_artifacts_sha256="$digest"
  compare_prepared_artifacts_manifest "$source_dir" "$pristine_source_dir" \
    "$prepared_artifacts_manifest" || return 1
  validate_reftable_artifact_metadata
}

validate_reftable_artifact_metadata() {
  local line count kind path base_sha post_sha transform_sha expected_base expected_post
  count="$(grep -c '^R[[:space:]]' "$prepared_artifacts_manifest" 2>/dev/null || true)"
  if [[ "$skip_unsupported_reftable" != "1" ]]; then
    [[ "$count" == "0" ]] || return 1
    return 0
  fi
  [[ "$count" == "1" ]] || return 1
  line="$(grep '^R[[:space:]]' "$prepared_artifacts_manifest")" || return 1
  IFS=$'\t' read -r kind path base_sha post_sha transform_sha <<<"$line"
  [[ "$kind" == "R" && "$path" == "./t/t5510-fetch.sh" &&
    "$base_sha" =~ ^[0-9a-f]{64}$ && "$post_sha" =~ ^[0-9a-f]{64}$ &&
    "$transform_sha" == "$t5510_transform_sha256" && "$base_sha" != "$post_sha" ]] || return 1
  expected_base="$(shasum -a 256 "$pristine_source_dir/t/t5510-fetch.sh" | awk '{ print $1 }')" || return 1
  expected_post="$(shasum -a 256 "$source_dir/t/t5510-fetch.sh" | awk '{ print $1 }')" || return 1
  [[ "$base_sha" == "$expected_base" && "$post_sha" == "$expected_post" ]]
}

validate_test_lib_patch() {
  "$perl_bin" - "$pristine_source_dir/t/test-lib.sh" "$source_dir/t/test-lib.sh" \
    "${RUNNER_OS:-}" "${OS:-}" <<'PERL'
use strict;
use warnings;
sub slurp {
  my ($path) = @_;
  open my $fh, '<', $path or die "cannot read $path: $!\n";
  binmode $fh;
  local $/;
  my $data = <$fh>;
  close $fh or die "cannot close $path: $!\n";
  return $data;
}
my ($base_path, $prepared_path, $runner_os, $os) = @ARGV;
my $expected = slurp($base_path);
if ($runner_os eq 'Windows' || $os eq 'Windows_NT') {
  $expected =~ s/\*MINGW\*\)/\*MINGW\*|\*MSYS\*)/g;
  $expected =~ s/GIT_TEST_CMP="GIT_DIR=\/dev\/null git diff --no-index --ignore-cr-at-eol --"/GIT_TEST_CMP="diff -u"/g;
  $expected =~ s/GIT_TEST_CMP="\$DIFF -u"/GIT_TEST_CMP="diff -u"/g;
  $expected =~ s/GIT_TEST_CMP=" +-u"/GIT_TEST_CMP="diff -u"/g;
}
my $injection = "\tif test -n \"\$ZMIN_UPSTREAM_STOP_AFTER_TEST\" &&\n" .
  "\t   test \"\$test_count\" -ge \"\$ZMIN_UPSTREAM_STOP_AFTER_TEST\"\n" .
  "\tthen\n\t\ttest_done\n\tfi\n";
my $needle = "test_finish_ () {\n";
my $count = ($expected =~ s/\Q$needle\E/$needle . $injection/e);
die "unexpected test_finish_ definition count\n" unless $count == 1;
die "prepared t/test-lib.sh is not the exact pinned patch\n"
  unless slurp($prepared_path) eq $expected;
PERL
}

write_perl_module_manifest() {
  local root="$1"
  local perl_source generated_source relative target source source_sha target_sha
  local source_count=0 generated_count=0
  [[ -d "$root/perl/build/lib" && ! -L "$root/perl/build" &&
    ! -L "$root/perl/build/lib" ]] || return 1
  while IFS= read -r perl_source; do
    [[ -z "$perl_source" ]] && continue
    return 1
  done < <(
    find "$root/perl" -path "$root/perl/build" -prune -o \
      -type l -name '*.pm' -print | LC_ALL=C sort
  )
  while IFS= read -r perl_source; do
    [[ -n "$perl_source" ]] || continue
    [[ ! -L "$perl_source" && -f "$perl_source" ]] || return 1
    relative="${perl_source#"$root/perl/"}"
    [[ "$relative" != *$'\t'* && "$relative" != *$'\r'* &&
      "$relative" != *$'\n'* ]] || return 1
    target="$root/perl/build/lib/$relative"
    [[ ! -L "$target" && -f "$target" ]] || return 1
    source_sha="$(shasum -a 256 "$perl_source" | awk '{ print $1 }')" || return 1
    printf 'S\tperl/%s\t%s\n' "$relative" "$source_sha"
    source_count=$((source_count + 1))
  done < <(
    find "$root/perl" -path "$root/perl/build" -prune -o -type f -name '*.pm' -print |
      LC_ALL=C sort
  )
  while IFS= read -r generated_source; do
    [[ -z "$generated_source" ]] && continue
    return 1
  done < <(find "$root/perl/build/lib" -type l -print | LC_ALL=C sort)
  while IFS= read -r generated_source; do
    [[ -n "$generated_source" ]] || continue
    relative="${generated_source#"$root/perl/build/lib/"}"
    [[ "$relative" != "$generated_source" && "$relative" != *$'\t'* &&
      "$relative" != *$'\r'* && "$relative" != *$'\n'* ]] || return 1
    source="$root/perl/$relative"
    [[ ! -L "$source" && -f "$source" ]] || return 1
    target_sha="$(shasum -a 256 "$generated_source" | awk '{ print $1 }')" || return 1
    printf 'G\tperl/build/lib/%s\t%s\n' "$relative" "$target_sha"
    generated_count=$((generated_count + 1))
  done < <(find "$root/perl/build/lib" -type f -name '*.pm' -print | LC_ALL=C sort)
  [[ "$source_count" -gt 0 && "$generated_count" == "$source_count" ]]
}

perl_modules_manifest_sha256() {
  local root="$1"
  local manifest digest
  manifest="$(mktemp "$cache_root/.zmin-perl-module-manifest.XXXXXX")" || return 1
  if ! write_perl_module_manifest "$root" >"$manifest"; then
    rm -f "$manifest"
    return 1
  fi
  digest="$(shasum -a 256 "$manifest" | awk '{ print $1 }')" || {
    rm -f "$manifest"
    return 1
  }
  rm -f "$manifest"
  printf '%s\n' "$digest"
}

classify_executable_path() {
  local candidate="$1"
  "$perl_bin" -e '
    use strict;
    use warnings;
    use Errno qw(ENOENT);
    my @stat = lstat($ARGV[0]);
    exit($!{ENOENT} ? 1 : 2) unless @stat;
    exit 2 if -l _ || !-f _ || !-x _;
    exit 0;
  ' "$candidate"
}

platform_executable_path() {
  local candidate="$1"
  local fallback="$2"
  local state
  if [[ "${RUNNER_OS:-}" == "Windows" || "${OS:-}" == "Windows_NT" ]]; then
    if classify_executable_path "$candidate"; then
      printf '%s\n' "$candidate"
      return 0
    else
      state=$?
    fi
    if [[ "$state" == "1" ]]; then
      if classify_executable_path "$fallback"; then
        printf '%s\n' "$fallback"
        return 0
      fi
      echo "Windows fallback executable is not a valid regular executable: $fallback" >&2
    else
      echo "Windows .exe artifact is not a valid regular executable: $candidate" >&2
    fi
    return 1
  fi
  if classify_executable_path "$fallback"; then
    printf '%s\n' "$fallback"
    return 0
  fi
  echo "Unix executable artifact is not a valid regular executable: $fallback" >&2
  return 1
}

test_tool_real_path() {
  if [[ "${RUNNER_OS:-}" == "Windows" || "${OS:-}" == "Windows_NT" ]]; then
    platform_executable_path "$source_dir/t/helper/test-tool.exe" \
      "$source_dir/t/helper/test-tool"
  else
    platform_executable_path "$source_dir/t/helper/test-tool-real" \
      "$source_dir/t/helper/test-tool-real"
  fi
}

test_tool_exec_path() {
  if [[ "${RUNNER_OS:-}" == "Windows" || "${OS:-}" == "Windows_NT" ]]; then
    platform_executable_path "$source_dir/t/helper/test-tool.exe" \
      "$source_dir/t/helper/test-tool"
  else
    platform_executable_path "$source_dir/t/helper/test-tool" \
      "$source_dir/t/helper/test-tool"
  fi
}

test_tool_provenance_path() {
  printf '%s\n' "$source_dir/.zmin-test-tool.provenance.tsv"
}

platform_helper_path() {
  if [[ "${RUNNER_OS:-}" == "Windows" || "${OS:-}" == "Windows_NT" ]]; then
    platform_executable_path "$source_dir/git-sh-i18n--envsubst.exe" \
      "$source_dir/git-sh-i18n--envsubst"
  else
    platform_executable_path "$source_dir/git-sh-i18n--envsubst" \
      "$source_dir/git-sh-i18n--envsubst"
  fi
}

helper_provenance_payload() {
  local helper="$1"
  local executed_helper platform_helper source_sha pristine_sha perl_modules_sha
  local perl_modules_manifest module_entry
  local reftable_metadata
  local helper_sha executed_helper_sha options_sha platform_helper_sha
  local test_lib_base_sha test_lib_prepared_sha
  validate_make_identity || return 1
  executed_helper="$(test_tool_exec_path)"
  platform_helper="$(platform_helper_path)"
  [[ -x "$helper" && ! -L "$helper" ]] || return 1
  [[ -x "$executed_helper" && ! -L "$executed_helper" ]] || return 1
  [[ -x "$platform_helper" && ! -L "$platform_helper" ]] || return 1
  source_sha="$(source_manifest_sha256 "$source_dir" "$prepared_artifacts_manifest")" || return 1
  pristine_sha="$(source_manifest_sha256 "$pristine_source_dir")" || return 1
  test_lib_base_sha="$(shasum -a 256 "$pristine_source_dir/t/test-lib.sh" | awk '{ print $1 }')" || return 1
  test_lib_prepared_sha="$(shasum -a 256 "$source_dir/t/test-lib.sh" | awk '{ print $1 }')" || return 1
  perl_modules_manifest="$(write_perl_module_manifest "$source_dir")" || return 1
  perl_modules_sha="$(printf '%s\n' "$perl_modules_manifest" | shasum -a 256 | awk '{ print $1 }')" || return 1
  helper_sha="$(shasum -a 256 "$helper" | awk '{ print $1 }')" || return 1
  executed_helper_sha="$(shasum -a 256 "$executed_helper" | awk '{ print $1 }')" || return 1
  options_sha="$(shasum -a 256 "$source_dir/GIT-BUILD-OPTIONS" | awk '{ print $1 }')" || return 1
  platform_helper_sha="$(shasum -a 256 "$platform_helper" | awk '{ print $1 }')" || return 1
  validate_reftable_artifact_metadata || return 1
  if [[ "$skip_unsupported_reftable" == "1" ]]; then
    reftable_metadata="$(grep '^R[[:space:]]' "$prepared_artifacts_manifest")" || return 1
  else
    reftable_metadata="R\tNONE\tNONE\tNONE\tNONE"
  fi
  {
    printf 'format_version\t1\n'
    printf 'helper_path\t%s\n' "$helper"
    printf 'helper_sha256\t%s\n' "$helper_sha"
    printf 'executed_helper_path\t%s\n' "$executed_helper"
    printf 'executed_helper_sha256\t%s\n' "$executed_helper_sha"
    printf 'platform_helper_path\t%s\n' "$platform_helper"
    printf 'platform_helper_sha256\t%s\n' "$platform_helper_sha"
    printf 'source_manifest_sha256\t%s\n' "$source_sha"
    printf 'pristine_source_manifest_sha256\t%s\n' "$pristine_sha"
    printf 'prepared_artifacts_manifest_path\t%s\n' "$prepared_artifacts_manifest"
    printf 'prepared_artifacts_manifest_sha256\t%s\n' "$prepared_artifacts_sha256"
    printf 'test_lib_base_sha256\t%s\n' "$test_lib_base_sha"
    printf 'test_lib_prepared_sha256\t%s\n' "$test_lib_prepared_sha"
    printf 'perl_modules_manifest_sha256\t%s\n' "$perl_modules_sha"
    while IFS= read -r module_entry; do
      [[ -n "$module_entry" ]] || continue
      printf 'perl_module_manifest_entry\t%s\n' "$module_entry"
    done <<<"$perl_modules_manifest"
    printf 'build_options_sha256\t%s\n' "$options_sha"
    printf 'upstream_tag\t%s\n' "$tag"
    printf 'upstream_archive_sha256\t%s\n' "$verified_archive_sha"
    printf 'upstream_commit_binding\t%s\n' "$archive_commit_binding"
    printf 'prepared_reftable_mode\t%s\n' "$skip_unsupported_reftable"
    printf 'prepared_reftable_entry\t%s\n' "$reftable_metadata"
    printf 'contract_make_path\t%s\n' "$make_bin"
    printf 'contract_make_version\t%s\n' "$make_version"
    printf 'contract_make_sha256\t%s\n' "$make_sha256"
    printf 'perl_path\t%s\n' "$perl_bin"
    printf 'perl_version\t%s\n' "$perl_version"
  }
}

helper_provenance_matches() {
  local helper="$(test_tool_real_path)"
  local executed_helper="$(test_tool_exec_path)"
  local provenance="$(test_tool_provenance_path)"
  [[ -x "$helper" && ! -L "$helper" && -x "$executed_helper" &&
    ! -L "$executed_helper" && -f "$provenance" && ! -L "$provenance" ]] || return 1
  [[ "$(cat "$provenance")" == "$(helper_provenance_payload "$helper")" ]]
}

remove_generated_perl_modules() {
  local root="$1"
  [[ "$prepare_lock_held" == "1" ]] || return 1
  [[ ! -L "$root/perl/build/lib" ]] || return 1
  [[ ! -e "$root/perl/build/lib" || -d "$root/perl/build/lib" ]] || return 1
  # Generated modules are retained and rebuilt under the authenticated Make
  # invocation.  The prepared-artifact manifest hashes every resulting module;
  # no deletion is needed to invalidate a stale provenance record.
  return 0
}

descriptor_relative_cleanup() {
  local operation="$1"
  shift
  if [[ "$manifest_fixture_mode" != "1" &&
    -n "${ZMIN_UPSTREAM_MANIFEST_FIXTURE_RACE_BARRIER:-}" ]]; then
    echo "manifest fixture race hook is unavailable outside fixtures" >&2
    return 1
  fi
  [[ "$manifest_fixture_mode" == "1" || "$prepared_cleanup_python_trust" == "1" ]] || {
    echo "descriptor-relative cleanup requires sidecar-authenticated Python" >&2
    return 1
  }
  local canonical
  [[ -n "$contract_python_bin" && "$contract_python_bin" == /* &&
    -x "$contract_python_bin" && ! -L "$contract_python_bin" ]] || {
    echo "descriptor-relative cleanup requires trusted contract Python" >&2
    return 1
  }
  canonical="$(canonical_path "$contract_python_bin")" || return 1
  [[ "$canonical" == "$contract_python_bin" ]] || return 1
  "$contract_python_bin" - "$operation" "$@" <<'PY'
import errno
import os
import stat
import sys
import time

operation = sys.argv[1]
root_path = sys.argv[2]
arguments = sys.argv[3:]
directory_flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW
maximum_nodes = 65536
opened_fds = []


def fail(message):
    raise RuntimeError(message)


def identity(value):
    return (value.st_dev, value.st_ino, stat.S_IFMT(value.st_mode))


def remember(fd):
    opened_fds.append(fd)
    return fd


def close_all():
    for fd in reversed(opened_fds):
        try:
            os.close(fd)
        except OSError:
            pass


def open_absolute_directory(path):
    if not path.startswith("/"):
        fail("cleanup root is not absolute")
    fd = remember(os.open("/", directory_flags))
    for component in path.split("/"):
        if component in ("", "."):
            continue
        if component == "..":
            fail("cleanup root contains parent traversal")
        child = remember(os.open(component, directory_flags, dir_fd=fd))
        fd = child
    return fd


def lstat_at(parent_fd, name):
    try:
        value = os.stat(name, dir_fd=parent_fd, follow_symlinks=False)
    except FileNotFoundError:
        raise
    except OSError as error:
        fail("cannot inspect %r: %s" % (name, error))
    if stat.S_ISLNK(value.st_mode):
        fail("symlink in generated cleanup tree: %r" % (name,))
    return value


def open_child_directory(parent_fd, name):
    before = lstat_at(parent_fd, name)
    if not stat.S_ISDIR(before.st_mode):
        fail("generated cleanup path is not a directory: %r" % (name,))
    try:
        child_fd = remember(os.open(name, directory_flags, dir_fd=parent_fd))
    except OSError as error:
        fail("cannot open generated cleanup directory %r: %s" % (name, error))
    after = os.fstat(child_fd)
    if identity(before) != identity(after):
        fail("generated cleanup directory changed during open: %r" % (name,))
    return child_fd, after


def list_names(directory_fd):
    try:
        names = os.listdir(directory_fd)
    except OSError as error:
        fail("cannot enumerate generated cleanup directory: %s" % (error,))
    if len(names) > maximum_nodes:
        fail("generated cleanup directory is too large")
    for name in sorted(names):
        if not name or "/" in name or name in (".", ".."):
            fail("invalid generated cleanup entry")
        yield name


def snapshot_tree(directory_fd, components, nodes, files):
    if len(nodes) + len(files) > maximum_nodes:
        fail("generated cleanup tree is too large")
    parent_stat = os.fstat(directory_fd)
    for name in list_names(directory_fd):
        value = lstat_at(directory_fd, name)
        if stat.S_ISDIR(value.st_mode):
            child_fd, child_stat = open_child_directory(directory_fd, name)
            node = {
                "parent": directory_fd,
                "parent_identity": identity(parent_stat),
                "name": name,
                "identity": identity(child_stat),
                "components": components + [name],
                "fd": child_fd,
            }
            nodes.append(node)
            snapshot_tree(child_fd, node["components"], nodes, files)
        elif stat.S_ISREG(value.st_mode):
            files.append({
                "parent": directory_fd,
                "parent_identity": identity(parent_stat),
                "name": name,
                "identity": identity(value),
                "components": components + [name],
            })
        else:
            fail("unsupported generated cleanup entry type: %r" % (name,))


def source_directory_kind(source_fd, components):
    current_fd = remember(os.dup(source_fd))
    for component in components:
        try:
            value = os.stat(component, dir_fd=current_fd, follow_symlinks=False)
        except FileNotFoundError:
            return "missing"
        except OSError as error:
            fail("cannot inspect source-backed Perl directory: %s" % (error,))
        if stat.S_ISLNK(value.st_mode):
            fail("source-backed Perl directory is a symlink")
        if not stat.S_ISDIR(value.st_mode):
            return "other"
        child_fd, child_stat = open_child_directory(current_fd, component)
        if identity(value) != identity(child_stat):
            fail("source-backed Perl directory changed during open")
        current_fd = child_fd
    return "directory"


def preserve_generated_cleanup_candidate(description):
    fail(
        "safe descriptor-bound deletion is unavailable; preserving generated artifact: %s"
        % description
    )


def open_relative_directory(root_fd, components):
    current_fd = root_fd
    for component in components:
        if component in ("", ".", "..") or "/" in component:
            fail("invalid cleanup path component")
        try:
            value = os.stat(component, dir_fd=current_fd, follow_symlinks=False)
        except FileNotFoundError:
            return None
        except OSError as error:
            fail("cannot inspect cleanup path: %s" % (error,))
        if stat.S_ISLNK(value.st_mode):
            fail("cleanup path contains a symlink")
        if not stat.S_ISDIR(value.st_mode):
            fail("cleanup path is not a directory")
        child_fd, child_stat = open_child_directory(current_fd, component)
        if identity(value) != identity(child_stat):
            fail("cleanup path changed during open")
        current_fd = child_fd
    return current_fd


def remove_trees(root_fd, relative_paths):
    normalized = []
    seen = set()
    for relative in relative_paths:
        relative = relative.strip()
        if relative.startswith("./"):
            relative = relative[2:]
        components = relative.split("/") if relative else []
        if not components or any(
            component in ("", ".", "..") or "/" in component
            for component in components
        ):
            fail("invalid evaluated cleanup path")
        if relative != "po/build" and not relative.endswith("/.depend") and relative != ".depend":
            fail("unevaluated generated cleanup path")
        if relative in seen:
            fail("duplicate evaluated cleanup path")
        seen.add(relative)
        normalized.append((relative, components))
    for relative, components in normalized:
        parent_components = components[:-1]
        parent_fd = open_relative_directory(root_fd, parent_components)
        if parent_fd is None:
            continue
        try:
            os.stat(components[-1], dir_fd=parent_fd, follow_symlinks=False)
        except FileNotFoundError:
            continue
        except OSError as error:
            fail("cannot inspect evaluated cleanup path: %s" % (error,))
        value = lstat_at(parent_fd, components[-1])
        if not stat.S_ISDIR(value.st_mode):
            fail("evaluated cleanup path is not a directory: %s" % relative)
        tree_fd, tree_stat = open_child_directory(parent_fd, components[-1])
        nodes = []
        files = []
        snapshot_tree(tree_fd, components, nodes, files)
        preserve_generated_cleanup_candidate(relative)


def remove_files(root_fd, relative_paths):
    seen = set()
    for relative in relative_paths:
        components = relative.split("/")
        if relative in seen or not components or any(
            component in ("", ".", "..") or "\x00" in component
            for component in components
        ):
            fail("invalid evaluated file cleanup path")
        seen.add(relative)
        parent_fd = open_relative_directory(root_fd, components[:-1])
        if parent_fd is None:
            continue
        name = components[-1]
        try:
            value = lstat_at(parent_fd, name)
        except FileNotFoundError:
            continue
        if stat.S_ISLNK(value.st_mode) or not stat.S_ISREG(value.st_mode):
            fail("evaluated file cleanup path is not a regular file: %s" % relative)
        preserve_generated_cleanup_candidate(relative)


def remove_test_outputs(root_fd):
    test_fd = open_relative_directory(root_fd, ["t"])
    if test_fd is None:
        return
    for name in list_names(test_fd):
        if name != "test-results" and not name.startswith("trash directory."):
            continue
        value = lstat_at(test_fd, name)
        if not stat.S_ISDIR(value.st_mode):
            fail("stale upstream test output is not a directory: %s" % name)
        child_fd, child_stat = open_child_directory(test_fd, name)
        nodes = []
        files = []
        snapshot_tree(child_fd, ["t", name], nodes, files)
        preserve_generated_cleanup_candidate("t/%s" % name)


def prune_perl_directories(root_fd):
    perl_fd = open_relative_directory(root_fd, ["perl"])
    if perl_fd is None:
        fail("prepared Perl source directory is missing")
    build_fd = open_relative_directory(perl_fd, ["build"])
    if build_fd is None:
        fail("prepared Perl build directory is missing")
    lib_fd = open_relative_directory(build_fd, ["lib"])
    if lib_fd is None:
        fail("prepared Perl generated library is missing")
    nodes = []
    files = []
    snapshot_tree(lib_fd, [], nodes, files)
    source_fd = remember(os.dup(perl_fd))
    source_kind = {}
    for node in nodes:
        kind = source_directory_kind(source_fd, node["components"])
        if kind == "other":
            fail("generated Perl directory collides with source file")
        source_kind[tuple(node["components"])] = kind
    for entry in files:
        if source_directory_kind(source_fd, entry["components"]) == "directory":
            fail("generated Perl file collides with source directory")
    files_by_directory = []
    for entry in files:
        files_by_directory.append(entry["components"])
    for node in nodes:
        components = node["components"]
        if source_kind[tuple(components)] != "missing":
            continue
        if any(
            file_components[:len(components)] == components
            for file_components in files_by_directory
        ):
            fail("stale generated Perl directory is nonempty")
        if any(
            child["components"][:len(components)] == components and
            len(child["components"]) > len(components) and
            source_kind[tuple(child["components"])] != "missing"
            for child in nodes
        ):
            fail("stale generated Perl directory has source-backed child")
    race_barrier = os.environ.get("ZMIN_UPSTREAM_MANIFEST_FIXTURE_RACE_BARRIER")
    if race_barrier:
        ready = race_barrier + ".ready"
        go = race_barrier + ".go"
        try:
            fd = os.open(ready, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
            os.close(fd)
        except OSError as error:
            fail("cannot publish descriptor cleanup race barrier: %s" % (error,))
        for _ in range(1000):
            if os.path.exists(go):
                break
            time.sleep(0.01)
        else:
            fail("descriptor cleanup race barrier timed out")
    stale_nodes = [
        node for node in nodes
        if source_kind[tuple(node["components"])] == "missing"
    ]
    if stale_nodes:
        for node in stale_nodes:
            current = lstat_at(node["parent"], node["name"])
            if (
                identity(current) != node["identity"]
                or identity(os.fstat(node["fd"])) != node["identity"]
            ):
                fail("generated cleanup entry changed before safe preservation")
        preserve_generated_cleanup_candidate(
            "perl/build/lib generated directories"
        )


try:
    root_fd = open_absolute_directory(root_path)
    root_identity = identity(os.fstat(root_fd))
    if operation == "remove-trees":
        graph = arguments[0].splitlines() if arguments else []
        remove_trees(root_fd, graph + arguments[1:])
    elif operation == "remove-files":
        remove_files(root_fd, arguments)
    elif operation == "remove-test-outputs":
        remove_test_outputs(root_fd)
    elif operation == "prune-perl":
        prune_perl_directories(root_fd)
    else:
        fail("unknown descriptor-relative cleanup operation")
    if identity(os.fstat(root_fd)) != root_identity:
        fail("cleanup root identity changed")
except (OSError, RuntimeError, ValueError) as error:
    print("descriptor-relative cleanup failed: %s" % error, file=sys.stderr)
    sys.exit(1)
finally:
    close_all()
PY
}

evaluate_prepared_dep_dirs() {
  local pristine="$1"
  local graph make_database
  [[ -d "$pristine" && ! -L "$pristine" ]] || return 1
  validate_make_identity || return 1
  make_database="$(mktemp "$cache_root/.zmin-make-database.XXXXXX")" || return 1
  if ! (
    cd -P "$pristine" || exit 1
    run_pinned_make -pn NO_GETTEXT=1 COMPUTE_HEADER_DEPENDENCIES=yes >"$make_database"
  ); then
    rm -f -- "$make_database"
    return 1
  fi
  if ! graph="$(
      "$perl_bin" -ne '
        chomp;
        s/\r$//;
        if (!$continuing) {
          next unless /^dep_dirs\s*:=\s*(.*)$/;
          $value = $1;
        } else {
          die "invalid evaluated Makefile continuation\n" unless /^\s*(.*)$/;
          $value .= " $1";
        }
        if ($value =~ s/\\\s*$//) { $continuing = 1; next; }
        push @records, $value;
        $continuing = 0;
        END {
          die "evaluated Makefile dep_dirs has an unfinished continuation\n"
            if $continuing;
          die "evaluated Makefile dep_dirs missing or duplicated\n" unless @records == 1;
          my %seen;
          my @tokens;
          my $token = "";
          my $escaped = 0;
          for my $character (split //, $records[0]) {
            if ($escaped) {
              die "invalid escaped Makefile dep_dirs byte\n" unless $character eq " ";
              $token .= $character;
              $escaped = 0;
            } elsif ($character eq "\\") {
              $escaped = 1;
            } elsif ($character =~ /\s/) {
              push @tokens, $token if length $token;
              $token = "";
            } else {
              $token .= $character;
            }
          }
          die "invalid escaped Makefile dep_dirs byte\n" if $escaped;
          push @tokens, $token if length $token;
          die "evaluated Makefile dep_dirs is empty\n" unless @tokens;
          for my $relative (@tokens) {
            $relative =~ s{^\./}{};
            my @components = split m{/}, $relative, -1;
            die "invalid evaluated Makefile dep_dirs entry\n"
              unless @components && $components[-1] eq ".depend" &&
                !grep { !length || $_ eq "." || $_ eq ".." ||
                  /[\x00-\x1f\x7f\$;|&`"'"'"'<>()[\]{}:]/ } @components;
            die "duplicate evaluated Makefile dep_dirs entry\n" if $seen{$relative}++;
            print "$relative\n";
          }
        }
      ' <"$make_database"
  )"; then
    rm -f -- "$make_database"
    return 1
  fi
  rm -f -- "$make_database"
  validate_make_identity || return 1
  [[ -n "$graph" ]] || return 1
  printf '%s\n' "$graph"
}

remove_stale_generated_perl_directories() {
  local root="$1"
  [[ "$prepare_lock_held" == "1" ]] || return 1
  descriptor_relative_cleanup prune-perl "$root"
}

remove_unneeded_prepared_outputs() {
  local root="$1"
  local pristine="$2"
  [[ "$prepare_lock_held" == "1" ]] || return 1
  if [[ -z "$prepared_generated_graph" ]]; then
    prepared_generated_graph="$(evaluate_prepared_dep_dirs "$pristine")" || return 1
    prepared_generated_graph="$(printf '%s\n%s\n' "$prepared_generated_graph" po/build)"
  fi
  # Keep the exact evaluated generated directories.  Their regular files are
  # authenticated and hashed by write_prepared_artifacts_manifest; unexpected
  # generated paths still fail manifest validation.
  return 0
}

refresh_contract_scope() {
  local source_ok=0
  local provenance_ok=0
  if source_matches_pristine; then source_ok=1; fi
  if helper_provenance_matches; then provenance_ok=1; fi
  contract_scope="exploratory"
  if [[ "$mode" == "all-nondeprecated" && "$tag" == "$contract_tag" && -z "$test_list" &&
    "$manifest_offset" == "0" && "$manifest_limit" == "0" && "$stock_git_control" == "0" &&
    "$bounded_run" == "0" && "$allow_failures" == "0" &&
    "$skip_unsupported_reftable" == "0" && "$custom_test_flags" == "0" &&
    "$cargo_profile" == "release" && "$test_timeout" == "0" &&
    "$zmin_current_contract_trust" == "1" && "$source_ok" == "1" &&
    "$provenance_ok" == "1" ]]; then
    contract_scope="current-contract"
  elif [[ "$mode" == "all-nondeprecated" ]]; then
    contract_scope="exploratory-bounded"
  fi
}

publish_descriptor_file() {
  local provenance="$1"
  local payload="$2"
  [[ -n "$contract_python_bin" && -x "$contract_python_bin" && ! -L "$contract_python_bin" ]] || {
    echo "descriptor publication requires the trusted contract Python" >&2
    return 1
  }
  printf '%s\n' "$payload" | "$contract_python_bin" \
    "$repo_root/tools/performance_contract.py" publish-descriptor --path "$provenance"
}

publish_helper_provenance() {
  local provenance="$1"
  local payload="$2"
  publish_descriptor_file "$provenance" "$payload"
}

write_helper_provenance() {
  local helper="$(test_tool_real_path)"
  local provenance="$(test_tool_provenance_path)"
  local payload
  [[ -x "$helper" && ! -L "$helper" ]] || return 1
  [[ ! -e "$provenance" || -f "$provenance" ]] || return 1
  [[ ! -L "$provenance" ]] || return 1
  payload="$(helper_provenance_payload "$helper")" || {
    return 1
  }
  if ! publish_helper_provenance "$provenance" "$payload"; then
    return 1
  fi
}

source_matches_pristine() {
  local source_sha pristine_sha
  validate_prepared_artifacts_manifest || return 1
  validate_test_lib_patch || return 1
  source_sha="$(source_manifest_sha256 "$source_dir" "$prepared_artifacts_manifest")" || return 1
  pristine_sha="$(source_manifest_sha256 "$pristine_source_dir")" || return 1
  [[ "$source_sha" == "$pristine_sha" ]]
}

remove_stale_test_outputs() {
  [[ -d "$source_dir/t" && ! -L "$source_dir/t" ]] || return 0
  descriptor_relative_cleanup remove-test-outputs "$source_dir"
}

verify_archive_source() {
  local archive="$1"
  local verify_dir
  local archive_sha pristine_sha
  verify_dir="$(mktemp -d "$cache_root/.zmin-archive-verify.XXXXXX")" || return 1
  if ! tar -xzf "$archive" -C "$verify_dir" --strip-components=1; then
    rm -rf "$verify_dir"
    return 1
  fi
  archive_sha="$(source_manifest_sha256 "$verify_dir")" || {
    rm -rf "$verify_dir"
    return 1
  }
  pristine_sha="$(source_manifest_sha256 "$pristine_source_dir")" || {
    rm -rf "$verify_dir"
    return 1
  }
  rm -rf "$verify_dir"
  [[ "$archive_sha" == "$pristine_sha" ]]
}

validate_archive_identity() {
  local archive="$1"
  local archive_sha def_ver
  [[ ! -L "$archive" && -s "$archive" ]] || {
    echo "pinned upstream archive is missing or symlinked: $archive" >&2
    return 1
  }
  archive_sha="$(shasum -a 256 "$archive" | awk '{ print $1 }')"
  if [[ "$tag" == "$contract_tag" && "$archive_sha" != "$contract_archive_sha" ]]; then
    echo "upstream archive SHA-256 mismatch for contract tag $tag" >&2
    echo "expected: $contract_archive_sha" >&2
    echo "actual:   $archive_sha" >&2
    return 1
  fi
  def_ver="$(tar -xOzf "$archive" "git-${tag#v}/GIT-VERSION-GEN" | sed -n 's/^DEF_VER=//p')"
  [[ "$def_ver" == "$tag" ]] || {
    echo "pinned archive generated-version binding mismatch: $def_ver" >&2
    return 1
  }
  verified_archive_sha="$archive_sha"
  archive_commit_binding="archive_sha256+tag+DEF_VER;commit_not_embedded_in_archive"
  if [[ "$tag" == "$contract_tag" ]]; then
    reported_upstream_commit="$contract_commit"
  fi
}

acquire_cache_lock() {
  acquire_owned_lock "$cache_lock" "upstream cache lock" "$cache_lock_fd" || return 1
  cache_lock_held=1
}

release_cache_lock() {
  if [[ "$cache_lock_held" == "1" ]]; then
    release_owned_lock "upstream cache lock" "$cache_lock_fd" || {
      cache_lock_held=0
      return 1
    }
    cache_lock_held=0
  fi
}

release_prepare_lock() {
  if [[ "$prepare_lock_held" == "1" ]]; then
    release_owned_lock "upstream preparation lock" "$prepare_lock_fd" || {
      prepare_lock_held=0
      return 1
    }
    prepare_lock_held=0
  fi
}

download_upstream() {
  local archive="$cache_root/$tag.tar.gz"
  local archive_sha marker pristine_valid=0
  acquire_cache_lock || return 1
  [[ ! -L "$archive" ]] || {
    echo "pinned upstream archive is a symlink: $archive" >&2
    return 1
  }
  if [[ ! -s "$archive" ]] || ! tar -tzf "$archive" >/dev/null 2>&1; then
    cache_temp_path="$(mktemp "$cache_root/.zmin-upstream-archive.XXXXXX")" || return 1
    curl -fsSL "https://github.com/git/git/archive/refs/tags/$tag.tar.gz" -o "$cache_temp_path" || return 1
    tar -tzf "$cache_temp_path" >/dev/null || return 1
    mv -f "$cache_temp_path" "$archive" || return 1
    cache_temp_path=""
  fi
  archive_sha="$(shasum -a 256 "$archive" | awk '{ print $1 }')"
  validate_archive_identity "$archive" || return 1
  require_cache_directory "$pristine_source_dir" pristine_source_dir || return 1
  require_cache_directory "$source_dir" source_dir || return 1
  marker="$pristine_source_dir/.zmin-pristine-source.sha256"
  if [[ -d "$pristine_source_dir" && ! -L "$pristine_source_dir" &&
    -f "$marker" && ! -L "$marker" ]] &&
    [[ "$(cat "$marker")" == "$archive_sha" ]]
  then
    pristine_valid=1
  fi
  if [[ "$pristine_valid" != "1" ]]; then
    if [[ -e "$pristine_source_dir" || -L "$pristine_source_dir" ]]; then
      [[ ! -L "$pristine_source_dir" ]] || {
        echo "cannot replace symlinked pristine source: $pristine_source_dir" >&2
        return 1
      }
      rm -rf "$pristine_source_dir" || return 1
    fi
    pristine_temp_path="$(mktemp -d "$cache_root/.zmin-pristine-source.XXXXXX")" || return 1
    tar -xzf "$archive" -C "$pristine_temp_path" --strip-components=1 || return 1
    printf '%s\n' "$archive_sha" >"$pristine_temp_path/.zmin-pristine-source.sha256" || return 1
    chmod -R a-w "$pristine_temp_path" || return 1
    mv -f "$pristine_temp_path" "$pristine_source_dir" || return 1
    pristine_temp_path=""
  fi
  verify_archive_source "$archive" || {
    echo "pinned pristine source does not match the verified archive" >&2
    return 1
  }
  remove_stale_test_outputs || return 1
  if [[ -d "$source_dir" ]] && ! source_matches_pristine; then
    echo "prepared upstream source changed; rebuilding exact cache leaf: $source_dir" >&2
    [[ ! -L "$source_dir" ]] || {
      echo "cannot replace symlinked prepared source: $source_dir" >&2
      return 1
    }
    rm -rf "$source_dir" || return 1
  fi
  if [[ ! -e "$source_dir" ]]; then
    source_temp_path="$(mktemp -d "$cache_root/.zmin-prepared-source.XXXXXX")" || return 1
    cp -R "$pristine_source_dir/." "$source_temp_path/" || return 1
    chmod -R u+w "$source_temp_path" || return 1
    mv -f "$source_temp_path" "$source_dir" || return 1
    source_temp_path=""
  fi
}

transform_t5510_fetch() {
  local path="$1"
  "$perl_bin" -0pi -e '
    s/\Q &&
	git clone --ref-format=reftable . case_sensitive &&
	(
		cd case_sensitive &&
		git branch branch1 &&
		git branch bRanch1
	) &&
	git clone --ref-format=reftable . case_sensitive_fd &&
	(
		cd case_sensitive_fd &&
		git branch foo\/bar &&
		git branch Foo
	) &&
	git clone --ref-format=reftable . case_sensitive_df &&
	(
		cd case_sensitive_df &&
		git branch Foo\/bar &&
		git branch foo
	)\E//;
    s/test_expect_success CASE_INSENSITIVE_FS,REFFILES /test_expect_success REFTABLE,CASE_INSENSITIVE_FS,REFFILES /g;
    s/test_expect_success REFFILES '\''existing reference lock/test_expect_success REFTABLE,REFFILES '\''existing reference lock/g;
    s/test_expect_success REFFILES '\''D\/F conflict on case sensitive filesystem with lock/test_expect_success REFTABLE,REFFILES '\''D\/F conflict on case sensitive filesystem with lock/g;
  ' "$path"
}

prepare_upstream_harness() {
  acquire_owned_lock "$prepare_lock" "upstream preparation lock" "$prepare_lock_fd" || return 1
  prepare_lock_held=1
  trap 'release_prepare_lock; release_cache_lock; if [[ -n "$cache_temp_path" && ! -L "$cache_temp_path" ]]; then rm -f "$cache_temp_path"; fi; if [[ -n "$pristine_temp_path" && ! -L "$pristine_temp_path" ]]; then rm -rf "$pristine_temp_path"; fi; if [[ -n "$source_temp_path" && ! -L "$source_temp_path" ]]; then rm -rf "$source_temp_path"; fi' EXIT
  download_upstream
  (
    cd "$source_dir"
    if [[ "${RUNNER_OS:-}" == "Windows" || "${OS:-}" == "Windows_NT" ]]; then
      "$perl_bin" -0pi -e 's/\*MINGW\*\)/\*MINGW\*|\*MSYS\*\)/g' t/test-lib.sh
      "$perl_bin" -0pi -e 's/GIT_TEST_CMP="GIT_DIR=\/dev\/null git diff --no-index --ignore-cr-at-eol --"/GIT_TEST_CMP="diff -u"/g' t/test-lib.sh
      "$perl_bin" -0pi -e 's/GIT_TEST_CMP="\$DIFF -u"/GIT_TEST_CMP="diff -u"/g' t/test-lib.sh
      "$perl_bin" -0pi -e 's/GIT_TEST_CMP=" +-u"/GIT_TEST_CMP="diff -u"/g' t/test-lib.sh
    fi
    validate_make_identity || exit 1
    if [[ -n "$make_bin" ]]; then
      local build_stock_git=0
      local need_prerequisites=0
      local perl_source perl_target
      local platform_helper="$(platform_helper_path)"
      local make_targets=(NO_GETTEXT=1 GIT-BUILD-OPTIONS t/helper/test-tool git-sh-i18n--envsubst)
      if [[ "$stock_git_control" == "1" && ! -x git && ! -x git.exe ]]; then
        build_stock_git=1
      fi
      local rebuild_prerequisites=0
      if [[ ! -f GIT-BUILD-OPTIONS || ! -x t/helper/test-tool ||
        ("${RUNNER_OS:-}" != "Windows" && "${OS:-}" != "Windows_NT" &&
         ! -x t/helper/test-tool-real) ||
        ! -x "$platform_helper" ||
        "$build_stock_git" == "1" ]]; then
        need_prerequisites=1
      fi
      if ! helper_provenance_matches; then
        need_prerequisites=1
        rebuild_prerequisites=1
        remove_generated_perl_modules "$source_dir" || {
          echo "generated Perl module tree is unsafe" >&2
          exit 1
        }
      fi
      while IFS= read -r perl_source; do
        perl_target="perl/build/lib/${perl_source#perl/}"
        make_targets+=("$perl_target")
        if [[ ! -f "$perl_target" ]]; then
          need_prerequisites=1
        fi
      done < <(find perl -path './perl/build' -prune -o -type f -name '*.pm' -print)
      if [[ "$stock_git_control" == "1" ]]; then
        make_targets+=(git git-http-backend git-sh-i18n git-sh-setup)
      fi
      if [[ "$need_prerequisites" == "1" ]]; then
        validate_make_identity || exit 1
        if [[ "$rebuild_prerequisites" == "1" ]]; then
          run_pinned_make -B -j"$jobs" "${make_targets[@]}"
        else
          run_pinned_make -j"$jobs" "${make_targets[@]}"
        fi
      fi
    fi
    if [[ ! -d templates/blt/hooks || ! -d templates/blt/info ]]; then
      validate_make_identity || exit 1
      run_pinned_make -C templates
    fi
    if [[ -L perl/build/lib ]]; then
      echo "unsafe symlinked generated Perl library" >&2
      exit 1
    fi
    if [[ ! -d perl/build/lib ]]; then
      mkdir -p perl/build/lib
    fi
    if [[ ! -f GIT-BUILD-OPTIONS ]]; then
      local shell_path perl_path x_suffix
      shell_path="$(command -v sh)"
      perl_path="$perl_bin"
      x_suffix=""
      if [[ "${RUNNER_OS:-}" == "Windows" || "${OS:-}" == "Windows_NT" ]]; then
        x_suffix=".exe"
      fi
      cat >GIT-BUILD-OPTIONS <<EOF
BROKEN_PATH_FIX='/^# @BROKEN_PATH_FIX@$/d'
DIFF='diff'
GIT_SOURCE_DIR='$source_dir'
GIT_TEST_CMP='diff -u'
GIT_TEST_CMP_USE_COPIED_CONTEXT=''
GIT_TEST_GITPERLLIB='$source_dir/perl/build/lib'
GIT_TEST_INDEX_VERSION=''
GIT_TEST_OPTS=''
GIT_TEST_PERL_FATAL_WARNINGS=''
GIT_TEST_TEMPLATE_DIR='$source_dir/templates/blt'
GIT_TEST_TEXTDOMAINDIR='$source_dir/po/build/locale'
GIT_TEST_UTF8_LOCALE=''
NO_CURL='1'
NO_EXPAT='1'
NO_GETTEXT='1'
NO_PERL=''
NO_PYTHON='1'
PERL_PATH='$perl_path'
SHELL_PATH='$shell_path'
TEST_OUTPUT_DIRECTORY=''
TEST_SHELL_PATH='$shell_path'
X='$x_suffix'
EOF
    fi
    if [[ -f GIT-BUILD-OPTIONS ]]; then
      "$perl_bin" -0pi -e "s/GIT_TEST_CMP='[^']*'/GIT_TEST_CMP='diff -u'/" GIT-BUILD-OPTIONS
    fi
    "$perl_bin" -0pi -e 's/\n\tif test -n "(?:\$ZMIN_UPSTREAM_STOP_AFTER_TEST)?" &&\n\t   test "(?:\$ZMIN_UPSTREAM_STOP_AFTER_TEST)?" -ge "(?:\$ZMIN_UPSTREAM_STOP_AFTER_TEST)?"\n\tthen\n\t\ttest_done\n\tfi\n/\n/' t/test-lib.sh
    if ! grep -q 'ZMIN_UPSTREAM_STOP_AFTER_TEST' t/test-lib.sh; then
      "$perl_bin" -0pi -e 's/test_finish_ \(\) \{\n/test_finish_ () {\n\tif test -n "\$ZMIN_UPSTREAM_STOP_AFTER_TEST" &&\n\t   test "\$test_count" -ge "\$ZMIN_UPSTREAM_STOP_AFTER_TEST"\n\tthen\n\t\ttest_done\n\tfi\n/' t/test-lib.sh
    fi
    if [[ ! -x t/helper/test-tool && ! -x t/helper/test-tool.exe ]] ||
      grep -qs "upstream test-tool helper is not available" t/helper/test-tool t/helper/test-tool.exe 2>/dev/null
    then
      echo "pinned upstream test-tool is missing or synthetic; refusing installed-binary fallback" >&2
      exit 1
    fi
    if false; then
      mkdir -p t/helper
      cat >t/helper/test-tool <<'EOF'
#!/usr/bin/env sh
if test "$1" = "path-utils" && test "$2" = "absolute_path"
then
  shift 2
  for path
  do
    case "$path" in
      /* | [A-Za-z]:*) printf '%s\n' "$path" ;;
      *) printf '%s/%s\n' "$(pwd -W 2>/dev/null || pwd)" "$path" ;;
    esac
  done
  exit 0
fi
if test "$1" = "path-utils" && test "$2" = "file-size"
then
  shift 2
  "$ZMIN_TEST_PERL" -e '
    use strict;
    use warnings;
    my $status = 0;
    for my $path (@ARGV) {
      my @st = stat($path);
      if (!@st) {
        warn "Cannot stat '\''$path'\'': $!\n";
        $status = 1;
      } else {
        print $st[7], "\n";
      }
    }
    exit $status;
  ' "$@"
  exit $?
fi
if test "$1" = "genrandom"
then
  shift
  seed="${1-}"
  count="${2-}"
  if test -z "$seed" || test $# -gt 2
  then
    echo "usage: test-tool genrandom <seed_string> [<size>]" >&2
    exit 1
  fi
  "$ZMIN_TEST_PERL" -e '
    use strict;
    use warnings;
    binmode STDOUT;
    my ($seed, $count) = @ARGV;
    my $next = 0;
    for my $byte (unpack("C*", $seed . "\0")) {
      $next = (($next * 11) + $byte) & 0xffff_ffff;
    }
    if (!defined($count) || $count eq "") {
      $count = 0xffff_ffff;
    } elsif ($count =~ /^([0-9]+)([kKmMgG]?)$/) {
      my %suffix = (
        "" => 1,
        k => 1024, K => 1024,
        m => 1024 * 1024, M => 1024 * 1024,
        g => 1024 * 1024 * 1024, G => 1024 * 1024 * 1024,
      );
      $count = $1 * $suffix{$2};
    } else {
      die "cannot parse argument '$count'\n";
    }
    while ($count-- > 0) {
      $next = (($next * 1103515245) + 12345) & 0xffff_ffff;
      print chr(($next >> 16) & 0xff);
    }
  ' "$seed" "$count"
  exit $?
fi
if test "$1" = "env-helper"
then
  shift
  type=
  default=
  exit_code=
  while test $# -gt 0
  do
    case "$1" in
      --type=*) type="${1#--type=}"; shift ;;
      --default=*) default="${1#--default=}"; shift ;;
      --exit-code) exit_code=1; shift ;;
      --) shift; break ;;
      *) break ;;
    esac
  done
  name="${1-}"
  if test "$type" != "bool" || test -z "$exit_code" || test -z "$name"
  then
    echo "unsupported env-helper invocation" >&2
    exit 127
  fi
  case "$default" in
    true|TRUE|yes|YES|on|ON|1|false|FALSE|no|NO|off|OFF|0|'') ;;
    *) exit 128 ;;
  esac
  if eval "test \"\${$name+set}\" = set"
  then
    eval "value=\${$name}"
  else
    value="$default"
  fi
  case "$value" in
    true|TRUE|yes|YES|on|ON|1) exit 0 ;;
    false|FALSE|no|NO|off|OFF|0|'') exit 1 ;;
    *) exit 128 ;;
  esac
fi
if test "$1" = "rot13-filter"
then
  shift
  log=
  while test $# -gt 0
  do
    case "$1" in
      --log=*) log="${1#--log=}"; shift ;;
      clean|smudge) break ;;
      *) shift ;;
    esac
  done
  "$ZMIN_TEST_PERL" -e '
    use strict;
    use warnings;
    binmode STDIN;
    binmode STDOUT;
    select(STDOUT);
    $| = 1;

    use Cwd qw(abs_path);

    my $log_path = shift @ARGV;
    my %allowed = map { $_ => 1 } @ARGV;
    open my $init_log, ">>", $log_path or die "rot13-filter: cannot open $log_path: $!\n";
    close $init_log;
    $log_path = abs_path($log_path) || $log_path;
    chdir "/" or die "rot13-filter: cannot chdir away from repository: $!\n";

    sub log_line {
      my ($path, $line) = @_;
      open my $log, ">>", $path or die "rot13-filter: cannot open $path: $!\n";
      print {$log} $line;
      close $log;
    }

    log_line($log_path, "START\n");

    sub read_pkt {
      my $hdr = "";
      my $n = read(STDIN, $hdr, 4);
      return undef if !defined($n) || $n == 0;
      die "short pkt-line header\n" if $n != 4;
      my $len = hex($hdr);
      return undef if $len == 0;
      die "invalid pkt-line length\n" if $len < 4;
      my $data = "";
      my $need = $len - 4;
      while (length($data) < $need) {
        my $chunk = "";
        my $got = read(STDIN, $chunk, $need - length($data));
        die "short pkt-line payload\n" if !defined($got) || $got == 0;
        $data .= $chunk;
      }
      return $data;
    }

    sub read_list {
      my @items;
      while (1) {
        my $pkt = read_pkt();
        last if !defined($pkt);
        chomp $pkt;
        push @items, $pkt;
      }
      return @items;
    }

    sub write_pkt {
      my ($data) = @_;
      printf STDOUT "%04x%s", length($data) + 4, $data;
    }

    sub write_flush {
      print STDOUT "0000";
    }

    sub rot13 {
      my ($data) = @_;
      $data =~ tr/A-Za-z/N-ZA-Mn-za-m/;
      return $data;
    }

    my @hello = read_list();
    if (!@hello || $hello[0] ne "git-filter-client") {
      die "unexpected filter hello\n";
    }
    write_pkt("git-filter-server\n");
    write_pkt("version=2\n");
    write_flush();

    my @client_caps = read_list();
    for my $cap (@client_caps) {
      if ($cap =~ /^capability=(.+)$/ && $allowed{$1}) {
        write_pkt("capability=$1\n");
      }
    }
    write_flush();
    log_line($log_path, "init handshake complete\n");

    my %delayed;
    my $list_available_calls = 0;

    while (1) {
      my @headers = read_list();
      last if !@headers;
      my ($command, $pathname);
      my @meta;
      for my $header (@headers) {
        if ($header =~ /^command=(.*)$/) {
          $command = $1;
        } elsif ($header =~ /^pathname=(.*)$/) {
          $pathname = $1;
        } elsif ($header =~ /^(ref|treeish|blob)=/) {
          push @meta, $header;
        }
      }

      if ($command eq "list_available_blobs") {
        $list_available_calls++;
        my @available = sort grep {
          ($list_available_calls == 1 && /test-delay1[01]\./) ||
          ($list_available_calls == 2 && /test-delay20\./)
        } keys %delayed;
        if (grep { /invalid-delay\./ } keys %delayed) {
          @available = ("unfiltered");
        }
        my $available_text = @available ? " " . join(" ", @available) : "";
        log_line($log_path, "IN: list_available_blobs$available_text [OK]\n");
        for my $path (@available) {
          write_pkt("pathname=$path\n");
        }
        write_flush();
        write_pkt("status=success\n");
        write_flush();
        next;
      }

      my $input = "";
      while (1) {
        my $pkt = read_pkt();
        last if !defined($pkt);
        $input .= $pkt;
      }
      my $meta = @meta ? join(" ", @meta) . " " : "";
      if ($command eq "clean" && $pathname eq "clean-write-fail.r") {
        log_line(
          $log_path,
          "IN: $command $pathname ${meta}" . length($input) . " [OK] -- [WRITE FAIL]\n"
        );
        print STDERR "clean write error\n";
        exit 1;
      }
      if ($command eq "smudge" && $pathname eq "smudge-write-fail.r") {
        log_line(
          $log_path,
          "IN: $command $pathname ${meta}" . length($input) . " [OK] -- [WRITE FAIL]\n"
        );
        print STDERR "smudge write error\n";
        exit 1;
      }
      if ($pathname eq "error.r") {
        log_line(
          $log_path,
          "IN: $command $pathname ${meta}" . length($input) . " [OK] -- [ERROR]\n"
        );
        write_pkt("status=error\n");
        write_flush();
        next;
      }
      if ($pathname eq "abort.r") {
        log_line(
          $log_path,
          "IN: $command $pathname ${meta}" . length($input) . " [OK] -- [ABORT]\n"
        );
        write_pkt("status=abort\n");
        write_flush();
        next;
      }
      my $output;
      if ($command eq "smudge" && exists $delayed{$pathname} && !length($input)) {
        $output = delete $delayed{$pathname};
      } elsif ($command eq "smudge" && $pathname =~ /(?:test-delay(?:10|11|20)|missing-delay|invalid-delay)\./) {
        $delayed{$pathname} = rot13($input);
        log_line(
          $log_path,
          "IN: $command $pathname ${meta}" . length($input) . " [OK] -- [DELAYED]\n"
        );
        write_pkt("status=delayed\n");
        write_flush();
        next;
      } else {
        $output = rot13($input);
      }
      my $out_packets = length($output) ? int((length($output) + 65515) / 65516) : 0;
      my $out_marker = "." x $out_packets;
      log_line(
        $log_path,
        "IN: $command $pathname ${meta}" . length($input) .
          " [OK] -- OUT: " . length($output) . " $out_marker [OK]\n"
      );

      write_pkt("status=success\n");
      write_flush();
      my $offset = 0;
      while ($offset < length($output)) {
        my $chunk = substr($output, $offset, 65516);
        write_pkt($chunk);
        $offset += length($chunk);
      }
      write_flush();
      write_pkt("status=success\n");
      write_flush();
    }

    log_line($log_path, "STOP\n");
  ' "$log" "$@"
  exit $?
fi
if test "$1" = "sha1"
then
  "$ZMIN_TEST_PERL" -MDigest::SHA=sha1_hex -e '
    use strict;
    use warnings;
    binmode STDIN;
    local $/;
    print sha1_hex(<STDIN>), "\n";
  '
  exit $?
fi
if test "$1" = "zlib" && test "${2-}" = "deflate"
then
  "$ZMIN_TEST_PERL" -MCompress::Zlib=compress -e '
    use strict;
    use warnings;
    binmode STDIN;
    binmode STDOUT;
    local $/;
    my $input = <STDIN>;
    my $compressed = compress($input);
    die "zlib deflate failed\n" unless defined $compressed;
    print $compressed;
  '
  exit $?
fi
if test "$1" = "chmtime"
then
  shift
  get=
  verbose=
  while test $# -gt 0
  do
    case "$1" in
      --get|-g) get=1; shift ;;
      --verbose) verbose=1; shift ;;
      --) shift; break ;;
      *) break ;;
    esac
  done
  spec=
  case "${1-}" in
    =*|+*|-*) spec="$1"; shift ;;
  esac
  "$ZMIN_TEST_PERL" -e '
    use strict;
    use warnings;
    my ($get, $verbose, $spec, @paths) = @ARGV;
    die "chmtime: missing path\n" unless @paths;
    for my $path (@paths) {
      my @st = stat($path) or die "chmtime: cannot stat $path: $!\n";
      my $mtime = $st[9];
      my $target = $mtime;
      if (defined($spec) && length($spec)) {
        if ($spec =~ /^=([+-]\d+)$/) {
          $target = time() + $1;
        } elsif ($spec =~ /^=(\d+)$/) {
          $target = $1;
        } elsif ($spec =~ /^([+-]\d+)$/) {
          $target = $mtime + $1;
        } else {
          die "chmtime: unsupported time spec $spec\n";
        }
        utime($target, $target, $path) or die "chmtime: cannot update $path: $!\n";
      }
      if ($get) {
        if ($verbose) {
          print "$path $target\n";
        } else {
          print "$target\n";
        }
      }
    }
  ' "${get:-}" "${verbose:-}" "$spec" "$@"
  exit $?
fi
ref_store_gitdir () {
  store="$1"
  case "$store" in
    main)
      worktree=.
      ;;
    worktree:*)
      worktree="${store#worktree:}"
      if test -d ".git/worktrees/$worktree"
      then
        printf '%s\n' ".git/worktrees/$worktree"
        return 0
      fi
      ;;
    *)
      return 1
      ;;
  esac
  gitfile="$worktree/.git"
  if test -f "$gitfile"
  then
    gitdir="$(sed -n 's/^gitdir: //p' "$gitfile")"
    case "$gitdir" in
      /* | [A-Za-z]:*) ;;
      *) gitdir="$worktree/$gitdir" ;;
    esac
  else
    gitdir="$gitfile"
  fi
  printf '%s\n' "$gitdir"
}

if test "$1" = "ref-store" && test "${3-}" = "create-reflog"
then
  gitdir="$(ref_store_gitdir "${2-}")" || exit 1
  ref="${4-}"
  log_path="$gitdir/logs/$ref"
  mkdir -p "$(dirname "$log_path")" &&
    : >"$log_path"
  exit $?
fi
if test "$1" = "ref-store" && test "${3-}" = "update-ref"
then
  gitdir="$(ref_store_gitdir "${2-}")" || exit 1
  message="${4-}"
  ref="${5-}"
  new_oid="${6-}"
  old_oid="${7-}"
  ref_path="$gitdir/$ref"
  log_path="$gitdir/logs/$ref"
  ident="${GIT_COMMITTER_NAME:-A U Thor} <${GIT_COMMITTER_EMAIL:-author@example.com}> 1112911993 -0700"
  mkdir -p "$(dirname "$ref_path")" "$(dirname "$log_path")" &&
    printf '%s\n' "$new_oid" >"$ref_path" &&
    printf '%s %s %s\t%s\n' "$old_oid" "$new_oid" "$ident" "$message" >"$log_path"
  exit $?
fi
if test "$1" = "ref-store" && test "${3-}" = "reflog-exists"
then
  gitdir="$(ref_store_gitdir "${2-}")" || exit 1
  ref="${4-}"
  test -f "$gitdir/logs/$ref"
  exit $?
fi
if test "$1" = "ref-store" && test "${3-}" = "for-each-reflog-ent"
then
  gitdir="$(ref_store_gitdir "$2")" || exit 1
  ref="${4-}"
  log_path="$gitdir/logs/$ref"
  if test -f "$log_path"
  then
    cat "$log_path"
  fi
  exit 0
fi
echo "upstream test-tool helper is not available in this installed-binary audit" >&2
exit 127
EOF
      chmod +x t/helper/test-tool
      cp t/helper/test-tool t/helper/test-tool.exe 2>/dev/null || true
      chmod +x t/helper/test-tool.exe 2>/dev/null || true
    fi
    if [[ ! "${RUNNER_OS:-}" == "Windows" && ! "${OS:-}" == "Windows_NT" ]]; then
      if [[ ! -f t/helper/test-tool-real && -x t/helper/test-tool ]]; then
        mv t/helper/test-tool t/helper/test-tool-real
      fi
      if [[ -x t/helper/test-tool-real ]]; then
        cat >t/helper/test-tool <<EOF
#!/usr/bin/env sh
if test "\${ZMIN_UPSTREAM_TEST_TOOL_TRACE2:-0}" = "1" && test "\${1-}" = "trace2"
then
  exec "$zmin_bin" test-tool "\$@"
fi
exec "$source_dir/t/helper/test-tool-real" "\$@"
EOF
        chmod +x t/helper/test-tool
      fi
    fi
    if [[ -s "$cache_root/$tag.tar.gz" ]]; then
      [[ ! -L t/t5510-fetch.sh && ! -d t/t5510-fetch.sh ]] || {
        echo "unsafe prepared t5510-fetch.sh destination" >&2
        exit 1
      }
      tar -xOzf "$cache_root/$tag.tar.gz" "git-${tag#v}/t/t5510-fetch.sh" >t/t5510-fetch.sh
    fi
    if [[ "${ZMIN_UPSTREAM_SKIP_UNSUPPORTED_REFTABLE:-0}" == "1" ]]; then
      transform_t5510_fetch t/t5510-fetch.sh
    fi
    remove_unneeded_prepared_outputs "$source_dir" "$pristine_source_dir" || {
      echo "prepared dynamic build output could not be removed safely" >&2
      exit 1
    }
    validate_test_lib_patch || {
      echo "prepared t/test-lib.sh is not the exact pinned preparation patch" >&2
      exit 1
    }
    publish_prepared_artifacts_manifest || {
      echo "prepared source contains unauthenticated generated output" >&2
      exit 1
    }
    write_helper_provenance
  )
  release_prepare_lock || return 1
  release_cache_lock || return 1
  trap - EXIT
}

validate_upstream_prerequisites() {
  local helper="$(test_tool_exec_path)"
  local helper_real="$(test_tool_real_path)"
  local platform_helper="$(platform_helper_path)"
  local source_marker="$source_dir/.zmin-pristine-source.sha256"
  local archive="$cache_root/$tag.tar.gz"
  local archive_sha=""
  local helper_probe=""

  validate_make_identity || return 1

  require_cache_directory "$pristine_source_dir" pristine_source_dir || return 1
  require_cache_directory "$source_dir" source_dir || return 1
  if [[ ! -f "$archive" || -L "$archive" || ! -f "$source_marker" || -L "$source_marker" ]]; then
    echo "missing pinned upstream source marker or archive for $tag" >&2
    return 1
  fi
  validate_archive_identity "$archive" || return 1
  archive_sha="$(shasum -a 256 "$archive" | awk '{ print $1 }')"
  if [[ "$(cat "$source_marker")" != "$archive_sha" ]]; then
    echo "pinned upstream source marker mismatch for $source_dir" >&2
    return 1
  fi

  if ! source_matches_pristine; then
    echo "pinned prepared upstream source diverged from the verified pristine source" >&2
    return 1
  fi
  if ! write_perl_module_manifest "$source_dir" >/dev/null; then
    echo "pinned generated Perl module manifest is incomplete or unsafe" >&2
    return 1
  fi

  if [[ ! -x "$helper" || -L "$helper" || ! -x "$helper_real" || -L "$helper_real" ||
    ! -x "$platform_helper" || -L "$platform_helper" ]]; then
    echo "pinned upstream test-tool helper was not built at the expected paths" >&2
    return 1
  fi
  if ! helper_provenance_matches; then
    echo "pinned upstream test-tool helper provenance mismatch" >&2
    return 1
  fi
  if [[ "$helper" != *.exe ]]; then
    helper_probe="$("$helper" path-utils absolute_path "$source_dir" 2>/dev/null)" || {
      echo "pinned upstream test-tool behavioral validation failed" >&2
      return 1
    }
    if [[ "$helper_probe" != "$source_dir" ]]; then
      echo "pinned upstream test-tool resolved the wrong source path" >&2
      return 1
    fi
  fi
  PERL5LIB="$source_dir/perl/build/lib" "$perl_bin" -MGit -e 1
}

mode_rank() {
  case "$1" in
    quick) echo 1 ;;
    standard) echo 2 ;;
    exhaustive) echo 3 ;;
    all-nondeprecated) echo 4 ;;
    all-top-level) echo 5 ;;
    *) echo 99 ;;
  esac
}

selected_tests() {
  local max_rank
  max_rank="$(mode_rank "$mode")"
  awk -F '\t' -v max_rank="$max_rank" -v offset="$manifest_offset" -v limit="$manifest_limit" '
    /^#/ || NF < 2 { next }
    {
      rank = ($1 == "quick" ? 1 : ($1 == "standard" ? 2 : ($1 == "exhaustive" || $1 == "full-core" ? 3 : ($1 == "all-nondeprecated" ? 4 : ($1 == "all-top-level" ? 5 : 99)))))
      if (rank <= max_rank) {
        selected += 1
        if (selected <= offset) {
          next
        }
        emitted += 1
        if (limit > 0 && emitted > limit) {
          exit
        }
        print $2 "\t" $1 "\t" $3
      }
    }
  ' "$resolved_test_list"
}

resolve_test_list() {
  if [[ -n "$test_list" ]]; then
    resolved_test_list="$test_list"
    return
  fi

  case "$mode" in
    exhaustive)
      resolved_test_list="$out_dir/exhaustive-full-core.tsv"
      run_command_with_timeout "$phase_timeout_seconds" \
        "$repo_root/tools/git-upstream-compat-manifest.sh" full-core >"$resolved_test_list"
      ;;
    all-nondeprecated)
      resolved_test_list="$out_dir/all-nondeprecated.tsv"
      run_command_with_timeout "$phase_timeout_seconds" \
        "$repo_root/tools/git-upstream-compat-manifest.sh" all-nondeprecated >"$resolved_test_list"
      ;;
    all-top-level)
      resolved_test_list="$out_dir/all-top-level.tsv"
      run_command_with_timeout "$phase_timeout_seconds" \
        "$repo_root/tools/git-upstream-compat-manifest.sh" all-top-level >"$resolved_test_list"
      ;;
    *)
      resolved_test_list="$default_core_test_list"
      ;;
  esac
}

run_list_from_flags() {
  local arg want_next=0
  for arg in $test_flags; do
    if [[ "$want_next" == "1" ]]; then
      printf '%s\n' "$arg"
      return 0
    fi
    case "$arg" in
      --run=*)
        printf '%s\n' "${arg#--run=}"
        return 0
        ;;
      -r)
        want_next=1
        ;;
    esac
  done
  return 1
}

max_numeric_run_selector() {
  local run_list="$1"
  awk -v run_list="$run_list" '
    BEGIN {
      max = 0
      n = split(run_list, parts, /[,[:space:]]+/)
      for (i = 1; i <= n; i++) {
        part = parts[i]
        if (part == "") {
          continue
        }
        if (part ~ /^[0-9]+$/) {
          value = part + 0
        } else if (part ~ /^[0-9]+-[0-9]+$/) {
          split(part, range, "-")
          value = range[2] + 0
        } else {
          printf "unsupported bounded --run selector: %s\n", part > "/dev/stderr"
          exit 2
        }
        if (value > max) {
          max = value
        }
      }
      if (max <= 0) {
        print "bounded run requires a numeric --run selector" > "/dev/stderr"
        exit 2
      }
      print max
    }
  '
}

todo_breakage_vanished_only() {
  local log="$1"
  [[ -f "$log" ]] || return 1
  if ! grep -q 'known breakage(s) vanished' "$log"; then
    return 1
  fi
  if grep -q '^not ok ' "$log"; then
    return 1
  fi
  return 0
}

run_command_with_timeout() {
  local timeout="$1"
  shift
  "$perl_bin" -e '
    use strict;
    use warnings;
    use POSIX qw(setpgid);

    my $timeout = shift @ARGV;
    die "invalid test timeout: $timeout\n" unless $timeout =~ /^[1-9][0-9]*$/;
    my $pid = fork();
    die "fork failed: $!\n" unless defined $pid;
    if ($pid == 0) {
      setpgid(0, 0) or die "setpgid failed: $!\n";
      exec @ARGV;
      die "exec failed: $!\n";
    }
    setpgid($pid, $pid);
    $SIG{ALRM} = sub {
      print STDERR "upstream test timed out after ${timeout}s\n";
      kill "TERM", -$pid;
      select undef, undef, undef, 0.25;
      kill "KILL", -$pid;
      waitpid($pid, 0);
      exit 124;
    };
    alarm $timeout;
    waitpid($pid, 0);
    alarm 0;
    my $status = $?;
    exit(($status & 127) ? 128 + ($status & 127) : $status >> 8);
  ' "$timeout" "$@"
}

run_test_with_timeout() {
  if [[ "$test_timeout" == "0" ]]; then
    "$@"
    return
  fi
  run_command_with_timeout "$test_timeout" "$@"
}

make_git_shim() {
  local shim_dir="$1"
  local shim_helper platform_helper platform_helper_shell
  mkdir -p "$shim_dir"
  shim_helper="$shim_dir/$zmin_remote_http_helper_name"
  if [[ "${RUNNER_OS:-}" == "Windows" || "${OS:-}" == "Windows_NT" ]]; then
    cp "$zmin_bin" "$shim_dir/git.exe"
    cp "$zmin_bin" "$shim_dir/git-http-backend.exe"
    cp "$zmin_bin" "$shim_dir/git-sh-i18n.exe"
    cp "$zmin_bin" "$shim_dir/git-sh-setup.exe"
    cp "$zmin_remote_http_helper" "$shim_helper"
  else
    ln -sf "$zmin_bin" "$shim_dir/git"
    ln -sf "$zmin_bin" "$shim_dir/git-http-backend"
    ln -sf "$zmin_remote_http_helper" "$shim_helper"
    cat >"$shim_dir/test-tool" <<EOF
#!/usr/bin/env sh
if test "\${1-}" = "trace2"
then
  exec "$zmin_bin" test-tool "\$@"
fi
exec "$source_dir/t/helper/test-tool" "\$@"
EOF
    chmod +x "$shim_dir/test-tool"
    platform_helper="$(platform_helper_path)"
    [[ -x "$platform_helper" && ! -L "$platform_helper" ]] || {
      echo "pinned git-sh-i18n--envsubst helper is missing: $platform_helper" >&2
      return 1
    }
    platform_helper_shell="$("$perl_bin" -e 'my $path = shift; $path =~ s/([\\\$"`])/\\$1/g; print qq{"$path"}' "$platform_helper")"
    ZMIN_ENV_SUBST="$platform_helper_shell" "$perl_bin" \
     -0pe 's/\@\@LOCALEDIR\@\@/$ENV{ZMIN_UPSTREAM_LOCALEDIR}/g; s/\@\@USE_GETTEXT_SCHEME\@\@/fallthrough/g; s/git sh-i18n--envsubst/$ENV{ZMIN_ENV_SUBST}/g' \
    "$source_dir/git-sh-i18n.sh" >"$shim_dir/git-sh-i18n"
    chmod +x "$shim_dir/git-sh-i18n"
    "$perl_bin" \
      -0pe 's/# \@BROKEN_PATH_FIX\@/:/g; s/\@PAGER_ENV\@//g; s/\@DIFF\@/diff/g' \
      "$source_dir/git-sh-setup.sh" >"$shim_dir/git-sh-setup"
    chmod +x "$shim_dir/git-sh-setup"
  fi
}

make_stock_git_http_shim() {
  local shim_dir="$1"
  local helper path expected actual
  mkdir -p "$shim_dir"
  for helper in "$http_git_relative" "$http_remote_http_relative" "$http_backend_relative"; do
    [[ -f "$http_bundle/$helper" && ! -L "$http_bundle/$helper" && -x "$http_bundle/$helper" ]] || {
      echo "validated HTTP bundle member is missing: $helper" >&2
      return 1
    }
    cp "$http_bundle/$helper" "$shim_dir/$helper"
    expected="$(awk -F '\t' -v member="$helper" \
      '$1 == "member" && $3 == member { print $4; count += 1 } END { if (count != 1) exit 1 }' \
      "$http_bundle/bundle.tsv")" || {
      echo "validated HTTP bundle has no unique checksum for: $helper" >&2
      return 1
    }
    actual="$(sha256_file_with_validated_perl "$shim_dir/$helper")" || {
      echo "cannot hash copied HTTP bundle member: $helper" >&2
      return 1
    }
    [[ "$actual" == "$expected" ]] || {
      echo "copied HTTP shim member checksum mismatch: $helper" >&2
      return 1
    }
  done
  [[ -f "$(test_tool_exec_path)" && ! -L "$(test_tool_exec_path)" && -x "$(test_tool_exec_path)" ]] || {
    echo "prepared pinned test-tool is missing" >&2
    return 1
  }
  cp "$(test_tool_exec_path)" "$shim_dir/test-tool"
  [[ -f "$source_dir/git-sh-setup" && ! -L "$source_dir/git-sh-setup" ]] || {
    echo "prepared pinned git-sh-setup helper is missing" >&2
    return 1
  }
  cp "$source_dir/git-sh-setup" "$shim_dir/git-sh-setup"
  platform_helper="$(platform_helper_path)"
  [[ -f "$platform_helper" && ! -L "$platform_helper" && -x "$platform_helper" ]] || {
    echo "prepared pinned git-sh-i18n--envsubst helper is missing" >&2
    return 1
  }
  cp "$platform_helper" "$shim_dir/git-sh-i18n--envsubst"
  [[ -f "$source_dir/git-sh-i18n" && ! -L "$source_dir/git-sh-i18n" ]] || {
    echo "prepared pinned git-sh-i18n helper is missing" >&2
    return 1
  }
  cp "$source_dir/git-sh-i18n" "$shim_dir/git-sh-i18n"
  chmod +x "$shim_dir"/*
  for path in "$shim_dir/$http_git_relative" "$shim_dir/$http_remote_http_relative" \
    "$shim_dir/$http_backend_relative" "$shim_dir/test-tool" "$shim_dir/git-sh-setup" \
    "$shim_dir/git-sh-i18n" "$shim_dir/git-sh-i18n--envsubst"; do
    [[ -f "$path" && ! -L "$path" && -x "$path" ]] || {
      echo "stock HTTP shim member is not a regular executable: $path" >&2
      return 1
    }
  done
}

run_manifest_fixture() (
  set -euo pipefail
  local fixture_root pristine prepared expected target extra saved missing_payload
  local limits_pristine limits_prepared
  local published_marker_payload published_manifest_payload sentinel sentinel_expected
  local type_saved optional_expected optional_saved symlink_outside symlink_saved relative
  local fixture_make generated_graph_saved
  fixture_root="$(mktemp -d "$out_dir/.zmin-manifest-fixture.XXXXXX")"
  trap 'release_prepare_lock || true; chmod -R u+w "$fixture_root" 2>/dev/null || true; rm -rf "$fixture_root"' EXIT
  pristine="$fixture_root/pristine"
  prepared="$fixture_root/prepared"
  contract_python_bin="${ZMIN_UPSTREAM_CONTRACT_PYTHON:?set ZMIN_UPSTREAM_CONTRACT_PYTHON to the trusted absolute Python binary}"
  zmin_current_contract_trust=1
  prepared_cleanup_python_trust=1
  mkdir -p "$pristine/t" "$pristine/perl" "$pristine/templates" "$pristine/t/helper"
  mkdir -p "$pristine/builtin" "$pristine/compat" \
    "$pristine/compat/poll" "$pristine/compat/regex" "$pristine/compat/stub" \
    "$pristine/block-sha1" "$pristine/sha1collisiondetection/lib" \
    "$pristine/contrib/credential/osxkeychain"
  mkdir -p "$prepared/t"
  [[ "$prepare_lock" == "$lock_root/prepare.lock" ]] || return 1
  acquire_owned_lock "$prepare_lock" "manifest fixture preparation lock" "$prepare_lock_fd"
  prepare_lock_held=1
  validate_fixture_make_identity() {
    require_absolute_executable ZMIN_UPSTREAM_CONTRACT_MAKE "$make_bin"
    if [[ "$(uname -s 2>/dev/null || printf '%s' unknown)" == "Linux" ]]; then
      if [[ "$make_bin" == "$fixture_make" ]]; then
        make_sha256="$(shasum -a 256 "$make_bin" | awk '{ print $1 }')"
        make_version='GNU Make 4.4 (manifest fixture)'
        make_anchor_path="$make_bin"
        make_anchor_sha256="$make_sha256"
        make_anchor_version="$make_version"
      else
        validate_make_identity
      fi
    else
      make_sha256="$(shasum -a 256 "$make_bin" | awk '{ print $1 }')"
      make_version="descriptor-bound Make unsupported on this platform"
      make_anchor_path="$make_bin"
      make_anchor_sha256="$make_sha256"
      make_anchor_version="$make_version"
    fi
  }
  fixture_make="$fixture_root/make"
  cat >"$fixture_make" <<'EOF'
#!/bin/sh
if test "${1-}" = "--version"
then
  printf '%s\n' 'GNU Make 4.4 (manifest fixture)'
  exit 0
fi
if test -n "${MAKEFILES-}${MAKEFLAGS-}${MFLAGS-}${GNUMAKEFLAGS-}${MAKEOVERRIDES-}${MAKEPATH-}${MAKE_INCLUDE_PATH-}${MAKE_PATH-}${MAKE_MODE-}"
then
  echo 'make environment injection was not cleared' >&2
  exit 97
fi
cat <<'MAKE_DATABASE'
# Files
dep_dirs := .depend builtin/.depend compat/.depend \
  compat/poll/.depend compat/regex/.depend compat/stub/.depend \
  block-sha1/.depend sha1collisiondetection/lib/.depend \
  contrib/credential/osxkeychain/.depend conditional\ dir/.depend
MAKE_DATABASE
EOF
  chmod +x "$fixture_make"
  make_bin="$fixture_make"
  validate_fixture_make_identity
  cat >"$pristine/Makefile" <<'EOF'
dep_dirs := .depend builtin/.depend compat/.depend \
  compat/poll/.depend compat/regex/.depend compat/stub/.depend \
  block-sha1/.depend sha1collisiondetection/lib/.depend \
  contrib/credential/osxkeychain/.depend conditional\ dir/.depend
all:
	@:
EOF
  make_bin="${ZMIN_UPSTREAM_FIXTURE_MAKE:-$fixture_make}"
  validate_fixture_make_identity
  fixture_platform="$(uname -s 2>/dev/null || printf '%s' unknown)"
  if [[ "$fixture_platform" == "Linux" ]]; then
    make_bin="${ZMIN_UPSTREAM_CONTRACT_MAKE:?set ZMIN_UPSTREAM_CONTRACT_MAKE for the Linux memfd probe}"
    make_anchor_path=""
    make_anchor_sha256=""
    make_anchor_version=""
    validate_fixture_make_identity
    parser_output="$(evaluate_prepared_dep_dirs "$pristine")"
    [[ -n "$parser_output" ]]
  elif [[ "$fixture_platform" == "Darwin" || "${RUNNER_OS:-}" == "Windows" || "${OS:-}" == "Windows_NT" ]]; then
    if evaluate_prepared_dep_dirs "$pristine" >/dev/null 2>&1; then
      echo "platform descriptor-bound Make runner did not fail closed" >&2
      exit 1
    fi
    parser_output=''
  else
    parser_output="$(evaluate_prepared_dep_dirs "$pristine")"
  fi
  prepared_generated_graph="$(cat <<'EOF'
.depend
builtin/.depend
compat/.depend
compat/poll/.depend
compat/regex/.depend
compat/stub/.depend
block-sha1/.depend
sha1collisiondetection/lib/.depend
contrib/credential/osxkeychain/.depend
conditional dir/.depend
po/build
EOF
)"
  if [[ "$fixture_platform" != "Darwin" && "${RUNNER_OS:-}" != "Windows" && "${OS:-}" != "Windows_NT" ]]; then
    printf '%s\n' "$parser_output" | grep -Fqx 'conditional dir/.depend'
    if ! (MAKEFILES=forged MAKEFLAGS=forged MFLAGS=forged GNUMAKEFLAGS=forged MAKEPATH=forged MAKE_INCLUDE_PATH=forged MAKE_PATH=forged MAKE_MODE=forged \
      evaluate_prepared_dep_dirs "$pristine" >/dev/null); then
      echo "pinned Make runner did not clear injected Make variables" >&2
      exit 1
    fi
    make_saved="$fixture_root/make-saved"
    cp "$fixture_make" "$make_saved"
    printf '%s\n' '#!/bin/sh' 'printf "%s\n" "GNU Make forged"' >"$fixture_make"
    chmod +x "$fixture_make"
    if evaluate_prepared_dep_dirs "$pristine" >/dev/null 2>&1; then
      echo "Make replacement race was accepted" >&2
      exit 1
    fi
    mv "$make_saved" "$fixture_make"
    make_anchor_path=""
    make_anchor_sha256=""
    make_anchor_version=""
    validate_fixture_make_identity
    for parser_case in duplicate injection traversal; do
      parser_make="$fixture_root/make-$parser_case"
      case "$parser_case" in
        duplicate)
          printf '%s\n' '#!/bin/sh' 'if test "${1-}" = "--version"; then printf "%s\\n" "GNU Make 4.4 (manifest fixture)"; exit 0; fi' '# Files' 'dep_dirs := .depend .depend' >"$parser_make"
          ;;
        injection)
          printf '%s\n' '#!/bin/sh' 'if test "${1-}" = "--version"; then printf "%s\\n" "GNU Make 4.4 (manifest fixture)"; exit 0; fi' '# Files' 'dep_dirs := .depend;touch external' >"$parser_make"
          ;;
        traversal)
          printf '%s\n' '#!/bin/sh' 'if test "${1-}" = "--version"; then printf "%s\\n" "GNU Make 4.4 (manifest fixture)"; exit 0; fi' '# Files' 'dep_dirs := ../.depend' >"$parser_make"
          ;;
      esac
      chmod +x "$parser_make"
      make_bin="$parser_make"
      make_anchor_path=""
      make_anchor_sha256=""
      make_anchor_version=""
      validate_fixture_make_identity
      if evaluate_prepared_dep_dirs "$pristine" >/dev/null 2>&1; then
        echo "fixture accepted invalid evaluated Makefile case: $parser_case" >&2
        exit 1
      fi
    done
  fi
  make_bin="$fixture_make"
  make_anchor_path=""
  make_anchor_sha256=""
  make_anchor_version=""
  validate_fixture_make_identity
  windows_fixture="$fixture_root/windows-helper"
  windows_fallback="$windows_fixture/test-tool"
  windows_candidate="$windows_fixture/test-tool.exe"
  mkdir -p "$windows_fixture"
  printf 'fallback\n' >"$windows_fallback"
  chmod +x "$windows_fallback"
  [[ "$(RUNNER_OS=Windows platform_executable_path "$windows_candidate" "$windows_fallback")" == "$windows_fallback" ]]
  printf 'candidate\n' >"$windows_candidate"
  chmod +x "$windows_candidate"
  [[ "$(RUNNER_OS=Windows platform_executable_path "$windows_candidate" "$windows_fallback")" == "$windows_candidate" ]]
  rm -f "$windows_candidate"
  printf 'sentinel\n' >"$fixture_root/windows-sentinel"
  ln -s "$fixture_root/windows-sentinel" "$windows_candidate"
  if RUNNER_OS=Windows platform_executable_path "$windows_candidate" "$windows_fallback" >/dev/null 2>&1; then
    echo "Windows symlink .exe artifact was accepted" >&2
    exit 1
  fi
  rm -f "$windows_candidate"
  mkdir "$windows_candidate"
  if RUNNER_OS=Windows platform_executable_path "$windows_candidate" "$windows_fallback" >/dev/null 2>&1; then
    echo "Windows directory .exe artifact was accepted" >&2
    exit 1
  fi
  rmdir "$windows_candidate"
  printf 'not executable\n' >"$windows_candidate"
  chmod -x "$windows_candidate"
  if RUNNER_OS=Windows platform_executable_path "$windows_candidate" "$windows_fallback" >/dev/null 2>&1; then
    echo "Windows non-executable .exe artifact was accepted" >&2
    exit 1
  fi
  rm -f "$windows_candidate"
  ln -s "$fixture_make" "$fixture_root/make-symlink"
  make_bin="$fixture_root/make-symlink"
  if (validate_fixture_make_identity); then
    echo "fixture accepted symlinked Make identity" >&2
    exit 1
  fi
  make_bin="$fixture_make"
  make_anchor_path=""
  make_anchor_sha256=""
  make_anchor_version=""
  validate_fixture_make_identity
  mkdir -p "$pristine/conditional dir"
  printf 'fixture static source\n' >"$pristine/README"
  printf 'test_finish_ () {\n\tprintf "fixture\\n"\n}\n' >"$pristine/t/test-lib.sh"
  printf 'test_expect_success CASE_INSENSITIVE_FS,REFFILES '\''fixture reftable'\'' true\n' >"$pristine/t/t5510-fetch.sh"
  printf 'package Git;\n1;\n' >"$pristine/perl/Git.pm"
  mkdir -p "$pristine/perl/FromCPAN"
  printf 'package FromCPAN::Error;\n1;\n' >"$pristine/perl/FromCPAN/Error.pm"
  cp -R "$pristine/." "$prepared/"
  mkdir -p \
    "$prepared/.depend/nested" \
    "$prepared/builtin/.depend/nested" \
    "$prepared/compat/.depend/nested" \
    "$prepared/compat/poll/.depend/nested" \
    "$prepared/compat/regex/.depend/nested" \
    "$prepared/compat/stub/.depend/nested" \
    "$prepared/block-sha1/.depend/nested" \
    "$prepared/sha1collisiondetection/lib/.depend/nested" \
    "$prepared/contrib/credential/osxkeychain/.depend/nested" \
    "$prepared/conditional dir/.depend/nested" \
    "$prepared/perl/build/lib" \
    "$prepared/perl/build/lib/FromCPAN" \
    "$prepared/perl/build/lib/stale/empty" \
    "$prepared/perl/build/lib/unexpected" \
    "$prepared/templates" \
    "$prepared/t/helper"
  printf 'perl defines\n' >"$prepared/GIT-PERL-DEFINES"
  printf 'nested dependency\n' >"$prepared/.depend/nested/one.d"
  printf 'builtin dependency\n' >"$prepared/builtin/.depend/nested/one.d"
  printf 'compat dependency\n' >"$prepared/compat/.depend/nested/one.d"
  printf 'poll dependency\n' >"$prepared/compat/poll/.depend/nested/one.d"
  printf 'regex dependency\n' >"$prepared/compat/regex/.depend/nested/one.d"
  printf 'stub dependency\n' >"$prepared/compat/stub/.depend/nested/one.d"
  printf 'block dependency\n' >"$prepared/block-sha1/.depend/nested/one.d"
  printf 'collision dependency\n' >"$prepared/sha1collisiondetection/lib/.depend/nested/one.d"
  printf 'osxkeychain dependency\n' >"$prepared/contrib/credential/osxkeychain/.depend/nested/one.d"
  printf 'hook list\n' >"$prepared/hook-list.h"
  printf 'version definition\n' >"$prepared/version-def.h"
  printf 'envsubst\n' >"$prepared/git-sh-i18n--envsubst"
  printf 'template marker\n' >"$prepared/templates/boilerplates.made"
  printf 'helper\n' >"$prepared/t/helper/test-tool-real"
  printf 'perl module\n' >"$prepared/perl/build/lib/Git.pm"
  printf 'generated perl module\n' >"$prepared/perl/build/lib/FromCPAN/Error.pm"
  printf 'unexpected generated perl output\n' >"$prepared/perl/build/lib/unexpected/Extra.pm"
  printf 'root object\n' >"$prepared/abspath.o"
  chmod +x "$prepared/git-sh-i18n--envsubst" "$prepared/t/helper/test-tool-real"
  printf 'test_finish_ () {\n\tif test -n "$ZMIN_UPSTREAM_STOP_AFTER_TEST" &&\n\t   test "$test_count" -ge "$ZMIN_UPSTREAM_STOP_AFTER_TEST"\n\tthen\n\t\ttest_done\n\tfi\n\tprintf "fixture\\n"\n}\n' >"$prepared/t/test-lib.sh"
  expected="$fixture_root/artifacts.tsv"
  if ! remove_unneeded_prepared_outputs "$prepared" "$pristine"; then
    echo "fixture failed to retain generated output for manifest validation" >&2
    exit 1
  fi
  [[ -d "$prepared/perl/build/lib/unexpected" &&
    -d "$prepared/perl/build/lib/stale" &&
    -f "$prepared/abspath.o" ]] || {
    echo "fail-closed cleanup changed a generated artifact before rejection" >&2
    exit 1
  }
  rm -rf "$prepared/perl/build/lib/unexpected"
  rm "$prepared/abspath.o"
  rm -rf "$prepared/perl/build/lib/stale"
  if ! remove_unneeded_prepared_outputs "$prepared" "$pristine"; then
    echo "fixture failed after explicit fixture-owned cleanup" >&2
    exit 1
  fi
  for relative in \
    .depend \
    builtin/.depend \
    compat/.depend \
    compat/poll/.depend \
    compat/regex/.depend \
    compat/stub/.depend \
    block-sha1/.depend \
    sha1collisiondetection/lib/.depend \
    contrib/credential/osxkeychain/.depend \
    'conditional dir/.depend'; do
    rm -rf "$prepared/$relative"
  done
  type_saved="$fixture_root/from-cpan-saved"
  mv "$prepared/perl/build/lib/FromCPAN" "$type_saved"
  printf 'type-swapped generated path\n' >"$prepared/perl/build/lib/FromCPAN"
  if remove_stale_generated_perl_directories "$prepared"; then
    echo "fixture accepted generated/source type swap" >&2
    exit 1
  fi
  rm "$prepared/perl/build/lib/FromCPAN"
  mv "$type_saved" "$prepared/perl/build/lib/FromCPAN"
  race_root="$prepared/perl/build/lib/race"
  race_barrier="$fixture_root/descriptor-race"
  mkdir -p "$race_root/empty"
  ZMIN_UPSTREAM_MANIFEST_FIXTURE_RACE_BARRIER="$race_barrier" \
    remove_stale_generated_perl_directories "$prepared" &
  race_pid=$!
  race_wait=0
  while [[ ! -e "$race_barrier.ready" ]]; do
    race_wait=$((race_wait + 1))
    [[ "$race_wait" -lt 1000 ]] || {
      echo "fixture descriptor race helper did not publish barrier" >&2
      kill "$race_pid" 2>/dev/null || true
      wait "$race_pid" 2>/dev/null || true
      exit 1
    }
    sleep 0.01
  done
  mv "$race_root" "$fixture_root/race-old"
  mkdir "$race_root"
  printf 'replacement sentinel\n' >"$race_root/replacement"
  : >"$race_barrier.go"
  if wait "$race_pid"; then
    echo "fixture descriptor race accepted replaced directory" >&2
    exit 1
  fi
  [[ -f "$race_root/replacement" && -d "$fixture_root/race-old" ]] || {
    echo "fixture descriptor race lost replacement identity" >&2
    exit 1
  }
  rm -rf "$race_root" "$fixture_root/race-old"
  [[ ! -e "$prepared/abspath.o" ]] || {
    echo "fixture retained dynamic output after explicit fixture cleanup" >&2
    exit 1
  }
  saved="$fixture_root/helper-saved"
  cp "$prepared/t/helper/test-tool-real" "$saved"
  if descriptor_relative_cleanup remove-files "$prepared" \
    t/helper/test-tool-real t/helper/test-tool-real.exe; then
    echo "fixture performed unsafe descriptor file cleanup" >&2
    exit 1
  fi
  [[ -f "$prepared/t/helper/test-tool-real" ]] || {
    echo "fixture fail-closed file cleanup lost generated helper" >&2
    exit 1
  }
  cmp -s "$saved" "$prepared/t/helper/test-tool-real" || {
    echo "fixture fail-closed file cleanup changed generated helper" >&2
    exit 1
  }

  mkdir -p "$prepared/unexpected/.depend/nested"
  printf 'unexpected dependency\n' >"$prepared/unexpected/.depend/nested/one.d"
  if ! remove_unneeded_prepared_outputs "$prepared" "$pristine"; then
    echo "fixture failed to retain unexpected dependency directory" >&2
    exit 1
  fi
  [[ -d "$prepared/unexpected/.depend" ]] || {
    echo "fixture lost unexpected dependency directory after rejection" >&2
    exit 1
  }
  if write_prepared_artifacts_manifest "$prepared" "$pristine" "$fixture_root/unexpected.tsv"; then
    echo "fixture accepted unexpected dependency directory" >&2
    exit 1
  fi
  rm -rf "$prepared/unexpected"

  symlink_outside="$fixture_root/symlink-outside"
  symlink_saved="$fixture_root/compat-saved"
  mkdir -p "$symlink_outside"
  printf 'outside sentinel\n' >"$symlink_outside/sentinel"
  mv "$prepared/compat" "$symlink_saved"
  ln -s "$symlink_outside" "$prepared/compat"
  if ! remove_unneeded_prepared_outputs "$prepared" "$pristine"; then
    echo "fixture failed before symlink manifest validation" >&2
    exit 1
  fi
  [[ -f "$symlink_outside/sentinel" ]] || {
    echo "fixture removed outside symlink target" >&2
    exit 1
  }
  if write_prepared_artifacts_manifest "$prepared" "$pristine" "$fixture_root/symlink.tsv"; then
    echo "fixture accepted symlinked dependency parent" >&2
    exit 1
  fi
  rm "$prepared/compat"
  mv "$symlink_saved" "$prepared/compat"

  write_prepared_artifacts_manifest "$prepared" "$pristine" "$expected"
  compare_prepared_artifacts_manifest "$prepared" "$pristine" "$expected"
  generated_graph_saved="$prepared_generated_graph"
  limits_pristine="$fixture_root/limits-pristine"
  limits_prepared="$fixture_root/limits-prepared"
  mkdir -p "$limits_pristine" "$limits_prepared/.depend"
  printf '1234' >"$limits_prepared/.depend/boundary.d"
  prepared_generated_graph='.depend'
  if ! ZMIN_UPSTREAM_MANIFEST_FIXTURE_LIMITS='2:4' \
    write_prepared_artifacts_manifest "$limits_prepared" "$limits_pristine" \
      "$fixture_root/limits-exact.tsv"; then
    echo "generated-output exact node/byte boundary was rejected" >&2
    exit 1
  fi
  printf '5' >>"$limits_prepared/.depend/boundary.d"
  if ZMIN_UPSTREAM_MANIFEST_FIXTURE_LIMITS='2:4' \
    write_prepared_artifacts_manifest "$limits_prepared" "$limits_pristine" \
      "$fixture_root/limits-byte-overflow.tsv"; then
    echo "generated-output byte overflow was accepted" >&2
    exit 1
  fi
  printf '1234' >"$limits_prepared/.depend/boundary.d"
  printf '5678' >"$limits_prepared/.depend/second.d"
  if ZMIN_UPSTREAM_MANIFEST_FIXTURE_LIMITS='2:8' \
    write_prepared_artifacts_manifest "$limits_prepared" "$limits_pristine" \
      "$fixture_root/limits-node-overflow.tsv"; then
    echo "generated-output node overflow was accepted" >&2
    exit 1
  fi
  rm -rf "$limits_prepared/.depend"
  prepared_generated_graph="$generated_graph_saved"
  validate_test_lib_patch_fixture() {
    local old_pristine="$pristine_source_dir"
    local old_source="$source_dir"
    pristine_source_dir="$pristine"
    source_dir="$prepared"
    if ! validate_test_lib_patch; then
      pristine_source_dir="$old_pristine"
      source_dir="$old_source"
      return 1
    fi
    pristine_source_dir="$old_pristine"
    source_dir="$old_source"
  }
  validate_test_lib_patch_fixture
  for target in \
    "$prepared/GIT-PERL-DEFINES" \
    "$prepared/hook-list.h" \
    "$prepared/version-def.h" \
    "$prepared/git-sh-i18n--envsubst" \
    "$prepared/templates/boilerplates.made" \
    "$prepared/t/helper/test-tool-real" \
    "$prepared/perl/build/lib/Git.pm"; do
    saved="$fixture_root/saved"
    cp "$target" "$saved"
    printf 'x' >>"$target"
    if compare_prepared_artifacts_manifest "$prepared" "$pristine" "$expected" 2>/dev/null; then
      echo "prepared manifest accepted one-byte mutation: $target" >&2
      exit 1
    fi
    mv "$saved" "$target"
    extra="$target.extra"
    printf 'unexpected generated extra\n' >"$extra"
    if compare_prepared_artifacts_manifest "$prepared" "$pristine" "$expected" 2>/dev/null; then
      echo "prepared manifest accepted unexpected generated entry: $extra" >&2
      exit 1
    fi
    rm -f "$extra"
  done
  saved="$fixture_root/saved-test-lib"
  cp "$prepared/t/test-lib.sh" "$saved"
  printf 'x' >>"$prepared/t/test-lib.sh"
  if validate_test_lib_patch_fixture 2>/dev/null; then
    echo "prepared manifest accepted one-byte test-lib mutation" >&2
    exit 1
  fi
  mv "$saved" "$prepared/t/test-lib.sh"
  extra="$prepared/t/test-lib.sh.extra"
  printf 'unexpected test-lib companion\n' >"$extra"
  if compare_prepared_artifacts_manifest "$prepared" "$pristine" "$expected" 2>/dev/null; then
    echo "prepared manifest accepted unexpected test-lib companion: $extra" >&2
    exit 1
  fi
  rm -f "$extra"
  printf 'x' >>"$prepared/README"
  if compare_prepared_artifacts_manifest "$prepared" "$pristine" "$expected" 2>/dev/null; then
    echo "prepared manifest accepted static source mutation" >&2
    exit 1
  fi
  printf 'unexpected static source\n' >"$prepared/unexpected-source"
  if compare_prepared_artifacts_manifest "$prepared" "$pristine" "$expected" 2>/dev/null; then
    echo "prepared manifest accepted unexpected static entry" >&2
    exit 1
  fi
  printf 'fixture static source\n' >"$prepared/README"
  rm -f "$prepared/unexpected-source"
  compare_prepared_artifacts_manifest "$prepared" "$pristine" "$expected"

  optional_saved="$fixture_root/t5510-saved"
  cp "$prepared/t/t5510-fetch.sh" "$optional_saved"
  skip_unsupported_reftable=1
  transform_t5510_fetch "$prepared/t/t5510-fetch.sh"
  optional_expected="$fixture_root/optional-artifacts.tsv"
  write_prepared_artifacts_manifest "$prepared" "$pristine" "$optional_expected"
  grep -q '^R[[:space:]]\./t/t5510-fetch.sh[[:space:]]' "$optional_expected"
  compare_prepared_artifacts_manifest "$prepared" "$pristine" "$optional_expected"
  mv "$prepared/t/t5510-fetch.sh" "$fixture_root/t5510-missing"
  if write_prepared_artifacts_manifest "$prepared" "$pristine" "$fixture_root/missing-optional.tsv"; then
    echo "optional manifest accepted missing t5510 transform" >&2
    exit 1
  fi
  mv "$fixture_root/t5510-missing" "$prepared/t/t5510-fetch.sh"
  printf 'tampered optional transform\n' >>"$prepared/t/t5510-fetch.sh"
  if compare_prepared_artifacts_manifest "$prepared" "$pristine" "$optional_expected"; then
    echo "optional transformed artifact mutation was accepted" >&2
    exit 1
  fi
  mv "$optional_saved" "$prepared/t/t5510-fetch.sh"
  skip_unsupported_reftable=0
  compare_prepared_artifacts_manifest "$prepared" "$pristine" "$expected"

  source_dir="$prepared"
  pristine_source_dir="$pristine"
  prepared_artifacts_manifest="$fixture_root/published.tsv"
  prepared_artifacts_marker="$fixture_root/published.sha256"
  publish_prepared_artifacts_manifest
  validate_prepared_artifacts_manifest
  source_manifest_sha256 "$prepared" "$prepared_artifacts_manifest" >/dev/null
  source_manifest_sha256 "$pristine" >/dev/null

  target="$prepared/GIT-PERL-DEFINES"
  saved="$fixture_root/missing-saved"
  mv "$target" "$saved"
  if compare_prepared_artifacts_manifest "$prepared" "$pristine" "$expected"; then
    echo "prepared manifest accepted missing generated entry" >&2
    exit 1
  fi
  mv "$saved" "$target"

  missing_payload="$(sed '1d' "$prepared_artifacts_manifest")"
  published_marker_payload="$(printf '%s\n' "$missing_payload" | shasum -a 256 | awk '{ print $1 }')"
  publish_descriptor_file "$prepared_artifacts_manifest" "$missing_payload"
  publish_descriptor_file "$prepared_artifacts_marker" "$published_marker_payload"
  if validate_prepared_artifacts_manifest; then
    echo "published manifest accepted missing entry" >&2
    exit 1
  fi
  publish_prepared_artifacts_manifest
  validate_prepared_artifacts_manifest

  published_manifest_payload="$(cat "$prepared_artifacts_manifest")"
  publish_descriptor_file "$prepared_artifacts_manifest" "$published_manifest_payload$'\n'F\t./unexpected-generated\t0000000000000000000000000000000000000000000000000000000000000000"
  published_marker_payload="$(shasum -a 256 "$prepared_artifacts_manifest" | awk '{ print $1 }')"
  publish_descriptor_file "$prepared_artifacts_marker" "$published_marker_payload"
  if validate_prepared_artifacts_manifest; then
    echo "published manifest accepted extra entry" >&2
    exit 1
  fi
  publish_prepared_artifacts_manifest
  validate_prepared_artifacts_manifest

  published_marker_payload="$(cat "$prepared_artifacts_marker")"
  publish_descriptor_file "$prepared_artifacts_marker" "0000000000000000000000000000000000000000000000000000000000000000"
  if validate_prepared_artifacts_manifest; then
    echo "published marker mutation was accepted" >&2
    exit 1
  fi
  publish_descriptor_file "$prepared_artifacts_marker" "$published_marker_payload"
  validate_prepared_artifacts_manifest

  sentinel="$fixture_root/publication-sentinel"
  sentinel_expected="$fixture_root/publication-sentinel.expected"
  printf 'sentinel\n' >"$sentinel"
  cp "$sentinel" "$sentinel_expected"
  rm -f "$prepared_artifacts_manifest"
  ln -s "$sentinel" "$prepared_artifacts_manifest"
  if publish_descriptor_file "$prepared_artifacts_manifest" "must-not-follow"; then
    echo "safe manifest publication followed a replaceable symlink" >&2
    exit 1
  fi
  cmp -s "$sentinel" "$sentinel_expected"
  rm -f "$prepared_artifacts_manifest"
  ln "$sentinel" "$prepared_artifacts_manifest"
  publish_descriptor_file "$prepared_artifacts_manifest" "replaced-hardlink"
  cmp -s "$sentinel" "$sentinel_expected"
  [[ "$(cat "$prepared_artifacts_manifest")" == "replaced-hardlink" ]]
  rm -f "$prepared_artifacts_manifest"
  publish_prepared_artifacts_manifest
  validate_prepared_artifacts_manifest

  type_saved="$fixture_root/type-saved"
  cp "$target" "$type_saved"
  rm -f "$target"
  ln -s "$sentinel" "$target"
  if compare_prepared_artifacts_manifest "$prepared" "$pristine" "$expected"; then
    echo "prepared manifest accepted generated symlink replacement" >&2
    exit 1
  fi
  rm -f "$target"
  cp "$type_saved" "$target"
  rm -f "$target"
  mkdir "$target"
  if compare_prepared_artifacts_manifest "$prepared" "$pristine" "$expected"; then
    echo "prepared manifest accepted generated type replacement" >&2
    exit 1
  fi
  rmdir "$target"
  mv "$type_saved" "$target"
  compare_prepared_artifacts_manifest "$prepared" "$pristine" "$expected"
  printf 'lock-rendezvous=%s\n' "$lock_rendezvous_identity"
  printf 'manifest-fixture=pass\n'
)

if [[ "$manifest_fixture_mode" == "1" ]]; then
  run_manifest_fixture
  exit 0
fi

validate_make_identity || {
  echo "authenticated absolute Make identity is required before upstream preparation" >&2
  exit 2
}

if [[ "$stock_git_control" != "1" ]]; then
  ensure_git_http_backend
  if [[ -n "$http_bundle" ]]; then
    if [[ "${RUNNER_OS:-}:${OS:-}" == Windows:* || "${RUNNER_OS:-}:${OS:-}" == *:Windows_NT ]]; then
      http_git_relative="git.exe"
    else
      http_git_relative="git"
    fi
    validated_http_bundle="$(
      ZMIN_UPSTREAM_GIT_CACHE="$cache_root" ZMIN_HTTP_PERL="$perl_bin" \
        ZMIN_GIT_HTTP_BUNDLE="$http_bundle" ZMIN_STOCK_GIT="$http_bundle/$http_git_relative" \
        "$repo_root/tools/git-upstream-http-provenance.sh" validate
    )" || {
      echo "supplied HTTP comparator bundle failed provenance validation" >&2
      exit 2
    }
    [[ "$validated_http_bundle" == "$http_bundle" ]] || {
      echo "supplied HTTP comparator bundle resolved to an unexpected path" >&2
      exit 2
    }
    http_remote_http_relative="$(awk -F '\t' '$1 == "member" && $2 == "git-remote-http" { print $3; count += 1 } END { if (count != 1) exit 1 }' "$http_bundle/bundle.tsv")" || {
      echo "validated HTTP comparator bundle has no unique remote-http member" >&2
      exit 2
    }
    http_backend_relative="$(awk -F '\t' '$1 == "member" && $2 == "git-http-backend" { print $3; count += 1 } END { if (count != 1) exit 1 }' "$http_bundle/bundle.tsv")" || {
      echo "validated HTTP comparator bundle has no unique HTTP backend member" >&2
      exit 2
    }
    zmin_remote_http_helper_name="$http_remote_http_relative"
    export ZMIN_GIT_HTTP_BUNDLE="$http_bundle"
    export ZMIN_STOCK_GIT="$http_bundle/$http_git_relative"
  fi
else
  http_bundle="$(ZMIN_UPSTREAM_GIT_CACHE="$cache_root" \
    ZMIN_HTTP_MAKE="$make_bin" ZMIN_HTTP_PERL="$perl_bin" \
    "$repo_root/tools/git-upstream-http-provenance.sh" build)" || {
    echo "cannot prepare the exact pinned Git HTTP comparator bundle" >&2
    exit 2
  }
  http_git_relative="$(awk -F '\t' '$1 == "member" && $2 == "git" { print $3; count += 1 } END { if (count != 1) exit 1 }' "$http_bundle/bundle.tsv")" || {
    echo "validated HTTP comparator bundle has no unique Git member" >&2
    exit 2
  }
  http_remote_http_relative="$(awk -F '\t' '$1 == "member" && $2 == "git-remote-http" { print $3; count += 1 } END { if (count != 1) exit 1 }' "$http_bundle/bundle.tsv")" || {
    echo "validated HTTP comparator bundle has no unique remote-http member" >&2
    exit 2
  }
  http_backend_relative="$(awk -F '\t' '$1 == "member" && $2 == "git-http-backend" { print $3; count += 1 } END { if (count != 1) exit 1 }' "$http_bundle/bundle.tsv")" || {
    echo "validated HTTP comparator bundle has no unique HTTP backend member" >&2
    exit 2
  }
  zmin_remote_http_helper_name="$http_remote_http_relative"
  ZMIN_HTTP_MAKE="$make_bin" ZMIN_HTTP_PERL="$perl_bin" \
    ZMIN_UPSTREAM_GIT_CACHE="$cache_root" ZMIN_GIT_HTTP_BUNDLE="$http_bundle" \
    ZMIN_STOCK_GIT="$http_bundle/$http_git_relative" \
    "$repo_root/tools/test-git-upstream-http-provenance.sh" || {
      echo "pinned Git HTTP provenance regression failed" >&2
      exit 2
    }
  export ZMIN_GIT_HTTP_BUNDLE="$http_bundle"
  export ZMIN_STOCK_GIT="$http_bundle/$http_git_relative"
fi
prepare_upstream_harness
validate_upstream_prerequisites
refresh_contract_scope
if [[ "$prepare_only" == "1" ]]; then
  printf 'prepared_source=%s\n' "$source_dir"
  printf 'http_bundle=%s\n' "$http_bundle"
  printf 'perl=%s\n' "$perl_bin"
  printf 'make=%s\n' "$make_bin"
  printf 'make_version=%s\n' "$make_version"
  printf 'make_sha256=%s\n' "$make_sha256"
  printf 'perl_module=%s\n' "$source_dir/perl/build/lib/Git.pm"
  printf 'test_tool=%s\n' "$(test_tool_exec_path)"
  printf 'test_tool_real=%s\n' "$(test_tool_real_path)"
  printf 'test_tool_exec_sha256=%s\n' "$(shasum -a 256 "$(test_tool_exec_path)" | awk '{ print $1 }')"
  printf 'platform_helper=%s\n' "$(platform_helper_path)"
  printf 'zmin_bin=%s\n' "$zmin_bin"
  printf 'zmin_bin_sha256=%s\n' "$zmin_bin_sha256"
  printf 'zmin_version=%s\n' "$zmin_version"
  printf 'zmin_profile=%s\n' "$zmin_profile"
  printf 'zmin_binary_trust=%s\n' "$zmin_binary_trust"
  printf 'zmin_trust_binding=%s\n' "$zmin_trust_binding"
  printf 'zmin_current_contract_detail=%s\n' "$zmin_current_contract_detail"
  printf 'archive_commit_binding=%s\n' "$archive_commit_binding"
  exit 0
fi
resolve_test_list

if [[ "$stock_git_control" == "1" ]]; then
  if [[ -n "$http_bundle" && -n "$http_git_relative" && -x "$http_bundle/$http_git_relative" ]]; then
    stock_git="$http_bundle/$http_git_relative"
  else
    echo "missing manifest-selected Git member in the validated HTTP comparator bundle" >&2
    exit 2
  fi
  zmin_bin="$stock_git"
  shim_dir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-upstream-stock-http-shim.XXXXXX")"
  trap 'rm -rf "$shim_dir"' EXIT
  make_stock_git_http_shim "$shim_dir"
else
  shim_dir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-upstream-git-shim.XXXXXX")"
  trap 'rm -rf "$shim_dir"' EXIT
  ZMIN_UPSTREAM_LOCALEDIR="$source_dir/po/build/locale" make_git_shim "$shim_dir"
fi
if [[ -z "$zmin_bin_sha256" && -f "$zmin_bin" && ! -L "$zmin_bin" ]]; then
  zmin_bin_sha256="$(shasum -a 256 "$zmin_bin" | awk '{ print $1 }')"
fi
if [[ -z "$zmin_version" && -x "$zmin_bin" && ! -L "$zmin_bin" ]]; then
  zmin_version="$("$zmin_bin" --version 2>&1)"
fi

summary="$out_dir/summary.tsv"
metadata="$out_dir/run-metadata.tsv"
: >"$summary"
printf 'mode\ttest\tstatus\treason\tlog\n' >>"$summary"

total=0
passed=0
failed=0
bounded_stop_after=""

if [[ "$bounded_run" == "1" ]]; then
  run_list="$(run_list_from_flags)" || {
    echo "ZMIN_UPSTREAM_BOUNDED_RUN=1 requires ZMIN_UPSTREAM_TEST_FLAGS to include --run" >&2
    exit 2
  }
  bounded_stop_after="$(max_numeric_run_selector "$run_list")"
printf 'bounded_run_stop_after=%s\n' "$bounded_stop_after"
fi

printf 'manifest_offset=%s\n' "$manifest_offset"
printf 'manifest_limit=%s\n' "$manifest_limit"

while IFS=$'\t' read -r test_name test_mode reason; do
  [[ -n "$test_name" ]] || continue
  total=$((total + 1))
  log="$out_dir/${test_name%.sh}.log"
  trash_dir="$source_dir/t/trash directory.${test_name%.sh}"
  printf 'upstream git compat: %s (%s)\n' "$test_name" "$reason"
  if [[ -e "$trash_dir" || -L "$trash_dir" ]]; then
    if [[ ! -d "$trash_dir" || -L "$trash_dir" ]]; then
      echo "upstream trash path is not a real directory: $trash_dir" >&2
      exit 1
    fi
    chmod -R u+w "$trash_dir" || {
      echo "failed to make stale upstream trash dir writable: $trash_dir" >&2
      exit 1
    }
    rm -rf "$trash_dir" || {
      echo "failed to remove stale upstream trash dir: $trash_dir" >&2
      exit 1
    }
 fi
  set +e
  if [[ "${RUNNER_OS:-}" == "Windows" || "${OS:-}" == "Windows_NT" ]]; then
    (
      cd "$source_dir/t"
      ZMIN_UPSTREAM_STOP_AFTER_TEST="$bounded_stop_after" \
      ZMIN_UPSTREAM_TEST_TOOL_TRACE2="$([[ "$stock_git_control" == "1" ]] && printf 0 || printf 1)" \
      ZMIN_GIT_REMOTE_HTTP="$shim_dir/$zmin_remote_http_helper_name" \
      GIT_TEST_DEFAULT_HASH=sha1 \
      GIT_TEST_INSTALLED="$shim_dir" \
      GIT_EXEC_PATH="$shim_dir" \
      run_test_with_timeout sh "$test_name" $test_flags
    ) >"$log" 2>&1
    rc=$?
  else
  (
    cd "$source_dir/t"
    ZMIN_UPSTREAM_STOP_AFTER_TEST="$bounded_stop_after" \
    ZMIN_UPSTREAM_TEST_TOOL_TRACE2="$([[ "$stock_git_control" == "1" ]] && printf 0 || printf 1)" \
    ZMIN_GIT_REMOTE_HTTP="$shim_dir/$zmin_remote_http_helper_name" \
    GIT_TEST_DEFAULT_HASH=sha1 \
    GIT_TEST_INSTALLED="$shim_dir" \
    GIT_EXEC_PATH="$shim_dir" \
    run_test_with_timeout sh "$test_name" $test_flags
  ) >"$log" 2>&1
  rc=$?
  fi
  set -e
  if [[ "$rc" == "0" ]]; then
    passed=$((passed + 1))
    printf '%s\t%s\tpass\t%s\t%s\n' "$test_mode" "$test_name" "$reason" "$log" >>"$summary"
  elif todo_breakage_vanished_only "$log"; then
    passed=$((passed + 1))
    printf '%s\t%s\tpass\t%s (upstream TODO breakage vanished only)\t%s\n' \
      "$test_mode" "$test_name" "$reason" "$log" >>"$summary"
  else
    failed=$((failed + 1))
    printf '%s\t%s\tfail\t%s\t%s\n' "$test_mode" "$test_name" "$reason" "$log" >>"$summary"
    tail -n 40 "$log" >&2 || true
  fi
done < <(selected_tests)

evidence_scope="exploratory"
if [[ "$contract_scope" == "current-contract" ]]; then
  if [[ "$total" == "$contract_denominator" && "$passed" == "$contract_denominator" && "$failed" == "0" ]]; then
    evidence_scope="authoritative-upstream-suite"
  else
    evidence_scope="authoritative-suite-incomplete"
  fi
fi
manifest_test_count="$(awk -F '\t' 'NR > 1 { count += 1 } END { print count + 0 }' "$resolved_test_list")"
manifest_sha256="$(awk -F '\t' 'NR > 1 { print $2 }' "$resolved_test_list" | shasum -a 256 | awk '{ print $1 }')"
summary_sha256="$(shasum -a 256 "$summary" | awk '{ print $1 }')"
{
  printf 'key\tvalue\n'
  printf 'upstream_git_tag\t%s\n' "$tag"
  printf 'upstream_git_commit\t%s\n' "$reported_upstream_commit"
  printf 'upstream_archive_sha256\t%s\n' "$verified_archive_sha"
  printf 'upstream_commit_binding\t%s\n' "$archive_commit_binding"
  printf 'perl_path\t%s\n' "$perl_bin"
  printf 'perl_version\t%s\n' "$perl_version"
  printf 'make_path\t%s\n' "$make_bin"
  printf 'make_version\t%s\n' "$make_version"
  printf 'make_sha256\t%s\n' "$make_sha256"
  printf 'test_tool_real\t%s\n' "$(test_tool_real_path)"
  printf 'test_tool_exec\t%s\n' "$(test_tool_exec_path)"
  if [[ -x "$(test_tool_exec_path)" && ! -L "$(test_tool_exec_path)" ]]; then
    printf 'test_tool_exec_sha256\t%s\n' "$(shasum -a 256 "$(test_tool_exec_path)" | awk '{ print $1 }')"
  else
    printf 'test_tool_exec_sha256\t\n'
  fi
  printf 'platform_helper\t%s\n' "$(platform_helper_path)"
  if [[ -x "$(platform_helper_path)" && ! -L "$(platform_helper_path)" ]]; then
    printf 'platform_helper_sha256\t%s\n' "$(shasum -a 256 "$(platform_helper_path)" | awk '{ print $1 }')"
  else
    printf 'platform_helper_sha256\t\n'
  fi
  printf 'zmin_bin\t%s\n' "$zmin_bin"
  printf 'zmin_bin_sha256\t%s\n' "$zmin_bin_sha256"
  printf 'zmin_version\t%s\n' "$zmin_version"
  printf 'zmin_profile\t%s\n' "$zmin_profile"
  printf 'zmin_binary_trust\t%s\n' "$zmin_binary_trust"
  printf 'zmin_trust_binding\t%s\n' "$zmin_trust_binding"
  printf 'zmin_current_contract_detail\t%s\n' "$zmin_current_contract_detail"
  printf 'zmin_trust_reason\t%s\n' "$zmin_trust_reason"
  printf 'zmin_identity_sidecar\t%s\n' "$zmin_identity_path"
  printf 'zmin_identity_sidecar_sha256\t%s\n' "$zmin_identity_sha256"
  printf 'mode\t%s\n' "$mode"
  printf 'scope\t%s\n' "$contract_scope"
  printf 'evidence_scope\t%s\n' "$evidence_scope"
  printf 'compatibility_claim\tunverified\n'
  printf 'manifest_file\t%s\n' "$resolved_test_list"
  printf 'manifest_sha256\t%s\n' "$manifest_sha256"
  printf 'manifest_test_count\t%s\n' "$manifest_test_count"
  printf 'summary_file\t%s\n' "$summary"
  printf 'summary_sha256\t%s\n' "$summary_sha256"
  printf 'manifest_offset\t%s\n' "$manifest_offset"
  printf 'manifest_limit\t%s\n' "$manifest_limit"
  printf 'total\t%s\n' "$total"
  printf 'passed\t%s\n' "$passed"
  printf 'failed\t%s\n' "$failed"
} >"$metadata"

printf 'upstream_git_tag=%s\n' "$tag"
printf 'mode=%s\n' "$mode"
printf 'scope=%s\n' "$contract_scope"
printf 'evidence_scope=%s\n' "$evidence_scope"
printf 'total=%s\n' "$total"
printf 'passed=%s\n' "$passed"
printf 'failed=%s\n' "$failed"
printf 'summary=%s\n' "$summary"
printf 'metadata=%s\n' "$metadata"

if [[ "$failed" != "0" && "${ZMIN_UPSTREAM_ALLOW_FAILURES:-0}" != "1" ]]; then
  exit 1
fi
