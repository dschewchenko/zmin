#!/usr/bin/env bash
set -euo pipefail

repo_url="${1:-https://github.com/octocat/Hello-World.git}"

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
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

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

GIT_TERMINAL_PROMPT=0 git ls-remote --refs "$repo_url" >"$tmp_dir/git-remote-refs.out"
GIT_TERMINAL_PROMPT=0 "$skron_bin" ls-remote --refs "$repo_url" >"$tmp_dir/skron-remote-refs.out"
diff -u "$tmp_dir/git-remote-refs.out" "$tmp_dir/skron-remote-refs.out"
echo "ok: remote ls-remote --refs"

repo_dir="$tmp_dir/repo"
git clone --quiet "$repo_url" "$repo_dir"
first_file="$(git -C "$repo_dir" ls-tree -r --name-only HEAD | head -n 1)"
if [[ -z "$first_file" ]]; then
  echo "repository has no files at HEAD: $repo_url" >&2
  exit 1
fi

compare() {
  local label="$1"
  shift
  local git_out="$tmp_dir/git.out"
  local skron_out="$tmp_dir/skron.out"

  git -C "$repo_dir" "$@" >"$git_out"
  (cd "$repo_dir" && "$skron_bin" "$@") >"$skron_out"
  if ! diff -u "$git_out" "$skron_out"; then
    echo "mismatch: $label" >&2
    echo "repo: $repo_url" >&2
    echo "command: $*" >&2
    exit 1
  fi
  echo "ok: $label"
}

compare_stdin() {
  local label="$1"
  local input="$2"
  shift 2
  local git_out="$tmp_dir/git.out"
  local skron_out="$tmp_dir/skron.out"

  printf '%s' "$input" | git -C "$repo_dir" "$@" >"$git_out"
  (cd "$repo_dir" && printf '%s' "$input" | "$skron_bin" "$@") >"$skron_out"
  if ! diff -u "$git_out" "$skron_out"; then
    echo "mismatch: $label" >&2
    echo "repo: $repo_url" >&2
    echo "command: $*" >&2
    exit 1
  fi
  echo "ok: $label"
}

compare_status() {
  local label="$1"
  shift
  local git_status
  local skron_status

  set +e
  git -C "$repo_dir" "$@" >/dev/null 2>&1
  git_status="$?"
  (cd "$repo_dir" && "$skron_bin" "$@" >/dev/null 2>&1)
  skron_status="$?"
  set -e
  if [[ "$git_status" != "$skron_status" ]]; then
    echo "status mismatch: $label" >&2
    echo "repo: $repo_url" >&2
    echo "command: $*" >&2
    echo "git=$git_status skron=$skron_status" >&2
    exit 1
  fi
  echo "ok: $label"
}

