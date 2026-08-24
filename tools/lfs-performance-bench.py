#!/usr/bin/env python3
"""Cross-platform Git LFS throughput and peak-memory comparison gate.

The benchmark never builds or downloads a comparator.  It requires pinned,
absolute executables, runs an IPv4 loopback Basic Transfer fixture outside the
measured process, and streams every object through fixed-size buffers.
"""

from __future__ import annotations

import argparse
import ctypes
import csv
import errno
import hashlib
import http.client
import http.server
import importlib.util
import io
import json
import math
import os
import pathlib
import platform
import re
import secrets
import shutil
import socket
import statistics
import stat
import subprocess
import sys
import tempfile
import threading
import time
import unicodedata
import urllib.parse
from dataclasses import dataclass, field, replace
from enum import Enum
from typing import BinaryIO, Callable, Iterable, Sequence

sys.dont_write_bytecode = True
os.environ["PYTHONDONTWRITEBYTECODE"] = "1"
BENCHMARK_SCRIPT_PATH = pathlib.Path(__file__).resolve(strict=True)
BENCHMARK_TOOLS_DIR = BENCHMARK_SCRIPT_PATH.parent


def load_canonical_tools_module(module_name: str, file_name: str):
    module_path = (BENCHMARK_TOOLS_DIR / file_name).resolve(strict=True)
    if module_path.parent != BENCHMARK_TOOLS_DIR or not module_path.is_file():
        raise RuntimeError("performance support module path is not canonical")
    spec = importlib.util.spec_from_file_location(module_name, module_path)
    if spec is None or spec.loader is None:
        raise RuntimeError("performance support module could not be loaded")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


contract = load_canonical_tools_module("performance_contract", "performance_contract.py")
host_noise = load_canonical_tools_module(
    "performance_host_noise", "performance-host-noise.py"
)


GATE_OBJECT_COUNT = 8
GATE_OBJECT_BYTES = 16 * 1024 * 1024
STREAM_CHUNK_BYTES = 128 * 1024
GATE_CONCURRENCIES = (1, 2, 8)
DEFAULT_WARMUPS = 2
DEFAULT_REPEATS = 40
MAX_BATCH_BYTES = 64 * 1024
MAX_CAPTURE_BYTES = 256 * 1024
MAX_MANIFEST_BYTES = 64 * 1024
MAX_REQUEST_PATH_BYTES = 4 * 1024
TRANSFER_START_WINDOW_SECONDS = 5.0
SERVER_COMPLETION_TIMEOUT_SECONDS = 10.0
DEFAULT_CHILD_TIMEOUT_SECONDS = 180.0
MAX_CHILD_TIMEOUT_SECONDS = 60 * 60.0
LFS_MEDIA_TYPE = "application/vnd.git-lfs+json"
MAX_RELEASE_SIDECAR_BYTES = 64 * 1024
MAX_CARGO_LOCK_BYTES = 8 * 1024 * 1024
MAX_EVIDENCE_ARTIFACT_BYTES = 8 * 1024 * 1024
MAX_EVIDENCE_MANIFEST_BYTES = 256 * 1024
MAX_FAILED_EVIDENCE_BYTES = 16 * 1024 * 1024
MAX_EVIDENCE_CELL_BYTES = 1024
MAX_EVIDENCE_COUNTER = (1 << 64) - 1
LFS_PERFORMANCE_SCHEMA_NAME = "zmin-lfs-performance-v4"
LFS_PERFORMANCE_SCHEMA_VERSION = "4"
RELEASE_PROVENANCE_SCHEMA = "zmin-lfs-release-provenance-v2"
EVIDENCE_SET_SCHEMA = "zmin-lfs-performance-evidence-set-v4"
EVIDENCE_SET_FILES = (
    "rows.tsv",
    "summary.tsv",
    "comparison.tsv",
    "metadata.tsv",
)
EVIDENCE_MANIFEST_NAME = "evidence-set.json"
FAILED_EVIDENCE_SCHEMA = "zmin-lfs-performance-failed-evidence-v1"
FAILED_EVIDENCE_MANIFEST_NAME = "failed-evidence.json"
FAILED_CANDIDATE_MANIFEST_NAME = "candidate-evidence-set.json"
FAILED_ARTIFACT_NAMES = {
    "rows.tsv": "acquired-rows.tsv",
    "summary.tsv": "candidate-summary.tsv",
    "comparison.tsv": "candidate-comparison.tsv",
    "metadata.tsv": "candidate-metadata.tsv",
}
FAILED_EVIDENCE_FILES = (
    *FAILED_ARTIFACT_NAMES.values(),
    FAILED_CANDIDATE_MANIFEST_NAME,
    FAILED_EVIDENCE_MANIFEST_NAME,
)
EVIDENCE_ARTIFACT_SCHEMAS = {
    "rows.tsv": "zmin-lfs-performance-rows-v4",
    "summary.tsv": "zmin-lfs-performance-summary-v4",
    "comparison.tsv": "zmin-lfs-performance-comparison-v4",
    "metadata.tsv": "zmin-lfs-performance-metadata-v4",
}
MEMORY_EVIDENCE_CONTRACTS = {
    "peak_rss_bytes": ("working_set_peak", "waited_child_processes"),
    "peak_job_commit_bytes": ("job_commit_peak", "job_process_tree"),
}
RELEASE_PROVENANCE_FIELDS = (
    "provenance_schema",
    "source_commit",
    "source_tree",
    "source_status_sha256",
    "cargo_lock_sha256",
    "identity_sidecar_sha256",
    "identity_sidecar_payload_sha256",
    "identity_sidecar_schema_version",
    "identity_sidecar_marker",
    "zmin_sha256",
    "git_sha256",
    "python_sha256",
    "python_version",
)
SCHEDULE_BINDING_FIELDS = (
    "schema_name",
    "schema_version",
    "source_commit",
    "source_tree",
    "cargo_lock_sha256",
    "identity_sidecar_sha256",
    "identity_sidecar_payload_sha256",
    "zmin_sha256",
    "git_sha256",
    "git_lfs_sha256",
    "python_sha256",
    "fixture_chunk_bytes",
    "fixture_object_count",
    "fixture_object_bytes",
    "concurrencies",
    "warmups",
    "repeats",
    "timeout_seconds",
)
LFS_PERFORMANCE_METADATA_FIELDS = (
    "schema_name",
    "schema_version",
    "claim",
    "cross_platform_memory_comparison",
    "fixture_chunk_bytes",
    "fixture_object_bytes",
    "fixture_object_count",
    "concurrencies",
    "host",
    "host_monitor_adapter",
    "host_monitor_interval_seconds",
    "host_noise_cpu_policy",
    "host_noise_pressure_policy",
    "memory_metric",
    "ordering",
    "schedule_binding_sha256",
    "repeats",
    "timeout_seconds",
    "warmups",
    "git_lfs_role",
    "git_lfs_sha256",
    "git_lfs_version",
    "git_role",
    "git_sha256",
    "git_version",
    "python_role",
    "python_sha256",
    "python_version",
    "zmin_role",
    "zmin_sha256",
    "zmin_version",
    "source_commit",
    "source_tree",
    "source_status_sha256",
    "cargo_lock_sha256",
    "identity_sidecar_role",
    "identity_sidecar_sha256",
    "identity_sidecar_payload_sha256",
    "identity_sidecar_schema_version",
    "identity_sidecar_marker",
    "provenance_schema",
    "release_provenance_sha256",
    "release_provenance_verified",
    "build_profile",
    "binary_path_profile",
)


class BenchmarkError(RuntimeError):
    """The benchmark contract or a functional check failed."""


class BenchmarkOperation(str, Enum):
    DOWNLOAD = "download"
    UPLOAD = "upload"


class BenchmarkTool(str, Enum):
    STOCK = "stock-lfs"
    ZMIN = "zmin"


class SampleKind(str, Enum):
    WARMUP = "warmup"
    MEASURED = "measured"


class BenchmarkClaim(str, Enum):
    STRICT_SUPERIORITY = "strict-within-host-superiority"
    STRICT_NOT_ESTABLISHED = "strict-within-host-superiority-not-established"
    FUNCTIONAL_SMOKE = "functional-smoke"


@dataclass(frozen=True)
class BenchmarkLane:
    operation: BenchmarkOperation
    concurrency: int


METADATA_FIXED_VALUE_ALLOWLIST = {
    "schema_name": frozenset({LFS_PERFORMANCE_SCHEMA_NAME}),
    "schema_version": frozenset({LFS_PERFORMANCE_SCHEMA_VERSION}),
    "claim": frozenset(claim.value for claim in BenchmarkClaim),
    "cross_platform_memory_comparison": frozenset({"forbidden"}),
    "fixture_chunk_bytes": frozenset({str(STREAM_CHUNK_BYTES)}),
    "memory_metric": frozenset(MEMORY_EVIDENCE_CONTRACTS),
    "host_monitor_adapter": frozenset(
        {"darwin-stdlib-v1", "linux-stdlib-v1", "windows-stdlib-v1"}
    ),
    "host_monitor_interval_seconds": frozenset(
        {f"{host_noise.SAMPLE_INTERVAL_SECONDS:.3f}"}
    ),
    "host_noise_cpu_policy": frozenset({"diagnostic-only-no-numeric-threshold"}),
    "host_noise_pressure_policy": frozenset({"normal-only"}),
    "ordering": frozenset({"sha256-block-rounds-adjacent-balanced"}),
    "git_lfs_role": frozenset({"pinned-git-lfs"}),
    "git_role": frozenset({"source-inspector-git"}),
    "python_role": frozenset({"release-builder-python"}),
    "zmin_role": frozenset({"canonical-release-zmin"}),
    "identity_sidecar_role": frozenset({"sanitized-release-identity"}),
    "identity_sidecar_marker": frozenset({contract.RELEASE_BUILD_MARKER}),
    "provenance_schema": frozenset({RELEASE_PROVENANCE_SCHEMA}),
    "release_provenance_verified": frozenset({"true"}),
    "build_profile": frozenset({contract.AUTHORITATIVE_PROFILE}),
    "binary_path_profile": frozenset({contract.AUTHORITATIVE_PROFILE}),
}


@dataclass(frozen=True)
class ExecutableIdentity:
    path: pathlib.Path
    sha256: str
    version: str


@dataclass(frozen=True)
class ReleaseProvenance:
    repo_root: pathlib.Path
    source_commit: str
    source_tree: str
    source_status_sha256: str
    cargo_lock_sha256: str
    identity_sidecar_path: pathlib.Path
    identity_sidecar_sha256: str
    identity_sidecar_payload_sha256: str
    identity_sidecar_schema_version: int
    identity_sidecar_marker: str
    zmin_sha256: str
    git_sha256: str
    python_sha256: str
    python_version: str
    provenance_sha256: str


def verify_benchmark_source_binding(provenance: ReleaseProvenance) -> None:
    try:
        expected_tools = contract.absolute_directory(
            provenance.repo_root / "tools", reject_symlinks=True
        )
        expected_script = contract.absolute_file(
            expected_tools / "lfs-performance-bench.py", reject_symlinks=True
        )
        expected_contract = contract.absolute_file(
            expected_tools / "performance_contract.py", reject_symlinks=True
        )
        expected_host_noise = contract.absolute_file(
            expected_tools / "performance-host-noise.py", reject_symlinks=True
        )
        loaded_contract = pathlib.Path(contract.__file__).resolve(strict=True)
        loaded_host_noise = pathlib.Path(host_noise.__file__).resolve(strict=True)
    except (AttributeError, OSError, contract.ContractError) as error:
        raise BenchmarkError("benchmark source binding is unavailable") from error
    if (
        BENCHMARK_TOOLS_DIR != expected_tools
        or BENCHMARK_SCRIPT_PATH != expected_script
        or loaded_contract != expected_contract
        or loaded_host_noise != expected_host_noise
    ):
        raise BenchmarkError("benchmark source binding does not match provenance")


@dataclass(frozen=True)
class EvidenceArtifact:
    name: str
    schema: str
    columns: tuple[str, ...]
    records: int
    data: bytes


@dataclass(frozen=True)
class EvidenceSet:
    artifacts: tuple[EvidenceArtifact, ...]
    manifest: bytes
    root_sha256: str


@dataclass(frozen=True)
class FailedEvidenceBundle:
    files: tuple[tuple[str, bytes], ...]
    manifest: bytes
    root_sha256: str


@dataclass(frozen=True)
class BenchmarkInvocation:
    argv_sha256: str


@dataclass(frozen=True)
class VerifiedEvidenceSet:
    root_sha256: str
    metadata: dict[str, str]


@dataclass(frozen=True)
class EvidencePublicationHooks:
    before_final_check: Callable[[], None] | None = None
    before_publish: Callable[[], None] | None = None
    after_publish: Callable[[], None] | None = None


@dataclass(frozen=True)
class FailedEvidencePublicationHooks:
    before_freeze: Callable[[pathlib.Path], None] | None = None
    before_publish: Callable[[pathlib.Path], None] | None = None
    after_readback: Callable[[pathlib.Path], None] | None = None


@dataclass(frozen=True)
class LfsObject:
    index: int
    oid: str
    size: int
    path: pathlib.Path


@dataclass(frozen=True)
class RunSpecification:
    token: str
    operation: BenchmarkOperation
    concurrency: int
    objects: tuple[LfsObject, ...]


@dataclass(frozen=True)
class RunEvidence:
    batch_requests: int
    transfer_requests: int
    max_active: int
    residual_active: int
    completed_oids: tuple[str, ...]
    completed_bytes: int
    errors: tuple[str, ...]


@dataclass(frozen=True)
class ProcessMetrics:
    wall_seconds: float
    user_seconds: str
    system_seconds: str
    peak_rss_bytes: str
    peak_job_commit_bytes: str
    major_page_faults: str
    minor_page_faults: str
    read_bytes: str
    write_bytes: str
    memory_metric: str
    memory_semantics: str
    memory_scope: str
    memory_unit: str
    availability: str

    def memory_bytes(self) -> int:
        value = (
            self.peak_rss_bytes
            if self.memory_metric == "peak_rss_bytes"
            else self.peak_job_commit_bytes
            if self.memory_metric == "peak_job_commit_bytes"
            else "unsupported"
        )
        if not value.isdecimal() or int(value) <= 0:
            raise BenchmarkError("required platform peak-memory metric is unavailable")
        return int(value)


@dataclass(frozen=True)
class BenchmarkRow:
    tool: str
    operation: str
    concurrency: int
    sample_kind: str
    pair_id: str
    order_index: int
    sample_index: int
    object_count: int
    object_bytes: int
    payload_bytes: int
    wall_seconds: float
    throughput_mib_per_second: float
    user_seconds: str
    system_seconds: str
    peak_rss_bytes: str
    peak_job_commit_bytes: str
    memory_metric: str
    memory_semantics: str
    memory_scope: str
    exit_code: int
    batch_requests: int
    transfer_requests: int
    max_active: int
    completed_objects: int
    completed_bytes: int
    residual_active: int
    host_scheduler_lag_max_seconds: float
    host_system_cpu_busy_ratio: str
    host_load_average_1m: str
    host_memory_pressure: str
    host_thermal_state: str
    host_monitor_samples: int
    stdout_sha256: str
    stderr_sha256: str


@dataclass(frozen=True)
class SummaryRow:
    operation: str
    concurrency: int
    tool: str
    samples: int
    median_seconds: float
    p95_seconds: float
    median_mib_per_second: float
    maximum_peak_memory_bytes: int
    memory_metric: str


@dataclass(frozen=True)
class ComparisonRow:
    operation: str
    concurrency: int
    median_wall_ratio: float
    p95_wall_ratio: float
    peak_memory_ratio: float
    paired_median_wall_ratio: float
    paired_median_upper_95_ratio: str
    stock_first_paired_median_ratio: float
    zmin_first_paired_median_ratio: float
    verdict: str


@dataclass(frozen=True)
class BenchmarkPolicy:
    object_count: int
    object_bytes: int
    concurrencies: tuple[int, ...]
    warmups: int
    repeats: int
    child_timeout_seconds: float
    strict_gate: bool

    @classmethod
    def from_args(cls, args: argparse.Namespace) -> BenchmarkPolicy:
        concurrencies = parse_concurrencies(args.concurrency)
        policy = cls(
            object_count=args.object_count,
            object_bytes=args.object_bytes,
            concurrencies=concurrencies,
            warmups=args.warmups,
            repeats=args.repeats,
            child_timeout_seconds=args.timeout_seconds,
            strict_gate=not args.smoke,
        )
        policy.validate()
        return policy

    def validate(self) -> None:
        EvidencePolicy.from_benchmark_policy(self).validate()


class EvidencePolicyMode(str, Enum):
    STRICT = "strict"
    FUNCTIONAL_SMOKE = "functional-smoke"


