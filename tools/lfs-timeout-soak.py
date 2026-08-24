#!/usr/bin/env python3
"""Reproducible Git LFS idle-activity timeout evidence harness.

The release mode runs an exact, hashed Zmin binary through four local HTTP
Basic-transfer scenarios.  It is deliberately a correctness soak, never a
throughput or memory benchmark.  The unit mode exercises the same fixture and
evidence validation with millisecond-scale timings and an internal reference
client so the harness can be tested without a release build.
"""

from __future__ import annotations

import argparse
import csv
import hashlib
import http.client
import http.server
import io
import json
import os
import pathlib
import re
import secrets
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time
import urllib.parse
from dataclasses import asdict, dataclass, field
from enum import Enum
from stat import S_ISDIR, S_ISREG
from typing import Iterable, Sequence

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import performance_contract as contract


OBJECT_BYTES = b"zminlfs!"
OBJECT_OID = hashlib.sha256(OBJECT_BYTES).hexdigest()
MAX_BATCH_BYTES = 64 * 1024
MAX_CAPTURE_BYTES = 256 * 1024
MAX_OUTPUT_BYTES = 64 * 1024
MAX_REQUEST_PATH_BYTES = 4 * 1024
MAX_RESPONSE_HEADER_BYTES = 16 * 1024
MAX_IDENTITY_SIDECAR_BYTES = 1024 * 1024
MAX_FAILURE_EXCERPT_BYTES = 2 * 1024
FAILURE_ARTIFACT_NAME = "failure.json"
FAILURE_TEMP_PREFIX = ".failure.json."
FAILURE_TEMP_SUFFIX = ".tmp"
FAILURE_TEMP_ATTEMPTS = 32
IS_WINDOWS = os.name == "nt"
ATOMIC_PUBLICATION_THREAT_BOUNDARY = (
    "During publish, failure.json is either the complete expected negative JSON or absent "
    "despite detected path, symlink, rename, temp-name, and content swaps. Arbitrary "
    "same-UID mutation after a successful return is outside this user-space artifact "
    "contract and cannot be prevented."
)
SERVER_JOIN_SECONDS = 10.0
LFS_MEDIA_TYPE = "application/vnd.git-lfs+json"
SCHEMA_VERSION = 1
EXPECTED_BATCH_REF = "refs/heads/main"
ZMIN_VERSION_PREFIX = "git version 2.47.1.zmin (zmin "


class SoakError(RuntimeError):
    """The soak contract, fixture, or child execution failed."""


class FailureArtifactPublicationError(SoakError):
    """Portable atomic failure-artifact publication could not be guaranteed."""


class AtomicEvidencePublicationUnsupported(FailureArtifactPublicationError):
    """The platform cannot provide this failure artifact's atomic contract."""


class EvidenceMode(str, Enum):
    UNIT = "unit"
    RELEASE = "release"


class Operation(str, Enum):
    DOWNLOAD = "download"
    UPLOAD = "upload"


class Behavior(str, Enum):
    PROGRESS = "progress"
    STALL = "stall"


class FailureKind(str, Enum):
    NONE = "none"
    HTTP_CONTROL = "http-400-control"
    PROCESS_DEADLINE = "process-deadline"
    OTHER = "other"


class TimeoutEvidence(str, Enum):
    NONE = "none"
    ACTIVITY_RETRY_BEFORE_CONTROL = "activity-retry-before-control"


class Scenario(str, Enum):
    DOWNLOAD_PROGRESS = "download-progress"
    UPLOAD_PROGRESS = "upload-progress"
    DOWNLOAD_STALL = "download-stall"
    UPLOAD_STALL = "upload-stall"

    @property
    def operation(self) -> Operation:
        if self in {self.DOWNLOAD_PROGRESS, self.DOWNLOAD_STALL}:
            return Operation.DOWNLOAD
        return Operation.UPLOAD

    @property
    def behavior(self) -> Behavior:
        if self in {self.DOWNLOAD_PROGRESS, self.UPLOAD_PROGRESS}:
            return Behavior.PROGRESS
        return Behavior.STALL


SCENARIOS = (
    Scenario.DOWNLOAD_PROGRESS,
    Scenario.UPLOAD_PROGRESS,
    Scenario.DOWNLOAD_STALL,
    Scenario.UPLOAD_STALL,
)


@dataclass(frozen=True)
class TimingProfile:
    mode: EvidenceMode
    activity_seconds: float
    progress_gap_seconds: float
    progress_chunks: int
    stall_seconds: float
    progress_lower_seconds: float
    progress_upper_seconds: float
    stall_lower_seconds: float
    stall_upper_seconds: float
    child_timeout_seconds: float

    @property
    def progress_total_seconds(self) -> float:
        return self.progress_gap_seconds * (self.progress_chunks - 1)

    def validate(self) -> None:
        values = (
            self.activity_seconds,
            self.progress_gap_seconds,
            self.stall_seconds,
            self.progress_lower_seconds,
            self.progress_upper_seconds,
            self.stall_lower_seconds,
            self.stall_upper_seconds,
            self.child_timeout_seconds,
        )
        if any(value <= 0 for value in values):
            raise SoakError("timing profile contains a nonpositive duration")
        if self.progress_chunks < 2:
            raise SoakError("progress profile requires at least two chunks")
        if self.progress_gap_seconds >= self.activity_seconds:
            raise SoakError("progress gaps must remain below the activity timeout")
        if self.progress_total_seconds <= 4 * self.activity_seconds:
            raise SoakError("progress duration must exceed four activity windows")
        if self.progress_total_seconds <= self.progress_lower_seconds:
            raise SoakError("planned progress must complete after its evidence lower bound")
        if self.progress_total_seconds >= self.progress_upper_seconds:
            raise SoakError("planned progress must complete before its evidence upper bound")
        if self.stall_seconds <= self.activity_seconds:
            raise SoakError("stall duration must exceed the activity timeout")
        if not self.progress_lower_seconds < self.progress_upper_seconds:
            raise SoakError("progress evidence window is invalid")
        if not self.stall_lower_seconds < self.stall_upper_seconds:
            raise SoakError("stall evidence window is invalid")
        if self.stall_seconds <= self.stall_upper_seconds:
            raise SoakError("stall control must occur after the accepted timeout window")
        if self.child_timeout_seconds <= max(
            self.progress_upper_seconds, self.stall_upper_seconds
        ):
            raise SoakError("child timeout does not bound the evidence windows")


def timing_profile(mode: EvidenceMode) -> TimingProfile:
    if mode == EvidenceMode.RELEASE:
        profile = TimingProfile(
            mode=mode,
            activity_seconds=30.0,
            progress_gap_seconds=19.0,
            progress_chunks=8,
            stall_seconds=50.0,
            progress_lower_seconds=120.0,
            progress_upper_seconds=155.0,
            stall_lower_seconds=25.0,
            stall_upper_seconds=40.0,
            child_timeout_seconds=170.0,
        )
    else:
        profile = TimingProfile(
            mode=mode,
            activity_seconds=0.100,
            progress_gap_seconds=0.065,
            progress_chunks=8,
            stall_seconds=0.250,
            progress_lower_seconds=0.400,
            progress_upper_seconds=1.5,
            stall_lower_seconds=0.070,
            stall_upper_seconds=0.180,
            child_timeout_seconds=2.0,
        )
    profile.validate()
    return profile


def download_progress_chunks(profile: TimingProfile) -> tuple[bytes, ...]:
    chunks = tuple(bytes((byte,)) for byte in OBJECT_BYTES)
    if len(chunks) != profile.progress_chunks:
        raise SoakError("download progress chunk count does not match the timing profile")
    return chunks


def upload_progress_response_chunks(profile: TimingProfile) -> tuple[bytes, ...]:
    chunks = (
        b"HTTP/1.1 200 OK\r\n",
        b"Content-Type: application/octet-stream\r\n",
        b"Content-Length: 0\r\n",
        b"Connection: close\r\n",
        b"X-Zmin-Progress-A: 1\r\n",
        b"X-Zmin-Progress-B: 1\r\n",
        b"X-Zmin-Progress-C: 1\r\n",
        b"\r\n",
    )
    if len(chunks) != profile.progress_chunks:
        raise SoakError("upload progress chunk count does not match the timing profile")
    return chunks


def first_complete_header_seconds(
    chunks: Sequence[bytes],
    fragment_seconds: Sequence[float],
) -> float:
    if len(chunks) != len(fragment_seconds) or not chunks:
        raise SoakError("response fragment timing evidence is incomplete")
    header = bytearray()
    previous = -1.0
    for chunk, timestamp in zip(chunks, fragment_seconds, strict=True):
        if not chunk or timestamp < 0 or timestamp <= previous:
            raise SoakError("response fragment timing evidence is not monotonic")
        previous = timestamp
        header.extend(chunk)
        if len(header) > MAX_RESPONSE_HEADER_BYTES:
            raise SoakError("response header evidence exceeded its bound")
        if b"\r\n\r\n" in header:
            return timestamp
    raise SoakError("response header terminator was never transmitted")


@dataclass(frozen=True)
class ExecutableIdentity:
    path: pathlib.Path
    sha256: str
    version: str


@dataclass(frozen=True)
class ReleaseSourceIdentity:
    repo_root: pathlib.Path
    source_commit: str
    source_tree: str
    sidecar_path: pathlib.Path
    sidecar_sha256: str


