#!/usr/bin/env python3
"""Bounded, stdlib-only host-noise telemetry for performance evidence."""

from __future__ import annotations

import collections
import ctypes
import os
import pathlib
import platform
import re
import subprocess
import threading
import time
from dataclasses import dataclass
from typing import BinaryIO, Protocol, Sequence


SAMPLE_INTERVAL_SECONDS = 0.05
MAX_MONITOR_POINTS = 16_384
MONITOR_JOIN_SECONDS = 3.0
DARWIN_THERMAL_CACHE_SECONDS = 10.0
MAX_HOST_COMMAND_BYTES = 16 * 1024
HOST_COMMAND_TIMEOUT_SECONDS = 2.0
DARWIN_PROCESSOR_CPU_LOAD_INFO = 2
DARWIN_CPU_STATE_COUNT = 4
DARWIN_CPU_STATE_IDLE = 2
DARWIN_MAX_PROCESSORS = 4_096

DarwinInteger = ctypes.c_int32
DarwinNatural = ctypes.c_uint32
DarwinPort = ctypes.c_uint32


class HostNoiseError(RuntimeError):
    """Host telemetry was unavailable, incomplete, or explicitly non-normal."""


@dataclass(frozen=True)
class BoundedSubprocessResult:
    returncode: int
    output: bytes


class BoundedOutputReader:
    def __init__(self, source: BinaryIO, maximum_bytes: int) -> None:
        self.source = source
        self.maximum_bytes = maximum_bytes
        self.output = bytearray()
        self.overflow = False
        self.error: BaseException | None = None
        self.done = threading.Event()
        self.thread = threading.Thread(
            target=self._read,
            name="zmin-performance-host-command-output",
            daemon=True,
        )

    def _read(self) -> None:
        try:
            while True:
                remaining = self.maximum_bytes - len(self.output)
                chunk = self.source.read1(min(4096, remaining + 1))
                if not chunk:
                    break
                self.output.extend(chunk[:remaining])
                if len(chunk) > remaining:
                    self.overflow = True
                    break
        except BaseException as error:
            self.error = error
        finally:
            self.done.set()


def bounded_subprocess_output(
    command: Sequence[str],
    *,
    maximum_bytes: int,
    timeout_seconds: float,
    environment: dict[str, str],
) -> BoundedSubprocessResult:
    if (
        not command
        or maximum_bytes <= 0
        or timeout_seconds <= 0
        or any(not isinstance(argument, str) or not argument for argument in command)
    ):
        raise HostNoiseError("bounded host command policy is invalid")
    try:
        process = subprocess.Popen(
            tuple(command),
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            env=dict(environment),
            close_fds=True,
        )
    except OSError as error:
        raise HostNoiseError("bounded host command could not start") from error
    if process.stdout is None:
        process.kill()
        process.wait()
        raise HostNoiseError("bounded host command output pipe is unavailable")
    reader = BoundedOutputReader(process.stdout, maximum_bytes)
    reader.thread.start()
    deadline = time.monotonic() + timeout_seconds
    timed_out = False
    while not reader.done.wait(timeout=0.005):
        if time.monotonic() >= deadline:
            timed_out = True
            break
    if timed_out or reader.overflow:
        process.kill()
    try:
        returncode = process.wait(timeout=1.0)
    except subprocess.TimeoutExpired as error:
        process.kill()
        process.wait()
        raise HostNoiseError("bounded host command did not terminate") from error
    reader.thread.join(timeout=1.0)
    if reader.thread.is_alive():
        process.stdout.close()
        reader.thread.join(timeout=1.0)
    if reader.thread.is_alive():
        raise HostNoiseError("bounded host command output reader did not terminate")
    process.stdout.close()
    if timed_out:
        raise HostNoiseError("bounded host command timed out")
    if reader.overflow:
        raise HostNoiseError("bounded host command output exceeded its limit")
    if reader.error is not None:
        raise HostNoiseError("bounded host command output failed") from reader.error
    return BoundedSubprocessResult(returncode=returncode, output=bytes(reader.output))