compare_mutation() {
  local git_repo="$tmp_dir/git-mutation"
  local skron_repo="$tmp_dir/skron-mutation"

  git clone --quiet "$repo_dir" "$git_repo"
  git clone --quiet "$repo_dir" "$skron_repo"
  git -C "$git_repo" config user.name Bench
  git -C "$git_repo" config user.email bench@example.test
  git -C "$git_repo" config commit.gpgsign false
  git -C "$skron_repo" config user.name Bench
  git -C "$skron_repo" config user.email bench@example.test
  git -C "$skron_repo" config commit.gpgsign false

  printf 'worktree diff\n' >>"$git_repo/$first_file"
  printf 'worktree diff\n' >>"$skron_repo/$first_file"
  git -C "$git_repo" diff --name-status >"$tmp_dir/git-worktree-diff.out"
  (cd "$skron_repo" && "$skron_bin" diff --name-status) >"$tmp_dir/skron-worktree-diff.out"
  diff -u "$tmp_dir/git-worktree-diff.out" "$tmp_dir/skron-worktree-diff.out"
  echo "ok: diff --name-status mutation"
  git -C "$git_repo" diff --name-only >"$tmp_dir/git-worktree-diff-names.out"
  (cd "$skron_repo" && "$skron_bin" diff --name-only) >"$tmp_dir/skron-worktree-diff-names.out"
  diff -u "$tmp_dir/git-worktree-diff-names.out" "$tmp_dir/skron-worktree-diff-names.out"
  echo "ok: diff --name-only mutation"
  git -C "$git_repo" diff --stat >"$tmp_dir/git-worktree-diff-stat.out"
  (cd "$skron_repo" && "$skron_bin" diff --stat) >"$tmp_dir/skron-worktree-diff-stat.out"
  diff -u "$tmp_dir/git-worktree-diff-stat.out" "$tmp_dir/skron-worktree-diff-stat.out"
  echo "ok: diff --stat mutation"
  git -C "$git_repo" diff --numstat >"$tmp_dir/git-worktree-diff-numstat.out"
  (cd "$skron_repo" && "$skron_bin" diff --numstat) >"$tmp_dir/skron-worktree-diff-numstat.out"
  diff -u "$tmp_dir/git-worktree-diff-numstat.out" "$tmp_dir/skron-worktree-diff-numstat.out"
  echo "ok: diff --numstat mutation"
  git -C "$git_repo" diff --shortstat >"$tmp_dir/git-worktree-diff-shortstat.out"
  (cd "$skron_repo" && "$skron_bin" diff --shortstat) >"$tmp_dir/skron-worktree-diff-shortstat.out"
  diff -u "$tmp_dir/git-worktree-diff-shortstat.out" "$tmp_dir/skron-worktree-diff-shortstat.out"
  echo "ok: diff --shortstat mutation"
  git -C "$git_repo" diff --raw >"$tmp_dir/git-worktree-diff-raw.out"
  (cd "$skron_repo" && "$skron_bin" diff --raw) >"$tmp_dir/skron-worktree-diff-raw.out"
  diff -u "$tmp_dir/git-worktree-diff-raw.out" "$tmp_dir/skron-worktree-diff-raw.out"
  echo "ok: diff --raw mutation"
  git -C "$git_repo" diff --summary >"$tmp_dir/git-worktree-diff-summary.out"
  (cd "$skron_repo" && "$skron_bin" diff --summary) >"$tmp_dir/skron-worktree-diff-summary.out"
  diff -u "$tmp_dir/git-worktree-diff-summary.out" "$tmp_dir/skron-worktree-diff-summary.out"
  echo "ok: diff --summary mutation"
  git -C "$git_repo" diff >"$tmp_dir/git-worktree-diff-patch.out"
  (cd "$skron_repo" && "$skron_bin" diff) >"$tmp_dir/skron-worktree-diff-patch.out"
  diff -u "$tmp_dir/git-worktree-diff-patch.out" "$tmp_dir/skron-worktree-diff-patch.out"
  echo "ok: diff patch mutation"
  git -C "$git_repo" diff --name-status "$first_file" >"$tmp_dir/git-worktree-diff-path.out"
  (cd "$skron_repo" && "$skron_bin" diff --name-status "$first_file") >"$tmp_dir/skron-worktree-diff-path.out"
  diff -u "$tmp_dir/git-worktree-diff-path.out" "$tmp_dir/skron-worktree-diff-path.out"
  echo "ok: diff pathspec mutation"
  git -C "$git_repo" diff --name-status HEAD >"$tmp_dir/git-worktree-diff-head.out"
  (cd "$skron_repo" && "$skron_bin" diff --name-status HEAD) >"$tmp_dir/skron-worktree-diff-head.out"
  diff -u "$tmp_dir/git-worktree-diff-head.out" "$tmp_dir/skron-worktree-diff-head.out"
  echo "ok: diff HEAD --name-status mutation"
  git -C "$git_repo" diff --stat HEAD >"$tmp_dir/git-worktree-diff-head-stat.out"
  (cd "$skron_repo" && "$skron_bin" diff --stat HEAD) >"$tmp_dir/skron-worktree-diff-head-stat.out"
  diff -u "$tmp_dir/git-worktree-diff-head-stat.out" "$tmp_dir/skron-worktree-diff-head-stat.out"
  echo "ok: diff HEAD --stat mutation"
  git -C "$git_repo" diff HEAD -- "$first_file" >"$tmp_dir/git-worktree-diff-head-path.out"
  (cd "$skron_repo" && "$skron_bin" diff HEAD -- "$first_file") >"$tmp_dir/skron-worktree-diff-head-path.out"
  diff -u "$tmp_dir/git-worktree-diff-head-path.out" "$tmp_dir/skron-worktree-diff-head-path.out"
  echo "ok: diff HEAD pathspec mutation"
  set +e
  git -C "$git_repo" diff --quiet >/dev/null 2>&1
  local git_worktree_quiet="$?"
  (cd "$skron_repo" && "$skron_bin" diff --quiet >/dev/null 2>&1)
  local skron_worktree_quiet="$?"
  set -e
  if [[ "$git_worktree_quiet" != "$skron_worktree_quiet" ]]; then
    echo "status mismatch: diff --quiet mutation" >&2
    echo "git=$git_worktree_quiet skron=$skron_worktree_quiet" >&2
    exit 1
  fi
  echo "ok: diff --quiet mutation"

  printf 'skron smoke\n' >"$git_repo/skron-smoke.txt"
  printf 'skron smoke\n' >"$skron_repo/skron-smoke.txt"
  git -C "$git_repo" add -A
  (cd "$skron_repo" && "$skron_bin" add -A)
  git -C "$git_repo" diff --cached --name-status >"$tmp_dir/git-cached-diff.out"
  (cd "$skron_repo" && "$skron_bin" diff --cached --name-status) >"$tmp_dir/skron-cached-diff.out"
  diff -u "$tmp_dir/git-cached-diff.out" "$tmp_dir/skron-cached-diff.out"
  echo "ok: diff --cached --name-status mutation"
  git -C "$git_repo" diff --cached --name-status HEAD >"$tmp_dir/git-cached-diff-head.out"
  (cd "$skron_repo" && "$skron_bin" diff --cached --name-status HEAD) >"$tmp_dir/skron-cached-diff-head.out"
  diff -u "$tmp_dir/git-cached-diff-head.out" "$tmp_dir/skron-cached-diff-head.out"
  echo "ok: diff --cached HEAD --name-status mutation"
  git -C "$git_repo" diff --cached --stat HEAD >"$tmp_dir/git-cached-diff-head-stat.out"
  (cd "$skron_repo" && "$skron_bin" diff --cached --stat HEAD) >"$tmp_dir/skron-cached-diff-head-stat.out"
  diff -u "$tmp_dir/git-cached-diff-head-stat.out" "$tmp_dir/skron-cached-diff-head-stat.out"
  echo "ok: diff --cached HEAD --stat mutation"
  git -C "$git_repo" diff --cached --name-only >"$tmp_dir/git-cached-diff-names.out"
  (cd "$skron_repo" && "$skron_bin" diff --cached --name-only) >"$tmp_dir/skron-cached-diff-names.out"
  diff -u "$tmp_dir/git-cached-diff-names.out" "$tmp_dir/skron-cached-diff-names.out"
  echo "ok: diff --cached --name-only mutation"
  git -C "$git_repo" diff --cached --stat >"$tmp_dir/git-cached-diff-stat.out"
  (cd "$skron_repo" && "$skron_bin" diff --cached --stat) >"$tmp_dir/skron-cached-diff-stat.out"
  diff -u "$tmp_dir/git-cached-diff-stat.out" "$tmp_dir/skron-cached-diff-stat.out"
  echo "ok: diff --cached --stat mutation"
  git -C "$git_repo" diff --cached --numstat >"$tmp_dir/git-cached-diff-numstat.out"
  (cd "$skron_repo" && "$skron_bin" diff --cached --numstat) >"$tmp_dir/skron-cached-diff-numstat.out"
  diff -u "$tmp_dir/git-cached-diff-numstat.out" "$tmp_dir/skron-cached-diff-numstat.out"
  echo "ok: diff --cached --numstat mutation"
  git -C "$git_repo" diff --cached --shortstat >"$tmp_dir/git-cached-diff-shortstat.out"
  (cd "$skron_repo" && "$skron_bin" diff --cached --shortstat) >"$tmp_dir/skron-cached-diff-shortstat.out"
  diff -u "$tmp_dir/git-cached-diff-shortstat.out" "$tmp_dir/skron-cached-diff-shortstat.out"
  echo "ok: diff --cached --shortstat mutation"
  git -C "$git_repo" diff --cached --raw >"$tmp_dir/git-cached-diff-raw.out"
  (cd "$skron_repo" && "$skron_bin" diff --cached --raw) >"$tmp_dir/skron-cached-diff-raw.out"
  diff -u "$tmp_dir/git-cached-diff-raw.out" "$tmp_dir/skron-cached-diff-raw.out"
  echo "ok: diff --cached --raw mutation"
  git -C "$git_repo" diff --cached --summary >"$tmp_dir/git-cached-diff-summary.out"
  (cd "$skron_repo" && "$skron_bin" diff --cached --summary) >"$tmp_dir/skron-cached-diff-summary.out"
  diff -u "$tmp_dir/git-cached-diff-summary.out" "$tmp_dir/skron-cached-diff-summary.out"
  echo "ok: diff --cached --summary mutation"
  git -C "$git_repo" diff --cached >"$tmp_dir/git-cached-diff-patch.out"
  (cd "$skron_repo" && "$skron_bin" diff --cached) >"$tmp_dir/skron-cached-diff-patch.out"
  diff -u "$tmp_dir/git-cached-diff-patch.out" "$tmp_dir/skron-cached-diff-patch.out"
  echo "ok: diff --cached patch mutation"
  git -C "$git_repo" diff --cached --name-status skron-smoke.txt >"$tmp_dir/git-cached-diff-path.out"
  (cd "$skron_repo" && "$skron_bin" diff --cached --name-status skron-smoke.txt) >"$tmp_dir/skron-cached-diff-path.out"
  diff -u "$tmp_dir/git-cached-diff-path.out" "$tmp_dir/skron-cached-diff-path.out"
  echo "ok: diff --cached pathspec mutation"
  set +e
  git -C "$git_repo" diff --cached --quiet >/dev/null 2>&1
  local git_cached_quiet="$?"
  (cd "$skron_repo" && "$skron_bin" diff --cached --quiet >/dev/null 2>&1)
  local skron_cached_quiet="$?"
  set -e
  if [[ "$git_cached_quiet" != "$skron_cached_quiet" ]]; then
    echo "status mismatch: diff --cached --quiet mutation" >&2
    echo "git=$git_cached_quiet skron=$skron_cached_quiet" >&2
    exit 1
  fi
  echo "ok: diff --cached --quiet mutation"

  git -C "$git_repo" status --porcelain=v1 --branch >"$tmp_dir/git-mutation-status.out"
  (cd "$skron_repo" && "$skron_bin" status --porcelain=v1 --branch) >"$tmp_dir/skron-mutation-status.out"
  diff -u "$tmp_dir/git-mutation-status.out" "$tmp_dir/skron-mutation-status.out"
  git -C "$git_repo" status >"$tmp_dir/git-mutation-human-status.out"
  (cd "$skron_repo" && "$skron_bin" status) >"$tmp_dir/skron-mutation-human-status.out"
  diff -u "$tmp_dir/git-mutation-human-status.out" "$tmp_dir/skron-mutation-human-status.out"

  env "${commit_env[@]}" git -C "$git_repo" -c commit.gpgsign=false commit -qm "smoke"
  (
    cd "$skron_repo"
    env "${commit_env[@]}" "$skron_bin" commit -m "smoke" >/dev/null
  )

  git -C "$git_repo" cat-file -p HEAD^{tree} >"$tmp_dir/git-mutation-tree.out"
  git -C "$skron_repo" cat-file -p HEAD^{tree} >"$tmp_dir/skron-mutation-tree.out"
  diff -u "$tmp_dir/git-mutation-tree.out" "$tmp_dir/skron-mutation-tree.out"
  git -C "$git_repo" diff --name-status HEAD~1 HEAD >"$tmp_dir/git-tree-diff-name-status.out"
  (cd "$skron_repo" && "$skron_bin" diff --name-status HEAD~1 HEAD) >"$tmp_dir/skron-tree-diff-name-status.out"
  diff -u "$tmp_dir/git-tree-diff-name-status.out" "$tmp_dir/skron-tree-diff-name-status.out"
  git -C "$git_repo" diff --raw HEAD~1 HEAD >"$tmp_dir/git-tree-diff-raw.out"
  (cd "$skron_repo" && "$skron_bin" diff --raw HEAD~1 HEAD) >"$tmp_dir/skron-tree-diff-raw.out"
  diff -u "$tmp_dir/git-tree-diff-raw.out" "$tmp_dir/skron-tree-diff-raw.out"
  git -C "$git_repo" diff HEAD~1 HEAD >"$tmp_dir/git-tree-diff-patch.out"
  (cd "$skron_repo" && "$skron_bin" diff HEAD~1 HEAD) >"$tmp_dir/skron-tree-diff-patch.out"
  diff -u "$tmp_dir/git-tree-diff-patch.out" "$tmp_dir/skron-tree-diff-patch.out"
  echo "ok: diff HEAD~1 HEAD mutation"
  echo "ok: add -A and commit mutation"

  printf 'skron smoke updated\n' >"$git_repo/skron-smoke.txt"
  printf 'skron smoke updated\n' >"$skron_repo/skron-smoke.txt"
  printf 'not staged by add update\n' >"$git_repo/add-update-new.txt"
  printf 'not staged by add update\n' >"$skron_repo/add-update-new.txt"
  git -C "$git_repo" add -u
  (cd "$skron_repo" && "$skron_bin" add -u)
  git -C "$git_repo" diff --cached --name-status >"$tmp_dir/git-add-update-cached.out"
  (cd "$skron_repo" && "$skron_bin" diff --cached --name-status) >"$tmp_dir/skron-add-update-cached.out"
  diff -u "$tmp_dir/git-add-update-cached.out" "$tmp_dir/skron-add-update-cached.out"
  git -C "$git_repo" status --porcelain=v1 --branch >"$tmp_dir/git-add-update-status.out"
  (cd "$skron_repo" && "$skron_bin" status --porcelain=v1 --branch) >"$tmp_dir/skron-add-update-status.out"
  diff -u "$tmp_dir/git-add-update-status.out" "$tmp_dir/skron-add-update-status.out"
  git -C "$git_repo" status --porcelain=v1 --branch -uno >"$tmp_dir/git-status-uno.out"
  (cd "$skron_repo" && "$skron_bin" status --porcelain=v1 --branch -uno) >"$tmp_dir/skron-status-uno.out"
  diff -u "$tmp_dir/git-status-uno.out" "$tmp_dir/skron-status-uno.out"
  git -C "$git_repo" status --porcelain=v1 --branch -uall >"$tmp_dir/git-status-uall.out"
  (cd "$skron_repo" && "$skron_bin" status --porcelain=v1 --branch -uall) >"$tmp_dir/skron-status-uall.out"
  diff -u "$tmp_dir/git-status-uall.out" "$tmp_dir/skron-status-uall.out"
  rm "$git_repo/add-update-new.txt" "$skron_repo/add-update-new.txt"
  env "${commit_env[@]}" git -C "$git_repo" -c commit.gpgsign=false commit -qm "smoke update"
  (
    cd "$skron_repo"
    env "${commit_env[@]}" "$skron_bin" commit -m "smoke update" >/dev/null
  )
  echo "ok: add -u and commit mutation"

  printf 'multi message\n' >"$git_repo/skron-multi-message.txt"
  printf 'multi message\n' >"$skron_repo/skron-multi-message.txt"
  git -C "$git_repo" add -A
  (cd "$skron_repo" && "$skron_bin" add -A)
  env "${commit_env[@]}" git -C "$git_repo" -c commit.gpgsign=false commit -qm "multi subject" -m "multi body"
  (
    cd "$skron_repo"
    env "${commit_env[@]}" "$skron_bin" commit -m "multi subject" -m "multi body" >/dev/null
  )
  git -C "$git_repo" cat-file -p HEAD >"$tmp_dir/git-multi-message-commit.out"
  git -C "$skron_repo" cat-file -p HEAD >"$tmp_dir/skron-multi-message-commit.out"
  diff -u "$tmp_dir/git-multi-message-commit.out" "$tmp_dir/skron-multi-message-commit.out"
  echo "ok: commit multi-message mutation"

  printf 'from file\n\nbody' >"$git_repo/skron-message.txt"
  printf 'from file\n\nbody' >"$skron_repo/skron-message.txt"
  printf 'file message content\n' >"$git_repo/skron-file-message.txt"
  printf 'file message content\n' >"$skron_repo/skron-file-message.txt"
  git -C "$git_repo" add -A
  (cd "$skron_repo" && "$skron_bin" add -A)
  env "${commit_env[@]}" git -C "$git_repo" -c commit.gpgsign=false commit -F skron-message.txt -q
  (
    cd "$skron_repo"
    env "${commit_env[@]}" "$skron_bin" commit -F skron-message.txt >/dev/null
  )
  git -C "$git_repo" cat-file -p HEAD >"$tmp_dir/git-file-message-commit.out"
  git -C "$skron_repo" cat-file -p HEAD >"$tmp_dir/skron-file-message-commit.out"
  diff -u "$tmp_dir/git-file-message-commit.out" "$tmp_dir/skron-file-message-commit.out"
  git -C "$git_repo" show --stat HEAD >"$tmp_dir/git-show-stat.out"
  (cd "$skron_repo" && "$skron_bin" show --stat HEAD) >"$tmp_dir/skron-show-stat.out"
  diff -u "$tmp_dir/git-show-stat.out" "$tmp_dir/skron-show-stat.out"
  git -C "$git_repo" show --numstat --format=%H HEAD >"$tmp_dir/git-show-numstat.out"
  (cd "$skron_repo" && "$skron_bin" show --numstat --format=%H HEAD) >"$tmp_dir/skron-show-numstat.out"
  diff -u "$tmp_dir/git-show-numstat.out" "$tmp_dir/skron-show-numstat.out"
  git -C "$git_repo" show --shortstat HEAD >"$tmp_dir/git-show-shortstat.out"
  (cd "$skron_repo" && "$skron_bin" show --shortstat HEAD) >"$tmp_dir/skron-show-shortstat.out"
  diff -u "$tmp_dir/git-show-shortstat.out" "$tmp_dir/skron-show-shortstat.out"
  git -C "$git_repo" show --raw --format=%H HEAD >"$tmp_dir/git-show-raw.out"
  (cd "$skron_repo" && "$skron_bin" show --raw --format=%H HEAD) >"$tmp_dir/skron-show-raw.out"
  diff -u "$tmp_dir/git-show-raw.out" "$tmp_dir/skron-show-raw.out"
  git -C "$git_repo" show --summary --format=%H HEAD >"$tmp_dir/git-show-summary.out"
  (cd "$skron_repo" && "$skron_bin" show --summary --format=%H HEAD) >"$tmp_dir/skron-show-summary.out"
  diff -u "$tmp_dir/git-show-summary.out" "$tmp_dir/skron-show-summary.out"
  git -C "$git_repo" show --name-only --format=%H HEAD >"$tmp_dir/git-show-name-only.out"
  (cd "$skron_repo" && "$skron_bin" show --name-only --format=%H HEAD) >"$tmp_dir/skron-show-name-only.out"
  diff -u "$tmp_dir/git-show-name-only.out" "$tmp_dir/skron-show-name-only.out"
  git -C "$git_repo" show --name-status --format=%H HEAD >"$tmp_dir/git-show-name-status.out"
  (cd "$skron_repo" && "$skron_bin" show --name-status --format=%H HEAD) >"$tmp_dir/skron-show-name-status.out"
  diff -u "$tmp_dir/git-show-name-status.out" "$tmp_dir/skron-show-name-status.out"
  echo "ok: commit -F mutation"

  printf 'reuse message\n' >"$git_repo/skron-reuse-message.txt"
  printf 'reuse message\n' >"$skron_repo/skron-reuse-message.txt"
  git -C "$git_repo" add -A
  (cd "$skron_repo" && "$skron_bin" add -A)
  env "${commit_env[@]}" git -C "$git_repo" -c commit.gpgsign=false commit -C HEAD -q
  (
    cd "$skron_repo"
    env "${commit_env[@]}" "$skron_bin" commit -C HEAD >/dev/null
  )
  git -C "$git_repo" cat-file -p HEAD >"$tmp_dir/git-reuse-message-commit.out"
  git -C "$skron_repo" cat-file -p HEAD >"$tmp_dir/skron-reuse-message-commit.out"
  diff -u "$tmp_dir/git-reuse-message-commit.out" "$tmp_dir/skron-reuse-message-commit.out"
  echo "ok: commit -C mutation"

  printf 'author override\n' >"$git_repo/skron-author.txt"
  printf 'author override\n' >"$skron_repo/skron-author.txt"
  git -C "$git_repo" add -A
  (cd "$skron_repo" && "$skron_bin" add -A)
  env "${commit_env[@]}" git -C "$git_repo" -c commit.gpgsign=false commit --author "Alice Example <alice@example.test>" -qm "author override"
  (
    cd "$skron_repo"
    env "${commit_env[@]}" "$skron_bin" commit --author "Alice Example <alice@example.test>" -m "author override" >/dev/null
  )
  git -C "$git_repo" cat-file -p HEAD >"$tmp_dir/git-author-commit.out"
  git -C "$skron_repo" cat-file -p HEAD >"$tmp_dir/skron-author-commit.out"
  diff -u "$tmp_dir/git-author-commit.out" "$tmp_dir/skron-author-commit.out"
  echo "ok: commit --author mutation"

  printf 'reset author\n' >"$git_repo/skron-author-reset.txt"
  printf 'reset author\n' >"$skron_repo/skron-author-reset.txt"
  git -C "$git_repo" add -A
  (cd "$skron_repo" && "$skron_bin" add -A)
  env "${commit_env[@]}" git -C "$git_repo" -c commit.gpgsign=false commit --amend --reset-author -qm "reset author"
  (
    cd "$skron_repo"
    env "${commit_env[@]}" "$skron_bin" commit --amend --reset-author -m "reset author" >/dev/null
  )
  git -C "$git_repo" cat-file -p HEAD >"$tmp_dir/git-reset-author-commit.out"
  git -C "$skron_repo" cat-file -p HEAD >"$tmp_dir/skron-reset-author-commit.out"
  diff -u "$tmp_dir/git-reset-author-commit.out" "$tmp_dir/skron-reset-author-commit.out"
  echo "ok: commit --reset-author mutation"

  printf 'date override\n' >"$git_repo/skron-date.txt"
  printf 'date override\n' >"$skron_repo/skron-date.txt"
  git -C "$git_repo" add -A
  (cd "$skron_repo" && "$skron_bin" add -A)
  env "${commit_env[@]}" git -C "$git_repo" -c commit.gpgsign=false commit --date "1700001234 +0000" -qm "date override"
  (
    cd "$skron_repo"
    env "${commit_env[@]}" "$skron_bin" commit --date "1700001234 +0000" -m "date override" >/dev/null
  )
  git -C "$git_repo" cat-file -p HEAD >"$tmp_dir/git-date-commit.out"
  git -C "$skron_repo" cat-file -p HEAD >"$tmp_dir/skron-date-commit.out"
  diff -u "$tmp_dir/git-date-commit.out" "$tmp_dir/skron-date-commit.out"
  echo "ok: commit --date mutation"

  printf 'skron smoke amended\n' >"$git_repo/skron-smoke.txt"
  printf 'skron smoke amended\n' >"$skron_repo/skron-smoke.txt"
  git -C "$git_repo" add -u
  (cd "$skron_repo" && "$skron_bin" add -u)
  env "${commit_env[@]}" git -C "$git_repo" -c commit.gpgsign=false commit --amend -qm "smoke amended"
  (
    cd "$skron_repo"
    env "${commit_env[@]}" "$skron_bin" commit --amend -m "smoke amended" >/dev/null
  )
  git -C "$git_repo" cat-file -p HEAD >"$tmp_dir/git-amend-commit.out"
  git -C "$skron_repo" cat-file -p HEAD >"$tmp_dir/skron-amend-commit.out"
  diff -u "$tmp_dir/git-amend-commit.out" "$tmp_dir/skron-amend-commit.out"
  echo "ok: commit --amend mutation"

  printf 'skron no-edit amend\n' >"$git_repo/skron-amend-no-edit.txt"
  printf 'skron no-edit amend\n' >"$skron_repo/skron-amend-no-edit.txt"
  git -C "$git_repo" add -A
  (cd "$skron_repo" && "$skron_bin" add -A)
  env "${commit_env[@]}" git -C "$git_repo" -c commit.gpgsign=false commit --amend --no-edit -q
  (
    cd "$skron_repo"
    env "${commit_env[@]}" "$skron_bin" commit --amend --no-edit >/dev/null
  )
  git -C "$git_repo" cat-file -p HEAD >"$tmp_dir/git-amend-no-edit-commit.out"
  git -C "$skron_repo" cat-file -p HEAD >"$tmp_dir/skron-amend-no-edit-commit.out"
  diff -u "$tmp_dir/git-amend-no-edit-commit.out" "$tmp_dir/skron-amend-no-edit-commit.out"
  echo "ok: commit --amend --no-edit mutation"

  git -C "$git_repo" grep -n skron skron-smoke.txt >"$tmp_dir/git-grep.out"
  (cd "$skron_repo" && "$skron_bin" grep -n skron skron-smoke.txt) >"$tmp_dir/skron-grep.out"
  diff -u "$tmp_dir/git-grep.out" "$tmp_dir/skron-grep.out"
  echo "ok: grep tracked text mutation"
  git -C "$git_repo" grep -n skron HEAD -- skron-smoke.txt >"$tmp_dir/git-grep-head.out"
  (cd "$skron_repo" && "$skron_bin" grep -n skron HEAD -- skron-smoke.txt) >"$tmp_dir/skron-grep-head.out"
  diff -u "$tmp_dir/git-grep-head.out" "$tmp_dir/skron-grep-head.out"
  echo "ok: grep HEAD mutation"
  printf 'cached grep needle\n' >"$git_repo/skron-grep-cache.txt"
  printf 'cached grep needle\n' >"$skron_repo/skron-grep-cache.txt"
  git -C "$git_repo" add skron-grep-cache.txt
  (cd "$skron_repo" && "$skron_bin" add skron-grep-cache.txt)
  printf 'worktree grep needle\n' >"$git_repo/skron-grep-cache.txt"
  printf 'worktree grep needle\n' >"$skron_repo/skron-grep-cache.txt"
  git -C "$git_repo" grep grep skron-grep-cache.txt >"$tmp_dir/git-grep-worktree.out"
  (cd "$skron_repo" && "$skron_bin" grep grep skron-grep-cache.txt) >"$tmp_dir/skron-grep-worktree.out"
  diff -u "$tmp_dir/git-grep-worktree.out" "$tmp_dir/skron-grep-worktree.out"
  git -C "$git_repo" grep --cached grep skron-grep-cache.txt >"$tmp_dir/git-grep-cached.out"
  (cd "$skron_repo" && "$skron_bin" grep --cached grep skron-grep-cache.txt) >"$tmp_dir/skron-grep-cached.out"
  diff -u "$tmp_dir/git-grep-cached.out" "$tmp_dir/skron-grep-cached.out"
  echo "ok: grep --cached mutation"
  git -C "$git_repo" add skron-grep-cache.txt
  (cd "$skron_repo" && "$skron_bin" add skron-grep-cache.txt)
  env "${commit_env[@]}" git -C "$git_repo" -c commit.gpgsign=false commit -qm "grep cache smoke"
  (
    cd "$skron_repo"
    env "${commit_env[@]}" "$skron_bin" commit -m "grep cache smoke" >/dev/null
  )
  git -C "$git_repo" cat-file -p HEAD >"$tmp_dir/git-grep-cache-commit.out"
  git -C "$skron_repo" cat-file -p HEAD >"$tmp_dir/skron-grep-cache-commit.out"
  diff -u "$tmp_dir/git-grep-cache-commit.out" "$tmp_dir/skron-grep-cache-commit.out"

  env "${commit_env[@]}" git -C "$git_repo" -c commit.gpgsign=false tag -a skron-smoke-tag -m "smoke tag"
  (
    cd "$skron_repo"
    env "${commit_env[@]}" "$skron_bin" tag -a skron-smoke-tag -m "smoke tag"
  )
  git -C "$git_repo" cat-file -p refs/tags/skron-smoke-tag >"$tmp_dir/git-annotated-tag.out"
  git -C "$skron_repo" cat-file -p refs/tags/skron-smoke-tag >"$tmp_dir/skron-annotated-tag.out"
  diff -u "$tmp_dir/git-annotated-tag.out" "$tmp_dir/skron-annotated-tag.out"
  git -C "$git_repo" rev-parse --verify 'skron-smoke-tag^{commit}' >"$tmp_dir/git-tag-peel-commit.out"
  (cd "$skron_repo" && "$skron_bin" rev-parse --verify 'skron-smoke-tag^{commit}') >"$tmp_dir/skron-tag-peel-commit.out"
  diff -u "$tmp_dir/git-tag-peel-commit.out" "$tmp_dir/skron-tag-peel-commit.out"
  git -C "$git_repo" rev-parse --verify 'skron-smoke-tag^{}' >"$tmp_dir/git-tag-peel-auto.out"
  (cd "$skron_repo" && "$skron_bin" rev-parse --verify 'skron-smoke-tag^{}') >"$tmp_dir/skron-tag-peel-auto.out"
  diff -u "$tmp_dir/git-tag-peel-auto.out" "$tmp_dir/skron-tag-peel-auto.out"
  echo "ok: tag -a -m mutation"

  git -C "$git_repo" branch smoke-delete
  (cd "$skron_repo" && "$skron_bin" branch smoke-delete)
  git -C "$git_repo" branch -d smoke-delete >/dev/null
  (cd "$skron_repo" && "$skron_bin" branch -d smoke-delete >/dev/null)
  git -C "$git_repo" show-ref --heads >"$tmp_dir/git-branch-delete.out"
  (cd "$skron_repo" && "$skron_bin" show-ref --heads) >"$tmp_dir/skron-branch-delete.out"
  diff -u "$tmp_dir/git-branch-delete.out" "$tmp_dir/skron-branch-delete.out"
  echo "ok: branch -d mutation"

  git -C "$git_repo" branch smoke-rename
  (cd "$skron_repo" && "$skron_bin" branch smoke-rename)
  git -C "$git_repo" branch -m smoke-rename smoke-renamed
  (cd "$skron_repo" && "$skron_bin" branch -m smoke-rename smoke-renamed)
  git -C "$git_repo" show-ref --heads >"$tmp_dir/git-branch-rename.out"
  (cd "$skron_repo" && "$skron_bin" show-ref --heads) >"$tmp_dir/skron-branch-rename.out"
  diff -u "$tmp_dir/git-branch-rename.out" "$tmp_dir/skron-branch-rename.out"
  echo "ok: branch -m mutation"

  local default_branch
  default_branch="$(git -C "$git_repo" rev-parse --abbrev-ref HEAD)"
  git -C "$git_repo" branch -u "origin/$default_branch" >/dev/null
  (cd "$skron_repo" && "$skron_bin" branch -u "origin/$default_branch" >/dev/null)
  git -C "$git_repo" status --porcelain=v1 --branch >"$tmp_dir/git-branch-upstream-status.out"
  (cd "$skron_repo" && "$skron_bin" status --porcelain=v1 --branch) >"$tmp_dir/skron-branch-upstream-status.out"
  diff -u "$tmp_dir/git-branch-upstream-status.out" "$tmp_dir/skron-branch-upstream-status.out"
  git -C "$git_repo" branch --unset-upstream >/dev/null
  (cd "$skron_repo" && "$skron_bin" branch --unset-upstream >/dev/null)
  git -C "$git_repo" status --porcelain=v1 --branch >"$tmp_dir/git-branch-unset-upstream-status.out"
  (cd "$skron_repo" && "$skron_bin" status --porcelain=v1 --branch) >"$tmp_dir/skron-branch-unset-upstream-status.out"
  diff -u "$tmp_dir/git-branch-unset-upstream-status.out" "$tmp_dir/skron-branch-unset-upstream-status.out"
  echo "ok: branch -u/--unset-upstream mutation"

  git -C "$git_repo" checkout --quiet -b smoke-feature
  (cd "$skron_repo" && "$skron_bin" checkout -b smoke-feature >/dev/null)
  printf 'feature branch\n' >"$git_repo/skron-branch-smoke.txt"
  printf 'feature branch\n' >"$skron_repo/skron-branch-smoke.txt"
  git -C "$git_repo" add -A
  (cd "$skron_repo" && "$skron_bin" add -A)
  env "${commit_env[@]}" git -C "$git_repo" -c commit.gpgsign=false commit -qm "branch smoke"
  (
    cd "$skron_repo"
    env "${commit_env[@]}" "$skron_bin" commit -m "branch smoke" >/dev/null
  )
  git -C "$git_repo" checkout --quiet "$default_branch"
  (cd "$skron_repo" && "$skron_bin" checkout "$default_branch" >/dev/null)
  git -C "$git_repo" status --porcelain=v1 --branch >"$tmp_dir/git-checkout-status.out"
  (cd "$skron_repo" && "$skron_bin" status --porcelain=v1 --branch) >"$tmp_dir/skron-checkout-status.out"
  diff -u "$tmp_dir/git-checkout-status.out" "$tmp_dir/skron-checkout-status.out"
  git -C "$git_repo" checkout --quiet smoke-feature
  (cd "$skron_repo" && "$skron_bin" checkout smoke-feature >/dev/null)
  git -C "$git_repo" cat-file -p HEAD^{tree} >"$tmp_dir/git-checkout-tree.out"
  git -C "$skron_repo" cat-file -p HEAD^{tree} >"$tmp_dir/skron-checkout-tree.out"
  diff -u "$tmp_dir/git-checkout-tree.out" "$tmp_dir/skron-checkout-tree.out"
  git -C "$git_repo" checkout --quiet -B smoke-reset "$default_branch"
  (cd "$skron_repo" && "$skron_bin" checkout -B smoke-reset "$default_branch" >/dev/null)
  git -C "$git_repo" status --porcelain=v1 --branch >"$tmp_dir/git-checkout-reset-status.out"
  (cd "$skron_repo" && "$skron_bin" status --porcelain=v1 --branch) >"$tmp_dir/skron-checkout-reset-status.out"
  diff -u "$tmp_dir/git-checkout-reset-status.out" "$tmp_dir/skron-checkout-reset-status.out"
  git -C "$git_repo" checkout --quiet --detach smoke-feature
  (cd "$skron_repo" && "$skron_bin" checkout --detach smoke-feature >/dev/null)
  git -C "$git_repo" rev-parse --abbrev-ref HEAD >"$tmp_dir/git-checkout-detach-ref.out"
  git -C "$skron_repo" rev-parse --abbrev-ref HEAD >"$tmp_dir/skron-checkout-detach-ref.out"
  diff -u "$tmp_dir/git-checkout-detach-ref.out" "$tmp_dir/skron-checkout-detach-ref.out"
  git -C "$git_repo" rev-parse HEAD >"$tmp_dir/git-checkout-detach-head.out"
  git -C "$skron_repo" rev-parse HEAD >"$tmp_dir/skron-checkout-detach-head.out"
  diff -u "$tmp_dir/git-checkout-detach-head.out" "$tmp_dir/skron-checkout-detach-head.out"
  echo "ok: checkout -b/-B/--detach branch mutation"
}

