#!/usr/bin/env python3
"""Replay privacy-safe, read-only GUI captures against Git and Zmin."""

from __future__ import annotations

import argparse
import collections
import dataclasses
import hashlib
import json
import os
import pathlib
import shutil
import stat
import subprocess
import sys
import tempfile

SCHEMA_VERSION = 1
MAX_RECORD_BYTES = 1024 * 1024
READ_ONLY_COMMANDS = frozenset(
    {
        "cat-file",
        "diff",
        "diff-files",
        "diff-index",
        "diff-tree",
        "for-each-ref",
        "log",
        "ls-files",
        "ls-tree",
        "merge-base",
        "name-rev",
        "rev-list",
        "rev-parse",
        "show",
        "show-ref",
        "status",
    }
)
GLOBAL_OPTIONS_WITH_VALUE = frozenset(
    {"-c", "-C", "--exec-path", "--git-dir", "--namespace", "--work-tree"}
)
STDIN_OPTIONS = frozenset({"--batch", "--batch-check", "--batch-command", "--stdin"})
CAPTURE_ENV_KEYS = frozenset(
    {"GIT_CONFIG_COUNT", "GIT_OPTIONAL_LOCKS", "GIT_PAGER", "GIT_TERMINAL_PROMPT", "LANG", "LC_ALL"}
)


@dataclasses.dataclass(frozen=True)
class Invocation:
    identifier: str
    argv: tuple[str, ...]
    environment: dict[str, str]


@dataclasses.dataclass(frozen=True)
class ProcessResult:
    exit_code: int
    stdout: bytes
    stderr: bytes


def executable_path(value: str) -> pathlib.Path:
    path = pathlib.Path(value)
    if not path.is_absolute() or not os.access(path, os.X_OK):
        raise argparse.ArgumentTypeError(f"not an absolute executable path: {value}")
    return path


def directory_path(value: str) -> pathlib.Path:
    path = pathlib.Path(value).resolve()
    if not path.is_dir():
        raise argparse.ArgumentTypeError(f"not a directory: {value}")
    return path


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("capture", type=pathlib.Path)
    parser.add_argument("repository", type=directory_path)
    parser.add_argument("--stock-git", required=True, type=executable_path)
    parser.add_argument("--zmin", required=True, type=executable_path)
    parser.add_argument("--out", type=pathlib.Path)
    parser.add_argument("--timeout", type=float, default=30.0)
    return parser.parse_args()


def command_name(argv: tuple[str, ...]) -> str | None:
    index = 0
    while index < len(argv):
        value = argv[index]
        if value in GLOBAL_OPTIONS_WITH_VALUE:
            index += 2
            continue
        if any(value.startswith(f"{option}=") for option in GLOBAL_OPTIONS_WITH_VALUE if option.startswith("--")):
            index += 1
            continue
        if value in {"--literal-pathspecs", "--no-optional-locks", "--no-pager", "--paginate"}:
            index += 1
            continue
        if value.startswith("-"):
            return None
        return value
    return None


def digest(value: str) -> str:
    return hashlib.sha256(value.encode("utf-8", "surrogateescape")).hexdigest()


def skip_reason(record: object, argv: tuple[str, ...], repository_hash: str) -> str | None:
    if not isinstance(record, dict) or record.get("schema") != SCHEMA_VERSION:
        return "schema"
    repository = record.get("repository")
    if not isinstance(repository, dict) or repository.get("kind") != "worktree":
        return "repository"
    if repository.get("cwd_depth_from_root") != 0:
        return "nested_cwd"
    if repository.get("root_sha256") != repository_hash:
        return "repository_mismatch"
    if any("<redacted>" in value or "<path " in value for value in argv):
        return "sanitized_argument"
    if any(value in STDIN_OPTIONS or any(value.startswith(f"{option}=") for option in STDIN_OPTIONS) for value in argv):
        return "stdin"
    command = command_name(argv)
    if command not in READ_ONLY_COMMANDS:
        return "not_read_only"
    return None