@dataclass(frozen=True)
class CpuTimes:
    total: int
    busy: int


@dataclass(frozen=True)
class HostSnapshot:
    cpu: CpuTimes
    load_average_1m: float | None
    memory_pressure: str
    thermal_state: str


@dataclass(frozen=True)
class MonitorPoint:
    sequence: int
    monotonic_ns: int
    scheduler_lag_seconds: float
    snapshot: HostSnapshot


@dataclass(frozen=True)
class HostIntervalToken:
    sequence: int
    monotonic_ns: int
    snapshot: HostSnapshot


@dataclass(frozen=True)
class HostIntervalEvidence:
    scheduler_lag_max_seconds: float
    system_cpu_busy_ratio: str
    load_average_1m: str
    memory_pressure: str
    thermal_state: str
    monitor_samples: int


class HostAdapter(Protocol):
    name: str

    def snapshot(self) -> HostSnapshot:
        """Return one bounded system-wide snapshot."""


def _finite_nonnegative(value: float, label: str) -> float:
    if value < 0 or not (value < float("inf")):
        raise HostNoiseError(f"host monitor {label} is invalid")
    return value


def _cpu_interval_ratio(starting: CpuTimes, ending: CpuTimes) -> str:
    if (
        starting.total < 0
        or starting.busy < 0
        or starting.busy > starting.total
        or ending.total < 0
        or ending.busy < 0
        or ending.busy > ending.total
    ):
        raise HostNoiseError("host monitor CPU counters are invalid")
    total_delta = ending.total - starting.total
    busy_delta = ending.busy - starting.busy
    if total_delta == 0 and busy_delta == 0:
        return "unobserved"
    if total_delta <= 0 or busy_delta < 0 or busy_delta > total_delta:
        raise HostNoiseError("host monitor CPU interval is invalid")
    return f"{busy_delta / total_delta:.9f}"


def _load_average() -> float | None:
    try:
        value = float(os.getloadavg()[0])
    except (AttributeError, OSError):
        return None
    return _finite_nonnegative(value, "load average")


def _state_rank(state: str) -> int:
    ranks = {"normal": 0, "warning": 1, "critical": 2, "unsupported": 3}
    try:
        return ranks[state]
    except KeyError as error:
        raise HostNoiseError("host monitor returned an invalid categorical state") from error


def _worst_state(states: Sequence[str]) -> str:
    return max(states, key=_state_rank)


