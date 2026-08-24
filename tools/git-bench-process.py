#!/usr/bin/env python3
"""Measure one child process with typed platform-native memory metrics."""

from __future__ import annotations

import argparse
import functools
import os
import pathlib
import signal
import subprocess
import sys
import threading
import time
from typing import BinaryIO, Callable, NamedTuple

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import performance_contract as contract

if os.name == "nt":  # pragma: no cover - exercised by the Windows runner.
    import windows_child_metrics
else:
    windows_child_metrics = None


CHILD_TIMEOUT_SECONDS = 120.0
TIMEOUT_EXIT_STATUS = 124
PROCESS_GROUP_TERM_GRACE_SECONDS = 2.0
PROCESS_GROUP_KILL_WAIT_SECONDS = 5.0
OUTPUT_CAPTURE_CHUNK_BYTES = 64 * 1024
DEFAULT_MAX_OUTPUT_BYTES = 256 * 1024
MAX_MAX_OUTPUT_BYTES = 16 * 1024 * 1024
OUTPUT_LIMIT_EXIT_STATUS = 125
SURVIVING_DESCENDANT_EXIT_STATUS = 126
OUTPUT_CAPTURE_JOIN_SECONDS = 5.0

try:
    import resource
except ImportError:  # pragma: no cover - Windows has no resource module.
    resource = None


class BoundDirectory(NamedTuple):
    fd: int
    env_name: str
    env_value: str


class BoundedOutputCapture:
    """Drain one child pipe while retaining no more than the configured limit."""

    def __init__(
        self,
        source: BinaryIO,
        destination: BinaryIO,
        maximum_bytes: int,
        label: str,
        terminate_execution_unit: Callable[[], None] | None = None,
    ) -> None:
        self.source = source
        self.destination = destination
        self.maximum_bytes = maximum_bytes
        self.label = label
        self.terminate_execution_unit = terminate_execution_unit
        self.written_bytes = 0
        self.exceeded = threading.Event()
        self.failed = threading.Event()
        self.error: BaseException | None = None
        self.thread = threading.Thread(
            target=self._drain,
            name=f"git-bench-{label}-capture",
            daemon=True,
        )

    def start(self) -> None:
        self.thread.start()

    def _request_termination(self) -> None:
        callback = self.terminate_execution_unit
        if callback is not None:
            callback()

    def _drain(self) -> None:
        try:
            while True:
                chunk = self.source.read(OUTPUT_CAPTURE_CHUNK_BYTES)
                if not chunk:
                    break
                remaining = max(0, self.maximum_bytes - self.written_bytes)
                retained = chunk[:remaining]
                if retained:
                    self.destination.write(retained)
                    self.written_bytes += len(retained)
                if len(retained) != len(chunk) and not self.exceeded.is_set():
                    self.exceeded.set()
                    self._request_termination()
            self.destination.flush()
        except BaseException as error:
            self.error = error
            self.failed.set()
            try:
                self._request_termination()
            except BaseException:
                pass
        finally:
            self.source.close()

    def join(self) -> None:
        self.thread.join(timeout=OUTPUT_CAPTURE_JOIN_SECONDS)
        if self.thread.is_alive():
            raise contract.ContractError(f"{self.label} capture did not reach EOF")
        if self.error is not None:
            raise contract.ContractError(
                f"{self.label} capture failed: {self.error}"
            ) from self.error


class OutputLimitedWindowsApi:
    """Expose one Job Object to the output drainers without changing its policy."""

    def __init__(self, delegate: object) -> None:
        self.delegate = delegate
        self.lock = threading.Lock()
        self.job: object | None = None
        self.terminated = False

    def __getattr__(self, name: str) -> object:
        return getattr(self.delegate, name)

    def create_job(self) -> object:
        job = self.delegate.create_job()
        with self.lock:
            self.job = job
        return job

    def terminate_job(self, job: object, exit_code: int) -> None:
        with self.lock:
            if self.terminated:
                return
            self.delegate.terminate_job(job, exit_code)
            self.terminated = True

    def terminate_for_output_limit(self) -> None:
        with self.lock:
            job = self.job
            if job is None or self.terminated:
                return
            self.delegate.terminate_job(job, OUTPUT_LIMIT_EXIT_STATUS)
            self.terminated = True

    def close(self, handle: object) -> None:
        with self.lock:
            if handle == self.job:
                self.delegate.close(handle)
                self.job = None
                return
        self.delegate.close(handle)


