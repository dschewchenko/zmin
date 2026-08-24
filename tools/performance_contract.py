#!/usr/bin/env python3
"""Fail-closed identity and evidence contract for local benchmarks."""

from __future__ import annotations

import argparse
import csv
import decimal
from decimal import Decimal, InvalidOperation
import hashlib
import io
import json
import os
import pathlib
import platform
import random
import re
import shutil
import stat
import subprocess
import sys
import math
import secrets
import tempfile
from fractions import Fraction
from typing import Any, BinaryIO, Callable, Iterable


SCHEMA_VERSION = 2
AUTHORITATIVE_PROFILE = "release"
AUTHORITATIVE_WARMUPS = 3
AUTHORITATIVE_MEASURED_PAIRS = 30
AUTHORITATIVE_COLD_STARTS = 10
AUTHORITATIVE_ORDERING = "interleaved-paired"
SAMPLE_PHASE_POLICY = {
    "warmup": {
        "phase": "warmup",
        "filesystem_cache": "warm",
        "filesystem_cache_drop": "no-drop",
    },
    "measured": {
        "phase": "measured",
        "filesystem_cache": "warm",
        "filesystem_cache_drop": "no-drop",
    },
    "cold": {
        "phase": "process-cold",
        "filesystem_cache": "warm",
        "filesystem_cache_drop": "no-drop",
    },
}
STANDARD_MANDATORY_LANES = (
    "init",
    "status",
    "log",
    "rev-list",
    "merge-base",
    "pack-objects",
    "index-pack",
)
OBSERVED_MANDATORY_LANES = (
    "observed_status",
    "observed_show_numstat",
    "observed_ls_files_idea",
    "observed_show_stdin_name_status",
    "observed_rev_parse_repo",
    "observed_config_list",
    "observed_branch_current",
    "observed_for_each_ref",
    "observed_ls_tree",
    "observed_log_unbounded",
)
MANDATORY_LANE_MANIFESTS = {
    "standard": STANDARD_MANDATORY_LANES,
    "observed": OBSERVED_MANDATORY_LANES,
}
EQUIVALENCE_MANIFEST_FIELDS = (
    "lane",
    "sample_kind",
    "phase",
    "pair_id",
    "pair_index",
    "git_order_index",
    "zmin_order_index",
    "git_exit",
    "zmin_exit",
    "git_stdout_sha256",
    "zmin_stdout_sha256",
    "git_stderr_sha256",
    "zmin_stderr_sha256",
    "exit_equal",
    "stdout_equal",
    "stderr_equal",
)
STANDARD_RESULT_FIELDS = (
    "tool",
    "op",
    "sample_kind",
    "pair_id",
    "order_index",
    "real",
    "user",
    "sys",
    "rss_bytes",
    "job_commit_bytes",
    "major_page_faults",
    "minor_page_faults",
    "read_bytes",
    "write_bytes",
    "memory_metric",
    "memory_semantics",
    "memory_scope",
    "memory_unit",
    "metrics_availability",
    "exit",
    "extra",
)
OBSERVED_RESULT_FIELDS = (
    "tool",
    "lane",
    "sample_kind",
    "pair_id",
    "order_index",
    "run",
    "real_seconds",
    "user_seconds",
    "sys_seconds",
    "max_rss_bytes",
    "job_commit_bytes",
    "major_page_faults",
    "minor_page_faults",
    "read_bytes",
    "write_bytes",
    "memory_metric",
    "memory_semantics",
    "memory_scope",
    "memory_unit",
    "metrics_availability",
    "exit",
)
SUPERIORITY_MANIFEST_FIELDS = (
    "lane",
    "sample_kind",
    "sample_count",
    "stock_median_wall_ns",
    "zmin_median_wall_ns",
    "stock_p95_wall_ns",
    "zmin_p95_wall_ns",
    "memory_metric",
    "memory_semantics",
    "memory_scope",
    "memory_unit",
    "stock_peak_memory_bytes",
    "zmin_peak_memory_bytes",
    "stock_peak_memory_kb",
    "zmin_peak_memory_kb",
    "median_wall_strict",
    "p95_wall_strict",
    "peak_memory_strict",
    "median_wall_ratio",
    "median_wall_ci_low",
    "median_wall_ci_high",
    "median_memory_ratio",
    "median_memory_ci_low",
    "median_memory_ci_high",
    "wall_sign_wins",
    "wall_sign_pairs",
    "wall_sign_p",
    "memory_sign_wins",
    "memory_sign_pairs",
    "memory_sign_p",
    "statistics_seed",
    "memory_statistics_seed",
    "statistics_stable",
    "verdict",
)
SUPERIORITY_SAMPLE_COUNTS = {
    "measured": AUTHORITATIVE_MEASURED_PAIRS,
    "cold": AUTHORITATIVE_COLD_STARTS,
}
STATISTICS_POLICY_VERSION = "paired-log-ratio-bootstrap-v1"
STATISTICS_CONFIDENCE = "0.95"
STATISTICS_ALPHA = "0.05"
STATISTICS_BOOTSTRAP_METHOD = "paired-log-ratio-percentile"
STATISTICS_BOOTSTRAP_RESAMPLES = 20_000
STATISTICS_SIGN_TEST = "exact-one-sided-binomial"
STATISTICS_POLICY = {
    "version": STATISTICS_POLICY_VERSION,
    "confidence": STATISTICS_CONFIDENCE,
    "alpha": STATISTICS_ALPHA,
    "bootstrap_method": STATISTICS_BOOTSTRAP_METHOD,
    "bootstrap_resamples": STATISTICS_BOOTSTRAP_RESAMPLES,
    "sign_test": STATISTICS_SIGN_TEST,
    "median_memory_inferential": True,
    "max_memory_guardrail": True,
}
SUPERIORITY_THRESHOLD_ENV_NAMES = (
    "ZMIN_BENCH_MAX_ZMIN_VS_GIT_MEAN_RATIO",
    "ZMIN_BENCH_MAX_ZMIN_VS_GIT_MEDIAN_RATIO",
    "ZMIN_BENCH_MAX_ZMIN_VS_GIT_PAIR_MEDIAN_RATIO",
    "ZMIN_BENCH_MAX_ZMIN_VS_GIT_P95_MEMORY_RATIO",
    "ZMIN_BENCH_MAX_ZMIN_VS_GIX_MEAN_RATIO",
    "ZMIN_BENCH_MAX_ZMIN_VS_GIX_MEDIAN_RATIO",
    "ZMIN_BENCH_MAX_ZMIN_VS_GIX_PAIR_MEDIAN_RATIO",
    "ZMIN_OBSERVED_MAX_TIME_P95_RATIO",
    "ZMIN_OBSERVED_MAX_MEMORY_P95_RATIO",
)
REQUIRED_METRICS = (
    "wall_seconds",
    "user_seconds",
    "sys_seconds",
)
MEMORY_METRICS = ("peak_rss_bytes", "peak_job_commit_bytes")
MEMORY_CONTRACTS = {
    "Linux": {
        "metric": "peak_rss_bytes",
        "semantics": "working_set_peak",
        "scope": "waited_child_processes",
        "unit": "bytes",
    },
    "Darwin": {
        "metric": "peak_rss_bytes",
        "semantics": "working_set_peak",
        "scope": "waited_child_processes",
        "unit": "bytes",
    },
    "Windows": {
        "metric": "peak_job_commit_bytes",
        "semantics": "job_commit_peak",
        "scope": "job_process_tree",
        "unit": "bytes",
    },
}
MAX_NUMERIC_TEXT_LENGTH = 1024
MAX_JSON_BYTES = 32 * 1024 * 1024
MAX_JSON_DEPTH = 128
MAX_JSON_NODES = 100_000
OPTIONAL_METRICS = (
    "major_page_faults",
    "minor_page_faults",
    "read_bytes",
    "write_bytes",
)
METRIC_FIELD_ALIASES = {
    "wall_seconds": ("real", "real_seconds"),
    "user_seconds": ("user", "user_seconds"),
    "sys_seconds": ("sys", "sys_seconds"),
    "peak_rss_bytes": ("rss_bytes", "max_rss_bytes"),
    "peak_job_commit_bytes": ("job_commit_bytes",),
    "major_page_faults": ("major_page_faults",),
    "minor_page_faults": ("minor_page_faults",),
    "read_bytes": ("read_bytes",),
    "write_bytes": ("write_bytes",),
}
FLOAT_METRICS = {"wall_seconds", "user_seconds", "sys_seconds"}
INTEGER_METRICS = set(METRIC_FIELD_ALIASES) - FLOAT_METRICS
HOST_IDENTITY_FIELDS = (
    "os",
    "os_release",
    "kernel",
    "platform",
    "arch",
    "cpu_count",
    "memory_bytes",
    "filesystem_type",
    "filesystem_mount_total_bytes",
)
ENV_KEYS = {
    "CARGO_BUILD_JOBS",
    "CARGO_TARGET_DIR",
    "PATH",
    "HOME",
    "GIT_CONFIG_GLOBAL",
    "GIT_CONFIG_NOSYSTEM",
    "GIT_CONFIG_SYSTEM",
    "GIT_CEILING_DIRECTORIES",
    "GIT_OPTIONAL_LOCKS",
    "GIT_TEMPLATE_DIR",
    "GIT_TERMINAL_PROMPT",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "LOGNAME",
    "USER",
    "PYTHONHASHSEED",
    "RUSTFLAGS",
    "TMPDIR",
    "TZ",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "CURL_CA_BUNDLE",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "ALL_PROXY",
    "NO_PROXY",
    "http_proxy",
    "https_proxy",
    "all_proxy",
    "no_proxy",
}
ENV_PREFIXES = ("ZMIN_BENCH_", "ZMIN_OBSERVED_")
ENVIRONMENT_POLICY = "sanitized-allowlist-v1"
RELEASE_BUILD_MARKER = "zmin-sanitized-release-build-v1"
RELEASE_BUILD_ENVIRONMENT_POLICY = "sanitized-release-build-v1"
AUTHORITATIVE_GIT_COMPARATOR = {
    "tag": "v2.55.0",
    "commit": "e9019fcafe0040228b8631c30f97ae1adb61bcdc",
    "archive_sha256": "72923418db7b26dfddc21e2268660c5118e560bdfaa09b4489b67b38e9b69c49",
}
BUILD_ENVIRONMENT_KEYS = (
    "PATH",
    "HOME",
    "TMPDIR",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "TZ",
    "USER",
    "LOGNAME",
    "CARGO_HOME",
    "CARGO_TARGET_DIR",
    "RUSTC",
    "RUSTFLAGS",
    "RUSTC_WRAPPER",
    "RUSTC_WORKSPACE_WRAPPER",
    "CARGO_BUILD_RUSTC_WRAPPER",
    "CARGO_NET_OFFLINE",
    "CARGO_TERM_COLOR",
    "SOURCE_DATE_EPOCH",
    "GIT_CONFIG_GLOBAL",
    "GIT_CONFIG_NOSYSTEM",
    "GIT_TERMINAL_PROMPT",
)
BUILD_BEHAVIOR_VARIABLE_PATTERNS = (
    "CARGO_CONFIG",
    "CARGO_HOME",
    "CARGO_TARGET_DIR",
    "CARGO_BUILD_RUSTC_WRAPPER",
    "RUSTC",
    "RUSTC_WRAPPER",
    "RUSTC_WORKSPACE_WRAPPER",
    "RUSTFLAGS",
    "RUSTUP_TOOLCHAIN",
    "RUST_LOG",
    "RUST_BACKTRACE",
    "LD_",
    "DYLD_",
)
RELEASE_BUILD_POLICY_FIELDS = (
    "policy",
    "allowed_variables",
    "values",
    "forbidden_extra_variables",
    "forbidden_behavior_patterns",
)
RELEASE_BINARY_FIELDS = ("path", "sha256", "version")
RELEASE_TOOLCHAIN_FIELDS = ("path", "resolved_path", "sha256", "version")
RELEASE_DURABILITY_FIELDS = ("supported", "method", "platform")
RELEASE_CONFIG_FILE_FIELDS = ("path", "sha256", "signature", "bytes")
RELEASE_CONFIG_FIELDS = ("files", "sha256")
RELEASE_SOURCE_SNAPSHOT_FIELDS = ("commit", "dirty", "status_sha256")
RELEASE_SOURCE_INPUT_FIELDS = ("path", "sha256", "signature", "bytes")
RELEASE_SOURCE_INPUTS_FIELDS = ("cargo_lock", "cargo_config")
RELEASE_ENVIRONMENT_FIELDS = (
    *RELEASE_BUILD_POLICY_FIELDS,
    "rejected_inherited_variables",
    "rejected_behavior_variables",
    "repo_root",
    "source_epoch",
    "sha256",
)
RELEASE_MARKER_FIELDS = (
    "schema_version",
    "marker",
    "producer",
    "repo_root",
    "code_commit",
    "code_dirty",
    "code_status_sha256",
    "cargo_profile",
    "cargo_lock_sha256",
    "git",
    "binary",
    "toolchain",
    "python",
    "make",
    "build_manifest",
    "cargo_config",
    "source_snapshot",
    "source_inputs",
    "build_command",
    "sidecar_path",
    "durability",
    "payload_sha256",
)


class ContractError(ValueError):
    """A benchmark contract is incomplete or inconsistent."""


def canonical_json(value: Any) -> bytes:
    return json.dumps(value, sort_keys=True, separators=(",", ":")).encode()


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def normalized_lane_list(value: Any) -> list[str]:
    if isinstance(value, str):
        return [item for item in value.split(",") if item]
    if isinstance(value, (list, tuple)) and all(
        isinstance(item, str) and item for item in value
    ):
        return list(value)
    return []


def mandatory_manifest_digest(name: str, lanes: Iterable[str]) -> str:
    return sha256_bytes(
        canonical_json({"name": name, "lanes": list(lanes)})
    )


def equivalence_plan_digest(
    manifest_name: str,
    lanes: Iterable[str],
    policy: dict[str, Any],
) -> str:
    return sha256_bytes(
        canonical_json(
            {
                "manifest": manifest_name,
                "lanes": list(lanes),
                "sample_counts": {
                    "warmups": policy.get("warmups"),
                    "measured_pairs": policy.get("measured_pairs"),
                    "cold_starts": policy.get("cold_starts"),
                },
                "ordering": policy.get("ordering"),
                "fields": list(EQUIVALENCE_MANIFEST_FIELDS),
            }
        )
    )