class DarwinHostAdapter:
    name = "darwin-stdlib-v1"

    def __init__(self) -> None:
        self._library = ctypes.CDLL(None, use_errno=True)
        self._library.mach_host_self.argtypes = []
        self._library.mach_host_self.restype = DarwinPort
        self._library.host_processor_info.argtypes = [
            DarwinPort,
            ctypes.c_int,
            ctypes.POINTER(DarwinNatural),
            ctypes.POINTER(ctypes.POINTER(DarwinInteger)),
            ctypes.POINTER(DarwinNatural),
        ]
        self._library.host_processor_info.restype = ctypes.c_int
        self._library.vm_deallocate.argtypes = [
            DarwinPort,
            ctypes.c_size_t,
            ctypes.c_size_t,
        ]
        self._library.vm_deallocate.restype = ctypes.c_int
        self._task_port = DarwinPort.in_dll(
            self._library, "mach_task_self_"
        ).value
        self._library.sysctlbyname.argtypes = [
            ctypes.c_char_p,
            ctypes.c_void_p,
            ctypes.POINTER(ctypes.c_size_t),
            ctypes.c_void_p,
            ctypes.c_size_t,
        ]
        self._library.sysctlbyname.restype = ctypes.c_int
        self._thermal_lock = threading.Lock()
        self._thermal_checked_at = 0.0
        self._thermal_state = "unsupported"

    def _cpu_times(self) -> CpuTimes:
        processor_count = DarwinNatural()
        information = ctypes.POINTER(DarwinInteger)()
        information_count = DarwinNatural()
        result = self._library.host_processor_info(
            self._library.mach_host_self(),
            DARWIN_PROCESSOR_CPU_LOAD_INFO,
            ctypes.byref(processor_count),
            ctypes.byref(information),
            ctypes.byref(information_count),
        )
        if result != 0:
            raise HostNoiseError("Darwin system CPU telemetry is unavailable")
        count = int(information_count.value)
        address = ctypes.cast(information, ctypes.c_void_p).value
        byte_count = count * ctypes.sizeof(DarwinInteger)
        try:
            processors = int(processor_count.value)
            if (
                address is None
                or processors <= 0
                or processors > DARWIN_MAX_PROCESSORS
                or count != processors * DARWIN_CPU_STATE_COUNT
            ):
                raise HostNoiseError("Darwin system CPU telemetry is invalid")
            unsigned_information = ctypes.cast(
                information, ctypes.POINTER(DarwinNatural)
            )
            ticks = [int(unsigned_information[index]) for index in range(count)]
            idle = sum(
                ticks[index + DARWIN_CPU_STATE_IDLE]
                for index in range(0, count, DARWIN_CPU_STATE_COUNT)
            )
            total = sum(ticks)
        finally:
            if address is not None and self._library.vm_deallocate(
                self._task_port, address, byte_count
            ) != 0:
                raise HostNoiseError("Darwin system CPU telemetry could not be released")
        return CpuTimes(total=total, busy=total - idle)

    def _memory_pressure(self) -> str:
        value = ctypes.c_int()
        size = ctypes.c_size_t(ctypes.sizeof(value))
        result = self._library.sysctlbyname(
            b"kern.memorystatus_vm_pressure_level",
            ctypes.byref(value),
            ctypes.byref(size),
            None,
            0,
        )
        if result != 0 or size.value != ctypes.sizeof(value):
            return "unsupported"
        return {1: "normal", 2: "warning", 4: "critical"}.get(
            int(value.value), "unsupported"
        )

    def _read_thermal_state(self) -> str:
        now = time.monotonic()
        with self._thermal_lock:
            if now - self._thermal_checked_at < DARWIN_THERMAL_CACHE_SECONDS:
                return self._thermal_state
            try:
                completed = bounded_subprocess_output(
                    ("/usr/bin/pmset", "-g", "therm"),
                    maximum_bytes=MAX_HOST_COMMAND_BYTES,
                    timeout_seconds=HOST_COMMAND_TIMEOUT_SECONDS,
                    environment={"LC_ALL": "C", "PATH": "/usr/bin:/bin"},
                )
                output = completed.output.decode("utf-8", "strict")
            except (HostNoiseError, UnicodeError):
                state = "unsupported"
            else:
                normal_markers = (
                    "No thermal warning level has been recorded",
                    "No performance warning level has been recorded",
                )
                if completed.returncode != 0:
                    state = "unsupported"
                elif all(marker in output for marker in normal_markers):
                    state = "normal"
                elif re.search(r"(?i)(critical|danger)", output):
                    state = "critical"
                else:
                    state = "warning"
            self._thermal_checked_at = now
            self._thermal_state = state
            return state

    def snapshot(self) -> HostSnapshot:
        return HostSnapshot(
            cpu=self._cpu_times(),
            load_average_1m=_load_average(),
            memory_pressure=self._memory_pressure(),
            thermal_state=self._read_thermal_state(),
        )