@dataclass(frozen=True)
class EvidencePolicy:
    mode: EvidencePolicyMode
    object_count: int
    object_bytes: int
    concurrencies: tuple[int, ...]
    warmups: int
    repeats: int
    timeout_seconds: float

    @classmethod
    def from_benchmark_policy(cls, policy: BenchmarkPolicy) -> EvidencePolicy:
        return cls(
            mode=(
                EvidencePolicyMode.STRICT
                if policy.strict_gate
                else EvidencePolicyMode.FUNCTIONAL_SMOKE
            ),
            object_count=policy.object_count,
            object_bytes=policy.object_bytes,
            concurrencies=policy.concurrencies,
            warmups=policy.warmups,
            repeats=policy.repeats,
            timeout_seconds=policy.child_timeout_seconds,
        )

    @classmethod
    def from_metadata(cls, metadata: dict[str, str]) -> EvidencePolicy:
        try:
            claim = BenchmarkClaim(metadata["claim"])
            object_count = parse_decimal_policy(metadata["fixture_object_count"])
            object_bytes = parse_decimal_policy(metadata["fixture_object_bytes"])
            warmups = parse_decimal_policy(metadata["warmups"])
            repeats = parse_decimal_policy(metadata["repeats"])
            concurrencies = parse_concurrencies(metadata["concurrencies"])
            timeout_seconds = parse_canonical_timeout_seconds(
                metadata["timeout_seconds"]
            )
        except (KeyError, ValueError) as error:
            raise BenchmarkError("LFS performance workload policy is invalid") from error
        policy = cls(
            mode=(
                EvidencePolicyMode.FUNCTIONAL_SMOKE
                if claim == BenchmarkClaim.FUNCTIONAL_SMOKE
                else EvidencePolicyMode.STRICT
            ),
            object_count=object_count,
            object_bytes=object_bytes,
            concurrencies=concurrencies,
            warmups=warmups,
            repeats=repeats,
            timeout_seconds=timeout_seconds,
        )
        policy.validate()
        return policy

    def validate(self) -> None:
        if self.object_count <= 0 or self.object_count > GATE_OBJECT_COUNT:
            raise BenchmarkError("object count must be in 1..=8")
        if self.object_bytes <= 0 or self.object_bytes > GATE_OBJECT_BYTES:
            raise BenchmarkError("object bytes must be in 1..=16777216")
        if not self.concurrencies:
            raise BenchmarkError("at least one concurrency is required")
        if len(self.concurrencies) != len(set(self.concurrencies)):
            raise BenchmarkError("concurrency values must be unique")
        if tuple(sorted(self.concurrencies)) != self.concurrencies:
            raise BenchmarkError("concurrency values must be sorted")
        if any(value <= 0 or value > self.object_count for value in self.concurrencies):
            raise BenchmarkError("every concurrency must be positive and no larger than object count")
        if self.warmups < 0 or self.repeats <= 0:
            raise BenchmarkError("warmups must be nonnegative and repeats must be positive")
        validate_child_timeout(self.timeout_seconds)
        if self.mode == EvidencePolicyMode.STRICT and (
            self.object_count != GATE_OBJECT_COUNT
            or self.object_bytes != GATE_OBJECT_BYTES
            or self.concurrencies != GATE_CONCURRENCIES
            or self.warmups != DEFAULT_WARMUPS
            or self.repeats != DEFAULT_REPEATS
            or self.timeout_seconds != DEFAULT_CHILD_TIMEOUT_SECONDS
        ):
            raise BenchmarkError(
                "the strict gate requires exactly 8 objects of 16 MiB, concurrency "
                "1,2,8, two warmups, forty measured repeats, and a 180-second timeout"
            )

    @property
    def lane_count(self) -> int:
        return len(BenchmarkOperation) * len(self.concurrencies)

    @property
    def raw_row_count(self) -> int:
        return self.lane_count * (self.warmups + self.repeats) * len(BenchmarkTool)

    @property
    def summary_row_count(self) -> int:
        return self.lane_count * len(BenchmarkTool)


@dataclass
class MutableRunEvidence:
    batch_requests: int = 0
    transfer_requests: int = 0
    active: int = 0
    max_active: int = 0
    completed_bytes: int = 0
    completed_oids: set[str] = field(default_factory=set)
    errors: list[str] = field(default_factory=list)


class FixtureRunState:
    def __init__(self, specification: RunSpecification) -> None:
        self.specification = specification
        self.objects = {item.oid: item for item in specification.objects}
        self.lock = threading.Lock()
        self.changed = threading.Condition(self.lock)
        self.evidence = MutableRunEvidence()

    def record_batch(self, operation: str, objects: object) -> None:
        expected = {(item.oid, item.size) for item in self.specification.objects}
        if operation != self.specification.operation.value:
            raise BenchmarkError("Batch operation does not match the active benchmark lane")
        if not isinstance(objects, list):
            raise BenchmarkError("Batch objects must be an array")
        actual: list[tuple[str, int]] = []
        for entry in objects:
            if not isinstance(entry, dict):
                raise BenchmarkError("Batch object must be a JSON object")
            oid = entry.get("oid")
            size = entry.get("size")
            if not isinstance(oid, str) or not isinstance(size, int):
                raise BenchmarkError("Batch object identity is invalid")
            actual.append((oid, size))
        if len(actual) != len(set(actual)) or set(actual) != expected:
            raise BenchmarkError("Batch object set does not match the fixture")
        with self.lock:
            self.evidence.batch_requests += 1
            if self.evidence.batch_requests != 1:
                raise BenchmarkError("the fixture requires exactly one Batch request")

    def object_for_transfer(self, oid: str) -> LfsObject:
        item = self.objects.get(oid)
        if item is None:
            raise BenchmarkError("transfer requested an unknown object")
        return item

    def begin_transfer(self, oid: str) -> LfsObject:
        item = self.object_for_transfer(oid)
        with self.lock:
            if oid in self.evidence.completed_oids:
                raise BenchmarkError("transfer requested an object more than once")
            self.evidence.transfer_requests += 1
            self.evidence.active += 1
            self.evidence.max_active = max(self.evidence.max_active, self.evidence.active)
            self.changed.notify_all()
            deadline = time.monotonic() + TRANSFER_START_WINDOW_SECONDS
            while self.evidence.active < self.specification.concurrency:
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    break
                self.changed.wait(remaining)
        return item

    def finish_transfer(self, oid: str | None, transferred: int) -> None:
        with self.lock:
            if self.evidence.active <= 0:
                self.evidence.errors.append("active transfer accounting underflow")
            else:
                self.evidence.active -= 1
            if oid is not None:
                self.evidence.completed_oids.add(oid)
                self.evidence.completed_bytes += transferred
            self.changed.notify_all()

    def record_error(self, message: str) -> None:
        with self.lock:
            self.evidence.errors.append(message)
            self.changed.notify_all()

    def wait_for_completion(self, timeout_seconds: float) -> RunEvidence:
        deadline = time.monotonic() + timeout_seconds
        with self.changed:
            while (
                len(self.evidence.completed_oids) < len(self.specification.objects)
                or self.evidence.active != 0
            ) and not self.evidence.errors:
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    self.evidence.errors.append("fixture completion timed out")
                    break
                self.changed.wait(remaining)
            return RunEvidence(
                batch_requests=self.evidence.batch_requests,
                transfer_requests=self.evidence.transfer_requests,
                max_active=self.evidence.max_active,
                residual_active=self.evidence.active,
                completed_oids=tuple(sorted(self.evidence.completed_oids)),
                completed_bytes=self.evidence.completed_bytes,
                errors=tuple(self.evidence.errors),
            )


class LfsFixtureHttpServer(http.server.ThreadingHTTPServer):
    address_family = socket.AF_INET
    daemon_threads = False
    block_on_close = True
    request_queue_size = 128

    def __init__(self) -> None:
        super().__init__(("127.0.0.1", 0), LfsFixtureRequestHandler)
        self.states: dict[str, FixtureRunState] = {}
        self.states_lock = threading.Lock()

    def register(self, specification: RunSpecification) -> FixtureRunState:
        state = FixtureRunState(specification)
        with self.states_lock:
            if specification.token in self.states:
                raise BenchmarkError("duplicate fixture run token")
            self.states[specification.token] = state
        return state

    def state(self, token: str) -> FixtureRunState:
        with self.states_lock:
            state = self.states.get(token)
        if state is None:
            raise BenchmarkError("unknown fixture run token")
        return state

    def unregister(self, token: str) -> None:
        with self.states_lock:
            self.states.pop(token, None)

    def endpoint(self, token: str) -> str:
        host, port = self.server_address
        return f"http://{host}:{port}/{token}"


class LfsFixtureRequestHandler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, _format: str, *_arguments: object) -> None:
        return

    def do_POST(self) -> None:
        self._dispatch(None)

    def do_GET(self) -> None:
        self._dispatch(BenchmarkOperation.DOWNLOAD)

    def do_PUT(self) -> None:
        self._dispatch(BenchmarkOperation.UPLOAD)

    def _dispatch(self, transfer_operation: BenchmarkOperation | None) -> None:
        state: FixtureRunState | None = None
        try:
            if len(self.path.encode("utf-8", "surrogatepass")) > MAX_REQUEST_PATH_BYTES:
                raise BenchmarkError("request path is too large")
            parsed = urllib.parse.urlsplit(self.path)
            if parsed.query or parsed.fragment:
                raise BenchmarkError("fixture request URL must not contain query or fragment")
            parts = [part for part in parsed.path.split("/") if part]
            if len(parts) < 3:
                raise BenchmarkError("fixture request path is invalid")
            token = parts[0]
            server = self.server
            if not isinstance(server, LfsFixtureHttpServer):
                raise BenchmarkError("fixture server type is invalid")
            state = server.state(token)
            if self.command == "POST" and parts[1:] == ["objects", "batch"]:
                self._batch(state, server)
                return
            if transfer_operation is None or len(parts) != 3 or parts[1] != "objects":
                raise BenchmarkError(
                    f"fixture request path is unsupported: {self.command} {parsed.path}"
                )
            if transfer_operation != state.specification.operation:
                raise BenchmarkError("transfer method does not match the active lane")
            oid = parts[2]
            if transfer_operation == BenchmarkOperation.DOWNLOAD:
                self._download(state, oid)
            else:
                self._upload(state, oid)
        except Exception as error:
            if state is not None:
                state.record_error(str(error))
            self._error_response()

    def _batch(self, state: FixtureRunState, server: LfsFixtureHttpServer) -> None:
        body = self._read_bounded_body(MAX_BATCH_BYTES)
        try:
            payload = json.loads(body)
        except (UnicodeDecodeError, json.JSONDecodeError) as error:
            raise BenchmarkError("Batch request is not valid bounded JSON") from error
        if not isinstance(payload, dict):
            raise BenchmarkError("Batch request must be a JSON object")
        operation = payload.get("operation")
        state.record_batch(operation, payload.get("objects"))
        action_name = state.specification.operation.value
        response_objects = []
        for item in sorted(state.specification.objects, key=lambda value: value.oid):
            response_objects.append(
                {
                    "oid": item.oid,
                    "size": item.size,
                    "authenticated": True,
                    "actions": {
                        action_name: {
                            "href": f"{server.endpoint(state.specification.token)}/objects/{item.oid}"
                        }
                    },
                }
            )
        response = json.dumps(
            {"transfer": "basic", "objects": response_objects},
            ensure_ascii=True,
            separators=(",", ":"),
            sort_keys=True,
        ).encode("ascii")
        self._write_response(200, response, LFS_MEDIA_TYPE)

    def _download(self, state: FixtureRunState, oid: str) -> None:
        item = state.object_for_transfer(oid)
        transferred = 0
        completed = False
        try:
            # Git LFS v3.7.1 deliberately lets worker zero validate the first
            # response headers before releasing its other Basic-transfer
            # workers.  Publish those headers before the fixture rendezvous so
            # both comparators can reach the configured concurrency exactly.
            self.send_response(200)
            self.send_header("Content-Type", "application/octet-stream")
            self.send_header("Content-Length", str(item.size))
            self.send_header("Connection", "close")
            self.end_headers()
            state.begin_transfer(oid)
            with item.path.open("rb", buffering=0) as source:
                while transferred < item.size:
                    chunk = source.read(min(STREAM_CHUNK_BYTES, item.size - transferred))
                    if not chunk:
                        raise BenchmarkError("fixture source was truncated")
                    self.wfile.write(chunk)
                    transferred += len(chunk)
            self.wfile.flush()
            completed = True
        finally:
            state.finish_transfer(oid if completed else None, transferred if completed else 0)

    def _upload(self, state: FixtureRunState, oid: str) -> None:
        item = state.begin_transfer(oid)
        transferred = 0
        completed = False
        try:
            length = self._content_length()
            if length != item.size:
                raise BenchmarkError("upload Content-Length does not match the pointer size")
            digest = hashlib.sha256()
            while transferred < item.size:
                chunk = self.rfile.read(min(STREAM_CHUNK_BYTES, item.size - transferred))
                if not chunk:
                    raise BenchmarkError("upload body was truncated")
                digest.update(chunk)
                transferred += len(chunk)
            if digest.hexdigest() != oid:
                raise BenchmarkError("upload body hash does not match the pointer OID")
            completed = True
            self._write_response(200, b"", "application/octet-stream")
        finally:
            state.finish_transfer(oid if completed else None, transferred if completed else 0)

    def _content_length(self) -> int:
        if self.headers.get("Transfer-Encoding") is not None:
            raise BenchmarkError("chunked fixture requests are unsupported")
        value = self.headers.get("Content-Length")
        if value is None or not value.isdecimal():
            raise BenchmarkError("fixture request requires a decimal Content-Length")
        return int(value)

    def _read_bounded_body(self, maximum: int) -> bytes:
        length = self._content_length()
        if length > maximum:
            raise BenchmarkError("fixture request body is too large")
        body = self.rfile.read(length)
        if len(body) != length:
            raise BenchmarkError("fixture request body was truncated")
        return body

    def _write_response(self, status: int, body: bytes, content_type: str) -> None:
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Connection", "close")
        self.end_headers()
        if body:
            self.wfile.write(body)
        self.wfile.flush()

    def _error_response(self) -> None:
        try:
            self._write_response(500, b"fixture error\n", "text/plain")
        except (BrokenPipeError, ConnectionError, OSError):
            pass


class FixtureServer:
    def __init__(self) -> None:
        self.server = LfsFixtureHttpServer()
        self.thread = threading.Thread(
            target=self.server.serve_forever,
            name="zmin-lfs-performance-fixture",
        )

    def __enter__(self) -> FixtureServer:
        self.thread.start()
        return self

    def __exit__(self, _kind: object, _value: object, _traceback: object) -> None:
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=SERVER_COMPLETION_TIMEOUT_SECONDS)
        if self.thread.is_alive():
            raise BenchmarkError("fixture server thread did not terminate")