def output_pipe(
    destination: BinaryIO,
    maximum_bytes: int,
    label: str,
    terminate_execution_unit: Callable[[], None] | None,
) -> tuple[BoundedOutputCapture, BinaryIO]:
    read_fd, write_fd = os.pipe()
    source = os.fdopen(read_fd, "rb", buffering=0)
    child_output = os.fdopen(write_fd, "wb", buffering=0)
    return (
        BoundedOutputCapture(
            source,
            destination,
            maximum_bytes,
            label,
            terminate_execution_unit,
        ),
        child_output,
    )


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--artifact-root", required=True, type=pathlib.Path)
    parser.add_argument("--artifact-root-identity", required=True)
    parser.add_argument("--stdout", required=True, type=pathlib.Path)
    parser.add_argument("--stderr", required=True, type=pathlib.Path)
    parser.add_argument("--metrics", required=True, type=pathlib.Path)
    parser.add_argument("--stdin", type=pathlib.Path)
    parser.add_argument("--bound-directory-env")
    parser.add_argument("--bound-directory", type=pathlib.Path)
    parser.add_argument("--bound-directory-identity", default="")
    parser.add_argument("--timeout-seconds", type=float, default=CHILD_TIMEOUT_SECONDS)
    parser.add_argument(
        "--max-output-bytes",
        type=int,
        default=DEFAULT_MAX_OUTPUT_BYTES,
    )
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    if args.command[:1] == ["--"]:
        args.command = args.command[1:]
    if not args.command:
        parser.error("missing command after --")
    if args.timeout_seconds <= 0:
        parser.error("--timeout-seconds must be positive")
    if args.max_output_bytes <= 0 or args.max_output_bytes > MAX_MAX_OUTPUT_BYTES:
        parser.error(
            f"--max-output-bytes must be in 1..={MAX_MAX_OUTPUT_BYTES}"
        )
    bound_options = (
        args.bound_directory_env,
        args.bound_directory,
        args.bound_directory_identity,
    )
    if any(value is not None and value != "" for value in bound_options) and not all(
        value is not None and value != "" for value in bound_options
    ):
        parser.error(
            "--bound-directory-env, --bound-directory, and "
            "--bound-directory-identity must be supplied together"
        )
    if args.bound_directory_env is not None and not args.bound_directory_env.isidentifier():
        parser.error("invalid bound directory environment variable name")
    return args


def bind_directory_for_child(
    env_name: str,
    directory: pathlib.Path,
    expected_identity: dict[str, object],
) -> BoundDirectory:
    """Pin an empty directory and expose that same inode to one child process."""
    directory_flag = getattr(os, "O_DIRECTORY", None)
    nofollow_flag = getattr(os, "O_NOFOLLOW", None)
    if (
        os.name == "nt"
        or not isinstance(directory_flag, int)
        or not isinstance(nofollow_flag, int)
        or not hasattr(os, "fchdir")
    ):
        raise contract.ContractError(
            "bound directory requires POSIX O_DIRECTORY, O_NOFOLLOW, and fchdir"
        )
    try:
        directory = contract.absolute_directory(directory, reject_symlinks=True)
    except (FileNotFoundError, OSError) as error:
        raise contract.ContractError(
            f"bound directory is missing or unsafe: {directory}"
        ) from error
    try:
        fd = os.open(str(directory), os.O_RDONLY | directory_flag | nofollow_flag)
    except OSError as error:
        raise contract.ContractError(
            f"cannot bind directory without following symlinks: {directory}: {error}"
        ) from error
    try:
        state = os.fstat(fd)
        actual_identity = {
            "platform": os.name,
            "st_dev": int(state.st_dev),
            "st_ino": int(state.st_ino),
        }
        if actual_identity != expected_identity:
            raise contract.ContractError(f"bound directory identity changed: {directory}")
        try:
            entries = os.listdir(fd)
        except (OSError, TypeError) as error:
            raise contract.ContractError(
                f"cannot verify bound directory contents: {directory}"
            ) from error
        if entries:
            raise contract.ContractError(f"bound directory must be empty: {directory}")
        return BoundDirectory(fd, env_name, ".")
    except Exception:
        os.close(fd)
        raise


