#!/usr/bin/env bash

benchmark_resolve_python() {
  local candidate="${ZMIN_BENCH_PYTHON_BIN:-/usr/bin/python3}"
  if [[ "$candidate" != /* ]]; then
    printf 'benchmark Python interpreter must be an absolute path: %s\n' "$candidate" >&2
    return 1
  fi
  if [[ ! -f "$candidate" || ! -x "$candidate" ]]; then
    printf 'benchmark Python interpreter is not executable: %s\n' "$candidate" >&2
    return 1
  fi
  printf '%s\n' "$candidate"
}

benchmark_resolve_observed_git() {
  local candidate="${ZMIN_STOCK_GIT:-${GIT_BIN:-}}"
  if [[ -n "$candidate" ]]; then
    printf '%s\n' "$candidate"
    return 0
  fi
  if [[ -x /usr/bin/git ]]; then
    printf '%s\n' /usr/bin/git
    return 0
  fi
  command -v git
}

benchmark_authoritative_trace_preflight() {
  local evidence_mode="$1"
  shift
  if [[ "$evidence_mode" != "authoritative" ]]; then
    return 0
  fi

  local explicit_option
  for explicit_option in "$@"; do
    if [[ -n "$explicit_option" ]]; then
      printf 'authoritative mode forbids diagnostic tracing: %s\n' "$explicit_option" >&2
      return 1
    fi
  done

  local env_name
  for env_name in $(compgen -e); do
    case "$env_name" in
      GIT_TRACE*|ZMIN_*TRACE*|ZMIN_*TRACE_DIR|ZMIN_*PACKET_TRACE*)
        if [[ -n "${!env_name:-}" ]]; then
          printf 'authoritative mode forbids diagnostic tracing environment: %s\n' "$env_name" >&2
          return 1
        fi
        ;;
    esac
  done
}

benchmark_validate_authoritative_git_comparator() {
  local repo_root="$1"
  local git_bin="$2"
  local bundle="${ZMIN_GIT_HTTP_BUNDLE:-}"
  local expected_git="${ZMIN_STOCK_GIT:-}"
  local git_name=git
  local helper

  if [[ -z "$bundle" ]]; then
    printf 'authoritative benchmark requires explicit ZMIN_GIT_HTTP_BUNDLE\n' >&2
    return 1
  fi
  if [[ -n "$expected_git" && "$expected_git" != "$git_bin" ]]; then
    printf 'authoritative comparator mismatch: ZMIN_STOCK_GIT must equal the selected Git binary\n' >&2
    return 1
  fi
  case "${RUNNER_OS:-${OS:-}}:$(uname -s 2>/dev/null || true)" in
    Windows*:*|*:MINGW*|*:MSYS*|*:CYGWIN*) git_name=git.exe ;;
  esac
  if [[ "$git_bin" != "$bundle/$git_name" ]]; then
    printf 'authoritative comparator must be the canonical pinned bundle Git: %s\n' "$bundle/$git_name" >&2
    return 1
  fi
  for helper in "git-remote-http${git_name#git}" "git-http-backend${git_name#git}"; do
    if [[ ! -f "$bundle/$helper" || -L "$bundle/$helper" || ! -x "$bundle/$helper" ]]; then
      printf 'authoritative comparator helper is missing or invalid: %s\n' "$bundle/$helper" >&2
      return 1
    fi
  done
  if [[ ! -x "$repo_root/tools/git-upstream-http-provenance.sh" ]]; then
    printf 'authoritative comparator provenance validator is missing\n' >&2
    return 1
  fi
  ZMIN_GIT_HTTP_BUNDLE="$bundle" \
    ZMIN_STOCK_GIT="$git_bin" \
    "$repo_root/tools/git-upstream-http-provenance.sh" validate >/dev/null || {
      printf 'authoritative comparator provenance validation failed for Git v2.55.0 bundle\n' >&2
      return 1
    }
  printf '%s\n' "$bundle"
}

benchmark_sanitize_environment() {
  local sandbox_root="$1"
  local git_bin="$2"
  local zmin_bin="$3"
  local python_bin="$4"
  local preserve_network="${ZMIN_BENCH_PRESERVE_NETWORK_ENV:-0}"
  local git_dir zmin_dir python_dir env_name rejected_csv
  local rejected=()

  if [[ "$python_bin" != /* || ! -f "$python_bin" || ! -x "$python_bin" ]]; then
    printf 'benchmark sanitizer requires an executable absolute Python interpreter: %s\n' "$python_bin" >&2
    return 1
  fi

  git_dir="$(cd "$(dirname "$git_bin")" && pwd)"
  zmin_dir="$(cd "$(dirname "$zmin_bin")" && pwd)"
  python_dir="$(cd "$(dirname "$python_bin")" && pwd)"
  export PATH="$python_dir:$zmin_dir:$git_dir:/usr/bin:/bin:/usr/sbin:/sbin"

  mkdir -p "$sandbox_root/home" "$sandbox_root/tmp"
  : >"$sandbox_root/gitconfig"

  for env_name in $(compgen -e); do
    case "$env_name" in
      SSL_CERT_FILE|SSL_CERT_DIR|CURL_CA_BUNDLE|HTTP_PROXY|HTTPS_PROXY|ALL_PROXY|NO_PROXY|http_proxy|https_proxy|all_proxy|no_proxy)
        if [[ "$preserve_network" == "1" ]]; then
          continue
        fi
        ;;
    esac
    rejected+=("$env_name")
    unset "$env_name"
  done

  export PATH="$python_dir:$zmin_dir:$git_dir:/usr/bin:/bin:/usr/sbin:/sbin"
  rejected_csv=""
  if [[ "${#rejected[@]}" -gt 0 ]]; then
    rejected_csv="$(printf '%s\n' "${rejected[@]}" | LC_ALL=C sort -u | paste -sd, -)"
  fi

  export PATH="$python_dir:$zmin_dir:$git_dir:/usr/bin:/bin:/usr/sbin:/sbin"
  export HOME="$sandbox_root/home"
  export TMPDIR="$sandbox_root/tmp"
  export LANG=C
  export LC_ALL=C
  export LC_CTYPE=C
  export TZ=UTC
  export USER=benchmark
  export LOGNAME=benchmark
  export GIT_CONFIG_GLOBAL="$sandbox_root/gitconfig"
  export GIT_CONFIG_NOSYSTEM=1
  export GIT_OPTIONAL_LOCKS=0
  export GIT_TERMINAL_PROMPT=0
  export PYTHONHASHSEED=0
  export ZMIN_BENCH_REJECTED_ENV="${rejected_csv:-none}"
  if [[ "$preserve_network" == "1" ]]; then
    export ZMIN_BENCH_NETWORK_ENV_POLICY=preserved
  else
    export ZMIN_BENCH_NETWORK_ENV_POLICY=rejected
  fi
}