compare_reset_mutation() {
  local git_repo="$tmp_dir/git-reset"
  local skron_repo="$tmp_dir/skron-reset"
  local target

  git clone --quiet "$repo_dir" "$git_repo"
  git clone --quiet "$repo_dir" "$skron_repo"
  git -C "$git_repo" config user.name Bench
  git -C "$git_repo" config user.email bench@example.test
  git -C "$git_repo" config commit.gpgsign false
  git -C "$skron_repo" config user.name Bench
  git -C "$skron_repo" config user.email bench@example.test
  git -C "$skron_repo" config commit.gpgsign false

  target="$(git -C "$git_repo" rev-parse HEAD)"
  printf 'reset smoke\n' >"$git_repo/skron-reset-smoke.txt"
  printf 'reset smoke\n' >"$skron_repo/skron-reset-smoke.txt"
  git -C "$git_repo" add -A
  (cd "$skron_repo" && "$skron_bin" add -A)
  env "${commit_env[@]}" git -C "$git_repo" -c commit.gpgsign=false commit -qm "reset smoke"
  (
    cd "$skron_repo"
    env "${commit_env[@]}" "$skron_bin" commit -m "reset smoke" >/dev/null
  )

  git -C "$git_repo" reset --hard "$target" >/dev/null
  (cd "$skron_repo" && "$skron_bin" reset --hard "$target" >/dev/null)
  git -C "$git_repo" status --porcelain=v1 --branch >"$tmp_dir/git-reset-status.out"
  (cd "$skron_repo" && "$skron_bin" status --porcelain=v1 --branch) >"$tmp_dir/skron-reset-status.out"
  diff -u "$tmp_dir/git-reset-status.out" "$tmp_dir/skron-reset-status.out"
  git -C "$git_repo" cat-file -p HEAD^{tree} >"$tmp_dir/git-reset-tree.out"
  git -C "$skron_repo" cat-file -p HEAD^{tree} >"$tmp_dir/skron-reset-tree.out"
  diff -u "$tmp_dir/git-reset-tree.out" "$tmp_dir/skron-reset-tree.out"
  echo "ok: reset --hard mutation"

  printf 'reset path staged\n' >>"$git_repo/$first_file"
  printf 'reset path staged\n' >>"$skron_repo/$first_file"
  git -C "$git_repo" add "$first_file"
  (cd "$skron_repo" && "$skron_bin" add "$first_file")
  git -C "$git_repo" reset HEAD -- "$first_file" >/dev/null
  (cd "$skron_repo" && "$skron_bin" reset HEAD -- "$first_file" >/dev/null)
  git -C "$git_repo" status --porcelain=v1 --branch >"$tmp_dir/git-reset-path-status.out"
  (cd "$skron_repo" && "$skron_bin" status --porcelain=v1 --branch) >"$tmp_dir/skron-reset-path-status.out"
  diff -u "$tmp_dir/git-reset-path-status.out" "$tmp_dir/skron-reset-path-status.out"
  echo "ok: reset path mutation"
}