def linux_io_bytes(pid: int) -> tuple[int | None, int | None]:
    if not sys.platform.startswith("linux"):
        return None, None
    try:
        values: dict[str, int] = {}
        for line in pathlib.Path(f"/proc/{pid}/io").read_text().splitlines():
            key, value = line.split(":", 1)
            if key in {"read_bytes", "write_bytes"}:
                values[key] = int(value.strip())
        return values.get("read_bytes"), values.get("write_bytes")
    except (FileNotFoundError, OSError, ValueError):
        return None, None


def process_group_exists(process_group: int) -> bool:
    try:
        os.killpg(process_group, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True


def wait_for_process_group_exit(process_group: int, timeout_seconds: float) -> bool:
    deadline = time.monotonic() + timeout_seconds
    while process_group_exists(process_group):
        if time.monotonic() >= deadline:
            return False
        time.sleep(0.002)
    return True


def signal_process_group(
    completed: subprocess.Popen[bytes],
    process_group: int,
    signal_number: int,
) -> bool:
    """Signal one owned group, accepting EPERM only after proven quiescence."""
    try:
        os.killpg(process_group, signal_number)
        return True
    except ProcessLookupError:
        return False
    except PermissionError as error:
        if completed.poll() is None:
            raise
        try:
            os.killpg(process_group, 0)
        except ProcessLookupError:
            return False
        except PermissionError:
            raise error
        raise error


def terminate_process_group(
    completed: subprocess.Popen[bytes],
    *,
    graceful: bool = True,
) -> None:
    """Terminate the child and every descendant in its dedicated POSIX group."""
    if os.name == "nt":
        raise contract.ContractError(
            "child timeout cleanup requires POSIX process-group support on this platform"
        )
    process_group = completed.pid
    if graceful:
        if not signal_process_group(completed, process_group, signal.SIGTERM):
            completed.wait()
            return
        if wait_for_process_group_exit(
            process_group,
            PROCESS_GROUP_TERM_GRACE_SECONDS,
        ):
            completed.wait()
            return
    signal_process_group(completed, process_group, signal.SIGKILL)
    completed.wait()
    if not wait_for_process_group_exit(process_group, PROCESS_GROUP_KILL_WAIT_SECONDS):
        raise contract.ContractError("terminated process group did not quiesce")


def main() -> int:
    args = parse_args()
    artifact_root = contract.absolute_directory(args.artifact_root, reject_symlinks=True)
    artifact_identity = contract.parse_artifact_identity(args.artifact_root_identity)
    output_names = [
        contract.artifact_relative_name(artifact_root, args.stdout),
        contract.artifact_relative_name(artifact_root, args.stderr),
        contract.artifact_relative_name(artifact_root, args.metrics),
    ]
    stdin_name = (
        contract.artifact_relative_name(artifact_root, args.stdin)
        if args.stdin is not None
        else None
    )
    contract.artifact_preflight_paths(
        artifact_root,
        output_names + ([stdin_name] if stdin_name is not None else []),
        expected_directory_identity=artifact_identity,
    )
    stdin_owned = False
    if stdin_name is not None:
        stdin = contract.artifact_open(
            artifact_root,
            stdin_name,
            mode="read",
            expected_directory_identity=artifact_identity,
        )
    elif os.name == "nt":
        stdin = open(os.devnull, "rb")
        stdin_owned = True
    else:
        stdin = subprocess.DEVNULL
    bound_directory: BoundDirectory | None = None
    before = resource.getrusage(resource.RUSAGE_CHILDREN) if resource else None
    read_bytes: int | None = None
    write_bytes: int | None = None
    timed_out = False
    output_limit_exceeded = False
    surviving_descendants = False
    windows_result = None
    completed: subprocess.Popen[bytes] | None = None
    started = time.perf_counter_ns()
    try:
        with (
            contract.artifact_open(
                artifact_root,
                output_names[0],
                mode="write",
                expected_directory_identity=artifact_identity,
            ) as stdout,
            contract.artifact_open(
                artifact_root,
                output_names[1],
                mode="write",
                expected_directory_identity=artifact_identity,
            ) as stderr,
        ):
            windows_api = None
            if os.name == "nt":
                if args.bound_directory_env is not None:
                    raise contract.ContractError(
                        "Windows native child metrics do not support descriptor-bound cwd"
                    )
                if windows_child_metrics is None:  # pragma: no cover - defensive.
                    raise contract.ContractError("Windows native child metrics backend unavailable")
                windows_api = OutputLimitedWindowsApi(
                    windows_child_metrics.WindowsChildRunner().api
                )
            terminate_for_output_limit = (
                windows_api.terminate_for_output_limit
                if windows_api is not None
                else None
            )
            stdout_capture, child_stdout = output_pipe(
                stdout,
                args.max_output_bytes,
                "stdout",
                terminate_for_output_limit,
            )
            stderr_capture, child_stderr = output_pipe(
                stderr,
                args.max_output_bytes,
                "stderr",
                terminate_for_output_limit,
            )
            captures = (stdout_capture, stderr_capture)
            child_outputs = (child_stdout, child_stderr)
            for capture in captures:
                capture.start()
            try:
                if os.name == "nt":
                    windows_result = windows_child_metrics.WindowsChildRunner(
                        api=windows_api
                    ).run(
                        args.command,
                        stdin=stdin,
                        stdout=child_stdout,
                        stderr=child_stderr,
                        environment=os.environ.copy(),
                        cwd=os.getcwd(),
                        timeout_seconds=args.timeout_seconds,
                    )
                    timed_out = windows_result.timed_out
                else:
                    if args.bound_directory_env is not None:
                        expected_identity = contract.parse_artifact_identity(
                            args.bound_directory_identity
                        )
                        if expected_identity is None:
                            raise contract.ContractError("bound directory identity is missing")
                        bound_directory = bind_directory_for_child(
                            args.bound_directory_env,
                            args.bound_directory,
                            expected_identity,
                        )
                        child_environment = os.environ.copy()
                        child_environment[bound_directory.env_name] = bound_directory.env_value
                        completed = subprocess.Popen(
                            args.command,
                            stdin=stdin,
                            stdout=child_stdout,
                            stderr=child_stderr,
                            env=child_environment,
                            pass_fds=(bound_directory.fd,),
                            preexec_fn=functools.partial(os.fchdir, bound_directory.fd),
                            start_new_session=True,
                        )
                    else:
                        completed = subprocess.Popen(
                            args.command,
                            stdin=stdin,
                            stdout=child_stdout,
                            stderr=child_stderr,
                            start_new_session=True,
                        )
                    for child_output in child_outputs:
                        child_output.close()
                    deadline = time.monotonic() + args.timeout_seconds
                    while completed.poll() is None:
                        if any(capture.exceeded.is_set() for capture in captures):
                            output_limit_exceeded = True
                            terminate_process_group(completed, graceful=False)
                            print(
                                "git-bench-process: child output exceeded the bounded "
                                "capture policy; terminated its process group; result is "
                                "non-authoritative",
                                file=sys.stderr,
                            )
                            break
                        if any(capture.failed.is_set() for capture in captures):
                            terminate_process_group(completed, graceful=False)
                            break
                        if time.monotonic() >= deadline:
                            timed_out = True
                            terminate_process_group(completed)
                            print(
                                "git-bench-process: child timed out after "
                                f"{args.timeout_seconds:g}s; terminated its process group; "
                                "result is non-authoritative",
                                file=sys.stderr,
                            )
                            break
                        current_read, current_write = linux_io_bytes(completed.pid)
                        if current_read is not None:
                            read_bytes = max(read_bytes or 0, current_read)
                        if current_write is not None:
                            write_bytes = max(write_bytes or 0, current_write)
                        time.sleep(0.002)
                    current_read, current_write = linux_io_bytes(completed.pid)
                    if current_read is not None:
                        read_bytes = max(read_bytes or 0, current_read)
                    if current_write is not None:
                        write_bytes = max(write_bytes or 0, current_write)
                    completed.wait()
                    if process_group_exists(completed.pid):
                        surviving_descendants = True
                        terminate_process_group(completed, graceful=False)
                        print(
                            "git-bench-process: child left surviving process-group "
                            "descendants; terminated them; result is non-authoritative",
                            file=sys.stderr,
                        )
            except BaseException:
                if (
                    os.name != "nt"
                    and completed is not None
                    and process_group_exists(completed.pid)
                ):
                    terminate_process_group(completed, graceful=False)
                raise
            finally:
                for child_output in child_outputs:
                    if not child_output.closed:
                        child_output.close()
                for capture in captures:
                    capture.join()
            output_limit_exceeded = output_limit_exceeded or any(
                capture.exceeded.is_set() for capture in captures
            )
            if output_limit_exceeded and os.name == "nt":
                print(
                    "git-bench-process: child output exceeded the bounded capture "
                    "policy; terminated its Job Object; result is non-authoritative",
                    file=sys.stderr,
                )
    finally:
        if bound_directory is not None:
            os.close(bound_directory.fd)
        if args.stdin is not None or stdin_owned:
            stdin.close()
    if windows_result is not None:
        elapsed_seconds = windows_result.wall_seconds
        rss_bytes = None
        job_commit_bytes = windows_result.metrics.peak_job_commit_bytes
        user_seconds = windows_result.metrics.user_seconds
        sys_seconds = windows_result.metrics.sys_seconds
        major_faults = None
        minor_faults = None
        read_bytes = windows_result.metrics.read_bytes
        write_bytes = windows_result.metrics.write_bytes
    else:
        elapsed_seconds = (time.perf_counter_ns() - started) / 1_000_000_000
        after = resource.getrusage(resource.RUSAGE_CHILDREN) if resource else None
        if after is not None:
            rss_bytes = int(
                after.ru_maxrss if sys.platform == "darwin" else after.ru_maxrss * 1024
            )
            user_seconds = after.ru_utime - before.ru_utime
            sys_seconds = after.ru_stime - before.ru_stime
            major_faults = after.ru_majflt - before.ru_majflt
            minor_faults = after.ru_minflt - before.ru_minflt
        else:
            rss_bytes = None
            user_seconds = None
            sys_seconds = None
            major_faults = None
            minor_faults = None
        job_commit_bytes = None

    if windows_result is not None:
        memory_metric = "peak_job_commit_bytes"
        memory_semantics = "job_commit_peak"
        memory_scope = "job_process_tree"
    else:
        memory_metric = "peak_rss_bytes"
        memory_semantics = "working_set_peak"
        memory_scope = "waited_child_processes"
    memory_unit = "bytes"

    def metric(value: int | float | None) -> str:
        return "unsupported" if value is None else str(value)

    availability = ";".join(
        f"{name}={'available' if value is not None else 'unsupported'}"
        for name, value in (
            ("wall_seconds", elapsed_seconds),
            ("user_seconds", user_seconds),
            ("sys_seconds", sys_seconds),
            ("peak_rss_bytes", rss_bytes),
            ("peak_job_commit_bytes", job_commit_bytes),
            ("major_page_faults", major_faults),
            ("minor_page_faults", minor_faults),
            ("read_bytes", read_bytes),
            ("write_bytes", write_bytes),
        )
    )
    metrics = (
        f"{elapsed_seconds:.9f}\t"
        f"{metric(user_seconds)}\t"
        f"{metric(sys_seconds)}\t"
        f"{metric(rss_bytes)}\t"
        f"{metric(job_commit_bytes)}\t"
        f"{metric(major_faults)}\t"
        f"{metric(minor_faults)}\t"
        f"{metric(read_bytes)}\t"
        f"{metric(write_bytes)}\t"
        f"{memory_metric}\t"
        f"{memory_semantics}\t"
        f"{memory_scope}\t"
        f"{memory_unit}\t"
        f"{availability}\n"
    )
    contract.artifact_write_bytes(
        artifact_root,
        output_names[2],
        metrics.encode(),
        expected_directory_identity=artifact_identity,
    )
    if output_limit_exceeded:
        return OUTPUT_LIMIT_EXIT_STATUS
    if surviving_descendants:
        return SURVIVING_DESCENDANT_EXIT_STATUS
    if timed_out:
        return TIMEOUT_EXIT_STATUS
    if windows_result is not None:
        return windows_result.returncode
    if completed is None:
        raise contract.ContractError("POSIX child process result is missing")
    return completed.returncode


if __name__ == "__main__":
    raise SystemExit(main())
