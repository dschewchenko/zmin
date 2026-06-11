#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
smoke_script="$repo_root/tools/git-real-repo-smoke.sh"

skron_bin="${SKRON_BIN:-}"
if [[ -z "$skron_bin" ]]; then
  rustup run stable cargo build --manifest-path "$repo_root/Cargo.toml" --release -p skron-cli --bin skron-git >/dev/null
  skron_bin="$repo_root/target/release/skron-git"
elif [[ "$skron_bin" != /* ]]; then
  if command -v realpath >/dev/null 2>&1; then
    skron_bin="$(realpath "$skron_bin")"
  else
    skron_bin="$(cd "$repo_root" && cd "$(dirname "$skron_bin")" && pwd)/$(basename "$skron_bin")"
  fi
fi

if [[ "${RUNNER_OS:-}" == "Windows" || "${OS:-}" == "Windows_NT" ]]; then
  if [[ ! -x "$skron_bin" && -x "${skron_bin}.exe" ]]; then
    skron_bin="${skron_bin}.exe"
  fi
else
  if [[ ! -x "$skron_bin" && -x "${skron_bin}.exe" ]]; then
    skron_bin="${skron_bin}.exe"
  fi
fi

provider_only=",${SKRON_PROVIDER_ONLY:-},"
allow_skip="${SKRON_PROVIDER_ALLOW_SKIP:-0}"
remote_only="${SKRON_PROVIDER_REMOTE_ONLY:-0}"
retry_count="${SKRON_PROVIDER_RETRIES:-2}"
retry_delay="${SKRON_PROVIDER_RETRY_DELAY_SECONDS:-3}"
tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

run_with_retries() {
  local name="$1"
  shift
  local attempts=0
  local delay="$retry_delay"

  while :; do
    attempts=$((attempts + 1))
    if "$@"; then
      if (( attempts > 1 )); then
        echo "retry: $name succeeded on attempt $attempts"
      fi
      return 0
    fi

    local rc=$?
    if (( attempts >= retry_count )); then
      echo "failed: $name (attempt $attempts/$retry_count)" >&2
      return "$rc"
    fi

    echo "retrying: $name (attempt $attempts/$retry_count) in ${delay}s" >&2
    sleep "$delay"
  done
}

providers=(
  "github|https://github.com/octocat/Hello-World.git"
  "gitlab|https://gitlab.com/gitlab-examples/minimal-ruby-app.git"
  "bitbucket|https://bitbucket.org/atlassianlabs/atlascode.git"
  "gitea|https://gitea.com/gitea/tea.git"
  "forgejo|https://codeberg.org/Codeberg/Documentation.git"
)

if [[ -n "${SKRON_PROVIDER_AZURE_URL:-}" ]]; then
  providers+=("azure-devops|$SKRON_PROVIDER_AZURE_URL")
fi

failures=0
ran=0

for entry in "${providers[@]}"; do
  provider="${entry%%|*}"
  url="${entry#*|}"

  if [[ "$provider_only" != ",," && "$provider_only" != *",$provider,"* ]]; then
    continue
  fi

  ran=$((ran + 1))
  echo "provider smoke: $provider $url"

  if ! run_with_retries "$provider ls-remote" bash -lc "GIT_TERMINAL_PROMPT=0 git ls-remote \"$url\" HEAD >/dev/null"; then
    if [[ "$allow_skip" == "1" ]]; then
      echo "skip: $provider is not reachable without interactive credentials"
      continue
    fi
    echo "failed: $provider is not reachable without interactive credentials" >&2
    failures=$((failures + 1))
    continue
  fi

  if [[ "$remote_only" == "1" ]]; then
    git_refs="$tmp_dir/$provider.git.refs"
    skron_refs="$tmp_dir/$provider.skron.refs"
    if ! run_with_retries "$provider git ls-remote --refs" bash -lc "GIT_TERMINAL_PROMPT=0 git ls-remote --refs \"$url\" >\"$git_refs\""; then
      if [[ "$allow_skip" == "1" ]]; then
        echo "skip: $provider is not reachable without interactive credentials"
        continue
      fi
      echo "failed: $provider is not reachable without interactive credentials" >&2
      failures=$((failures + 1))
      continue
    fi

    if ! run_with_retries "$provider skron ls-remote --refs" bash -lc "GIT_TERMINAL_PROMPT=0 \"$skron_bin\" ls-remote --refs \"$url\" >\"$skron_refs\""; then
      if [[ "$allow_skip" == "1" ]]; then
        echo "skip: $provider skron-cli unreachable in this environment"
        continue
      fi
      echo "failed: $provider skron ls-remote --refs mismatch/unreachable" >&2
      failures=$((failures + 1))
      continue
    fi
    if ! diff -u "$git_refs" "$skron_refs"; then
      echo "failed: $provider remote refs mismatch" >&2
      failures=$((failures + 1))
      continue
    fi
    echo "passed: $provider remote refs"
    continue
  fi

  if ! run_with_retries "$provider real-repo-smoke" bash -c "GIT_TERMINAL_PROMPT=0 SKRON_BIN=\"$skron_bin\" \"$smoke_script\" \"$url\""; then
    echo "failed: $provider smoke mismatch" >&2
    failures=$((failures + 1))
    continue
  fi

  echo "passed: $provider"
done

if [[ "$ran" == "0" ]]; then
  echo "no providers selected" >&2
  exit 2
fi

if [[ "$failures" != "0" ]]; then
  echo "provider smoke failed: $failures provider(s)" >&2
  exit 1
fi

echo "provider smoke passed: $ran provider(s)"