compare_switch_mutation() {
  local git_repo="$tmp_dir/git-switch"
  local skron_repo="$tmp_dir/skron-switch"
  local default_branch

  git clone --quiet "$repo_dir" "$git_repo"
  git clone --quiet "$repo_dir" "$skron_repo"
  git -C "$git_repo" config user.name Bench
  git -C "$git_repo" config user.email bench@example.test
  git -C "$git_repo" config commit.gpgsign false
  git -C "$skron_repo" config user.name Bench
  git -C "$skron_repo" config user.email bench@example.test
  git -C "$skron_repo" config commit.gpgsign false
  default_branch="$(git -C "$git_repo" rev-parse --abbrev-ref HEAD)"

  git -C "$git_repo" switch --quiet -c smoke-switch
  (cd "$skron_repo" && "$skron_bin" switch -c smoke-switch >/dev/null)
  printf 'switch branch\n' >"$git_repo/skron-switch-smoke.txt"
  printf 'switch branch\n' >"$skron_repo/skron-switch-smoke.txt"
  git -C "$git_repo" add -A
  (cd "$skron_repo" && "$skron_bin" add -A >/dev/null)
  env "${commit_env[@]}" git -C "$git_repo" -c commit.gpgsign=false commit -qm "switch smoke"
  (
    cd "$skron_repo"
    env "${commit_env[@]}" "$skron_bin" commit -m "switch smoke" >/dev/null
  )

  git -C "$git_repo" switch --quiet "$default_branch"
  (cd "$skron_repo" && "$skron_bin" switch "$default_branch" >/dev/null)
  git -C "$git_repo" status --porcelain=v1 --branch >"$tmp_dir/git-switch-status.out"
  (cd "$skron_repo" && "$skron_bin" status --porcelain=v1 --branch) >"$tmp_dir/skron-switch-status.out"
  diff -u "$tmp_dir/git-switch-status.out" "$tmp_dir/skron-switch-status.out"

  git -C "$git_repo" switch --quiet --detach smoke-switch
  (cd "$skron_repo" && "$skron_bin" switch --detach smoke-switch >/dev/null)
  git -C "$git_repo" rev-parse --abbrev-ref HEAD >"$tmp_dir/git-switch-detached-ref.out"
  git -C "$skron_repo" rev-parse --abbrev-ref HEAD >"$tmp_dir/skron-switch-detached-ref.out"
  diff -u "$tmp_dir/git-switch-detached-ref.out" "$tmp_dir/skron-switch-detached-ref.out"
  git -C "$git_repo" rev-parse HEAD >"$tmp_dir/git-switch-detached-head.out"
  git -C "$skron_repo" rev-parse HEAD >"$tmp_dir/skron-switch-detached-head.out"
  diff -u "$tmp_dir/git-switch-detached-head.out" "$tmp_dir/skron-switch-detached-head.out"
  git -C "$git_repo" cat-file -p HEAD^{tree} >"$tmp_dir/git-switch-tree.out"
  git -C "$skron_repo" cat-file -p HEAD^{tree} >"$tmp_dir/skron-switch-tree.out"
  diff -u "$tmp_dir/git-switch-tree.out" "$tmp_dir/skron-switch-tree.out"
  echo "ok: switch branch and detach mutation"
}

