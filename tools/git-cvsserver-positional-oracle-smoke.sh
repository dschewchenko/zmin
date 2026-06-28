#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

python3 - "$GIT_BIN" "$ZMIN_BIN" <<'PY'
import subprocess
import sys
import tempfile
from pathlib import Path

git_bin = sys.argv[1]
zmin_bin = sys.argv[2]
root = Path(tempfile.mkdtemp(prefix="zmin-cvsserver-oracle-"))

cases = [
    ("cvsserver_positional_unknown_noop", 0, ["unknown"]),
    ("cvsserver_export_all_requires_directory", 255, ["--export-all"]),
    ("cvsserver_strict_paths_noop", 0, ["--strict-paths"]),
    ("cvsserver_base_path_noop", 0, ["--base-path", "/tmp/base"]),
    ("cvsserver_help_short_noop", 0, ["-h"]),
    ("cvsserver_help_short_alt_noop", 0, ["-H"]),
]

for name, expected_exit, args in cases:
    git = subprocess.run([git_bin, "cvsserver", *args], cwd=root, capture_output=True)
    zmin = subprocess.run([zmin_bin, "cvsserver", *args], cwd=root, capture_output=True)
    if git.returncode != expected_exit:
        raise SystemExit(f"{name}: stock git exit {git.returncode} != expected {expected_exit}")
    if zmin.returncode != expected_exit:
        raise SystemExit(f"{name}: zmin exit {zmin.returncode} != expected {expected_exit}")
    if (git.returncode, git.stdout, git.stderr) != (zmin.returncode, zmin.stdout, zmin.stderr):
        raise SystemExit(
            f"{name}: mismatch\n"
            f"git stdout={git.stdout!r}\n"
            f"zmin stdout={zmin.stdout!r}\n"
            f"git stderr={git.stderr!r}\n"
            f"zmin stderr={zmin.stderr!r}"
        )
    print(f"{name}\tok\texit={expected_exit}")
PY