@dataclass(frozen=True)
class FailureTempIdentity:
    device: int
    inode: int
    size: int
    mode: int
    links: int
    owner: int


@dataclass(frozen=True)
class PinnedDirectoryIdentity:
    device: int
    inode: int
    mode: int
    owner: int


@dataclass(frozen=True)
class FixtureEvidence:
    batch_attempts: int
    action_attempts: int
    active_connections: int
    maximum_active_connections: int
    completed: bool
    completed_bytes: int
    upload_attempt_bytes: tuple[int, ...]
    upload_attempt_oids: tuple[str, ...]
    action_start_seconds: tuple[float, ...]
    control_statuses: tuple[tuple[int, int], ...]
    protocol_errors: tuple[str, ...]
    upload_response_fragment_seconds: tuple[float, ...] = ()


@dataclass(frozen=True)
class ChildEvidence:
    exit_code: int
    elapsed_seconds: float
    failure_kind: FailureKind
    stdout_sha256: str
    stderr_sha256: str
    runner_metrics_sha256: str = ""
    runner_metrics: str = ""
    stdout_excerpt: str = ""
    stderr_excerpt: str = ""


@dataclass(frozen=True)
class BoundedProcessEvidence:
    exit_code: int
    elapsed_seconds: float
    stdout_sha256: str
    stderr_sha256: str
    runner_metrics_sha256: str
    runner_metrics: str
    stdout_excerpt: str
    stderr_excerpt: str


@dataclass
class RunDiagnostics:
    mode: str | None = None
    current_scenario: str | None = None
    source: ReleaseSourceIdentity | None = None
    zmin: ExecutableIdentity | None = None
    git: ExecutableIdentity | None = None
    child: ChildEvidence | None = None
    fixture: FixtureEvidence | None = None
    completed_scenarios: list[ScenarioResult] = field(default_factory=list)
    temporary_cleanup: bool | None = None
    residual_processes: int | None = None
    residual_lfs_temp_files: int | None = None


@dataclass(frozen=True)
class ScenarioResult:
    scenario: str
    operation: str
    behavior: str
    mode: str
    activity_seconds: float
    progress_gap_seconds: float
    planned_progress_seconds: float
    stall_seconds: float
    elapsed_seconds: float
    exit_code: int
    failure_kind: str
    timeout_evidence: str
    retry_after_seconds: float | None
    batch_attempts: int
    action_attempts: int
    active_connections: int
    maximum_active_connections: int
    residual_processes: int
    residual_lfs_temp_files: int
    completed: bool
    completed_bytes: int
    object_oid: str
    object_bytes: int
    upload_attempt_bytes: str
    upload_attempt_oids: str
    stdout_sha256: str
    stderr_sha256: str
    verdict: str


@dataclass
class FixtureState:
    scenario: Scenario
    profile: TimingProfile
    lock: threading.Lock = field(default_factory=threading.Lock)
    batch_attempts: int = 0
    action_attempts: int = 0
    active_connections: int = 0
    maximum_active_connections: int = 0
    completed: bool = False
    completed_bytes: int = 0
    upload_attempt_bytes: list[int] = field(default_factory=list)
    upload_attempt_oids: list[str] = field(default_factory=list)
    action_start_seconds: list[float] = field(default_factory=list)
    control_statuses: list[tuple[int, int]] = field(default_factory=list)
    protocol_errors: list[str] = field(default_factory=list)
    upload_response_fragment_seconds: list[float] = field(default_factory=list)
    started: float = field(default_factory=time.monotonic)
    actions_quiesced: threading.Condition = field(init=False)

    def __post_init__(self) -> None:
        self.actions_quiesced = threading.Condition(self.lock)

    def record_batch(self, payload: dict[str, object], headers: http.client.HTTPMessage) -> None:
        expected = {
            "operation": self.scenario.operation.value,
            "transfers": ["basic"],
            "objects": [{"oid": OBJECT_OID, "size": len(OBJECT_BYTES)}],
            "ref": {"name": EXPECTED_BATCH_REF},
            "hash_algo": "sha256",
        }
        if payload != expected:
            raise SoakError("Batch request envelope is not the exact expected Basic request")
        if headers.get("Accept") != LFS_MEDIA_TYPE:
            raise SoakError("Batch request Accept header is not the LFS JSON media type")
        if headers.get("Content-Type") != LFS_MEDIA_TYPE:
            raise SoakError("Batch request Content-Type header is not the LFS JSON media type")
        with self.lock:
            self.batch_attempts += 1
            if self.batch_attempts != 1:
                raise SoakError("scenario issued more than one Batch request")

    def begin_action(self) -> int:
        with self.lock:
            self.action_attempts += 1
            self.action_start_seconds.append(time.monotonic() - self.started)
            self.active_connections += 1
            self.maximum_active_connections = max(
                self.maximum_active_connections, self.active_connections
            )
            return self.action_attempts

    def finish_action(self) -> None:
        with self.actions_quiesced:
            if self.active_connections <= 0:
                self.protocol_errors.append("action connection accounting underflow")
            else:
                self.active_connections -= 1
            self.actions_quiesced.notify_all()

    def wait_for_actions(self, timeout: float) -> bool:
        deadline = time.monotonic() + timeout
        with self.actions_quiesced:
            while self.active_connections:
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    return False
                self.actions_quiesced.wait(remaining)
            return True

    def record_control(self, attempt: int, status: int) -> None:
        with self.lock:
            self.control_statuses.append((attempt, status))

    def record_upload(self, body: bytes) -> None:
        with self.lock:
            self.upload_attempt_bytes.append(len(body))
            self.upload_attempt_oids.append(hashlib.sha256(body).hexdigest())
            if body != OBJECT_BYTES:
                self.protocol_errors.append("upload body failed exact OID/size verification")

    def record_upload_response_fragment(self) -> None:
        with self.lock:
            self.upload_response_fragment_seconds.append(time.monotonic() - self.started)

    def mark_complete(self, amount: int) -> None:
        with self.lock:
            self.completed = True
            self.completed_bytes = amount

    def record_error(self, error: BaseException) -> None:
        message = str(error).splitlines()[0] if str(error) else type(error).__name__
        with self.lock:
            self.protocol_errors.append(message[:256])

    def snapshot(self) -> FixtureEvidence:
        with self.lock:
            return FixtureEvidence(
                batch_attempts=self.batch_attempts,
                action_attempts=self.action_attempts,
                active_connections=self.active_connections,
                maximum_active_connections=self.maximum_active_connections,
                completed=self.completed,
                completed_bytes=self.completed_bytes,
                upload_attempt_bytes=tuple(self.upload_attempt_bytes),
                upload_attempt_oids=tuple(self.upload_attempt_oids),
                action_start_seconds=tuple(self.action_start_seconds),
                control_statuses=tuple(sorted(self.control_statuses)),
                protocol_errors=tuple(self.protocol_errors),
                upload_response_fragment_seconds=tuple(
                    self.upload_response_fragment_seconds
                ),
            )


class SoakHttpServer(http.server.ThreadingHTTPServer):
    address_family = socket.AF_INET
    daemon_threads = True
    block_on_close = False
    request_queue_size = 16

    def __init__(self, state: FixtureState) -> None:
        super().__init__(("127.0.0.1", 0), SoakRequestHandler)
        self.state = state

    def endpoint(self) -> str:
        host, port = self.server_address
        return f"http://{host}:{port}/soak"