compare_restore_mutation() {
  local git_repo="$tmp_dir/git-restore"
  local skron_repo="$tmp_dir/skron-restore"

  git clone --quiet "$repo_dir" "$git_repo"
  git clone --quiet "$repo_dir" "$skron_repo"
  printf 'restore dirty\n' >>"$git_repo/$first_file"
  printf 'restore dirty\n' >>"$skron_repo/$first_file"
  printf 'restore staged add\n' >"$git_repo/skron-restore-new.txt"
  printf 'restore staged add\n' >"$skron_repo/skron-restore-new.txt"
  git -C "$git_repo" add -A
  (cd "$skron_repo" && "$skron_bin" add -A >/dev/null)

  git -C "$git_repo" restore --staged "$first_file" skron-restore-new.txt
  (cd "$skron_repo" && "$skron_bin" restore --staged "$first_file" skron-restore-new.txt >/dev/null)
  git -C "$git_repo" status --porcelain=v1 --branch >"$tmp_dir/git-restore-staged-status.out"
  (cd "$skron_repo" && "$skron_bin" status --porcelain=v1 --branch) >"$tmp_dir/skron-restore-staged-status.out"
  diff -u "$tmp_dir/git-restore-staged-status.out" "$tmp_dir/skron-restore-staged-status.out"

  git -C "$git_repo" restore "$first_file"
  (cd "$skron_repo" && "$skron_bin" restore "$first_file" >/dev/null)
  git -C "$git_repo" status --porcelain=v1 --branch >"$tmp_dir/git-restore-worktree-status.out"
  (cd "$skron_repo" && "$skron_bin" status --porcelain=v1 --branch) >"$tmp_dir/skron-restore-worktree-status.out"
  diff -u "$tmp_dir/git-restore-worktree-status.out" "$tmp_dir/skron-restore-worktree-status.out"
  echo "ok: restore staged and worktree mutation"

  printf 'checkout path dirty\n' >>"$git_repo/$first_file"
  printf 'checkout path dirty\n' >>"$skron_repo/$first_file"
  git -C "$git_repo" checkout -- "$first_file"
  (cd "$skron_repo" && "$skron_bin" checkout -- "$first_file" >/dev/null)
  git -C "$git_repo" status --porcelain=v1 --branch >"$tmp_dir/git-checkout-path-status.out"
  (cd "$skron_repo" && "$skron_bin" status --porcelain=v1 --branch) >"$tmp_dir/skron-checkout-path-status.out"
  diff -u "$tmp_dir/git-checkout-path-status.out" "$tmp_dir/skron-checkout-path-status.out"
  echo "ok: checkout path mutation"
}