class LinuxHostAdapter:
    name = "linux-stdlib-v1"

    def __init__(
        self,
        proc_root: pathlib.Path = pathlib.Path("/proc"),
        thermal_root: pathlib.Path = pathlib.Path("/sys/class/thermal"),
    ) -> None:
        self._proc_root = proc_root
        self._thermal_root = thermal_root

    def _cpu_times(self) -> CpuTimes:
        try:
            fields = (self._proc_root / "stat").read_text(
                encoding="ascii", errors="strict"
            ).splitlines()[0].split()
            if not fields or fields[0] != "cpu" or len(fields) < 8:
                raise ValueError
            values = [int(value) for value in fields[1:]]
        except (OSError, UnicodeError, ValueError) as error:
            raise HostNoiseError("Linux system CPU telemetry is unavailable") from error
        if any(value < 0 for value in values):
            raise HostNoiseError("Linux system CPU telemetry is invalid")
        idle = values[3] + (values[4] if len(values) > 4 else 0)
        total = sum(values)
        return CpuTimes(total=total, busy=total - idle)

    def _memory_pressure(self) -> str:
        try:
            lines = (self._proc_root / "pressure" / "memory").read_text(
                encoding="ascii", errors="strict"
            ).splitlines()
            full = next(line for line in lines if line.startswith("full "))
            fields = dict(field.split("=", 1) for field in full.split()[1:])
            average = float(fields["avg10"])
        except (OSError, UnicodeError, KeyError, StopIteration, ValueError):
            return "unsupported"
        if average < 0 or not (average < float("inf")):
            return "unsupported"
        return "normal" if average == 0 else "warning"

    def _thermal_state(self) -> str:
        zones = sorted(self._thermal_root.glob("thermal_zone*"))
        observed = False
        warning = False
        try:
            for zone in zones:
                temperature = int((zone / "temp").read_text(encoding="ascii").strip())
                pressure_thresholds: list[int] = []
                for type_path in zone.glob("trip_point_*_type"):
                    trip_type = type_path.read_text(encoding="ascii").strip().lower()
                    if trip_type not in {"critical", "hot", "passive"}:
                        continue
                    prefix = type_path.name.removesuffix("_type")
                    pressure_thresholds.append(
                        int((zone / f"{prefix}_temp").read_text(encoding="ascii").strip())
                    )
                if pressure_thresholds:
                    observed = True
                    warning = warning or temperature >= min(pressure_thresholds)
        except (OSError, UnicodeError, ValueError):
            return "unsupported"
        if not observed:
            return "unsupported"
        return "warning" if warning else "normal"

    def snapshot(self) -> HostSnapshot:
        return HostSnapshot(
            cpu=self._cpu_times(),
            load_average_1m=_load_average(),
            memory_pressure=self._memory_pressure(),
            thermal_state=self._thermal_state(),
        )


WindowsDword = ctypes.c_uint32
WindowsBool = ctypes.c_int32
WindowsUlong = ctypes.c_uint32


class WindowsFileTime(ctypes.Structure):
    _fields_ = (("low", WindowsDword), ("high", WindowsDword))

    def value(self) -> int:
        return (int(self.high) << 32) | int(self.low)


class WindowsMemoryStatus(ctypes.Structure):
    _fields_ = (
        ("length", WindowsDword),
        ("memory_load", WindowsDword),
        ("total_physical", ctypes.c_ulonglong),
        ("available_physical", ctypes.c_ulonglong),
        ("total_page_file", ctypes.c_ulonglong),
        ("available_page_file", ctypes.c_ulonglong),
        ("total_virtual", ctypes.c_ulonglong),
        ("available_virtual", ctypes.c_ulonglong),
        ("available_extended_virtual", ctypes.c_ulonglong),
    )


class WindowsPowerInformation(ctypes.Structure):
    _fields_ = (
        ("maximum_idleness_allowed", WindowsUlong),
        ("idleness", WindowsUlong),
        ("time_remaining", WindowsUlong),
        ("cooling_mode", ctypes.c_ubyte),
    )


