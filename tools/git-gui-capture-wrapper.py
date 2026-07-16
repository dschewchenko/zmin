#!/usr/bin/env python3
"""Capture privacy-safe Git GUI invocations before execing the selected binary."""

from __future__ import annotations

import hashlib
import json
import os
import pathlib
import re
import sys
import time
import urllib.parse

SCHEMA_VERSION = 1
SENSITIVE_TEXT = re.compile(
    r"(?i)(authorization|credential|oauth|password|private[-_]?key|secret|token)"
)
SAFE_REVISION = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._/@{}^~:+-]*$")
SAFE_ENV_KEYS = (
    "GIT_OPTIONAL_LOCKS",
    "GIT_TERMINAL_PROMPT",
    "GIT_PAGER",
    "GIT_CONFIG_COUNT",
    "LANG",
    "LC_ALL",
)
PRESENCE_ENV_KEYS = (
    "GIT_ASKPASS",
    "SSH_ASKPASS",
    "SSH_AUTH_SOCK",
)
PATH_VALUE_OPTIONS = ("--exec-path", "--git-dir", "--work-tree")


def digest(value: str) -> str:
    return hashlib.sha256(value.encode("utf-8", "surrogateescape")).hexdigest()


def sanitize_url(value: str) -> str | None:
    parsed = urllib.parse.urlsplit(value)
    if parsed.scheme not in {"file", "git", "http", "https", "ssh"}:
        return None
    hostname = parsed.hostname or ""
    if not hostname and parsed.scheme != "file":
        return None
    port = f":{parsed.port}" if parsed.port is not None else ""
    netloc = f"{hostname}{port}"
    suffix = "".join(pathlib.PurePosixPath(parsed.path).suffixes[-2:])
    sanitized_path = "/" if parsed.path == "/" else f"/path-{digest(parsed.path)[:16]}{suffix}"
    return urllib.parse.urlunsplit((parsed.scheme, netloc, sanitized_path, "", ""))


def sanitize_path(value: str) -> str:
    path = pathlib.PurePath(value)
    suffix = "".join(path.suffixes[-2:])
    depth = max(1, len(path.parts))
    return f"<path depth={depth} suffix={suffix or '-'} sha256={digest(value)[:16]}>"


def sanitize_config(value: str) -> str:
    key, separator, config_value = value.partition("=")
    if SENSITIVE_TEXT.search(key):
        return f"{key}=<redacted>" if separator else key
    if not separator:
        return value
    sanitized_url = sanitize_url(config_value)
    if sanitized_url is not None:
        return f"{key}={sanitized_url}"
    if os.path.isabs(config_value):
        return f"{key}={sanitize_path(config_value)}"
    return value


def sanitize_arg(value: str, previous: str | None) -> str:
    if previous == "-c":
        return sanitize_config(value)
    sanitized_url = sanitize_url(value)
    if sanitized_url is not None:
        return sanitized_url
    if SENSITIVE_TEXT.search(value):
        key, separator, _ = value.partition("=")
        return f"{key}=<redacted>" if separator else "<redacted>"
    for option in PATH_VALUE_OPTIONS:
        prefix = f"{option}="
        if value.startswith(prefix):
            path_value = value[len(prefix) :]
            return f"{prefix}{sanitize_path(path_value)}"
    if os.path.isabs(value):
        return sanitize_path(value)
    if value.startswith("-") or SAFE_REVISION.fullmatch(value):
        return value
    if "/" in value or "\\" in value:
        return sanitize_path(value)
    return value


def sanitized_argv(argv: list[str]) -> list[str]:
    result = []
    previous = None
    for value in argv:
        result.append(sanitize_arg(value, previous))
        previous = value
    return result


def repository_context(cwd: pathlib.Path) -> dict[str, object]:
    current = cwd
    depth = 0
    while True:
        marker = current / ".git"
        if marker.is_dir() or marker.is_file():
            return {
                "kind": "worktree",
                "cwd_depth_from_root": depth,
                "root_sha256": digest(str(current)),
            }
        if current.parent == current:
            break
        current = current.parent
        depth += 1
    return {"kind": "outside", "cwd_depth_from_root": None, "root_sha256": None}


def environment_context() -> dict[str, object]:
    values: dict[str, object] = {}
    for key in SAFE_ENV_KEYS:
        if key in os.environ:
            value = os.environ[key]
            values[key] = "<redacted>" if SENSITIVE_TEXT.search(value) else value
    for key in PRESENCE_ENV_KEYS:
        values[f"{key}_present"] = key in os.environ
    return values


def capture_record(argv: list[str]) -> dict[str, object]:
    raw_argv = "\0".join(argv)
    cwd = pathlib.Path.cwd()
    return {
        "schema": SCHEMA_VERSION,
        "timestamp_unix_ns": time.time_ns(),
        "pid": os.getpid(),
        "argv": sanitized_argv(argv),
        "argv_sha256": digest(raw_argv),
        "argc": len(argv),
        "repository": repository_context(cwd),
        "environment": environment_context(),
        "tty": {
            "stdin": sys.stdin.isatty(),
            "stdout": sys.stdout.isatty(),
            "stderr": sys.stderr.isatty(),
        },
    }


def append_record(path: pathlib.Path, record: dict[str, object]) -> None:
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    try:
        path.parent.chmod(0o700)
    except OSError:
        pass
    descriptor = os.open(path, os.O_APPEND | os.O_CREAT | os.O_WRONLY, 0o600)
    try:
        os.write(descriptor, json.dumps(record, separators=(",", ":")).encode() + b"\n")
    finally:
        os.close(descriptor)


def main() -> int:
    target = os.environ.get("ZMIN_GUI_CAPTURE_TARGET", "")
    log_path = os.environ.get("ZMIN_GUI_CAPTURE_LOG", "")
    if not target or not os.path.isabs(target):
        print("ZMIN_GUI_CAPTURE_TARGET must be an absolute executable path", file=sys.stderr)
        return 2
    if not log_path or not os.path.isabs(log_path):
        print("ZMIN_GUI_CAPTURE_LOG must be an absolute path", file=sys.stderr)
        return 2
    if not os.access(target, os.X_OK):
        print(f"ZMIN_GUI_CAPTURE_TARGET is not executable: {target}", file=sys.stderr)
        return 2
    append_record(pathlib.Path(log_path), capture_record(sys.argv[1:]))
    os.execve(target, [target, *sys.argv[1:]], os.environ.copy())
    return 127


if __name__ == "__main__":
    raise SystemExit(main())
