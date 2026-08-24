#!/usr/bin/env python3
"""Focused tests for the streaming Git LFS performance harness."""

from __future__ import annotations

import concurrent.futures
import hashlib
import http.client
import importlib.util
import json
import os
import pathlib
import shutil
import stat
import subprocess
import sys
import tempfile
import tracemalloc
import unittest
import urllib.parse
from dataclasses import replace
from unittest import mock


def load_benchmark_module():
    module_path = pathlib.Path(__file__).with_name("lfs-performance-bench.py")
    spec = importlib.util.spec_from_file_location("lfs_performance_bench_test", module_path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {module_path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


bench = load_benchmark_module()


class FixtureHostAdapter:
    name = "fixture-stdlib-v1"

    def __init__(self, snapshots, failure_at=None):
        self.snapshots = tuple(snapshots)
        self.failure_at = failure_at
        self.calls = 0
        self.lock = bench.threading.Lock()

    def snapshot(self):
        with self.lock:
            self.calls += 1
            if self.failure_at == self.calls:
                raise bench.host_noise.HostNoiseError("fixture monitor failure")
            return self.snapshots[min(self.calls - 1, len(self.snapshots) - 1)]


class FixtureDarwinCpuLibrary:
    def __init__(
        self,
        tick_snapshots,
        *,
        processor_counts=None,
        deallocate_result=0,
    ):
        self.tick_snapshots = tuple(tick_snapshots)
        self.processor_counts = (
            None if processor_counts is None else tuple(processor_counts)
        )
        self.deallocate_result = deallocate_result
        self.calls = 0
        self.allocations = []
        self.deallocations = []

    def mach_host_self(self):
        return 7

    def host_processor_info(
        self,
        _host,
        flavor,
        processor_count,
        information,
        information_count,
    ):
        ctypes = bench.host_noise.ctypes
        call_index = self.calls
        ticks = self.tick_snapshots[call_index]
        self.calls += 1
        allocation = (ctypes.c_uint * len(ticks))(*ticks)
        self.allocations.append(allocation)
        ctypes.cast(processor_count, ctypes.POINTER(ctypes.c_uint))[0] = int(
            len(ticks) // bench.host_noise.DARWIN_CPU_STATE_COUNT
            if self.processor_counts is None
            else self.processor_counts[call_index]
        )
        ctypes.cast(information_count, ctypes.POINTER(ctypes.c_uint))[0] = len(ticks)
        pointer_type = ctypes.POINTER(ctypes.c_uint)
        ctypes.cast(information, ctypes.POINTER(pointer_type))[0] = ctypes.cast(
            allocation, pointer_type
        )
        self.last_flavor = flavor
        return 0

    def vm_deallocate(self, task_port, address, byte_count):
        self.deallocations.append((task_port, address, byte_count))
        return self.deallocate_result


class FixtureNativeFunction:
    def __init__(self):
        self.argtypes = None
        self.restype = None

    def __call__(self, *_arguments):
        return 1


class FixtureNativeLibrary:
    def __init__(self, functions):
        for name in functions:
            setattr(self, name, FixtureNativeFunction())


class LfsPerformanceBenchTests(unittest.TestCase):
    def test_fixture_generation_is_streaming_and_deterministic(self) -> None:
        object_bytes = 2 * 1024 * 1024 + 17
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            tracemalloc.start()
            objects = bench.create_fixture_objects(root / "first", 2, object_bytes)
            _current, peak = tracemalloc.get_traced_memory()
            tracemalloc.stop()
            self.assertLess(peak, object_bytes)
            duplicate = bench.create_fixture_objects(root / "second", 2, object_bytes)
            self.assertEqual(
                [(item.oid, item.size) for item in objects],
                [(item.oid, item.size) for item in duplicate],
            )
            for item in objects:
                digest, size = bench.hash_file(item.path)
                self.assertEqual(digest, item.oid)
                self.assertEqual(size, object_bytes)

    def test_fixture_streams_download_and_upload_with_exact_concurrency(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            objects = bench.create_fixture_objects(
                pathlib.Path(directory) / "objects",
                2,
                3 * bench.STREAM_CHUNK_BYTES + 17,
            )
            with bench.FixtureServer() as fixture:
                self._exercise_download(fixture, objects)
                self._exercise_upload(fixture, objects)

    def test_order_is_deterministic_and_alternating(self) -> None:
        self.assertEqual(
            bench.sample_order(0, 0),
            (bench.BenchmarkTool.STOCK, bench.BenchmarkTool.ZMIN),
        )
        self.assertEqual(
            bench.sample_order(0, 1),
            (bench.BenchmarkTool.ZMIN, bench.BenchmarkTool.STOCK),
        )
        self.assertEqual(bench.sample_order(1, 0), bench.sample_order(0, 1))

    def test_v4_block_schedule_shape_binding_balance_and_p95(self) -> None:
        policy = bench.BenchmarkPolicy(
            object_count=bench.GATE_OBJECT_COUNT,
            object_bytes=bench.GATE_OBJECT_BYTES,
            concurrencies=bench.GATE_CONCURRENCIES,
            warmups=bench.DEFAULT_WARMUPS,
            repeats=bench.DEFAULT_REPEATS,
            child_timeout_seconds=bench.DEFAULT_CHILD_TIMEOUT_SECONDS,
            strict_gate=True,
        )
        policy.validate()
        evidence_policy = bench.EvidencePolicy.from_benchmark_policy(policy)
        self.assertEqual(evidence_policy.raw_row_count, 504)
        binding_values = {
            key: f"fixture-{key}" for key in bench.SCHEDULE_BINDING_FIELDS
        }
        first_binding = bench.schedule_binding_from_metadata(binding_values)
        changed_values = dict(binding_values)
        changed_values["zmin_sha256"] = "changed-zmin"
        second_binding = bench.schedule_binding_from_metadata(changed_values)
        self.assertNotEqual(first_binding, second_binding)
        changed_rounds = 0
        lanes = bench.canonical_lanes(evidence_policy)
        for sample_index in range(1, bench.DEFAULT_REPEATS + 1):
            scheduled = bench.scheduled_lanes(
                evidence_policy,
                first_binding,
                bench.SampleKind.MEASURED,
                sample_index,
            )
            self.assertEqual(set(scheduled), set(lanes))
            self.assertEqual(
                scheduled,
                bench.scheduled_lanes(
                    evidence_policy,
                    first_binding,
                    bench.SampleKind.MEASURED,
                    sample_index,
                ),
            )
            changed_rounds += scheduled != bench.scheduled_lanes(
                evidence_policy,
                second_binding,
                bench.SampleKind.MEASURED,
                sample_index,
            )
        self.assertGreater(changed_rounds, 0)
        for lane_index in range(len(lanes)):
            orders = [
                bench.sample_order(lane_index, index)
                for index in range(bench.DEFAULT_REPEATS)
            ]
            self.assertEqual(
                sum(order[0] == bench.BenchmarkTool.STOCK for order in orders),
                20,
            )
            self.assertEqual(
                sum(order[0] == bench.BenchmarkTool.ZMIN for order in orders),
                20,
            )
        self.assertEqual(bench.nearest_rank_p95(tuple(range(1, 41))), 38)
        self.assertEqual(
            bench.paired_median_upper_95([0.5] * 26 + [1.5] * 14),
            0.5,
        )
        self.assertEqual(
            bench.paired_median_upper_95([0.5] * 25 + [1.0] * 15),
            1.0,
        )

    def test_host_monitor_clean_pressure_crash_and_platform_adapters(self) -> None:
        noise = bench.host_noise
        normal = noise.HostSnapshot(
            cpu=noise.CpuTimes(total=100, busy=20),
            load_average_1m=1.0,
            memory_pressure="normal",
            thermal_state="normal",
        )
        later = replace(normal, cpu=noise.CpuTimes(total=200, busy=60))
        adapter = FixtureHostAdapter((normal, normal, later))
        with noise.HostNoiseMonitor(
            adapter, sample_interval_seconds=1.0
        ) as monitor:
            token = monitor.begin_interval()
            evidence = monitor.end_interval(token)
        self.assertEqual(evidence.memory_pressure, "normal")
        self.assertEqual(evidence.thermal_state, "normal")
        self.assertGreaterEqual(evidence.monitor_samples, 2)
        self.assertGreaterEqual(float(evidence.system_cpu_busy_ratio), 0)

        pressured = replace(later, memory_pressure="warning")
        adapter = FixtureHostAdapter((normal, normal, pressured))
        with noise.HostNoiseMonitor(
            adapter, sample_interval_seconds=1.0
        ) as monitor:
            token = monitor.begin_interval()
            with self.assertRaisesRegex(noise.HostNoiseError, "memory pressure"):
                monitor.end_interval(token)

        with self.assertRaisesRegex(noise.HostNoiseError, "sampling failed"):
            with noise.HostNoiseMonitor(
                FixtureHostAdapter((normal,), failure_at=1),
                sample_interval_seconds=1.0,
            ):
                pass
        background_pressure = replace(normal, memory_pressure="warning")
        with self.assertRaisesRegex(noise.HostNoiseError, "memory pressure"):
            with noise.HostNoiseMonitor(
                FixtureHostAdapter((normal, background_pressure)),
                sample_interval_seconds=0.001,
            ):
                bench.time.sleep(0.02)
        with self.assertRaisesRegex(noise.HostNoiseError, "unsupported"):
            noise.current_host_adapter("Plan9")

        darwin = object.__new__(noise.DarwinHostAdapter)
        with (
            mock.patch.object(noise.DarwinHostAdapter, "_cpu_times", return_value=normal.cpu),
            mock.patch.object(
                noise.DarwinHostAdapter, "_memory_pressure", return_value="normal"
            ),
            mock.patch.object(
                noise.DarwinHostAdapter, "_read_thermal_state", return_value="normal"
            ),
        ):
            self.assertEqual(darwin.snapshot().thermal_state, "normal")
        linux = object.__new__(noise.LinuxHostAdapter)
        with (
            mock.patch.object(noise.LinuxHostAdapter, "_cpu_times", return_value=normal.cpu),
            mock.patch.object(
                noise.LinuxHostAdapter, "_memory_pressure", return_value="normal"
            ),
            mock.patch.object(
                noise.LinuxHostAdapter, "_thermal_state", return_value="normal"
            ),
        ):
            self.assertEqual(linux.snapshot().memory_pressure, "normal")
        windows = object.__new__(noise.WindowsHostAdapter)
        with (
            mock.patch.object(noise.WindowsHostAdapter, "_cpu_times", return_value=normal.cpu),
            mock.patch.object(
                noise.WindowsHostAdapter, "_memory_pressure", return_value="normal"
            ),
            mock.patch.object(
                noise.WindowsHostAdapter, "_thermal_state", return_value="normal"
            ),
        ):
            self.assertIsNone(windows.snapshot().load_average_1m)

        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            proc_root = root / "proc"
            pressure = proc_root / "pressure"
            pressure.mkdir(parents=True)
            (proc_root / "stat").write_text(
                "cpu  10 2 3 80 5 0 0 0\n", encoding="ascii"
            )
            (pressure / "memory").write_text(
                "some avg10=0.00 avg60=0.00 avg300=0.00 total=0\n"
                "full avg10=0.00 avg60=0.00 avg300=0.00 total=0\n",
                encoding="ascii",
            )
            thermal_root = root / "thermal"
            zone = thermal_root / "thermal_zone0"
            zone.mkdir(parents=True)
            (zone / "temp").write_text("40000\n", encoding="ascii")
            (zone / "trip_point_0_type").write_text("critical\n", encoding="ascii")
            (zone / "trip_point_0_temp").write_text("90000\n", encoding="ascii")
            adapter = noise.LinuxHostAdapter(proc_root, thermal_root)
            snapshot = adapter.snapshot()
            self.assertEqual(snapshot.cpu, noise.CpuTimes(total=100, busy=15))
            self.assertEqual(snapshot.memory_pressure, "normal")
            self.assertEqual(snapshot.thermal_state, "normal")
            (pressure / "memory").write_text(
                "some avg10=1.00 avg60=0.00 avg300=0.00 total=1\n"
                "full avg10=0.25 avg60=0.00 avg300=0.00 total=1\n",
                encoding="ascii",
            )
            (zone / "temp").write_text("95000\n", encoding="ascii")
            snapshot = adapter.snapshot()
            self.assertEqual(snapshot.memory_pressure, "warning")
            self.assertEqual(snapshot.thermal_state, "warning")

    def test_host_monitor_accepts_repeated_short_idle_intervals(self) -> None:
        noise = bench.host_noise

        def snapshot(total, busy):
            return noise.HostSnapshot(
                cpu=noise.CpuTimes(total=total, busy=busy),
                load_average_1m=0.5,
                memory_pressure="normal",
                thermal_state="normal",
            )

        adapter = FixtureHostAdapter(
            (
                snapshot(100, 20),
                snapshot(100, 20),
                snapshot(100, 20),
                snapshot(100, 20),
                snapshot(101, 20),
                snapshot(101, 20),
                snapshot(103, 21),
                snapshot(103, 21),
                snapshot(106, 22),
            )
        )
        ratios = []
        with noise.HostNoiseMonitor(
            adapter, sample_interval_seconds=60.0
        ) as monitor:
            for _index in range(4):
                token = monitor.begin_interval()
                ratios.append(monitor.end_interval(token).system_cpu_busy_ratio)
        self.assertEqual(
            ratios,
            ["unobserved", "0.000000000", "0.500000000", "0.333333333"],
        )
        self.assertEqual(adapter.calls, 9)

    def test_cpu_interval_marks_zero_unobserved_and_rejects_invalid_deltas(self) -> None:
        noise = bench.host_noise
        starting = noise.CpuTimes(total=100, busy=20)
        self.assertEqual(
            noise._cpu_interval_ratio(starting, noise.CpuTimes(total=100, busy=20)),
            "unobserved",
        )
        cases = (
            ("zero-total-busy-change", noise.CpuTimes(total=100, busy=21)),
            ("rollback", noise.CpuTimes(total=99, busy=20)),
            ("busy-rollback", noise.CpuTimes(total=110, busy=19)),
            ("busy-overflow", noise.CpuTimes(total=110, busy=40)),
        )
        for name, ending in cases:
            with self.subTest(name=name):
                with self.assertRaisesRegex(noise.HostNoiseError, "CPU interval"):
                    noise._cpu_interval_ratio(starting, ending)
        for value in ("unobserved", "0.000000000", "0.500000000", "1.000000000"):
            bench.validate_host_cpu_busy_ratio(value)
        for value in (
            "Unobserved",
            " unobserved",
            "unobserved ",
            "0",
            "0.0",
            "+0.000000000",
            "-0.000000000",
            "00.000000000",
            "0e0",
            "0.50000000",
            "0.5000000000",
            "nan",
        ):
            with self.subTest(value=value):
                with self.assertRaisesRegex(
                    bench.BenchmarkError, "host CPU diagnostic"
                ):
                    bench.validate_host_cpu_busy_ratio(value)

    def test_darwin_cpu_uses_fresh_per_processor_ticks_and_releases_memory(self) -> None:
        noise = bench.host_noise
        library = FixtureDarwinCpuLibrary(
            (
                (10, 5, 80, 1, 20, 6, 70, 2),
                (11, 6, 81, 1, 21, 7, 72, 2),
            )
        )
        adapter = object.__new__(noise.DarwinHostAdapter)
        adapter._library = library
        adapter._task_port = 9
        starting = adapter._cpu_times()
        ending = adapter._cpu_times()
        self.assertEqual(starting, noise.CpuTimes(total=194, busy=44))
        self.assertEqual(ending, noise.CpuTimes(total=201, busy=48))
        self.assertEqual(noise._cpu_interval_ratio(starting, ending), "0.571428571")
        self.assertEqual(
            library.last_flavor, noise.DARWIN_PROCESSOR_CPU_LOAD_INFO
        )
        self.assertEqual(len(library.deallocations), 2)
        self.assertTrue(all(item[0] == 9 for item in library.deallocations))
        self.assertTrue(
            all(
                item[2] == 8 * noise.ctypes.sizeof(noise.ctypes.c_uint)
                for item in library.deallocations
            )
        )

        malformed = FixtureDarwinCpuLibrary(
            ((10, 5, 80, 1, 20, 6, 70),), processor_counts=(2,)
        )
        adapter._library = malformed
        with self.assertRaisesRegex(noise.HostNoiseError, "telemetry is invalid"):
            adapter._cpu_times()
        self.assertEqual(len(malformed.deallocations), 1)

        release_failure = FixtureDarwinCpuLibrary(
            ((10, 5, 80, 1),), deallocate_result=1
        )
        adapter._library = release_failure
        with self.assertRaisesRegex(noise.HostNoiseError, "could not be released"):
            adapter._cpu_times()
        self.assertEqual(len(release_failure.deallocations), 1)

    def test_host_command_output_is_bounded_and_times_out(self) -> None:
        noise = bench.host_noise
        reader = noise.BoundedOutputReader(bench.io.BytesIO(b"x" * 2048), 1024)
        reader._read()
        self.assertTrue(reader.overflow)
        self.assertEqual(len(reader.output), 1024)
        environment = os.environ.copy()
        environment["PYTHONDONTWRITEBYTECODE"] = "1"
        completed = noise.bounded_subprocess_output(
            (sys.executable, "-B", "-c", "print('bounded')"),
            maximum_bytes=1024,
            timeout_seconds=2.0,
            environment=environment,
        )
        self.assertEqual(completed.returncode, 0)
        self.assertEqual(completed.output, b"bounded\n")
        with self.assertRaisesRegex(noise.HostNoiseError, "exceeded"):
            noise.bounded_subprocess_output(
                (
                    sys.executable,
                    "-B",
                    "-c",
                    "import os; os.write(1, b'x' * 20000)",
                ),
                maximum_bytes=1024,
                timeout_seconds=2.0,
                environment=environment,
            )
        started = bench.time.monotonic()
        with self.assertRaisesRegex(noise.HostNoiseError, "timed out"):
            noise.bounded_subprocess_output(
                (
                    sys.executable,
                    "-B",
                    "-c",
                    "import time; time.sleep(10)",
                ),
                maximum_bytes=1024,
                timeout_seconds=0.05,
                environment=environment,
            )
        self.assertLess(bench.time.monotonic() - started, 2.0)

    def test_windows_host_adapter_declares_native_signatures(self) -> None:
        noise = bench.host_noise
        kernel = FixtureNativeLibrary(("GetSystemTimes", "GlobalMemoryStatusEx"))
        power = FixtureNativeLibrary(("CallNtPowerInformation",))
        with mock.patch.object(
            noise.ctypes,
            "WinDLL",
            side_effect=(kernel, power),
            create=True,
        ):
            adapter = noise.WindowsHostAdapter()
        self.assertEqual(kernel.GetSystemTimes.restype, noise.WindowsBool)
        self.assertEqual(len(kernel.GetSystemTimes.argtypes), 3)
        self.assertEqual(kernel.GlobalMemoryStatusEx.restype, noise.WindowsBool)
        self.assertEqual(len(kernel.GlobalMemoryStatusEx.argtypes), 1)
        self.assertEqual(power.CallNtPowerInformation.restype, noise.WindowsUlong)
        self.assertEqual(len(power.CallNtPowerInformation.argtypes), 5)
        self.assertEqual(noise.ctypes.sizeof(noise.WindowsFileTime), 8)
        self.assertEqual(noise.ctypes.sizeof(noise.WindowsMemoryStatus), 64)
        self.assertEqual(noise.ctypes.sizeof(noise.WindowsPowerInformation), 16)
        self.assertIs(adapter._kernel, kernel)
        self.assertIs(adapter._power, power)

    def test_strict_policy_cannot_be_diluted_but_smoke_can_be_tiny(self) -> None:
        self.assertEqual(bench.LFS_PERFORMANCE_SCHEMA_VERSION, "4")
        self.assertEqual(bench.LFS_PERFORMANCE_SCHEMA_NAME, "zmin-lfs-performance-v4")
        strict = bench.BenchmarkPolicy(
            object_count=2,
            object_bytes=1024,
            concurrencies=(1, 2),
            warmups=1,
            repeats=5,
            child_timeout_seconds=10,
            strict_gate=True,
        )
        with self.assertRaisesRegex(bench.BenchmarkError, "strict gate requires"):
            strict.validate()
        smoke = replace(strict, repeats=1, strict_gate=False)
        smoke.validate()
        canonical = replace(
            strict,
            object_count=bench.GATE_OBJECT_COUNT,
            object_bytes=bench.GATE_OBJECT_BYTES,
            concurrencies=bench.GATE_CONCURRENCIES,
            warmups=bench.DEFAULT_WARMUPS,
            repeats=bench.DEFAULT_REPEATS,
            child_timeout_seconds=bench.DEFAULT_CHILD_TIMEOUT_SECONDS,
        )
        canonical.validate()
        for diluted in (
            replace(canonical, warmups=1),
            replace(canonical, repeats=39),
            replace(canonical, concurrencies=(2, 1, 8)),
            replace(canonical, concurrencies=(1, 1, 8)),
        ):
            with self.assertRaises(bench.BenchmarkError):
                diluted.validate()

    def test_cli_defaults_bind_the_strict_protocol(self) -> None:
        base_args = [
            "--repo-root",
            "/absolute/source",
            "--zmin-bin",
            "/absolute/zmin",
            "--zmin-sha256",
            "a" * 64,
            "--git-bin",
            "/absolute/git",
            "--git-sha256",
            "b" * 64,
            "--git-lfs-manifest",
            "/absolute/git-lfs.manifest.txt",
            "--output-dir",
            "/absolute/output",
            "--failure-dir",
            "/absolute/failed-output",
        ]
        args = bench.parse_args(base_args)
        policy = bench.BenchmarkPolicy.from_args(args)
        self.assertEqual(policy.object_count, 8)
        self.assertEqual(policy.object_bytes, 16 * 1024 * 1024)
        self.assertEqual(policy.concurrencies, (1, 2, 8))
        self.assertEqual(policy.warmups, 2)
        self.assertEqual(policy.repeats, 40)
        self.assertTrue(policy.strict_gate)
        for invalid_timeout in ("nan", "+inf", "-inf", "0", "-1", "3600.0001"):
            invalid_args = bench.parse_args(
                [*base_args, f"--timeout-seconds={invalid_timeout}"]
            )
            with self.assertRaisesRegex(bench.BenchmarkError, "finite"):
                bench.BenchmarkPolicy.from_args(invalid_args)
            diagnostic = bench.io.StringIO()
            with mock.patch.object(bench.sys, "stderr", diagnostic):
                self.assertEqual(
                    bench.main([*base_args, f"--timeout-seconds={invalid_timeout}"]),
                    2,
                )
            self.assertIn("finite", diagnostic.getvalue())
            self.assertNotIn("Traceback", diagnostic.getvalue())
        maximum = bench.parse_args(
            [*base_args, "--timeout-seconds", "3600", "--smoke"]
        )
        self.assertEqual(
            bench.BenchmarkPolicy.from_args(maximum).child_timeout_seconds,
            bench.MAX_CHILD_TIMEOUT_SECONDS,
        )

    def test_host_monitor_failure_is_sanitized_exit_two(self) -> None:
        arguments = [
            "--repo-root",
            "/absolute/source",
            "--zmin-bin",
            "/absolute/zmin",
            "--zmin-sha256",
            "a" * 64,
            "--git-bin",
            "/absolute/git",
            "--git-sha256",
            "b" * 64,
            "--git-lfs-manifest",
            "/absolute/git-lfs.manifest.txt",
            "--output-dir",
            "/absolute/output",
            "--failure-dir",
            "/absolute/failed-output",
        ]
        diagnostic = bench.io.StringIO()
        with (
            mock.patch.object(
                bench,
                "run_benchmark",
                side_effect=bench.host_noise.HostNoiseError(
                    "host monitor sampling failed"
                ),
            ),
            mock.patch.object(bench.sys, "stderr", diagnostic),
        ):
            self.assertEqual(bench.main(arguments), 2)
        self.assertIn("host monitor sampling failed", diagnostic.getvalue())
        self.assertNotIn("Traceback", diagnostic.getvalue())

    def test_nested_runner_forces_no_bytecode_in_clean_tools(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            source_tools = root / "source" / "tools"
            source_tools.mkdir(parents=True)
            current_tools = pathlib.Path(bench.__file__).resolve().parent
            for name in (
                "git-bench-process.py",
                "performance_contract.py",
                "windows_child_metrics.py",
            ):
                source = current_tools / name
                if source.is_file():
                    shutil.copy2(source, source_tools / name)
            probe_module = source_tools / "nested_probe_module.py"
            probe_script = source_tools / "nested_probe.py"
            probe_module.write_text("VALUE = 'clean'\n", encoding="ascii")
            probe_script.write_text(
                "import os\n"
                "import nested_probe_module\n"
                "print(os.environ.get('PYTHONDONTWRITEBYTECODE', '') + ':' "
                "+ nested_probe_module.VALUE)\n",
                encoding="ascii",
            )
            repository = root / "repository"
            metrics = root / "metrics"
            repository.mkdir()
            metrics.mkdir()
            environment = os.environ.copy()
            environment["PYTHONDONTWRITEBYTECODE"] = "0"
            actual_run = subprocess.run
            with mock.patch.object(
                bench.subprocess,
                "run",
                wraps=actual_run,
            ) as runner_call:
                exit_code, _process_metrics, _stdout_sha256, _stderr_sha256 = (
                    bench.measured_command(
                        source_tools / "git-bench-process.py",
                        metrics,
                        bench.contract.artifact_identity_token(
                            bench.contract.path_identity(metrics)
                        ),
                        "nested-python",
                        (sys.executable, "-B", str(probe_script)),
                        repository,
                        environment,
                        10.0,
                    )
                )
            self.assertEqual(exit_code, 0)
            self.assertEqual((metrics / "nested-python.stdout").read_bytes(), b"1:clean\n")
            invocation = runner_call.call_args.args[0]
            self.assertEqual(invocation[0:2], [sys.executable, "-B"])
            self.assertEqual(
                runner_call.call_args.kwargs["env"]["PYTHONDONTWRITEBYTECODE"],
                "1",
            )
            self.assertEqual(environment["PYTHONDONTWRITEBYTECODE"], "0")
            cache_paths = [
                path
                for path in source_tools.rglob("*")
                if path.name == "__pycache__" or path.suffix == ".pyc"
            ]
            self.assertEqual(cache_paths, [])

    def test_top_level_hostile_python_import_cannot_dirty_clean_source(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source = pathlib.Path(directory) / "clean-source"
            tools = source / "tools"
            tools.mkdir(parents=True)
            current_tools = pathlib.Path(bench.__file__).resolve().parent
            for name in (
                "lfs-performance-bench.py",
                "performance-host-noise.py",
                "performance_contract.py",
            ):
                shutil.copy2(current_tools / name, tools / name)
            environment = os.environ.copy()
            environment["PYTHONDONTWRITEBYTECODE"] = "0"
            result = subprocess.run(
                [sys.executable, str(tools / "lfs-performance-bench.py"), "--help"],
                cwd=source,
                env=environment,
                stdin=subprocess.DEVNULL,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
                timeout=10,
            )
            self.assertEqual(result.returncode, 0, result.stderr.decode("utf-8", "replace"))
            self.assertNotIn(b"Traceback", result.stderr)
            generated = [
                path
                for path in source.rglob("*")
                if path.name == "__pycache__" or path.suffix == ".pyc"
            ]
            self.assertEqual(generated, [])

    def test_symlink_invocation_uses_only_provenance_bound_tools(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            trusted_root = root / "trusted-source"
            trusted_tools = trusted_root / "tools"
            hostile_tools = root / "hostile-tools"
            trusted_tools.mkdir(parents=True)
            hostile_tools.mkdir()
            current_tools = pathlib.Path(bench.__file__).resolve().parent
            for name in (
                "lfs-performance-bench.py",
                "performance-host-noise.py",
                "performance_contract.py",
            ):
                shutil.copy2(current_tools / name, trusted_tools / name)
            marker = hostile_tools / "executed"
            malicious = (
                "import pathlib\n"
                f"pathlib.Path({str(marker)!r}).write_text('executed')\n"
                "raise RuntimeError('malicious sibling executed')\n"
            )
            (hostile_tools / "performance-host-noise.py").write_text(
                malicious, encoding="utf-8"
            )
            (hostile_tools / "performance_contract.py").write_text(
                malicious, encoding="utf-8"
            )
            invocation = hostile_tools / "lfs-performance-bench.py"
            invocation.symlink_to(trusted_tools / "lfs-performance-bench.py")
            probe = (
                "import importlib.util,pathlib,sys,types\n"
                "script=pathlib.Path(sys.argv[1])\n"
                "spec=importlib.util.spec_from_file_location('symlink_bench',script)\n"
                "module=importlib.util.module_from_spec(spec)\n"
                "sys.modules[spec.name]=module\n"
                "spec.loader.exec_module(module)\n"
                "module.verify_benchmark_source_binding("
                "types.SimpleNamespace(repo_root=pathlib.Path(sys.argv[2]).resolve()))\n"
                "print(pathlib.Path(module.contract.__file__).resolve())\n"
                "print(pathlib.Path(module.host_noise.__file__).resolve())\n"
            )
            environment = os.environ.copy()
            environment["PYTHONDONTWRITEBYTECODE"] = "1"
            result = subprocess.run(
                [
                    sys.executable,
                    "-B",
                    "-c",
                    probe,
                    str(invocation),
                    str(trusted_root),
                ],
                cwd=hostile_tools,
                env=environment,
                stdin=subprocess.DEVNULL,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
                timeout=10,
            )
            self.assertEqual(result.returncode, 0, result.stderr.decode("utf-8"))
            self.assertFalse(marker.exists())
            output = result.stdout.decode("utf-8", "strict").splitlines()
            self.assertEqual(
                output,
                [
                    str((trusted_tools / "performance_contract.py").resolve()),
                    str((trusted_tools / "performance-host-noise.py").resolve()),
                ],
            )

    def test_non_utf8_executable_version_is_sanitized_exit_two(self) -> None:
        if os.name == "nt":
            self.skipTest("POSIX executable fixture")
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            executable = root / "non-utf8-version"
            executable.write_text(
                f"#!{sys.executable}\n"
                "import sys\n"
                "sys.stdout.buffer.write(bytes([255]))\n",
                encoding="utf-8",
            )
            executable.chmod(0o700)
            expected, _size = bench.hash_file(executable)
            result = subprocess.run(
                [
                    sys.executable,
                    "-B",
                    str(pathlib.Path(bench.__file__).resolve()),
                    "--repo-root",
                    str(root),
                    "--zmin-bin",
                    str(executable),
                    "--zmin-sha256",
                    expected,
                    "--git-bin",
                    str(executable),
                    "--git-sha256",
                    expected,
                    "--git-lfs-manifest",
                    str(root / "missing"),
                    "--output-dir",
                    str(root / "output"),
                    "--failure-dir",
                    str(root / "failed-output"),
                    "--object-count",
                    "1",
                    "--object-bytes",
                    "1",
                    "--concurrency",
                    "1",
                    "--warmups",
                    "0",
                    "--repeats",
                    "1",
                    "--smoke",
                ],
                stdin=subprocess.DEVNULL,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
                timeout=10,
            )
            diagnostic = result.stderr.decode("utf-8", "strict")
            self.assertEqual(result.returncode, 2)
            self.assertIn("not valid UTF-8", diagnostic)
            self.assertNotIn(str(root), diagnostic)
            self.assertNotIn("Traceback", diagnostic)

    def test_release_provenance_binds_standalone_metadata_and_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            repo, zmin, git, sidecar = self._provenance_fixture(root)
            with mock.patch.object(
                bench.contract,
                "sidecar_matches",
                return_value=(True, "matched"),
            ):
                provenance = bench.load_release_provenance(repo, zmin, git)
            metadata = self._metadata(provenance)
            bench.validate_lfs_performance_metadata(metadata)
            historical = dict(metadata)
            historical["schema_name"] = "zmin-lfs-performance-v3"
            historical["schema_version"] = "3"
            historical["schedule_binding_sha256"] = (
                bench.schedule_binding_from_metadata(historical)
            )
            with self.assertRaisesRegex(bench.BenchmarkError, "fixed metadata"):
                bench.validate_lfs_performance_metadata(historical)
            self.assertEqual(metadata["source_commit"], provenance.source_commit)
            self.assertEqual(metadata["source_tree"], provenance.source_tree)
            self.assertEqual(
                metadata["identity_sidecar_sha256"],
                bench.hash_file(sidecar)[0],
            )
            self.assertEqual(
                metadata["identity_sidecar_payload_sha256"],
                "a" * 64,
            )
            changed_provenance = replace(provenance, source_tree="0" * 40)
            with mock.patch.object(
                bench,
                "load_release_provenance",
                return_value=changed_provenance,
            ):
                with self.assertRaisesRegex(bench.BenchmarkError, "changed"):
                    bench.verify_release_provenance_unchanged(provenance, zmin, git)

            tampered = dict(metadata)
            tampered["source_tree"] = "0" * 40
            with self.assertRaisesRegex(bench.BenchmarkError, "digest|schedule binding"):
                bench.validate_lfs_performance_metadata(tampered)
            missing_field = dict(metadata)
            del missing_field["source_commit"]
            with self.assertRaisesRegex(bench.BenchmarkError, "schema"):
                bench.validate_lfs_performance_metadata(missing_field)

            original_sidecar = sidecar.read_bytes()
            wrong_python = json.loads(original_sidecar)
            wrong_python["python"]["sha256"] = "0" * 64
            sidecar.write_text(
                json.dumps(wrong_python, sort_keys=True) + "\n",
                encoding="ascii",
            )
            with self.assertRaisesRegex(bench.BenchmarkError, "Python identity changed"):
                bench.load_release_provenance(repo, zmin, git)
            sidecar.write_bytes(original_sidecar)

            with mock.patch.object(
                bench.contract,
                "sidecar_matches",
                return_value=(False, "tampered Cargo.lock"),
            ):
                with self.assertRaisesRegex(bench.BenchmarkError, "does not match"):
                    bench.load_release_provenance(repo, zmin, git)
            sidecar.unlink()
            with self.assertRaisesRegex(bench.BenchmarkError, "could not be read"):
                bench.load_release_provenance(repo, zmin, git)

    def test_release_provenance_rejects_dirty_source_and_output_inside_source(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            repo, zmin, git, _sidecar = self._provenance_fixture(root)
            (repo / "untracked-source").write_text("dirty\n", encoding="ascii")
            with self.assertRaisesRegex(bench.BenchmarkError, "must be clean"):
                bench.load_release_provenance(repo, zmin, git)
            (repo / "untracked-source").unlink()
            with mock.patch.object(
                bench.contract,
                "sidecar_matches",
                return_value=(True, "matched"),
            ):
                provenance = bench.load_release_provenance(repo, zmin, git)
            output = repo / "retained-output"
            output.mkdir()
            with self.assertRaisesRegex(bench.BenchmarkError, "outside"):
                bench.validate_output_location(output.resolve(), provenance.repo_root)

    def test_evidence_set_manifest_readback_and_every_tamper_fail_closed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            repo, zmin, git, _sidecar = self._provenance_fixture(root)
            with mock.patch.object(
                bench.contract,
                "sidecar_matches",
                return_value=(True, "matched"),
            ):
                provenance = bench.load_release_provenance(repo, zmin, git)
            evidence = self._evidence_fixture(provenance)
            output = root / "published"
            verified = bench.publish_evidence_set(output, evidence, lambda: None)
            self.assertEqual(verified.root_sha256, evidence.root_sha256)
            self.assertEqual(
                set(path.name for path in output.iterdir()),
                {*bench.EVIDENCE_SET_FILES, bench.EVIDENCE_MANIFEST_NAME},
            )
            retained = b"".join((output / name).read_bytes() for name in output.iterdir())
            self.assertNotIn(str(root).encode(), retained)
            self.assertNotIn(str(pathlib.Path.home()).encode(), retained)
            self.assertNotIn(pathlib.Path.home().name.encode(), retained)

            for name in bench.EVIDENCE_SET_FILES:
                path = output / name
                original = path.read_bytes()
                path.write_bytes(original + b"x")
                with self.assertRaises(bench.BenchmarkError, msg=name):
                    bench.verify_evidence_set_directory(output, evidence.root_sha256)
                path.write_bytes(original)

            metadata_path = output / "metadata.tsv"
            original_metadata = metadata_path.read_bytes()
            metadata_rows = bench.contract.read_strict_tsv_bytes(
                original_metadata,
                metadata_path,
                expected_fields=("key", "value"),
            )
            for selected in range(len(metadata_rows)):
                tampered = [dict(row) for row in metadata_rows]
                tampered[selected]["value"] += "x"
                metadata_path.write_bytes(bench.tsv_bytes(("key", "value"), tampered))
                with self.assertRaises(bench.BenchmarkError, msg=tampered[selected]["key"]):
                    bench.verify_evidence_set_directory(output, evidence.root_sha256)
            metadata_path.write_bytes(original_metadata)

            manifest_path = output / bench.EVIDENCE_MANIFEST_NAME
            original_manifest = manifest_path.read_bytes()
            manifest = json.loads(original_manifest)
            for key in ("schema", "artifacts", "contract", "root_sha256"):
                changed = dict(manifest)
                changed[key] = "tampered"
                manifest_path.write_bytes(bench.contract.canonical_json(changed) + b"\n")
                with self.assertRaises(bench.BenchmarkError, msg=key):
                    bench.verify_evidence_set_directory(output, evidence.root_sha256)
            manifest_path.write_bytes(original_manifest)
            bench.verify_evidence_set_directory(output, evidence.root_sha256)

            originals = {
                name: (output / name).read_bytes()
                for name in (*bench.EVIDENCE_SET_FILES, bench.EVIDENCE_MANIFEST_NAME)
            }
            adversarial_mutations = (
                ("rows.tsv", "wall_seconds", "9.000000000"),
                ("summary.tsv", "median_seconds", "9.000000000"),
                ("comparison.tsv", "verdict", "pass"),
                ("comparison.tsv", "paired_median_upper_95_ratio", "0.1"),
                ("metadata.tsv", "claim", bench.BenchmarkClaim.STRICT_SUPERIORITY.value),
                ("metadata.tsv", "repeats", "2"),
                ("metadata.tsv", "git_lfs_version", "git-lfs/9.9.9 adversarial"),
            )
            for name, key, value in adversarial_mutations:
                for original_name, original_bytes in originals.items():
                    (output / original_name).write_bytes(original_bytes)
                self._mutate_tsv_value(output / name, key, value)
                forged_root = self._recompute_evidence_manifest(output)
                with self.assertRaises(bench.BenchmarkError, msg=f"{name}:{key}"):
                    bench.verify_evidence_set_directory(output, forged_root)
            for original_name, original_bytes in originals.items():
                (output / original_name).write_bytes(original_bytes)
            bench.verify_evidence_set_directory(output, evidence.root_sha256)

    def test_coherent_policy_and_raw_row_forgeries_fail_closed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            repo, zmin, git, _sidecar = self._provenance_fixture(root)
            with mock.patch.object(
                bench.contract,
                "sidecar_matches",
                return_value=(True, "matched"),
            ):
                provenance = bench.load_release_provenance(repo, zmin, git)

            strict_evidence = self._strict_evidence_fixture(provenance)
            strict_output = root / "strict"
            bench.publish_evidence_set(strict_output, strict_evidence, lambda: None)
            strict_originals = {
                name: (strict_output / name).read_bytes()
                for name in (*bench.EVIDENCE_SET_FILES, bench.EVIDENCE_MANIFEST_NAME)
            }
            policy_mutations = (
                ("fixture_object_count", "7"),
                ("fixture_object_bytes", "1024"),
                ("concurrencies", "2,1,8"),
                ("concurrencies", "1,1,8"),
                ("concurrencies", "1,2"),
                ("concurrencies", "1,2,4,8"),
                ("warmups", "0"),
                ("warmups", "1"),
                ("warmups", "3"),
                ("repeats", "39"),
                ("repeats", "41"),
                ("timeout_seconds", "179.0"),
                ("timeout_seconds", "3600.0"),
                ("timeout_seconds", "nan"),
                ("timeout_seconds", "+inf"),
                ("timeout_seconds", "-inf"),
                ("schedule_binding_sha256", "0" * 64),
            )
            for key, value in policy_mutations:
                self._restore_evidence(strict_output, strict_originals)
                self._mutate_tsv_value(strict_output / "metadata.tsv", key, value)
                forged_root = self._recompute_evidence_manifest(strict_output)
                with self.assertRaises(bench.BenchmarkError, msg=f"policy:{key}:{value}"):
                    bench.verify_evidence_set_directory(strict_output, forged_root)

            smoke_evidence = self._evidence_fixture(provenance)
            smoke_output = root / "smoke"
            bench.publish_evidence_set(smoke_output, smoke_evidence, lambda: None)
            smoke_originals = {
                name: (smoke_output / name).read_bytes()
                for name in (*bench.EVIDENCE_SET_FILES, bench.EVIDENCE_MANIFEST_NAME)
            }
            for claim in (
                bench.BenchmarkClaim.STRICT_SUPERIORITY.value,
                bench.BenchmarkClaim.STRICT_NOT_ESTABLISHED.value,
            ):
                self._restore_evidence(smoke_output, smoke_originals)
                self._mutate_tsv_value(smoke_output / "metadata.tsv", "claim", claim)
                forged_root = self._recompute_evidence_manifest(smoke_output)
                with self.assertRaises(bench.BenchmarkError, msg=claim):
                    bench.verify_evidence_set_directory(smoke_output, forged_root)

            raw_mutations = (
                ("tool", "forged-tool"),
                ("operation", "forged-operation"),
                ("concurrency", "2"),
                ("sample_kind", "forged-kind"),
                ("pair_id", "forged"),
                ("order_index", "77"),
                ("sample_index", "2"),
                ("object_count", "2"),
                ("object_bytes", "2048"),
                ("payload_bytes", "1"),
                ("wall_seconds", "NaN"),
                ("wall_seconds", "Inf"),
                ("wall_seconds", "-1"),
                ("throughput_mib_per_second", "9"),
                ("throughput_mib_per_second", "NaN"),
                ("user_seconds", "NaN"),
                ("system_seconds", "-1"),
                ("peak_rss_bytes", "0"),
                ("peak_rss_bytes", "0200"),
                ("peak_job_commit_bytes", "1"),
                ("memory_metric", "forged-memory"),
                ("memory_semantics", "/private/secret-memory"),
                ("memory_scope", "arbitrary-scope"),
                ("exit_code", "42"),
                ("batch_requests", "0"),
                ("batch_requests", "+1"),
                ("transfer_requests", "0"),
                ("max_active", "0"),
                ("completed_objects", "0"),
                ("completed_bytes", "0"),
                ("completed_bytes", "01024"),
                ("residual_active", "9"),
                ("host_scheduler_lag_max_seconds", "NaN"),
                ("host_system_cpu_busy_ratio", "1.1"),
                ("host_system_cpu_busy_ratio", "0.25"),
                ("host_system_cpu_busy_ratio", "idle"),
                ("host_load_average_1m", "-1"),
                ("host_memory_pressure", "warning"),
                ("host_thermal_state", "critical"),
                ("host_monitor_samples", "0"),
                ("stdout_sha256", "not-a-sha"),
                ("stderr_sha256", "secret-token"),
            )
            for key, value in raw_mutations:
                self._restore_evidence(smoke_output, smoke_originals)
                self._mutate_tsv_value(smoke_output / "rows.tsv", key, value)
                forged_root = self._recompute_evidence_manifest(smoke_output)
                with self.assertRaises(bench.BenchmarkError, msg=f"raw:{key}:{value}"):
                    bench.verify_evidence_set_directory(smoke_output, forged_root)

            for mutation in ("missing", "extra", "reordered"):
                self._restore_evidence(smoke_output, smoke_originals)
                rows_path = smoke_output / "rows.tsv"
                rows = bench.contract.read_strict_tsv_bytes(rows_path.read_bytes(), rows_path)
                if mutation == "missing":
                    changed = rows[1:]
                elif mutation == "extra":
                    changed = [*rows, dict(rows[0])]
                else:
                    changed = [rows[1], rows[0], *rows[2:]]
                rows_path.write_bytes(bench.tsv_bytes(tuple(rows[0]), changed))
                forged_root = self._recompute_evidence_manifest(smoke_output)
                with self.assertRaises(bench.BenchmarkError, msg=mutation):
                    bench.verify_evidence_set_directory(smoke_output, forged_root)

    def test_metadata_privacy_and_canonical_tsv_fail_closed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            repo, zmin, git, _sidecar = self._provenance_fixture(root)
            with mock.patch.object(
                bench.contract,
                "sidecar_matches",
                return_value=(True, "matched"),
            ):
                provenance = bench.load_release_provenance(repo, zmin, git)
            evidence = self._evidence_fixture(provenance)
            output = root / "privacy"
            bench.publish_evidence_set(output, evidence, lambda: None)
            originals = {
                name: (output / name).read_bytes()
                for name in (*bench.EVIDENCE_SET_FILES, bench.EVIDENCE_MANIFEST_NAME)
            }
            username = pathlib.Path.home().name
            privacy_values = (
                (username.lower(), "private user"),
                (username.upper(), "private user"),
                ("secret", "secret-bearing"),
                ("TOKEN", "secret-bearing"),
                ("password", "secret-bearing"),
                ("auth", "secret-bearing"),
                ("cookie", "secret-bearing"),
                ("x" * (bench.MAX_EVIDENCE_CELL_BYTES + 1), "too large"),
                ("header\r\ninjection", "control"),
                ("nul\x00injection", "control"),
                ("c0\x01injection", "control"),
                ("c1\x85injection", "control"),
                ("delete\x7finjection", "control"),
                ("bidi\u202einjection", "control"),
                ("zero-width\u200binjection", "control"),
                ("tab\tinjection", "control"),
                ("line-separator\u2028injection", "control"),
                ("nonbreaking\u00a0space", "control"),
                ("/private/evidence", "private location"),
                (r"C:\Users\private\evidence", "private location"),
                ("https://user:password@example.invalid/evidence", "private location"),
            )
            for value, reason in privacy_values:
                self._restore_evidence(output, originals)
                self._mutate_tsv_value(output / "metadata.tsv", "host", value)
                forged_root = self._recompute_evidence_manifest(output)
                with self.assertRaisesRegex(bench.BenchmarkError, reason):
                    bench.verify_evidence_set_directory(output, forged_root)
                self._restore_evidence(output, originals)
                self._mutate_metadata_key(output / "metadata.tsv", "host", value)
                forged_root = self._recompute_evidence_manifest(output)
                with self.assertRaisesRegex(bench.BenchmarkError, reason):
                    bench.verify_evidence_set_directory(output, forged_root)

            self._restore_evidence(output, originals)
            metadata_path = output / "metadata.tsv"
            metadata_rows = bench.contract.read_strict_tsv_bytes(
                metadata_path.read_bytes(),
                metadata_path,
                expected_fields=("key", "value"),
            )
            metadata_path.write_bytes(
                bench.tsv_bytes(("key", "value"), reversed(metadata_rows))
            )
            forged_root = self._recompute_evidence_manifest(output)
            with self.assertRaisesRegex(bench.BenchmarkError, "not canonical"):
                bench.verify_evidence_set_directory(output, forged_root)

            malformed_artifacts = (
                b"key\tvalue\nhost\tinvalid-utf8-\xff\n",
                b'key\tvalue\n"unterminated\tvalue\n',
                b"key\tvalue\nhost\t" + b"x" * 200_000 + b"\n",
            )
            for malformed in malformed_artifacts:
                self._restore_evidence(output, originals)
                (output / "metadata.tsv").write_bytes(malformed)
                forged_root = self._recompute_evidence_manifest(output)
                with self.assertRaisesRegex(bench.BenchmarkError, "malformed") as failure:
                    bench.verify_evidence_set_directory(output, forged_root)
                self.assertNotIn(str(root), str(failure.exception))

            self._restore_evidence(output, originals)
            rows_path = output / "rows.tsv"
            rows_path.write_bytes(
                rows_path.read_bytes().replace(b"stock-lfs", b"stock\x00-lfs", 1)
            )
            forged_root = self._recompute_evidence_manifest(output)
            with self.assertRaisesRegex(bench.BenchmarkError, "control") as failure:
                bench.verify_evidence_set_directory(output, forged_root)
            self.assertNotIn(str(root), str(failure.exception))

            decimal_mutations = (
                ("rows.tsv", "peak_rss_bytes", "0200"),
                ("rows.tsv", "batch_requests", "+1"),
                ("rows.tsv", "completed_bytes", " 1024"),
                ("summary.tsv", "samples", "01"),
                ("summary.tsv", "maximum_peak_memory_bytes", "0100"),
                ("comparison.tsv", "concurrency", "01"),
                ("metadata.tsv", "repeats", "01"),
                ("metadata.tsv", "timeout_seconds", "180"),
            )
            for name, key, value in decimal_mutations:
                self._restore_evidence(output, originals)
                self._mutate_tsv_value(output / name, key, value)
                forged_root = self._recompute_evidence_manifest(output)
                with self.assertRaises(bench.BenchmarkError, msg=f"{name}:{key}"):
                    bench.verify_evidence_set_directory(output, forged_root)

    def test_atomic_publication_rolls_back_and_never_replaces_foreign_target(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            repo, zmin, git, _sidecar = self._provenance_fixture(root)
            with mock.patch.object(
                bench.contract,
                "sidecar_matches",
                return_value=(True, "matched"),
            ):
                provenance = bench.load_release_provenance(repo, zmin, git)
            evidence = self._evidence_fixture(provenance)
            rolled_back = root / "rolled-back"

            def fail_after_publish() -> None:
                raise bench.BenchmarkError("injected post-publication failure")

            with self.assertRaisesRegex(bench.BenchmarkError, "injected"):
                bench.publish_evidence_set(
                    rolled_back,
                    evidence,
                    lambda: None,
                    bench.EvidencePublicationHooks(after_publish=fail_after_publish),
                )
            self.assertFalse(os.path.lexists(rolled_back))
            self.assertEqual(list(root.glob(".rolled-back.*.tmp")), [])

            foreign = root / "foreign"

            def create_foreign_target() -> None:
                foreign.mkdir()
                (foreign / "owner.txt").write_text("foreign\n", encoding="ascii")

            with self.assertRaisesRegex(bench.BenchmarkError, "already exists"):
                bench.publish_evidence_set(
                    foreign,
                    evidence,
                    lambda: None,
                    bench.EvidencePublicationHooks(before_publish=create_foreign_target),
                )
            self.assertEqual((foreign / "owner.txt").read_text(encoding="ascii"), "foreign\n")
            self.assertEqual(list(root.glob(".foreign.*.tmp")), [])

            if os.name != "nt":
                pinned_parent = root / "pinned-parent"
                displaced_parent = root / "displaced-parent"
                pinned_parent.mkdir()

                def swap_parent() -> None:
                    pinned_parent.rename(displaced_parent)
                    pinned_parent.mkdir()

                with self.assertRaisesRegex(bench.BenchmarkError, "parent changed"):
                    bench.publish_evidence_set(
                        pinned_parent / "evidence",
                        evidence,
                        lambda: None,
                        bench.EvidencePublicationHooks(before_publish=swap_parent),
                    )
                self.assertFalse((pinned_parent / "evidence").exists())
                self.assertEqual(list(displaced_parent.glob(".evidence.*.tmp")), [])
                pinned_parent.rmdir()
                displaced_parent.rename(pinned_parent)

    def test_windows_directory_publication_uses_no_replace_write_through(self) -> None:
        move = mock.Mock(return_value=1)
        kernel32 = mock.Mock()
        kernel32.MoveFileExW = move
        with mock.patch.object(
            bench.ctypes,
            "WinDLL",
            return_value=kernel32,
            create=True,
        ):
            bench.rename_directory_noreplace_windows(
                pathlib.Path("C:/evidence"),
                "staging",
                "final",
            )
        source, destination, flags = move.call_args.args
        self.assertEqual(source, "C:/evidence/staging")
        self.assertEqual(destination, "C:/evidence/final")
        self.assertEqual(flags, 0x00000008)
        move.return_value = 0
        with mock.patch.object(
            bench.ctypes,
            "WinDLL",
            return_value=kernel32,
            create=True,
        ), mock.patch.object(
            bench.ctypes,
            "get_last_error",
            return_value=183,
            create=True,
        ):
            with self.assertRaisesRegex(bench.BenchmarkError, "already exists"):
                bench.rename_directory_noreplace_windows(
                    pathlib.Path("C:/evidence"),
                    "staging",
                    "final",
                )

    def test_final_publication_revalidates_source_sidecar_binary_and_python(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            repo, zmin, git, sidecar = self._provenance_fixture(root)
            with mock.patch.object(
                bench.contract,
                "sidecar_matches",
                return_value=(True, "matched"),
            ):
                provenance = bench.load_release_provenance(repo, zmin, git)
            evidence = self._evidence_fixture(provenance)

            def final_check() -> None:
                bench.verify_release_inputs_unchanged(provenance, zmin, git, git)

            mutations = (
                (
                    "source",
                    lambda: (repo / "late-source").write_text("dirty\n", encoding="ascii"),
                    lambda: (repo / "late-source").unlink(),
                ),
                (
                    "sidecar",
                    lambda: sidecar.write_bytes(sidecar.read_bytes() + b" "),
                    None,
                ),
                (
                    "binary",
                    lambda: zmin.path.write_bytes(zmin.path.read_bytes() + b"x"),
                    None,
                ),
            )
            for label, mutate, restore in mutations:
                original_sidecar = sidecar.read_bytes()
                original_binary = zmin.path.read_bytes()
                output = root / f"mutated-{label}"
                with mock.patch.object(
                    bench.contract,
                    "sidecar_matches",
                    return_value=(True, "matched"),
                ):
                    with self.assertRaises(bench.BenchmarkError, msg=label):
                        bench.publish_evidence_set(
                            output,
                            evidence,
                            final_check,
                            bench.EvidencePublicationHooks(before_final_check=mutate),
                        )
                self.assertFalse(os.path.lexists(output))
                sidecar.write_bytes(original_sidecar)
                zmin.path.write_bytes(original_binary)
                if restore is not None:
                    restore()

            current_python = bench.authenticated_python_identity(json.loads(sidecar.read_bytes()))
            changed_python = replace(current_python, sha256="0" * 64)
            with mock.patch.object(
                bench,
                "authenticated_python_identity",
                return_value=changed_python,
            ), mock.patch.object(
                bench.contract,
                "sidecar_matches",
                return_value=(True, "matched"),
            ):
                python_output = root / "mutated-python"
                with self.assertRaisesRegex(bench.BenchmarkError, "changed"):
                    bench.publish_evidence_set(
                        python_output,
                        evidence,
                        final_check,
                    )
                self.assertFalse(os.path.lexists(python_output))

            for role in ("stock-git", "git-lfs"):
                tool_path = root / role
                tool_path.write_bytes(b"trusted-tool")
                tool_path.chmod(0o700)
                tool_sha256, _size = bench.hash_file(tool_path)
                identity = bench.ExecutableIdentity(tool_path, tool_sha256, role)
                tool_output = root / f"mutated-{role}"

                def mutate_tool(path=tool_path) -> None:
                    path.write_bytes(path.read_bytes() + b"x")

                def check_tool(selected=identity) -> None:
                    bench.verify_identity_unchanged(selected)

                with self.assertRaisesRegex(bench.BenchmarkError, "changed"):
                    bench.publish_evidence_set(
                        tool_output,
                        evidence,
                        check_tool,
                        bench.EvidencePublicationHooks(before_final_check=mutate_tool),
                    )
                self.assertFalse(os.path.lexists(tool_output))

    def test_git_lfs_manifest_requires_the_exact_reported_version(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            binary = root / "git-lfs-pinned"
            binary.write_bytes(b"fixture")
            binary.chmod(0o700)
            manifest = root / "git-lfs.manifest.txt"
            with manifest.open("w", encoding="utf-8", newline="\n") as output:
                output.write(
                    "artifact=git-lfs-pinned\n"
                    "release_version=3.7.1\n"
                    f"platform={bench.host_manifest_platform()}\n"
                    f"binary_sha256={'a' * 64}\n"
                    "binary_version=git-lfs/3.7.1 exact fixture\n"
                )
            mismatched = bench.ExecutableIdentity(
                binary,
                "a" * 64,
                "git-lfs/3.7.1 substituted fixture",
            )
            with mock.patch.object(bench, "executable_identity", return_value=mismatched):
                with self.assertRaisesRegex(
                    bench.BenchmarkError,
                    "does not match its pinned manifest",
                ):
                    bench.parse_pinned_git_lfs(manifest)

    def test_summary_requires_strict_wall_and_memory_superiority(self) -> None:
        rows = []
        for sample in range(1, 6):
            rows.append(self._row(bench.BenchmarkTool.STOCK, sample, 2.0 + sample / 100, 200))
            rows.append(self._row(bench.BenchmarkTool.ZMIN, sample, 1.0 + sample / 100, 100))
        _summary, comparison = bench.summarize(rows, 5, evaluate_strict=True)
        self.assertEqual([item.verdict for item in comparison], ["pass"])
        slower = [
            replace(row, wall_seconds=3.0, throughput_mib_per_second=1.0)
            if row.tool == bench.BenchmarkTool.ZMIN.value
            else row
            for row in rows
        ]
        _summary, comparison = bench.summarize(slower, 5, evaluate_strict=True)
        self.assertEqual([item.verdict for item in comparison], ["fail"])

    def test_raw_tsv_canonicalization_precedes_every_derived_statistic(self) -> None:
        rows = []
        for tool, memory in (
            (bench.BenchmarkTool.STOCK, 200),
            (bench.BenchmarkTool.ZMIN, 100),
        ):
            for sample, seconds in enumerate(
                (0.50000000001, 0.50000000050),
                start=1,
            ):
                rows.append(
                    replace(
                        self._row(tool, sample, seconds, memory),
                        throughput_mib_per_second=(1024 / (1024 * 1024)) / seconds,
                    )
                )
        before, _comparisons = bench.summarize(
            rows,
            2,
            evaluate_strict=False,
        )
        canonical = bench.canonicalize_benchmark_rows(rows)
        after, _comparisons = bench.summarize(
            canonical,
            2,
            evaluate_strict=False,
        )
        self.assertEqual(bench.row_dict(canonical[0])["wall_seconds"], "0.500000000")
        self.assertEqual(bench.row_dict(canonical[1])["wall_seconds"], "0.500000001")
        self.assertNotEqual(
            [bench.summary_dict(row) for row in before],
            [bench.summary_dict(row) for row in after],
        )

    def test_failed_validation_preserves_negative_evidence_and_success_is_atomic(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            repo, zmin, git, _sidecar = self._provenance_fixture(root)
            with mock.patch.object(
                bench.contract,
                "sidecar_matches",
                return_value=(True, "matched"),
            ):
                provenance = bench.load_release_provenance(repo, zmin, git)
            valid = self._evidence_fixture(provenance)
            manifest = json.loads(valid.manifest)
            metadata = manifest["contract"]
            rows_artifact = next(
                artifact for artifact in valid.artifacts if artifact.name == "rows.tsv"
            )
            parsed_rows = bench.read_evidence_tsv_bytes(
                rows_artifact.data,
                pathlib.Path("rows.tsv"),
                expected_fields=tuple(bench.BenchmarkRow.__dataclass_fields__),
            )
            rows = bench.parse_benchmark_rows(parsed_rows)
            summaries, comparisons = bench.summarize(
                rows,
                1,
                evaluate_strict=False,
            )
            invocation = bench.BenchmarkInvocation("d" * 64)

            mismatched = list(summaries)
            mismatched[0] = replace(
                mismatched[0],
                median_seconds=mismatched[0].median_seconds + 0.000000001,
            )
            positive = root / "summary-mismatch-positive"
            failed = root / "summary-mismatch-failed"
            with self.assertRaisesRegex(bench.BenchmarkError, "summary does not match"):
                bench.publish_evidence_with_failure_custody(
                    positive,
                    failed,
                    rows,
                    mismatched,
                    comparisons,
                    metadata,
                    invocation,
                    lambda: None,
                )
            self.assertFalse(os.path.lexists(positive))
            self.assertEqual(
                {path.name for path in failed.iterdir()},
                set(bench.FAILED_EVIDENCE_FILES),
            )
            failure_manifest = json.loads(
                (failed / bench.FAILED_EVIDENCE_MANIFEST_NAME).read_bytes()
            )
            self.assertIs(failure_manifest["publishable"], False)
            self.assertEqual(failure_manifest["claim"], "not-established")
            bench.verify_failed_evidence_directory(
                failed,
                failure_manifest["root_sha256"],
            )
            self.assertEqual(
                (failed / bench.FAILED_ARTIFACT_NAMES["rows.tsv"]).read_bytes(),
                rows_artifact.data,
            )

            forged_positive = root / "forged-positive"
            forged_failed = root / "forged-failed"
            with mock.patch.object(
                bench,
                "verify_evidence_set_directory",
                side_effect=bench.BenchmarkError("forged evidence root"),
            ):
                with self.assertRaisesRegex(bench.BenchmarkError, "forged evidence root"):
                    bench.publish_evidence_with_failure_custody(
                        forged_positive,
                        forged_failed,
                        rows,
                        summaries,
                        comparisons,
                        metadata,
                        invocation,
                        lambda: None,
                    )
            self.assertFalse(os.path.lexists(forged_positive))
            forged_manifest = json.loads(
                (forged_failed / bench.FAILED_EVIDENCE_MANIFEST_NAME).read_bytes()
            )
            bench.verify_failed_evidence_directory(
                forged_failed,
                forged_manifest["root_sha256"],
            )
            self.assertEqual(
                (forged_failed / bench.FAILED_ARTIFACT_NAMES["rows.tsv"]).read_bytes(),
                rows_artifact.data,
            )

            successful = root / "successful-positive"
            unused_failure = root / "unused-failure"
            verified = bench.publish_evidence_with_failure_custody(
                successful,
                unused_failure,
                rows,
                summaries,
                comparisons,
                metadata,
                invocation,
                lambda: None,
            )
            self.assertEqual(verified.root_sha256, valid.root_sha256)
            self.assertFalse(os.path.lexists(unused_failure))

    @unittest.skipIf(os.name == "nt", "POSIX no-follow race contract")
    def test_failed_custody_rejects_child_directory_and_late_swaps(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            repo, zmin, git, _sidecar = self._provenance_fixture(root)
            with mock.patch.object(
                bench.contract,
                "sidecar_matches",
                return_value=(True, "matched"),
            ):
                provenance = bench.load_release_provenance(repo, zmin, git)
            candidate = self._evidence_fixture(provenance)
            metadata = json.loads(candidate.manifest)["contract"]
            rows_artifact = next(
                artifact for artifact in candidate.artifacts if artifact.name == "rows.tsv"
            )
            rows = bench.parse_benchmark_rows(
                bench.read_evidence_tsv_bytes(
                    rows_artifact.data,
                    pathlib.Path("rows.tsv"),
                    expected_fields=tuple(bench.BenchmarkRow.__dataclass_fields__),
                )
            )
            bundle = bench.build_failed_evidence_bundle(
                candidate,
                metadata,
                rows,
                bench.BenchmarkInvocation("d" * 64),
                bench.BenchmarkError("fixture failure"),
            )

            sentinel = root / "sentinel"
            sentinel.write_bytes(b"do not chmod\n")
            sentinel.chmod(0o600)

            def replace_child_with_symlink(staging: pathlib.Path) -> None:
                child = staging / bench.FAILED_ARTIFACT_NAMES["rows.tsv"]
                child.unlink()
                child.symlink_to(sentinel)

            symlink_output = root / "symlink-race"
            with self.assertRaises(Exception):
                bench.publish_failed_evidence_bundle(
                    symlink_output,
                    bundle,
                    bench.FailedEvidencePublicationHooks(
                        before_freeze=replace_child_with_symlink
                    ),
                )
            self.assertFalse(os.path.lexists(symlink_output))
            self.assertEqual(stat.S_IMODE(sentinel.stat().st_mode), 0o600)

            displaced = root / "displaced-owned-staging"

            def replace_staging_directory(staging: pathlib.Path) -> None:
                staging.rename(displaced)
                staging.mkdir()

            directory_output = root / "directory-race"
            with self.assertRaises(Exception):
                bench.publish_failed_evidence_bundle(
                    directory_output,
                    bundle,
                    bench.FailedEvidencePublicationHooks(
                        before_publish=replace_staging_directory
                    ),
                )
            self.assertFalse(os.path.lexists(directory_output))

            late_output = root / "late-byte-race"

            def mutate_after_readback(published: pathlib.Path) -> None:
                child = published / bench.FAILED_ARTIFACT_NAMES["rows.tsv"]
                child.chmod(0o600)
                child.write_bytes(child.read_bytes() + b"x")

            with self.assertRaises(bench.BenchmarkError):
                bench.publish_failed_evidence_bundle(
                    late_output,
                    bundle,
                    bench.FailedEvidencePublicationHooks(
                        after_readback=mutate_after_readback
                    ),
                )
            self.assertFalse(os.path.lexists(late_output))

            for path in root.glob(".*-race.*.tmp"):
                if path.is_symlink():
                    path.unlink()
                    continue
                path.chmod(0o700)
                for child in path.iterdir():
                    if child.is_symlink():
                        child.unlink()
                    elif child.is_file():
                        child.chmod(0o600)
                        child.unlink()
                path.rmdir()
            if displaced.exists():
                displaced.chmod(0o700)
                for child in displaced.iterdir():
                    child.chmod(0o600)
                    child.unlink()
                displaced.rmdir()

    def test_v4_paired_gate_retains_spikes_ties_and_order_effects(self) -> None:
        def lane_rows(stock_seconds, zmin_seconds):
            rows = []
            for sample, (stock_wall, zmin_wall) in enumerate(
                zip(stock_seconds, zmin_seconds, strict=True), start=1
            ):
                rows.append(
                    self._row(
                        bench.BenchmarkTool.STOCK, sample, stock_wall, 200
                    )
                )
                rows.append(
                    self._row(bench.BenchmarkTool.ZMIN, sample, zmin_wall, 100)
                )
            return rows

        passing = lane_rows([2.0] * 40, [1.0] * 40)
        _summaries, comparisons = bench.summarize(
            passing, 40, evaluate_strict=True
        )
        self.assertEqual(comparisons[0].verdict, "pass")
        self.assertEqual(comparisons[0].paired_median_upper_95_ratio, "0.5")

        tied = lane_rows([2.0] * 40, [1.0] * 25 + [2.0] * 15)
        _summaries, comparisons = bench.summarize(tied, 40, evaluate_strict=True)
        self.assertEqual(comparisons[0].paired_median_upper_95_ratio, "1.0")
        self.assertEqual(comparisons[0].verdict, "fail")

        spiked = lane_rows([2.0] * 40, [1.0] * 37 + [6.0] * 3)
        _summaries, comparisons = bench.summarize(
            spiked, 40, evaluate_strict=True
        )
        self.assertEqual(len(spiked), 80)
        self.assertEqual(comparisons[0].verdict, "fail")
        self.assertEqual(comparisons[0].p95_wall_ratio, 3.0)

        stock_seconds = []
        zmin_seconds = []
        zmin_first_losses = 0
        for sample in range(1, 41):
            zmin_first = bench.sample_order(0, sample - 1)[0] == bench.BenchmarkTool.ZMIN
            if zmin_first and zmin_first_losses < 11:
                stock_seconds.append(1.0)
                zmin_seconds.append(1.1)
                zmin_first_losses += 1
            else:
                stock_seconds.append(10.0)
                zmin_seconds.append(5.0)
        order_biased = lane_rows(stock_seconds, zmin_seconds)
        _summaries, comparisons = bench.summarize(
            order_biased, 40, evaluate_strict=True
        )
        self.assertLess(float(comparisons[0].paired_median_upper_95_ratio), 1.0)
        self.assertGreater(comparisons[0].zmin_first_paired_median_ratio, 1.0)
        self.assertEqual(comparisons[0].verdict, "fail")
        higher_peak_memory = [
            replace(row, peak_rss_bytes="300")
            if row.tool == bench.BenchmarkTool.ZMIN.value
            else row
            for row in passing
        ]
        _summary, comparison = bench.summarize(
            higher_peak_memory, 40, evaluate_strict=True
        )
        self.assertEqual([item.verdict for item in comparison], ["fail"])

    def test_failed_strict_result_never_claims_superiority(self) -> None:
        policy = bench.BenchmarkPolicy(
            object_count=bench.GATE_OBJECT_COUNT,
            object_bytes=bench.GATE_OBJECT_BYTES,
            concurrencies=bench.GATE_CONCURRENCIES,
            warmups=bench.DEFAULT_WARMUPS,
            repeats=bench.DEFAULT_REPEATS,
            child_timeout_seconds=10,
            strict_gate=True,
        )
        comparison = bench.ComparisonRow(
            operation=bench.BenchmarkOperation.DOWNLOAD.value,
            concurrency=1,
            median_wall_ratio=1.1,
            p95_wall_ratio=1.1,
            peak_memory_ratio=1.1,
            paired_median_wall_ratio=1.1,
            paired_median_upper_95_ratio="1.100000",
            stock_first_paired_median_ratio=1.1,
            zmin_first_paired_median_ratio=1.1,
            verdict="fail",
        )
        self.assertEqual(
            bench.benchmark_claim(policy, [comparison]),
            bench.BenchmarkClaim.STRICT_NOT_ESTABLISHED,
        )
        self.assertNotIn(
            "strict-within-host-superiority\t",
            f"{bench.benchmark_claim(policy, [comparison]).value}\t",
        )

    def test_tsv_has_canonical_lf_schema_and_quoting(self) -> None:
        encoded = bench.tsv_bytes(
            ("key", "value"),
            ({"key": "a", "value": "tab\tvalue"}, {"key": "b", "value": "plain"}),
        )
        self.assertTrue(encoded.endswith(b"\n"))
        self.assertNotIn(b"\r", encoded)
        self.assertEqual(encoded.count(b"\n"), 3)

    def _mutate_tsv_value(self, path, key, value):
        rows = bench.contract.read_strict_tsv_bytes(path.read_bytes(), path)
        if path.name == "metadata.tsv":
            selected = next(row for row in rows if row["key"] == key)
            selected["value"] = value
            columns = ("key", "value")
        else:
            rows[0][key] = value
            columns = tuple(rows[0])
        path.write_bytes(bench.tsv_bytes(columns, rows))

    def _mutate_metadata_key(self, path, old_key, new_key):
        rows = bench.contract.read_strict_tsv_bytes(
            path.read_bytes(),
            path,
            expected_fields=("key", "value"),
        )
        selected = next(row for row in rows if row["key"] == old_key)
        selected["key"] = new_key
        path.write_bytes(bench.tsv_bytes(("key", "value"), rows))

    def _recompute_evidence_manifest(self, root):
        manifest_path = root / bench.EVIDENCE_MANIFEST_NAME
        manifest = json.loads(manifest_path.read_bytes())
        descriptors = []
        for descriptor in manifest["artifacts"]:
            changed = dict(descriptor)
            data = (root / changed["name"]).read_bytes()
            try:
                rows = bench.contract.read_strict_tsv_bytes(
                    data,
                    root / changed["name"],
                    expected_fields=tuple(changed["columns"]),
                )
                changed["records"] = len(rows)
            except (bench.csv.Error, UnicodeError, bench.contract.ContractError):
                pass
            changed["bytes"] = len(data)
            changed["sha256"] = bench.contract.sha256_bytes(data)
            descriptors.append(changed)
        try:
            metadata_rows = bench.contract.read_strict_tsv_bytes(
                (root / "metadata.tsv").read_bytes(),
                root / "metadata.tsv",
                expected_fields=("key", "value"),
            )
            metadata = {row["key"]: row["value"] for row in metadata_rows}
        except (bench.csv.Error, UnicodeError, bench.contract.ContractError):
            metadata = manifest["contract"]
        payload = {
            "schema": manifest["schema"],
            "artifacts": descriptors,
            "contract": metadata,
        }
        root_sha256 = bench.contract.sha256_bytes(bench.contract.canonical_json(payload))
        payload["root_sha256"] = root_sha256
        manifest_path.write_bytes(bench.contract.canonical_json(payload) + b"\n")
        return root_sha256

    def _restore_evidence(self, root, originals):
        for name, data in originals.items():
            (root / name).write_bytes(data)

    def _provenance_fixture(self, root):
        git_path = pathlib.Path(shutil.which("git") or "").resolve()
        self.assertTrue(git_path.is_file())
        repo = root / "source"
        repo.mkdir()
        subprocess.run(
            [str(git_path), "-C", str(repo), "init", "--quiet", "--initial-branch=main"],
            check=True,
        )
        (repo / ".gitignore").write_text("/target/\n", encoding="ascii")
        (repo / "Cargo.lock").write_text("fixture lock\n", encoding="ascii")
        (repo / "source.txt").write_text("source\n", encoding="ascii")
        subprocess.run(
            [
                str(git_path),
                "-C",
                str(repo),
                "add",
                "--",
                ".gitignore",
                "Cargo.lock",
                "source.txt",
            ],
            check=True,
        )
        subprocess.run(
            [
                str(git_path),
                "-C",
                str(repo),
                "-c",
                "user.name=LFS Benchmark",
                "-c",
                "user.email=lfs-benchmark@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "--quiet",
                "-m",
                "source",
            ],
            check=True,
        )
        binary = bench.contract.canonical_release_binary(repo.resolve())
        binary.parent.mkdir(parents=True)
        binary.write_bytes(b"fixture-zmin")
        binary.chmod(0o700)
        zmin_sha256, _ = bench.hash_file(binary)
        zmin = bench.ExecutableIdentity(
            binary.resolve(),
            zmin_sha256,
            "git version fixture.zmin",
        )
        git_sha256, _ = bench.hash_file(git_path)
        git = bench.ExecutableIdentity(
            git_path,
            git_sha256,
            subprocess.run(
                [str(git_path), "--version"],
                check=True,
                stdout=subprocess.PIPE,
                text=True,
            ).stdout.strip(),
        )
        sidecar = bench.contract.identity_sidecar(binary)
        python_facts = bench.contract.binary_facts(
            bench.contract.absolute_file(
                pathlib.Path(sys.executable).resolve(strict=True),
                executable=True,
                reject_symlinks=True,
            )
        )
        sidecar.write_text(
            json.dumps(
                {
                    "schema_version": bench.contract.SCHEMA_VERSION + 1,
                    "marker": bench.contract.RELEASE_BUILD_MARKER,
                    "payload_sha256": "a" * 64,
                    "python": python_facts,
                    "make": {"path": str(pathlib.Path(sys.executable).resolve())},
                },
                sort_keys=True,
            )
            + "\n",
            encoding="ascii",
        )
        return repo.resolve(), zmin, git, sidecar.resolve()

    def _metadata(self, provenance):
        metadata = {
            "schema_name": bench.LFS_PERFORMANCE_SCHEMA_NAME,
            "schema_version": bench.LFS_PERFORMANCE_SCHEMA_VERSION,
            "claim": bench.BenchmarkClaim.STRICT_SUPERIORITY.value,
            "cross_platform_memory_comparison": "forbidden",
            "fixture_chunk_bytes": str(bench.STREAM_CHUNK_BYTES),
            "fixture_object_bytes": str(bench.GATE_OBJECT_BYTES),
            "fixture_object_count": str(bench.GATE_OBJECT_COUNT),
            "concurrencies": "1,2,8",
            "host": "fixture-host",
            "host_monitor_adapter": "darwin-stdlib-v1",
            "host_monitor_interval_seconds": (
                f"{bench.host_noise.SAMPLE_INTERVAL_SECONDS:.3f}"
            ),
            "host_noise_cpu_policy": "diagnostic-only-no-numeric-threshold",
            "host_noise_pressure_policy": "normal-only",
            "memory_metric": "peak_rss_bytes",
            "ordering": "sha256-block-rounds-adjacent-balanced",
            "repeats": str(bench.DEFAULT_REPEATS),
            "timeout_seconds": bench.canonical_timeout_seconds(
                bench.DEFAULT_CHILD_TIMEOUT_SECONDS
            ),
            "warmups": str(bench.DEFAULT_WARMUPS),
            "git_lfs_role": "pinned-git-lfs",
            "git_lfs_sha256": "c" * 64,
            "git_lfs_version": "git-lfs/3.7.1 fixture",
            "git_role": "source-inspector-git",
            "git_sha256": provenance.git_sha256,
            "git_version": "git version fixture",
            "python_role": "release-builder-python",
            "python_sha256": provenance.python_sha256,
            "python_version": provenance.python_version,
            "zmin_role": "canonical-release-zmin",
            "zmin_sha256": provenance.zmin_sha256,
            "zmin_version": "git version fixture.zmin",
        }
        metadata.update(bench.release_provenance_metadata(provenance))
        metadata["schedule_binding_sha256"] = bench.schedule_binding_from_metadata(
            metadata
        )
        return metadata

    def _evidence_fixture(self, provenance):
        metadata = self._metadata(provenance)
        metadata.update(
            {
                "claim": bench.BenchmarkClaim.FUNCTIONAL_SMOKE.value,
                "fixture_object_bytes": "1024",
                "fixture_object_count": "1",
                "concurrencies": "1",
                "repeats": "1",
                "warmups": "0",
            }
        )
        metadata["schedule_binding_sha256"] = bench.schedule_binding_from_metadata(
            metadata
        )
        policy = bench.EvidencePolicy.from_metadata(metadata)
        rows = []
        lanes = bench.canonical_lanes(policy)
        for lane in bench.scheduled_lanes(
            policy,
            metadata["schedule_binding_sha256"],
            bench.SampleKind.MEASURED,
            1,
        ):
            lane_index = lanes.index(lane)
            samples = {
                bench.BenchmarkTool.STOCK: self._row(
                    bench.BenchmarkTool.STOCK, 1, 2.0, 200
                ),
                bench.BenchmarkTool.ZMIN: self._row(
                    bench.BenchmarkTool.ZMIN, 1, 1.0, 100
                ),
            }
            for order_index, tool in enumerate(
                bench.sample_order(lane_index, 0), start=1
            ):
                rows.append(
                    replace(
                        samples[tool],
                        operation=lane.operation.value,
                        pair_id=f"{lane.operation.value}-c1-measured-001",
                        order_index=order_index,
                    )
                )
        summaries, comparisons = bench.summarize(rows, 1, evaluate_strict=False)
        return bench.build_evidence_set(rows, summaries, comparisons, metadata)

    def _strict_evidence_fixture(self, provenance):
        metadata = self._metadata(provenance)
        policy = bench.EvidencePolicy.from_metadata(metadata)
        rows = []
        lanes = bench.canonical_lanes(policy)
        payload_bytes = bench.GATE_OBJECT_COUNT * bench.GATE_OBJECT_BYTES
        for sample_kind, count in (
            (bench.SampleKind.WARMUP, bench.DEFAULT_WARMUPS),
            (bench.SampleKind.MEASURED, bench.DEFAULT_REPEATS),
        ):
            for sample_index in range(1, count + 1):
                for lane in bench.scheduled_lanes(
                    policy,
                    metadata["schedule_binding_sha256"],
                    sample_kind,
                    sample_index,
                ):
                    lane_index = lanes.index(lane)
                    pair_id = (
                        f"{lane.operation.value}-c{lane.concurrency}-"
                        f"{sample_kind.value}-"
                        f"{sample_index:03d}"
                    )
                    for order_index, tool in enumerate(
                        bench.sample_order(lane_index, sample_index - 1), start=1
                    ):
                        seconds = 2.0 if tool == bench.BenchmarkTool.STOCK else 1.0
                        memory = 200 if tool == bench.BenchmarkTool.STOCK else 100
                        rows.append(
                            replace(
                                self._row(tool, sample_index, seconds, memory),
                                operation=lane.operation.value,
                                concurrency=lane.concurrency,
                                sample_kind=sample_kind.value,
                                pair_id=pair_id,
                                order_index=order_index,
                                object_count=bench.GATE_OBJECT_COUNT,
                                object_bytes=bench.GATE_OBJECT_BYTES,
                                payload_bytes=payload_bytes,
                                throughput_mib_per_second=(
                                    payload_bytes / (1024 * 1024)
                                )
                                / seconds,
                                transfer_requests=bench.GATE_OBJECT_COUNT,
                                max_active=lane.concurrency,
                                completed_objects=bench.GATE_OBJECT_COUNT,
                                completed_bytes=payload_bytes,
                            )
                        )
        summaries, comparisons = bench.summarize(
            rows, bench.DEFAULT_REPEATS, evaluate_strict=True
        )
        return bench.build_evidence_set(
            rows,
            summaries,
            comparisons,
            metadata,
        )

    def _exercise_download(self, fixture, objects) -> None:
        token = "unit-download"
        specification = bench.RunSpecification(
            token,
            bench.BenchmarkOperation.DOWNLOAD,
            2,
            objects,
        )
        state = fixture.server.register(specification)
        try:
            actions = self._batch(fixture, specification)

            def download(item):
                connection = self._connection(fixture)
                try:
                    path = pathlib.PurePosixPath(
                        urllib.parse.urlsplit(actions[item.oid]).path
                    ).as_posix()
                    connection.request("GET", path)
                    response = connection.getresponse()
                    self.assertEqual(response.status, 200)
                    digest = hashlib.sha256()
                    total = 0
                    while True:
                        chunk = response.read(37 * 1024)
                        if not chunk:
                            break
                        digest.update(chunk)
                        total += len(chunk)
                    return digest.hexdigest(), total
                finally:
                    connection.close()

            with concurrent.futures.ThreadPoolExecutor(max_workers=2) as executor:
                results = list(executor.map(download, objects))
            self.assertEqual(results, [(item.oid, item.size) for item in objects])
            evidence = state.wait_for_completion(2)
            bench.validate_run_evidence(evidence, specification)
        finally:
            fixture.server.unregister(token)

    def _exercise_upload(self, fixture, objects) -> None:
        token = "unit-upload"
        specification = bench.RunSpecification(
            token,
            bench.BenchmarkOperation.UPLOAD,
            2,
            objects,
        )
        state = fixture.server.register(specification)
        try:
            actions = self._batch(fixture, specification)

            def upload(item):
                connection = self._connection(fixture)
                try:
                    path = urllib.parse.urlsplit(actions[item.oid]).path
                    with item.path.open("rb", buffering=0) as source:
                        connection.request(
                            "PUT",
                            path,
                            body=source,
                            headers={"Content-Length": str(item.size)},
                        )
                    response = connection.getresponse()
                    response.read()
                    return response.status
                finally:
                    connection.close()

            with concurrent.futures.ThreadPoolExecutor(max_workers=2) as executor:
                statuses = list(executor.map(upload, objects))
            self.assertEqual(statuses, [200, 200])
            evidence = state.wait_for_completion(2)
            bench.validate_run_evidence(evidence, specification)
        finally:
            fixture.server.unregister(token)

    def _batch(self, fixture, specification):
        connection = self._connection(fixture)
        try:
            body = json.dumps(
                {
                    "operation": specification.operation.value,
                    "objects": [
                        {"oid": item.oid, "size": item.size}
                        for item in specification.objects
                    ],
                },
                separators=(",", ":"),
            ).encode("ascii")
            connection.request(
                "POST",
                f"/{specification.token}/objects/batch",
                body=body,
                headers={"Content-Length": str(len(body))},
            )
            response = connection.getresponse()
            payload = json.loads(response.read())
            self.assertEqual(response.status, 200)
            action = specification.operation.value
            return {
                item["oid"]: item["actions"][action]["href"]
                for item in payload["objects"]
            }
        finally:
            connection.close()

    def _connection(self, fixture):
        host, port = fixture.server.server_address
        return http.client.HTTPConnection(host, port, timeout=5)

    def _row(self, tool, sample, seconds, memory):
        return bench.BenchmarkRow(
            tool=tool.value,
            operation=bench.BenchmarkOperation.DOWNLOAD.value,
            concurrency=1,
            sample_kind=bench.SampleKind.MEASURED.value,
            pair_id=f"download-c1-measured-{sample:03d}",
            order_index=(
                bench.sample_order(0, sample - 1).index(tool) + 1
            ),
            sample_index=sample,
            object_count=1,
            object_bytes=1024,
            payload_bytes=1024,
            wall_seconds=seconds,
            throughput_mib_per_second=(1024 / (1024 * 1024)) / seconds,
            user_seconds="0.1",
            system_seconds="0.1",
            peak_rss_bytes=str(memory),
            peak_job_commit_bytes="unsupported",
            memory_metric="peak_rss_bytes",
            memory_semantics="working_set_peak",
            memory_scope="waited_child_processes",
            exit_code=0,
            batch_requests=1,
            transfer_requests=1,
            max_active=1,
            completed_objects=1,
            completed_bytes=1024,
            residual_active=0,
            host_scheduler_lag_max_seconds=0.001,
            host_system_cpu_busy_ratio=(
                "unobserved" if sample == 1 else "0.250000000"
            ),
            host_load_average_1m="1.000000000",
            host_memory_pressure="normal",
            host_thermal_state="normal",
            host_monitor_samples=2,
            stdout_sha256="a" * 64,
            stderr_sha256="b" * 64,
        )


if __name__ == "__main__":
    unittest.main()