class WindowsHostAdapter:
    name = "windows-stdlib-v1"

    def __init__(self) -> None:
        self._kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        self._power = ctypes.WinDLL("powrprof", use_last_error=True)
        file_time_pointer = ctypes.POINTER(WindowsFileTime)
        self._kernel.GetSystemTimes.argtypes = (
            file_time_pointer,
            file_time_pointer,
            file_time_pointer,
        )
        self._kernel.GetSystemTimes.restype = WindowsBool
        self._kernel.GlobalMemoryStatusEx.argtypes = (
            ctypes.POINTER(WindowsMemoryStatus),
        )
        self._kernel.GlobalMemoryStatusEx.restype = WindowsBool
        self._power.CallNtPowerInformation.argtypes = (
            ctypes.c_int,
            ctypes.c_void_p,
            WindowsUlong,
            ctypes.c_void_p,
            WindowsUlong,
        )
        self._power.CallNtPowerInformation.restype = WindowsUlong

    def _cpu_times(self) -> CpuTimes:
        idle = WindowsFileTime()
        kernel = WindowsFileTime()
        user = WindowsFileTime()
        if not self._kernel.GetSystemTimes(
            ctypes.byref(idle), ctypes.byref(kernel), ctypes.byref(user)
        ):
            raise HostNoiseError("Windows system CPU telemetry is unavailable")
        total = kernel.value() + user.value()
        return CpuTimes(total=total, busy=total - idle.value())

    def _memory_pressure(self) -> str:
        status = WindowsMemoryStatus()
        status.length = ctypes.sizeof(status)
        if not self._kernel.GlobalMemoryStatusEx(ctypes.byref(status)):
            return "unsupported"
        if status.memory_load >= 97:
            return "critical"
        return "warning" if status.memory_load >= 90 else "normal"

    def _thermal_state(self) -> str:
        information = WindowsPowerInformation()
        result = self._power.CallNtPowerInformation(
            12,
            None,
            0,
            ctypes.byref(information),
            ctypes.sizeof(information),
        )
        if result != 0:
            return "unsupported"
        return {0: "normal", 1: "warning"}.get(
            int(information.cooling_mode), "unsupported"
        )

    def snapshot(self) -> HostSnapshot:
        return HostSnapshot(
            cpu=self._cpu_times(),
            load_average_1m=None,
            memory_pressure=self._memory_pressure(),
            thermal_state=self._thermal_state(),
        )


def current_host_adapter(system: str | None = None) -> HostAdapter:
    selected = platform.system() if system is None else system
    if selected == "Darwin":
        return DarwinHostAdapter()
    if selected == "Linux":
        return LinuxHostAdapter()
    if selected == "Windows":
        return WindowsHostAdapter()
    raise HostNoiseError("host monitor platform is unsupported")