compare_clean_mutation() {
  local git_repo="$tmp_dir/git-clean"
  local skron_repo="$tmp_dir/skron-clean"

  git clone --quiet "$repo_dir" "$git_repo"
  git clone --quiet "$repo_dir" "$skron_repo"
  mkdir -p "$git_repo/skron-clean-dir" "$skron_repo/skron-clean-dir"
  printf 'clean file\n' >"$git_repo/skron-clean.txt"
  printf 'clean file\n' >"$skron_repo/skron-clean.txt"
  printf 'clean dir\n' >"$git_repo/skron-clean-dir/file.txt"
  printf 'clean dir\n' >"$skron_repo/skron-clean-dir/file.txt"

  git -C "$git_repo" clean -n -d >"$tmp_dir/git-clean-dry-run.out"
  (cd "$skron_repo" && "$skron_bin" clean -n -d) >"$tmp_dir/skron-clean-dry-run.out"
  diff -u "$tmp_dir/git-clean-dry-run.out" "$tmp_dir/skron-clean-dry-run.out"

  git -C "$git_repo" clean -f -d >"$tmp_dir/git-clean-force.out"
  (cd "$skron_repo" && "$skron_bin" clean -f -d) >"$tmp_dir/skron-clean-force.out"
  diff -u "$tmp_dir/git-clean-force.out" "$tmp_dir/skron-clean-force.out"
  git -C "$git_repo" status --porcelain=v1 --branch >"$tmp_dir/git-clean-status.out"
  (cd "$skron_repo" && "$skron_bin" status --porcelain=v1 --branch) >"$tmp_dir/skron-clean-status.out"
  diff -u "$tmp_dir/git-clean-status.out" "$tmp_dir/skron-clean-status.out"
  echo "ok: clean dry-run and force mutation"
}

compare_merge_ff_mutation() {
  local git_repo="$tmp_dir/git-merge-ff"
  local skron_repo="$tmp_dir/skron-merge-ff"
  local default_branch

  git clone --quiet "$repo_dir" "$git_repo"
  git clone --quiet "$repo_dir" "$skron_repo"
  git -C "$git_repo" config user.name Bench
  git -C "$git_repo" config user.email bench@example.test
  git -C "$git_repo" config commit.gpgsign false
  git -C "$skron_repo" config user.name Bench
  git -C "$skron_repo" config user.email bench@example.test
  git -C "$skron_repo" config commit.gpgsign false
  default_branch="$(git -C "$git_repo" rev-parse --abbrev-ref HEAD)"

  git -C "$git_repo" switch --quiet -c smoke-merge-ff
  (cd "$skron_repo" && "$skron_bin" switch -c smoke-merge-ff >/dev/null)
  printf 'merge fast-forward\n' >"$git_repo/skron-merge-ff-smoke.txt"
  printf 'merge fast-forward\n' >"$skron_repo/skron-merge-ff-smoke.txt"
  git -C "$git_repo" add -A
  (cd "$skron_repo" && "$skron_bin" add -A >/dev/null)
  env "${commit_env[@]}" git -C "$git_repo" -c commit.gpgsign=false commit -qm "merge ff smoke"
  (
    cd "$skron_repo"
    env "${commit_env[@]}" "$skron_bin" commit -m "merge ff smoke" >/dev/null
  )

  git -C "$git_repo" switch --quiet "$default_branch"
  (cd "$skron_repo" && "$skron_bin" switch "$default_branch" >/dev/null)
  git -C "$git_repo" merge --ff-only smoke-merge-ff >/dev/null
  (cd "$skron_repo" && "$skron_bin" merge --ff-only smoke-merge-ff >/dev/null)
  git -C "$git_repo" rev-parse HEAD >"$tmp_dir/git-merge-ff-head.out"
  git -C "$skron_repo" rev-parse HEAD >"$tmp_dir/skron-merge-ff-head.out"
  diff -u "$tmp_dir/git-merge-ff-head.out" "$tmp_dir/skron-merge-ff-head.out"
  git -C "$git_repo" status --porcelain=v1 --branch >"$tmp_dir/git-merge-ff-status.out"
  (cd "$skron_repo" && "$skron_bin" status --porcelain=v1 --branch) >"$tmp_dir/skron-merge-ff-status.out"
  diff -u "$tmp_dir/git-merge-ff-status.out" "$tmp_dir/skron-merge-ff-status.out"
  git -C "$git_repo" cat-file -p HEAD^{tree} >"$tmp_dir/git-merge-ff-tree.out"
  git -C "$skron_repo" cat-file -p HEAD^{tree} >"$tmp_dir/skron-merge-ff-tree.out"
  diff -u "$tmp_dir/git-merge-ff-tree.out" "$tmp_dir/skron-merge-ff-tree.out"
  echo "ok: merge --ff-only mutation"
}

compare_update_ref_mutation() {
  local git_repo="$tmp_dir/git-update-ref"
  local skron_repo="$tmp_dir/skron-update-ref"

  git clone --quiet "$repo_dir" "$git_repo"
  git clone --quiet "$repo_dir" "$skron_repo"
  git -C "$git_repo" update-ref refs/heads/skron-update-ref HEAD
  (cd "$skron_repo" && "$skron_bin" update-ref refs/heads/skron-update-ref HEAD)
  git -C "$git_repo" show-ref --heads >"$tmp_dir/git-update-ref-heads.out"
  (cd "$skron_repo" && "$skron_bin" show-ref --heads) >"$tmp_dir/skron-update-ref-heads.out"
  diff -u "$tmp_dir/git-update-ref-heads.out" "$tmp_dir/skron-update-ref-heads.out"

  git -C "$git_repo" symbolic-ref HEAD refs/heads/skron-update-ref
  (cd "$skron_repo" && "$skron_bin" symbolic-ref HEAD refs/heads/skron-update-ref)
  git -C "$git_repo" symbolic-ref --short HEAD >"$tmp_dir/git-symbolic-ref.out"
  (cd "$skron_repo" && "$skron_bin" symbolic-ref --short HEAD) >"$tmp_dir/skron-symbolic-ref.out"
  diff -u "$tmp_dir/git-symbolic-ref.out" "$tmp_dir/skron-symbolic-ref.out"

  git -C "$git_repo" update-ref -d refs/heads/skron-update-ref
  (cd "$skron_repo" && "$skron_bin" update-ref -d refs/heads/skron-update-ref)
  git -C "$git_repo" symbolic-ref -q HEAD >"$tmp_dir/git-symbolic-ref-deleted.out"
  (cd "$skron_repo" && "$skron_bin" symbolic-ref -q HEAD) >"$tmp_dir/skron-symbolic-ref-deleted.out"
  diff -u "$tmp_dir/git-symbolic-ref-deleted.out" "$tmp_dir/skron-symbolic-ref-deleted.out"
  echo "ok: update-ref and symbolic-ref mutation"
}

compare_rm_mutation() {
  local git_repo="$tmp_dir/git-rm"
  local skron_repo="$tmp_dir/skron-rm"

  git clone --quiet "$repo_dir" "$git_repo"
  git clone --quiet "$repo_dir" "$skron_repo"
  git -C "$git_repo" config user.name Bench
  git -C "$git_repo" config user.email bench@example.test
  git -C "$git_repo" config commit.gpgsign false
  git -C "$skron_repo" config user.name Bench
  git -C "$skron_repo" config user.email bench@example.test
  git -C "$skron_repo" config commit.gpgsign false

  mkdir -p "$git_repo/rm-dir" "$skron_repo/rm-dir"
  printf 'tracked dir file\n' >"$git_repo/rm-dir/tracked.txt"
  printf 'tracked dir file\n' >"$skron_repo/rm-dir/tracked.txt"
  printf 'cached only\n' >"$git_repo/rm-cached.txt"
  printf 'cached only\n' >"$skron_repo/rm-cached.txt"
  git -C "$git_repo" add -A
  (cd "$skron_repo" && "$skron_bin" add -A)
  env "${commit_env[@]}" git -C "$git_repo" -c commit.gpgsign=false commit -qm "rm fixture"
  (
    cd "$skron_repo"
    env "${commit_env[@]}" "$skron_bin" commit -m "rm fixture" >/dev/null
  )
  printf 'untracked\n' >"$git_repo/rm-dir/untracked.txt"
  printf 'untracked\n' >"$skron_repo/rm-dir/untracked.txt"

  git -C "$git_repo" rm "$first_file" >/dev/null
  (cd "$skron_repo" && "$skron_bin" rm "$first_file" >/dev/null)
  git -C "$git_repo" rm -r rm-dir >/dev/null
  (cd "$skron_repo" && "$skron_bin" rm -r rm-dir >/dev/null)
  git -C "$git_repo" rm --cached rm-cached.txt >/dev/null
  (cd "$skron_repo" && "$skron_bin" rm --cached rm-cached.txt >/dev/null)
  git -C "$git_repo" status --porcelain=v1 --branch >"$tmp_dir/git-rm-status.out"
  (cd "$skron_repo" && "$skron_bin" status --porcelain=v1 --branch) >"$tmp_dir/skron-rm-status.out"
  diff -u "$tmp_dir/git-rm-status.out" "$tmp_dir/skron-rm-status.out"
  echo "ok: rm file dir cached mutation"
}

