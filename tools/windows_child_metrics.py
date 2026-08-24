#!/usr/bin/env python3
"""Small fail-closed Windows child-process/job metrics backend.

The public runner is deliberately independent of the Win32 bindings.  Tests
can provide a fake API on any host, while the native implementation is only
constructed on Windows.  A child is always created suspended, assigned to a
kill-on-close job, and resumed only after the assignment succeeds.
"""

from __future__ import annotations

import ctypes
from ctypes import wintypes
from dataclasses import dataclass
import math
import os
import subprocess
import time
from typing import Any, Mapping, Sequence


TIMEOUT_EXIT_STATUS = 124
WAIT_TIMEOUT = 258
INFINITE = 0xFFFFFFFF
JOB_QUIESCENCE_TIMEOUT_MS = 5_000


class WindowsChildMetricsError(RuntimeError):
    """A native launch, cleanup, wait, or metrics query failed."""


@dataclass(frozen=True)
class JobMetrics:
    peak_job_commit_bytes: int
    user_seconds: float
    sys_seconds: float
    total_page_faults: int
    read_bytes: int
    write_bytes: int


@dataclass(frozen=True)
class ChildResult:
    returncode: int
    timed_out: bool
    wall_seconds: float
    metrics: JobMetrics


def windows_command_line(command: Sequence[str]) -> str:
    """Return the deterministic command line consumed by CreateProcessW."""
    if not command:
        raise WindowsChildMetricsError("Windows child command is empty")
    values = tuple(str(value) for value in command)
    if any("\x00" in value for value in values):
        raise WindowsChildMetricsError("Windows child command contains NUL")
    return subprocess.list2cmdline(values)


def windows_environment_block(environment: Mapping[str, str]) -> str:
    """Encode a sorted UTF-16 environment block for CreateProcessW."""
    entries: list[tuple[str, str]] = []
    for key, value in environment.items():
        key = str(key)
        value = str(value)
        if not key or "=" in key or "\x00" in key or "\x00" in value:
            raise WindowsChildMetricsError(f"invalid Windows environment entry: {key!r}")
        entries.append((key, value))
    entries.sort(key=lambda item: item[0].upper())
    return "\x00".join(f"{key}={value}" for key, value in entries) + "\x00\x00"


def _stream_handle(stream: Any) -> int:
    if isinstance(stream, int):
        return stream
    if os.name != "nt":
        raise WindowsChildMetricsError("Windows stdio handle requested on a non-Windows host")
    try:
        import msvcrt

        return int(msvcrt.get_osfhandle(stream.fileno()))
    except (AttributeError, OSError, ValueError) as error:
        raise WindowsChildMetricsError("cannot obtain inheritable Windows stdio handle") from error