class SoakRequestHandler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, _format: str, *_arguments: object) -> None:
        return

    def do_POST(self) -> None:
        self._dispatch()

    def do_GET(self) -> None:
        self._dispatch()

    def do_PUT(self) -> None:
        self._dispatch()

    def _dispatch(self) -> None:
        server = self.server
        if not isinstance(server, SoakHttpServer):
            self._error_response(500)
            return
        state = server.state
        try:
            if len(self.path.encode("utf-8", "surrogatepass")) > MAX_REQUEST_PATH_BYTES:
                raise SoakError("request path exceeded its bound")
            parsed = urllib.parse.urlsplit(self.path)
            if parsed.query or parsed.fragment:
                raise SoakError("fixture request URL contains query or fragment")
            if self.command == "POST" and parsed.path == "/soak/objects/batch":
                self._batch(server, state)
                return
            if parsed.path != f"/soak/objects/{OBJECT_OID}":
                raise SoakError("fixture received an unsupported action path")
            expected_method = "GET" if state.scenario.operation == Operation.DOWNLOAD else "PUT"
            if self.command != expected_method:
                raise SoakError("fixture received the wrong action method")
            attempt = state.begin_action()
            try:
                if state.scenario.operation == Operation.DOWNLOAD:
                    self._download(state, attempt)
                else:
                    self._upload(state, attempt)
            finally:
                state.finish_action()
        except (BrokenPipeError, ConnectionError, OSError):
            # A timed-out first stall attempt is expected to close its socket.
            return
        except BaseException as error:
            state.record_error(error)
            self._error_response(500)

    def _batch(self, server: SoakHttpServer, state: FixtureState) -> None:
        body = self._read_bounded_body(MAX_BATCH_BYTES)
        try:
            payload = json.loads(body)
        except (UnicodeDecodeError, json.JSONDecodeError) as error:
            raise SoakError("Batch request is not valid bounded JSON") from error
        if not isinstance(payload, dict):
            raise SoakError("Batch request must be an object")
        state.record_batch(payload, self.headers)
        action = state.scenario.operation.value
        response = json.dumps(
            {
                "transfer": "basic",
                "hash_algo": "sha256",
                "objects": [
                    {
                        "oid": OBJECT_OID,
                        "size": len(OBJECT_BYTES),
                        "authenticated": True,
                        "actions": {
                            action: {
                                "href": f"{server.endpoint()}/objects/{OBJECT_OID}"
                            }
                        },
                    }
                ],
            },
            ensure_ascii=True,
            separators=(",", ":"),
            sort_keys=True,
        ).encode("ascii")
        self._write_response(200, response, LFS_MEDIA_TYPE)

    def _download(self, state: FixtureState, attempt: int) -> None:
        if state.scenario.behavior == Behavior.STALL:
            if attempt == 1:
                time.sleep(state.profile.stall_seconds)
                state.record_control(attempt, 408)
                self._write_response(408, b"", "text/plain")
            else:
                state.record_control(attempt, 400)
                self._write_response(400, b"", "text/plain")
            return
        if attempt != 1:
            raise SoakError("progress download was retried")
        self.close_connection = True
        self.send_response(200)
        self.send_header("Content-Type", "application/octet-stream")
        self.send_header("Content-Length", str(len(OBJECT_BYTES)))
        self.send_header("Connection", "close")
        self.end_headers()
        for index, chunk in enumerate(download_progress_chunks(state.profile)):
            if index:
                time.sleep(state.profile.progress_gap_seconds)
            self.wfile.write(chunk)
            self.wfile.flush()
        state.mark_complete(len(OBJECT_BYTES))

    def _upload(self, state: FixtureState, attempt: int) -> None:
        body = self._read_bounded_body(len(OBJECT_BYTES))
        state.record_upload(body)
        if body != OBJECT_BYTES:
            self._write_response(422, b"", "text/plain")
            return
        if state.scenario.behavior == Behavior.STALL:
            if attempt == 1:
                time.sleep(state.profile.stall_seconds)
                state.record_control(attempt, 408)
                self._write_response(408, b"", "text/plain")
            else:
                state.record_control(attempt, 400)
                self._write_response(400, b"", "text/plain")
            return
        if attempt != 1:
            raise SoakError("progress upload was retried")
        self.close_connection = True
        for index, chunk in enumerate(upload_progress_response_chunks(state.profile)):
            if index:
                time.sleep(state.profile.progress_gap_seconds)
            self.wfile.write(chunk)
            self.wfile.flush()
            state.record_upload_response_fragment()
        state.mark_complete(len(body))

    def _content_length(self) -> int:
        if self.headers.get("Transfer-Encoding") is not None:
            raise SoakError("chunked fixture requests are unsupported")
        raw = self.headers.get("Content-Length")
        if raw is None or not raw.isdecimal():
            raise SoakError("fixture requires a decimal Content-Length")
        return int(raw)

    def _read_bounded_body(self, maximum: int) -> bytes:
        length = self._content_length()
        if length > maximum:
            raise SoakError("request body exceeded its bound")
        body = self.rfile.read(length)
        if len(body) != length:
            raise SoakError("request body was truncated")
        return body

    def _write_response(self, status: int, body: bytes, content_type: str) -> None:
        self.close_connection = True
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Connection", "close")
        self.end_headers()
        if body:
            self.wfile.write(body)
        self.wfile.flush()

    def _error_response(self, status: int) -> None:
        try:
            self._write_response(status, b"fixture error\n", "text/plain")
        except (BrokenPipeError, ConnectionError, OSError):
            pass


class FixtureServer:
    def __init__(self, state: FixtureState) -> None:
        self.server = SoakHttpServer(state)
        self.thread = threading.Thread(
            target=self.server.serve_forever,
            name=f"zmin-lfs-timeout-{state.scenario.value}",
        )

    def __enter__(self) -> FixtureServer:
        self.thread.start()
        return self

    def __exit__(self, _kind: object, _value: object, _traceback: object) -> None:
        failure: SoakError | None = None
        try:
            self.server.shutdown()
            self.thread.join(timeout=SERVER_JOIN_SECONDS)
            if self.thread.is_alive():
                failure = SoakError("fixture server thread did not terminate")
            else:
                action_timeout = self.server.state.profile.stall_seconds + SERVER_JOIN_SECONDS
                if not self.server.state.wait_for_actions(action_timeout):
                    failure = SoakError(
                        "fixture action handlers did not terminate within their bound"
                    )
        finally:
            self.server.server_close()
        if failure is not None:
            raise failure

    @property
    def endpoint(self) -> str:
        return self.server.endpoint()


def validate_sha256(value: str, label: str) -> str:
    if len(value) != 64 or value.lower() != value:
        raise SoakError(f"{label} SHA-256 must be 64 lowercase hexadecimal characters")
    try:
        bytes.fromhex(value)
    except ValueError as error:
        raise SoakError(f"{label} SHA-256 is invalid") from error
    return value


def hash_file(path: pathlib.Path, maximum: int | None = None) -> tuple[str, int]:
    digest = hashlib.sha256()
    total = 0
    with path.open("rb", buffering=0) as source:
        while True:
            chunk = source.read(128 * 1024)
            if not chunk:
                break
            total += len(chunk)
            if maximum is not None and total > maximum:
                raise SoakError("bounded file exceeded its maximum size")
            digest.update(chunk)
    return digest.hexdigest(), total


def executable_identity(
    path: pathlib.Path,
    expected_sha256: str,
    version_args: Sequence[str],
    version_prefix: str | None,
) -> ExecutableIdentity:
    expected = validate_sha256(expected_sha256, "executable")
    resolved = contract.absolute_file(path, executable=True, reject_symlinks=True)
    actual, _ = hash_file(resolved)
    if actual != expected:
        raise SoakError("executable SHA-256 does not match its pinned identity")
    with tempfile.TemporaryDirectory(prefix="zmin-lfs-version-probe-") as directory:
        root = pathlib.Path(directory)
        process = run_bounded_process(
            root / "process",
            root,
            os.environ.copy(),
            15.0,
            (str(resolved), *version_args),
        )
        stdout = bounded_bytes(root / "process" / "stdout")
        stderr = bounded_bytes(root / "process" / "stderr")
    if process.exit_code != 0 or len(stdout) > 16 * 1024 or len(stderr) > 16 * 1024:
        raise SoakError("executable version probe failed")
    version = stdout.decode("utf-8", "strict").strip()
    if version_prefix is not None and not version.startswith(version_prefix):
        raise SoakError("executable version does not match its required identity")
    return ExecutableIdentity(resolved, actual, version)


def release_source_identity(
    repo_root: pathlib.Path,
    source_commit: str,
    git: ExecutableIdentity,
    zmin: ExecutableIdentity,
) -> ReleaseSourceIdentity:
    root = contract.absolute_directory(repo_root, reject_symlinks=True)
    expected_binary = contract.canonical_release_binary(root)
    expected_binary = contract.absolute_file(
        expected_binary,
        executable=True,
        reject_symlinks=True,
    )
    if zmin.path != expected_binary:
        raise SoakError("Zmin is not the canonical release-profile binary")
    if not zmin.version.startswith(ZMIN_VERSION_PREFIX) or not zmin.version.endswith(")"):
        raise SoakError("Zmin version does not match the required compatibility identity")
    sidecar_path = contract.identity_sidecar(expected_binary)
    try:
        snapshot = contract.read_file_snapshot(
            sidecar_path,
            max_bytes=MAX_IDENTITY_SIDECAR_BYTES,
            reject_symlinks=True,
        )
        sidecar_path = snapshot["path"]
        sidecar_sha256 = snapshot["sha256"]
        sidecar = contract.load_json_bytes(snapshot["data"], sidecar_path)
    except (contract.ContractError, OSError) as error:
        raise SoakError("sanitized release identity sidecar is missing or invalid") from error
    try:
        state = contract.repo_state(root, git.path)
        source_tree = contract.git_value(git.path, root, "rev-parse", "HEAD^{tree}")
    except (contract.ContractError, OSError) as error:
        raise SoakError("release source archive identity could not be verified") from error
    if state.get("dirty") is not False or state.get("commit") != source_commit:
        raise SoakError("release source archive does not match the clean pinned commit")
    if len(source_tree) != 40 or any(byte not in "0123456789abcdef" for byte in source_tree):
        raise SoakError("release source archive tree identity is invalid")
    python_data = sidecar.get("python")
    make_data = sidecar.get("make")
    if not isinstance(python_data, dict) or not isinstance(make_data, dict):
        raise SoakError("sanitized release identity sidecar is incomplete")
    try:
        python_path = contract.absolute_file(
            pathlib.Path(python_data["path"]),
            executable=True,
            reject_symlinks=True,
        )
        make_path = contract.absolute_file(
            pathlib.Path(make_data["path"]),
            executable=True,
            reject_symlinks=True,
        )
        lock_path = contract.absolute_file(root / "Cargo.lock", reject_symlinks=True)
        matched, _detail = contract.sidecar_matches(
            sidecar,
            repo_root=root,
            git_bin=git.path,
            binary=zmin.path,
            profile=contract.AUTHORITATIVE_PROFILE,
            state=state,
            cargo_lock_sha256=contract.sha256_file(lock_path),
            python_bin=python_path,
            make_bin=make_path,
        )
    except (KeyError, TypeError, contract.ContractError, OSError) as error:
        raise SoakError("sanitized release identity sidecar could not be verified") from error
    if not matched:
        raise SoakError("sanitized release identity sidecar does not match current inputs")
    return ReleaseSourceIdentity(
        root,
        source_commit,
        source_tree,
        sidecar_path,
        sidecar_sha256,
    )