def sha256_file(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def file_signature(path: pathlib.Path) -> tuple[int, int, int, int]:
    state = path.stat()
    return (state.st_dev, state.st_ino, state.st_size, state.st_mtime_ns)


def read_regular_descriptor_snapshot(
    descriptor: int,
    path: pathlib.Path,
    *,
    max_bytes: int | None = None,
) -> tuple[bytes, tuple[int, int, int, int]]:
    before = os.fstat(descriptor)
    if not stat.S_ISREG(before.st_mode):
        raise ContractError(f"input is not a regular file: {path}")
    if max_bytes is not None and before.st_size > max_bytes:
        raise ContractError(f"input exceeds the {max_bytes}-byte limit: {path}")
    chunks: list[bytes] = []
    total = 0
    while True:
        chunk = os.read(descriptor, 1024 * 1024)
        if not chunk:
            break
        total += len(chunk)
        if max_bytes is not None and total > max_bytes:
            raise ContractError(
                f"input exceeds the {max_bytes}-byte limit while being read: {path}"
            )
        chunks.append(chunk)
    after = os.fstat(descriptor)
    before_signature = (
        before.st_dev,
        before.st_ino,
        before.st_size,
        before.st_mtime_ns,
    )
    after_signature = (
        after.st_dev,
        after.st_ino,
        after.st_size,
        after.st_mtime_ns,
    )
    if before_signature != after_signature:
        raise ContractError(f"input changed while being read: {path}")
    return b"".join(chunks), after_signature


def read_file_snapshot(
    path: pathlib.Path,
    *,
    max_bytes: int | None = None,
    reject_symlinks: bool = False,
) -> dict[str, Any]:
    try:
        resolved = absolute_file(path, reject_symlinks=reject_symlinks)
        flags = os.O_RDONLY
        nofollow = getattr(os, "O_NOFOLLOW", 0)
        if reject_symlinks and isinstance(nofollow, int):
            flags |= nofollow
        descriptor = os.open(resolved, flags)
    except (OSError, ContractError) as error:
        raise ContractError(f"cannot open input as a regular file: {path}: {error}") from error
    try:
        data, signature = read_regular_descriptor_snapshot(
            descriptor,
            resolved,
            max_bytes=max_bytes,
        )
        path_state = os.stat(resolved, follow_symlinks=False)
    except OSError as error:
        raise ContractError(f"cannot read regular input: {resolved}: {error}") from error
    finally:
        os.close(descriptor)
    current_path = (
        path_state.st_dev,
        path_state.st_ino,
        path_state.st_size,
        path_state.st_mtime_ns,
    )
    if current_path != signature or not stat.S_ISREG(path_state.st_mode):
        raise ContractError(f"input changed while being read: {resolved}")
    return {
        "path": resolved,
        "data": data,
        "signature": signature,
        "sha256": sha256_bytes(data),
    }


def verify_file_snapshots(snapshots: Iterable[dict[str, Any]]) -> list[str]:
    reasons: list[str] = []
    for snapshot in snapshots:
        path = snapshot["path"]
        try:
            current_signature = file_signature(path)
            current_hash = sha256_file(path)
        except OSError as error:
            reasons.append(f"input unavailable during finish: {path}: {error}")
            continue
        if current_signature != snapshot["signature"] or current_hash != snapshot["sha256"]:
            reasons.append(f"input changed during finish: {path}")
    return reasons


def load_json_bytes(data: bytes, path: pathlib.Path) -> dict[str, Any]:
    if len(data) > MAX_JSON_BYTES:
        raise ContractError(
            f"JSON exceeds the {MAX_JSON_BYTES}-byte limit: {path}"
        )

    def strict_object(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
        value: dict[str, Any] = {}
        for key, item in pairs:
            if key in value:
                raise ContractError(f"duplicate JSON key {key!r}: {path}")
            value[key] = item
        return value

    try:
        value = json.loads(data.decode("utf-8"), object_pairs_hook=strict_object)
    except (UnicodeDecodeError, json.JSONDecodeError, RecursionError) as error:
        raise ContractError(f"invalid JSON {path}: {error}") from error
    if not isinstance(value, dict):
        raise ContractError(f"JSON root must be an object: {path}")
    nodes = 0
    pending: list[tuple[Any, int]] = [(value, 1)]
    while pending:
        item, depth = pending.pop()
        nodes += 1
        if nodes > MAX_JSON_NODES:
            raise ContractError(
                f"JSON exceeds the {MAX_JSON_NODES}-node limit: {path}"
            )
        if depth > MAX_JSON_DEPTH:
            raise ContractError(
                f"JSON exceeds the {MAX_JSON_DEPTH}-level depth limit: {path}"
            )
        if isinstance(item, dict):
            pending.extend((child, depth + 1) for child in item.values())
        elif isinstance(item, list):
            pending.extend((child, depth + 1) for child in item)
    return value


def read_strict_tsv_bytes(
    data: bytes,
    path: pathlib.Path,
    *,
    expected_fields: tuple[str, ...] | None = None,
) -> list[dict[str, str]]:
    try:
        text = data.decode("utf-8")
    except UnicodeDecodeError as error:
        raise ContractError(f"result rows are not UTF-8: {path}: {error}") from error
    reader = csv.reader(io.StringIO(text, newline=""), delimiter="\t")
    try:
        fieldnames = next(reader)
    except StopIteration as error:
        raise ContractError(f"result rows have no header: {path}")
    if not fieldnames or any(not field for field in fieldnames):
        raise ContractError(f"result rows have an invalid header: {path}")
    if len(set(fieldnames)) != len(fieldnames):
        raise ContractError(f"result rows have duplicate header fields: {path}")
    if expected_fields is not None and tuple(fieldnames) != expected_fields:
        raise ContractError(f"result rows have an unexpected schema: {path}")
    rows: list[dict[str, str]] = []
    for row_index, values in enumerate(reader, start=2):
        if len(values) != len(fieldnames):
            raise ContractError(
                f"result rows have an extra or missing cell at row {row_index}: {path}"
            )
        rows.append(dict(zip(fieldnames, values)))
    return rows


def read_rows_bytes(data: bytes, path: pathlib.Path) -> list[dict[str, str]]:
    return read_strict_tsv_bytes(data, path)


def atomic_write_bytes(
    path: pathlib.Path,
    data: bytes,
    *,
    reject_symlinks: bool = False,
    require_directory_fsync: bool = False,
    expected_directory_identity: dict[str, Any] | None = None,
    prepublication_check: Callable[[], None] | None = None,
) -> None:
    if expected_directory_identity is not None and reject_symlinks and os.name == "nt":
        raise ContractError(
            "pinned retained-directory publication is unsupported on Windows"
        )
    destination = absolute_output_path(path, reject_symlinks=reject_symlinks)
    if expected_directory_identity is not None and reject_symlinks:
        if not destination.parent.is_dir():
            raise ContractError(
                f"publication directory does not exist: {destination.parent}"
            )
    else:
        destination.parent.mkdir(parents=True, exist_ok=True)
    if reject_symlinks and os.name != "nt":
        _atomic_write_posix_dirfd(
            destination,
            data,
            require_directory_fsync=require_directory_fsync,
            expected_directory_identity=expected_directory_identity,
            prepublication_check=prepublication_check,
        )
        return
    capability = durability_capability(destination.parent)
    if require_directory_fsync and not capability["supported"]:
        raise ContractError(
            "atomic evidence publication is unsupported on this platform: "
            + str(capability["method"])
        )
    temporary_path: pathlib.Path | None = None
    try:
        with tempfile.NamedTemporaryFile(
            dir=destination.parent,
            prefix=f".{destination.name}.",
            suffix=".tmp",
            delete=False,
        ) as handle:
            temporary_path = pathlib.Path(handle.name)
            handle.write(data)
            handle.flush()
            os.fsync(handle.fileno())
        if prepublication_check is not None:
            prepublication_check()
        os.replace(temporary_path, destination)
        if capability["supported"]:
            directory_fd = os.open(destination.parent, os.O_RDONLY)
            try:
                os.fsync(directory_fd)
            finally:
                os.close(directory_fd)
    except OSError as error:
        raise ContractError(f"atomic evidence publication failed: {destination}: {error}") from error
    finally:
        if temporary_path is not None:
            try:
                temporary_path.unlink()
            except FileNotFoundError:
                pass

def _require_posix_publication_primitives() -> tuple[int, int]:
    if os.name == "nt":
        raise ContractError("pinned directory publication is unsupported on Windows")
    directory_flag = getattr(os, "O_DIRECTORY", None)
    no_follow_flag = getattr(os, "O_NOFOLLOW", None)
    if not directory_flag or not no_follow_flag:
        missing = ",".join(
            name
            for name, value in (
                ("O_DIRECTORY", directory_flag),
                ("O_NOFOLLOW", no_follow_flag),
            )
            if not value
        )
        raise ContractError(
            "pinned directory publication requires POSIX primitives: " + missing
        )
    return int(directory_flag), int(no_follow_flag)


def _pinned_directory_fd(
    directory: pathlib.Path,
    expected_identity: dict[str, Any],
) -> tuple[int, dict[str, Any]]:
    if os.name == "nt":
        raise ContractError("pinned directory publication is unsupported on Windows")
    if not isinstance(expected_identity, dict) or not all(
        isinstance(expected_identity.get(key), int) for key in ("st_dev", "st_ino")
    ):
        raise ContractError("publication directory identity is incomplete")
    directory_flag, no_follow_flag = _require_posix_publication_primitives()
    directory_flags = os.O_RDONLY | directory_flag | no_follow_flag
    try:
        descriptor = os.open(directory, directory_flags)
    except OSError as error:
        raise ContractError(f"cannot pin publication directory: {directory}: {error}") from error
    actual = os.fstat(descriptor)
    if (actual.st_dev, actual.st_ino) != (
        expected_identity["st_dev"],
        expected_identity["st_ino"],
    ):
        os.close(descriptor)
        raise ContractError(f"publication directory changed while being pinned: {directory}")
    return descriptor, expected_identity


def _atomic_write_posix_dirfd(
    destination: pathlib.Path,
    data: bytes,
    *,
    require_directory_fsync: bool,
    expected_directory_identity: dict[str, Any] | None,
    prepublication_check: Callable[[], None] | None,
) -> None:
    expected = expected_directory_identity or path_identity(destination.parent)
    descriptor, expected = _pinned_directory_fd(destination.parent, expected)
    temporary_name: str | None = None
    temporary_fd: int | None = None
    try:
        if require_directory_fsync:
            try:
                os.fsync(descriptor)
            except OSError as error:
                raise ContractError(
                    f"publication directory fsync is unsupported: {error}"
                ) from error
        _directory_flag, no_follow_flag = _require_posix_publication_primitives()
        flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | no_follow_flag
        for _ in range(32):
            candidate = f".{destination.name}.{secrets.token_hex(12)}.tmp"
            try:
                temporary_fd = os.open(
                    candidate,
                    flags,
                    0o600,
                    dir_fd=descriptor,
                )
            except FileExistsError:
                continue
            temporary_name = candidate
            break
        if temporary_fd is None or temporary_name is None:
            raise ContractError("could not allocate a unique publication temporary file")
        view = memoryview(data)
        while view:
            written = os.write(temporary_fd, view)
            if written <= 0:
                raise ContractError("publication temporary file write made no progress")
            view = view[written:]
        os.fsync(temporary_fd)
        os.close(temporary_fd)
        temporary_fd = None
        if prepublication_check is not None:
            prepublication_check()
        os.replace(
            temporary_name,
            destination.name,
            src_dir_fd=descriptor,
            dst_dir_fd=descriptor,
        )
        current_fd = os.fstat(descriptor)
        current_path = path_identity(destination.parent)
        if (current_fd.st_dev, current_fd.st_ino) != (expected["st_dev"], expected["st_ino"]):
            raise ContractError("pinned publication directory descriptor changed")
        if current_path != expected:
            raise ContractError("publication directory was replaced during publication")
        try:
            os.fsync(descriptor)
        except OSError as error:
            if require_directory_fsync:
                raise ContractError(f"publication directory fsync failed: {error}") from error
    except OSError as error:
        raise ContractError(f"atomic evidence publication failed: {destination}: {error}") from error
    finally:
        if temporary_fd is not None:
            os.close(temporary_fd)
        if temporary_name is not None:
            try:
                os.unlink(temporary_name, dir_fd=descriptor)
            except FileNotFoundError:
                pass
        os.close(descriptor)


def parse_nonnegative_number(value: str, *, integer: bool) -> bool:
    if len(value) > MAX_NUMERIC_TEXT_LENGTH:
        return False
    if integer:
        if not re.fullmatch(r"[0-9]+", value):
            return False
        return int(value) >= 0
    try:
        number = float(value)
    except (OverflowError, ValueError):
        return False
    return math.isfinite(number) and number >= 0


def _superiority_availability(row: dict[str, str]) -> dict[str, str]:
    raw = row.get("metrics_availability", "")
    values: dict[str, str] = {}
    for item in raw.split(";"):
        name, separator, state = item.partition("=")
        if separator:
            values[name] = state
    return values


def memory_contract_for_metadata(metadata: Mapping[str, Any]) -> dict[str, str] | None:
    host = metadata.get("host")
    os_name = host.get("os") if isinstance(host, dict) else platform.system()
    contract = MEMORY_CONTRACTS.get(str(os_name))
    return None if contract is None else dict(contract)


def _validate_memory_identity(
    row: Mapping[str, str],
    expected: Mapping[str, str] | None,
    label: str,
    reasons: list[str],
) -> None:
    if expected is None:
        reasons.append(f"{label} has unsupported host memory metric contract")
        return
    field_map = {
        "memory_metric": "metric",
        "memory_semantics": "semantics",
        "memory_scope": "scope",
        "memory_unit": "unit",
    }
    for field, contract_field in field_map.items():
        value = row.get(field, "")
        expected_value = expected[contract_field]
        if value != expected_value:
            reasons.append(
                f"{label} {field} must be {expected_value}, got {value or '<missing>'}"
            )


def _superiority_metric_value(
    row: dict[str, str],
    metric: str,
    label: str,
    reasons: list[str],
) -> Fraction | int | None:
    availability = _superiority_availability(row)
    if availability.get(metric) != "available":
        reasons.append(f"{label} superiority {metric} is missing or unsupported")
        return None
    field = next((name for name in METRIC_FIELD_ALIASES[metric] if name in row), None)
    if field is None:
        reasons.append(f"{label} superiority {metric} field is missing")
        return None
    value = row.get(field, "")
    if value == "unsupported" or not value:
        reasons.append(f"{label} superiority {metric} is missing or unsupported")
        return None
    if len(value) > MAX_NUMERIC_TEXT_LENGTH:
        reasons.append(f"{label} superiority {metric} numeric value is too large")
        return None
    if metric == "wall_seconds":
        try:
            number = Decimal(value)
        except InvalidOperation:
            number = Decimal("NaN")
        if not number.is_finite() or number <= 0:
            reasons.append(f"{label} superiority wall_seconds must be finite and positive")
            return None
        try:
            numeric_float = float(number)
        except (OverflowError, ValueError):
            numeric_float = math.nan
        if not math.isfinite(numeric_float) or numeric_float <= 0:
            reasons.append(f"{label} superiority wall_seconds must fit a finite positive float")
            return None
        wall_ns = Fraction(number) * 1_000_000_000
        if wall_ns <= 0:
            reasons.append(f"{label} superiority wall_ns must be positive")
            return None
        return wall_ns
    if not re.fullmatch(r"[0-9]+", value):
        reasons.append(f"{label} superiority {metric} must be a positive integer")
        return None
    numeric = int(value)
    if numeric <= 0:
        reasons.append(f"{label} superiority {metric} must be a positive integer")
        return None
    try:
        numeric_float = float(numeric)
    except (OverflowError, ValueError):
        numeric_float = math.nan
    if not math.isfinite(numeric_float) or numeric_float <= 0:
        reasons.append(f"{label} superiority {metric} must fit a finite positive float")
        return None
    return numeric


def _nearest_rank_p95(values: list[Fraction | int]) -> Fraction | int:
    ordered = sorted(values)
    rank = max(1, (len(ordered) * 95 + 99) // 100)
    return ordered[rank - 1]


def _even_median(values: list[Fraction | int]) -> Fraction:
    ordered = [Fraction(value) for value in sorted(values)]
    middle = len(ordered) // 2
    if len(ordered) % 2:
        return Fraction(ordered[middle], 1)
    return Fraction(ordered[middle - 1] + ordered[middle], 2)


def _canonical_fraction(value: Fraction) -> str:
    if value.denominator == 1:
        return str(value.numerator)
    if value.denominator == 2:
        whole, remainder = divmod(value.numerator, value.denominator)
        if remainder == 1:
            return f"{whole}.5"
    with decimal.localcontext() as context:
        context.prec = max(
            28,
            len(str(abs(value.numerator))) + len(str(value.denominator)) + 10,
        )
        rendered = format(
            Decimal(value.numerator) / Decimal(value.denominator),
            "f",
        )
    return rendered.rstrip("0").rstrip(".") or "0"


def _optional_superiority_thresholds(
    metadata: dict[str, Any],
    summary_rows: list[dict[str, str]],
) -> list[str]:
    environment = metadata.get("environment")
    values = environment.get("values", {}) if isinstance(environment, dict) else {}
    reasons: list[str] = []
    for name in SUPERIORITY_THRESHOLD_ENV_NAMES:
        raw = values.get(name)
        if raw in (None, ""):
            continue
        try:
            threshold = Decimal(str(raw))
        except InvalidOperation:
            threshold = Decimal("NaN")
        if not threshold.is_finite() or threshold <= 0 or threshold > 1:
            reasons.append(
                f"optional superiority threshold {name} must be finite and in (0,1]"
            )
            continue
        if "MEMORY" in name:
            metric_pairs = (
                ("zmin_peak_memory_bytes", "stock_peak_memory_bytes", "peak memory"),
            )
        elif "P95" in name or "TIME" in name:
            metric_pairs = (("zmin_p95_wall_ns", "stock_p95_wall_ns", "p95 wall"),)
        else:
            metric_pairs = (("zmin_median_wall_ns", "stock_median_wall_ns", "median wall"),)
        for row in summary_rows:
            numerator = Decimal(row[metric_pairs[0][0]])
            denominator = Decimal(row[metric_pairs[0][1]])
            if numerator / denominator > threshold:
                reasons.append(
                    f"optional superiority threshold {name} failed for "
                    f"{row['lane']} {row['sample_kind']} {metric_pairs[0][2]}"
                )
    return reasons


def statistics_policy_payload() -> dict[str, Any]:
    """Return the immutable, authenticated inferential statistics policy."""
    return dict(STATISTICS_POLICY)


def _statistics_policy_reasons(metadata: dict[str, Any]) -> list[str]:
    if metadata.get("statistics_policy") != statistics_policy_payload():
        return ["authoritative statistics policy is missing or unauthenticated"]
    return []


def _statistics_seed(
    metadata: dict[str, Any],
    lane: str,
    sample_kind: str,
    metric: str,
) -> int:
    """Derive a stable seed from authenticated evidence identity and scope."""
    identity = {
        "policy_version": STATISTICS_POLICY_VERSION,
        "random_seed": metadata.get("policy", {}).get("random_seed"),
        "code_commit": metadata.get("code_commit"),
        "fixture_sha256": metadata.get("fixture_sha256"),
        "lane": lane,
        "sample_kind": sample_kind,
        "metric": metric,
    }
    return int.from_bytes(hashlib.sha256(canonical_json(identity)).digest()[:8], "big")


def _canonical_float(value: float) -> str:
    if not math.isfinite(value) or value <= 0:
        raise ContractError("statistics value must be finite and positive")
    return format(value, ".15g")


def _exact_one_sided_sign_pvalue(wins: int, count: int) -> Fraction:
    if count <= 0 or wins < 0 or wins > count:
        raise ContractError("invalid exact sign-test inputs")
    return Fraction(
        sum(math.comb(count, index) for index in range(wins, count + 1)),
        2**count,
    )


def _paired_log_ratio_statistics(
    stock_values: list[Fraction | int],
    zmin_values: list[Fraction | int],
    *,
    seed: int,
) -> dict[str, str]:
    """Compute paired log-ratio percentile CI and an exact one-sided sign test."""
    if len(stock_values) != len(zmin_values) or not stock_values:
        raise ContractError("paired statistics require equal non-empty samples")
    logs: list[float] = []
    wins = 0
    sign_pairs = 0
    for stock, zmin in zip(stock_values, zmin_values):
        if stock <= 0 or zmin <= 0:
            raise ContractError("paired statistics require positive samples")
        try:
            stock_float = float(stock)
            zmin_float = float(zmin)
        except (OverflowError, ValueError) as error:
            raise ContractError(
                "paired statistics require finite positive float-representable samples"
            ) from error
        if (
            not math.isfinite(stock_float)
            or not math.isfinite(zmin_float)
            or stock_float <= 0
            or zmin_float <= 0
        ):
            raise ContractError(
                "paired statistics require finite positive float-representable samples"
            )
        try:
            logs.append(math.log(zmin_float) - math.log(stock_float))
        except (OverflowError, ValueError) as error:
            raise ContractError("paired statistics require finite log ratios") from error
        if zmin != stock:
            sign_pairs += 1
        if zmin < stock:
            wins += 1
    logs.sort()
    count = len(logs)
    rng = random.Random(seed)
    medians: list[float] = []
    for _ in range(STATISTICS_BOOTSTRAP_RESAMPLES):
        sample = sorted(logs[rng.randrange(count)] for _ in range(count))
        middle = count // 2
        if count % 2:
            medians.append(sample[middle])
        else:
            medians.append((sample[middle - 1] + sample[middle]) / 2.0)
    medians.sort()
    alpha = float(STATISTICS_ALPHA)
    low_rank = max(1, math.ceil((alpha / 2.0) * STATISTICS_BOOTSTRAP_RESAMPLES))
    high_rank = min(
        STATISTICS_BOOTSTRAP_RESAMPLES,
        math.ceil((1.0 - (alpha / 2.0)) * STATISTICS_BOOTSTRAP_RESAMPLES),
    )
    observed = sorted(logs)
    middle = count // 2
    observed_median = (
        observed[middle]
        if count % 2
        else (observed[middle - 1] + observed[middle]) / 2.0
    )
    pvalue = (
        _exact_one_sided_sign_pvalue(wins, sign_pairs)
        if sign_pairs
        else Fraction(1, 1)
    )
    try:
        ratio = _canonical_float(math.exp(observed_median))
        ci_low = _canonical_float(math.exp(medians[low_rank - 1]))
        ci_high = _canonical_float(math.exp(medians[high_rank - 1]))
    except (OverflowError, ValueError) as error:
        raise ContractError("paired statistics require finite positive ratios") from error
    return {
        "ratio": ratio,
        "ci_low": ci_low,
        "ci_high": ci_high,
        "wins": str(wins),
        "pairs": str(sign_pairs),
        "pvalue": _canonical_fraction(pvalue),
        "stable": str(
            medians[high_rank - 1] < 0.0
            and pvalue <= Fraction(5, 100)
        ).lower(),
    }


def _statistics_verdict(
    metadata: dict[str, Any],
    summary_rows: list[dict[str, str]],
    reasons: list[str],
) -> str:
    if metadata.get("mode") != "authoritative":
        return "non-authoritative"
    if any(row.get("verdict") == "fail" for row in summary_rows):
        return "fail"
    if any(row.get("verdict") == "inconclusive" for row in summary_rows):
        return "inconclusive"
    if reasons:
        return "fail"
    return "pass"


def compute_superiority_summary(
    rows: list[dict[str, str]],
    metadata: dict[str, Any],
) -> tuple[list[dict[str, str]], list[str]]:
    """Recompute strict speed/memory gates from raw rows, never shell summaries."""
    if metadata.get("mode") != "authoritative":
        return [], []
    manifest_name = metadata.get("mandatory_manifest")
    expected_lanes = MANDATORY_LANE_MANIFESTS.get(manifest_name)
    if expected_lanes is None:
        return [], ["authoritative superiority requires a canonical lane manifest"]
    if normalized_lane_list(metadata.get("mandatory_lanes")) != list(expected_lanes):
        return [], ["authoritative superiority lane manifest is incomplete or reordered"]
    policy = metadata.get("policy")
    if not isinstance(policy, dict):
        return [], ["authoritative superiority policy is missing"]
    if (
        policy.get("measured_pairs") != AUTHORITATIVE_MEASURED_PAIRS
        or policy.get("cold_starts") != AUTHORITATIVE_COLD_STARTS
    ):
        return [], ["authoritative superiority requires 30 measured and 10 cold pairs"]
    policy_reasons = _statistics_policy_reasons(metadata)
    if policy_reasons:
        return [], policy_reasons
    memory_contract = memory_contract_for_metadata(metadata)
    if memory_contract is None:
        return [], ["authoritative superiority has no platform memory contract"]
    memory_metric = memory_contract["metric"]

    expected_tools = {"git", "zmin"} if manifest_name == "standard" else {"stock", "zmin"}
    indexed: dict[tuple[str, str], list[dict[str, str]]] = {}
    for row in rows:
        lane = row.get("lane") or row.get("op") or ""
        indexed.setdefault((lane, row.get("sample_kind", "")), []).append(row)

    summary_rows: list[dict[str, str]] = []
    reasons: list[str] = []
    for lane in expected_lanes:
        for sample_kind, expected_count in SUPERIORITY_SAMPLE_COUNTS.items():
            lane_rows = indexed.get((lane, sample_kind), [])
            for tool in sorted(expected_tools):
                tool_rows = [row for row in lane_rows if row.get("tool") == tool]
                expected_ids = expected_pair_ids(lane, sample_kind, expected_count)
                actual_ids = [row.get("pair_id", "") for row in tool_rows]
                if len(tool_rows) != expected_count:
                    reasons.append(
                        f"superiority {lane} {sample_kind} {tool} has "
                        f"{len(tool_rows)} samples, expected {expected_count}"
                    )
                if len(set(actual_ids)) != len(actual_ids) or set(actual_ids) != expected_ids:
                    reasons.append(
                        f"superiority {lane} {sample_kind} {tool} pair coverage is incomplete"
                    )
                if len(tool_rows) != expected_count or set(actual_ids) != expected_ids:
                    continue

            values: dict[str, tuple[list[Fraction | int], list[int]]] = {}
            for tool in sorted(expected_tools):
                tool_rows = [row for row in lane_rows if row.get("tool") == tool]
                tool_rows.sort(key=lambda row: row.get("pair_id", ""))
                wall_values: list[Fraction | int] = []
                memory_values: list[int] = []
                for row in tool_rows:
                    label = f"{lane} {sample_kind} {tool} pair {row.get('pair_id', '')}"
                    _validate_memory_identity(row, memory_contract, label, reasons)
                    wall = _superiority_metric_value(row, "wall_seconds", label, reasons)
                    memory = _superiority_metric_value(row, memory_metric, label, reasons)
                    if wall is not None and memory is not None:
                        wall_values.append(wall)
                        memory_values.append(memory)
                if len(wall_values) != expected_count or len(memory_values) != expected_count:
                    continue
                values[tool] = (wall_values, memory_values)
            if len(values) != 2:
                continue
            stock_wall, stock_memory = values["git" if manifest_name == "standard" else "stock"]
            zmin_wall, zmin_memory = values["zmin"]
            stock_median = _even_median(stock_wall)
            zmin_median = _even_median(zmin_wall)
            stock_p95 = _nearest_rank_p95(stock_wall)
            zmin_p95 = _nearest_rank_p95(zmin_wall)
            stock_peak_bytes = max(stock_memory)
            zmin_peak_bytes = max(zmin_memory)
            stock_peak_kb = (stock_peak_bytes + 1023) // 1024
            zmin_peak_kb = (zmin_peak_bytes + 1023) // 1024
            median_strict = zmin_median < stock_median
            p95_strict = zmin_p95 < stock_p95
            peak_strict = zmin_peak_bytes < stock_peak_bytes
            try:
                wall_stats = _paired_log_ratio_statistics(
                    stock_wall,
                    zmin_wall,
                    seed=_statistics_seed(metadata, lane, sample_kind, "median_wall"),
                )
                memory_stats = _paired_log_ratio_statistics(
                    stock_memory,
                    zmin_memory,
                    seed=_statistics_seed(metadata, lane, sample_kind, "median_memory"),
                )
            except ContractError as error:
                reasons.append(
                    f"superiority {lane} {sample_kind} statistics are invalid: {error}"
                )
                continue
            raw_strict = median_strict and p95_strict and peak_strict
            stable = wall_stats["stable"] == "true" and memory_stats["stable"] == "true"
            verdict = "pass" if raw_strict and stable else (
                "inconclusive" if raw_strict else "fail"
            )
            summary_rows.append(
                {
                    "lane": lane,
                    "sample_kind": sample_kind,
                    "sample_count": str(expected_count),
                    "stock_median_wall_ns": _canonical_fraction(stock_median),
                    "zmin_median_wall_ns": _canonical_fraction(zmin_median),
                    "stock_p95_wall_ns": _canonical_fraction(Fraction(stock_p95)),
                    "zmin_p95_wall_ns": _canonical_fraction(Fraction(zmin_p95)),
                    "memory_metric": memory_contract["metric"],
                    "memory_semantics": memory_contract["semantics"],
                    "memory_scope": memory_contract["scope"],
                    "memory_unit": memory_contract["unit"],
                    "stock_peak_memory_bytes": str(stock_peak_bytes),
                    "zmin_peak_memory_bytes": str(zmin_peak_bytes),
                    "stock_peak_memory_kb": str(stock_peak_kb),
                    "zmin_peak_memory_kb": str(zmin_peak_kb),
                    "median_wall_strict": str(median_strict).lower(),
                    "p95_wall_strict": str(p95_strict).lower(),
                    "peak_memory_strict": str(peak_strict).lower(),
                    "median_wall_ratio": wall_stats["ratio"],
                    "median_wall_ci_low": wall_stats["ci_low"],
                    "median_wall_ci_high": wall_stats["ci_high"],
                    "median_memory_ratio": memory_stats["ratio"],
                    "median_memory_ci_low": memory_stats["ci_low"],
                    "median_memory_ci_high": memory_stats["ci_high"],
                    "wall_sign_wins": wall_stats["wins"],
                    "wall_sign_pairs": wall_stats["pairs"],
                    "wall_sign_p": wall_stats["pvalue"],
                    "memory_sign_wins": memory_stats["wins"],
                    "memory_sign_pairs": memory_stats["pairs"],
                    "memory_sign_p": memory_stats["pvalue"],
                    "statistics_seed": str(
                        _statistics_seed(metadata, lane, sample_kind, "median_wall")
                    ),
                    "memory_statistics_seed": str(
                        _statistics_seed(metadata, lane, sample_kind, "median_memory")
                    ),
                    "statistics_stable": str(stable).lower(),
                    "verdict": verdict,
                }
            )
            if not median_strict:
                reasons.append(f"superiority {lane} {sample_kind} median wall gate failed")
            if not p95_strict:
                reasons.append(f"superiority {lane} {sample_kind} p95 wall gate failed")
            if not peak_strict:
                reasons.append(f"superiority {lane} {sample_kind} peak memory gate failed")
            if raw_strict and wall_stats["stable"] != "true":
                reasons.append(
                    f"superiority {lane} {sample_kind} median wall statistics are inconclusive"
                )
            if raw_strict and memory_stats["stable"] != "true":
                reasons.append(
                    f"superiority {lane} {sample_kind} median memory statistics are inconclusive"
                )
    reasons.extend(_optional_superiority_thresholds(metadata, summary_rows))
    return summary_rows, reasons


def serialize_superiority_summary(rows: list[dict[str, str]]) -> bytes:
    buffer = io.StringIO(newline="")
    writer = csv.DictWriter(
        buffer,
        fieldnames=list(SUPERIORITY_MANIFEST_FIELDS),
        delimiter="\t",
        lineterminator="\n",
        extrasaction="raise",
    )
    writer.writeheader()
    writer.writerows(rows)
    return buffer.getvalue().encode("utf-8")


def read_superiority_summary_bytes(
    data: bytes,
    path: pathlib.Path,
) -> list[dict[str, str]]:
    return read_strict_tsv_bytes(
        data,
        path,
        expected_fields=SUPERIORITY_MANIFEST_FIELDS,
    )


def validate_superiority_summary_artifact(
    data: bytes,
    path: pathlib.Path,
    actual_rows: list[dict[str, str]],
    expected_rows: list[dict[str, str]],
) -> list[str]:
    reasons: list[str] = []
    if actual_rows != expected_rows:
        reasons.append("superiority summary rows do not exactly match recomputed raw-row summary")
    if data != serialize_superiority_summary(expected_rows):
        reasons.append("superiority summary bytes are not canonical")
    if len(actual_rows) != len(expected_rows):
        reasons.append("superiority summary has missing or extra rows")
    return reasons


def reject_symlink_components(path: str | pathlib.Path) -> pathlib.Path:
    candidate = pathlib.Path(path).expanduser()
    if not candidate.is_absolute():
        raise ContractError(f"path must be absolute: {candidate}")
    current = pathlib.Path(candidate.anchor)
    for component in candidate.parts[1:]:
        current /= component
        if current.is_symlink():
            if platform.system() == "Darwin" and current in {
                pathlib.Path("/var"),
                pathlib.Path("/tmp"),
            }:
                current = current.resolve(strict=True)
                continue
            raise ContractError(f"symlink path component is not allowed: {current}")
    return candidate


def path_identity(path: pathlib.Path) -> dict[str, Any]:
    state = path.stat()
    return {
        "platform": os.name,
        "st_dev": int(state.st_dev),
        "st_ino": int(state.st_ino),
    }


def absolute_file(
    path: str | pathlib.Path,
    *,
    executable: bool = False,
    reject_symlinks: bool = False,
) -> pathlib.Path:
    candidate = reject_symlink_components(path) if reject_symlinks else pathlib.Path(path).expanduser()
    resolved = candidate.resolve(strict=True)
    if not resolved.is_file():
        raise ContractError(f"not a file: {resolved}")
    if executable and not os.access(resolved, os.X_OK):
        raise ContractError(f"not executable: {resolved}")
    return resolved


def absolute_directory(
    path: str | pathlib.Path,
    *,
    reject_symlinks: bool = False,
) -> pathlib.Path:
    candidate = reject_symlink_components(path) if reject_symlinks else pathlib.Path(path).expanduser()
    resolved = candidate.resolve(strict=True)
    if not resolved.is_dir():
        raise ContractError(f"not a directory: {resolved}")
    return resolved


def absolute_output_path(path: str | pathlib.Path, *, reject_symlinks: bool = False) -> pathlib.Path:
    candidate = reject_symlink_components(path) if reject_symlinks else pathlib.Path(path).expanduser()
    return candidate.resolve(strict=False)


def prepare_results_directory(
    path: str | pathlib.Path,
    *,
    require_existing: bool,
) -> pathlib.Path:
    candidate = absolute_output_path(path, reject_symlinks=True)
    if require_existing:
        if not candidate.exists():
            raise ContractError(
                f"authoritative retained results directory must already exist: {candidate}"
            )
        directory = absolute_directory(candidate, reject_symlinks=True)
    else:
        if not candidate.exists():
            try:
                candidate.mkdir(parents=True, exist_ok=False)
            except FileExistsError:
                pass
        directory = absolute_directory(candidate, reject_symlinks=True)
    if directory != candidate:
        raise ContractError(f"retained results directory was redirected: {candidate}")
    path_identity(directory)
    return directory


def artifact_identity_token(identity: dict[str, Any]) -> str:
    """Return the compact identity passed between harness and artifact helpers."""
    return f"{int(identity['st_dev'])}:{int(identity['st_ino'])}"


def parse_artifact_identity(value: str | None) -> dict[str, Any] | None:
    if not value:
        return None
    match = re.fullmatch(r"([0-9]+):([0-9]+)", value)
    if match is None:
        raise ContractError(f"invalid artifact directory identity: {value}")
    return {
        "platform": os.name,
        "st_dev": int(match.group(1)),
        "st_ino": int(match.group(2)),
    }


def _artifact_no_follow_flags(base: int) -> int:
    no_follow = getattr(os, "O_NOFOLLOW", None)
    if not no_follow:
        raise ContractError("artifact no-follow file operations are unsupported on this platform")
    return base | no_follow


def _artifact_name_parts(name: str | pathlib.Path) -> tuple[str, ...]:
    candidate = pathlib.PurePath(name)
    if candidate.is_absolute() or not candidate.parts:
        raise ContractError(f"artifact name must be relative: {name}")
    if any(part in {"", ".", ".."} for part in candidate.parts):
        raise ContractError(f"artifact name contains unsafe path components: {name}")
    return tuple(candidate.parts)


def artifact_relative_name(root: pathlib.Path, path: pathlib.Path) -> str:
    root = absolute_directory(root, reject_symlinks=True)
    candidate = absolute_output_path(path, reject_symlinks=True)
    try:
        relative = candidate.relative_to(root)
    except ValueError as error:
        raise ContractError(f"artifact is outside its pinned directory: {candidate}") from error
    return relative.as_posix()


def _open_artifact_root(
    root: pathlib.Path,
    expected_directory_identity: dict[str, Any] | None,
) -> tuple[int, dict[str, Any]]:
    root = absolute_directory(root, reject_symlinks=True)
    directory_flag = getattr(os, "O_DIRECTORY", None)
    if not directory_flag:
        raise ContractError("artifact directory pinning is unsupported on this platform")
    flags = _artifact_no_follow_flags(os.O_RDONLY | directory_flag)
    try:
        descriptor = os.open(root, flags)
    except OSError as error:
        raise ContractError(f"cannot pin artifact directory {root}: {error}") from error
    state = os.fstat(descriptor)
    actual = {
        "platform": os.name,
        "st_dev": int(state.st_dev),
        "st_ino": int(state.st_ino),
    }
    if expected_directory_identity is not None and (
        actual["platform"] != expected_directory_identity.get("platform", actual["platform"])
        or actual["st_dev"] != int(expected_directory_identity["st_dev"])
        or actual["st_ino"] != int(expected_directory_identity["st_ino"])
    ):
        os.close(descriptor)
        raise ContractError("artifact directory identity changed")
    return descriptor, actual


def _open_artifact_parent(root_fd: int, parts: tuple[str, ...]) -> tuple[int, str]:
    if not parts:
        raise ContractError("artifact name is empty")
    parent_fd = os.dup(root_fd)
    directory_flag = getattr(os, "O_DIRECTORY", None)
    if not directory_flag:
        os.close(parent_fd)
        raise ContractError("artifact directory pinning is unsupported on this platform")
    directory_flags = _artifact_no_follow_flags(os.O_RDONLY | directory_flag)
    try:
        for component in parts[:-1]:
            child_fd = os.open(component, directory_flags, dir_fd=parent_fd)
            os.close(parent_fd)
            parent_fd = child_fd
        return parent_fd, parts[-1]
    except OSError as error:
        os.close(parent_fd)
        raise ContractError(f"cannot open artifact parent: {error}") from error


def artifact_preflight_paths(
    root: pathlib.Path,
    names: Iterable[str],
    *,
    expected_directory_identity: dict[str, Any] | None = None,
) -> dict[str, Any]:
    names = tuple(names)
    if len(set(names)) != len(names):
        raise ContractError("artifact plan contains duplicate names")
    root_fd, identity = _open_artifact_root(root, expected_directory_identity)
    try:
        for name in names:
            parts = _artifact_name_parts(name)
            parent_fd, final_name = _open_artifact_parent(root_fd, parts)
            try:
                try:
                    state = os.stat(final_name, dir_fd=parent_fd, follow_symlinks=False)
                except FileNotFoundError:
                    continue
                if stat.S_ISLNK(state.st_mode):
                    raise ContractError(f"planned artifact is a symlink: {name}")
                if not stat.S_ISREG(state.st_mode):
                    raise ContractError(f"planned artifact is not a regular file: {name}")
            finally:
                os.close(parent_fd)
    finally:
        os.close(root_fd)
    return identity


def artifact_open(
    root: pathlib.Path,
    name: str,
    *,
    mode: str,
    expected_directory_identity: dict[str, Any] | None = None,
) -> BinaryIO:
    if mode not in {"write", "append", "exclusive", "read"}:
        raise ContractError(f"unsupported artifact open mode: {mode}")
    parts = _artifact_name_parts(name)
    root_fd, _identity = _open_artifact_root(root, expected_directory_identity)
    parent_fd: int | None = None
    try:
        parent_fd, final_name = _open_artifact_parent(root_fd, parts)
        try:
            state = os.stat(final_name, dir_fd=parent_fd, follow_symlinks=False)
        except FileNotFoundError:
            state = None
        if state is not None:
            if stat.S_ISLNK(state.st_mode):
                raise ContractError(f"planned artifact is a symlink: {name}")
            if not stat.S_ISREG(state.st_mode):
                raise ContractError(f"planned artifact is not a regular file: {name}")
        flags = _artifact_no_follow_flags(
            os.O_RDONLY
            if mode == "read"
            else os.O_WRONLY
            | os.O_CREAT
            | (os.O_APPEND if mode == "append" else 0)
            | (os.O_EXCL if mode == "exclusive" else 0)
            | (os.O_TRUNC if mode == "write" else 0)
        )
        descriptor = os.open(final_name, flags, 0o600, dir_fd=parent_fd)
        return os.fdopen(descriptor, "rb" if mode == "read" else "wb")
    except FileExistsError as error:
        raise ContractError(f"artifact already exists: {name}") from error
    except OSError as error:
        raise ContractError(f"cannot open artifact {name}: {error}") from error
    finally:
        if parent_fd is not None:
            os.close(parent_fd)
        os.close(root_fd)


def artifact_write_bytes(
    root: pathlib.Path,
    name: str,
    data: bytes,
    *,
    append: bool = False,
    exclusive: bool = False,
    expected_directory_identity: dict[str, Any] | None = None,
) -> None:
    mode = "exclusive" if exclusive else "append" if append else "write"
    with artifact_open(
        root,
        name,
        mode=mode,
        expected_directory_identity=expected_directory_identity,
    ) as output:
        output.write(data)
        output.flush()


def artifact_read_bytes(
    root: pathlib.Path,
    name: str,
    *,
    expected_directory_identity: dict[str, Any] | None = None,
    max_bytes: int | None = None,
) -> bytes:
    return artifact_read_snapshot(
        root,
        name,
        expected_directory_identity=expected_directory_identity,
        max_bytes=max_bytes,
    )["data"]


def artifact_read_snapshot(
    root: pathlib.Path,
    name: str,
    *,
    expected_directory_identity: dict[str, Any] | None = None,
    max_bytes: int | None = None,
) -> dict[str, Any]:
    """Read one retained artifact through a pinned no-follow directory fd."""
    parts = _artifact_name_parts(name)
    root = absolute_directory(root, reject_symlinks=True)
    root_fd, _identity = _open_artifact_root(root, expected_directory_identity)
    parent_fd: int | None = None
    descriptor: int | None = None
    try:
        parent_fd, final_name = _open_artifact_parent(root_fd, parts)
        descriptor = os.open(
            final_name,
            _artifact_no_follow_flags(os.O_RDONLY),
            dir_fd=parent_fd,
        )
        data, after_signature = read_regular_descriptor_snapshot(
            descriptor,
            root / name,
            max_bytes=max_bytes,
        )
        return {
            "path": absolute_output_path(root / pathlib.Path(*parts), reject_symlinks=True),
            "data": data,
            "signature": after_signature,
            "sha256": sha256_bytes(data),
        }
    except OSError as error:
        raise ContractError(f"cannot read retained artifact {name}: {error}") from error
    finally:
        if descriptor is not None:
            os.close(descriptor)
        if parent_fd is not None:
            os.close(parent_fd)
        os.close(root_fd)


def verify_retained_file_snapshots(
    root: pathlib.Path,
    expected_directory_identity: dict[str, Any],
    snapshots: Iterable[dict[str, Any]],
) -> list[str]:
    reasons: list[str] = []
    seen: set[pathlib.Path] = set()
    for snapshot in snapshots:
        path = pathlib.Path(snapshot["path"])
        if path in seen:
            continue
        seen.add(path)
        try:
            current = artifact_read_snapshot(
                root,
                artifact_relative_name(root, path),
                expected_directory_identity=expected_directory_identity,
            )
        except (ContractError, OSError) as error:
            reasons.append(f"retained input could not be revalidated: {path}: {error}")
            continue
        if (
            current["signature"] != snapshot["signature"]
            or current["sha256"] != snapshot["sha256"]
        ):
            reasons.append(f"input changed during finish: {path}")
    return reasons


def artifact_copy_file(
    root: pathlib.Path,
    name: str,
    source: pathlib.Path,
    *,
    expected_directory_identity: dict[str, Any] | None = None,
) -> None:
    source = absolute_file(source, reject_symlinks=True)
    source_flags = _artifact_no_follow_flags(os.O_RDONLY)
    try:
        source_fd = os.open(source, source_flags)
        try:
            source_data = b""
            while True:
                chunk = os.read(source_fd, 1024 * 1024)
                if not chunk:
                    break
                source_data += chunk
        finally:
            os.close(source_fd)
    except OSError as error:
        raise ContractError(f"cannot read artifact copy source {source}: {error}") from error
    parts = _artifact_name_parts(name)
    destination = absolute_directory(root, reject_symlinks=True).joinpath(*parts)
    atomic_write_bytes(
        destination,
        source_data,
        reject_symlinks=True,
        expected_directory_identity=expected_directory_identity,
    )


def path_is_within(path: pathlib.Path, directory: pathlib.Path) -> bool:
    try:
        path.relative_to(directory)
    except ValueError:
        return False
    return True


def validate_empty_template_directory(
    fixture_root: pathlib.Path,
    template_directory: pathlib.Path,
    *,
    expected_identity: dict[str, Any] | None = None,
) -> pathlib.Path:
    """Validate the deterministic empty Git template fixture."""
    try:
        fixture_root = absolute_directory(fixture_root, reject_symlinks=True)
        template_directory = absolute_directory(template_directory, reject_symlinks=True)
    except (FileNotFoundError, OSError) as error:
        raise ContractError(
            f"benchmark init template is missing or unavailable: {template_directory}"
        ) from error
    if template_directory == fixture_root or not path_is_within(
        template_directory, fixture_root
    ):
        raise ContractError(
            f"benchmark init template must be contained by the fixture: {template_directory}"
        )
    try:
        next(template_directory.iterdir())
    except StopIteration:
        identity = path_identity(template_directory)
        if expected_identity is not None and identity != expected_identity:
            raise ContractError(
                f"benchmark init template identity changed: {template_directory}"
            )
        return template_directory
    except OSError as error:
        raise ContractError(
            f"cannot inspect benchmark init template: {template_directory}: {error}"
        ) from error
    raise ContractError(
        f"benchmark init template must be empty: {template_directory}"
    )


def benchmark_template_binding(
    fixture_root: pathlib.Path,
    template_directory: pathlib.Path,
    *,
    expected_identity: dict[str, Any] | None = None,
) -> dict[str, Any]:
    validated = validate_empty_template_directory(
        fixture_root,
        template_directory,
        expected_identity=expected_identity,
    )
    return {
        "path": str(validated),
        "identity": path_identity(validated),
    }


def benchmark_template_binding_and_identity_map(
    fixture_root: pathlib.Path,
    template_directory: pathlib.Path,
    *,
    expected_identity: dict[str, Any] | None = None,
) -> tuple[dict[str, Any], dict[str, dict[str, Any]]]:
    binding = benchmark_template_binding(
        fixture_root,
        template_directory,
        expected_identity=expected_identity,
    )
    return binding, {binding["path"]: binding["identity"]}


def publish_benchmark_artifact(
    path: pathlib.Path,
    data: bytes,
    *,
    results_dir: pathlib.Path | None,
    results_dir_identity: dict[str, Any] | None,
    require_directory_fsync: bool,
) -> None:
    """Publish metadata/evidence through the retained directory when supplied."""
    if results_dir is None:
        atomic_write_bytes(
            path,
            data,
            reject_symlinks=False,
            require_directory_fsync=require_directory_fsync,
        )
        return
    retained = absolute_directory(results_dir, reject_symlinks=True)
    if results_dir_identity is None:
        raise ContractError("retained results directory identity is missing")
    destination = absolute_output_path(path, reject_symlinks=True)
    if not path_is_within(destination, retained):
        raise ContractError(
            f"published artifact must be inside the retained results directory: {destination}"
        )
    atomic_write_bytes(
        destination,
        data,
        reject_symlinks=True,
        require_directory_fsync=require_directory_fsync,
        expected_directory_identity=results_dir_identity,
    )


def reject_output_collisions(
    output: pathlib.Path,
    inputs: Iterable[tuple[str, pathlib.Path]],
) -> None:
    for label, path in inputs:
        if output == path:
            raise ContractError(f"evidence output collides with {label}: {path}")


def durability_capability(directory: pathlib.Path) -> dict[str, Any]:
    if os.name == "nt":
        return {
            "supported": False,
            "method": "windows-directory-fsync-not-implemented",
            "platform": os.name,
        }
    try:
        directory_flag, no_follow_flag = _require_posix_publication_primitives()
    except ContractError as error:
        return {
            "supported": False,
            "method": f"directory-fsync-primitives-unavailable:{error}",
            "platform": os.name,
        }
    directory_flags = os.O_RDONLY | directory_flag | no_follow_flag
    try:
        descriptor = os.open(directory, directory_flags)
        expected = path_identity(directory)
        actual = os.fstat(descriptor)
        if (actual.st_dev, actual.st_ino) != (expected["st_dev"], expected["st_ino"]):
            raise OSError("directory identity changed while opening")
        try:
            os.fsync(descriptor)
        finally:
            os.close(descriptor)
    except OSError as error:
        return {
            "supported": False,
            "method": f"directory-fsync-unavailable:{error.__class__.__name__}",
            "platform": os.name,
        }
    return {
        "supported": True,
        "method": "file-fsync-os-replace-directory-fsync",
        "platform": os.name,
    }


def command_output(command: list[str], *, cwd: pathlib.Path | None = None) -> str:
    try:
        completed = subprocess.run(
            command,
            cwd=cwd,
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
        )
    except (OSError, subprocess.CalledProcessError) as error:
        raise ContractError(f"command failed: {' '.join(command)}: {error}") from error
    return completed.stdout.strip()


def trusted_git_binary(path: str | pathlib.Path) -> pathlib.Path:
    return absolute_file(path, executable=True, reject_symlinks=True)


def git_value(git_bin: pathlib.Path, repo_root: pathlib.Path, *args: str) -> str:
    selected_git = trusted_git_binary(git_bin)
    return command_output([str(selected_git), "-C", str(repo_root), *args])


def repo_state(repo_root: pathlib.Path, git_bin: pathlib.Path) -> dict[str, Any]:
    status = git_value(git_bin, repo_root, "status", "--porcelain=v1", "--untracked-files=all")
    return {
        "commit": git_value(git_bin, repo_root, "rev-parse", "HEAD"),
        "dirty": bool(status),
        "status_sha256": sha256_bytes(status.encode()),
        "status_entries": status.splitlines(),
    }


def config_fingerprint(git_bin: pathlib.Path, fixture_root: pathlib.Path) -> str:
    selected_git = trusted_git_binary(git_bin)
    try:
        completed = subprocess.run(
            [str(selected_git), "-C", str(fixture_root), "config", "--null", "--list", "--show-origin"],
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
    except (OSError, subprocess.CalledProcessError) as error:
        raise ContractError(f"cannot capture fixture Git config: {error}") from error
    return sha256_bytes(completed.stdout)


def _tree_fingerprint(root: pathlib.Path, *, skip_git_metadata: bool) -> str:
    if not root.exists():
        raise ContractError(f"fixture root does not exist: {root}")
    digest = hashlib.sha256()
    entries: list[pathlib.Path] = []
    for path in root.rglob("*"):
        relative = path.relative_to(root)
        if skip_git_metadata and relative.parts and relative.parts[0] == ".git":
            continue
        entries.append(path)
    for path in sorted(entries, key=lambda item: item.relative_to(root).as_posix()):
        relative = path.relative_to(root).as_posix().encode()
        mode = path.lstat().st_mode
        digest.update(len(relative).to_bytes(8, "big"))
        digest.update(relative)
        if stat.S_ISLNK(mode):
            digest.update(b"l")
            digest.update(os.readlink(path).encode())
        elif stat.S_ISREG(mode):
            digest.update(b"f")
            digest.update((mode & 0o777).to_bytes(4, "big"))
            digest.update(sha256_file(path).encode())
        elif stat.S_ISDIR(mode):
            digest.update(b"d")
        else:
            digest.update(b"o")
    return digest.hexdigest()


def _git_metadata_paths(
    git_dir: pathlib.Path,
    common_dir: pathlib.Path,
    alternates_path: pathlib.Path,
) -> list[pathlib.Path]:
    paths: set[pathlib.Path] = set()
    for base in (git_dir, common_dir):
        for name in ("HEAD", "config", "config.worktree", "index", "packed-refs", "shallow", "commondir"):
            candidate = base / name
            if candidate.is_file() or candidate.is_symlink():
                paths.add(candidate)
        for subtree in (base / "refs", base / "worktrees"):
            if not subtree.is_dir():
                continue
            for candidate in subtree.rglob("*"):
                if candidate.is_file() or candidate.is_symlink():
                    paths.add(candidate)
    if alternates_path.is_file() or alternates_path.is_symlink():
        paths.add(alternates_path)
    return sorted(paths)


def git_state_fingerprint(root: pathlib.Path, git_bin: pathlib.Path) -> str:
    selected_git = trusted_git_binary(git_bin)
    try:
        git_dir_value = command_output(
            [str(selected_git), "-C", str(root), "rev-parse", "--git-dir"]
        )
        common_dir_value = command_output(
            [str(selected_git), "-C", str(root), "rev-parse", "--git-common-dir"]
        )
    except ContractError as error:
        raise ContractError(f"cannot resolve fixture Git metadata: {error}") from error

    git_dir = pathlib.Path(git_dir_value)
    if not git_dir.is_absolute():
        git_dir = (root / git_dir).resolve()
    else:
        git_dir = git_dir.resolve()
    common_dir = pathlib.Path(common_dir_value)
    if not common_dir.is_absolute():
        common_dir = (root / common_dir).resolve()
    else:
        common_dir = common_dir.resolve()
    alternates_value = command_output(
        [str(selected_git), "-C", str(root), "rev-parse", "--git-path", "objects/info/alternates"]
    )
    alternates_path = pathlib.Path(alternates_value)
    if not alternates_path.is_absolute():
        alternates_path = (root / alternates_path).resolve()
    else:
        alternates_path = alternates_path.resolve()

    digest = hashlib.sha256()
    for label, value in (
        ("git-dir", git_dir),
        ("git-common-dir", common_dir),
        ("objects-info-alternates", alternates_path),
    ):
        digest.update(label.encode())
        digest.update(b"\0")
        digest.update(str(value).encode())
        digest.update(b"\0")
    for path in _git_metadata_paths(git_dir, common_dir, alternates_path):
        relative = str(path).encode()
        mode = path.lstat().st_mode
        digest.update(len(relative).to_bytes(8, "big"))
        digest.update(relative)
        if stat.S_ISLNK(mode):
            digest.update(b"l")
            digest.update(os.readlink(path).encode())
        elif stat.S_ISREG(mode):
            digest.update(b"f")
            digest.update((mode & 0o777).to_bytes(4, "big"))
            digest.update(sha256_file(path).encode())
    return digest.hexdigest()


def fixture_fingerprint(
    root: pathlib.Path,
    git_bin: pathlib.Path,
    *,
    bound_directory_identities: dict[str, dict[str, Any]] | None = None,
) -> str:
    content = _tree_fingerprint(root, skip_git_metadata=True)
    git_state = git_state_fingerprint(root, git_bin)
    payload: dict[str, Any] = {"content": content, "git_state": git_state}
    if bound_directory_identities is not None:
        bound_identities: dict[str, dict[str, Any]] = {}
        for path in sorted(bound_directory_identities):
            directory = absolute_directory(
                pathlib.Path(path),
                reject_symlinks=True,
            )
            actual_identity = path_identity(directory)
            expected_identity = bound_directory_identities[path]
            if actual_identity != expected_identity:
                raise ContractError(f"bound directory identity changed: {directory}")
            bound_identities[str(directory)] = actual_identity
        payload["bound_directory_identities"] = bound_identities
    return sha256_bytes(canonical_json(payload))


def filesystem_type(path: pathlib.Path) -> str:
    commands = [
        ["stat", "-f", "%T", str(path)],
        ["stat", "-f", "-c", "%T", str(path)],
    ]
    for command in commands:
        try:
            value = command_output(command)
        except ContractError:
            continue
        if value:
            return value
    return "unsupported"


def memory_bytes() -> int | None:
    if hasattr(os, "sysconf"):
        try:
            return int(os.sysconf("SC_PHYS_PAGES") * os.sysconf("SC_PAGE_SIZE"))
        except (ValueError, OSError):
            pass
    try:
        value = command_output(["sysctl", "-n", "hw.memsize"])
        return int(value)
    except (ContractError, ValueError):
        return None


def host_facts(path: pathlib.Path) -> dict[str, Any]:
    uname = platform.uname()
    usage = shutil.disk_usage(path)
    return {
        "os": platform.system(),
        "os_release": platform.release(),
        "kernel": uname.release,
        "platform": platform.platform(),
        "arch": platform.machine(),
        "cpu_count": os.cpu_count(),
        "memory_bytes": memory_bytes(),
        "filesystem_type": filesystem_type(path),
        "filesystem_mount_free_bytes": usage.free,
        "filesystem_mount_total_bytes": usage.total,
    }


def relevant_environment() -> dict[str, Any]:
    values = {
        key: value
        for key, value in sorted(os.environ.items())
        if key in ENV_KEYS or key.startswith(ENV_PREFIXES)
    }
    return {
        "values": values,
        "sha256": sha256_bytes(canonical_json(values)),
    }


def normalized_version_output(path: pathlib.Path) -> str:
    version = command_output([str(path), "--version"])
    if any(character in version for character in "\t\r\n"):
        raise ContractError(f"version output is not a single normalized line: {path}")
    return version


def binary_facts(path: pathlib.Path) -> dict[str, Any]:
    return {
        "path": str(path),
        "sha256": sha256_file(path),
        "version": normalized_version_output(path),
    }


def make_binary_facts(path: pathlib.Path) -> dict[str, Any]:
    """Record Make's stable version line and executable identity."""
    candidate = absolute_file(path, executable=True, reject_symlinks=True)
    version_lines = command_output([str(candidate), "--version"]).splitlines()
    if not version_lines or any("\t" in line or "\r" in line for line in version_lines):
        raise ContractError(f"Make version output is not normalized: {candidate}")
    return {
        "path": str(candidate),
        "sha256": sha256_file(candidate),
        "version": version_lines[0],
    }


def toolchain_facts(path: str | pathlib.Path) -> dict[str, Any]:
    candidate = absolute_file(path, executable=True, reject_symlinks=True)
    return {
        "path": str(candidate),
        "resolved_path": str(candidate),
        "sha256": sha256_file(candidate),
        "version": command_output([str(candidate), "--version"]),
    }


def canonical_release_build_policy(
    *,
    repo_root: pathlib.Path,
    git_bin: pathlib.Path,
    cargo_bin: pathlib.Path,
    rustc_bin: pathlib.Path,
    source_epoch: str,
) -> dict[str, Any]:
    target_dir = repo_root / "target"
    cargo_home = target_dir / ".zmin-release-build-cargo-home"
    build_home = target_dir / ".zmin-release-build-home"
    build_tmp = target_dir / ".zmin-release-build-tmp"
    values = {
        "PATH": ":".join(
            [
                str(cargo_bin.parent),
                str(rustc_bin.parent),
                str(git_bin.parent),
                "/usr/bin",
                "/bin",
                "/usr/sbin",
                "/sbin",
            ]
        ),
        "HOME": str(build_home),
        "TMPDIR": str(build_tmp),
        "LANG": "C",
        "LC_ALL": "C",
        "LC_CTYPE": "C",
        "TZ": "UTC",
        "USER": "zmin-build",
        "LOGNAME": "zmin-build",
        "CARGO_HOME": str(cargo_home),
        "CARGO_TARGET_DIR": str(target_dir),
        "RUSTC": str(rustc_bin),
        "RUSTFLAGS": "",
        "RUSTC_WRAPPER": "",
        "RUSTC_WORKSPACE_WRAPPER": "",
        "CARGO_BUILD_RUSTC_WRAPPER": "",
        "CARGO_NET_OFFLINE": "true",
        "CARGO_TERM_COLOR": "never",
        "SOURCE_DATE_EPOCH": source_epoch,
        "GIT_CONFIG_GLOBAL": "/dev/null",
        "GIT_CONFIG_NOSYSTEM": "1",
        "GIT_TERMINAL_PROMPT": "0",
    }
    return {
        "policy": RELEASE_BUILD_ENVIRONMENT_POLICY,
        "allowed_variables": list(BUILD_ENVIRONMENT_KEYS),
        "values": values,
        "forbidden_extra_variables": [],
        "forbidden_behavior_patterns": list(BUILD_BEHAVIOR_VARIABLE_PATTERNS),
        "repo_root": str(repo_root),
        "source_epoch": source_epoch,
    }


def build_environment_facts(policy: dict[str, Any]) -> dict[str, Any]:
    if set(policy) != set(RELEASE_BUILD_POLICY_FIELDS) | {"repo_root", "source_epoch"}:
        raise ContractError("internal release build policy is incomplete")
    rejected: list[str] = []
    payload = {key: policy[key] for key in RELEASE_BUILD_POLICY_FIELDS}
    facts = dict(payload)
    facts["rejected_inherited_variables"] = rejected
    facts["rejected_behavior_variables"] = []
    facts["repo_root"] = policy["repo_root"]
    facts["source_epoch"] = policy["source_epoch"]
    facts["sha256"] = sha256_bytes(
        canonical_json({key: value for key, value in facts.items() if key != "sha256"})
    )
    return facts


def marker_payload(marker: dict[str, Any]) -> dict[str, Any]:
    payload = dict(marker)
    payload.pop("payload_sha256", None)
    return payload


def exact_fields(value: Any, expected: tuple[str, ...], label: str) -> tuple[bool, str]:
    if not isinstance(value, dict):
        return False, f"{label} must be an object"
    if set(value) != set(expected):
        return False, f"{label} has unknown or missing fields"
    return True, "matched"


def validate_snapshot_descriptor(value: Any, label: str) -> tuple[bool, str]:
    ok, detail = exact_fields(value, RELEASE_SOURCE_INPUT_FIELDS, label)
    if not ok:
        return ok, detail
    if not isinstance(value["path"], str) or not isinstance(value["sha256"], str):
        return False, f"{label} path or digest is invalid"
    signature = value["signature"]
    if not isinstance(signature, list) or len(signature) != 4 or any(
        isinstance(item, bool) or not isinstance(item, int) for item in signature
    ):
        return False, f"{label} signature is invalid"
    if isinstance(value["bytes"], bool) or not isinstance(value["bytes"], int) or value["bytes"] < 0:
        return False, f"{label} byte count is invalid"
    return True, "matched"


def validate_release_marker_schema(marker: Any) -> tuple[bool, str]:
    ok, detail = exact_fields(marker, RELEASE_MARKER_FIELDS, "release marker")
    if not ok:
        return ok, detail
    if marker["schema_version"] != SCHEMA_VERSION + 1:
        return False, "release marker schema version is unknown"
    if not isinstance(marker["build_command"], list) or not all(
        isinstance(item, str) for item in marker["build_command"]
    ):
        return False, "release marker build command is invalid"
    for label in ("git", "binary", "python", "make"):
        ok, detail = exact_fields(marker[label], RELEASE_BINARY_FIELDS, label)
        if not ok:
            return ok, detail
    ok, detail = exact_fields(marker["toolchain"], ("cargo", "rustc"), "toolchain")
    if not ok:
        return ok, detail
    for label in ("cargo", "rustc"):
        ok, detail = exact_fields(
            marker["toolchain"][label],
            RELEASE_TOOLCHAIN_FIELDS,
            f"toolchain.{label}",
        )
        if not ok:
            return ok, detail
    ok, detail = exact_fields(marker["build_manifest"], RELEASE_ENVIRONMENT_FIELDS, "build manifest")
    if not ok:
        return ok, detail
    ok, detail = exact_fields(marker["cargo_config"], RELEASE_CONFIG_FIELDS, "Cargo config")
    if not ok:
        return ok, detail
    if not isinstance(marker["cargo_config"]["files"], list):
        return False, "Cargo config files must be a list"
    for item in marker["cargo_config"]["files"]:
        ok, detail = validate_snapshot_descriptor(item, "Cargo config file")
        if not ok:
            return ok, detail
    ok, detail = exact_fields(
        marker["source_snapshot"],
        RELEASE_SOURCE_SNAPSHOT_FIELDS,
        "source snapshot",
    )
    if not ok:
        return ok, detail
    ok, detail = exact_fields(
        marker["source_inputs"],
        RELEASE_SOURCE_INPUTS_FIELDS,
        "source inputs",
    )
    if not ok:
        return ok, detail
    if not isinstance(marker["source_inputs"]["cargo_config"], list):
        return False, "Cargo config snapshots must be a list"
    ok, detail = validate_snapshot_descriptor(
        marker["source_inputs"]["cargo_lock"], "Cargo.lock snapshot"
    )
    if not ok:
        return ok, detail
    for item in marker["source_inputs"]["cargo_config"]:
        ok, detail = validate_snapshot_descriptor(item, "Cargo config snapshot")
        if not ok:
            return ok, detail
    ok, detail = exact_fields(marker["durability"], RELEASE_DURABILITY_FIELDS, "durability")
    if not ok:
        return ok, detail
    if not isinstance(marker["durability"]["supported"], bool):
        return False, "release marker durability support flag is invalid"
    if not isinstance(marker["payload_sha256"], str):
        return False, "release marker payload digest is invalid"
    return True, "matched"


def snapshot_descriptor(snapshot: dict[str, Any]) -> dict[str, Any]:
    return {
        "path": str(snapshot["path"]),
        "sha256": snapshot["sha256"],
        "signature": list(snapshot["signature"]),
        "bytes": len(snapshot["data"]),
    }


def cargo_config_snapshots(
    repo_root: pathlib.Path,
    cargo_home: pathlib.Path,
) -> list[dict[str, Any]]:
    candidates: list[pathlib.Path] = []
    current = repo_root
    while True:
        candidates.extend(
            [current / ".cargo" / "config", current / ".cargo" / "config.toml"]
        )
        if current.parent == current:
            break
        current = current.parent
    candidates.extend([cargo_home / "config", cargo_home / "config.toml"])
    snapshots: list[dict[str, Any]] = []
    for candidate in candidates:
        if not candidate.exists():
            continue
        candidate = absolute_file(candidate, reject_symlinks=True)
        if not candidate.is_file():
            raise ContractError(f"Cargo config input is not a file: {candidate}")
        snapshots.append(read_file_snapshot(candidate))
    return snapshots


def cargo_config_facts_from_snapshots(
    snapshots: Iterable[dict[str, Any]],
) -> dict[str, Any]:
    files = [snapshot_descriptor(snapshot) for snapshot in snapshots]
    return {
        "files": files,
        "sha256": sha256_bytes(canonical_json(files)),
    }


def cargo_config_facts(repo_root: pathlib.Path, cargo_home: pathlib.Path) -> dict[str, Any]:
    return cargo_config_facts_from_snapshots(
        cargo_config_snapshots(repo_root, cargo_home)
    )


def validate_build_environment_facts(build_environment: Any) -> tuple[bool, str]:
    ok, detail = exact_fields(build_environment, RELEASE_ENVIRONMENT_FIELDS, "build environment")
    if not ok:
        return ok, detail
    if build_environment.get("policy") != RELEASE_BUILD_ENVIRONMENT_POLICY:
        return False, "release build marker has unknown build environment policy"
    allowed = build_environment.get("allowed_variables")
    values = build_environment.get("values")
    forbidden_extra = build_environment.get("forbidden_extra_variables")
    forbidden_patterns = build_environment.get("forbidden_behavior_patterns")
    rejected = build_environment.get("rejected_inherited_variables")
    rejected_behavior = build_environment.get("rejected_behavior_variables")
    if allowed != list(BUILD_ENVIRONMENT_KEYS):
        return False, "release build marker has a non-canonical environment allowlist"
    if not isinstance(values, dict) or set(values) != set(BUILD_ENVIRONMENT_KEYS):
        return False, "release build marker has incomplete build environment values"
    if forbidden_extra != []:
        return False, "release build marker permits extra build environment variables"
    if forbidden_patterns != list(BUILD_BEHAVIOR_VARIABLE_PATTERNS):
        return False, "release build marker has non-canonical forbidden behavior patterns"
    if not isinstance(rejected, list) or not all(isinstance(item, str) for item in rejected):
        return False, "release build marker has invalid rejected environment facts"
    if not isinstance(rejected_behavior, list) or not all(
        isinstance(item, str) for item in rejected_behavior
    ):
        return False, "release build marker has invalid rejected behavior facts"
    if not isinstance(build_environment.get("repo_root"), str) or not isinstance(
        build_environment.get("source_epoch"), str
    ):
        return False, "release build marker has incomplete source policy facts"
    expected_rejected_behavior = [
        name
        for name in rejected
        if any(
            name == pattern or name.startswith(pattern)
            for pattern in BUILD_BEHAVIOR_VARIABLE_PATTERNS
        )
    ]
    if rejected_behavior != expected_rejected_behavior:
        return False, "release build marker rejected behavior facts are inconsistent"
    if rejected or rejected_behavior:
        return False, "release build marker contains ambient environment facts"
    payload = {
        key: build_environment[key]
        for key in RELEASE_ENVIRONMENT_FIELDS
        if key != "sha256"
    }
    if build_environment.get("sha256") != sha256_bytes(canonical_json(payload)):
        return False, "release build marker build environment digest mismatch"
    if values.get("RUSTFLAGS") or values.get("RUSTC_WRAPPER"):
        return False, "release build marker allows unsafe Rust build injection"
    if values.get("RUSTC_WORKSPACE_WRAPPER") or values.get("CARGO_BUILD_RUSTC_WRAPPER"):
        return False, "release build marker allows Rust wrapper injection"
    return True, "matched"


def profile_from_path(path: pathlib.Path) -> str:
    for profile in ("release", "compat", "debug"):
        if profile in path.parts:
            return profile
    return "unknown"


def canonical_release_target_dir(repo_root: pathlib.Path) -> pathlib.Path:
    return repo_root / "target"


def canonical_release_binary(repo_root: pathlib.Path) -> pathlib.Path:
    binary_name = "zmin.exe" if os.name == "nt" else "zmin"
    return canonical_release_target_dir(repo_root) / "release" / binary_name


def identity_sidecar(path: pathlib.Path) -> pathlib.Path:
    return pathlib.Path(f"{path}.identity.json")


def load_json(path: pathlib.Path) -> dict[str, Any]:
    snapshot = read_file_snapshot(
        path,
        max_bytes=MAX_JSON_BYTES,
        reject_symlinks=True,
    )
    return load_json_bytes(snapshot["data"], snapshot["path"])


def verify_release_build_inputs(
    *,
    repo_root: pathlib.Path,
    git_bin: pathlib.Path,
    state_before: dict[str, Any],
    lock_snapshot: dict[str, Any],
    config_snapshots: list[dict[str, Any]],
    config_before: dict[str, Any],
    cargo_bin: pathlib.Path,
    cargo_facts: dict[str, Any],
    rustc_bin: pathlib.Path,
    rustc_facts: dict[str, Any],
    python_bin: pathlib.Path,
    python_facts: dict[str, Any],
    make_bin: pathlib.Path,
    make_facts: dict[str, Any],
    policy: dict[str, Any],
    build_environment: dict[str, str],
    cargo_home: pathlib.Path,
    source_epoch: str,
) -> dict[str, Any]:
    for snapshot in [lock_snapshot, *config_snapshots]:
        try:
            if absolute_file(snapshot["path"], reject_symlinks=True) != snapshot["path"]:
                raise ContractError(
                    f"input path changed during sanitized release build: {snapshot['path']}"
                )
        except (ContractError, OSError) as error:
            raise ContractError(str(error)) from error
    snapshot_reasons = verify_file_snapshots([lock_snapshot, *config_snapshots])
    if snapshot_reasons:
        raise ContractError("; ".join(snapshot_reasons))
    state_after = repo_state(repo_root, git_bin)
    if state_after != state_before:
        raise ContractError("source state changed during sanitized release build")
    if absolute_directory(repo_root, reject_symlinks=True) != repo_root:
        raise ContractError("source path changed during sanitized release build")
    if toolchain_facts(cargo_bin) != cargo_facts or toolchain_facts(rustc_bin) != rustc_facts:
        raise ContractError("cargo or rustc changed during sanitized release build")
    python_path_after = absolute_file(
        python_bin,
        executable=True,
        reject_symlinks=True,
    )
    if binary_facts(python_path_after) != python_facts:
        raise ContractError("Python changed during sanitized release build")
    make_path_after = absolute_file(
        make_bin,
        executable=True,
        reject_symlinks=True,
    )
    if make_binary_facts(make_path_after) != make_facts:
        raise ContractError("Make changed during sanitized release build")
    if absolute_file(lock_snapshot["path"], reject_symlinks=True) != lock_snapshot["path"]:
        raise ContractError("Cargo.lock path changed during sanitized release build")
    if cargo_config_facts(repo_root, cargo_home) != config_before:
        raise ContractError("Cargo config changed during sanitized release build")
    policy_after = canonical_release_build_policy(
        repo_root=repo_root,
        git_bin=git_bin,
        cargo_bin=cargo_bin,
        rustc_bin=rustc_bin,
        source_epoch=source_epoch,
    )
    if policy_after != policy or dict(policy_after["values"]) != build_environment:
        raise ContractError("sanitized release build policy changed during build")
    return state_after


def build_release_identity(args: argparse.Namespace) -> dict[str, Any]:
    repo_root = absolute_directory(args.repo_root, reject_symlinks=True)
    git_bin = trusted_git_binary(args.git_bin)
    cargo_bin = absolute_file(args.cargo_bin, executable=True, reject_symlinks=True)
    rustc_bin = absolute_file(args.rustc_bin, executable=True, reject_symlinks=True)
    cargo_facts = toolchain_facts(cargo_bin)
    rustc_facts = toolchain_facts(rustc_bin)
    git_facts = binary_facts(git_bin)
    python_bin = absolute_file(args.python_bin, executable=True, reject_symlinks=True)
    make_bin = absolute_file(args.make_bin, executable=True, reject_symlinks=True)
    target_dir = absolute_output_path(
        canonical_release_target_dir(repo_root),
        reject_symlinks=True,
    )
    binary = canonical_release_binary(repo_root)
    sidecar = identity_sidecar(binary)
    expected_cargo_home = target_dir / ".zmin-release-build-cargo-home"
    cargo_home = expected_cargo_home
    cargo_home.mkdir(parents=True, exist_ok=True)
    target_dir.mkdir(parents=True, exist_ok=True)
    build_home = target_dir / ".zmin-release-build-home"
    build_tmp = target_dir / ".zmin-release-build-tmp"
    build_home.mkdir(parents=True, exist_ok=True)
    build_tmp.mkdir(parents=True, exist_ok=True)
    lock = repo_root / "Cargo.lock"
    if not lock.is_file():
        raise ContractError(f"missing Cargo.lock: {lock}")
    lock = absolute_file(lock, reject_symlinks=True)
    state_before = repo_state(repo_root, git_bin)
    if state_before["dirty"]:
        raise ContractError("sanitized release build requires a clean source tree")
    source_epoch = git_value(git_bin, repo_root, "show", "-s", "--format=%ct", "HEAD")
    policy = canonical_release_build_policy(
        repo_root=repo_root,
        git_bin=git_bin,
        cargo_bin=cargo_bin,
        rustc_bin=rustc_bin,
        source_epoch=source_epoch,
    )
    build_environment = dict(policy["values"])
    build_manifest = build_environment_facts(policy)
    lock_snapshot = read_file_snapshot(lock)
    config_snapshots = cargo_config_snapshots(repo_root, cargo_home)
    config_before = cargo_config_facts_from_snapshots(config_snapshots)
    python_facts_before = binary_facts(python_bin)
    make_facts_before = make_binary_facts(make_bin)
    command = [
        str(cargo_bin),
        "build",
        "--manifest-path",
        str(repo_root / "Cargo.toml"),
        "--release",
        "--locked",
        "--offline",
        "-p",
        "zmin-cli",
        "--bin",
        "zmin",
    ]
    try:
        completed = subprocess.run(
            command,
            cwd=repo_root,
            env=build_environment,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            check=False,
        )
    except OSError as error:
        raise ContractError(f"sanitized release build could not start: {error}") from error
    if completed.returncode != 0:
        detail = (
            completed.stderr.splitlines()[-1]
            if completed.stderr.splitlines()
            else "no compiler diagnostics"
        )
        raise ContractError(f"sanitized release build failed: {detail}")
    state_after = verify_release_build_inputs(
        repo_root=repo_root,
        git_bin=git_bin,
        state_before=state_before,
        lock_snapshot=lock_snapshot,
        config_snapshots=config_snapshots,
        config_before=config_before,
        cargo_bin=cargo_bin,
        cargo_facts=cargo_facts,
        rustc_bin=rustc_bin,
        rustc_facts=rustc_facts,
        python_bin=python_bin,
        python_facts=python_facts_before,
        make_bin=make_bin,
        make_facts=make_facts_before,
        policy=policy,
        build_environment=build_environment,
        cargo_home=cargo_home,
        source_epoch=source_epoch,
    )
    binary = absolute_file(binary, executable=True, reject_symlinks=True)
    binary_after = binary_facts(binary)
    sidecar_parent_identity = path_identity(sidecar.parent)
    marker = {
        "schema_version": SCHEMA_VERSION + 1,
        "marker": RELEASE_BUILD_MARKER,
        "producer": "tools/performance_contract.py build-release",
        "repo_root": str(repo_root),
        "code_commit": state_after["commit"],
        "code_dirty": state_after["dirty"],
        "code_status_sha256": state_after["status_sha256"],
        "cargo_profile": "release",
        "cargo_lock_sha256": lock_snapshot["sha256"],
        "git": git_facts,
        "binary": binary_after,
        "toolchain": {"cargo": cargo_facts, "rustc": rustc_facts},
        "python": python_facts_before,
        "make": make_facts_before,
        "build_manifest": build_manifest,
        "cargo_config": config_before,
        "source_snapshot": {
            "commit": state_before["commit"],
            "dirty": state_before["dirty"],
            "status_sha256": state_before["status_sha256"],
        },
        "source_inputs": {
            "cargo_lock": snapshot_descriptor(lock_snapshot),
            "cargo_config": [snapshot_descriptor(snapshot) for snapshot in config_snapshots],
        },
        "build_command": command,
        "sidecar_path": str(sidecar),
        "durability": durability_capability(sidecar.parent),
    }
    marker["payload_sha256"] = sha256_bytes(canonical_json(marker_payload(marker)))
    schema_ok, schema_detail = validate_release_marker_schema(marker)
    if not schema_ok:
        raise ContractError(f"release marker construction failed: {schema_detail}")
    def final_input_check() -> None:
        verify_release_build_inputs(
            repo_root=repo_root,
            git_bin=git_bin,
            state_before=state_before,
            lock_snapshot=lock_snapshot,
            config_snapshots=config_snapshots,
            config_before=config_before,
            cargo_bin=cargo_bin,
            cargo_facts=cargo_facts,
            rustc_bin=rustc_bin,
            rustc_facts=rustc_facts,
            python_bin=python_bin,
            python_facts=python_facts_before,
            make_bin=make_bin,
            make_facts=make_facts_before,
            policy=policy,
            build_environment=build_environment,
            cargo_home=cargo_home,
            source_epoch=source_epoch,
        )

    atomic_write_bytes(
        sidecar,
        canonical_json(marker) + b"\n",
        reject_symlinks=True,
        expected_directory_identity=sidecar_parent_identity,
        prepublication_check=final_input_check,
    )
    return marker


def sidecar_matches(
    sidecar: dict[str, Any] | None,
    *,
    repo_root: pathlib.Path,
    git_bin: pathlib.Path,
    binary: pathlib.Path,
    profile: str,
    state: dict[str, Any],
    cargo_lock_sha256: str,
    python_bin: pathlib.Path,
    make_bin: pathlib.Path,
) -> tuple[bool, str]:
    if sidecar is None:
        return False, "missing binary identity sidecar"
    if sidecar.get("schema_version") != SCHEMA_VERSION + 1:
        return False, "binary identity sidecar has an unknown schema"
    if sidecar.get("marker") != RELEASE_BUILD_MARKER:
        return False, "binary identity sidecar was not produced by the sanitized release builder"
    if sidecar.get("producer") != "tools/performance_contract.py build-release":
        return False, "binary identity sidecar producer is not the sanitized release builder"
    schema_ok, schema_detail = validate_release_marker_schema(sidecar)
    if not schema_ok:
        return False, schema_detail
    if sidecar.get("payload_sha256") != sha256_bytes(
        canonical_json(marker_payload(sidecar))
    ):
        return False, "release marker payload digest mismatch"
    build_manifest = sidecar["build_manifest"]
    environment_ok, environment_detail = validate_build_environment_facts(build_manifest)
    if not environment_ok:
        return False, environment_detail
    try:
        expected_binary = canonical_release_binary(repo_root)
        expected_target_dir = canonical_release_target_dir(repo_root)
    except (ContractError, KeyError, TypeError) as error:
        return False, f"release marker canonical target directory is invalid: {error}"
    if binary != expected_binary:
        return False, "binary path is not the canonical Cargo release output"
    if build_manifest["values"].get("CARGO_TARGET_DIR") != str(expected_target_dir):
        return False, "release marker target directory is not canonical"
    if sidecar.get("sidecar_path") != str(identity_sidecar(expected_binary)):
        return False, "release marker sidecar path is not canonical"
    if sidecar.get("durability") != durability_capability(expected_binary.parent):
        return False, "release marker sidecar durability facts changed"
    binary_data = sidecar.get("binary")
    if not isinstance(binary_data, dict):
        return False, "binary identity sidecar has no binary facts"
    expected = {
        "repo_root": str(repo_root),
        "code_commit": state["commit"],
        "code_dirty": state["dirty"],
        "code_status_sha256": state["status_sha256"],
        "cargo_profile": profile,
        "cargo_lock_sha256": cargo_lock_sha256,
    }
    for key, value in expected.items():
        if sidecar.get(key) != value:
            return False, f"binary identity sidecar mismatch for {key}"
    try:
        actual_git = binary_facts(trusted_git_binary(git_bin))
    except (ContractError, OSError) as error:
        return False, f"current Git facts are invalid: {error}"
    if sidecar["git"] != actual_git:
        return False, "binary identity sidecar Git facts changed"
    if sidecar.get("source_snapshot") != {
        "commit": state["commit"],
        "dirty": state["dirty"],
        "status_sha256": state["status_sha256"],
    }:
        return False, "binary identity sidecar source snapshot mismatch"
    try:
        current_lock = read_file_snapshot(
            absolute_file(repo_root / "Cargo.lock", reject_symlinks=True)
        )
        current_config = cargo_config_facts(
            repo_root,
            pathlib.Path(
                build_manifest["values"]["CARGO_HOME"]
            ),
        )
    except (ContractError, KeyError, OSError, TypeError) as error:
        return False, f"current release source inputs are invalid: {error}"
    if sidecar["source_inputs"]["cargo_lock"] != snapshot_descriptor(current_lock):
        return False, "binary identity sidecar Cargo.lock snapshot changed"
    if sidecar["source_inputs"]["cargo_config"] != current_config["files"]:
        return False, "binary identity sidecar Cargo config snapshots changed"
    if binary_data.get("path") != str(expected_binary):
        return False, "binary identity sidecar path mismatch"
    try:
        actual_binary = binary_facts(expected_binary)
    except (ContractError, OSError) as error:
        return False, f"current binary facts are invalid: {error}"
    for key in ("path", "sha256", "version"):
        if binary_data.get(key) != actual_binary[key]:
            return False, f"binary identity sidecar binary {key} mismatch"
    if sidecar.get("cargo_profile") != AUTHORITATIVE_PROFILE:
        return False, "binary identity sidecar is not a release build"
    python_data = sidecar.get("python")
    if not isinstance(python_data, dict):
        return False, "binary identity sidecar has no trusted Python facts"
    try:
        actual_python = binary_facts(python_bin)
    except (ContractError, OSError) as error:
        return False, f"current Python facts are invalid: {error}"
    for key in ("path", "sha256", "version"):
        if python_data.get(key) != actual_python[key]:
            return False, f"binary identity sidecar Python {key} mismatch"
    make_data = sidecar.get("make")
    if not isinstance(make_data, dict):
        return False, "binary identity sidecar has no trusted Make facts"
    try:
        actual_make = make_binary_facts(
            absolute_file(make_bin, executable=True, reject_symlinks=True)
        )
    except (ContractError, OSError) as error:
        return False, f"current Make facts are invalid: {error}"
    for key in ("path", "sha256", "version"):
        if make_data.get(key) != actual_make[key]:
            return False, f"binary identity sidecar Make {key} mismatch"
    toolchain = sidecar.get("toolchain")
    if not isinstance(toolchain, dict):
        return False, "binary identity sidecar has no toolchain facts"
    for name in ("cargo", "rustc"):
        facts = toolchain.get(name)
        if not isinstance(facts, dict):
            return False, f"binary identity sidecar has no {name} facts"
        try:
            actual = toolchain_facts(facts["path"])
        except (KeyError, ContractError, OSError, TypeError) as error:
            return False, f"binary identity sidecar {name} facts are invalid: {error}"
        for key in ("path", "resolved_path", "sha256", "version"):
            if facts.get(key) != actual[key]:
                return False, f"binary identity sidecar {name} {key} mismatch"
    expected_command = [
        str(toolchain["cargo"]["path"]),
        "build",
        "--manifest-path",
        str(repo_root / "Cargo.toml"),
        "--release",
        "--locked",
        "--offline",
        "-p",
        "zmin-cli",
        "--bin",
        "zmin",
    ]
    if sidecar.get("build_command") != expected_command:
        return False, "binary identity sidecar build command is not canonical"
    try:
        source_epoch = git_value(git_bin, repo_root, "show", "-s", "--format=%ct", "HEAD")
        expected_policy = canonical_release_build_policy(
            repo_root=repo_root,
            git_bin=trusted_git_binary(git_bin),
            cargo_bin=pathlib.Path(toolchain["cargo"]["path"]),
            rustc_bin=pathlib.Path(toolchain["rustc"]["path"]),
            source_epoch=source_epoch,
        )
    except (ContractError, KeyError, OSError, TypeError) as error:
        return False, f"current release build policy is invalid: {error}"
    expected_manifest = build_environment_facts(expected_policy)
    if build_manifest != expected_manifest:
        return False, "binary identity sidecar canonical build manifest mismatch"
    if build_manifest.get("repo_root") != str(repo_root):
        return False, "binary identity sidecar release policy repo mismatch"
    if build_manifest.get("source_epoch") != source_epoch:
        return False, "binary identity sidecar release policy source epoch mismatch"
    cargo_config = sidecar.get("cargo_config")
    if not isinstance(cargo_config, dict):
        return False, "binary identity sidecar has no Cargo config facts"
    try:
        expected_config = cargo_config_facts(
            repo_root,
            pathlib.Path(expected_policy["values"]["CARGO_HOME"]),
        )
    except (KeyError, ContractError, OSError, TypeError) as error:
        return False, f"current Cargo config facts are invalid: {error}"
    if cargo_config != expected_config:
        return False, "binary identity sidecar Cargo config facts changed"
    return True, "matched"


def policy_reasons(metadata: dict[str, Any]) -> list[str]:
    reasons: list[str] = []
    if metadata.get("mode") != "authoritative":
        reasons.append("run mode is not authoritative")
    if metadata.get("build_profile") != AUTHORITATIVE_PROFILE:
        reasons.append("build profile is not release")
    if metadata.get("mode") == "authoritative" and metadata.get("binary_path_profile") != AUTHORITATIVE_PROFILE:
        reasons.append("binary path profile is not canonical release")
    if metadata.get("code_dirty"):
        reasons.append("working tree is dirty")
    if not metadata.get("identity_complete"):
        reasons.append("binary identity is incomplete or stale")
    if metadata.get("mode") == "authoritative":
        comparator = metadata.get("git_comparator")
        if not isinstance(comparator, dict):
            reasons.append("authoritative Git comparator metadata is missing")
        else:
            if comparator.get("status") != "validated":
                reasons.append("authoritative Git comparator was not provenance-validated")
            if not isinstance(comparator.get("bundle"), str) or not comparator["bundle"].startswith("/"):
                reasons.append("authoritative Git comparator bundle path is missing or non-absolute")
            for field, expected in AUTHORITATIVE_GIT_COMPARATOR.items():
                if comparator.get(field) != expected:
                    reasons.append(f"authoritative Git comparator {field} does not match the pinned contract")
        if metadata.get("build_marker") != RELEASE_BUILD_MARKER:
            reasons.append("release build marker is missing or unknown")
        zmin_facts = metadata.get("zmin")
        sidecar_path = metadata.get("identity_sidecar")
        zmin_path = zmin_facts.get("path") if isinstance(zmin_facts, dict) else None
        if isinstance(zmin_path, str) and isinstance(sidecar_path, str):
            if sidecar_path != str(identity_sidecar(pathlib.Path(zmin_path))):
                reasons.append("binary identity sidecar path is not canonical")
        results_dir = metadata.get("results_dir")
        results_identity = metadata.get("results_dir_identity")
        durability = metadata.get("durability")
        if not isinstance(results_dir, str) or not results_dir.startswith("/"):
            reasons.append("authoritative results directory is not retained explicitly")
        if not isinstance(results_identity, dict):
            reasons.append("authoritative results directory identity is missing")
        if not isinstance(durability, dict) or not durability.get("supported"):
            reasons.append("atomic evidence durability is unsupported")
    python_facts = metadata.get("python")
    if not isinstance(python_facts, dict) or not all(
        isinstance(python_facts.get(key), str) and python_facts.get(key)
        for key in ("path", "sha256", "version")
    ):
        reasons.append("benchmark Python identity is incomplete")
    policy = metadata.get("policy")
    if not isinstance(policy, dict):
        reasons.append("benchmark policy is missing")
        policy = {}
    if policy.get("ordering") != AUTHORITATIVE_ORDERING:
        reasons.append("ordering is not interleaved-paired")
    if policy.get("warmups") != AUTHORITATIVE_WARMUPS:
        reasons.append("warmup count is not 3")
    if policy.get("measured_pairs") != AUTHORITATIVE_MEASURED_PAIRS:
        reasons.append("measured pair count is not 30")
    if policy.get("cold_starts") != AUTHORITATIVE_COLD_STARTS:
        reasons.append("cold-start count is not 10")
    if metadata.get("mode") == "authoritative":
        reasons.extend(_statistics_policy_reasons(metadata))
    if metadata.get("sample_phase_policy") != SAMPLE_PHASE_POLICY:
        reasons.append("sample phase policy is missing or inaccurate")
    if not metadata.get("command_corpus_sha256"):
        reasons.append("command corpus is missing")
    if not metadata.get("fixture_sha256"):
        reasons.append("fixture fingerprint is missing")
    if metadata.get("environment_policy") != ENVIRONMENT_POLICY:
        reasons.append("environment is not sanitized by the benchmark harness")
    manifest_name = metadata.get("mandatory_manifest")
    manifest_lanes = normalized_lane_list(metadata.get("mandatory_lanes"))
    if metadata.get("mode") == "authoritative":
        expected_lanes = MANDATORY_LANE_MANIFESTS.get(manifest_name)
        if expected_lanes is None:
            reasons.append("authoritative scope requires a canonical mandatory manifest")
        elif manifest_lanes != list(expected_lanes):
            reasons.append("authoritative mandatory lane manifest is incomplete or reordered")
        if metadata.get("mandatory_manifest_sha256") != mandatory_manifest_digest(
            str(manifest_name), manifest_lanes
        ):
            reasons.append("mandatory lane manifest digest is invalid")
        if manifest_name in {"standard", "observed"}:
            if metadata.get("equivalence_manifest") != "equivalence.tsv":
                reasons.append(
                    f"{manifest_name} authoritative scope requires equivalence.tsv"
                )
            if metadata.get("equivalence_plan_sha256") != equivalence_plan_digest(
                str(manifest_name), manifest_lanes, metadata.get("policy", {})
            ):
                reasons.append("equivalence manifest plan digest is invalid")
        if manifest_name in MANDATORY_LANE_MANIFESTS:
            if metadata.get("superiority_manifest") != "superiority.tsv":
                reasons.append("authoritative scope requires superiority.tsv")
            reasons.extend(_optional_superiority_thresholds(metadata, []))
    environment = metadata.get("environment")
    environment_values = environment.get("values") if isinstance(environment, dict) else None
    if not isinstance(environment_values, dict) or not environment_values.get("ZMIN_BENCH_REJECTED_ENV"):
        reasons.append("sanitized environment rejection record is missing")
    else:
        required_environment = (
            "PATH",
            "HOME",
            "TMPDIR",
            "LANG",
            "LC_ALL",
            "LC_CTYPE",
            "TZ",
            "GIT_CONFIG_GLOBAL",
            "GIT_CONFIG_NOSYSTEM",
            "PYTHONHASHSEED",
            "ZMIN_BENCH_NETWORK_ENV_POLICY",
        )
        missing_environment = [
            name for name in required_environment if not environment_values.get(name)
        ]
        if missing_environment:
            reasons.append(
                "sanitized environment is missing: " + ",".join(missing_environment)
            )
    template_required = (
        metadata.get("mode") == "authoritative"
        and metadata.get("mandatory_manifest") == "standard"
    )
    template_binding = metadata.get("benchmark_init_template")
    if template_binding is None:
        if template_required:
            reasons.append("standard authoritative init template binding is missing")
    elif not isinstance(template_binding, dict):
        reasons.append("benchmark init template binding is invalid")
    else:
        template_path = template_binding.get("path")
        template_identity = template_binding.get("identity")
        template_environment = (
            environment_values.get("GIT_TEMPLATE_DIR")
            if isinstance(environment_values, dict)
            else None
        )
        if not isinstance(template_path, str) or not template_path.startswith("/"):
            reasons.append("benchmark init template path is invalid")
        if not isinstance(template_identity, dict):
            reasons.append("benchmark init template identity is missing")
        if template_environment != template_path:
            reasons.append("benchmark init template environment is not bound")
        if isinstance(template_path, str) and isinstance(template_identity, dict):
            try:
                current_binding = benchmark_template_binding(
                    pathlib.Path(str(metadata["fixture_root"])),
                    pathlib.Path(template_path),
                    expected_identity=template_identity,
                )
                if current_binding != template_binding:
                    reasons.append("benchmark init template binding changed")
            except (ContractError, KeyError, OSError) as error:
                reasons.append(f"benchmark init template binding is invalid: {error}")
    return reasons


def start_metadata(args: argparse.Namespace) -> dict[str, Any]:
    reject_paths = args.mode == "authoritative"
    repo_root = absolute_directory(args.repo_root, reject_symlinks=reject_paths)
    git_bin = absolute_file(args.git_bin, executable=True, reject_symlinks=reject_paths)
    zmin_bin = absolute_file(args.zmin_bin, executable=True, reject_symlinks=reject_paths)
    python_bin = absolute_file(args.python_bin, executable=True, reject_symlinks=reject_paths)
    fixture_root = absolute_directory(args.fixture_root, reject_symlinks=reject_paths)
    requested_results_dir = getattr(args, "results_dir", "")
    results_dir = (
        absolute_directory(
            requested_results_dir,
            reject_symlinks=reject_paths or bool(requested_results_dir),
        )
        if requested_results_dir
        else None
    )
    state = repo_state(repo_root, git_bin)
    lock = repo_root / "Cargo.lock"
    if not lock.is_file():
        raise ContractError(f"missing Cargo.lock: {lock}")
    lock = absolute_file(lock, reject_symlinks=reject_paths)
    profile = args.build_profile or profile_from_path(zmin_bin)
    sidecar_path = (
        absolute_file(args.identity_sidecar, reject_symlinks=reject_paths)
        if args.identity_sidecar and pathlib.Path(args.identity_sidecar).expanduser().exists()
        else absolute_output_path(
            args.identity_sidecar or identity_sidecar(zmin_bin),
            reject_symlinks=reject_paths,
        )
    )
    sidecar = load_json(sidecar_path) if sidecar_path.is_file() else None
    make_bin = absolute_file(
        args.make_bin,
        executable=True,
        reject_symlinks=reject_paths,
    )
    sidecar_ok, sidecar_detail = sidecar_matches(
        sidecar,
        repo_root=repo_root,
        git_bin=git_bin,
        binary=zmin_bin,
        profile=profile,
        state=state,
        cargo_lock_sha256=sha256_file(lock),
        python_bin=python_bin,
        make_bin=make_bin,
    )
    binary_profile = profile_from_path(zmin_bin)
    if binary_profile not in {"unknown", profile}:
        sidecar_ok = False
        sidecar_detail = f"binary path profile {binary_profile} does not match {profile}"
    command_corpus = args.command_corpus
    harnesses = [absolute_file(path, reject_symlinks=reject_paths) for path in args.harness]
    mandatory_manifest = getattr(args, "mandatory_manifest", "pilot") or "pilot"
    requested_lanes = normalized_lane_list(getattr(args, "mandatory_lanes", ""))
    mandatory_lanes = list(
        MANDATORY_LANE_MANIFESTS.get(mandatory_manifest, tuple(requested_lanes))
    )
    environment = relevant_environment()
    template_value = environment["values"].get("GIT_TEMPLATE_DIR")
    template_binding = None
    if template_value:
        template_binding, bound_template_directories = benchmark_template_binding_and_identity_map(
            fixture_root,
            pathlib.Path(template_value),
        )
        if template_value != template_binding["path"]:
            raise ContractError("GIT_TEMPLATE_DIR is not a canonical fixture path")
    elif args.mode == "authoritative" and mandatory_manifest == "standard":
        raise ContractError(
            "standard authoritative benchmark requires GIT_TEMPLATE_DIR"
        )
    else:
        bound_template_directories = None
    policy = {
        "warmups": int(args.warmups),
        "measured_pairs": int(args.measured_pairs),
        "cold_starts": int(args.cold_starts),
        "ordering": args.ordering,
        "random_seed": int(args.seed),
    }
    equivalence_manifest = getattr(args, "equivalence_manifest", "") or None
    superiority_manifest = (
        "superiority.tsv"
        if args.mode == "authoritative" and mandatory_manifest in MANDATORY_LANE_MANIFESTS
        else None
    )
    metadata: dict[str, Any] = {
        "schema_version": SCHEMA_VERSION,
        "evidence_scope": "benchmark",
        "mode": args.mode,
        "claim_status": "non-authoritative",
        "statistics_verdict": "non-authoritative",
        "repo_root": str(repo_root),
        "git": binary_facts(git_bin),
        "git_comparator": {
            "status": environment["values"].get("ZMIN_BENCH_GIT_COMPARATOR_STATUS", "not-authoritative"),
            "bundle": environment["values"].get("ZMIN_BENCH_GIT_COMPARATOR_BUNDLE"),
            **AUTHORITATIVE_GIT_COMPARATOR,
        },
        "zmin": binary_facts(zmin_bin),
        "python": binary_facts(python_bin),
        "make": make_binary_facts(make_bin),
        "code_commit": state["commit"],
        "code_dirty": state["dirty"],
        "code_status_sha256": state["status_sha256"],
        "build_profile": profile,
        "binary_path_profile": binary_profile,
        "cargo_lock_sha256": sha256_file(lock),
        "identity_sidecar": str(sidecar_path),
        "identity_sidecar_sha256": sha256_file(sidecar_path) if sidecar_path.is_file() else None,
        "identity_complete": sidecar_ok,
        "identity_detail": sidecar_detail,
        "build_marker": sidecar.get("marker") if isinstance(sidecar, dict) else None,
        "toolchain": sidecar.get("toolchain") if isinstance(sidecar, dict) else None,
        "build_manifest": sidecar.get("build_manifest") if isinstance(sidecar, dict) else None,
        "fixture_root": str(fixture_root),
        "benchmark_init_template": template_binding,
        "results_dir": str(results_dir) if results_dir is not None else None,
        "results_dir_identity": path_identity(results_dir) if results_dir is not None else None,
        "durability": durability_capability(results_dir) if results_dir is not None else None,
        "fixture_sha256": fixture_fingerprint(
            fixture_root,
            git_bin,
            bound_directory_identities=bound_template_directories,
        ),
        "git_config_sha256": config_fingerprint(git_bin, fixture_root),
        "host": host_facts(fixture_root),
        "environment": environment,
        "environment_policy": ENVIRONMENT_POLICY,
        "command_corpus": command_corpus,
        "command_corpus_sha256": sha256_bytes(command_corpus.encode()),
        "mandatory_manifest": mandatory_manifest,
        "mandatory_lanes": mandatory_lanes,
        "mandatory_manifest_sha256": mandatory_manifest_digest(
            mandatory_manifest, mandatory_lanes
        ),
        "equivalence_manifest": equivalence_manifest,
        "superiority_manifest": superiority_manifest,
        "equivalence_plan_sha256": equivalence_plan_digest(
            mandatory_manifest, mandatory_lanes, policy
        ),
        "sample_phase_policy": SAMPLE_PHASE_POLICY,
        "harnesses": [{"path": str(path), "sha256": sha256_file(path)} for path in harnesses],
        "policy": policy,
        "statistics_policy": statistics_policy_payload(),
        "metrics": {
            "required": list(REQUIRED_METRICS) + list(MEMORY_METRICS),
            "optional": list(OPTIONAL_METRICS),
            "memory_contracts": MEMORY_CONTRACTS,
            "unsupported_value": "unsupported",
        },
    }
    reasons = policy_reasons(metadata)
    metadata["authoritative_reasons"] = reasons
    metadata["claim_status"] = "authoritative-eligible" if not reasons else "non-authoritative"
    return metadata


def result_files(
    results_dir: pathlib.Path | None,
    results: Iterable[str],
    *,
    reject_symlinks: bool = False,
) -> list[pathlib.Path]:
    control_names = {"metadata.json", "evidence.json"}
    paths = [
        absolute_file(item, reject_symlinks=reject_symlinks)
        for item in results
    ]
    if results_dir is not None:
        root = absolute_directory(results_dir, reject_symlinks=reject_symlinks)
        for path in sorted(root.rglob("*")):
            if reject_symlinks and path.is_symlink():
                raise ContractError(f"symlink result path is not allowed: {path}")
            if not path.is_file():
                continue
            if path.name in control_names:
                if path.parent != root:
                    raise ContractError(f"nested control artifact is not allowed: {path}")
                continue
            if path not in paths:
                paths.append(path)
        for path in paths:
            if path.name not in control_names:
                continue
            try:
                relative = path.relative_to(root)
            except ValueError:
                continue
            if relative.parent != pathlib.Path("."):
                raise ContractError(f"nested control artifact is not allowed: {path}")
    return paths


def finish_anchor_reasons(metadata: dict[str, Any], args: argparse.Namespace) -> list[str]:
    reasons: list[str] = []
    try:
        reject_paths = args.require_authoritative
        anchor_repo_root = absolute_directory(args.anchor_repo_root, reject_symlinks=reject_paths)
        anchor_fixture_root = absolute_directory(args.anchor_fixture_root, reject_symlinks=reject_paths)
        anchor_git_bin = absolute_file(args.anchor_git_bin, executable=True, reject_symlinks=reject_paths)
        anchor_zmin_bin = absolute_file(args.anchor_zmin_bin, executable=True, reject_symlinks=reject_paths)
        anchor_python_bin = absolute_file(args.anchor_python_bin, executable=True, reject_symlinks=reject_paths)
        anchor_make_bin = absolute_file(
            args.anchor_make_bin,
            executable=True,
            reject_symlinks=reject_paths,
        )
        anchor_sidecar = absolute_file(
            args.anchor_identity_sidecar,
            reject_symlinks=reject_paths,
        ) if pathlib.Path(args.anchor_identity_sidecar).expanduser().exists() else absolute_output_path(
            args.anchor_identity_sidecar,
            reject_symlinks=reject_paths,
        )
    except (AttributeError, ContractError, OSError) as error:
        return [f"finish anchors are invalid: {error}"]

    expected_paths = {
        "repo_root": str(anchor_repo_root),
        "fixture_root": str(anchor_fixture_root),
        "identity_sidecar": str(anchor_sidecar),
    }
    for key, expected in expected_paths.items():
        if metadata.get(key) != expected:
            reasons.append(f"finish anchor mismatch for {key}")

    try:
        start_metadata = pathlib.Path(args.metadata).expanduser().resolve(strict=True)
        if sha256_file(start_metadata) != args.anchor_start_metadata_sha256:
            reasons.append("start metadata changed from finish anchor")
    except (AttributeError, OSError) as error:
        reasons.append(f"start metadata anchor is invalid: {error}")

    expected_corpus = args.anchor_command_corpus
    if metadata.get("command_corpus") != expected_corpus:
        reasons.append("finish anchor mismatch for command corpus")
    if metadata.get("command_corpus_sha256") != sha256_bytes(expected_corpus.encode()):
        reasons.append("finish anchor mismatch for command corpus hash")
    if metadata.get("fixture_sha256") != args.anchor_fixture_sha256:
        reasons.append("finish anchor mismatch for fixture fingerprint")

    for label, binary_path, expected_sha, expected_version in (
        ("git", anchor_git_bin, args.anchor_git_sha256, args.anchor_git_version),
        ("zmin", anchor_zmin_bin, args.anchor_zmin_sha256, args.anchor_zmin_version),
        ("python", anchor_python_bin, args.anchor_python_sha256, args.anchor_python_version),
    ):
        facts = metadata.get(label)
        if not isinstance(facts, dict):
            reasons.append(f"finish anchor missing {label} binary facts")
            continue
        if facts.get("path") != str(binary_path):
            reasons.append(f"finish anchor mismatch for {label} binary path")
        if facts.get("sha256") != expected_sha:
            reasons.append(f"finish anchor mismatch for {label} binary hash")
        if facts.get("version") != expected_version:
            reasons.append(f"finish anchor mismatch for {label} binary version")
    make_facts = metadata.get("make")
    if not isinstance(make_facts, dict):
        reasons.append("finish anchor missing make binary facts")
    else:
        if make_facts.get("path") != str(anchor_make_bin):
            reasons.append("finish anchor mismatch for make binary path")
        if make_facts.get("sha256") != args.anchor_make_sha256:
            reasons.append("finish anchor mismatch for make binary hash")
        if make_facts.get("version") != args.anchor_make_version:
            reasons.append("finish anchor mismatch for make binary version")

    try:
        actual_sidecar_hash = sha256_file(anchor_sidecar) if anchor_sidecar.is_file() else "missing"
    except OSError as error:
        actual_sidecar_hash = f"error:{error}"
    recorded_sidecar_hash = metadata.get("identity_sidecar_sha256")
    if (recorded_sidecar_hash or "missing") != args.anchor_identity_sidecar_sha256:
        reasons.append("finish anchor mismatch for identity sidecar")
    if actual_sidecar_hash != args.anchor_identity_sidecar_sha256:
        reasons.append("identity sidecar changed from finish anchor")

    anchor_harnesses = getattr(args, "anchor_harness", [])
    anchor_harness_hashes = getattr(args, "anchor_harness_sha256", [])
    if len(anchor_harnesses) != len(anchor_harness_hashes) or not anchor_harnesses:
        reasons.append("finish harness anchors are incomplete")
    else:
        try:
            expected_harnesses = [
                {
                    "path": str(absolute_file(path, reject_symlinks=args.require_authoritative)),
                    "sha256": expected_hash,
                }
                for path, expected_hash in zip(anchor_harnesses, anchor_harness_hashes)
            ]
        except (ContractError, OSError) as error:
            reasons.append(f"finish harness anchors are invalid: {error}")
        else:
            if metadata.get("harnesses") != expected_harnesses:
                reasons.append("finish anchor mismatch for harness list")
    return reasons


def read_rows(path: pathlib.Path) -> list[dict[str, str]]:
    return read_rows_bytes(path.read_bytes(), path)


def paired_tool_names(lane_items: list[dict[str, str]]) -> set[str]:
    observed = {row.get("tool", "") for row in lane_items}
    if "stock" in observed and "git" not in observed:
        return {"stock", "zmin"}
    return {"git", "zmin"}


def validate_row_metrics(
    row: dict[str, str],
    label: str,
    reasons: list[str],
    expected_memory: Mapping[str, str] | None,
) -> None:
    _validate_memory_identity(row, expected_memory, label, reasons)
    availability_raw = row.get("metrics_availability")
    availability: dict[str, str] = {}
    if availability_raw is None:
        reasons.append(f"{label} is missing metrics_availability")
    else:
        for item in availability_raw.split(";"):
            name, separator, state = item.partition("=")
            if not separator or name in availability or state not in {"available", "unsupported"}:
                reasons.append(f"{label} has invalid metric availability: {item}")
                continue
            availability[name] = state
        unknown = set(availability) - set(METRIC_FIELD_ALIASES)
        for name in sorted(unknown):
            reasons.append(f"{label} has unknown metric availability: {name}")

    for metric, aliases in METRIC_FIELD_ALIASES.items():
        field = next((name for name in aliases if name in row), None)
        if field is None:
            reasons.append(f"{label} is missing metric field: {metric}")
            continue
        value = row.get(field, "")
        state = availability.get(metric)
        if state not in {"available", "unsupported"}:
            reasons.append(f"{label} has no availability for metric: {metric}")
            continue
        required_memory = expected_memory is not None and metric == expected_memory["metric"]
        if (metric in REQUIRED_METRICS or required_memory) and state != "available":
            reasons.append(f"required metric unavailable: {metric}")
        if metric in MEMORY_METRICS and expected_memory is not None and not required_memory:
            if state != "unsupported" or value != "unsupported":
                reasons.append(
                    f"{label} non-selected memory metric must be unsupported: {metric}"
                )
        if state == "unsupported":
            if value != "unsupported":
                reasons.append(f"{label} encodes unsupported {metric} as {value}")
            continue
        if value == "unsupported":
            reasons.append(f"{label} marks available {metric} as unsupported")
            continue
        if not parse_nonnegative_number(value, integer=metric in INTEGER_METRICS):
            reasons.append(f"{label} has invalid nonnegative {metric}: {value}")

    if "exit" in row:
        exit_value = row.get("exit", "")
        if not re.fullmatch(r"[0-9]+", exit_value):
            reasons.append(f"{label} has invalid exit status: {exit_value}")
        elif int(exit_value) != 0:
            reasons.append(f"{label} has non-zero command exit: {exit_value}")


def expected_pair_ids(lane: str, sample_kind: str, count: int) -> set[str]:
    return {f"{lane}-{sample_kind}-{index}" for index in range(1, count + 1)}


def read_equivalence_bytes(data: bytes, path: pathlib.Path) -> list[dict[str, str]]:
    return read_strict_tsv_bytes(
        data,
        path,
        expected_fields=EQUIVALENCE_MANIFEST_FIELDS,
    )


def canonical_pair_sequence(
    lanes: list[str],
    counts: dict[str, int],
) -> list[tuple[str, str, str]]:
    return [
        (lane, sample_kind, f"{lane}-{sample_kind}-{index}")
        for sample_kind in ("warmup", "measured", "cold")
        for index in range(1, counts.get(sample_kind, 0) + 1)
        for lane in lanes
    ]


def canonical_result_row_sequence(
    rows: list[dict[str, str]],
    lanes: list[str],
    counts: dict[str, int],
    equivalence_rows: list[dict[str, str]] | None = None,
) -> list[tuple[str, str, str, str, str]]:
    row_groups: dict[tuple[str, str, str], list[dict[str, str]]] = {}
    for row in rows:
        key = (
            row.get("lane") or row.get("op") or "",
            row.get("sample_kind", ""),
            row.get("pair_id", ""),
        )
        row_groups.setdefault(key, []).append(row)
    equivalence_orders = {
        (
            row.get("lane", ""),
            row.get("sample_kind", ""),
            row.get("pair_id", ""),
        ): {
            "git": row.get("git_order_index", ""),
            "zmin": row.get("zmin_order_index", ""),
        }
        for row in equivalence_rows or []
    }
    expected: list[tuple[str, str, str, str, str]] = []
    for key in canonical_pair_sequence(lanes, counts):
        pair_rows = row_groups.get(key, [])
        declared = equivalence_orders.get(key)
        if declared is None:
            declared = {
                row.get("tool", ""): row.get("order_index", "")
                for row in pair_rows
            }
        ordered_tools = sorted(
            ((order, tool) for tool, order in declared.items()),
            key=lambda item: item[0],
        )
        if [order for order, _tool in ordered_tools] != ["1", "2"]:
            ordered_tools = [("1", "git"), ("2", "zmin")]
        expected.extend(
            (key[0], key[1], key[2], tool, order)
            for order, tool in ordered_tools
        )
    return expected


def validate_equivalence(
    rows: list[dict[str, str]], metadata: dict[str, Any]
) -> list[str]:
    reasons: list[str] = []
    manifest_name = metadata.get("mandatory_manifest")
    lanes = normalized_lane_list(metadata.get("mandatory_lanes"))
    if manifest_name == "standard" and lanes != list(STANDARD_MANDATORY_LANES):
        reasons.append("standard equivalence manifest lane scope is not canonical")
    if not lanes:
        reasons.append("equivalence manifest has no mandatory lanes")
        return reasons
    actual_lane_order: list[str] = []
    seen_lanes: set[str] = set()
    pair_ids: set[str] = set()
    policy = metadata.get("policy")
    if not isinstance(policy, dict):
        return ["equivalence manifest has no benchmark policy"]
    counts: dict[str, int] = {}
    for sample_kind, policy_key in (
        ("warmup", "warmups"),
        ("measured", "measured_pairs"),
        ("cold", "cold_starts"),
    ):
        value = policy.get(policy_key)
        if isinstance(value, bool) or not isinstance(value, int) or value < 0:
            reasons.append(f"equivalence policy has invalid {policy_key}: {value}")
        else:
            counts[sample_kind] = value
    for index, row in enumerate(rows, start=1):
        label = f"equivalence row {index}"
        lane = row.get("lane", "")
        if lane not in lanes:
            reasons.append(f"{label} has an unexpected lane: {lane}")
        elif lane not in seen_lanes:
            seen_lanes.add(lane)
            actual_lane_order.append(lane)
        sample_kind = row.get("sample_kind", "")
        count = counts.get(sample_kind)
        pair_id = row.get("pair_id", "")
        if count is None:
            reasons.append(f"{label} has an invalid sample kind: {sample_kind}")
        expected_phase = SAMPLE_PHASE_POLICY.get(sample_kind, {}).get("phase")
        if row.get("phase", "") != expected_phase:
            reasons.append(f"{label} has an invalid phase for {sample_kind}")
        if pair_id in pair_ids:
            reasons.append(f"{label} duplicates pair ID: {pair_id}")
        pair_ids.add(pair_id)
        expected_index = None
        match = re.fullmatch(r".+-(?:warmup|measured|cold)-([0-9]+)", pair_id)
        if match is not None:
            expected_index = int(match.group(1))
        pair_index = row.get("pair_index", "")
        if not re.fullmatch(r"[0-9]+", pair_index):
            reasons.append(f"{label} has invalid pair index: {pair_index}")
        elif expected_index is None or int(pair_index) != expected_index:
            reasons.append(f"{label} pair index does not match pair ID")
        if count is not None and expected_index is not None and not 1 <= expected_index <= count:
            reasons.append(f"{label} pair index is outside its sample count")
        orders = []
        for field in ("git_order_index", "zmin_order_index"):
            value = row.get(field, "")
            if not re.fullmatch(r"[12]", value):
                reasons.append(f"{label} has invalid {field}: {value}")
            orders.append(value)
        if orders == ["1", "1"] or orders == ["2", "2"]:
            reasons.append(f"{label} tools do not have distinct pair order")
        for field in ("git_exit", "zmin_exit"):
            if not re.fullmatch(r"[0-9]+", row.get(field, "")):
                reasons.append(f"{label} has invalid {field}")
        for field in (
            "git_stdout_sha256",
            "zmin_stdout_sha256",
            "git_stderr_sha256",
            "zmin_stderr_sha256",
        ):
            if not re.fullmatch(r"[0-9a-f]{64}", row.get(field, "")):
                reasons.append(f"{label} has invalid {field}")
        for field in ("exit_equal", "stdout_equal", "stderr_equal"):
            expected_equal = {
                "exit_equal": row.get("git_exit") == row.get("zmin_exit"),
                "stdout_equal": row.get("git_stdout_sha256") == row.get("zmin_stdout_sha256"),
                "stderr_equal": row.get("git_stderr_sha256") == row.get("zmin_stderr_sha256"),
            }[field]
            recorded_equal = row.get(field) == "true"
            if recorded_equal != expected_equal:
                reasons.append(f"{label} has inconsistent {field}")
            if not recorded_equal:
                reasons.append(f"{label} does not prove {field}")
    expected_sequence = canonical_pair_sequence(lanes, counts)
    actual_sequence = [
        (row.get("lane", ""), row.get("sample_kind", ""), row.get("pair_id", ""))
        for row in rows
    ]
    if actual_sequence != expected_sequence:
        reasons.append("equivalence manifest rows are not in canonical order")
    if actual_lane_order != lanes:
        reasons.append("equivalence manifest lane order is missing, extra, or reordered")
    expected_ids = {
        f"{lane}-{sample_kind}-{index}"
        for lane in lanes
        for sample_kind, count in counts.items()
        for index in range(1, count + 1)
    }
    missing_ids = sorted(expected_ids - pair_ids)
    extra_ids = sorted(pair_ids - expected_ids)
    if missing_ids:
        reasons.append("equivalence manifest missing pair IDs: " + ",".join(missing_ids))
    if extra_ids:
        reasons.append("equivalence manifest has unexpected pair IDs: " + ",".join(extra_ids))
    if len(rows) != len(expected_ids):
        reasons.append("equivalence manifest has an unexpected row count")
    return reasons


def validate_rows(
    rows: list[dict[str, str]],
    metadata: dict[str, Any],
    equivalence_rows: list[dict[str, str]] | None = None,
) -> list[str]:
    reasons: list[str] = []
    required_columns = {
        "tool",
        "sample_kind",
        "pair_id",
        "order_index",
        "memory_metric",
        "memory_semantics",
        "memory_scope",
        "memory_unit",
        "metrics_availability",
    }
    if rows:
        missing = sorted(required_columns - set(rows[0]))
        if missing:
            reasons.append(f"result rows missing columns: {','.join(missing)}")
    else:
        reasons.append("result rows are empty")
        return reasons
    if metadata.get("mode") == "authoritative":
        manifest_name = metadata.get("mandatory_manifest")
        expected_lanes = MANDATORY_LANE_MANIFESTS.get(manifest_name)
        manifest_lanes = normalized_lane_list(metadata.get("mandatory_lanes"))
        if expected_lanes is None or manifest_lanes != list(expected_lanes):
            reasons.append("authoritative result lane manifest is not canonical")
        actual_lane_order = list(dict.fromkeys(
            row.get("lane") or row.get("op") or "" for row in rows
        ))
        if actual_lane_order != manifest_lanes:
            reasons.append("authoritative result lanes are missing, extra, or reordered")
    expected_memory = memory_contract_for_metadata(metadata)
    if expected_memory is None:
        reasons.append("result rows have no supported host memory metric contract")
    policy = metadata.get("policy")
    if not isinstance(policy, dict):
        reasons.append("benchmark policy is missing")
        return reasons
    lane_rows: dict[str, list[dict[str, str]]] = {}
    for row in rows:
        lane = row.get("lane") or row.get("op") or ""
        lane_rows.setdefault(lane, []).append(row)
    for lane, lane_items in lane_rows.items():
        expected_tools = paired_tool_names(lane_items)
        observed_tools = {row.get("tool", "") for row in lane_items}
        unexpected_tools = observed_tools - expected_tools
        if metadata.get("mode") == "authoritative":
            for tool in sorted(unexpected_tools):
                reasons.append(f"lane {lane} has unexpected authoritative tool: {tool}")
        for row_index, row in enumerate(lane_items, start=1):
            label = f"lane {lane} row {row_index}"
            if (
                metadata.get("mode") == "authoritative"
                and metadata.get("mandatory_manifest") in {"standard", "observed"}
                and "exit" not in row
            ):
                reasons.append(f"{label} is missing exit status")
            validate_row_metrics(row, label, reasons, expected_memory)
            kind = row.get("sample_kind", "")
            if kind not in {"warmup", "measured", "cold"}:
                reasons.append(f"{label} has invalid sample kind: {kind}")
            order = row.get("order_index", "")
            if not re.fullmatch(r"[0-9]+", order) or int(order) < 1:
                reasons.append(f"{label} has invalid order index: {order}")
            if not row.get("pair_id"):
                reasons.append(f"{label} has an empty pair ID")

        expected_counts: dict[str, int] = {}
        for sample_kind, policy_key in (
            ("warmup", "warmups"),
            ("measured", "measured_pairs"),
            ("cold", "cold_starts"),
        ):
            value = policy.get(policy_key)
            if isinstance(value, bool) or not isinstance(value, int) or value < 0:
                reasons.append(f"benchmark policy has invalid {policy_key}: {value}")
                continue
            expected_counts[sample_kind] = value
        for sample_kind, expected_count in expected_counts.items():
            expected_ids = expected_pair_ids(lane, sample_kind, expected_count)
            relevant = [
                row
                for row in lane_items
                if row.get("sample_kind") == sample_kind
                and row.get("tool") in expected_tools
            ]
            actual_ids = {row.get("pair_id", "") for row in relevant}
            missing_ids = sorted(expected_ids - actual_ids)
            extra_ids = sorted(actual_ids - expected_ids)
            if missing_ids:
                reasons.append(
                    f"lane {lane} {sample_kind} missing pair IDs: {','.join(missing_ids)}"
                )
            if extra_ids:
                reasons.append(
                    f"lane {lane} {sample_kind} has unexpected pair IDs: {','.join(extra_ids)}"
                )
            for pair_id in sorted(expected_ids):
                pair_rows = [row for row in relevant if row.get("pair_id") == pair_id]
                tool_counts = {
                    tool: sum(1 for row in pair_rows if row.get("tool") == tool)
                    for tool in expected_tools
                }
                if len(pair_rows) != 2 or any(count != 1 for count in tool_counts.values()):
                    reasons.append(
                        f"lane {lane} {sample_kind} pair {pair_id} must have exactly one row per tool"
                    )
                order_values = [int(row["order_index"]) for row in pair_rows if row.get("order_index", "").isdigit()]
                expected_order = [1, 2]
                if unexpected_tools:
                    expected_order = sorted(set(order_values))
                if sorted(order_values) != expected_order or len(order_values) != len(set(order_values)):
                    reasons.append(
                        f"lane {lane} {sample_kind} pair {pair_id} order must be 1,2"
                    )
    if metadata.get("mode") == "authoritative" and metadata.get("mandatory_manifest") == "standard":
        counts = {}
        for sample_kind, policy_key in (
            ("warmup", "warmups"),
            ("measured", "measured_pairs"),
            ("cold", "cold_starts"),
        ):
            value = policy.get(policy_key)
            if isinstance(value, int) and not isinstance(value, bool) and value >= 0:
                counts[sample_kind] = value
        expected_sequence = canonical_result_row_sequence(
            rows,
            list(STANDARD_MANDATORY_LANES),
            counts,
            equivalence_rows,
        )
        actual_sequence = [
            (
                row.get("lane") or row.get("op") or "",
                row.get("sample_kind", ""),
                row.get("pair_id", ""),
                row.get("tool", ""),
                row.get("order_index", ""),
            )
            for row in rows
        ]
        if actual_sequence != expected_sequence:
            reasons.append("authoritative result rows are not in exact canonical order")
    return reasons


def validate_equivalence_against_rows(
    equivalence_rows: list[dict[str, str]],
    rows: list[dict[str, str]],
    metadata: dict[str, Any],
) -> list[str]:
    if metadata.get("mode") != "authoritative" or metadata.get("mandatory_manifest") not in {
        "standard",
        "observed",
    }:
        return []
    stock_tool = "git" if metadata.get("mandatory_manifest") == "standard" else "stock"
    indexed: dict[tuple[str, str, str, str], list[dict[str, str]]] = {}
    for row in rows:
        key = (
            row.get("lane") or row.get("op") or "",
            row.get("sample_kind", ""),
            row.get("pair_id", ""),
            row.get("tool", ""),
        )
        indexed.setdefault(key, []).append(row)
    reasons: list[str] = []
    for index, equivalence in enumerate(equivalence_rows, start=1):
        label = f"equivalence row {index}"
        lane = equivalence.get("lane", "")
        sample_kind = equivalence.get("sample_kind", "")
        pair_id = equivalence.get("pair_id", "")
        for tool, exit_field, order_field in (
            (stock_tool, "git_exit", "git_order_index"),
            ("zmin", "zmin_exit", "zmin_order_index"),
        ):
            matching = indexed.get((lane, sample_kind, pair_id, tool), [])
            if len(matching) != 1:
                reasons.append(f"{label} requires exactly one {tool} result row")
                continue
            row = matching[0]
            row_exit = row.get("exit")
            if row_exit is None:
                reasons.append(f"{label} {tool} result row is missing exit status")
            elif row_exit != equivalence.get(exit_field, ""):
                reasons.append(f"{label} {tool} exit differs from result row")
            if row.get("order_index") != equivalence.get(order_field, ""):
                reasons.append(f"{label} {tool} order differs from result row")
    return reasons


def finish_metadata(args: argparse.Namespace) -> dict[str, Any]:
    reject_paths = args.require_authoritative or bool(args.results_dir)
    metadata_path = absolute_file(args.metadata, reject_symlinks=reject_paths)
    results_dir = (
        absolute_directory(args.results_dir, reject_symlinks=reject_paths)
        if args.results_dir
        else None
    )
    if args.require_authoritative and results_dir is None:
        raise ContractError("authoritative evidence requires an explicit retained results directory")
    start_results_identity = path_identity(results_dir) if results_dir is not None else None
    if results_dir is not None:
        if not path_is_within(metadata_path, results_dir):
            raise ContractError("metadata must be inside the retained results directory")
        metadata_snapshot = artifact_read_snapshot(
            results_dir,
            artifact_relative_name(results_dir, metadata_path),
            expected_directory_identity=start_results_identity,
            max_bytes=MAX_JSON_BYTES,
        )
    else:
        metadata_snapshot = read_file_snapshot(
            metadata_path,
            max_bytes=MAX_JSON_BYTES,
            reject_symlinks=reject_paths,
        )
    metadata = load_json_bytes(metadata_snapshot["data"], metadata_path)
    output = absolute_output_path(args.output, reject_symlinks=reject_paths)
    if results_dir is not None:
        if not path_is_within(output, results_dir):
            raise ContractError("evidence output must stay inside retained results directory")
        if metadata.get("results_dir") != str(results_dir):
            raise ContractError("results directory does not match its start anchor")
        recorded_identity = metadata.get("results_dir_identity")
        if recorded_identity != start_results_identity:
            raise ContractError("results directory identity changed before finish")
        expected_results_identity = recorded_identity
    else:
        expected_results_identity = None
    if args.require_authoritative:
        recorded_durability = metadata.get("durability")
        current_durability = durability_capability(results_dir)
        if recorded_durability != current_durability:
            raise ContractError("authoritative results directory durability changed before finish")
        if not current_durability.get("supported"):
            raise ContractError("authoritative evidence durability is unsupported")
    paths = result_files(
        results_dir,
        args.result,
        reject_symlinks=reject_paths,
    )
    if not paths:
        raise ContractError("no raw result files were supplied")
    rows_path = absolute_file(args.rows, reject_symlinks=reject_paths)
    if results_dir is not None:
        if not path_is_within(rows_path, results_dir):
            raise ContractError(
                "rows file must be a direct regular file inside the retained results directory"
            )
        for path in paths:
            if not path_is_within(path, results_dir):
                raise ContractError(
                    "raw result must be a direct regular file inside the retained results directory"
                )
    if args.require_authoritative:
        try:
            rows_path.relative_to(results_dir)
        except ValueError as error:
            raise ContractError(
                "authoritative rows file must be inside the retained results directory"
            ) from error
        if rows_path not in paths:
            raise ContractError(
                "authoritative rows file must be included in retained raw results"
            )
        for path in paths:
            if not path_is_within(path, results_dir):
                raise ContractError(
                    "authoritative raw result must be inside the retained results directory"
                )
        collision_inputs: list[tuple[str, pathlib.Path]] = [
            ("metadata", metadata_path),
            ("rows", rows_path),
        ]
        collision_inputs.extend(("raw result", path) for path in paths)
        anchor_sidecar = getattr(args, "anchor_identity_sidecar", "")
        if anchor_sidecar:
            collision_inputs.append(
                (
                    "identity sidecar",
                    absolute_output_path(
                        anchor_sidecar,
                        reject_symlinks=True,
                    ),
                )
            )
        for label, path in (
            ("git anchor", getattr(args, "anchor_git_bin", "")),
            ("zmin anchor", getattr(args, "anchor_zmin_bin", "")),
            ("Python anchor", getattr(args, "anchor_python_bin", "")),
            ("Make anchor", getattr(args, "anchor_make_bin", "")),
        ):
            if not path:
                continue
            collision_inputs.append(
                (
                    label,
                    absolute_file(path, executable=True, reject_symlinks=True),
                )
            )
        reject_output_collisions(output, collision_inputs)

    equivalence_snapshot: dict[str, Any] | None = None
    equivalence_rows: list[dict[str, str]] = []
    equivalence_reasons: list[str] = []
    equivalence_name = metadata.get("equivalence_manifest")
    equivalence_path: pathlib.Path | None = None
    if equivalence_name:
        if results_dir is None:
            equivalence_reasons.append(
                "equivalence manifest requires a retained results directory"
            )
        else:
            equivalence_candidate = absolute_output_path(
                results_dir / str(equivalence_name),
                reject_symlinks=reject_paths,
            )
            if not path_is_within(equivalence_candidate, results_dir):
                raise ContractError(
                    "equivalence manifest must be a direct file inside the retained results directory"
                )
            try:
                equivalence_path = absolute_file(
                    equivalence_candidate,
                    reject_symlinks=reject_paths,
                )
                if equivalence_path not in paths:
                    paths.append(equivalence_path)
            except (ContractError, OSError) as error:
                equivalence_reasons.append(f"equivalence manifest could not be read: {error}")
    elif args.require_authoritative and metadata.get("mandatory_manifest") == "standard":
        equivalence_reasons.append("standard authoritative scope requires equivalence.tsv")

    superiority_snapshot: dict[str, Any] | None = None
    superiority_rows: list[dict[str, str]] = []
    superiority_reasons: list[str] = []
    superiority_name = metadata.get("superiority_manifest")
    superiority_path: pathlib.Path | None = None
    if superiority_name:
        if results_dir is None:
            superiority_reasons.append(
                "superiority manifest requires a retained results directory"
            )
        elif superiority_name != "superiority.tsv":
            superiority_reasons.append("superiority manifest name is not canonical")
        else:
            superiority_candidate = absolute_output_path(
                results_dir / superiority_name,
                reject_symlinks=reject_paths,
            )
            if not path_is_within(superiority_candidate, results_dir):
                superiority_reasons.append(
                    "superiority manifest must be inside the retained results directory"
                )
            else:
                try:
                    superiority_path = absolute_file(
                        superiority_candidate,
                        reject_symlinks=reject_paths,
                    )
                except (ContractError, OSError) as error:
                    superiority_reasons.append(
                        f"superiority manifest could not be read: {error}"
                    )
                else:
                    if superiority_path not in paths:
                        if superiority_path.is_file():
                            paths.append(superiority_path)
                        else:
                            superiority_reasons.append(
                                "authoritative superiority manifest is missing"
                            )

    snapshots: dict[pathlib.Path, dict[str, Any]] = {}
    retained_snapshots: list[dict[str, Any]] = (
        [metadata_snapshot] if results_dir is not None else []
    )
    external_snapshots: list[dict[str, Any]] = (
        [] if results_dir is not None else [metadata_snapshot]
    )
    for path in paths:
        if results_dir is not None and path_is_within(path, results_dir):
            snapshot = artifact_read_snapshot(
                results_dir,
                artifact_relative_name(results_dir, path),
                expected_directory_identity=expected_results_identity,
            )
            retained_snapshots.append(snapshot)
        else:
            snapshot = read_file_snapshot(path)
            external_snapshots.append(snapshot)
        snapshots[path] = snapshot
    rows_snapshot = snapshots.get(rows_path)
    if rows_snapshot is None:
        if results_dir is not None and path_is_within(rows_path, results_dir):
            rows_snapshot = artifact_read_snapshot(
                results_dir,
                artifact_relative_name(results_dir, rows_path),
                expected_directory_identity=expected_results_identity,
            )
            retained_snapshots.append(rows_snapshot)
        else:
            rows_snapshot = read_file_snapshot(rows_path)
            external_snapshots.append(rows_snapshot)
    if equivalence_path is not None:
        equivalence_snapshot = snapshots.get(equivalence_path)
        if equivalence_snapshot is None:
            equivalence_reasons.append("equivalence manifest was not retained")
        else:
            equivalence_rows = read_equivalence_bytes(
                equivalence_snapshot["data"], equivalence_path
            )
            equivalence_reasons = validate_equivalence(equivalence_rows, metadata)
            metadata["equivalence_manifest_sha256"] = equivalence_snapshot["sha256"]
    if superiority_path is not None:
        superiority_snapshot = snapshots.get(superiority_path)
        if superiority_snapshot is None:
            superiority_reasons.append("superiority manifest was not retained")
        else:
            try:
                superiority_rows = read_superiority_summary_bytes(
                    superiority_snapshot["data"], superiority_path
                )
            except ContractError as error:
                superiority_reasons.append(str(error))
            metadata["superiority_manifest_sha256"] = superiority_snapshot["sha256"]
    raw = [
        {
            "path": str(path),
            "bytes": len(snapshots[path]["data"]),
            "sha256": snapshots[path]["sha256"],
        }
        for path in paths
    ]
    metadata["raw_results"] = raw
    metadata["raw_results_sha256"] = sha256_bytes(canonical_json(raw))
    expected_result_fields = None
    if metadata.get("mandatory_manifest") == "standard":
        expected_result_fields = STANDARD_RESULT_FIELDS
    elif metadata.get("mandatory_manifest") == "observed":
        expected_result_fields = OBSERVED_RESULT_FIELDS
    rows = read_strict_tsv_bytes(
        rows_snapshot["data"],
        rows_path,
        expected_fields=expected_result_fields,
    )
    row_reasons = validate_rows(rows, metadata, equivalence_rows)
    expected_superiority_rows, recomputed_superiority_reasons = compute_superiority_summary(
        rows, metadata
    )
    metadata["statistics_verdict"] = _statistics_verdict(
        metadata,
        expected_superiority_rows,
        recomputed_superiority_reasons,
    )
    superiority_reasons.extend(recomputed_superiority_reasons)
    if superiority_snapshot is not None:
        superiority_reasons.extend(
            validate_superiority_summary_artifact(
                superiority_snapshot["data"],
                superiority_path or pathlib.Path("superiority.tsv"),
                superiority_rows,
                expected_superiority_rows,
            )
        )
    equivalence_reasons.extend(
        validate_equivalence_against_rows(equivalence_rows, rows, metadata)
    )
    reasons = policy_reasons(metadata)
    reasons.extend(finish_anchor_reasons(metadata, args))
    reasons.extend(equivalence_reasons)
    reasons.extend(superiority_reasons)
    try:
        repo_root = absolute_directory(
            args.anchor_repo_root,
            reject_symlinks=reject_paths,
        )
        current_state = repo_state(repo_root, trusted_git_binary(args.anchor_git_bin))
        if current_state["commit"] != metadata.get("code_commit"):
            reasons.append("source commit changed during benchmark")
        if current_state["dirty"] != metadata.get("code_dirty"):
            reasons.append("working-tree state changed during benchmark")
        if current_state["status_sha256"] != metadata.get("code_status_sha256"):
            reasons.append("working-tree status changed during benchmark")
        lock = absolute_file(
            repo_root / "Cargo.lock",
            reject_symlinks=reject_paths,
        )
        if sha256_file(lock) != metadata.get("cargo_lock_sha256"):
            reasons.append("Cargo.lock changed during benchmark")
        anchor_binaries = {
            "git": (args.anchor_git_bin, args.anchor_git_sha256, args.anchor_git_version),
            "zmin": (args.anchor_zmin_bin, args.anchor_zmin_sha256, args.anchor_zmin_version),
            "python": (args.anchor_python_bin, args.anchor_python_sha256, args.anchor_python_version),
            "make": (args.anchor_make_bin, args.anchor_make_sha256, args.anchor_make_version),
        }
        for key, (path, expected_sha, expected_version) in anchor_binaries.items():
            candidate = absolute_file(path, executable=True, reject_symlinks=reject_paths)
            facts = make_binary_facts(candidate) if key == "make" else binary_facts(candidate)
            if facts["sha256"] != expected_sha or facts["version"] != expected_version:
                reasons.append(f"{key} binary identity changed during benchmark")
        sidecar_path = absolute_file(
            args.anchor_identity_sidecar,
            reject_symlinks=reject_paths,
        )
        if not sidecar_path.is_file():
            reasons.append("binary identity sidecar missing during benchmark")
        elif sha256_file(sidecar_path) != args.anchor_identity_sidecar_sha256:
            reasons.append("binary identity sidecar changed during benchmark")
        else:
            sidecar_ok, sidecar_detail = sidecar_matches(
                load_json(sidecar_path),
                repo_root=repo_root,
                git_bin=trusted_git_binary(args.anchor_git_bin),
                binary=absolute_file(args.anchor_zmin_bin, executable=True),
                profile=args.anchor_build_profile,
                state=current_state,
                cargo_lock_sha256=sha256_file(lock),
                python_bin=absolute_file(
                    args.anchor_python_bin,
                    executable=True,
                    reject_symlinks=reject_paths,
                ),
                make_bin=absolute_file(
                    args.anchor_make_bin,
                    executable=True,
                    reject_symlinks=reject_paths,
                ),
            )
            if not sidecar_ok:
                reasons.append(f"binary identity sidecar recheck failed: {sidecar_detail}")
        fixture_root = absolute_directory(
            args.anchor_fixture_root,
            reject_symlinks=reject_paths,
        )
        bound_template_directories = None
        template_binding = metadata.get("benchmark_init_template")
        if isinstance(template_binding, dict):
            try:
                checked_template, bound_template_directories = benchmark_template_binding_and_identity_map(
                    fixture_root,
                    pathlib.Path(str(template_binding["path"])),
                    expected_identity=template_binding["identity"],
                )
                if checked_template != template_binding:
                    reasons.append("benchmark init template binding changed")
            except (ContractError, KeyError, OSError) as error:
                reasons.append(f"benchmark init template recheck failed: {error}")
        if fixture_fingerprint(
            fixture_root,
            absolute_file(
                args.anchor_git_bin,
                executable=True,
                reject_symlinks=reject_paths,
            ),
            bound_directory_identities=bound_template_directories,
        ) != args.anchor_fixture_sha256:
            reasons.append("fixture changed during benchmark")
        if config_fingerprint(
            absolute_file(
                args.anchor_git_bin,
                executable=True,
                reject_symlinks=reject_paths,
            ),
            fixture_root,
        ) != metadata.get("git_config_sha256"):
            reasons.append("fixture Git config changed during benchmark")
        current_host = host_facts(fixture_root)
        metadata["host_recheck"] = current_host
        recorded_host = metadata.get("host", {})
        for field in HOST_IDENTITY_FIELDS:
            if current_host.get(field) != recorded_host.get(field):
                reasons.append(f"host fact changed during benchmark: {field}")
        current_environment = relevant_environment()
        metadata["environment_recheck"] = current_environment
        if current_environment != metadata.get("environment"):
            reasons.append("relevant environment changed during benchmark")
        command_corpus = args.anchor_command_corpus
        if sha256_bytes(command_corpus.encode()) != metadata.get("command_corpus_sha256"):
            reasons.append("command corpus identity changed during benchmark")
        for harness_path, expected_hash in zip(args.anchor_harness, args.anchor_harness_sha256):
            harness_path = absolute_file(
                harness_path,
                reject_symlinks=reject_paths,
            )
            if sha256_file(harness_path) != expected_hash:
                reasons.append(f"harness changed during benchmark: {harness_path}")
    except (AttributeError, ContractError, KeyError, TypeError) as error:
        reasons.append(f"final identity recheck failed: {error}")
    reasons.extend(row_reasons)
    if results_dir is not None:
        try:
            current_paths = result_files(
                results_dir,
                args.result,
                reject_symlinks=reject_paths,
            )
        except (ContractError, OSError) as error:
            reasons.append(f"retained result set could not be rechecked: {error}")
        else:
            if set(current_paths) != set(paths):
                reasons.append("retained result set changed during finish")
    if results_dir is not None:
        reasons.extend(
            verify_retained_file_snapshots(
                results_dir,
                expected_results_identity,
                [*retained_snapshots, rows_snapshot],
            )
        )
        reasons.extend(verify_file_snapshots(external_snapshots))
    else:
        reasons.extend(
            verify_file_snapshots(
                [metadata_snapshot, rows_snapshot, *snapshots.values()]
            )
        )
    metadata["authoritative_reasons"] = sorted(set(reasons))
    metadata["claim_status"] = "authoritative" if not metadata["authoritative_reasons"] else "non-authoritative"
    if args.require_authoritative:
        try:
            if path_identity(results_dir) != metadata.get("results_dir_identity"):
                metadata["authoritative_reasons"].append(
                    "authoritative results directory identity changed during finish"
                )
            if durability_capability(results_dir) != metadata.get("durability"):
                metadata["authoritative_reasons"].append(
                    "authoritative results directory durability changed during finish"
                )
        except (ContractError, OSError) as error:
            metadata["authoritative_reasons"].append(
                f"authoritative results directory recheck failed: {error}"
            )
        metadata["authoritative_reasons"] = sorted(set(metadata["authoritative_reasons"]))
        metadata["claim_status"] = (
            "authoritative"
            if not metadata["authoritative_reasons"]
            else "non-authoritative"
        )
    publish_benchmark_artifact(
        output,
        canonical_json(metadata) + b"\n",
        results_dir=results_dir,
        require_directory_fsync=args.require_authoritative,
        results_dir_identity=(
            metadata.get("results_dir_identity")
            if results_dir is not None
            else None
        ),
    )
    if args.require_authoritative and metadata["claim_status"] != "authoritative":
        raise ContractError("benchmark evidence is not authoritative: " + "; ".join(metadata["authoritative_reasons"]))
    return metadata


def write_superiority_summary(args: argparse.Namespace) -> None:
    root = absolute_directory(args.results_dir, reject_symlinks=True)
    identity = parse_artifact_identity(args.root_identity)
    metadata_path = absolute_file(args.metadata, reject_symlinks=True)
    rows_path = absolute_file(args.rows, reject_symlinks=True)
    output_path = absolute_output_path(args.output, reject_symlinks=True)
    for label, path in (
        ("metadata", metadata_path),
        ("rows", rows_path),
        ("superiority output", output_path),
    ):
        if not path_is_within(path, root):
            raise ContractError(f"{label} must be inside the retained results directory")
    metadata = load_json_bytes(
        artifact_read_bytes(
            root,
            artifact_relative_name(root, metadata_path),
            expected_directory_identity=identity,
            max_bytes=MAX_JSON_BYTES,
        ),
        metadata_path,
    )
    expected_fields = (
        STANDARD_RESULT_FIELDS
        if metadata.get("mandatory_manifest") == "standard"
        else OBSERVED_RESULT_FIELDS
    )
    rows = read_strict_tsv_bytes(
        artifact_read_bytes(
            root,
            artifact_relative_name(root, rows_path),
            expected_directory_identity=identity,
        ),
        rows_path,
        expected_fields=expected_fields,
    )
    summary_rows, _reasons = compute_superiority_summary(rows, metadata)
    artifact_write_bytes(
        root,
        artifact_relative_name(root, output_path),
        serialize_superiority_summary(summary_rows),
        exclusive=True,
        expected_directory_identity=identity,
    )


def parser() -> argparse.ArgumentParser:
    command = argparse.ArgumentParser(description=__doc__)
    subparsers = command.add_subparsers(dest="command", required=True)

    build = subparsers.add_parser("build-release")
    build.add_argument("--repo-root", required=True)
    build.add_argument("--git-bin", required=True)
    build.add_argument("--cargo-bin", required=True)
    build.add_argument("--rustc-bin", required=True)
    build.add_argument("--python-bin", required=True)
    build.add_argument("--make-bin", required=True)

    results = subparsers.add_parser("prepare-results-dir")
    results.add_argument("--path", required=True)
    results.add_argument("--require-existing", action="store_true")

    artifact_preflight = subparsers.add_parser("artifact-preflight")
    artifact_preflight.add_argument("--root", required=True)
    artifact_preflight.add_argument("--name", action="append", required=True)
    artifact_preflight.add_argument("--root-identity", default="")

    template_preflight = subparsers.add_parser("template-preflight")
    template_preflight.add_argument("--fixture-root", required=True)
    template_preflight.add_argument("--template-dir", required=True)
    template_preflight.add_argument("--expected-identity", default="")

    artifact_write = subparsers.add_parser("artifact-write")
    artifact_write.add_argument("--root", required=True)
    artifact_write.add_argument("--name", required=True)
    artifact_write.add_argument("--append", action="store_true")
    artifact_write.add_argument("--exclusive", action="store_true")
    artifact_write.add_argument("--root-identity", default="")

    artifact_read = subparsers.add_parser("artifact-read")
    artifact_read.add_argument("--root", required=True)
    artifact_read.add_argument("--name", required=True)
    artifact_read.add_argument("--root-identity", default="")

    artifact_copy = subparsers.add_parser("artifact-copy")
    artifact_copy.add_argument("--root", required=True)
    artifact_copy.add_argument("--name", required=True)
    artifact_copy.add_argument("--source", required=True)
    artifact_copy.add_argument("--root-identity", default="")

    publish_descriptor = subparsers.add_parser("publish-descriptor")
    publish_descriptor.add_argument("--path", required=True)

    superiority = subparsers.add_parser("superiority-summary")
    superiority.add_argument("--metadata", required=True)
    superiority.add_argument("--rows", required=True)
    superiority.add_argument("--output", required=True)
    superiority.add_argument("--results-dir", required=True)
    superiority.add_argument("--root-identity", required=True)

    hash_file = subparsers.add_parser("hash-file")
    hash_file.add_argument("--path", required=True)

    fixture_hash = subparsers.add_parser("fixture-hash")
    fixture_hash.add_argument("--root", required=True)
    fixture_hash.add_argument("--git-bin", required=True)
    fixture_hash.add_argument("--template-dir", default="")
    fixture_hash.add_argument("--template-identity", default="")
    fixture_hash.add_argument("--strict-paths", action="store_true")

    start = subparsers.add_parser("start")
    start.add_argument("--repo-root", required=True)
    start.add_argument("--git-bin", required=True)
    start.add_argument("--zmin-bin", required=True)
    start.add_argument("--python-bin", required=True)
    start.add_argument("--make-bin", required=True)
    start.add_argument("--fixture-root", required=True)
    start.add_argument("--results-dir", default="")
    start.add_argument("--output", required=True)
    start.add_argument("--mode", choices=("authoritative", "exploratory", "smoke"), default="exploratory")
    start.add_argument("--build-profile", default="")
    start.add_argument("--identity-sidecar", default="")
    start.add_argument("--command-corpus", required=True)
    start.add_argument("--warmups", required=True, type=int)
    start.add_argument("--measured-pairs", required=True, type=int)
    start.add_argument("--cold-starts", required=True, type=int)
    start.add_argument("--ordering", required=True)
    start.add_argument("--seed", required=True, type=int)
    start.add_argument("--harness", action="append", default=[])
    start.add_argument(
        "--mandatory-manifest",
        choices=("standard", "observed", "pilot"),
        default="pilot",
    )
    start.add_argument("--mandatory-lanes", default="")
    start.add_argument("--equivalence-manifest", default="")

    finish = subparsers.add_parser("finish")
    finish.add_argument("--metadata", required=True)
    finish.add_argument("--rows", required=True)
    finish.add_argument("--output", required=True)
    finish.add_argument("--result", action="append", default=[])
    finish.add_argument("--results-dir", default="")
    finish.add_argument("--require-authoritative", action="store_true")
    finish.add_argument("--anchor-repo-root", required=True)
    finish.add_argument("--anchor-fixture-root", required=True)
    finish.add_argument("--anchor-command-corpus", required=True)
    finish.add_argument("--anchor-fixture-sha256", required=True)
    finish.add_argument("--anchor-git-bin", required=True)
    finish.add_argument("--anchor-git-sha256", required=True)
    finish.add_argument("--anchor-git-version", required=True)
    finish.add_argument("--anchor-zmin-bin", required=True)
    finish.add_argument("--anchor-zmin-sha256", required=True)
    finish.add_argument("--anchor-zmin-version", required=True)
    finish.add_argument("--anchor-python-bin", required=True)
    finish.add_argument("--anchor-python-sha256", required=True)
    finish.add_argument("--anchor-python-version", required=True)
    finish.add_argument("--anchor-make-bin", required=True)
    finish.add_argument("--anchor-make-sha256", required=True)
    finish.add_argument("--anchor-make-version", required=True)
    finish.add_argument("--anchor-build-profile", required=True)
    finish.add_argument("--anchor-identity-sidecar", required=True)
    finish.add_argument("--anchor-identity-sidecar-sha256", required=True)
    finish.add_argument("--anchor-start-metadata-sha256", required=True)
    finish.add_argument("--anchor-harness", action="append", default=[])
    finish.add_argument("--anchor-harness-sha256", action="append", default=[])
    return command


def main() -> int:
    args = parser().parse_args()
    try:
        if args.command == "build-release":
            build_release_identity(args)
            return 0
        if args.command == "prepare-results-dir":
            print(
                prepare_results_directory(
                    args.path,
                    require_existing=args.require_existing,
                )
            )
            return 0
        if args.command == "artifact-preflight":
            identity = artifact_preflight_paths(
                pathlib.Path(args.root),
                args.name,
                expected_directory_identity=parse_artifact_identity(args.root_identity),
            )
            print(artifact_identity_token(identity))
            return 0
        if args.command == "template-preflight":
            binding = benchmark_template_binding(
                pathlib.Path(args.fixture_root),
                pathlib.Path(args.template_dir),
                expected_identity=parse_artifact_identity(args.expected_identity),
            )
            print(artifact_identity_token(binding["identity"]))
            return 0
        if args.command == "artifact-write":
            artifact_write_bytes(
                pathlib.Path(args.root),
                args.name,
                sys.stdin.buffer.read(),
                append=args.append,
                exclusive=args.exclusive,
                expected_directory_identity=parse_artifact_identity(args.root_identity),
            )
            return 0
        if args.command == "artifact-read":
            sys.stdout.buffer.write(
                artifact_read_bytes(
                    pathlib.Path(args.root),
                    args.name,
                    expected_directory_identity=parse_artifact_identity(args.root_identity),
                )
            )
            return 0
        if args.command == "artifact-copy":
            artifact_copy_file(
                pathlib.Path(args.root),
                args.name,
                pathlib.Path(args.source),
                expected_directory_identity=parse_artifact_identity(args.root_identity),
            )
            return 0
        if args.command == "publish-descriptor":
            destination = absolute_output_path(args.path, reject_symlinks=True)
            parent = absolute_directory(destination.parent, reject_symlinks=True)
            publish_benchmark_artifact(
                destination,
                sys.stdin.buffer.read(),
                results_dir=parent,
                results_dir_identity=path_identity(parent),
                require_directory_fsync=True,
            )
            return 0
        if args.command == "superiority-summary":
            write_superiority_summary(args)
            return 0
        if args.command == "hash-file":
            print(sha256_file(absolute_file(args.path)))
            return 0
        if args.command == "fixture-hash":
            has_template = bool(args.template_dir)
            has_template_identity = bool(args.template_identity)
            if has_template != has_template_identity:
                if has_template:
                    raise ContractError(
                        "fixture-hash --template-dir requires --template-identity"
                    )
                raise ContractError(
                    "fixture-hash --template-identity requires --template-dir"
                )
            root = absolute_directory(args.root, reject_symlinks=args.strict_paths)
            git_bin = absolute_file(
                args.git_bin,
                executable=True,
                reject_symlinks=args.strict_paths,
            )
            bound_template_directories = None
            if has_template:
                expected_identity = parse_artifact_identity(args.template_identity)
                if expected_identity is None:
                    raise ContractError(
                        "fixture-hash --template-identity is invalid"
                    )
                _binding, bound_template_directories = (
                    benchmark_template_binding_and_identity_map(
                        root,
                        pathlib.Path(args.template_dir),
                        expected_identity=expected_identity,
                    )
                )
            print(
                fixture_fingerprint(
                    root,
                    git_bin,
                    bound_directory_identities=bound_template_directories,
                )
            )
            return 0
        if args.command == "start":
            metadata = start_metadata(args)
            output = absolute_output_path(
                args.output,
                reject_symlinks=args.mode == "authoritative" or bool(args.results_dir),
            )
            retained_results_dir = (
                pathlib.Path(metadata["results_dir"])
                if metadata.get("results_dir")
                else None
            )
            if args.mode == "authoritative":
                results_dir = absolute_directory(args.results_dir, reject_symlinks=True)
                if not path_is_within(output, results_dir):
                    raise ContractError(
                        "authoritative metadata must be inside the retained results directory"
                    )
                start_collisions = [
                    ("git binary", absolute_file(args.git_bin, executable=True, reject_symlinks=True)),
                    ("zmin binary", absolute_file(args.zmin_bin, executable=True, reject_symlinks=True)),
                    ("Python binary", absolute_file(args.python_bin, executable=True, reject_symlinks=True)),
                    ("Make binary", absolute_file(args.make_bin, executable=True, reject_symlinks=True)),
                    (
                        "identity sidecar",
                        absolute_output_path(
                            args.identity_sidecar or identity_sidecar(
                                absolute_file(
                                    args.zmin_bin,
                                    executable=True,
                                    reject_symlinks=True,
                                )
                            ),
                            reject_symlinks=True,
                        ),
                    ),
                ]
                start_collisions.extend(
                    (
                        "harness",
                        absolute_file(path, reject_symlinks=True),
                    )
                    for path in args.harness
                )
                reject_output_collisions(output, start_collisions)
            publish_benchmark_artifact(
                output,
                canonical_json(metadata) + b"\n",
                results_dir=retained_results_dir,
                require_directory_fsync=args.mode == "authoritative",
                results_dir_identity=(
                    metadata.get("results_dir_identity")
                    if retained_results_dir is not None
                    else None
                ),
            )
            print(f"performance_metadata={output}")
            if args.mode == "authoritative" and metadata["authoritative_reasons"]:
                print("performance_contract=fail-closed", file=sys.stderr)
                for reason in metadata["authoritative_reasons"]:
                    print(f"performance_contract_reason={reason}", file=sys.stderr)
                return 1
            print(f"performance_claim={metadata['claim_status']}")
            return 0
        if args.command == "finish":
            metadata = finish_metadata(args)
            print(f"performance_evidence={pathlib.Path(args.output).expanduser().resolve()}")
            print(f"performance_claim={metadata['claim_status']}")
            return 0
    except ContractError as error:
        print(f"performance contract error: {error}", file=sys.stderr)
        return 1
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