class WindowsChildRunner:
    """Run one child with a job-owned timeout and aggregate metrics."""

    def __init__(self, api: Any | None = None, *, clock_ns=time.perf_counter_ns) -> None:
        self.api = _NativeWindowsApi() if api is None else api
        self.clock_ns = clock_ns

    def run(
        self,
        command: Sequence[str],
        *,
        stdin: Any,
        stdout: Any,
        stderr: Any,
        environment: Mapping[str, str],
        cwd: str,
        timeout_seconds: float,
    ) -> ChildResult:
        if timeout_seconds <= 0:
            raise WindowsChildMetricsError("Windows child timeout must be positive")
        command_line = windows_command_line(command)
        environment_block = windows_environment_block(environment)
        stdio = (
            _stream_handle(stdin),
            _stream_handle(stdout),
            _stream_handle(stderr),
        )
        job = process = thread = None
        assigned = False
        completed = False
        terminated = False
        quiescent = False
        timed_out = False
        started = self.clock_ns()
        try:
            job = self.api.create_job()
            self.api.set_kill_on_close(job)
            process, thread = self.api.create_process(command_line, cwd, environment_block, stdio)
            self.api.assign_process(job, process)
            assigned = True
            self.api.resume_thread(thread)
            while True:
                elapsed = (self.clock_ns() - started) / 1_000_000_000
                remaining = timeout_seconds - elapsed
                if remaining <= 0:
                    timed_out = True
                    self.api.terminate_job(job, TIMEOUT_EXIT_STATUS)
                    terminated = True
                    self._wait_for_job_quiescence(job)
                    quiescent = True
                    completed = True
                    break
                wait_ms = min(max(1, int(remaining * 1000)), 50)
                if self.api.wait(process, wait_ms):
                    completed = True
                    elapsed = (self.clock_ns() - started) / 1_000_000_000
                    remaining = timeout_seconds - elapsed
                    job_wait_ms = min(
                        max(1, math.ceil(remaining * 1000)),
                        INFINITE - 1,
                    )
                    if remaining <= 0 or not self.api.wait_job(job, job_wait_ms):
                        timed_out = True
                        self.api.terminate_job(job, TIMEOUT_EXIT_STATUS)
                        terminated = True
                        self._wait_for_job_quiescence(job)
                    quiescent = True
                    break
            metrics = self.api.query_metrics(job)
            returncode = TIMEOUT_EXIT_STATUS if timed_out else self.api.exit_code(process)
            wall_seconds = (self.clock_ns() - started) / 1_000_000_000
            return ChildResult(returncode, timed_out, wall_seconds, metrics)
        except Exception as error:
            cleanup_error: Exception | None = None
            if process is not None and not quiescent:
                try:
                    if assigned:
                        if not terminated:
                            self.api.terminate_job(job, 1)
                            terminated = True
                        self._wait_for_job_quiescence(job)
                    elif not completed:
                        self.api.terminate_process(process, 1)
                        self.api.wait(process, INFINITE)
                except Exception as cleanup_failure:  # pragma: no cover - native failure path.
                    cleanup_error = cleanup_failure
            if cleanup_error is not None:
                raise WindowsChildMetricsError(
                    f"Windows child failed and cleanup failed: {cleanup_error}"
                ) from error
            if isinstance(error, WindowsChildMetricsError):
                raise
            raise WindowsChildMetricsError(f"Windows child operation failed: {error}") from error
        finally:
            for handle in (thread, process, job):
                if handle is not None:
                    self.api.close(handle)

    def _wait_for_job_quiescence(self, job: Any) -> None:
        if not self.api.wait_job(job, JOB_QUIESCENCE_TIMEOUT_MS):
            raise WindowsChildMetricsError(
                "Windows child job did not quiesce after termination"
            )