def parse_args(argv: Sequence[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--repo-root",
        required=True,
        type=pathlib.Path,
        help="clean source repository bound to the sanitized release sidecar",
    )
    parser.add_argument("--zmin-bin", required=True, type=pathlib.Path)
    parser.add_argument("--zmin-sha256", required=True)
    parser.add_argument("--git-bin", required=True, type=pathlib.Path)
    parser.add_argument("--git-sha256", required=True)
    parser.add_argument("--git-lfs-manifest", required=True, type=pathlib.Path)
    parser.add_argument("--output-dir", required=True, type=pathlib.Path)
    parser.add_argument(
        "--failure-dir",
        required=True,
        type=pathlib.Path,
        help="fresh explicit custody directory for a failed post-acquisition evidence set",
    )
    parser.add_argument("--warmups", type=int, default=DEFAULT_WARMUPS)
    parser.add_argument("--repeats", type=int, default=DEFAULT_REPEATS)
    parser.add_argument("--object-count", type=int, default=GATE_OBJECT_COUNT)
    parser.add_argument("--object-bytes", type=int, default=GATE_OBJECT_BYTES)
    parser.add_argument("--concurrency", default="1,2,8")
    parser.add_argument("--timeout-seconds", type=float, default=DEFAULT_CHILD_TIMEOUT_SECONDS)
    parser.add_argument(
        "--smoke",
        action="store_true",
        help="allow a smaller functional fixture and do not make a superiority claim",
    )
    return parser.parse_args(argv)


def parse_concurrencies(raw: str) -> tuple[int, ...]:
    if not isinstance(raw, str) or len(raw) > 256:
        raise BenchmarkError("concurrency policy is invalid")
    parts = raw.split(",")
    try:
        values = tuple(parse_canonical_decimal_cell(part) for part in parts)
    except BenchmarkError as error:
        raise BenchmarkError("concurrency must be a canonical comma-separated integer list") from error
    if len(values) != len(set(values)) or tuple(sorted(values)) != values:
        raise BenchmarkError("concurrency values must be unique and sorted")
    return values


def parse_decimal_policy(raw: str) -> int:
    try:
        return parse_canonical_decimal_cell(raw)
    except BenchmarkError as error:
        raise BenchmarkError("LFS performance numeric policy is invalid") from error


def validate_child_timeout(value: float) -> None:
    if (
        isinstance(value, bool)
        or not isinstance(value, (int, float))
        or not math.isfinite(value)
        or value <= 0
        or value > MAX_CHILD_TIMEOUT_SECONDS
    ):
        raise BenchmarkError("child timeout must be finite and in (0, 3600] seconds")


def canonical_timeout_seconds(value: float) -> str:
    validate_child_timeout(value)
    return repr(float(value))


def parse_canonical_timeout_seconds(raw: str) -> float:
    if not isinstance(raw, str) or len(raw) > 64:
        raise BenchmarkError("LFS performance timeout policy is invalid")
    try:
        value = float(raw)
    except ValueError as error:
        raise BenchmarkError("LFS performance timeout policy is invalid") from error
    validate_child_timeout(value)
    if canonical_timeout_seconds(value) != raw:
        raise BenchmarkError("LFS performance timeout policy is not canonical")
    return value


def parse_canonical_decimal_cell(raw: str) -> int:
    if not isinstance(raw, str) or len(raw) > 20 or not raw.isdecimal():
        raise BenchmarkError("LFS performance integer cell is invalid")
    value = int(raw)
    if value > MAX_EVIDENCE_COUNTER or str(value) != raw:
        raise BenchmarkError("LFS performance integer cell is not canonical")
    return value


def validate_sha256(value: object, label: str) -> str:
    if not isinstance(value, str) or len(value) != 64 or value.lower() != value:
        raise BenchmarkError(f"{label} SHA-256 must be 64 lowercase hexadecimal bytes")
    try:
        bytes.fromhex(value)
    except ValueError as error:
        raise BenchmarkError(f"{label} SHA-256 is invalid") from error
    return value


def validate_shareable_evidence_value(value: str, *, allowlisted: bool = False) -> None:
    if not isinstance(value, str) or not value:
        raise BenchmarkError("LFS performance evidence text is invalid")
    try:
        encoded = value.encode("utf-8", "strict")
    except UnicodeError as error:
        raise BenchmarkError("LFS performance evidence text is not UTF-8") from error
    if len(value) > MAX_EVIDENCE_CELL_BYTES or len(encoded) > MAX_EVIDENCE_CELL_BYTES:
        raise BenchmarkError("LFS performance evidence text is too large")
    if any(
        unicodedata.category(character).startswith("C")
        or (
            unicodedata.category(character).startswith("Z")
            and character != " "
        )
        for character in value
    ):
        raise BenchmarkError("LFS performance evidence text contains control bytes")
    if (
        pathlib.PurePosixPath(value).is_absolute()
        or pathlib.PureWindowsPath(value).is_absolute()
        or "://" in value
        or re.search(r"(^|[\s(=:'\"])/(?!/)", value) is not None
        or re.search(r"(^|[\s(=:'\"])[A-Za-z]:[\\/]", value) is not None
        or "\\\\" in value
    ):
        raise BenchmarkError("LFS performance evidence text contains a private location")
    if allowlisted:
        return
    folded = value.casefold()
    usernames = {
        item.casefold()
        for item in (
            pathlib.Path.home().name,
            os.environ.get("USER", ""),
            os.environ.get("USERNAME", ""),
            os.environ.get("LOGNAME", ""),
        )
        if item
    }
    if any(
        re.search(rf"(^|[^a-z0-9]){re.escape(username)}([^a-z0-9]|$)", folded)
        for username in usernames
    ):
        raise BenchmarkError("LFS performance evidence text contains a private user identity")
    if re.search(
        r"(^|[^A-Za-z0-9])(secret|password|token|auth|authorization|cookie|credential|bearer)"
        r"([^A-Za-z0-9]|$)",
        value,
        re.I,
    ):
        raise BenchmarkError("LFS performance evidence text contains secret-bearing text")


def hash_file(path: pathlib.Path, maximum_bytes: int | None = None) -> tuple[str, int]:
    digest = hashlib.sha256()
    total = 0
    with path.open("rb", buffering=0) as source:
        while True:
            chunk = source.read(STREAM_CHUNK_BYTES)
            if not chunk:
                break
            total += len(chunk)
            if maximum_bytes is not None and total > maximum_bytes:
                raise BenchmarkError("bounded file exceeded its maximum size")
            digest.update(chunk)
    return digest.hexdigest(), total


def executable_identity(
    path: pathlib.Path,
    expected_sha256: str,
    version_args: Sequence[str],
    version_prefix: str | None,
) -> ExecutableIdentity:
    expected_sha256 = validate_sha256(expected_sha256, "executable")
    resolved = contract.absolute_file(path, executable=True, reject_symlinks=True)
    actual_sha256, _ = hash_file(resolved)
    if actual_sha256 != expected_sha256:
        raise BenchmarkError("executable SHA-256 does not match the pinned identity")
    result = subprocess.run(
        [str(resolved), *version_args],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
        timeout=15,
    )
    if result.returncode != 0 or len(result.stdout) > 16 * 1024 or len(result.stderr) > 16 * 1024:
        raise BenchmarkError("pinned executable version probe failed")
    try:
        version = result.stdout.decode("utf-8", "strict").strip()
    except UnicodeDecodeError as error:
        raise BenchmarkError("pinned executable version output is not valid UTF-8") from error
    if version_prefix is not None and not version.startswith(version_prefix):
        raise BenchmarkError("pinned executable version does not match the required release")
    return ExecutableIdentity(resolved, actual_sha256, version)


def validate_git_object_id(value: object, label: str) -> str:
    if not isinstance(value, str) or len(value) != 40 or value.lower() != value:
        raise BenchmarkError(f"{label} must be a 40-character lowercase object ID")
    try:
        bytes.fromhex(value)
    except ValueError as error:
        raise BenchmarkError(f"{label} is invalid") from error
    return value


def release_provenance_binding(values: dict[str, str]) -> str:
    if set(values) != set(RELEASE_PROVENANCE_FIELDS):
        raise BenchmarkError("release provenance binding has an invalid schema")
    return contract.sha256_bytes(contract.canonical_json(values))


def release_provenance_values(provenance: ReleaseProvenance) -> dict[str, str]:
    values = {
        "provenance_schema": RELEASE_PROVENANCE_SCHEMA,
        "source_commit": provenance.source_commit,
        "source_tree": provenance.source_tree,
        "source_status_sha256": provenance.source_status_sha256,
        "cargo_lock_sha256": provenance.cargo_lock_sha256,
        "identity_sidecar_sha256": provenance.identity_sidecar_sha256,
        "identity_sidecar_payload_sha256": provenance.identity_sidecar_payload_sha256,
        "identity_sidecar_schema_version": str(provenance.identity_sidecar_schema_version),
        "identity_sidecar_marker": provenance.identity_sidecar_marker,
        "zmin_sha256": provenance.zmin_sha256,
        "git_sha256": provenance.git_sha256,
        "python_sha256": provenance.python_sha256,
        "python_version": provenance.python_version,
    }
    if release_provenance_binding(values) != provenance.provenance_sha256:
        raise BenchmarkError("release provenance binding changed in memory")
    return values


def release_provenance_metadata(provenance: ReleaseProvenance) -> dict[str, str]:
    metadata = release_provenance_values(provenance)
    metadata.update(
        {
            "identity_sidecar_role": "sanitized-release-identity",
            "release_provenance_sha256": provenance.provenance_sha256,
            "release_provenance_verified": "true",
            "build_profile": contract.AUTHORITATIVE_PROFILE,
            "binary_path_profile": contract.AUTHORITATIVE_PROFILE,
        }
    )
    return metadata


def authenticated_python_identity(sidecar: dict[str, object]) -> ExecutableIdentity:
    python_data = sidecar.get("python")
    if not isinstance(python_data, dict):
        raise BenchmarkError("release identity sidecar has no Python identity")
    try:
        current = contract.absolute_file(
            pathlib.Path(sys.executable).resolve(strict=True),
            executable=True,
            reject_symlinks=True,
        )
        recorded = contract.absolute_file(
            pathlib.Path(python_data["path"]),
            executable=True,
            reject_symlinks=True,
        )
        if current != recorded:
            raise BenchmarkError("current Python does not match the release builder")
        facts = contract.binary_facts(current)
    except BenchmarkError:
        raise
    except (KeyError, TypeError, UnicodeError, contract.ContractError, OSError) as error:
        raise BenchmarkError("release Python identity could not be verified") from error
    if any(python_data.get(key) != facts.get(key) for key in ("path", "sha256", "version")):
        raise BenchmarkError("current Python identity changed since the release build")
    return ExecutableIdentity(current, facts["sha256"], facts["version"])


def load_release_provenance(
    repo_root: pathlib.Path,
    zmin: ExecutableIdentity,
    git: ExecutableIdentity,
) -> ReleaseProvenance:
    root = contract.absolute_directory(repo_root, reject_symlinks=True)
    expected_binary = contract.absolute_file(
        contract.canonical_release_binary(root),
        executable=True,
        reject_symlinks=True,
    )
    if zmin.path != expected_binary:
        raise BenchmarkError("Zmin is not the canonical release binary for the source root")
    sidecar_path = contract.identity_sidecar(expected_binary)
    try:
        sidecar_snapshot = contract.read_file_snapshot(
            sidecar_path,
            max_bytes=MAX_RELEASE_SIDECAR_BYTES,
            reject_symlinks=True,
        )
        sidecar = contract.load_json_bytes(
            sidecar_snapshot["data"],
            sidecar_snapshot["path"],
        )
        state = contract.repo_state(root, git.path)
        cargo_lock_snapshot = contract.read_file_snapshot(
            contract.absolute_file(root / "Cargo.lock", reject_symlinks=True),
            max_bytes=MAX_CARGO_LOCK_BYTES,
            reject_symlinks=True,
        )
        source_tree = contract.git_value(git.path, root, "rev-parse", "HEAD^{tree}")
    except (contract.ContractError, OSError, subprocess.SubprocessError) as error:
        raise BenchmarkError("release source provenance could not be read") from error
    if state.get("dirty") is not False:
        raise BenchmarkError("release source repository must be clean")
    source_commit = validate_git_object_id(state.get("commit"), "source commit")
    source_tree = validate_git_object_id(source_tree, "source tree")
    source_status_sha256 = validate_sha256(
        state.get("status_sha256", ""),
        "source status",
    )
    if source_status_sha256 != contract.sha256_bytes(b""):
        raise BenchmarkError("clean release source has a nonempty status identity")
    make_data = sidecar.get("make")
    if not isinstance(make_data, dict):
        raise BenchmarkError("release identity sidecar is incomplete")
    try:
        python = authenticated_python_identity(sidecar)
        make_bin = contract.absolute_file(
            pathlib.Path(make_data["path"]),
            executable=True,
            reject_symlinks=True,
        )
        matched, _detail = contract.sidecar_matches(
            sidecar,
            repo_root=root,
            git_bin=git.path,
            binary=expected_binary,
            profile=contract.AUTHORITATIVE_PROFILE,
            state=state,
            cargo_lock_sha256=cargo_lock_snapshot["sha256"],
            python_bin=python.path,
            make_bin=make_bin,
        )
    except (KeyError, TypeError, UnicodeError, contract.ContractError, OSError) as error:
        raise BenchmarkError("release identity sidecar could not be verified") from error
    if not matched:
        raise BenchmarkError("release identity sidecar does not match source and binary")
    payload_sha256 = validate_sha256(
        sidecar.get("payload_sha256", ""),
        "release sidecar payload",
    )
    schema_version = sidecar.get("schema_version")
    marker = sidecar.get("marker")
    if not isinstance(schema_version, int) or schema_version <= 0:
        raise BenchmarkError("release identity sidecar schema is invalid")
    if marker != contract.RELEASE_BUILD_MARKER:
        raise BenchmarkError("release identity sidecar marker is invalid")
    binding_values = {
        "provenance_schema": RELEASE_PROVENANCE_SCHEMA,
        "source_commit": source_commit,
        "source_tree": source_tree,
        "source_status_sha256": source_status_sha256,
        "cargo_lock_sha256": cargo_lock_snapshot["sha256"],
        "identity_sidecar_sha256": sidecar_snapshot["sha256"],
        "identity_sidecar_payload_sha256": payload_sha256,
        "identity_sidecar_schema_version": str(schema_version),
        "identity_sidecar_marker": marker,
        "zmin_sha256": zmin.sha256,
        "git_sha256": git.sha256,
        "python_sha256": python.sha256,
        "python_version": python.version,
    }
    return ReleaseProvenance(
        repo_root=root,
        source_commit=source_commit,
        source_tree=source_tree,
        source_status_sha256=source_status_sha256,
        cargo_lock_sha256=cargo_lock_snapshot["sha256"],
        identity_sidecar_path=sidecar_snapshot["path"],
        identity_sidecar_sha256=sidecar_snapshot["sha256"],
        identity_sidecar_payload_sha256=payload_sha256,
        identity_sidecar_schema_version=schema_version,
        identity_sidecar_marker=marker,
        zmin_sha256=zmin.sha256,
        git_sha256=git.sha256,
        python_sha256=python.sha256,
        python_version=python.version,
        provenance_sha256=release_provenance_binding(binding_values),
    )


def verify_release_provenance_unchanged(
    expected: ReleaseProvenance,
    zmin: ExecutableIdentity,
    git: ExecutableIdentity,
) -> None:
    if load_release_provenance(expected.repo_root, zmin, git) != expected:
        raise BenchmarkError("release source provenance changed during the benchmark")


def verify_release_inputs_unchanged(
    expected: ReleaseProvenance,
    zmin: ExecutableIdentity,
    git: ExecutableIdentity,
    git_lfs: ExecutableIdentity,
) -> None:
    verify_release_provenance_unchanged(expected, zmin, git)
    verify_identity_unchanged(zmin)
    verify_identity_unchanged(git)
    verify_identity_unchanged(git_lfs)


def validate_output_location(
    output_root: pathlib.Path,
    source_root: pathlib.Path,
) -> None:
    try:
        output_root.relative_to(source_root)
    except ValueError:
        return
    raise BenchmarkError("output directory must be outside the release source repository")


def parse_pinned_git_lfs(manifest_path: pathlib.Path) -> ExecutableIdentity:
    manifest_path = contract.absolute_file(manifest_path, reject_symlinks=True)
    with manifest_path.open("rb", buffering=0) as source:
        manifest_bytes = source.read(MAX_MANIFEST_BYTES + 1)
    if not manifest_bytes or len(manifest_bytes) > MAX_MANIFEST_BYTES:
        raise BenchmarkError("Git LFS manifest is empty or too large")
    try:
        manifest_text = manifest_bytes.decode("utf-8", "strict")
    except UnicodeDecodeError as error:
        raise BenchmarkError("Git LFS manifest is not UTF-8") from error
    values: dict[str, str] = {}
    for line in manifest_text.splitlines():
        if not line or "=" not in line:
            raise BenchmarkError("Git LFS manifest contains a malformed line")
        key, value = line.split("=", 1)
        if not key or key in values or not value:
            raise BenchmarkError("Git LFS manifest contains an invalid or duplicate field")
        values[key] = value
    required = ("artifact", "release_version", "platform", "binary_sha256", "binary_version")
    if any(key not in values for key in required):
        raise BenchmarkError("Git LFS manifest is missing a required field")
    if values["release_version"] != "3.7.1":
        raise BenchmarkError("Git LFS comparator must be exactly v3.7.1")
    artifact = pathlib.PurePath(values["artifact"])
    if artifact.name != values["artifact"] or artifact.name in {"", ".", ".."}:
        raise BenchmarkError("Git LFS manifest artifact must be a basename")
    expected_platform = host_manifest_platform()
    if values["platform"] != expected_platform:
        raise BenchmarkError("Git LFS manifest platform does not match this host")
    binary = manifest_path.parent / values["artifact"]
    identity = executable_identity(
        binary,
        values["binary_sha256"],
        ("--version",),
        "git-lfs/3.7.1 ",
    )
    if identity.version != values["binary_version"]:
        raise BenchmarkError("Git LFS binary version does not match its pinned manifest")
    return identity


def host_manifest_platform() -> str:
    system = platform.system().lower()
    machine = platform.machine().lower()
    machine_names = {
        "amd64": "amd64",
        "x86_64": "amd64",
        "aarch64": "arm64",
        "arm64": "arm64",
    }
    architecture = machine_names.get(machine)
    if system not in {"darwin", "linux", "windows"} or architecture is None:
        raise BenchmarkError("this host has no pinned Git LFS platform contract")
    return f"{system}-{architecture}"


def create_fixture_objects(
    root: pathlib.Path,
    count: int,
    object_bytes: int,
) -> tuple[LfsObject, ...]:
    root.mkdir(parents=True, exist_ok=False)
    objects: list[LfsObject] = []
    for index in range(count):
        path = root / f"object-{index:02d}.bin"
        digest = hashlib.sha256()
        remaining = object_bytes
        sequence = 0
        with path.open("xb", buffering=0) as output:
            while remaining:
                amount = min(STREAM_CHUNK_BYTES, remaining)
                seed = f"zmin-lfs-performance-v1:{index}:{sequence}".encode("ascii")
                chunk = hashlib.shake_256(seed).digest(amount)
                output.write(chunk)
                digest.update(chunk)
                remaining -= amount
                sequence += 1
            output.flush()
            os.fsync(output.fileno())
        objects.append(LfsObject(index, digest.hexdigest(), object_bytes, path))
    return tuple(objects)


def bounded_process_output(result: subprocess.CompletedProcess[bytes], label: str) -> None:
    if len(result.stdout) > MAX_CAPTURE_BYTES or len(result.stderr) > MAX_CAPTURE_BYTES:
        raise BenchmarkError(f"{label} emitted excessive setup output")
    if result.returncode != 0:
        raise BenchmarkError(f"{label} failed with exit {result.returncode}")


def run_git(git: ExecutableIdentity, environment: dict[str, str], *args: str) -> bytes:
    result = subprocess.run(
        [str(git.path), *args],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env=environment,
        check=False,
        timeout=30,
    )
    bounded_process_output(result, "Git fixture command")
    return result.stdout


def create_baseline_repository(
    root: pathlib.Path,
    objects: Sequence[LfsObject],
    git: ExecutableIdentity,
    environment: dict[str, str],
) -> pathlib.Path:
    repository = root / "baseline"
    repository.mkdir()
    run_git(git, environment, "-C", str(repository), "init", "--quiet", "--initial-branch=main")
    run_git(git, environment, "-C", str(repository), "config", "user.name", "LFS Benchmark")
    run_git(
        git,
        environment,
        "-C",
        str(repository),
        "config",
        "user.email",
        "lfs-benchmark@example.invalid",
    )
    run_git(git, environment, "-C", str(repository), "config", "commit.gpgsign", "false")
    run_git(git, environment, "-C", str(repository), "config", "core.autocrlf", "false")
    pointer_names = []
    for item in objects:
        name = f"asset-{item.index:02d}.bin"
        pointer_names.append(name)
        pointer = (
            "version https://git-lfs.github.com/spec/v1\n"
            f"oid sha256:{item.oid}\n"
            f"size {item.size}\n"
        )
        (repository / name).write_text(pointer, encoding="utf-8", newline="\n")
    run_git(git, environment, "-C", str(repository), "add", "--", *pointer_names)
    (repository / ".gitattributes").write_text(
        "*.bin filter=lfs diff=lfs merge=lfs -text\n",
        encoding="utf-8",
        newline="\n",
    )
    run_git(git, environment, "-C", str(repository), "add", "--", ".gitattributes")
    run_git(git, environment, "-C", str(repository), "commit", "--quiet", "-m", "LFS pointers")
    run_git(
        git,
        environment,
        "-C",
        str(repository),
        "remote",
        "add",
        "origin",
        "http://127.0.0.1/benchmark/repo.git",
    )
    return repository


def lfs_store_path(repository: pathlib.Path, oid: str) -> pathlib.Path:
    return repository / ".git" / "lfs" / "objects" / oid[:2] / oid[2:4] / oid


def install_upload_objects(repository: pathlib.Path, objects: Sequence[LfsObject]) -> None:
    for item in objects:
        destination = lfs_store_path(repository, item.oid)
        destination.parent.mkdir(parents=True, exist_ok=True)
        os.link(item.path, destination)


def configure_run_repository(
    repository: pathlib.Path,
    endpoint: str,
    concurrency: int,
    git: ExecutableIdentity,
    environment: dict[str, str],
) -> None:
    run_git(git, environment, "-C", str(repository), "config", "lfs.url", endpoint)
    run_git(
        git,
        environment,
        "-C",
        str(repository),
        "config",
        "lfs.concurrenttransfers",
        str(concurrency),
    )
    run_git(
        git,
        environment,
        "-C",
        str(repository),
        "config",
        f"lfs.{endpoint}.access",
        "none",
    )
    run_git(
        git,
        environment,
        "-C",
        str(repository),
        "config",
        f"lfs.{endpoint}.locksverify",
        "false",
    )


def benchmark_environment(
    temporary_root: pathlib.Path,
    git: ExecutableIdentity,
    git_lfs: ExecutableIdentity,
) -> dict[str, str]:
    home = temporary_root / "home"
    home.mkdir(exist_ok=True)
    global_config = temporary_root / "global.gitconfig"
    global_config.write_bytes(b"")
    path_parts = [str(git_lfs.path.parent), str(git.path.parent), str(pathlib.Path(sys.executable).parent)]
    inherited_path = os.environ.get("PATH", "")
    for item in os.pathsep.join(path_parts).split(os.pathsep) + inherited_path.split(os.pathsep):
        if item and item not in path_parts:
            path_parts.append(item)
    environment = {
        "PATH": os.pathsep.join(path_parts),
        "HOME": str(home),
        "GIT_CONFIG_GLOBAL": str(global_config),
        "GIT_CONFIG_NOSYSTEM": "1",
        "GIT_TERMINAL_PROMPT": "0",
        "GIT_OPTIONAL_LOCKS": "0",
        "LANG": "C",
        "LC_ALL": "C",
        "TZ": "UTC",
        "NO_PROXY": "127.0.0.1,localhost",
        "no_proxy": "127.0.0.1,localhost",
        "PYTHONDONTWRITEBYTECODE": "1",
    }
    for key in ("SYSTEMROOT", "WINDIR", "COMSPEC", "PATHEXT", "TEMP", "TMP"):
        value = os.environ.get(key)
        if value:
            environment[key] = value
    resolved_git = shutil.which("git", path=environment["PATH"])
    if resolved_git is None or pathlib.Path(resolved_git).resolve() != git.path:
        raise BenchmarkError("sanitized PATH does not resolve the pinned Git executable")
    return environment


def parse_metrics(path: pathlib.Path) -> ProcessMetrics:
    raw = path.read_bytes()
    if len(raw) > 16 * 1024:
        raise BenchmarkError("process metrics output is too large")
    fields = raw.decode("ascii", "strict").rstrip("\n").split("\t")
    if len(fields) != 14:
        raise BenchmarkError("process metrics output has an invalid schema")
    try:
        wall_seconds = float(fields[0])
    except ValueError as error:
        raise BenchmarkError("process wall time is invalid") from error
    if not math.isfinite(wall_seconds) or wall_seconds <= 0:
        raise BenchmarkError("process wall time must be finite and positive")
    metrics = ProcessMetrics(wall_seconds, *fields[1:])
    if metrics.memory_unit != "bytes":
        raise BenchmarkError("process memory metric is not in bytes")
    metrics.memory_bytes()
    return metrics


def bounded_log_hash(path: pathlib.Path) -> str:
    digest, size = hash_file(path, MAX_CAPTURE_BYTES)
    if size > MAX_CAPTURE_BYTES:
        raise BenchmarkError("child output exceeded the bounded log policy")
    return digest


def measured_command(
    runner: pathlib.Path,
    metrics_root: pathlib.Path,
    metrics_identity: str,
    token: str,
    command: Sequence[str],
    repository: pathlib.Path,
    environment: dict[str, str],
    timeout_seconds: float,
) -> tuple[int, ProcessMetrics, str, str]:
    stdout_path = metrics_root / f"{token}.stdout"
    stderr_path = metrics_root / f"{token}.stderr"
    metrics_path = metrics_root / f"{token}.metrics"
    invocation = [
        sys.executable,
        "-B",
        str(runner),
        "--artifact-root",
        str(metrics_root),
        "--artifact-root-identity",
        metrics_identity,
        "--stdout",
        str(stdout_path),
        "--stderr",
        str(stderr_path),
        "--metrics",
        str(metrics_path),
        "--timeout-seconds",
        f"{timeout_seconds:.3f}",
        "--max-output-bytes",
        str(MAX_CAPTURE_BYTES),
        "--",
        *command,
    ]
    child_environment = dict(environment)
    child_environment["PYTHONDONTWRITEBYTECODE"] = "1"
    result = subprocess.run(
        invocation,
        cwd=repository,
        env=child_environment,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
        timeout=timeout_seconds + 15,
    )
    if len(result.stdout) > MAX_CAPTURE_BYTES or len(result.stderr) > MAX_CAPTURE_BYTES:
        raise BenchmarkError("benchmark process runner emitted excessive diagnostics")
    if result.returncode != 0:
        diagnostic = result.stderr.decode("utf-8", "replace").splitlines()
        detail = diagnostic[0] if diagnostic else "no diagnostic"
        raise BenchmarkError(f"benchmark process runner failed for {token}: {detail}")
    metrics = parse_metrics(metrics_path)
    return (
        result.returncode,
        metrics,
        bounded_log_hash(stdout_path),
        bounded_log_hash(stderr_path),
    )


def verify_download_store(repository: pathlib.Path, objects: Sequence[LfsObject]) -> None:
    expected_paths = set()
    for item in objects:
        path = lfs_store_path(repository, item.oid)
        expected_paths.add(path.resolve())
        digest, size = hash_file(path)
        if digest != item.oid or size != item.size:
            raise BenchmarkError("downloaded LFS object failed streaming verification")
    object_root = repository / ".git" / "lfs" / "objects"
    actual_paths = {
        path.resolve()
        for path in object_root.rglob("*")
        if path.is_file()
    }
    if actual_paths != expected_paths:
        raise BenchmarkError("download store contains missing or unexpected object files")


def validate_run_evidence(
    evidence: RunEvidence,
    specification: RunSpecification,
) -> None:
    expected_oids = tuple(sorted(item.oid for item in specification.objects))
    expected_bytes = sum(item.size for item in specification.objects)
    if evidence.errors:
        raise BenchmarkError(f"fixture reported an error: {evidence.errors[0]}")
    if evidence.batch_requests != 1:
        raise BenchmarkError("run did not issue exactly one Batch request")
    if evidence.transfer_requests != len(specification.objects):
        raise BenchmarkError("run did not issue exactly one transfer per object")
    if evidence.max_active != specification.concurrency:
        raise BenchmarkError(
            f"{specification.token} reached {evidence.max_active} active transfers; "
            f"expected {specification.concurrency}"
        )
    if evidence.residual_active != 0:
        raise BenchmarkError("run left an active transfer behind")
    if evidence.completed_oids != expected_oids or evidence.completed_bytes != expected_bytes:
        raise BenchmarkError("run did not complete every requested object exactly once")


def tool_command(
    tool: BenchmarkTool,
    operation: BenchmarkOperation,
    zmin: ExecutableIdentity,
    git_lfs: ExecutableIdentity,
) -> list[str]:
    action = "fetch" if operation == BenchmarkOperation.DOWNLOAD else "push"
    if tool == BenchmarkTool.ZMIN:
        return [str(zmin.path), "lfs", action, "origin", "HEAD"]
    return [str(git_lfs.path), action, "origin", "HEAD"]


def execute_sample(
    *,
    temporary_root: pathlib.Path,
    baseline: pathlib.Path,
    objects: tuple[LfsObject, ...],
    fixture: FixtureServer,
    runner: pathlib.Path,
    metrics_root: pathlib.Path,
    metrics_identity: str,
    environment: dict[str, str],
    git: ExecutableIdentity,
    git_lfs: ExecutableIdentity,
    zmin: ExecutableIdentity,
    operation: BenchmarkOperation,
    concurrency: int,
    sample_kind: SampleKind,
    sample_index: int,
    pair_id: str,
    order_index: int,
    tool: BenchmarkTool,
    timeout_seconds: float,
) -> BenchmarkRow:
    token = f"{pair_id}-{order_index}-{tool.value}"
    repository = temporary_root / "runs" / token
    shutil.copytree(baseline, repository, symlinks=True)
    if operation == BenchmarkOperation.UPLOAD:
        install_upload_objects(repository, objects)
    specification = RunSpecification(token, operation, concurrency, objects)
    state = fixture.server.register(specification)
    endpoint = fixture.server.endpoint(token)
    sample_complete = False
    try:
        configure_run_repository(repository, endpoint, concurrency, git, environment)
        command = tool_command(tool, operation, zmin, git_lfs)
        exit_code, metrics, stdout_sha256, stderr_sha256 = measured_command(
            runner,
            metrics_root,
            metrics_identity,
            token,
            command,
            repository,
            environment,
            timeout_seconds,
        )
        evidence = state.wait_for_completion(SERVER_COMPLETION_TIMEOUT_SECONDS)
        if exit_code != 0:
            raise BenchmarkError(f"{tool.value} {operation.value} exited with {exit_code}")
        validate_run_evidence(evidence, specification)
        if operation == BenchmarkOperation.DOWNLOAD:
            verify_download_store(repository, objects)
        payload_bytes = sum(item.size for item in objects)
        row = BenchmarkRow(
            tool=tool.value,
            operation=operation.value,
            concurrency=concurrency,
            sample_kind=sample_kind.value,
            pair_id=pair_id,
            order_index=order_index,
            sample_index=sample_index,
            object_count=len(objects),
            object_bytes=objects[0].size,
            payload_bytes=payload_bytes,
            wall_seconds=metrics.wall_seconds,
            throughput_mib_per_second=(payload_bytes / (1024 * 1024)) / metrics.wall_seconds,
            user_seconds=metrics.user_seconds,
            system_seconds=metrics.system_seconds,
            peak_rss_bytes=metrics.peak_rss_bytes,
            peak_job_commit_bytes=metrics.peak_job_commit_bytes,
            memory_metric=metrics.memory_metric,
            memory_semantics=metrics.memory_semantics,
            memory_scope=metrics.memory_scope,
            exit_code=exit_code,
            batch_requests=evidence.batch_requests,
            transfer_requests=evidence.transfer_requests,
            max_active=evidence.max_active,
            completed_objects=len(evidence.completed_oids),
            completed_bytes=evidence.completed_bytes,
            residual_active=evidence.residual_active,
            host_scheduler_lag_max_seconds=0.0,
            host_system_cpu_busy_ratio="pending",
            host_load_average_1m="pending",
            host_memory_pressure="pending",
            host_thermal_state="pending",
            host_monitor_samples=0,
            stdout_sha256=stdout_sha256,
            stderr_sha256=stderr_sha256,
        )
        sample_complete = True
        return row
    finally:
        # On failure, leave the state and repository owned by the enclosing
        # server and TemporaryDirectory contexts: the server joins all handler
        # threads before the directory is removed.  Successful runs are known
        # quiescent from validate_run_evidence and can be reclaimed eagerly.
        if sample_complete:
            fixture.server.unregister(token)
            shutil.rmtree(repository)


def sample_order(lane_index: int, sample_index: int) -> tuple[BenchmarkTool, BenchmarkTool]:
    if (lane_index + sample_index) % 2 == 0:
        return (BenchmarkTool.STOCK, BenchmarkTool.ZMIN)
    return (BenchmarkTool.ZMIN, BenchmarkTool.STOCK)


def canonical_lanes(policy: EvidencePolicy | BenchmarkPolicy) -> tuple[BenchmarkLane, ...]:
    return tuple(
        BenchmarkLane(operation=operation, concurrency=concurrency)
        for operation in BenchmarkOperation
        for concurrency in policy.concurrencies
    )


def schedule_binding_from_metadata(metadata: dict[str, str]) -> str:
    try:
        values = {key: metadata[key] for key in SCHEDULE_BINDING_FIELDS}
    except KeyError as error:
        raise BenchmarkError("benchmark schedule binding input is incomplete") from error
    return contract.sha256_bytes(contract.canonical_json(values))


def schedule_binding_for_run(
    policy: BenchmarkPolicy,
    provenance: ReleaseProvenance,
    zmin: ExecutableIdentity,
    git: ExecutableIdentity,
    git_lfs: ExecutableIdentity,
) -> str:
    return schedule_binding_from_metadata(
        {
            "schema_name": LFS_PERFORMANCE_SCHEMA_NAME,
            "schema_version": LFS_PERFORMANCE_SCHEMA_VERSION,
            "source_commit": provenance.source_commit,
            "source_tree": provenance.source_tree,
            "cargo_lock_sha256": provenance.cargo_lock_sha256,
            "identity_sidecar_sha256": provenance.identity_sidecar_sha256,
            "identity_sidecar_payload_sha256": (
                provenance.identity_sidecar_payload_sha256
            ),
            "zmin_sha256": zmin.sha256,
            "git_sha256": git.sha256,
            "git_lfs_sha256": git_lfs.sha256,
            "python_sha256": provenance.python_sha256,
            "fixture_chunk_bytes": str(STREAM_CHUNK_BYTES),
            "fixture_object_count": str(policy.object_count),
            "fixture_object_bytes": str(policy.object_bytes),
            "concurrencies": ",".join(
                str(value) for value in policy.concurrencies
            ),
            "warmups": str(policy.warmups),
            "repeats": str(policy.repeats),
            "timeout_seconds": canonical_timeout_seconds(
                policy.child_timeout_seconds
            ),
        }
    )


def scheduled_lanes(
    policy: EvidencePolicy | BenchmarkPolicy,
    schedule_binding: str,
    sample_kind: SampleKind,
    sample_index: int,
) -> tuple[BenchmarkLane, ...]:
    validate_sha256(schedule_binding, "benchmark schedule binding")
    if sample_index <= 0:
        raise BenchmarkError("benchmark schedule round is invalid")
    prefix = (
        f"{schedule_binding}\0{sample_kind.value}\0{sample_index}\0".encode("ascii")
    )
    return tuple(
        sorted(
            canonical_lanes(policy),
            key=lambda lane: hashlib.sha256(
                prefix + f"{lane.operation.value}\0{lane.concurrency}".encode("ascii")
            ).digest(),
        )
    )


def nearest_rank_p95(values: Sequence[float]) -> float:
    if not values:
        raise BenchmarkError("cannot calculate p95 of an empty sample")
    ordered = sorted(values)
    return ordered[math.ceil(0.95 * len(ordered)) - 1]


def bind_host_interval(
    row: BenchmarkRow,
    interval: host_noise.HostIntervalEvidence,
) -> BenchmarkRow:
    return replace(
        row,
        host_scheduler_lag_max_seconds=interval.scheduler_lag_max_seconds,
        host_system_cpu_busy_ratio=interval.system_cpu_busy_ratio,
        host_load_average_1m=interval.load_average_1m,
        host_memory_pressure=interval.memory_pressure,
        host_thermal_state=interval.thermal_state,
        host_monitor_samples=interval.monitor_samples,
    )


def paired_median_upper_95(values: Sequence[float]) -> float | None:
    if not values:
        raise BenchmarkError("cannot calculate paired confidence bound of an empty sample")
    ordered = sorted(values)
    count = len(ordered)
    for rank in range(1, count + 1):
        upper_tail = sum(
            math.comb(count, successes) for successes in range(rank, count + 1)
        ) / (2**count)
        if upper_tail <= 0.05:
            return ordered[rank - 1]
    return None


def row_memory_bytes(row: BenchmarkRow) -> int:
    value = (
        row.peak_rss_bytes
        if row.memory_metric == "peak_rss_bytes"
        else row.peak_job_commit_bytes
        if row.memory_metric == "peak_job_commit_bytes"
        else "unsupported"
    )
    if not value.isdecimal() or int(value) <= 0:
        raise BenchmarkError("required row peak-memory metric is unavailable")
    return int(value)


def summarize(
    rows: Sequence[BenchmarkRow],
    repeats: int,
    *,
    evaluate_strict: bool = False,
) -> tuple[list[SummaryRow], list[ComparisonRow]]:
    summaries: list[SummaryRow] = []
    comparisons: list[ComparisonRow] = []
    lanes = sorted({(row.operation, row.concurrency) for row in rows})
    for operation, concurrency in lanes:
        lane_summaries: dict[str, SummaryRow] = {}
        for tool in (BenchmarkTool.STOCK.value, BenchmarkTool.ZMIN.value):
            samples = [
                row
                for row in rows
                if row.sample_kind == SampleKind.MEASURED.value
                and row.operation == operation
                and row.concurrency == concurrency
                and row.tool == tool
            ]
            if len(samples) != repeats:
                raise BenchmarkError("lane has a missing or duplicate measured sample")
            metrics = {row.memory_metric for row in samples}
            if len(metrics) != 1:
                raise BenchmarkError("lane changed platform memory metric between samples")
            summary = SummaryRow(
                operation=operation,
                concurrency=concurrency,
                tool=tool,
                samples=len(samples),
                median_seconds=statistics.median(row.wall_seconds for row in samples),
                p95_seconds=nearest_rank_p95([row.wall_seconds for row in samples]),
                median_mib_per_second=statistics.median(
                    row.throughput_mib_per_second for row in samples
                ),
                maximum_peak_memory_bytes=max(row_memory_bytes(row) for row in samples),
                memory_metric=next(iter(metrics)),
            )
            summaries.append(summary)
            lane_summaries[tool] = summary
        stock = lane_summaries[BenchmarkTool.STOCK.value]
        zmin = lane_summaries[BenchmarkTool.ZMIN.value]
        if stock.memory_metric != zmin.memory_metric:
            raise BenchmarkError("comparators use non-equivalent platform memory metrics")
        paired_rows: dict[str, dict[str, BenchmarkRow]] = {}
        for row in rows:
            if (
                row.sample_kind == SampleKind.MEASURED.value
                and row.operation == operation
                and row.concurrency == concurrency
            ):
                pair = paired_rows.setdefault(row.pair_id, {})
                if row.tool in pair:
                    raise BenchmarkError("lane has a duplicate paired sample")
                pair[row.tool] = row
        if len(paired_rows) != repeats or any(
            set(pair) != {BenchmarkTool.STOCK.value, BenchmarkTool.ZMIN.value}
            for pair in paired_rows.values()
        ):
            raise BenchmarkError("lane has an incomplete paired sample")
        ratios: list[float] = []
        stock_first_ratios: list[float] = []
        zmin_first_ratios: list[float] = []
        for pair_id in sorted(paired_rows):
            pair = paired_rows[pair_id]
            stock_row = pair[BenchmarkTool.STOCK.value]
            zmin_row = pair[BenchmarkTool.ZMIN.value]
            ratio = zmin_row.wall_seconds / stock_row.wall_seconds
            ratios.append(ratio)
            if stock_row.order_index == 1 and zmin_row.order_index == 2:
                stock_first_ratios.append(ratio)
            elif zmin_row.order_index == 1 and stock_row.order_index == 2:
                zmin_first_ratios.append(ratio)
            else:
                raise BenchmarkError("lane paired order is invalid")
        paired_upper = paired_median_upper_95(ratios)
        paired_upper_text = (
            "unbounded" if paired_upper is None else repr(float(paired_upper))
        )
        paired_median = statistics.median(ratios)
        if evaluate_strict and (not stock_first_ratios or not zmin_first_ratios):
            raise BenchmarkError("strict lane lacks both paired tool orders")
        stock_first_median = statistics.median(stock_first_ratios or ratios)
        zmin_first_median = statistics.median(zmin_first_ratios or ratios)
        verdict = (
            "pass"
            if evaluate_strict
            and zmin.median_seconds < stock.median_seconds
            and zmin.p95_seconds < stock.p95_seconds
            and zmin.maximum_peak_memory_bytes < stock.maximum_peak_memory_bytes
            and paired_upper is not None
            and paired_upper < 1.0
            and stock_first_median < 1.0
            and zmin_first_median < 1.0
            else "fail" if evaluate_strict else "not-evaluated"
        )
        comparisons.append(
            ComparisonRow(
                operation=operation,
                concurrency=concurrency,
                median_wall_ratio=zmin.median_seconds / stock.median_seconds,
                p95_wall_ratio=zmin.p95_seconds / stock.p95_seconds,
                peak_memory_ratio=(
                    zmin.maximum_peak_memory_bytes / stock.maximum_peak_memory_bytes
                ),
                paired_median_wall_ratio=paired_median,
                paired_median_upper_95_ratio=paired_upper_text,
                stock_first_paired_median_ratio=stock_first_median,
                zmin_first_paired_median_ratio=zmin_first_median,
                verdict=verdict,
            )
        )
    return summaries, comparisons


def benchmark_claim(
    policy: BenchmarkPolicy,
    comparisons: Sequence[ComparisonRow],
) -> BenchmarkClaim:
    if not policy.strict_gate:
        return BenchmarkClaim.FUNCTIONAL_SMOKE
    if comparisons and all(row.verdict == "pass" for row in comparisons):
        return BenchmarkClaim.STRICT_SUPERIORITY
    return BenchmarkClaim.STRICT_NOT_ESTABLISHED


def tsv_bytes(fieldnames: Sequence[str], rows: Iterable[dict[str, object]]) -> bytes:
    output = io.StringIO(newline="")
    writer = csv.DictWriter(
        output,
        fieldnames=fieldnames,
        delimiter="\t",
        lineterminator="\n",
        extrasaction="raise",
    )
    writer.writeheader()
    writer.writerows(rows)
    return output.getvalue().encode("utf-8")


def row_dict(row: BenchmarkRow) -> dict[str, object]:
    values = dict(row.__dict__)
    values["wall_seconds"] = f"{row.wall_seconds:.9f}"
    values["throughput_mib_per_second"] = f"{row.throughput_mib_per_second:.6f}"
    values["host_scheduler_lag_max_seconds"] = (
        f"{row.host_scheduler_lag_max_seconds:.9f}"
    )
    return values


def canonicalize_benchmark_rows(rows: Sequence[BenchmarkRow]) -> list[BenchmarkRow]:
    fields = tuple(BenchmarkRow.__dataclass_fields__)
    data = tsv_bytes(fields, (row_dict(row) for row in rows))
    parsed = read_evidence_tsv_bytes(
        data,
        pathlib.Path("rows.tsv"),
        expected_fields=fields,
    )
    return parse_benchmark_rows(parsed)


def summary_dict(row: SummaryRow) -> dict[str, object]:
    values = dict(row.__dict__)
    values["median_seconds"] = f"{row.median_seconds:.9f}"
    values["p95_seconds"] = f"{row.p95_seconds:.9f}"
    values["median_mib_per_second"] = f"{row.median_mib_per_second:.6f}"
    return values


def comparison_dict(row: ComparisonRow) -> dict[str, object]:
    values = dict(row.__dict__)
    values["median_wall_ratio"] = repr(float(row.median_wall_ratio))
    values["p95_wall_ratio"] = repr(float(row.p95_wall_ratio))
    values["peak_memory_ratio"] = repr(float(row.peak_memory_ratio))
    values["paired_median_wall_ratio"] = repr(float(row.paired_median_wall_ratio))
    values["stock_first_paired_median_ratio"] = repr(
        float(row.stock_first_paired_median_ratio)
    )
    values["zmin_first_paired_median_ratio"] = repr(
        float(row.zmin_first_paired_median_ratio)
    )
    return values


def validate_lfs_performance_metadata(metadata: dict[str, str]) -> EvidencePolicy:
    if not isinstance(metadata, dict):
        raise BenchmarkError("LFS performance metadata has an invalid schema")
    for key, value in metadata.items():
        validate_shareable_evidence_value(
            key,
            allowlisted=key in LFS_PERFORMANCE_METADATA_FIELDS,
        )
        allowed_values = METADATA_FIXED_VALUE_ALLOWLIST.get(key, frozenset())
        validate_shareable_evidence_value(
            value,
            allowlisted=value in allowed_values,
        )
    if set(metadata) != set(LFS_PERFORMANCE_METADATA_FIELDS):
        raise BenchmarkError("LFS performance metadata has an invalid schema")
    for key, allowed_values in METADATA_FIXED_VALUE_ALLOWLIST.items():
        if metadata[key] not in allowed_values:
            raise BenchmarkError("LFS performance fixed metadata value is invalid")
    if (
        metadata["schema_name"] != LFS_PERFORMANCE_SCHEMA_NAME
        or metadata["schema_version"] != LFS_PERFORMANCE_SCHEMA_VERSION
    ):
        raise BenchmarkError("LFS performance metadata schema version is invalid")
    if (
        metadata["release_provenance_verified"] != "true"
        or metadata["build_profile"] != contract.AUTHORITATIVE_PROFILE
        or metadata["binary_path_profile"] != contract.AUTHORITATIVE_PROFILE
    ):
        raise BenchmarkError("LFS performance release provenance is not authoritative")
    if metadata["claim"] not in {claim.value for claim in BenchmarkClaim}:
        raise BenchmarkError("LFS performance claim is invalid")
    if metadata["cross_platform_memory_comparison"] != "forbidden":
        raise BenchmarkError("LFS performance cross-platform claim is invalid")
    if metadata["ordering"] != "sha256-block-rounds-adjacent-balanced":
        raise BenchmarkError("LFS performance ordering is invalid")
    if metadata["host_monitor_adapter"] not in METADATA_FIXED_VALUE_ALLOWLIST[
        "host_monitor_adapter"
    ]:
        raise BenchmarkError("LFS performance host monitor is invalid")
    if metadata["host_monitor_interval_seconds"] != (
        f"{host_noise.SAMPLE_INTERVAL_SECONDS:.3f}"
    ):
        raise BenchmarkError("LFS performance host monitor interval is invalid")
    if metadata["host_noise_cpu_policy"] != "diagnostic-only-no-numeric-threshold":
        raise BenchmarkError("LFS performance host CPU policy is invalid")
    if metadata["host_noise_pressure_policy"] != "normal-only":
        raise BenchmarkError("LFS performance host pressure policy is invalid")
    if metadata["memory_metric"] not in MEMORY_EVIDENCE_CONTRACTS:
        raise BenchmarkError("LFS performance memory contract is invalid")
    if metadata["git_lfs_role"] != "pinned-git-lfs":
        raise BenchmarkError("LFS performance Git LFS role is invalid")
    if metadata["git_role"] != "source-inspector-git":
        raise BenchmarkError("LFS performance Git role is invalid")
    if metadata["python_role"] != "release-builder-python":
        raise BenchmarkError("LFS performance Python role is invalid")
    if metadata["zmin_role"] != "canonical-release-zmin":
        raise BenchmarkError("LFS performance Zmin role is invalid")
    if metadata["identity_sidecar_role"] != "sanitized-release-identity":
        raise BenchmarkError("LFS performance sidecar role is invalid")
    validate_git_object_id(metadata["source_commit"], "source commit")
    validate_git_object_id(metadata["source_tree"], "source tree")
    for key in (
        "source_status_sha256",
        "cargo_lock_sha256",
        "identity_sidecar_sha256",
        "identity_sidecar_payload_sha256",
        "zmin_sha256",
        "git_sha256",
        "git_lfs_sha256",
        "python_sha256",
        "release_provenance_sha256",
        "schedule_binding_sha256",
    ):
        validate_sha256(metadata[key], key.replace("_", " "))
    if metadata["source_status_sha256"] != contract.sha256_bytes(b""):
        raise BenchmarkError("LFS performance source status is not clean")
    if metadata["provenance_schema"] != RELEASE_PROVENANCE_SCHEMA:
        raise BenchmarkError("LFS performance provenance schema is invalid")
    if metadata["identity_sidecar_marker"] != contract.RELEASE_BUILD_MARKER:
        raise BenchmarkError("LFS performance sidecar marker is invalid")
    if metadata["identity_sidecar_schema_version"] != str(contract.SCHEMA_VERSION + 1):
        raise BenchmarkError("LFS performance sidecar schema version is invalid")
    if parse_decimal_policy(metadata["fixture_chunk_bytes"]) != STREAM_CHUNK_BYTES:
        raise BenchmarkError("LFS performance fixture chunk policy is invalid")
    policy = EvidencePolicy.from_metadata(metadata)
    if schedule_binding_from_metadata(metadata) != metadata["schedule_binding_sha256"]:
        raise BenchmarkError("LFS performance schedule binding is invalid")
    if not metadata["git_lfs_version"].startswith("git-lfs/3.7.1"):
        raise BenchmarkError("LFS performance Git LFS version is not pinned")
    if not metadata["python_version"].startswith("Python "):
        raise BenchmarkError("LFS performance Python version is invalid")
    binding_values = {key: metadata[key] for key in RELEASE_PROVENANCE_FIELDS}
    if release_provenance_binding(binding_values) != metadata["release_provenance_sha256"]:
        raise BenchmarkError("LFS performance provenance digest does not match metadata")
    return policy


def evidence_artifact(
    name: str,
    columns: Sequence[str],
    rows: Iterable[dict[str, object]],
) -> EvidenceArtifact:
    data = tsv_bytes(columns, rows)
    parsed = read_evidence_tsv_bytes(
        data,
        pathlib.Path(name),
        expected_fields=tuple(columns),
    )
    return EvidenceArtifact(
        name=name,
        schema=EVIDENCE_ARTIFACT_SCHEMAS[name],
        columns=tuple(columns),
        records=len(parsed),
        data=data,
    )


def read_evidence_tsv_bytes(
    data: bytes,
    path: pathlib.Path,
    *,
    expected_fields: tuple[str, ...],
) -> list[dict[str, str]]:
    try:
        text = data.decode("utf-8", "strict")
    except UnicodeError as error:
        raise BenchmarkError("LFS performance TSV input is malformed") from error
    if any(
        character not in "\t\r\n "
        and (
            unicodedata.category(character).startswith("C")
            or unicodedata.category(character).startswith("Z")
        )
        for character in text
    ):
        raise BenchmarkError("LFS performance TSV input contains control bytes")
    try:
        return contract.read_strict_tsv_bytes(
            data,
            path,
            expected_fields=expected_fields,
        )
    except (csv.Error, UnicodeError, contract.ContractError) as error:
        raise BenchmarkError("LFS performance TSV input is malformed") from error


def evidence_artifact_descriptor(artifact: EvidenceArtifact) -> dict[str, object]:
    return {
        "name": artifact.name,
        "schema": artifact.schema,
        "columns": list(artifact.columns),
        "records": artifact.records,
        "bytes": len(artifact.data),
        "sha256": contract.sha256_bytes(artifact.data),
    }


def serialize_evidence_artifacts(
    rows: Sequence[BenchmarkRow],
    summaries: Sequence[SummaryRow],
    comparisons: Sequence[ComparisonRow],
    metadata: dict[str, str],
) -> tuple[EvidenceArtifact, ...]:
    row_fields = tuple(BenchmarkRow.__dataclass_fields__)
    summary_fields = tuple(SummaryRow.__dataclass_fields__)
    comparison_fields = tuple(ComparisonRow.__dataclass_fields__)
    metadata_rows = tuple(
        {"key": key, "value": metadata[key]} for key in sorted(metadata)
    )
    return (
        evidence_artifact("rows.tsv", row_fields, (row_dict(row) for row in rows)),
        evidence_artifact(
            "summary.tsv",
            summary_fields,
            (summary_dict(row) for row in summaries),
        ),
        evidence_artifact(
            "comparison.tsv",
            comparison_fields,
            (comparison_dict(row) for row in comparisons),
        ),
        evidence_artifact("metadata.tsv", ("key", "value"), metadata_rows),
    )


def assemble_evidence_set(
    artifacts: tuple[EvidenceArtifact, ...],
    metadata: dict[str, str],
) -> EvidenceSet:
    manifest_payload = {
        "schema": EVIDENCE_SET_SCHEMA,
        "artifacts": [evidence_artifact_descriptor(artifact) for artifact in artifacts],
        "contract": dict(sorted(metadata.items())),
    }
    root_sha256 = contract.sha256_bytes(contract.canonical_json(manifest_payload))
    manifest = dict(manifest_payload)
    manifest["root_sha256"] = root_sha256
    return EvidenceSet(
        artifacts=artifacts,
        manifest=contract.canonical_json(manifest) + b"\n",
        root_sha256=root_sha256,
    )


def build_evidence_set(
    rows: Sequence[BenchmarkRow],
    summaries: Sequence[SummaryRow],
    comparisons: Sequence[ComparisonRow],
    metadata: dict[str, str],
) -> EvidenceSet:
    validate_lfs_performance_metadata(metadata)
    artifacts = serialize_evidence_artifacts(rows, summaries, comparisons, metadata)
    artifact_bytes = {artifact.name: artifact.data for artifact in artifacts}
    parsed_artifacts = {
        artifact.name: read_evidence_tsv_bytes(
            artifact.data,
            pathlib.Path(artifact.name),
            expected_fields=artifact.columns,
        )
        for artifact in artifacts
    }
    validate_evidence_row_counts(metadata, parsed_artifacts, artifact_bytes)
    return assemble_evidence_set(artifacts, metadata)


def benchmark_invocation(argv: Sequence[str]) -> BenchmarkInvocation:
    if any(
        not isinstance(value, str) or len(value) > MAX_EVIDENCE_CELL_BYTES * 8
        for value in argv
    ):
        raise BenchmarkError("benchmark invocation is invalid")
    try:
        script_sha256, _size = hash_file(BENCHMARK_SCRIPT_PATH)
    except OSError as error:
        raise BenchmarkError("benchmark invocation source is unavailable") from error
    payload = {
        "script_sha256": script_sha256,
        "arguments": list(argv),
    }
    return BenchmarkInvocation(
        argv_sha256=contract.sha256_bytes(contract.canonical_json(payload))
    )


def sample_log_binding(rows: Sequence[BenchmarkRow]) -> str:
    payload = [
        {
            "pair_id": row.pair_id,
            "tool": row.tool,
            "stdout_sha256": row.stdout_sha256,
            "stderr_sha256": row.stderr_sha256,
        }
        for row in rows
    ]
    return contract.sha256_bytes(contract.canonical_json(payload))


def failed_file_descriptor(name: str, data: bytes) -> dict[str, object]:
    return {
        "name": name,
        "bytes": len(data),
        "sha256": contract.sha256_bytes(data),
    }


def build_failed_evidence_bundle(
    candidate: EvidenceSet,
    metadata: dict[str, str],
    rows: Sequence[BenchmarkRow],
    invocation: BenchmarkInvocation,
    error: BaseException,
) -> FailedEvidenceBundle:
    policy = validate_lfs_performance_metadata(metadata)
    if len(rows) != policy.raw_row_count:
        raise BenchmarkError("failed evidence does not contain the complete acquisition")
    artifact_by_name = {artifact.name: artifact for artifact in candidate.artifacts}
    if set(artifact_by_name) != set(EVIDENCE_SET_FILES):
        raise BenchmarkError("failed evidence candidate artifacts are invalid")
    files = tuple(
        (
            FAILED_ARTIFACT_NAMES[name],
            artifact_by_name[name].data,
        )
        for name in EVIDENCE_SET_FILES
    ) + ((FAILED_CANDIDATE_MANIFEST_NAME, candidate.manifest),)
    if any(len(data) <= 0 or len(data) > MAX_EVIDENCE_ARTIFACT_BYTES for _name, data in files):
        raise BenchmarkError("failed evidence artifact size is invalid")
    if sum(len(data) for _name, data in files) > MAX_FAILED_EVIDENCE_BYTES:
        raise BenchmarkError("failed evidence bundle exceeds its size bound")
    error_payload = f"{type(error).__name__}:{error}".encode("utf-8", "backslashreplace")
    payload = {
        "schema": FAILED_EVIDENCE_SCHEMA,
        "claim": "not-established",
        "publishable": False,
        "failure_reason": "post-acquisition-validation-or-publication-failed",
        "error_sha256": contract.sha256_bytes(error_payload),
        "invocation_sha256": invocation.argv_sha256,
        "schedule_binding_sha256": metadata["schedule_binding_sha256"],
        "sample_log_binding_sha256": sample_log_binding(rows),
        "source_commit": metadata["source_commit"],
        "source_tree": metadata["source_tree"],
        "zmin_sha256": metadata["zmin_sha256"],
        "git_sha256": metadata["git_sha256"],
        "git_lfs_sha256": metadata["git_lfs_sha256"],
        "acquired_rows": len(rows),
        "candidate_root_sha256": candidate.root_sha256,
        "artifacts": [failed_file_descriptor(name, data) for name, data in files],
    }
    root_sha256 = contract.sha256_bytes(contract.canonical_json(payload))
    manifest = dict(payload)
    manifest["root_sha256"] = root_sha256
    manifest_bytes = contract.canonical_json(manifest) + b"\n"
    if len(manifest_bytes) > MAX_EVIDENCE_MANIFEST_BYTES:
        raise BenchmarkError("failed evidence manifest exceeds its size bound")
    return FailedEvidenceBundle(
        files=files,
        manifest=manifest_bytes,
        root_sha256=root_sha256,
    )


def parse_metadata_rows(rows: Sequence[dict[str, str]]) -> dict[str, str]:
    metadata: dict[str, str] = {}
    for row in rows:
        key = row["key"]
        if not key or key in metadata:
            raise BenchmarkError("LFS performance metadata has duplicate keys")
        metadata[key] = row["value"]
    validate_lfs_performance_metadata(metadata)
    return metadata


def parse_benchmark_rows(rows: Sequence[dict[str, str]]) -> list[BenchmarkRow]:
    integer_fields = {
        "concurrency",
        "order_index",
        "sample_index",
        "object_count",
        "object_bytes",
        "payload_bytes",
        "exit_code",
        "batch_requests",
        "transfer_requests",
        "max_active",
        "completed_objects",
        "completed_bytes",
        "residual_active",
        "host_monitor_samples",
    }
    float_fields = {
        "wall_seconds",
        "throughput_mib_per_second",
        "host_scheduler_lag_max_seconds",
    }
    parsed: list[BenchmarkRow] = []
    try:
        for row in rows:
            for value in row.values():
                validate_shareable_evidence_value(value)
            values: dict[str, object] = {}
            for key, value in row.items():
                if key in integer_fields:
                    values[key] = parse_canonical_decimal_cell(value)
                elif key in float_fields:
                    values[key] = float(value)
                else:
                    values[key] = value
            parsed.append(BenchmarkRow(**values))
    except (TypeError, ValueError) as error:
        raise BenchmarkError("LFS performance raw row value is invalid") from error
    if any(
        not math.isfinite(row.wall_seconds)
        or row.wall_seconds <= 0
        or not math.isfinite(row.throughput_mib_per_second)
        or row.throughput_mib_per_second <= 0
        or not math.isfinite(row.host_scheduler_lag_max_seconds)
        or row.host_scheduler_lag_max_seconds < 0
        for row in parsed
    ):
        raise BenchmarkError("LFS performance raw timing is invalid")
    return parsed


def validate_finite_metric(value: str, *, allow_zero: bool) -> float:
    try:
        parsed = float(value)
    except ValueError as error:
        raise BenchmarkError("LFS performance raw process timing is invalid") from error
    if not math.isfinite(parsed) or parsed < 0 or (not allow_zero and parsed == 0):
        raise BenchmarkError("LFS performance raw process timing is invalid")
    return parsed


def validate_host_cpu_busy_ratio(value: str) -> None:
    if value == "unobserved":
        return
    try:
        parsed = float(value)
    except ValueError as error:
        raise BenchmarkError("LFS performance host CPU diagnostic is invalid") from error
    if (
        value.startswith("-")
        or not math.isfinite(parsed)
        or not 0 <= parsed <= 1
        or value != f"{parsed:.9f}"
    ):
        raise BenchmarkError("LFS performance host CPU diagnostic is invalid")


def validate_benchmark_row(
    row: BenchmarkRow,
    policy: EvidencePolicy,
    metadata_memory_metric: str,
) -> None:
    if row.operation not in {item.value for item in BenchmarkOperation}:
        raise BenchmarkError("LFS performance raw operation is invalid")
    if row.tool not in {item.value for item in BenchmarkTool}:
        raise BenchmarkError("LFS performance raw tool is invalid")
    if row.sample_kind not in {item.value for item in SampleKind}:
        raise BenchmarkError("LFS performance raw sample kind is invalid")
    if row.concurrency not in policy.concurrencies:
        raise BenchmarkError("LFS performance raw concurrency is invalid")
    if row.sample_index <= 0:
        raise BenchmarkError("LFS performance raw sample index is invalid")
    sample_limit = (
        policy.warmups if row.sample_kind == SampleKind.WARMUP.value else policy.repeats
    )
    if row.sample_index > sample_limit:
        raise BenchmarkError("LFS performance raw sample index is invalid")
    pair_id = (
        f"{row.operation}-c{row.concurrency}-{row.sample_kind}-{row.sample_index:03d}"
    )
    if row.pair_id != pair_id:
        raise BenchmarkError("LFS performance raw pair identity is invalid")
    operation_index = tuple(BenchmarkOperation).index(BenchmarkOperation(row.operation))
    concurrency_index = policy.concurrencies.index(row.concurrency)
    lane_index = operation_index * len(policy.concurrencies) + concurrency_index
    expected_order = sample_order(lane_index, row.sample_index - 1)
    expected_order_index = expected_order.index(BenchmarkTool(row.tool)) + 1
    if row.order_index != expected_order_index:
        raise BenchmarkError("LFS performance raw alternating order is invalid")
    payload_bytes = policy.object_count * policy.object_bytes
    if (
        row.object_count != policy.object_count
        or row.object_bytes != policy.object_bytes
        or row.payload_bytes != payload_bytes
    ):
        raise BenchmarkError("LFS performance raw payload policy is invalid")
    if (
        row.exit_code != 0
        or row.batch_requests != 1
        or row.transfer_requests != policy.object_count
        or row.max_active != row.concurrency
        or row.completed_objects != policy.object_count
        or row.completed_bytes != payload_bytes
        or row.residual_active != 0
    ):
        raise BenchmarkError("LFS performance raw transfer evidence is invalid")
    validate_finite_metric(row.user_seconds, allow_zero=True)
    validate_finite_metric(row.system_seconds, allow_zero=True)
    validate_host_cpu_busy_ratio(row.host_system_cpu_busy_ratio)
    expected_throughput = (payload_bytes / (1024 * 1024)) / row.wall_seconds
    if not math.isclose(
        row.throughput_mib_per_second,
        expected_throughput,
        rel_tol=1e-6,
        abs_tol=0.5e-6,
    ):
        raise BenchmarkError("LFS performance raw throughput is inconsistent")
    if row.memory_metric != metadata_memory_metric:
        raise BenchmarkError("LFS performance raw memory metric changed")
    memory_contract = MEMORY_EVIDENCE_CONTRACTS.get(row.memory_metric)
    if memory_contract is None or (
        row.memory_semantics,
        row.memory_scope,
    ) != memory_contract:
        raise BenchmarkError("LFS performance raw memory contract is invalid")
    selected_memory = (
        row.peak_rss_bytes
        if row.memory_metric == "peak_rss_bytes"
        else row.peak_job_commit_bytes
    )
    unselected_memory = (
        row.peak_job_commit_bytes
        if row.memory_metric == "peak_rss_bytes"
        else row.peak_rss_bytes
    )
    if parse_canonical_decimal_cell(selected_memory) <= 0:
        raise BenchmarkError("LFS performance raw peak memory is invalid")
    if unselected_memory != "unsupported":
        raise BenchmarkError("LFS performance raw alternate memory metric is invalid")
    validate_sha256(row.stdout_sha256, "raw stdout")
    validate_sha256(row.stderr_sha256, "raw stderr")
    if row.host_load_average_1m != "unsupported":
        validate_finite_metric(row.host_load_average_1m, allow_zero=True)
    if (
        row.host_memory_pressure != "normal"
        or row.host_thermal_state != "normal"
        or row.host_monitor_samples <= 0
    ):
        raise BenchmarkError("LFS performance host monitor evidence is invalid")


def validate_evidence_row_counts(
    metadata: dict[str, str],
    parsed: dict[str, list[dict[str, str]]],
    artifact_bytes: dict[str, bytes],
    *,
    verify_derivations: bool = True,
) -> None:
    policy = validate_lfs_performance_metadata(metadata)
    if len(parsed["rows.tsv"]) != policy.raw_row_count:
        raise BenchmarkError("LFS performance raw row count is inconsistent")
    if len(parsed["summary.tsv"]) != policy.summary_row_count:
        raise BenchmarkError("LFS performance summary row count is inconsistent")
    if len(parsed["comparison.tsv"]) != policy.lane_count:
        raise BenchmarkError("LFS performance comparison row count is inconsistent")
    if policy.mode == EvidencePolicyMode.STRICT and (
        policy.lane_count != 6
        or policy.raw_row_count != 504
        or policy.summary_row_count != 12
    ):
        raise BenchmarkError("LFS performance strict evidence shape is invalid")
    allowed_concurrencies = {str(value) for value in policy.concurrencies}
    actual_raw_keys: set[tuple[str, str, str, str, str]] = set()
    for row in parsed["rows.tsv"]:
        if (
            row["operation"] not in {item.value for item in BenchmarkOperation}
            or row["tool"] not in {item.value for item in BenchmarkTool}
            or row["sample_kind"] not in {item.value for item in SampleKind}
            or row["concurrency"] not in allowed_concurrencies
            or row["object_count"] != metadata["fixture_object_count"]
            or row["object_bytes"] != metadata["fixture_object_bytes"]
        ):
            raise BenchmarkError("LFS performance raw row policy is inconsistent")
        raw_key = (
            row["operation"],
            row["concurrency"],
            row["sample_kind"],
            row["sample_index"],
            row["tool"],
        )
        if raw_key in actual_raw_keys:
            raise BenchmarkError("LFS performance raw row identity is duplicated")
        actual_raw_keys.add(raw_key)
    expected_raw_keys = {
        (operation.value, str(concurrency), sample_kind.value, str(index), tool.value)
        for operation in BenchmarkOperation
        for concurrency in policy.concurrencies
        for sample_kind, count in (
            (SampleKind.WARMUP, policy.warmups),
            (SampleKind.MEASURED, policy.repeats),
        )
        for index in range(1, count + 1)
        for tool in BenchmarkTool
    }
    if actual_raw_keys != expected_raw_keys:
        raise BenchmarkError("LFS performance raw row coverage is incomplete")
    expected_raw_sequence: list[tuple[str, str, str, str, str]] = []
    schedule_binding = metadata["schedule_binding_sha256"]
    lanes = canonical_lanes(policy)
    for sample_kind, count in (
        (SampleKind.WARMUP, policy.warmups),
        (SampleKind.MEASURED, policy.repeats),
    ):
        for sample_index in range(1, count + 1):
            for lane in scheduled_lanes(
                policy, schedule_binding, sample_kind, sample_index
            ):
                lane_index = lanes.index(lane)
                for tool in sample_order(lane_index, sample_index - 1):
                    expected_raw_sequence.append(
                        (
                            lane.operation.value,
                            str(lane.concurrency),
                            sample_kind.value,
                            str(sample_index),
                            tool.value,
                        )
                    )
    actual_raw_sequence = [
        (
            row["operation"],
            row["concurrency"],
            row["sample_kind"],
            row["sample_index"],
            row["tool"],
        )
        for row in parsed["rows.tsv"]
    ]
    if actual_raw_sequence != expected_raw_sequence:
        raise BenchmarkError("LFS performance raw schedule is not canonical")
    expected_summary_keys = {
        (operation.value, str(concurrency), tool.value)
        for operation in BenchmarkOperation
        for concurrency in policy.concurrencies
        for tool in BenchmarkTool
    }
    actual_summary_keys = {
        (row["operation"], row["concurrency"], row["tool"])
        for row in parsed["summary.tsv"]
    }
    if actual_summary_keys != expected_summary_keys or any(
        row["samples"] != str(policy.repeats) for row in parsed["summary.tsv"]
    ):
        raise BenchmarkError("LFS performance summary coverage is incomplete")
    expected_comparison_keys = {
        (operation.value, str(concurrency))
        for operation in BenchmarkOperation
        for concurrency in policy.concurrencies
    }
    actual_comparison_keys = {
        (row["operation"], row["concurrency"])
        for row in parsed["comparison.tsv"]
    }
    if actual_comparison_keys != expected_comparison_keys:
        raise BenchmarkError("LFS performance comparison coverage is incomplete")
    verdicts = {row["verdict"] for row in parsed["comparison.tsv"]}
    claim = metadata["claim"]
    if claim == BenchmarkClaim.STRICT_SUPERIORITY.value and verdicts != {"pass"}:
        raise BenchmarkError("LFS performance superiority claim is inconsistent")
    if claim == BenchmarkClaim.STRICT_NOT_ESTABLISHED.value and "fail" not in verdicts:
        raise BenchmarkError("LFS performance failed claim is inconsistent")
    if claim == BenchmarkClaim.FUNCTIONAL_SMOKE.value and verdicts != {"not-evaluated"}:
        raise BenchmarkError("LFS performance smoke claim is inconsistent")
    typed_rows = parse_benchmark_rows(parsed["rows.tsv"])
    for row in typed_rows:
        validate_benchmark_row(row, policy, metadata["memory_metric"])
    expected_raw = tsv_bytes(
        tuple(BenchmarkRow.__dataclass_fields__),
        (row_dict(row) for row in typed_rows),
    )
    if artifact_bytes["rows.tsv"] != expected_raw:
        raise BenchmarkError("LFS performance raw rows are not canonical")
    if not verify_derivations:
        return
    recomputed_summaries, recomputed_comparisons = summarize(
        typed_rows,
        policy.repeats,
        evaluate_strict=policy.mode == EvidencePolicyMode.STRICT,
    )
    expected_summary = tsv_bytes(
        tuple(SummaryRow.__dataclass_fields__),
        (summary_dict(row) for row in recomputed_summaries),
    )
    expected_comparison = tsv_bytes(
        tuple(ComparisonRow.__dataclass_fields__),
        (comparison_dict(row) for row in recomputed_comparisons),
    )
    if artifact_bytes["summary.tsv"] != expected_summary:
        raise BenchmarkError("LFS performance summary does not match raw rows")
    if artifact_bytes["comparison.tsv"] != expected_comparison:
        raise BenchmarkError("LFS performance comparison does not match raw rows")


def verify_evidence_set_directory(
    root: pathlib.Path,
    expected_root_sha256: str,
) -> VerifiedEvidenceSet:
    expected_root_sha256 = validate_sha256(expected_root_sha256, "evidence set")
    try:
        root = contract.absolute_directory(root, reject_symlinks=True)
        actual_names = {entry.name for entry in root.iterdir()}
        expected_names = {*EVIDENCE_SET_FILES, EVIDENCE_MANIFEST_NAME}
        if actual_names != expected_names:
            raise BenchmarkError("LFS performance evidence set has unexpected files")
        identity = contract.path_identity(root)
        manifest_snapshot = contract.artifact_read_snapshot(
            root,
            EVIDENCE_MANIFEST_NAME,
            expected_directory_identity=identity,
            max_bytes=MAX_EVIDENCE_MANIFEST_BYTES,
        )
        manifest = contract.load_json_bytes(
            manifest_snapshot["data"],
            pathlib.Path(EVIDENCE_MANIFEST_NAME),
        )
    except BenchmarkError:
        raise
    except (UnicodeError, contract.ContractError, OSError) as error:
        raise BenchmarkError("LFS performance evidence set could not be read") from error
    if set(manifest) != {"schema", "artifacts", "contract", "root_sha256"}:
        raise BenchmarkError("LFS performance evidence manifest schema is invalid")
    if manifest_snapshot["data"] != contract.canonical_json(manifest) + b"\n":
        raise BenchmarkError("LFS performance evidence manifest bytes are not canonical")
    if manifest["schema"] != EVIDENCE_SET_SCHEMA:
        raise BenchmarkError("LFS performance evidence manifest version is invalid")
    root_sha256 = validate_sha256(manifest["root_sha256"], "evidence manifest")
    payload = {key: manifest[key] for key in ("schema", "artifacts", "contract")}
    if contract.sha256_bytes(contract.canonical_json(payload)) != root_sha256:
        raise BenchmarkError("LFS performance evidence root digest is invalid")
    if root_sha256 != expected_root_sha256:
        raise BenchmarkError("LFS performance evidence root changed")
    metadata_value = manifest["contract"]
    if not isinstance(metadata_value, dict) or any(
        not isinstance(key, str) or not isinstance(value, str)
        for key, value in metadata_value.items()
    ):
        raise BenchmarkError("LFS performance evidence contract is invalid")
    metadata = dict(metadata_value)
    validate_lfs_performance_metadata(metadata)
    descriptors = manifest["artifacts"]
    if not isinstance(descriptors, list) or len(descriptors) != len(EVIDENCE_SET_FILES):
        raise BenchmarkError("LFS performance evidence artifact plan is invalid")
    expected_columns = {
        "rows.tsv": tuple(BenchmarkRow.__dataclass_fields__),
        "summary.tsv": tuple(SummaryRow.__dataclass_fields__),
        "comparison.tsv": tuple(ComparisonRow.__dataclass_fields__),
        "metadata.tsv": ("key", "value"),
    }
    parsed: dict[str, list[dict[str, str]]] = {}
    artifact_bytes: dict[str, bytes] = {}
    for expected_name, descriptor in zip(EVIDENCE_SET_FILES, descriptors):
        if not isinstance(descriptor, dict) or set(descriptor) != {
            "name",
            "schema",
            "columns",
            "records",
            "bytes",
            "sha256",
        }:
            raise BenchmarkError("LFS performance evidence artifact descriptor is invalid")
        columns = expected_columns[expected_name]
        if (
            descriptor["name"] != expected_name
            or descriptor["schema"] != EVIDENCE_ARTIFACT_SCHEMAS[expected_name]
            or descriptor["columns"] != list(columns)
            or isinstance(descriptor["records"], bool)
            or not isinstance(descriptor["records"], int)
            or descriptor["records"] < 0
            or isinstance(descriptor["bytes"], bool)
            or not isinstance(descriptor["bytes"], int)
            or descriptor["bytes"] <= 0
        ):
            raise BenchmarkError("LFS performance evidence artifact contract is invalid")
        digest = validate_sha256(descriptor["sha256"], "evidence artifact")
        try:
            snapshot = contract.artifact_read_snapshot(
                root,
                expected_name,
                expected_directory_identity=identity,
                max_bytes=MAX_EVIDENCE_ARTIFACT_BYTES,
            )
            rows = read_evidence_tsv_bytes(
                snapshot["data"],
                pathlib.Path(expected_name),
                expected_fields=columns,
            )
        except (UnicodeError, contract.ContractError, OSError) as error:
            raise BenchmarkError("LFS performance evidence artifact could not be read") from error
        if (
            snapshot["sha256"] != digest
            or len(snapshot["data"]) != descriptor["bytes"]
            or len(rows) != descriptor["records"]
            or tsv_bytes(columns, rows) != snapshot["data"]
        ):
            raise BenchmarkError("LFS performance evidence artifact bytes are invalid")
        parsed[expected_name] = rows
        artifact_bytes[expected_name] = snapshot["data"]
    parsed_metadata = parse_metadata_rows(parsed["metadata.tsv"])
    if parsed_metadata != metadata:
        raise BenchmarkError("LFS performance metadata and manifest disagree")
    expected_metadata = tsv_bytes(
        ("key", "value"),
        ({"key": key, "value": metadata[key]} for key in sorted(metadata)),
    )
    if artifact_bytes["metadata.tsv"] != expected_metadata:
        raise BenchmarkError("LFS performance metadata rows are not canonical")
    validate_evidence_row_counts(metadata, parsed, artifact_bytes)
    return VerifiedEvidenceSet(root_sha256=root_sha256, metadata=metadata)


def write_all(descriptor: int, data: bytes) -> None:
    view = memoryview(data)
    while view:
        written = os.write(descriptor, view)
        if written <= 0:
            raise BenchmarkError("evidence file write made no progress")
        view = view[written:]


def write_staged_evidence_file(
    directory_fd: int | None,
    directory: pathlib.Path,
    name: str,
    data: bytes,
) -> None:
    nofollow = getattr(os, "O_NOFOLLOW", 0)
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | nofollow
    descriptor: int | None = None
    try:
        if os.name == "nt":
            descriptor = os.open(directory / name, flags, 0o600)
        elif directory_fd is not None:
            descriptor = os.open(name, flags, 0o600, dir_fd=directory_fd)
        else:
            raise BenchmarkError("evidence staging directory is not pinned")
        write_all(descriptor, data)
        os.fsync(descriptor)
    except OSError as error:
        raise BenchmarkError("evidence staging write failed") from error
    finally:
        if descriptor is not None:
            os.close(descriptor)


def rename_directory_noreplace_posix(
    parent_fd: int,
    source_name: str,
    destination_name: str,
) -> None:
    library = ctypes.CDLL(None, use_errno=True)
    system = platform.system()
    if system == "Darwin":
        rename = getattr(library, "renameatx_np", None)
        flag = 0x00000004
    elif system == "Linux":
        rename = getattr(library, "renameat2", None)
        flag = 0x00000001
    else:
        rename = None
        flag = 0
    if rename is None:
        raise BenchmarkError("atomic no-replace evidence publication is unsupported")
    rename.argtypes = [ctypes.c_int, ctypes.c_char_p, ctypes.c_int, ctypes.c_char_p, ctypes.c_uint]
    rename.restype = ctypes.c_int
    result = rename(
        parent_fd,
        os.fsencode(source_name),
        parent_fd,
        os.fsencode(destination_name),
        flag,
    )
    if result == 0:
        return
    error_number = ctypes.get_errno()
    if error_number in {errno.EEXIST, errno.ENOTEMPTY}:
        raise BenchmarkError("evidence output already exists")
    raise BenchmarkError("atomic no-replace evidence publication failed") from OSError(
        error_number,
        "rename directory",
    )


def rename_directory_noreplace_windows(
    parent: pathlib.Path,
    source_name: str,
    destination_name: str,
) -> None:
    try:
        from ctypes import wintypes

        kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
        move = kernel32.MoveFileExW
        move.argtypes = [wintypes.LPCWSTR, wintypes.LPCWSTR, wintypes.DWORD]
        move.restype = wintypes.BOOL
        moved = move(
            str(parent / source_name),
            str(parent / destination_name),
            0x00000008,
        )
    except (AttributeError, OSError) as error:
        raise BenchmarkError("atomic Windows evidence publication is unsupported") from error
    if moved:
        return
    error_number = ctypes.get_last_error()
    if error_number in {80, 183}:
        raise BenchmarkError("evidence output already exists")
    raise BenchmarkError("atomic no-replace evidence publication failed") from OSError(
        error_number,
        "move directory",
    )


def rename_directory_noreplace(
    parent: pathlib.Path,
    parent_fd: int | None,
    source_name: str,
    destination_name: str,
) -> None:
    if os.name == "nt":
        rename_directory_noreplace_windows(parent, source_name, destination_name)
    elif parent_fd is not None:
        rename_directory_noreplace_posix(parent_fd, source_name, destination_name)
    else:
        raise BenchmarkError("atomic evidence publication has no pinned parent")


def directory_identity(path: pathlib.Path) -> tuple[int, int]:
    state = os.stat(path, follow_symlinks=False)
    if not stat.S_ISDIR(state.st_mode):
        raise BenchmarkError("evidence publication path is not a directory")
    return int(state.st_dev), int(state.st_ino)


def directory_entry_identity(
    parent: pathlib.Path,
    parent_fd: int | None,
    name: str,
) -> tuple[int, int]:
    if parent_fd is None:
        return directory_identity(parent / name)
    state = os.stat(name, dir_fd=parent_fd, follow_symlinks=False)
    if not stat.S_ISDIR(state.st_mode):
        raise BenchmarkError("evidence publication entry is not a directory")
    return int(state.st_dev), int(state.st_ino)


def remove_owned_evidence_directory(
    parent: pathlib.Path,
    parent_fd: int | None,
    name: str,
    expected_identity: tuple[int, int],
    child_names: Sequence[str] = (*EVIDENCE_SET_FILES, EVIDENCE_MANIFEST_NAME),
) -> None:
    if parent_fd is not None:
        directory_fd: int | None = None
        try:
            if directory_entry_identity(parent, parent_fd, name) != expected_identity:
                raise BenchmarkError("evidence cleanup refused a replaced directory")
            directory_fd = os.open(
                name,
                os.O_RDONLY | getattr(os, "O_DIRECTORY") | getattr(os, "O_NOFOLLOW"),
                dir_fd=parent_fd,
            )
            for child_name in child_names:
                try:
                    child_state = os.stat(
                        child_name,
                        dir_fd=directory_fd,
                        follow_symlinks=False,
                    )
                except FileNotFoundError:
                    continue
                if not stat.S_ISREG(child_state.st_mode):
                    raise BenchmarkError("evidence cleanup refused a replaced artifact")
                os.unlink(child_name, dir_fd=directory_fd)
            os.rmdir(name, dir_fd=parent_fd)
            return
        except FileNotFoundError:
            return
        except OSError as error:
            raise BenchmarkError("evidence staging cleanup failed") from error
        finally:
            if directory_fd is not None:
                os.close(directory_fd)
    candidate = parent / name
    try:
        if directory_identity(candidate) != expected_identity:
            raise BenchmarkError("evidence cleanup refused a replaced directory")
        for child_name in child_names:
            child = candidate / child_name
            try:
                child_state = os.stat(child, follow_symlinks=False)
            except FileNotFoundError:
                continue
            if not stat.S_ISREG(child_state.st_mode):
                raise BenchmarkError("evidence cleanup refused a replaced artifact")
            child.unlink()
        candidate.rmdir()
    except FileNotFoundError:
        return
    except OSError as error:
        raise BenchmarkError("evidence staging cleanup failed") from error


def verify_failed_evidence_directory(
    root: pathlib.Path,
    expected_root_sha256: str,
) -> None:
    expected_root_sha256 = validate_sha256(expected_root_sha256, "failed evidence")
    try:
        root = contract.absolute_directory(root, reject_symlinks=True)
        if {entry.name for entry in root.iterdir()} != set(FAILED_EVIDENCE_FILES):
            raise BenchmarkError("failed evidence set has unexpected files")
        identity = contract.path_identity(root)
        snapshots = {
            name: contract.artifact_read_snapshot(
                root,
                name,
                expected_directory_identity=identity,
                max_bytes=(
                    MAX_EVIDENCE_MANIFEST_BYTES
                    if name == FAILED_EVIDENCE_MANIFEST_NAME
                    else MAX_EVIDENCE_ARTIFACT_BYTES
                ),
            )
            for name in FAILED_EVIDENCE_FILES
        }
        manifest_bytes = snapshots[FAILED_EVIDENCE_MANIFEST_NAME]["data"]
        manifest = contract.load_json_bytes(
            manifest_bytes,
            pathlib.Path(FAILED_EVIDENCE_MANIFEST_NAME),
        )
    except (UnicodeError, contract.ContractError, OSError) as error:
        raise BenchmarkError("failed evidence set could not be read") from error
    if contract.canonical_json(manifest) + b"\n" != manifest_bytes:
        raise BenchmarkError("failed evidence manifest is not canonical")
    expected_keys = {
        "schema",
        "claim",
        "publishable",
        "failure_reason",
        "error_sha256",
        "invocation_sha256",
        "schedule_binding_sha256",
        "sample_log_binding_sha256",
        "source_commit",
        "source_tree",
        "zmin_sha256",
        "git_sha256",
        "git_lfs_sha256",
        "acquired_rows",
        "candidate_root_sha256",
        "artifacts",
        "root_sha256",
    }
    if (
        set(manifest) != expected_keys
        or manifest["schema"] != FAILED_EVIDENCE_SCHEMA
        or manifest["claim"] != "not-established"
        or manifest["publishable"] is not False
        or manifest["failure_reason"]
        != "post-acquisition-validation-or-publication-failed"
        or manifest["root_sha256"] != expected_root_sha256
    ):
        raise BenchmarkError("failed evidence manifest contract is invalid")
    payload = dict(manifest)
    payload.pop("root_sha256")
    if contract.sha256_bytes(contract.canonical_json(payload)) != expected_root_sha256:
        raise BenchmarkError("failed evidence root does not match its manifest")
    for key in (
        "error_sha256",
        "invocation_sha256",
        "schedule_binding_sha256",
        "sample_log_binding_sha256",
        "zmin_sha256",
        "git_sha256",
        "git_lfs_sha256",
        "candidate_root_sha256",
    ):
        validate_sha256(manifest[key], key.replace("_", " "))
    validate_git_object_id(manifest["source_commit"], "source commit")
    validate_git_object_id(manifest["source_tree"], "source tree")
    descriptors = manifest["artifacts"]
    candidate_names = (*FAILED_ARTIFACT_NAMES.values(), FAILED_CANDIDATE_MANIFEST_NAME)
    if not isinstance(descriptors, list) or len(descriptors) != len(candidate_names):
        raise BenchmarkError("failed evidence artifact manifest is invalid")
    total_bytes = 0
    for expected_name, descriptor in zip(candidate_names, descriptors, strict=True):
        if not isinstance(descriptor, dict) or set(descriptor) != {"name", "bytes", "sha256"}:
            raise BenchmarkError("failed evidence artifact descriptor is invalid")
        data = snapshots[expected_name]["data"]
        if (
            descriptor["name"] != expected_name
            or not isinstance(descriptor["bytes"], int)
            or descriptor["bytes"] != len(data)
            or descriptor["sha256"] != contract.sha256_bytes(data)
        ):
            raise BenchmarkError("failed evidence artifact bytes are invalid")
        total_bytes += len(data)
    if total_bytes > MAX_FAILED_EVIDENCE_BYTES:
        raise BenchmarkError("failed evidence bundle exceeds its size bound")
    parsed = {
        original_name: read_evidence_tsv_bytes(
            snapshots[failed_name]["data"],
            pathlib.Path(failed_name),
            expected_fields=(
                tuple(BenchmarkRow.__dataclass_fields__)
                if original_name == "rows.tsv"
                else tuple(SummaryRow.__dataclass_fields__)
                if original_name == "summary.tsv"
                else tuple(ComparisonRow.__dataclass_fields__)
                if original_name == "comparison.tsv"
                else ("key", "value")
            ),
        )
        for original_name, failed_name in FAILED_ARTIFACT_NAMES.items()
    }
    artifact_bytes = {
        original_name: snapshots[failed_name]["data"]
        for original_name, failed_name in FAILED_ARTIFACT_NAMES.items()
    }
    metadata = parse_metadata_rows(parsed["metadata.tsv"])
    validate_evidence_row_counts(
        metadata,
        parsed,
        artifact_bytes,
        verify_derivations=False,
    )
    typed_rows = parse_benchmark_rows(parsed["rows.tsv"])
    if (
        manifest["acquired_rows"] != len(typed_rows)
        or manifest["schedule_binding_sha256"] != metadata["schedule_binding_sha256"]
        or manifest["sample_log_binding_sha256"] != sample_log_binding(typed_rows)
        or manifest["source_commit"] != metadata["source_commit"]
        or manifest["source_tree"] != metadata["source_tree"]
        or manifest["zmin_sha256"] != metadata["zmin_sha256"]
        or manifest["git_sha256"] != metadata["git_sha256"]
        or manifest["git_lfs_sha256"] != metadata["git_lfs_sha256"]
    ):
        raise BenchmarkError("failed evidence provenance binding is invalid")
    candidate_bytes = snapshots[FAILED_CANDIDATE_MANIFEST_NAME]["data"]
    try:
        candidate_manifest = contract.load_json_bytes(
            candidate_bytes,
            pathlib.Path(FAILED_CANDIDATE_MANIFEST_NAME),
        )
    except (UnicodeError, contract.ContractError) as error:
        raise BenchmarkError("failed evidence candidate manifest is invalid") from error
    if (
        contract.canonical_json(candidate_manifest) + b"\n" != candidate_bytes
        or candidate_manifest.get("root_sha256") != manifest["candidate_root_sha256"]
    ):
        raise BenchmarkError("failed evidence candidate root is invalid")
    candidate_payload = dict(candidate_manifest)
    candidate_payload.pop("root_sha256", None)
    if (
        contract.sha256_bytes(contract.canonical_json(candidate_payload))
        != manifest["candidate_root_sha256"]
    ):
        raise BenchmarkError("failed evidence candidate root is invalid")
    reconstructed_artifacts = tuple(
        EvidenceArtifact(
            name=original_name,
            schema=EVIDENCE_ARTIFACT_SCHEMAS[original_name],
            columns=(
                tuple(BenchmarkRow.__dataclass_fields__)
                if original_name == "rows.tsv"
                else tuple(SummaryRow.__dataclass_fields__)
                if original_name == "summary.tsv"
                else tuple(ComparisonRow.__dataclass_fields__)
                if original_name == "comparison.tsv"
                else ("key", "value")
            ),
            records=len(parsed[original_name]),
            data=artifact_bytes[original_name],
        )
        for original_name in EVIDENCE_SET_FILES
    )
    reconstructed_candidate = assemble_evidence_set(reconstructed_artifacts, metadata)
    if (
        reconstructed_candidate.manifest != candidate_bytes
        or reconstructed_candidate.root_sha256 != manifest["candidate_root_sha256"]
    ):
        raise BenchmarkError("failed evidence candidate artifacts are invalid")


def publish_failed_evidence_bundle(
    failure_root: pathlib.Path,
    bundle: FailedEvidenceBundle,
    hooks: FailedEvidencePublicationHooks | None = None,
) -> None:
    hooks = hooks or FailedEvidencePublicationHooks()
    candidate = pathlib.Path(failure_root).expanduser()
    if not candidate.is_absolute() or not candidate.name:
        raise BenchmarkError("failed evidence output must be an absolute directory path")
    try:
        parent = contract.absolute_directory(candidate.parent, reject_symlinks=True)
    except (contract.ContractError, OSError) as error:
        raise BenchmarkError("failed evidence output parent is not trusted") from error
    failure_root = parent / candidate.name
    if os.path.lexists(failure_root):
        raise BenchmarkError("failed evidence output already exists")
    parent_fd: int | None = None
    staging_fd: int | None = None
    staging_name: str | None = None
    staging_identity: tuple[int, int] | None = None
    published = False
    parent_identity = directory_identity(parent)
    try:
        if os.name != "nt":
            directory_flag = getattr(os, "O_DIRECTORY", 0)
            nofollow = getattr(os, "O_NOFOLLOW", 0)
            if not directory_flag or not nofollow:
                raise BenchmarkError("atomic failed evidence pinning is unsupported")
            parent_fd = os.open(parent, os.O_RDONLY | directory_flag | nofollow)
            os.fsync(parent_fd)
        for _ in range(32):
            staging_name = f".{candidate.name}.{secrets.token_hex(12)}.tmp"
            try:
                if os.name == "nt":
                    os.mkdir(parent / staging_name, 0o700)
                else:
                    os.mkdir(staging_name, 0o700, dir_fd=parent_fd)
            except FileExistsError:
                continue
            break
        else:
            raise BenchmarkError("could not allocate failed evidence staging directory")
        staging_path = parent / staging_name
        staging_identity = directory_identity(staging_path)
        if os.name != "nt":
            staging_fd = os.open(
                staging_name,
                os.O_RDONLY | getattr(os, "O_DIRECTORY") | getattr(os, "O_NOFOLLOW"),
                dir_fd=parent_fd,
            )
        for name, data in bundle.files:
            write_staged_evidence_file(staging_fd, staging_path, name, data)
        write_staged_evidence_file(
            staging_fd,
            staging_path,
            FAILED_EVIDENCE_MANIFEST_NAME,
            bundle.manifest,
        )
        if os.name != "nt":
            os.fsync(staging_fd)
        verify_failed_evidence_directory(staging_path, bundle.root_sha256)
        if hooks.before_freeze is not None:
            hooks.before_freeze(staging_path)
        if os.name == "nt":
            for name in FAILED_EVIDENCE_FILES:
                os.chmod(staging_path / name, 0o444)
            os.chmod(staging_path, 0o555)
        else:
            for name in FAILED_EVIDENCE_FILES:
                os.chmod(name, 0o444, dir_fd=staging_fd, follow_symlinks=False)
            os.fchmod(staging_fd, 0o555)
            os.fsync(staging_fd)
        verify_failed_evidence_directory(staging_path, bundle.root_sha256)
        if hooks.before_publish is not None:
            hooks.before_publish(staging_path)
        if (
            directory_identity(parent) != parent_identity
            or directory_entry_identity(parent, parent_fd, staging_name)
            != staging_identity
        ):
            raise BenchmarkError("failed evidence output parent changed before publication")
        rename_directory_noreplace(parent, parent_fd, staging_name, candidate.name)
        published = True
        if (
            directory_identity(parent) != parent_identity
            or directory_entry_identity(parent, parent_fd, candidate.name)
            != staging_identity
        ):
            raise BenchmarkError("failed evidence namespace changed during publication")
        if hooks.after_readback is not None:
            hooks.after_readback(failure_root)
        verify_failed_evidence_directory(failure_root, bundle.root_sha256)
        if (
            directory_identity(parent) != parent_identity
            or directory_entry_identity(parent, parent_fd, candidate.name)
            != staging_identity
        ):
            raise BenchmarkError("failed evidence namespace changed during readback")
        if parent_fd is not None:
            os.fsync(parent_fd)
    except Exception:
        if published and staging_identity is not None:
            try:
                if directory_entry_identity(parent, parent_fd, candidate.name) == staging_identity:
                    rename_directory_noreplace(
                        parent,
                        parent_fd,
                        candidate.name,
                        staging_name or "",
                    )
                    published = False
                    if parent_fd is not None:
                        os.fsync(parent_fd)
            except (BenchmarkError, OSError):
                pass
        raise
    finally:
        if not published and staging_name is not None and staging_identity is not None:
            if staging_fd is not None:
                try:
                    os.fchmod(staging_fd, 0o700)
                except OSError:
                    pass
            else:
                try:
                    os.chmod(parent / staging_name, 0o700)
                except OSError:
                    pass
            remove_owned_evidence_directory(
                parent,
                parent_fd,
                staging_name,
                staging_identity,
                FAILED_EVIDENCE_FILES,
            )
        if staging_fd is not None:
            os.close(staging_fd)
        if parent_fd is not None:
            os.close(parent_fd)


def publish_evidence_set(
    output_root: pathlib.Path,
    evidence: EvidenceSet,
    final_check: Callable[[], None],
    hooks: EvidencePublicationHooks | None = None,
) -> VerifiedEvidenceSet:
    hooks = hooks or EvidencePublicationHooks()
    candidate = pathlib.Path(output_root).expanduser()
    if not candidate.is_absolute() or not candidate.name:
        raise BenchmarkError("evidence output must be an absolute directory path")
    try:
        parent = contract.absolute_directory(candidate.parent, reject_symlinks=True)
    except (contract.ContractError, OSError) as error:
        raise BenchmarkError("evidence output parent is not trusted") from error
    output_root = parent / candidate.name
    if os.path.lexists(output_root):
        raise BenchmarkError("evidence output already exists")
    parent_fd: int | None = None
    staging_fd: int | None = None
    staging_name: str | None = None
    staging_identity: tuple[int, int] | None = None
    published = False
    parent_identity = directory_identity(parent)
    try:
        if os.name != "nt":
            directory_flag = getattr(os, "O_DIRECTORY", 0)
            nofollow = getattr(os, "O_NOFOLLOW", 0)
            if not directory_flag or not nofollow:
                raise BenchmarkError("atomic evidence directory pinning is unsupported")
            parent_fd = os.open(parent, os.O_RDONLY | directory_flag | nofollow)
            os.fsync(parent_fd)
        for _ in range(32):
            staging_name = f".{candidate.name}.{secrets.token_hex(12)}.tmp"
            try:
                if os.name == "nt":
                    os.mkdir(parent / staging_name, 0o700)
                else:
                    os.mkdir(staging_name, 0o700, dir_fd=parent_fd)
            except FileExistsError:
                continue
            break
        else:
            raise BenchmarkError("could not allocate evidence staging directory")
        staging_path = parent / staging_name
        staging_identity = directory_identity(staging_path)
        if os.name != "nt":
            staging_fd = os.open(
                staging_name,
                os.O_RDONLY | getattr(os, "O_DIRECTORY") | getattr(os, "O_NOFOLLOW"),
                dir_fd=parent_fd,
            )
        for artifact in evidence.artifacts:
            write_staged_evidence_file(
                staging_fd,
                staging_path,
                artifact.name,
                artifact.data,
            )
        write_staged_evidence_file(
            staging_fd,
            staging_path,
            EVIDENCE_MANIFEST_NAME,
            evidence.manifest,
        )
        if os.name != "nt":
            os.fsync(staging_fd)
        verify_evidence_set_directory(staging_path, evidence.root_sha256)
        if hooks.before_final_check is not None:
            hooks.before_final_check()
        final_check()
        if hooks.before_publish is not None:
            hooks.before_publish()
            final_check()
        if directory_identity(parent) != parent_identity:
            raise BenchmarkError("evidence output parent changed before publication")
        rename_directory_noreplace(parent, parent_fd, staging_name, candidate.name)
        published = True
        if directory_identity(parent) != parent_identity:
            raise BenchmarkError("evidence output parent changed during publication")
        if directory_entry_identity(parent, parent_fd, candidate.name) != staging_identity:
            raise BenchmarkError("published evidence directory identity changed")
        verified = verify_evidence_set_directory(output_root, evidence.root_sha256)
        if (
            directory_identity(parent) != parent_identity
            or directory_entry_identity(parent, parent_fd, candidate.name) != staging_identity
        ):
            raise BenchmarkError("published evidence namespace changed during readback")
        if hooks.after_publish is not None:
            hooks.after_publish()
        if parent_fd is not None:
            os.fsync(parent_fd)
        return verified
    except Exception:
        if published and staging_identity is not None:
            try:
                if directory_entry_identity(parent, parent_fd, candidate.name) == staging_identity:
                    rename_directory_noreplace(
                        parent,
                        parent_fd,
                        candidate.name,
                        staging_name or "",
                    )
                    published = False
                    if parent_fd is not None:
                        os.fsync(parent_fd)
            except (BenchmarkError, OSError):
                pass
        raise
    finally:
        if staging_fd is not None:
            os.close(staging_fd)
        if not published and staging_name is not None and staging_identity is not None:
            remove_owned_evidence_directory(
                parent,
                parent_fd,
                staging_name,
                staging_identity,
            )
        if parent_fd is not None:
            os.close(parent_fd)


def publish_evidence_with_failure_custody(
    output_root: pathlib.Path,
    failure_root: pathlib.Path,
    rows: Sequence[BenchmarkRow],
    summaries: Sequence[SummaryRow],
    comparisons: Sequence[ComparisonRow],
    metadata: dict[str, str],
    invocation: BenchmarkInvocation,
    final_check: Callable[[], None],
    hooks: EvidencePublicationHooks | None = None,
) -> VerifiedEvidenceSet:
    artifacts = serialize_evidence_artifacts(rows, summaries, comparisons, metadata)
    candidate = assemble_evidence_set(artifacts, metadata)
    try:
        evidence = build_evidence_set(rows, summaries, comparisons, metadata)
        return publish_evidence_set(output_root, evidence, final_check, hooks)
    except Exception as error:
        if os.path.lexists(output_root):
            raise BenchmarkError(
                "positive evidence remained after failed publication"
            ) from error
        try:
            bundle = build_failed_evidence_bundle(
                candidate,
                metadata,
                rows,
                invocation,
                error,
            )
            publish_failed_evidence_bundle(failure_root, bundle)
        except Exception as custody_error:
            raise BenchmarkError(
                "failed evidence custody could not be published"
            ) from custody_error
        raise


def verify_identity_unchanged(identity: ExecutableIdentity) -> None:
    digest, _ = hash_file(identity.path)
    if digest != identity.sha256:
        raise BenchmarkError("pinned executable identity changed during the benchmark")


def run_benchmark(
    args: argparse.Namespace,
    invocation: BenchmarkInvocation,
) -> tuple[list[BenchmarkRow], list[ComparisonRow]]:
    policy = BenchmarkPolicy.from_args(args)
    try:
        output_root = contract.absolute_output_path(args.output_dir, reject_symlinks=True)
        contract.absolute_directory(output_root.parent, reject_symlinks=True)
    except (contract.ContractError, OSError) as error:
        raise BenchmarkError("output directory parent is not trusted") from error
    if os.path.lexists(output_root):
        raise BenchmarkError("output directory must not exist")
    try:
        failure_root = contract.absolute_output_path(
            args.failure_dir,
            reject_symlinks=True,
        )
        contract.absolute_directory(failure_root.parent, reject_symlinks=True)
    except (contract.ContractError, OSError) as error:
        raise BenchmarkError("failure directory parent is not trusted") from error
    if failure_root == output_root:
        raise BenchmarkError("positive and failed evidence directories must differ")
    if os.path.lexists(failure_root):
        raise BenchmarkError("failure directory must not exist")
    zmin = executable_identity(args.zmin_bin, args.zmin_sha256, ("--version",), None)
    git = executable_identity(args.git_bin, args.git_sha256, ("--version",), "git version ")
    provenance = load_release_provenance(args.repo_root, zmin, git)
    verify_benchmark_source_binding(provenance)
    validate_output_location(output_root, provenance.repo_root)
    validate_output_location(failure_root, provenance.repo_root)
    git_lfs = parse_pinned_git_lfs(args.git_lfs_manifest)
    schedule_binding = schedule_binding_for_run(
        policy, provenance, zmin, git, git_lfs
    )
    monitor_adapter = host_noise.current_host_adapter()
    runner = contract.absolute_file(
        BENCHMARK_TOOLS_DIR / "git-bench-process.py",
        executable=True,
        reject_symlinks=True,
    )
    rows: list[BenchmarkRow] = []
    with tempfile.TemporaryDirectory(prefix="zmin-lfs-performance-") as directory:
        temporary_root = pathlib.Path(directory)
        (temporary_root / "runs").mkdir()
        metrics_root = temporary_root / "metrics"
        metrics_root.mkdir()
        metrics_identity = contract.artifact_identity_token(contract.path_identity(metrics_root))
        environment = benchmark_environment(temporary_root, git, git_lfs)
        objects = create_fixture_objects(
            temporary_root / "objects",
            policy.object_count,
            policy.object_bytes,
        )
        baseline = create_baseline_repository(
            temporary_root,
            objects,
            git,
            environment,
        )
        lanes = canonical_lanes(policy)
        with (
            host_noise.HostNoiseMonitor(monitor_adapter) as monitor,
            FixtureServer() as fixture,
        ):
            for sample_kind, count in (
                (SampleKind.WARMUP, policy.warmups),
                (SampleKind.MEASURED, policy.repeats),
            ):
                for sample_index in range(1, count + 1):
                    for lane in scheduled_lanes(
                        policy, schedule_binding, sample_kind, sample_index
                    ):
                        lane_index = lanes.index(lane)
                        pair_id = (
                            f"{lane.operation.value}-c{lane.concurrency}-"
                            f"{sample_kind.value}-{sample_index:03d}"
                        )
                        for order_index, tool in enumerate(
                            sample_order(lane_index, sample_index - 1), start=1
                        ):
                            interval_token = monitor.begin_interval()
                            row = execute_sample(
                                temporary_root=temporary_root,
                                baseline=baseline,
                                objects=objects,
                                fixture=fixture,
                                runner=runner,
                                metrics_root=metrics_root,
                                metrics_identity=metrics_identity,
                                environment=environment,
                                git=git,
                                git_lfs=git_lfs,
                                zmin=zmin,
                                operation=lane.operation,
                                concurrency=lane.concurrency,
                                sample_kind=sample_kind,
                                sample_index=sample_index,
                                pair_id=pair_id,
                                order_index=order_index,
                                tool=tool,
                                timeout_seconds=policy.child_timeout_seconds,
                            )
                            rows.append(
                                bind_host_interval(
                                    row, monitor.end_interval(interval_token)
                                )
                            )
        rows = canonicalize_benchmark_rows(rows)
        summaries, comparisons = summarize(
            rows,
            policy.repeats,
            evaluate_strict=policy.strict_gate,
        )
        memory_metrics = {row.memory_metric for row in rows}
        if len(memory_metrics) != 1:
            raise BenchmarkError("host memory metric changed during the benchmark")
        metadata = {
            "schema_name": LFS_PERFORMANCE_SCHEMA_NAME,
            "schema_version": LFS_PERFORMANCE_SCHEMA_VERSION,
            "claim": benchmark_claim(policy, comparisons).value,
            "cross_platform_memory_comparison": "forbidden",
            "fixture_chunk_bytes": str(STREAM_CHUNK_BYTES),
            "fixture_object_bytes": str(policy.object_bytes),
            "fixture_object_count": str(policy.object_count),
            "concurrencies": ",".join(str(value) for value in policy.concurrencies),
            "host": platform.platform(),
            "host_monitor_adapter": monitor_adapter.name,
            "host_monitor_interval_seconds": (
                f"{host_noise.SAMPLE_INTERVAL_SECONDS:.3f}"
            ),
            "host_noise_cpu_policy": "diagnostic-only-no-numeric-threshold",
            "host_noise_pressure_policy": "normal-only",
            "memory_metric": next(iter(memory_metrics)),
            "ordering": "sha256-block-rounds-adjacent-balanced",
            "schedule_binding_sha256": schedule_binding,
            "repeats": str(policy.repeats),
            "timeout_seconds": canonical_timeout_seconds(
                policy.child_timeout_seconds
            ),
            "warmups": str(policy.warmups),
            "git_lfs_role": "pinned-git-lfs",
            "git_lfs_sha256": git_lfs.sha256,
            "git_lfs_version": git_lfs.version,
            "git_role": "source-inspector-git",
            "git_sha256": git.sha256,
            "git_version": git.version,
            "python_role": "release-builder-python",
            "python_sha256": provenance.python_sha256,
            "python_version": provenance.python_version,
            "zmin_role": "canonical-release-zmin",
            "zmin_sha256": zmin.sha256,
            "zmin_version": zmin.version,
        }
        metadata.update(release_provenance_metadata(provenance))
        def final_release_check() -> None:
            verify_release_inputs_unchanged(provenance, zmin, git, git_lfs)

        publish_evidence_with_failure_custody(
            output_root,
            failure_root,
            rows,
            summaries,
            comparisons,
            metadata,
            invocation,
            final_release_check,
        )
    if policy.strict_gate and any(row.verdict != "pass" for row in comparisons):
        raise BenchmarkError("Zmin did not beat pinned Git LFS in every strict benchmark lane")
    return rows, comparisons


def main(argv: Sequence[str] | None = None) -> int:
    try:
        raw_argv = tuple(sys.argv[1:] if argv is None else argv)
        args = parse_args(raw_argv)
        _rows, comparisons = run_benchmark(args, benchmark_invocation(raw_argv))
        for row in comparisons:
            print(
                f"{row.operation}\tc{row.concurrency}\t{row.verdict}\t"
                f"wall={row.median_wall_ratio:.3f}\t"
                f"p95={row.p95_wall_ratio:.3f}\tmem={row.peak_memory_ratio:.3f}\t"
                f"paired={row.paired_median_wall_ratio:.3f}\t"
                f"paired_u95={row.paired_median_upper_95_ratio}"
            )
        return 0
    except UnicodeError:
        print("lfs-performance-bench: input or tool output is not valid UTF-8", file=sys.stderr)
        return 2
    except (
        BenchmarkError,
        contract.ContractError,
        host_noise.HostNoiseError,
        OSError,
        subprocess.SubprocessError,
    ) as error:
        print(f"lfs-performance-bench: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