def load_invocations(
    path: pathlib.Path, repository_hash: str
) -> tuple[list[Invocation], collections.Counter[str]]:
    invocations = []
    skipped: collections.Counter[str] = collections.Counter()
    seen = set()
    with path.open("rb") as stream:
        for line_number, raw_line in enumerate(stream, start=1):
            if len(raw_line) > MAX_RECORD_BYTES:
                raise ValueError(f"capture line {line_number} exceeds {MAX_RECORD_BYTES} bytes")
            try:
                record = json.loads(raw_line)
            except json.JSONDecodeError as error:
                raise ValueError(f"invalid JSON on capture line {line_number}: {error}") from error
            raw_argv = record.get("argv") if isinstance(record, dict) else None
            if not isinstance(raw_argv, list) or not all(isinstance(value, str) for value in raw_argv):
                skipped["argv"] += 1
                continue
            argv = tuple(raw_argv)
            reason = skip_reason(record, argv, repository_hash)
            if reason is not None:
                skipped[reason] += 1
                continue
            identifier = record.get("argv_sha256")
            if not isinstance(identifier, str) or len(identifier) != 64:
                skipped["identifier"] += 1
                continue
            if identifier in seen:
                skipped["duplicate"] += 1
                continue
            seen.add(identifier)
            raw_environment = record.get("environment", {})
            environment = {
                key: value
                for key, value in raw_environment.items()
                if key in CAPTURE_ENV_KEYS and isinstance(value, str) and value != "<redacted>"
            }
            invocations.append(Invocation(identifier, argv, environment))
    return invocations, skipped


def replay_environment(invocation: Invocation) -> dict[str, str]:
    environment = os.environ.copy()
    for key in tuple(environment):
        if key.startswith("ZMIN_GUI_CAPTURE_"):
            environment.pop(key)
    environment.update(invocation.environment)
    environment["GIT_PAGER"] = "cat"
    environment["GIT_TERMINAL_PROMPT"] = "0"
    return environment


def run_process(
    executable: pathlib.Path,
    invocation: Invocation,
    worktree: pathlib.Path,
    timeout: float,
) -> ProcessResult:
    try:
        completed = subprocess.run(
            [str(executable), *invocation.argv],
            cwd=worktree,
            env=replay_environment(invocation),
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=timeout,
            check=False,
        )
        return ProcessResult(completed.returncode, completed.stdout, completed.stderr)
    except subprocess.TimeoutExpired as error:
        return ProcessResult(124, error.stdout or b"", (error.stderr or b"") + b"replay timed out\n")


def state_snapshot(stock_git: pathlib.Path, worktree: pathlib.Path) -> bytes:
    environment = os.environ.copy()
    environment.update({"GIT_OPTIONAL_LOCKS": "0", "GIT_PAGER": "cat", "GIT_TERMINAL_PROMPT": "0"})
    commands = (
        ("status", "--porcelain=v2", "-z", "--branch", "--untracked-files=all"),
        ("for-each-ref", "--format=%(refname)%00%(objectname)%00%(symref)%00"),
        ("ls-files", "--stage", "-z"),
    )
    snapshot = bytearray()
    for command in commands:
        completed = subprocess.run(
            [str(stock_git), *command],
            cwd=worktree,
            env=environment,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )
        snapshot.extend(f"{command[0]}\0{completed.returncode}\0".encode())
        snapshot.extend(completed.stdout)
        snapshot.extend(b"\0stderr\0")
        snapshot.extend(completed.stderr)
        snapshot.extend(b"\0end\0")
    return bytes(snapshot)


def copy_repository(source: pathlib.Path, destination: pathlib.Path) -> None:
    shutil.copytree(source, destination, symlinks=True, copy_function=copy_replay_entry)