if os.name == "nt":
    _DWORD = wintypes.DWORD
    _HANDLE = wintypes.HANDLE
    _SIZE_T = ctypes.c_size_t
    _LARGE_INTEGER = ctypes.c_longlong
    _LPVOID = wintypes.LPVOID

    _STARTF_USESTDHANDLES = 0x00000100
    _EXTENDED_STARTUPINFO_PRESENT = 0x00080000
    _CREATE_SUSPENDED = 0x00000004
    _CREATE_UNICODE_ENVIRONMENT = 0x00000400
    _HANDLE_FLAG_INHERIT = 0x00000001
    _PROC_THREAD_ATTRIBUTE_HANDLE_LIST = 0x00020002
    _ERROR_INSUFFICIENT_BUFFER = 122
    _WAIT_OBJECT_0 = 0
    _WAIT_FAILED = 0xFFFFFFFF
    _JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE = 0x00002000

    class _STARTUPINFOW(ctypes.Structure):
        _fields_ = [
            ("cb", _DWORD),
            ("lpReserved", wintypes.LPWSTR),
            ("lpDesktop", wintypes.LPWSTR),
            ("lpTitle", wintypes.LPWSTR),
            ("dwX", _DWORD),
            ("dwY", _DWORD),
            ("dwXSize", _DWORD),
            ("dwYSize", _DWORD),
            ("dwXCountChars", _DWORD),
            ("dwYCountChars", _DWORD),
            ("dwFillAttribute", _DWORD),
            ("dwFlags", _DWORD),
            ("wShowWindow", wintypes.WORD),
            ("cbReserved2", wintypes.WORD),
            ("lpReserved2", ctypes.POINTER(ctypes.c_ubyte)),
            ("hStdInput", _HANDLE),
            ("hStdOutput", _HANDLE),
            ("hStdError", _HANDLE),
        ]

    class _STARTUPINFOEXW(ctypes.Structure):
        _fields_ = [
            ("StartupInfo", _STARTUPINFOW),
            ("lpAttributeList", _LPVOID),
        ]

    class _PROCESS_INFORMATION(ctypes.Structure):
        _fields_ = [
            ("hProcess", _HANDLE),
            ("hThread", _HANDLE),
            ("dwProcessId", _DWORD),
            ("dwThreadId", _DWORD),
        ]

    class _IO_COUNTERS(ctypes.Structure):
        _fields_ = [(name, ctypes.c_ulonglong) for name in (
            "ReadOperationCount",
            "WriteOperationCount",
            "OtherOperationCount",
            "ReadTransferCount",
            "WriteTransferCount",
            "OtherTransferCount",
        )]

    class _BASIC_ACCOUNTING(ctypes.Structure):
        _fields_ = [
            ("TotalUserTime", _LARGE_INTEGER),
            ("TotalKernelTime", _LARGE_INTEGER),
            ("ThisPeriodTotalUserTime", _LARGE_INTEGER),
            ("ThisPeriodTotalKernelTime", _LARGE_INTEGER),
            ("TotalPageFaultCount", _DWORD),
            ("TotalProcesses", _DWORD),
            ("ActiveProcesses", _DWORD),
            ("TotalTerminatedProcesses", _DWORD),
        ]

    class _BASIC_AND_IO_ACCOUNTING(ctypes.Structure):
        _fields_ = [
            ("BasicInfo", _BASIC_ACCOUNTING),
            ("IoInfo", _IO_COUNTERS),
        ]

    class _BASIC_LIMIT_INFORMATION(ctypes.Structure):
        _fields_ = [
            ("PerProcessUserTimeLimit", _LARGE_INTEGER),
            ("PerJobUserTimeLimit", _LARGE_INTEGER),
            ("LimitFlags", _DWORD),
            ("MinimumWorkingSetSize", _SIZE_T),
            ("MaximumWorkingSetSize", _SIZE_T),
            ("ActiveProcessLimit", _DWORD),
            ("Affinity", ctypes.c_size_t),
            ("PriorityClass", _DWORD),
            ("SchedulingClass", _DWORD),
        ]

    class _EXTENDED_LIMIT_INFORMATION(ctypes.Structure):
        _fields_ = [
            ("BasicLimitInformation", _BASIC_LIMIT_INFORMATION),
            ("IoInfo", _IO_COUNTERS),
            ("ProcessMemoryLimit", _SIZE_T),
            ("JobMemoryLimit", _SIZE_T),
            ("PeakProcessMemoryUsed", _SIZE_T),
            ("PeakJobMemoryUsed", _SIZE_T),
        ]

    def _native_error(operation: str) -> WindowsChildMetricsError:
        return WindowsChildMetricsError(
            f"{operation} failed with Win32 error {ctypes.get_last_error()}"
        )

    class _NativeWindowsApi:
        def __init__(self) -> None:
            self.kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
            self.kernel32.CreateJobObjectW.restype = _HANDLE
            self.kernel32.CreateJobObjectW.argtypes = [_HANDLE, wintypes.LPCWSTR]
            self.kernel32.SetInformationJobObject.restype = wintypes.BOOL
            self.kernel32.SetInformationJobObject.argtypes = [
                _HANDLE,
                _DWORD,
                _LPVOID,
                _DWORD,
            ]
            self.kernel32.InitializeProcThreadAttributeList.restype = wintypes.BOOL
            self.kernel32.InitializeProcThreadAttributeList.argtypes = [
                _LPVOID,
                _DWORD,
                _DWORD,
                ctypes.POINTER(ctypes.c_size_t),
            ]
            self.kernel32.UpdateProcThreadAttribute.restype = wintypes.BOOL
            self.kernel32.UpdateProcThreadAttribute.argtypes = [
                _LPVOID,
                _DWORD,
                ctypes.c_size_t,
                _LPVOID,
                ctypes.c_size_t,
                _LPVOID,
                ctypes.POINTER(ctypes.c_size_t),
            ]
            self.kernel32.DeleteProcThreadAttributeList.argtypes = [_LPVOID]
            self.kernel32.CreateProcessW.restype = wintypes.BOOL
            self.kernel32.CreateProcessW.argtypes = [
                wintypes.LPWSTR,
                wintypes.LPWSTR,
                _LPVOID,
                _LPVOID,
                wintypes.BOOL,
                _DWORD,
                _LPVOID,
                wintypes.LPWSTR,
                ctypes.POINTER(_STARTUPINFOEXW),
                ctypes.POINTER(_PROCESS_INFORMATION),
            ]
            self.kernel32.AssignProcessToJobObject.restype = wintypes.BOOL
            self.kernel32.AssignProcessToJobObject.argtypes = [_HANDLE, _HANDLE]
            self.kernel32.ResumeThread.restype = _DWORD
            self.kernel32.ResumeThread.argtypes = [_HANDLE]
            self.kernel32.WaitForSingleObject.restype = _DWORD
            self.kernel32.WaitForSingleObject.argtypes = [_HANDLE, _DWORD]
            self.kernel32.TerminateJobObject.restype = wintypes.BOOL
            self.kernel32.TerminateJobObject.argtypes = [_HANDLE, _DWORD]
            self.kernel32.TerminateProcess.restype = wintypes.BOOL
            self.kernel32.TerminateProcess.argtypes = [_HANDLE, _DWORD]
            self.kernel32.GetExitCodeProcess.restype = wintypes.BOOL
            self.kernel32.GetExitCodeProcess.argtypes = [_HANDLE, ctypes.POINTER(_DWORD)]
            self.kernel32.QueryInformationJobObject.restype = wintypes.BOOL
            self.kernel32.QueryInformationJobObject.argtypes = [
                _HANDLE,
                _DWORD,
                _LPVOID,
                _DWORD,
                ctypes.POINTER(_DWORD),
            ]
            self.kernel32.GetHandleInformation.restype = wintypes.BOOL
            self.kernel32.GetHandleInformation.argtypes = [_HANDLE, ctypes.POINTER(_DWORD)]
            self.kernel32.SetHandleInformation.restype = wintypes.BOOL
            self.kernel32.SetHandleInformation.argtypes = [_HANDLE, _DWORD, _DWORD]
            self.kernel32.CloseHandle.restype = wintypes.BOOL
            self.kernel32.CloseHandle.argtypes = [_HANDLE]

        def create_job(self) -> Any:
            handle = self.kernel32.CreateJobObjectW(None, None)
            if not handle:
                raise _native_error("CreateJobObjectW")
            return handle

        def set_kill_on_close(self, job: Any) -> None:
            info = _EXTENDED_LIMIT_INFORMATION()
            info.BasicLimitInformation.LimitFlags = _JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
            if not self.kernel32.SetInformationJobObject(
                job,
                9,
                ctypes.byref(info),
                ctypes.sizeof(info),
            ):
                raise _native_error("SetInformationJobObject")

        def create_process(
            self,
            command_line: str,
            cwd: str,
            environment_block: str,
            stdio: Sequence[int],
        ) -> tuple[Any, Any]:
            if len(stdio) != 3:
                raise WindowsChildMetricsError("Windows child requires three stdio handles")
            handles = [_HANDLE(value) for value in stdio]
            old_flags: list[int] = []
            attribute_list = None
            attribute_initialized = False
            process_info = _PROCESS_INFORMATION()
            primary_error: Exception | None = None
            try:
                for handle in handles:
                    flags = _DWORD()
                    if not self.kernel32.GetHandleInformation(handle, ctypes.byref(flags)):
                        raise _native_error("GetHandleInformation")
                    old_flags.append(int(flags.value))
                    if not self.kernel32.SetHandleInformation(
                        handle,
                        _HANDLE_FLAG_INHERIT,
                        _HANDLE_FLAG_INHERIT,
                    ):
                        raise _native_error("SetHandleInformation")
                attribute_size = ctypes.c_size_t()
                self.kernel32.InitializeProcThreadAttributeList(
                    None, 1, 0, ctypes.byref(attribute_size)
                )
                if ctypes.get_last_error() != _ERROR_INSUFFICIENT_BUFFER:
                    raise _native_error("InitializeProcThreadAttributeList(size)")
                attribute_buffer = ctypes.create_string_buffer(attribute_size.value)
                attribute_list = ctypes.cast(attribute_buffer, _LPVOID)
                if not self.kernel32.InitializeProcThreadAttributeList(
                    attribute_list, 1, 0, ctypes.byref(attribute_size)
                ):
                    raise _native_error("InitializeProcThreadAttributeList")
                attribute_initialized = True
                handle_array = (_HANDLE * 3)(*handles)
                startup = _STARTUPINFOEXW()
                startup.StartupInfo.cb = ctypes.sizeof(startup)
                startup.StartupInfo.dwFlags = _STARTF_USESTDHANDLES
                startup.StartupInfo.hStdInput = handles[0]
                startup.StartupInfo.hStdOutput = handles[1]
                startup.StartupInfo.hStdError = handles[2]
                startup.lpAttributeList = attribute_list
                if not self.kernel32.UpdateProcThreadAttribute(
                    attribute_list,
                    0,
                    _PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
                    ctypes.cast(handle_array, _LPVOID),
                    ctypes.sizeof(handle_array),
                    None,
                    None,
                ):
                    raise _native_error("UpdateProcThreadAttribute")
                command_buffer = ctypes.create_unicode_buffer(command_line)
                environment_buffer = ctypes.create_unicode_buffer(environment_block)
                flags = _CREATE_SUSPENDED | _CREATE_UNICODE_ENVIRONMENT | _EXTENDED_STARTUPINFO_PRESENT
                if not self.kernel32.CreateProcessW(
                    None,
                    command_buffer,
                    None,
                    None,
                    True,
                    flags,
                    ctypes.cast(environment_buffer, _LPVOID),
                    cwd,
                    ctypes.byref(startup),
                    ctypes.byref(process_info),
                ):
                    raise _native_error("CreateProcessW")
            except Exception as error:
                primary_error = error
            finally:
                restore_error: Exception | None = None
                for handle, flags in zip(handles, old_flags):
                    if not self.kernel32.SetHandleInformation(handle, _HANDLE_FLAG_INHERIT, flags):
                        restore_error = _native_error("SetHandleInformation(restore)")
                if attribute_initialized:
                    self.kernel32.DeleteProcThreadAttributeList(attribute_list)
            if primary_error is not None:
                raise primary_error
            if restore_error is not None:
                if process_info.hProcess:
                    self.kernel32.TerminateProcess(process_info.hProcess, 1)
                    self.kernel32.WaitForSingleObject(process_info.hProcess, INFINITE)
                    self.kernel32.CloseHandle(process_info.hThread)
                    self.kernel32.CloseHandle(process_info.hProcess)
                raise restore_error
            return process_info.hProcess, process_info.hThread

        def assign_process(self, job: Any, process: Any) -> None:
            if not self.kernel32.AssignProcessToJobObject(job, process):
                raise _native_error("AssignProcessToJobObject")

        def resume_thread(self, thread: Any) -> None:
            if self.kernel32.ResumeThread(thread) == 0xFFFFFFFF:
                raise _native_error("ResumeThread")

        def wait(self, process: Any, timeout_ms: int) -> bool:
            result = self.kernel32.WaitForSingleObject(process, timeout_ms)
            if result == _WAIT_FAILED:
                raise _native_error("WaitForSingleObject")
            if result == WAIT_TIMEOUT:
                return False
            if result != _WAIT_OBJECT_0:
                raise WindowsChildMetricsError(f"WaitForSingleObject returned {result}")
            return True

        def wait_job(self, job: Any, timeout_ms: int) -> bool:
            result = self.kernel32.WaitForSingleObject(job, timeout_ms)
            if result == _WAIT_FAILED:
                raise _native_error("WaitForSingleObject(job)")
            if result == WAIT_TIMEOUT:
                return False
            if result != _WAIT_OBJECT_0:
                raise WindowsChildMetricsError(
                    f"WaitForSingleObject(job) returned {result}"
                )
            return True

        def terminate_job(self, job: Any, exit_code: int) -> None:
            if not self.kernel32.TerminateJobObject(job, exit_code):
                raise _native_error("TerminateJobObject")

        def terminate_process(self, process: Any, exit_code: int) -> None:
            if not self.kernel32.TerminateProcess(process, exit_code):
                raise _native_error("TerminateProcess")

        def exit_code(self, process: Any) -> int:
            result = _DWORD()
            if not self.kernel32.GetExitCodeProcess(process, ctypes.byref(result)):
                raise _native_error("GetExitCodeProcess")
            return int(result.value)

        def query_metrics(self, job: Any) -> JobMetrics:
            accounting = _BASIC_AND_IO_ACCOUNTING()
            if not self.kernel32.QueryInformationJobObject(
                job, 8, ctypes.byref(accounting), ctypes.sizeof(accounting), None
            ):
                raise _native_error("QueryInformationJobObject(accounting)")
            extended = _EXTENDED_LIMIT_INFORMATION()
            if not self.kernel32.QueryInformationJobObject(
                job, 9, ctypes.byref(extended), ctypes.sizeof(extended), None
            ):
                raise _native_error("QueryInformationJobObject(peak)")
            return JobMetrics(
                peak_job_commit_bytes=int(extended.PeakJobMemoryUsed),
                user_seconds=int(accounting.BasicInfo.TotalUserTime) / 10_000_000,
                sys_seconds=int(accounting.BasicInfo.TotalKernelTime) / 10_000_000,
                total_page_faults=int(accounting.BasicInfo.TotalPageFaultCount),
                read_bytes=int(accounting.IoInfo.ReadTransferCount),
                write_bytes=int(accounting.IoInfo.WriteTransferCount),
            )

        def close(self, handle: Any) -> None:
            if handle and not self.kernel32.CloseHandle(handle):
                raise _native_error("CloseHandle")

else:

    class _NativeWindowsApi:
        def __init__(self) -> None:
            raise WindowsChildMetricsError("native Windows metrics are unavailable on this host")