compare_mv_mutation() {
  local git_repo="$tmp_dir/git-mv"
  local skron_repo="$tmp_dir/skron-mv"

  git clone --quiet "$repo_dir" "$git_repo"
  git clone --quiet "$repo_dir" "$skron_repo"
  git -C "$git_repo" config user.name Bench
  git -C "$git_repo" config user.email bench@example.test
  git -C "$git_repo" config commit.gpgsign false
  git -C "$skron_repo" config user.name Bench
  git -C "$skron_repo" config user.email bench@example.test
  git -C "$skron_repo" config commit.gpgsign false

  mkdir -p "$git_repo/mv-dir" "$skron_repo/mv-dir"
  printf 'tracked move\n' >"$git_repo/mv-dir/tracked.txt"
  printf 'tracked move\n' >"$skron_repo/mv-dir/tracked.txt"
  git -C "$git_repo" add -A
  (cd "$skron_repo" && "$skron_bin" add -A)
  env "${commit_env[@]}" git -C "$git_repo" -c commit.gpgsign=false commit -qm "mv fixture"
  (
    cd "$skron_repo"
    env "${commit_env[@]}" "$skron_bin" commit -m "mv fixture" >/dev/null
  )
  printf 'untracked move\n' >"$git_repo/mv-dir/untracked.txt"
  printf 'untracked move\n' >"$skron_repo/mv-dir/untracked.txt"

  git -C "$git_repo" mv "$first_file" moved-file.txt
  (cd "$skron_repo" && "$skron_bin" mv "$first_file" moved-file.txt)
  git -C "$git_repo" mv mv-dir moved-dir
  (cd "$skron_repo" && "$skron_bin" mv mv-dir moved-dir)
  git -C "$git_repo" diff --cached --name-status --no-renames >"$tmp_dir/git-mv-diff.out"
  git -C "$skron_repo" diff --cached --name-status --no-renames >"$tmp_dir/skron-mv-diff.out"
  diff -u "$tmp_dir/git-mv-diff.out" "$tmp_dir/skron-mv-diff.out"
  env "${commit_env[@]}" git -C "$git_repo" -c commit.gpgsign=false commit -qm "mv smoke"
  (
    cd "$skron_repo"
    env "${commit_env[@]}" "$skron_bin" commit -m "mv smoke" >/dev/null
  )
  git -C "$git_repo" cat-file -p HEAD^{tree} >"$tmp_dir/git-mv-tree.out"
  git -C "$skron_repo" cat-file -p HEAD^{tree} >"$tmp_dir/skron-mv-tree.out"
  diff -u "$tmp_dir/git-mv-tree.out" "$tmp_dir/skron-mv-tree.out"
  echo "ok: mv file dir mutation"
}

compare_local_clone() {
  local git_clone="$tmp_dir/git-local-clone"
  local skron_clone="$tmp_dir/skron-local-clone"

  git -C "$tmp_dir" clone --quiet "$repo_dir" "$git_clone"
  (cd "$tmp_dir" && "$skron_bin" clone "$repo_dir" "$skron_clone" >/dev/null)
  git -C "$git_clone" rev-parse HEAD >"$tmp_dir/git-clone-head.out"
  git -C "$skron_clone" rev-parse HEAD >"$tmp_dir/skron-clone-head.out"
  diff -u "$tmp_dir/git-clone-head.out" "$tmp_dir/skron-clone-head.out"
  git -C "$git_clone" cat-file -p HEAD^{tree} >"$tmp_dir/git-clone-tree.out"
  git -C "$skron_clone" cat-file -p HEAD^{tree} >"$tmp_dir/skron-clone-tree.out"
  diff -u "$tmp_dir/git-clone-tree.out" "$tmp_dir/skron-clone-tree.out"
  git -C "$git_clone" branch -r >"$tmp_dir/git-clone-remotes.out"
  (cd "$skron_clone" && "$skron_bin" branch -r) >"$tmp_dir/skron-clone-remotes.out"
  diff -u "$tmp_dir/git-clone-remotes.out" "$tmp_dir/skron-clone-remotes.out"
  echo "ok: clone local repository mutation"
}

compare_local_ls_remote() {
  local remote="$tmp_dir/ls-remote-source.git"
  local work="$tmp_dir/ls-remote-work"

  git -C "$tmp_dir" init --bare "$remote" >/dev/null
  git -C "$tmp_dir" init -q -b main "$work"
  git -C "$work" config user.name Bench
  git -C "$work" config user.email bench@example.test
  git -C "$work" config commit.gpgsign false
  printf 'ls remote smoke\n' >"$work/ls-remote-smoke.txt"
  git -C "$work" add -A
  env "${commit_env[@]}" git -C "$work" -c commit.gpgsign=false commit -qm "ls-remote smoke"
  git -C "$work" branch ls-remote-feature
  env "${commit_env[@]}" git -C "$work" tag -a v-ls-remote -m "ls remote tag"
  git -C "$work" remote add origin "$remote"
  git -C "$work" push -q origin main ls-remote-feature --tags

  git -C "$work" ls-remote origin >"$tmp_dir/git-ls-remote.out"
  (cd "$work" && "$skron_bin" ls-remote origin) >"$tmp_dir/skron-ls-remote.out"
  diff -u "$tmp_dir/git-ls-remote.out" "$tmp_dir/skron-ls-remote.out"
  git -C "$work" ls-remote --heads origin >"$tmp_dir/git-ls-remote-heads.out"
  (cd "$work" && "$skron_bin" ls-remote --heads origin) >"$tmp_dir/skron-ls-remote-heads.out"
  diff -u "$tmp_dir/git-ls-remote-heads.out" "$tmp_dir/skron-ls-remote-heads.out"
  git -C "$work" ls-remote --tags origin >"$tmp_dir/git-ls-remote-tags.out"
  (cd "$work" && "$skron_bin" ls-remote --tags origin) >"$tmp_dir/skron-ls-remote-tags.out"
  diff -u "$tmp_dir/git-ls-remote-tags.out" "$tmp_dir/skron-ls-remote-tags.out"
  echo "ok: ls-remote local repository mutation"
}

compare_local_fetch() {
  local source="$tmp_dir/fetch-source"
  local git_client="$tmp_dir/git-fetch-client"
  local skron_client="$tmp_dir/skron-fetch-client"

  git clone --quiet "$repo_dir" "$source"
  git -C "$source" config user.name Bench
  git -C "$source" config user.email bench@example.test
  git -C "$source" config commit.gpgsign false
  git clone --quiet "$source" "$git_client"
  (cd "$tmp_dir" && "$skron_bin" clone "$source" "$skron_client" >/dev/null)

  git -C "$source" switch -q -c skron-fetch-smoke
  printf 'fetch branch\n' >"$source/skron-fetch-smoke.txt"
  git -C "$source" add -A
  env "${commit_env[@]}" git -C "$source" -c commit.gpgsign=false commit -qm "fetch smoke"
  git -C "$source" switch -q -

  git -C "$git_client" fetch origin >/dev/null
  (cd "$skron_client" && "$skron_bin" fetch origin >/dev/null)
  git -C "$git_client" branch -r >"$tmp_dir/git-fetch-remotes.out"
  (cd "$skron_client" && "$skron_bin" branch -r) >"$tmp_dir/skron-fetch-remotes.out"
  diff -u "$tmp_dir/git-fetch-remotes.out" "$tmp_dir/skron-fetch-remotes.out"
  git -C "$git_client" rev-parse refs/remotes/origin/skron-fetch-smoke >"$tmp_dir/git-fetch-head.out"
  git -C "$skron_client" rev-parse refs/remotes/origin/skron-fetch-smoke >"$tmp_dir/skron-fetch-head.out"
  diff -u "$tmp_dir/git-fetch-head.out" "$tmp_dir/skron-fetch-head.out"
  echo "ok: fetch local repository mutation"
}

compare_local_push() {
  local git_remote="$tmp_dir/git-push-remote.git"
  local skron_remote="$tmp_dir/skron-push-remote.git"
  local git_work="$tmp_dir/git-push-work"
  local skron_work="$tmp_dir/skron-push-work"

  git -C "$tmp_dir" init --bare "$git_remote" >/dev/null
  git -C "$tmp_dir" init --bare "$skron_remote" >/dev/null
  git -C "$tmp_dir" init -q -b main "$git_work"
  (cd "$tmp_dir" && "$skron_bin" init -b main "$skron_work" >/dev/null)
  git -C "$git_work" config user.name Bench
  git -C "$git_work" config user.email bench@example.test
  git -C "$git_work" config commit.gpgsign false
  git -C "$skron_work" config user.name Bench
  git -C "$skron_work" config user.email bench@example.test
  git -C "$skron_work" config commit.gpgsign false
  git -C "$git_work" remote add origin "$git_remote"
  (cd "$skron_work" && "$skron_bin" remote add origin "$skron_remote")

  printf 'push smoke\n' >"$git_work/push-smoke.txt"
  printf 'push smoke\n' >"$skron_work/push-smoke.txt"
  git -C "$git_work" add -A
  (cd "$skron_work" && "$skron_bin" add -A)
  env "${commit_env[@]}" git -C "$git_work" -c commit.gpgsign=false commit -qm "push smoke"
  (
    cd "$skron_work"
    env "${commit_env[@]}" "$skron_bin" commit -m "push smoke" >/dev/null
  )
  git -C "$git_work" push -u origin HEAD >/dev/null
  (cd "$skron_work" && "$skron_bin" push -u origin HEAD >/dev/null)

  git -C "$git_remote" rev-parse refs/heads/main >"$tmp_dir/git-push-head.out"
  git -C "$skron_remote" rev-parse refs/heads/main >"$tmp_dir/skron-push-head.out"
  diff -u "$tmp_dir/git-push-head.out" "$tmp_dir/skron-push-head.out"
  git -C "$git_remote" cat-file -p refs/heads/main^{tree} >"$tmp_dir/git-push-tree.out"
  git -C "$skron_remote" cat-file -p refs/heads/main^{tree} >"$tmp_dir/skron-push-tree.out"
  diff -u "$tmp_dir/git-push-tree.out" "$tmp_dir/skron-push-tree.out"
  echo "ok: push local repository mutation"
}