def copy_replay_entry(source: str, destination: str) -> str:
    mode = os.lstat(source).st_mode
    if stat.S_ISSOCK(mode) or stat.S_ISFIFO(mode) or stat.S_ISCHR(mode) or stat.S_ISBLK(mode):
        descriptor = os.open(destination, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
        os.close(descriptor)
        return destination
    return shutil.copy2(source, destination)


def clone_replay_template(source: pathlib.Path, destination: pathlib.Path) -> None:
    destination.mkdir(mode=0o700)
    if sys.platform == "darwin":
        command = ["/bin/cp", "-cR", f"{source}/.", str(destination)]
    elif sys.platform.startswith("linux"):
        command = ["/bin/cp", "-a", "--reflink=auto", f"{source}/.", str(destination)]
    else:
        destination.rmdir()
        copy_repository(source, destination)
        return
    subprocess.run(command, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)


def write_private(path: pathlib.Path, content: bytes) -> None:
    descriptor = os.open(path, os.O_CREAT | os.O_TRUNC | os.O_WRONLY, 0o600)
    try:
        os.write(descriptor, content)
    finally:
        os.close(descriptor)


def preserve_mismatch(
    out_dir: pathlib.Path,
    invocation: Invocation,
    stock: ProcessResult,
    zmin: ProcessResult,
    stock_state: bytes,
    zmin_state: bytes,
) -> None:
    case_dir = out_dir / invocation.identifier[:16]
    case_dir.mkdir(mode=0o700)
    write_private(case_dir / "argv.json", json.dumps(invocation.argv, separators=(",", ":")).encode() + b"\n")
    write_private(case_dir / "stock.stdout", stock.stdout)
    write_private(case_dir / "stock.stderr", stock.stderr)
    write_private(case_dir / "stock.state", stock_state)
    write_private(case_dir / "zmin.stdout", zmin.stdout)
    write_private(case_dir / "zmin.stderr", zmin.stderr)
    write_private(case_dir / "zmin.state", zmin_state)
    write_private(case_dir / "exit-codes.txt", f"stock={stock.exit_code}\nzmin={zmin.exit_code}\n".encode())


def main() -> int:
    args = parse_args()
    if not (args.repository / ".git").exists():
        raise SystemExit(f"repository is not a Git worktree: {args.repository}")
    out_dir = (args.out or pathlib.Path(tempfile.mkdtemp(prefix="zmin-gui-replay."))).resolve()
    if out_dir == args.repository or args.repository in out_dir.parents:
        raise SystemExit("replay output must be outside the source repository")
    out_dir.mkdir(mode=0o700, parents=True, exist_ok=True)
    out_dir.chmod(0o700)
    invocations, skipped = load_invocations(args.capture, digest(str(args.repository)))
    if not invocations:
        print(json.dumps({"replayed": 0, "passed": 0, "mismatched": 0, "skipped": skipped}, sort_keys=True))
        return 2

    replay_root = out_dir / "work"
    replay_root.mkdir(mode=0o700)
    template = replay_root / "template"
    worktree = replay_root / "repository"
    copy_repository(args.repository, template)
    passed = 0
    mismatched = 0
    for invocation in invocations:
        clone_replay_template(template, worktree)
        stock = run_process(args.stock_git, invocation, worktree, args.timeout)
        stock_state = state_snapshot(args.stock_git, worktree)
        shutil.rmtree(worktree)

        clone_replay_template(template, worktree)
        zmin = run_process(args.zmin, invocation, worktree, args.timeout)
        zmin_state = state_snapshot(args.stock_git, worktree)
        shutil.rmtree(worktree)

        if stock == zmin and stock_state == zmin_state:
            passed += 1
            continue
        mismatched += 1
        preserve_mismatch(out_dir, invocation, stock, zmin, stock_state, zmin_state)
    shutil.rmtree(replay_root)
    summary = {
        "replayed": len(invocations),
        "passed": passed,
        "mismatched": mismatched,
        "skipped": dict(sorted(skipped.items())),
        "out": str(out_dir),
    }
    print(json.dumps(summary, sort_keys=True))
    return 1 if mismatched else 0


if __name__ == "__main__":
    raise SystemExit(main())