def bounded_hash(path: pathlib.Path) -> str:
    digest, _ = hash_file(path, MAX_CAPTURE_BYTES)
    return digest


def bounded_bytes(path: pathlib.Path) -> bytes:
    with path.open("rb", buffering=0) as source:
        data = source.read(MAX_CAPTURE_BYTES + 1)
    if len(data) > MAX_CAPTURE_BYTES:
        raise SoakError("child output exceeded its bound")
    return data


def sanitized_environment(
    root: pathlib.Path,
    git: ExecutableIdentity,
    zmin: ExecutableIdentity,
) -> dict[str, str]:
    home = root / "home"
    home.mkdir(exist_ok=True)
    global_config = root / "global.gitconfig"
    global_config.write_bytes(b"")
    path_parts = [
        str(zmin.path.parent),
        str(git.path.parent),
        str(pathlib.Path(sys.executable).parent),
    ]
    environment = {
        "PATH": os.pathsep.join(path_parts),
        "HOME": str(home),
        "TMPDIR": str(root),
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
    for key in ("SYSTEMROOT", "WINDIR", "COMSPEC", "PATHEXT"):
        value = os.environ.get(key)
        if value:
            environment[key] = value
    if os.name == "nt":
        environment["TEMP"] = str(root)
        environment["TMP"] = str(root)
    return environment


def run_git(
    git: ExecutableIdentity,
    environment: dict[str, str],
    *arguments: str,
) -> bytes:
    temporary_parent = pathlib.Path(environment["TMPDIR"])
    with tempfile.TemporaryDirectory(prefix="git-setup-", dir=temporary_parent) as directory:
        root = pathlib.Path(directory)
        process = run_bounded_process(
            root / "process",
            root,
            environment,
            30.0,
            (str(git.path), *arguments),
        )
        stdout = bounded_bytes(root / "process" / "stdout")
    if process.exit_code != 0:
        raise SoakError("Git fixture setup failed")
    return stdout


def create_repository(
    root: pathlib.Path,
    endpoint: str,
    scenario: Scenario,
    profile: TimingProfile,
    git: ExecutableIdentity,
    environment: dict[str, str],
) -> pathlib.Path:
    repository = root / "repo"
    repository.mkdir()
    run_git(
        git,
        environment,
        "-C",
        str(repository),
        "init",
        "--quiet",
        "--initial-branch=main",
        "--template=",
    )
    run_git(git, environment, "-C", str(repository), "config", "user.name", "LFS Timeout Soak")
    run_git(
        git,
        environment,
        "-C",
        str(repository),
        "config",
        "user.email",
        "lfs-timeout@example.invalid",
    )
    pointer = (
        "version https://git-lfs.github.com/spec/v1\n"
        f"oid sha256:{OBJECT_OID}\n"
        f"size {len(OBJECT_BYTES)}\n"
    )
    (repository / "object.bin").write_text(pointer, encoding="utf-8", newline="\n")
    (repository / ".gitattributes").write_text(
        "*.bin filter=lfs diff=lfs merge=lfs -text\n",
        encoding="utf-8",
        newline="\n",
    )
    run_git(git, environment, "-C", str(repository), "add", "--", "object.bin", ".gitattributes")
    run_git(git, environment, "-C", str(repository), "commit", "--quiet", "-m", "LFS timeout pointer")
    run_git(
        git,
        environment,
        "-C",
        str(repository),
        "remote",
        "add",
        "origin",
        "http://127.0.0.1/timeout/repo.git",
    )
    settings = (
        ("lfs.url", endpoint),
        ("lfs.concurrenttransfers", "1"),
        ("lfs.activitytimeout", str(int(profile.activity_seconds))),
        ("lfs.dialtimeout", "30"),
        ("lfs.tlstimeout", "30"),
        (f"lfs.{endpoint}.access", "none"),
        (f"lfs.{endpoint}.locksverify", "false"),
    )
    for key, value in settings:
        run_git(git, environment, "-C", str(repository), "config", key, value)
    if scenario.operation == Operation.UPLOAD:
        destination = (
            repository
            / ".git"
            / "lfs"
            / "objects"
            / OBJECT_OID[:2]
            / OBJECT_OID[2:4]
            / OBJECT_OID
        )
        destination.parent.mkdir(parents=True)
        destination.write_bytes(OBJECT_BYTES)
    return repository


def verify_download_store(repository: pathlib.Path, expected_present: bool) -> None:
    object_root = repository / ".git" / "lfs" / "objects"
    destination = object_root / OBJECT_OID[:2] / OBJECT_OID[2:4] / OBJECT_OID
    files = set()
    if object_root.exists():
        files = {path.resolve() for path in object_root.rglob("*") if path.is_file()}
    if not expected_present:
        if files:
            raise SoakError("failed download left a published or temporary object file")
        return
    digest, size = hash_file(destination)
    if digest != OBJECT_OID or size != len(OBJECT_BYTES):
        raise SoakError("download store failed exact OID/size verification")
    if files != {destination.resolve()}:
        raise SoakError("download store contains an unexpected object file")


def verify_lfs_temp_empty(repository: pathlib.Path) -> None:
    temporary = repository / ".git" / "lfs" / "tmp"
    if temporary.exists() and any(path.is_file() for path in temporary.rglob("*")):
        raise SoakError("LFS transfer left a temporary file")


def parse_runner_metrics(path: pathlib.Path) -> tuple[float, str, str]:
    raw = path.read_bytes()
    if len(raw) > 16 * 1024:
        raise SoakError("process metrics exceeded its bound")
    decoded = raw.decode("ascii", "strict")
    fields = decoded.rstrip("\n").split("\t")
    if len(fields) != 14:
        raise SoakError("process metrics schema is invalid")
    try:
        elapsed = float(fields[0])
    except ValueError as error:
        raise SoakError("process elapsed time is invalid") from error
    if elapsed <= 0:
        raise SoakError("process elapsed time must be positive")
    return elapsed, decoded, hashlib.sha256(raw).hexdigest()


def sanitized_diagnostic_excerpt(data: bytes) -> str:
    bounded = data[:MAX_FAILURE_EXCERPT_BYTES].decode("utf-8", "replace")
    printable = "".join(
        character
        if character in "\n\t" or 0x20 <= ord(character) <= 0x7E
        else "?"
        for character in bounded
    )
    printable = re.sub(
        r"(?im)\b(authorization|proxy-authorization|cookie|set-cookie)\s*[:=][^\r\n]*",
        r"\1: [redacted]",
        printable,
    )
    printable = re.sub(
        r"(?im)\b(password|passwd|token|secret)\s*=\s*\S+",
        r"\1=[redacted]",
        printable,
    )
    printable = re.sub(r"(?i)\b[a-z][a-z0-9+.-]*://\S+", "[url-redacted]", printable)
    printable = re.sub(r"(?i)\b[a-z]:[\\/][^\s]*", "[path-redacted]", printable)
    printable = re.sub(r"\\\\[^\s]+", "[path-redacted]", printable)
    printable = re.sub(r"(?<![A-Za-z0-9])/(?:[^\s]+)", "[path-redacted]", printable)
    return printable[:MAX_FAILURE_EXCERPT_BYTES]


def run_bounded_process(
    artifacts: pathlib.Path,
    cwd: pathlib.Path,
    environment: dict[str, str],
    timeout_seconds: float,
    command: Sequence[str],
) -> BoundedProcessEvidence:
    artifacts.mkdir()
    identity = contract.artifact_identity_token(contract.path_identity(artifacts))
    stdout = artifacts / "stdout"
    stderr = artifacts / "stderr"
    metrics = artifacts / "metrics"
    runner = contract.absolute_file(
        pathlib.Path(__file__).with_name("git-bench-process.py"),
        executable=True,
        reject_symlinks=True,
    )
    invocation = [
        sys.executable,
        "-B",
        str(runner),
        "--artifact-root",
        str(artifacts),
        "--artifact-root-identity",
        identity,
        "--stdout",
        str(stdout),
        "--stderr",
        str(stderr),
        "--metrics",
        str(metrics),
        "--timeout-seconds",
        f"{timeout_seconds:.3f}",
        "--max-output-bytes",
        str(MAX_CAPTURE_BYTES),
        "--",
        *command,
    ]
    child_environment = dict(environment)
    child_environment["PYTHONDONTWRITEBYTECODE"] = "1"
    # git-bench-process owns the platform process unit, timeout, live bounded
    # output drains, and descendant cleanup.  Its own diagnostics are trusted,
    # bounded constants and are not evidence, so never retain them in memory.
    result = subprocess.run(
        invocation,
        cwd=cwd,
        env=child_environment,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        check=False,
    )
    elapsed, runner_metrics, runner_metrics_sha256 = parse_runner_metrics(metrics)
    stdout_bytes = bounded_bytes(stdout)
    stderr_bytes = bounded_bytes(stderr)
    return BoundedProcessEvidence(
        exit_code=result.returncode,
        elapsed_seconds=elapsed,
        stdout_sha256=hashlib.sha256(stdout_bytes).hexdigest(),
        stderr_sha256=hashlib.sha256(stderr_bytes).hexdigest(),
        runner_metrics_sha256=runner_metrics_sha256,
        runner_metrics=runner_metrics,
        stdout_excerpt=sanitized_diagnostic_excerpt(stdout_bytes),
        stderr_excerpt=sanitized_diagnostic_excerpt(stderr_bytes),
    )


def run_bounded_zmin(
    root: pathlib.Path,
    repository: pathlib.Path,
    scenario: Scenario,
    profile: TimingProfile,
    zmin: ExecutableIdentity,
    environment: dict[str, str],
) -> ChildEvidence:
    artifacts = root / "process"
    action = "fetch" if scenario.operation == Operation.DOWNLOAD else "push"
    process = run_bounded_process(
        artifacts,
        repository,
        environment,
        profile.child_timeout_seconds,
        (
            str(zmin.path),
            "lfs",
            action,
            "origin",
            "HEAD",
        ),
    )
    child_stderr = bounded_bytes(artifacts / "stderr")
    if process.exit_code == 124:
        failure = FailureKind.PROCESS_DEADLINE
    elif (
        scenario.behavior == Behavior.STALL
        and process.exit_code == 2
        and child_stderr == b"error: LFS transfer failed for 1 object(s)\n"
    ):
        failure = FailureKind.HTTP_CONTROL
    elif process.exit_code == 0:
        failure = FailureKind.NONE
    else:
        failure = FailureKind.OTHER
    return ChildEvidence(
        exit_code=process.exit_code,
        elapsed_seconds=process.elapsed_seconds,
        failure_kind=failure,
        stdout_sha256=process.stdout_sha256,
        stderr_sha256=process.stderr_sha256,
        runner_metrics_sha256=process.runner_metrics_sha256,
        runner_metrics=process.runner_metrics,
        stdout_excerpt=process.stdout_excerpt,
        stderr_excerpt=process.stderr_excerpt,
    )


def batch_action(endpoint: str, scenario: Scenario, timeout: float) -> str:
    parsed = urllib.parse.urlsplit(endpoint)
    connection = http.client.HTTPConnection(parsed.hostname, parsed.port, timeout=timeout)
    body = json.dumps(
        {
            "operation": scenario.operation.value,
            "transfers": ["basic"],
            "objects": [{"oid": OBJECT_OID, "size": len(OBJECT_BYTES)}],
            "ref": {"name": EXPECTED_BATCH_REF},
            "hash_algo": "sha256",
        },
        separators=(",", ":"),
    ).encode("ascii")
    try:
        connection.request(
            "POST",
            f"{parsed.path}/objects/batch",
            body=body,
            headers={
                "Accept": LFS_MEDIA_TYPE,
                "Content-Type": LFS_MEDIA_TYPE,
                "Content-Length": str(len(body)),
            },
        )
        response = connection.getresponse()
        payload = response.read(MAX_BATCH_BYTES + 1)
        if response.status != 200 or len(payload) > MAX_BATCH_BYTES:
            raise SoakError("reference Batch request failed")
        decoded = json.loads(payload)
        return decoded["objects"][0]["actions"][scenario.operation.value]["href"]
    finally:
        connection.close()


def reference_action(action: str, scenario: Scenario, profile: TimingProfile) -> ChildEvidence:
    parsed = urllib.parse.urlsplit(action)
    started = time.monotonic()
    exit_code = 0
    failure = FailureKind.NONE
    digest = hashlib.sha256()
    received = 0
    attempts = 2 if scenario.behavior == Behavior.STALL else 1
    for attempt in range(1, attempts + 1):
        connection = http.client.HTTPConnection(
            parsed.hostname,
            parsed.port,
            timeout=profile.activity_seconds,
        )
        try:
            if scenario.operation == Operation.DOWNLOAD:
                connection.request("GET", parsed.path)
            else:
                connection.request(
                    "PUT",
                    parsed.path,
                    body=OBJECT_BYTES,
                    headers={"Content-Length": str(len(OBJECT_BYTES))},
                )
            response = connection.getresponse()
            if scenario.behavior == Behavior.STALL:
                if attempt != 2 or response.status != 400:
                    raise SoakError("stall control response was not the expected HTTP 400")
                exit_code = 2
                break
            if response.status != 200:
                raise SoakError("progress action did not return HTTP 200")
            while True:
                chunk = response.read(1)
                if not chunk:
                    break
                digest.update(chunk)
                received += len(chunk)
        except (TimeoutError, socket.timeout):
            if scenario.behavior != Behavior.STALL or attempt != 1:
                raise
            exit_code = 2
        finally:
            connection.close()
    if scenario.operation == Operation.DOWNLOAD and scenario.behavior == Behavior.PROGRESS:
        if received != len(OBJECT_BYTES) or digest.hexdigest() != OBJECT_OID:
            raise SoakError("reference download failed exact OID/size verification")
    elapsed = time.monotonic() - started
    if scenario.behavior == Behavior.STALL and exit_code != 0:
        failure = FailureKind.HTTP_CONTROL
    return ChildEvidence(
        exit_code=exit_code,
        elapsed_seconds=elapsed,
        failure_kind=failure,
        stdout_sha256=hashlib.sha256(b"").hexdigest(),
        stderr_sha256=hashlib.sha256(b"").hexdigest(),
    )


def validate_scenario(
    scenario: Scenario,
    profile: TimingProfile,
    child: ChildEvidence,
    fixture: FixtureEvidence,
) -> tuple[str, tuple[str, ...]]:
    errors: list[str] = list(fixture.protocol_errors)
    expected_attempts = 1 if scenario.behavior == Behavior.PROGRESS else 2
    if fixture.batch_attempts != 1:
        errors.append("expected exactly one Batch request")
    if fixture.action_attempts != expected_attempts:
        errors.append("action attempt count does not prove the scenario")
    if fixture.active_connections != 0:
        errors.append("fixture retained an action connection")
    expected_maximum_active = 1 if scenario.behavior == Behavior.PROGRESS else 2
    if fixture.maximum_active_connections != expected_maximum_active:
        errors.append("action overlap does not prove timeout-before-control retry ordering")
    if scenario.behavior == Behavior.PROGRESS:
        if child.exit_code != 0 or child.failure_kind != FailureKind.NONE:
            errors.append("progress scenario did not succeed")
        if not profile.progress_lower_seconds < child.elapsed_seconds < profile.progress_upper_seconds:
            errors.append("progress scenario elapsed outside its monotonic evidence window")
        if not fixture.completed or fixture.completed_bytes != len(OBJECT_BYTES):
            errors.append("progress scenario did not complete the exact object")
        if len(fixture.action_start_seconds) != 1 or fixture.control_statuses:
            errors.append("progress scenario has invalid action timing/control evidence")
    else:
        if child.exit_code == 0 or child.failure_kind != FailureKind.HTTP_CONTROL:
            errors.append("stall scenario did not terminate on the exact HTTP control failure")
        if not profile.stall_lower_seconds < child.elapsed_seconds < profile.stall_upper_seconds:
            errors.append("stall scenario elapsed outside its monotonic evidence window")
        if fixture.completed or fixture.completed_bytes != 0:
            errors.append("stall scenario was incorrectly marked complete")
        if fixture.control_statuses != ((1, 408), (2, 400)):
            errors.append("stall scenario control responses are incomplete or unexpected")
        if len(fixture.action_start_seconds) != 2:
            errors.append("stall scenario does not have two action start timestamps")
        else:
            retry_after = fixture.action_start_seconds[1] - fixture.action_start_seconds[0]
            if not profile.stall_lower_seconds < retry_after < profile.stall_upper_seconds:
                errors.append("retry did not begin inside the activity-timeout evidence window")
            if retry_after >= profile.stall_seconds:
                errors.append("retry did not begin before the delayed control response")
    if scenario.operation == Operation.UPLOAD:
        expected_uploads = expected_attempts
        if fixture.upload_attempt_bytes != (len(OBJECT_BYTES),) * expected_uploads:
            errors.append("upload attempts did not each contain the exact size")
        if fixture.upload_attempt_oids != (OBJECT_OID,) * expected_uploads:
            errors.append("upload attempts did not each contain the exact OID")
    elif fixture.upload_attempt_bytes or fixture.upload_attempt_oids:
        errors.append("download scenario unexpectedly recorded upload bytes")
    if scenario == Scenario.UPLOAD_PROGRESS:
        if len(fixture.action_start_seconds) != 1:
            errors.append("upload progress lacks its action start timestamp")
        else:
            relative_fragments = tuple(
                timestamp - fixture.action_start_seconds[0]
                for timestamp in fixture.upload_response_fragment_seconds
            )
            try:
                header_complete = first_complete_header_seconds(
                    upload_progress_response_chunks(profile),
                    relative_fragments,
                )
            except SoakError as error:
                errors.append(str(error))
            else:
                gaps = tuple(
                    later - earlier
                    for earlier, later in zip(
                        relative_fragments,
                        relative_fragments[1:],
                    )
                )
                if not profile.progress_lower_seconds < header_complete:
                    errors.append("upload response headers completed before the lower bound")
                if header_complete >= profile.progress_upper_seconds:
                    errors.append("upload response headers completed after the upper bound")
                if any(gap <= 0 or gap >= profile.activity_seconds for gap in gaps):
                    errors.append("upload response TCP progress gap violated its bound")
    elif fixture.upload_response_fragment_seconds:
        errors.append("non-progress scenario recorded upload response fragments")
    return ("pass" if not errors else "fail", tuple(errors))


def scenario_result(
    scenario: Scenario,
    profile: TimingProfile,
    child: ChildEvidence,
    fixture: FixtureEvidence,
) -> tuple[ScenarioResult, tuple[str, ...]]:
    verdict, errors = validate_scenario(scenario, profile, child, fixture)
    retry_after = (
        fixture.action_start_seconds[1] - fixture.action_start_seconds[0]
        if len(fixture.action_start_seconds) == 2
        else None
    )
    timeout_evidence = (
        TimeoutEvidence.ACTIVITY_RETRY_BEFORE_CONTROL
        if scenario.behavior == Behavior.STALL and verdict == "pass"
        else TimeoutEvidence.NONE
    )
    return (
        ScenarioResult(
            scenario=scenario.value,
            operation=scenario.operation.value,
            behavior=scenario.behavior.value,
            mode=profile.mode.value,
            activity_seconds=profile.activity_seconds,
            progress_gap_seconds=profile.progress_gap_seconds,
            planned_progress_seconds=profile.progress_total_seconds,
            stall_seconds=profile.stall_seconds,
            elapsed_seconds=child.elapsed_seconds,
            exit_code=child.exit_code,
            failure_kind=child.failure_kind.value,
            timeout_evidence=timeout_evidence.value,
            retry_after_seconds=retry_after,
            batch_attempts=fixture.batch_attempts,
            action_attempts=fixture.action_attempts,
            active_connections=fixture.active_connections,
            maximum_active_connections=fixture.maximum_active_connections,
            residual_processes=0,
            residual_lfs_temp_files=0,
            completed=fixture.completed,
            completed_bytes=fixture.completed_bytes,
            object_oid=OBJECT_OID,
            object_bytes=len(OBJECT_BYTES),
            upload_attempt_bytes=",".join(str(value) for value in fixture.upload_attempt_bytes),
            upload_attempt_oids=",".join(fixture.upload_attempt_oids),
            stdout_sha256=child.stdout_sha256,
            stderr_sha256=child.stderr_sha256,
            verdict=verdict,
        ),
        errors,
    )


def run_unit_scenario(scenario: Scenario, profile: TimingProfile) -> ScenarioResult:
    state = FixtureState(scenario, profile)
    with FixtureServer(state) as fixture:
        action = batch_action(fixture.endpoint, scenario, profile.activity_seconds)
        child = reference_action(action, scenario, profile)
    evidence = state.snapshot()
    result, errors = scenario_result(scenario, profile, child, evidence)
    if errors:
        raise SoakError(errors[0])
    return result


def run_release_scenario(
    scenario: Scenario,
    profile: TimingProfile,
    root: pathlib.Path,
    git: ExecutableIdentity,
    zmin: ExecutableIdentity,
    diagnostics: RunDiagnostics,
) -> ScenarioResult:
    state = FixtureState(scenario, profile)
    try:
        with FixtureServer(state) as fixture:
            environment = sanitized_environment(root, git, zmin)
            repository = create_repository(
                root,
                fixture.endpoint,
                scenario,
                profile,
                git,
                environment,
            )
            child = run_bounded_zmin(root, repository, scenario, profile, zmin, environment)
            diagnostics.child = child
            diagnostics.residual_processes = 0
            if scenario.operation == Operation.DOWNLOAD:
                verify_download_store(
                    repository,
                    expected_present=(
                        scenario.behavior == Behavior.PROGRESS and child.exit_code == 0
                    ),
                )
            verify_lfs_temp_empty(repository)
            diagnostics.residual_lfs_temp_files = 0
    finally:
        diagnostics.fixture = state.snapshot()
    evidence = diagnostics.fixture
    result, errors = scenario_result(scenario, profile, child, evidence)
    if errors:
        raise SoakError(errors[0])
    return result


def tsv_bytes(rows: Iterable[ScenarioResult]) -> bytes:
    output = io.StringIO(newline="")
    fields = list(ScenarioResult.__dataclass_fields__)
    writer = csv.DictWriter(
        output,
        fieldnames=fields,
        delimiter="\t",
        lineterminator="\n",
        extrasaction="raise",
    )
    writer.writeheader()
    for row in rows:
        values = asdict(row)
        for key in (
            "activity_seconds",
            "progress_gap_seconds",
            "planned_progress_seconds",
            "stall_seconds",
            "elapsed_seconds",
        ):
            values[key] = f"{values[key]:.9f}"
        retry_after = values["retry_after_seconds"]
        values["retry_after_seconds"] = (
            "" if retry_after is None else f"{retry_after:.9f}"
        )
        writer.writerow(values)
    encoded = output.getvalue().encode("utf-8")
    if len(encoded) > MAX_OUTPUT_BYTES:
        raise SoakError("TSV evidence exceeded its bound")
    return encoded


def json_bytes(metadata: dict[str, object], rows: Sequence[ScenarioResult]) -> bytes:
    encoded = (
        json.dumps(
            {"metadata": metadata, "scenarios": [asdict(row) for row in rows]},
            ensure_ascii=True,
            separators=(",", ":"),
            sort_keys=True,
        )
        + "\n"
    ).encode("ascii")
    if len(encoded) > MAX_OUTPUT_BYTES:
        raise SoakError("JSON evidence exceeded its bound")
    return encoded


def write_evidence(
    output: pathlib.Path,
    metadata: dict[str, object],
    rows: Sequence[ScenarioResult],
) -> None:
    output = contract.absolute_directory(output, reject_symlinks=True)
    if any(output.iterdir()):
        raise SoakError("output directory must exist and be empty")
    identity = contract.path_identity(output)
    contract.artifact_write_bytes(
        output,
        "results.tsv",
        tsv_bytes(rows),
        exclusive=True,
        expected_directory_identity=identity,
    )
    contract.artifact_write_bytes(
        output,
        "metadata.json",
        json_bytes(metadata, rows),
        exclusive=True,
        expected_directory_identity=identity,
    )


def failure_identity(diagnostics: RunDiagnostics) -> dict[str, object] | None:
    if diagnostics.source is None or diagnostics.zmin is None or diagnostics.git is None:
        return None
    return {
        "verified": True,
        "source_commit": diagnostics.source.source_commit,
        "source_tree": diagnostics.source.source_tree,
        "repo_root": str(diagnostics.source.repo_root),
        "identity_sidecar": str(diagnostics.source.sidecar_path),
        "identity_sidecar_sha256": diagnostics.source.sidecar_sha256,
        "zmin_path": str(diagnostics.zmin.path),
        "zmin_sha256": diagnostics.zmin.sha256,
        "zmin_version": diagnostics.zmin.version,
        "git_path": str(diagnostics.git.path),
        "git_sha256": diagnostics.git.sha256,
        "git_version": diagnostics.git.version,
    }


def failure_json_bytes(
    diagnostics: RunDiagnostics,
    error: BaseException,
) -> bytes:
    child = None
    if diagnostics.child is not None:
        child = {
            "exit_code": diagnostics.child.exit_code,
            "elapsed_seconds": diagnostics.child.elapsed_seconds,
            "failure_kind": diagnostics.child.failure_kind.value,
            "runner_metrics_sha256": diagnostics.child.runner_metrics_sha256,
            "runner_metrics": diagnostics.child.runner_metrics,
            "stdout_sha256": diagnostics.child.stdout_sha256,
            "stderr_sha256": diagnostics.child.stderr_sha256,
            "stdout_excerpt": sanitized_diagnostic_excerpt(
                diagnostics.child.stdout_excerpt.encode("ascii", "replace")
            ),
            "stderr_excerpt": sanitized_diagnostic_excerpt(
                diagnostics.child.stderr_excerpt.encode("ascii", "replace")
            ),
        }
    fixture = asdict(diagnostics.fixture) if diagnostics.fixture is not None else None
    if fixture is not None:
        fixture["protocol_errors"] = [
            sanitized_diagnostic_excerpt(str(message).encode("utf-8", "replace"))
            for message in fixture["protocol_errors"]
        ]
    payload = {
        "schema_version": SCHEMA_VERSION,
        "claim": "timeout-contract-not-established",
        "verdict": "failure",
        "mode": diagnostics.mode,
        "scenario": diagnostics.current_scenario,
        "error_type": type(error).__name__,
        "error_excerpt": sanitized_diagnostic_excerpt(
            str(error).encode("utf-8", "replace")
        ),
        "identity": failure_identity(diagnostics),
        "child_runner": child,
        "fixture": fixture,
        "completed_scenarios": [
            asdict(scenario) for scenario in diagnostics.completed_scenarios
        ],
        "cleanup": {
            "temporary_root_removed": diagnostics.temporary_cleanup,
            "residual_processes": diagnostics.residual_processes,
            "residual_lfs_temp_files": diagnostics.residual_lfs_temp_files,
        },
    }
    encoded = (
        json.dumps(payload, ensure_ascii=True, separators=(",", ":"), sort_keys=True)
        + "\n"
    ).encode("ascii")
    if len(encoded) > MAX_OUTPUT_BYTES:
        raise SoakError("failure evidence exceeded its bound")
    return encoded


def _directory_identity(status: os.stat_result) -> PinnedDirectoryIdentity:
    if not S_ISDIR(status.st_mode):
        raise FailureArtifactPublicationError("failure artifact parent is not a directory")
    return PinnedDirectoryIdentity(
        device=status.st_dev,
        inode=status.st_ino,
        mode=status.st_mode,
        owner=status.st_uid,
    )


def _directory_matches(
    status: os.stat_result,
    identity: PinnedDirectoryIdentity,
) -> bool:
    return (
        S_ISDIR(status.st_mode)
        and status.st_dev == identity.device
        and status.st_ino == identity.inode
        and status.st_mode == identity.mode
        and status.st_uid == identity.owner
    )


def _failure_directory_flags() -> int:
    directory = getattr(os, "O_DIRECTORY", 0)
    no_follow = getattr(os, "O_NOFOLLOW", 0)
    if not directory or not no_follow:
        raise AtomicEvidencePublicationUnsupported(
            "platform lacks pinned no-follow directory support"
        )
    return os.O_RDONLY | directory | no_follow | getattr(os, "O_CLOEXEC", 0)


def _open_pinned_failure_directory(
    output: pathlib.Path,
) -> tuple[int, PinnedDirectoryIdentity]:
    if IS_WINDOWS:
        raise AtomicEvidencePublicationUnsupported(
            "atomic failure evidence publication is unsupported on Windows"
        )
    if (
        os.open not in os.supports_dir_fd
        or os.stat not in os.supports_dir_fd
        or os.stat not in os.supports_follow_symlinks
        or os.link not in os.supports_dir_fd
        or os.link not in os.supports_follow_symlinks
        or os.unlink not in os.supports_dir_fd
    ):
        raise AtomicEvidencePublicationUnsupported(
            "platform lacks descriptor-relative publication support"
        )
    try:
        path_identity = _directory_identity(output.lstat())
        descriptor = os.open(output, _failure_directory_flags())
    except FailureArtifactPublicationError:
        raise
    except OSError as error:
        raise FailureArtifactPublicationError(
            "failure artifact directory could not be pinned"
        ) from error
    descriptor_status = os.fstat(descriptor)
    if not _directory_matches(descriptor_status, path_identity):
        os.close(descriptor)
        raise FailureArtifactPublicationError("failure artifact directory identity changed")
    return descriptor, path_identity


def _revalidate_pinned_directory(
    output: pathlib.Path,
    directory_descriptor: int,
    identity: PinnedDirectoryIdentity,
) -> None:
    try:
        if not _directory_matches(os.fstat(directory_descriptor), identity):
            raise FailureArtifactPublicationError(
                "pinned failure artifact directory metadata changed"
            )
        if not _directory_matches(output.lstat(), identity):
            raise FailureArtifactPublicationError(
                "failure artifact path no longer names the pinned directory"
            )
        verification_descriptor = os.open(output, _failure_directory_flags())
        try:
            if not _directory_matches(os.fstat(verification_descriptor), identity):
                raise FailureArtifactPublicationError(
                    "failure artifact path resolved to a different directory"
                )
        finally:
            os.close(verification_descriptor)
    except FailureArtifactPublicationError:
        raise
    except OSError as error:
        raise FailureArtifactPublicationError(
            "failure artifact directory could not be revalidated"
        ) from error


def _failure_temp_identity(descriptor: int) -> FailureTempIdentity:
    status = os.fstat(descriptor)
    if not S_ISREG(status.st_mode) or status.st_nlink != 1:
        raise FailureArtifactPublicationError("failure artifact temp is not a private file")
    return FailureTempIdentity(
        device=status.st_dev,
        inode=status.st_ino,
        size=status.st_size,
        mode=status.st_mode,
        links=status.st_nlink,
        owner=status.st_uid,
    )


def _temp_status_matches(
    status: os.stat_result,
    identity: FailureTempIdentity,
    expected_links: int,
) -> bool:
    return (
        S_ISREG(status.st_mode)
        and status.st_dev == identity.device
        and status.st_ino == identity.inode
        and status.st_size == identity.size
        and status.st_mode == identity.mode
        and status.st_uid == identity.owner
        and status.st_nlink == expected_links
    )


def _verify_open_file_bytes(descriptor: int, expected: bytes) -> None:
    try:
        os.lseek(descriptor, 0, os.SEEK_SET)
        received = bytearray()
        while len(received) <= MAX_OUTPUT_BYTES:
            chunk = os.read(descriptor, min(16 * 1024, MAX_OUTPUT_BYTES + 1 - len(received)))
            if not chunk:
                break
            received.extend(chunk)
    except OSError as error:
        raise FailureArtifactPublicationError(
            "failure artifact bytes could not be verified"
        ) from error
    actual = bytes(received)
    if (
        actual != expected
        or hashlib.sha256(actual).digest() != hashlib.sha256(expected).digest()
    ):
        raise FailureArtifactPublicationError("failure artifact bytes changed")


def _validate_failure_temp_name(
    name: str,
    identity: FailureTempIdentity,
    directory_descriptor: int,
) -> None:
    try:
        status = os.stat(name, dir_fd=directory_descriptor, follow_symlinks=False)
    except OSError as error:
        raise FailureArtifactPublicationError(
            "failure artifact temp name could not be revalidated"
        ) from error
    if not _temp_status_matches(status, identity, 1):
        raise FailureArtifactPublicationError("failure artifact temp name changed")


def _link_failure_temp(
    temporary_name: str,
    final_name: str,
    directory_descriptor: int,
) -> None:
    try:
        os.link(
            temporary_name,
            final_name,
            src_dir_fd=directory_descriptor,
            dst_dir_fd=directory_descriptor,
            follow_symlinks=False,
        )
    except FileExistsError as error:
        raise FailureArtifactPublicationError(
            "failure artifact already exists and was not overwritten"
        ) from error
    except (NotImplementedError, TypeError, OSError) as error:
        raise FailureArtifactPublicationError(
            "failure artifact could not be linked atomically"
        ) from error


def _verify_published_failure(
    final_name: str,
    directory_descriptor: int,
    identity: FailureTempIdentity,
    expected: bytes,
    expected_links: int,
) -> None:
    flags = (
        os.O_RDONLY
        | getattr(os, "O_NOFOLLOW", 0)
        | getattr(os, "O_CLOEXEC", 0)
    )
    try:
        descriptor = os.open(final_name, flags, dir_fd=directory_descriptor)
    except OSError as error:
        raise FailureArtifactPublicationError(
            "published failure artifact could not be opened safely"
        ) from error
    try:
        if not _temp_status_matches(os.fstat(descriptor), identity, expected_links):
            raise FailureArtifactPublicationError(
                "published failure artifact identity changed"
            )
        _verify_open_file_bytes(descriptor, expected)
        if not _temp_status_matches(os.fstat(descriptor), identity, expected_links):
            raise FailureArtifactPublicationError(
                "published failure artifact metadata changed"
            )
    finally:
        os.close(descriptor)


def _fsync_pinned_directory(directory_descriptor: int) -> None:
    try:
        os.fsync(directory_descriptor)
    except OSError as error:
        raise FailureArtifactPublicationError(
            "failure artifact directory sync is unsupported or failed"
        ) from error


def _unlink_relative(name: str, directory_descriptor: int) -> None:
    try:
        os.unlink(name, dir_fd=directory_descriptor)
    except FileNotFoundError:
        return


def _rollback_published_failure(
    final_name: str,
    directory_descriptor: int,
) -> None:
    removal_error: OSError | None = None
    try:
        os.unlink(final_name, dir_fd=directory_descriptor)
    except FileNotFoundError:
        pass
    except OSError as error:
        removal_error = error
    sync_error: FailureArtifactPublicationError | None = None
    try:
        _fsync_pinned_directory(directory_descriptor)
    except FailureArtifactPublicationError as error:
        sync_error = error
    try:
        os.stat(final_name, dir_fd=directory_descriptor, follow_symlinks=False)
    except FileNotFoundError:
        final_absent = True
    except OSError as error:
        raise FailureArtifactPublicationError(
            "rolled-back failure artifact absence could not be verified"
        ) from error
    else:
        final_absent = False
    if removal_error is not None or not final_absent:
        raise FailureArtifactPublicationError(
            "published failure artifact could not be rolled back"
        ) from removal_error
    if sync_error is not None:
        raise sync_error


def _write_failure_bytes(descriptor: int, data: bytes) -> None:
    view = memoryview(data)
    while view:
        try:
            written = os.write(descriptor, view)
        except OSError as error:
            raise FailureArtifactPublicationError(
                "failure artifact temp write failed"
            ) from error
        if written <= 0:
            raise FailureArtifactPublicationError("failure artifact temp write stalled")
        view = view[written:]


def publish_failure_artifact(output: pathlib.Path, data: bytes) -> None:
    """Publish negative JSON atomically within ATOMIC_PUBLICATION_THREAT_BOUNDARY."""
    if not data or len(data) > MAX_OUTPUT_BYTES:
        raise FailureArtifactPublicationError("failure artifact bytes violate their bound")
    directory_descriptor, directory_identity = _open_pinned_failure_directory(output)
    temporary_name: str | None = None
    temporary_descriptor: int | None = None
    published = False
    final_name = FAILURE_ARTIFACT_NAME
    try:
        flags = (
            os.O_RDWR
            | os.O_CREAT
            | os.O_EXCL
            | getattr(os, "O_CLOEXEC", 0)
            | getattr(os, "O_NOFOLLOW", 0)
        )
        for _attempt in range(FAILURE_TEMP_ATTEMPTS):
            candidate = (
                FAILURE_TEMP_PREFIX
                + secrets.token_hex(16)
                + FAILURE_TEMP_SUFFIX
            )
            try:
                temporary_descriptor = os.open(
                    candidate,
                    flags,
                    0o600,
                    dir_fd=directory_descriptor,
                )
            except FileExistsError:
                continue
            temporary_name = candidate
            break
        if temporary_descriptor is None or temporary_name is None:
            raise FailureArtifactPublicationError(
                "failure artifact temp name could not be reserved"
            )
        _write_failure_bytes(temporary_descriptor, data)
        try:
            os.fsync(temporary_descriptor)
        except OSError as error:
            raise FailureArtifactPublicationError(
                "failure artifact temp sync failed"
            ) from error
        identity = _failure_temp_identity(temporary_descriptor)
        if identity.size != len(data):
            raise FailureArtifactPublicationError("failure artifact temp size changed")
        _verify_open_file_bytes(temporary_descriptor, data)
        if not _temp_status_matches(os.fstat(temporary_descriptor), identity, 1):
            raise FailureArtifactPublicationError("failure artifact temp metadata changed")
        _validate_failure_temp_name(
            temporary_name,
            identity,
            directory_descriptor,
        )
        _revalidate_pinned_directory(output, directory_descriptor, directory_identity)
        try:
            _link_failure_temp(temporary_name, final_name, directory_descriptor)
            published = True
            _verify_published_failure(
                final_name,
                directory_descriptor,
                identity,
                data,
                2,
            )
            _revalidate_pinned_directory(
                output,
                directory_descriptor,
                directory_identity,
            )
            _fsync_pinned_directory(directory_descriptor)
            _verify_published_failure(
                final_name,
                directory_descriptor,
                identity,
                data,
                2,
            )
            _unlink_relative(temporary_name, directory_descriptor)
            temporary_name = None
            _fsync_pinned_directory(directory_descriptor)
            _verify_published_failure(
                final_name,
                directory_descriptor,
                identity,
                data,
                1,
            )
            _revalidate_pinned_directory(
                output,
                directory_descriptor,
                directory_identity,
            )
        except Exception as error:
            if published:
                try:
                    _rollback_published_failure(final_name, directory_descriptor)
                except FailureArtifactPublicationError as rollback_error:
                    raise rollback_error from error
            if isinstance(error, FailureArtifactPublicationError):
                raise
            raise FailureArtifactPublicationError(
                "failure artifact post-link verification failed"
            ) from error
    finally:
        try:
            if temporary_name is not None:
                _unlink_relative(temporary_name, directory_descriptor)
        finally:
            try:
                if temporary_descriptor is not None:
                    os.close(temporary_descriptor)
            finally:
                os.close(directory_descriptor)


def write_failure_evidence(
    output: pathlib.Path,
    diagnostics: RunDiagnostics,
    error: BaseException,
) -> None:
    output = contract.absolute_directory(output, reject_symlinks=True)
    publish_failure_artifact(output, failure_json_bytes(diagnostics, error))


def valid_commit(value: str) -> str:
    if len(value) != 40 or value.lower() != value:
        raise SoakError("source commit must be 40 lowercase hexadecimal characters")
    try:
        bytes.fromhex(value)
    except ValueError as error:
        raise SoakError("source commit is invalid") from error
    return value


def parse_args(argv: Sequence[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--mode", choices=[mode.value for mode in EvidenceMode], required=True)
    parser.add_argument("--output-dir", required=True, type=pathlib.Path)
    parser.add_argument("--source-commit", default="")
    parser.add_argument(
        "--repo-root",
        type=pathlib.Path,
        help="clean archived source root used by the sanitized release builder",
    )
    parser.add_argument("--zmin-bin", type=pathlib.Path)
    parser.add_argument("--zmin-sha256", default="")
    parser.add_argument("--git-bin", type=pathlib.Path)
    parser.add_argument("--git-sha256", default="")
    return parser.parse_args(argv)


def run(
    args: argparse.Namespace,
    diagnostics: RunDiagnostics | None = None,
) -> list[ScenarioResult]:
    diagnostics = diagnostics if diagnostics is not None else RunDiagnostics()
    mode = EvidenceMode(args.mode)
    diagnostics.mode = mode.value
    profile = timing_profile(mode)
    rows: list[ScenarioResult] = []
    if mode == EvidenceMode.UNIT:
        diagnostics.temporary_cleanup = True
        diagnostics.residual_processes = 0
        diagnostics.residual_lfs_temp_files = 0
        for scenario in SCENARIOS:
            diagnostics.current_scenario = scenario.value
            row = run_unit_scenario(scenario, profile)
            rows.append(row)
            diagnostics.completed_scenarios.append(row)
        metadata: dict[str, object] = {
            "schema_version": SCHEMA_VERSION,
            "claim": "fixture-self-test-no-performance-claim",
            "mode": mode.value,
            "request_timeout": "not-applicable-reference-client",
            "operation_timeout": "not-applicable-reference-client",
            "temporary_cleanup": True,
        }
    else:
        if args.zmin_bin is None or args.git_bin is None or args.repo_root is None:
            raise SoakError(
                "release mode requires exact source, Zmin, and Git identity inputs"
            )
        source_commit = valid_commit(args.source_commit)
        zmin = executable_identity(
            args.zmin_bin,
            args.zmin_sha256,
            ("--version",),
            ZMIN_VERSION_PREFIX,
        )
        git = executable_identity(args.git_bin, args.git_sha256, ("--version",), "git version ")
        source = release_source_identity(args.repo_root, source_commit, git, zmin)
        diagnostics.zmin = zmin
        diagnostics.git = git
        diagnostics.source = source
        temporary_root: pathlib.Path | None = None
        try:
            with tempfile.TemporaryDirectory(prefix="zmin-lfs-timeout-soak-") as directory:
                temporary_root = pathlib.Path(directory)
                for index, scenario in enumerate(SCENARIOS):
                    diagnostics.current_scenario = scenario.value
                    diagnostics.child = None
                    diagnostics.fixture = None
                    diagnostics.residual_processes = None
                    diagnostics.residual_lfs_temp_files = None
                    scenario_root = temporary_root / f"scenario-{index:02d}"
                    scenario_root.mkdir()
                    row = run_release_scenario(
                        scenario,
                        profile,
                        scenario_root,
                        git,
                        zmin,
                        diagnostics,
                    )
                    rows.append(row)
                    diagnostics.completed_scenarios.append(row)
        finally:
            diagnostics.temporary_cleanup = (
                temporary_root is not None and not temporary_root.exists()
            )
        if not diagnostics.temporary_cleanup:
            raise SoakError("temporary soak root was not removed")
        metadata = {
            "schema_version": SCHEMA_VERSION,
            "claim": "timeout-correctness-only-no-speed-or-memory-claim",
            "mode": mode.value,
            "source_commit": source_commit,
            "source_tree": source.source_tree,
            "source_layout": "clean-archive",
            "repo_root": str(source.repo_root),
            "build_profile": contract.AUTHORITATIVE_PROFILE,
            "identity_sidecar": str(source.sidecar_path),
            "identity_sidecar_sha256": source.sidecar_sha256,
            "identity_verified": True,
            "zmin_path": str(zmin.path),
            "zmin_sha256": zmin.sha256,
            "zmin_version": zmin.version,
            "git_path": str(git.path),
            "git_sha256": git.sha256,
            "git_version": git.version,
            "activity_timeout_seconds": int(profile.activity_seconds),
            "request_timeout": "disabled-by-production-lfs-policy",
            "operation_timeout": "disabled-by-production-lfs-policy",
            "temporary_cleanup": True,
        }
    if len(rows) != len(SCENARIOS) or any(row.verdict != "pass" for row in rows):
        raise SoakError("soak did not produce four passing non-vacuous scenarios")
    diagnostics.current_scenario = None
    write_evidence(args.output_dir, metadata, rows)
    return rows


def main(argv: Sequence[str] | None = None) -> int:
    args: argparse.Namespace | None = None
    diagnostics = RunDiagnostics()
    try:
        args = parse_args(argv)
        rows = run(args, diagnostics)
        for row in rows:
            print(
                f"{row.scenario}\t{row.verdict}\telapsed={row.elapsed_seconds:.3f}\t"
                f"attempts={row.action_attempts}\tfailure={row.failure_kind}\t"
                f"timeout_evidence={row.timeout_evidence}"
            )
        return 0
    except Exception as error:
        artifact_error: Exception | None = None
        if args is not None:
            try:
                write_failure_evidence(args.output_dir, diagnostics, error)
            except Exception as failure:
                artifact_error = failure
        detail = sanitized_diagnostic_excerpt(str(error).encode("utf-8", "replace"))
        if not detail:
            detail = type(error).__name__
        print(f"lfs-timeout-soak: {detail[:512]}", file=sys.stderr)
        if artifact_error is not None:
            print(
                "lfs-timeout-soak: failure artifact could not be retained "
                f"({type(artifact_error).__name__})",
                file=sys.stderr,
            )
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