compare_local_pull() {
  local source="$tmp_dir/pull-source"
  local git_client="$tmp_dir/git-pull-client"
  local skron_client="$tmp_dir/skron-pull-client"

  git clone --quiet "$repo_dir" "$source"
  git -C "$source" config user.name Bench
  git -C "$source" config user.email bench@example.test
  git -C "$source" config commit.gpgsign false
  git clone --quiet "$source" "$git_client"
  (cd "$tmp_dir" && "$skron_bin" clone "$source" "$skron_client" >/dev/null)

  printf 'pull update\n' >"$source/skron-pull-smoke.txt"
  git -C "$source" add -A
  env "${commit_env[@]}" git -C "$source" -c commit.gpgsign=false commit -qm "pull smoke"

  git -C "$git_client" pull --ff-only >/dev/null
  (cd "$skron_client" && "$skron_bin" pull --ff-only >/dev/null)
  git -C "$git_client" rev-parse HEAD >"$tmp_dir/git-pull-head.out"
  git -C "$skron_client" rev-parse HEAD >"$tmp_dir/skron-pull-head.out"
  diff -u "$tmp_dir/git-pull-head.out" "$tmp_dir/skron-pull-head.out"
  git -C "$git_client" cat-file -p HEAD^{tree} >"$tmp_dir/git-pull-tree.out"
  git -C "$skron_client" cat-file -p HEAD^{tree} >"$tmp_dir/skron-pull-tree.out"
  diff -u "$tmp_dir/git-pull-tree.out" "$tmp_dir/skron-pull-tree.out"
  echo "ok: pull local repository mutation"
}

commit_env=(
  GIT_AUTHOR_NAME=Bench
  GIT_AUTHOR_EMAIL=bench@example.test
  GIT_COMMITTER_NAME=Bench
  GIT_COMMITTER_EMAIL=bench@example.test
  GIT_AUTHOR_DATE='1700000000 +0000'
  GIT_COMMITTER_DATE='1700000000 +0000'
)

compare "rev-parse HEAD" rev-parse HEAD
compare "rev-parse --git-dir" rev-parse --git-dir
compare "rev-parse --show-toplevel" rev-parse --show-toplevel
compare "rev-parse --show-prefix" rev-parse --show-prefix
compare "rev-parse --show-cdup" rev-parse --show-cdup
compare "rev-parse --is-inside-work-tree" rev-parse --is-inside-work-tree
compare "rev-parse --is-bare-repository" rev-parse --is-bare-repository
compare "rev-parse --short=12 HEAD" rev-parse --short=12 HEAD
compare "rev-parse --show-object-format" rev-parse --show-object-format
compare "rev-parse HEAD^{tree}" rev-parse 'HEAD^{tree}'
compare "rev-parse HEAD:path" rev-parse "HEAD:$first_file"
compare "cat-file -t HEAD" cat-file -t HEAD
compare "cat-file -s HEAD" cat-file -s HEAD
compare "cat-file -p HEAD" cat-file -p HEAD
compare "cat-file -p HEAD:path" cat-file -p "HEAD:$first_file"
compare "cat-file -s HEAD:path" cat-file -s "HEAD:$first_file"
compare "show HEAD:path" show "HEAD:$first_file"
compare "show HEAD tree" show 'HEAD^{tree}'
compare "show HEAD" show HEAD
compare "show --oneline HEAD" show --oneline HEAD
compare "show --format=%H HEAD patch" show '--format=%H' HEAD
compare "show raw HEAD" show --no-patch --format=raw HEAD
compare "show --format=%H HEAD" show --no-patch '--format=%H' HEAD
compare "show --pretty=format:%an <%ae> HEAD" show --no-patch '--pretty=format:%an <%ae>' HEAD
compare "show --oneline HEAD" show --no-patch --oneline HEAD
compare_stdin "cat-file --batch-check" $'HEAD\nmissing\n' cat-file --batch-check
compare_stdin "cat-file --batch" $'HEAD\nmissing\n' cat-file --batch
compare_stdin "cat-file --batch-command --buffer" $'info HEAD\nflush\ncontents HEAD\nflush\n' cat-file --batch-command --buffer
compare "cat-file --batch-all-objects --batch-check" cat-file --batch-all-objects --batch-check
compare "count-objects" count-objects
compare "count-objects -vH" count-objects -vH
compare "config --get core.bare" config --get core.bare
compare "config remote.origin.url" config remote.origin.url
compare "config --list" config --list
compare "remote" remote
compare "remote -v" remote -v
compare "remote get-url origin" remote get-url origin
compare "ls-files" ls-files
compare "ls-files --stage" ls-files --stage
compare "write-tree" write-tree
compare "show-ref --heads" show-ref --heads
compare "show-ref --branches" show-ref --branches
compare "show-ref --head" show-ref --head
compare "show-ref --hash=12" show-ref --hash=12
compare "show-ref --abbrev=9" show-ref --abbrev=9
compare "for-each-ref" for-each-ref
compare "for-each-ref format heads tags" for-each-ref '--format=%(refname) %(objectname) %(objecttype) %(subject)' refs/heads refs/tags
compare "branch -r" branch -r
compare "branch -a" branch -a
compare "branch --show-current" branch --show-current
compare "symbolic-ref HEAD" symbolic-ref HEAD
compare "symbolic-ref --short HEAD" symbolic-ref --short HEAD
compare "rev-parse --abbrev-ref HEAD" rev-parse --abbrev-ref HEAD
compare "tag -l" tag -l
compare "tag --list wildcard" tag --list '*'
compare "log --max-count 1" log --max-count 1
compare "log --all --format=%H --max-count 2" log --all '--format=%H' --max-count 2
compare "log --oneline --max-count 1" log --oneline --max-count 1
compare "log --parents --oneline --max-count 1" log --parents --oneline --max-count 1
compare "log --reverse --format=%s --max-count 2" log --reverse '--format=%s' --max-count 2
compare "log --format=%H --max-count 1" log --format=%H --max-count 1
compare "log --stat --max-count 1" log --stat --max-count 1
compare "log --numstat --format=%H --max-count 1" log --numstat '--format=%H' --max-count 1
compare "log --shortstat --max-count 1" log --shortstat --max-count 1
compare "log --raw --format=%H --max-count 1" log --raw '--format=%H' --max-count 1
compare "log --summary --format=%H --max-count 1" log --summary '--format=%H' --max-count 1
compare "log --name-only --format=%H --max-count 1" log --name-only '--format=%H' --max-count 1
compare "log --name-status --format=%H --max-count 1" log --name-status '--format=%H' --max-count 1
compare "log --format=%h %s --max-count 1" log '--format=%h %s' --max-count 1
compare "log --pretty=format:%an <%ae> --max-count 1" log '--pretty=format:%an <%ae>' --max-count 1
compare "log --pretty=oneline --max-count 1" log --pretty=oneline --max-count 1
compare "rev-list --max-count 1 HEAD" rev-list --max-count 1 HEAD
compare "rev-list --all --max-count 2" rev-list --all --max-count 2
compare "rev-list HEAD --not HEAD" rev-list HEAD --not HEAD
compare "rev-list HEAD..HEAD" rev-list HEAD..HEAD
compare "rev-list --count HEAD" rev-list --count HEAD
compare "rev-list --parents --max-count 2 HEAD" rev-list --parents --max-count 2 HEAD
compare "rev-list --objects --count HEAD" rev-list --objects --count HEAD
compare "rev-list --objects --all --max-count 2" rev-list --objects --all --max-count 2
compare "rev-list --objects --max-count 2 HEAD" rev-list --objects --max-count 2 HEAD
compare "rev-list --reverse --max-count 2 HEAD" rev-list --reverse --max-count 2 HEAD
compare "merge-base HEAD HEAD" merge-base HEAD HEAD
compare_status "merge-base --is-ancestor HEAD HEAD" merge-base --is-ancestor HEAD HEAD
compare "ls-tree HEAD" ls-tree HEAD
compare "ls-tree -r --name-only HEAD" ls-tree -r --name-only HEAD
compare "status --porcelain=v1 --branch" status --porcelain=v1 --branch
compare "status human" status
if git -C "$repo_dir" rev-parse --verify HEAD~1 >/dev/null 2>&1; then
  compare "diff --name-status HEAD~1 HEAD" diff --name-status HEAD~1 HEAD
  compare "diff --stat HEAD~1 HEAD" diff --stat HEAD~1 HEAD
  compare "diff --raw HEAD~1 HEAD" diff --raw HEAD~1 HEAD
  compare "diff HEAD~1 HEAD" diff HEAD~1 HEAD
fi
compare_mutation
compare_reset_mutation
compare_switch_mutation
compare_restore_mutation
compare_clean_mutation
compare_merge_ff_mutation
compare_update_ref_mutation
compare_rm_mutation
compare_mv_mutation
compare_local_clone
compare_local_ls_remote
compare_local_fetch
compare_local_push
compare_local_pull

echo "real repo smoke passed: $repo_url"