class HostNoiseMonitor:
    def __init__(
        self,
        adapter: HostAdapter,
        *,
        sample_interval_seconds: float = SAMPLE_INTERVAL_SECONDS,
        maximum_points: int = MAX_MONITOR_POINTS,
    ) -> None:
        if sample_interval_seconds <= 0 or maximum_points <= 0:
            raise HostNoiseError("host monitor bounds are invalid")
        self.adapter = adapter
        self.sample_interval_seconds = sample_interval_seconds
        self.maximum_points = maximum_points
        self._condition = threading.Condition()
        self._points: collections.deque[MonitorPoint] = collections.deque(
            maxlen=maximum_points
        )
        self._sequence = 0
        self._worst_memory_pressure = "normal"
        self._worst_thermal_state = "normal"
        self._error: BaseException | None = None
        self._started = False
        self._closed = False
        self._stop = threading.Event()
        self._ready = threading.Event()
        self._thread = threading.Thread(
            target=self._sample_loop,
            name="zmin-performance-host-noise",
            daemon=True,
        )

    def __enter__(self) -> HostNoiseMonitor:
        self.start()
        return self

    def __exit__(self, _type: object, _value: object, _traceback: object) -> None:
        self.close()

    def _raise_if_failed(self) -> None:
        if self._error is not None:
            raise HostNoiseError("host monitor sampling failed") from self._error

    def start(self) -> None:
        if self._started:
            raise HostNoiseError("host monitor was started twice")
        self._started = True
        self._thread.start()
        try:
            if not self._ready.wait(timeout=3.0):
                raise HostNoiseError("host monitor did not start")
            self._raise_if_failed()
        except BaseException:
            self._closed = True
            self._stop.set()
            self._thread.join(timeout=MONITOR_JOIN_SECONDS)
            raise

    def _append_point(
        self,
        monotonic_ns: int,
        scheduler_lag_seconds: float,
        snapshot: HostSnapshot,
    ) -> None:
        with self._condition:
            self._sequence += 1
            self._worst_memory_pressure = _worst_state(
                (self._worst_memory_pressure, snapshot.memory_pressure)
            )
            self._worst_thermal_state = _worst_state(
                (self._worst_thermal_state, snapshot.thermal_state)
            )
            self._points.append(
                MonitorPoint(
                    sequence=self._sequence,
                    monotonic_ns=monotonic_ns,
                    scheduler_lag_seconds=scheduler_lag_seconds,
                    snapshot=snapshot,
                )
            )
            self._condition.notify_all()

    def _sample_loop(self) -> None:
        deadline = time.monotonic()
        try:
            snapshot = self.adapter.snapshot()
            self._append_point(time.monotonic_ns(), 0.0, snapshot)
            self._ready.set()
            while not self._stop.is_set():
                deadline += self.sample_interval_seconds
                if self._stop.wait(max(0.0, deadline - time.monotonic())):
                    break
                observed = time.monotonic()
                snapshot = self.adapter.snapshot()
                self._append_point(
                    time.monotonic_ns(),
                    max(0.0, observed - deadline),
                    snapshot,
                )
        except BaseException as error:
            with self._condition:
                self._error = error
                self._condition.notify_all()
            self._ready.set()

    def begin_interval(self) -> HostIntervalToken:
        if not self._started or self._closed:
            raise HostNoiseError("host monitor is not active")
        self._raise_if_failed()
        snapshot = self.adapter.snapshot()
        with self._condition:
            sequence = self._sequence
        return HostIntervalToken(
            sequence=sequence,
            monotonic_ns=time.monotonic_ns(),
            snapshot=snapshot,
        )

    def end_interval(self, token: HostIntervalToken) -> HostIntervalEvidence:
        if self._closed:
            raise HostNoiseError("host monitor is closed")
        ending = self.adapter.snapshot()
        ended_ns = time.monotonic_ns()
        self._raise_if_failed()
        with self._condition:
            if self._points and token.sequence < self._points[0].sequence - 1:
                raise HostNoiseError("host monitor interval exceeded its bounded history")
            points = [
                point
                for point in self._points
                if point.sequence > token.sequence and point.monotonic_ns <= ended_ns
            ]
        snapshots = [token.snapshot, *(point.snapshot for point in points), ending]
        cpu_busy_ratio = _cpu_interval_ratio(token.snapshot.cpu, ending.cpu)
        memory_pressure = _worst_state(
            [snapshot.memory_pressure for snapshot in snapshots]
        )
        thermal_state = _worst_state([snapshot.thermal_state for snapshot in snapshots])
        if memory_pressure != "normal":
            raise HostNoiseError("host memory pressure was not normal")
        if thermal_state != "normal":
            raise HostNoiseError("host thermal state was not normal")
        loads = [
            snapshot.load_average_1m
            for snapshot in snapshots
            if snapshot.load_average_1m is not None
        ]
        return HostIntervalEvidence(
            scheduler_lag_max_seconds=max(
                (point.scheduler_lag_seconds for point in points), default=0.0
            ),
            system_cpu_busy_ratio=cpu_busy_ratio,
            load_average_1m=(
                "unsupported" if not loads else f"{max(loads):.9f}"
            ),
            memory_pressure=memory_pressure,
            thermal_state=thermal_state,
            monitor_samples=len(points) + 2,
        )

    def close(self) -> None:
        if self._closed:
            return
        self._closed = True
        self._stop.set()
        if self._started:
            self._thread.join(timeout=MONITOR_JOIN_SECONDS)
            if self._thread.is_alive():
                raise HostNoiseError("host monitor did not stop")
        self._raise_if_failed()
        with self._condition:
            memory_pressure = self._worst_memory_pressure
            thermal_state = self._worst_thermal_state
        if memory_pressure != "normal":
            raise HostNoiseError("host memory pressure was not normal")
        if thermal_state != "normal":
            raise HostNoiseError("host thermal state was not normal")
