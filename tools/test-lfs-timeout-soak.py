#!/usr/bin/env python3
"""Focused tests for the Git LFS timeout soak evidence harness."""

from __future__ import annotations

import contextlib
import hashlib
import http.client
import importlib.util
import io
import json
import os
import pathlib
import shutil
import subprocess
import sys
import tempfile
import threading
import time
import unittest
import urllib.parse
from dataclasses import replace
from unittest import mock

if os.name == "nt":  # pragma: no cover - exercised by the Windows gate.
    import ctypes
    from ctypes import wintypes


def load_soak_module():
    module_path = pathlib.Path(__file__).with_name("lfs-timeout-soak.py")
    spec = importlib.util.spec_from_file_location("lfs_timeout_soak_test", module_path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {module_path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def load_process_runner_module():
    module_path = pathlib.Path(__file__).with_name("git-bench-process.py")
    spec = importlib.util.spec_from_file_location("git_bench_process_test", module_path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {module_path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


soak = load_soak_module()
process_runner = load_process_runner_module()


def process_exists(pid: int) -> bool:
    if os.name != "nt":
        try:
            os.kill(pid, 0)
        except ProcessLookupError:
            return False
        return True
    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    open_process = kernel32.OpenProcess
    open_process.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
    open_process.restype = wintypes.HANDLE
    close_handle = kernel32.CloseHandle
    close_handle.argtypes = [wintypes.HANDLE]
    close_handle.restype = wintypes.BOOL
    get_exit_code = kernel32.GetExitCodeProcess
    get_exit_code.argtypes = [wintypes.HANDLE, ctypes.POINTER(wintypes.DWORD)]
    get_exit_code.restype = wintypes.BOOL
    handle = open_process(0x1000, False, pid)  # PROCESS_QUERY_LIMITED_INFORMATION
    if not handle:
        return False
    try:
        code = wintypes.DWORD()
        if not get_exit_code(handle, ctypes.byref(code)):
            return False
        return code.value == 259  # STILL_ACTIVE
    finally:
        close_handle(handle)


class LfsTimeoutSoakTests(unittest.TestCase):
    def test_profiles_preserve_release_inequalities_at_unit_scale(self) -> None:
        for mode in (soak.EvidenceMode.UNIT, soak.EvidenceMode.RELEASE):
            profile = soak.timing_profile(mode)
            download_chunks = soak.download_progress_chunks(profile)
            self.assertLess(profile.progress_gap_seconds, profile.activity_seconds)
            self.assertGreater(profile.progress_total_seconds, 4 * profile.activity_seconds)
            self.assertEqual(len(download_chunks), profile.progress_chunks)
            self.assertEqual(b"".join(download_chunks), soak.OBJECT_BYTES)
            self.assertGreater(profile.stall_seconds, profile.activity_seconds)
            self.assertGreater(profile.stall_seconds, profile.stall_upper_seconds)
        release = soak.timing_profile(soak.EvidenceMode.RELEASE)
        self.assertEqual(release.activity_seconds, 30.0)
        self.assertGreater(release.progress_total_seconds, 120.0)
        self.assertGreater(
            release.progress_total_seconds - release.progress_lower_seconds,
            10.0,
        )
        self.assertEqual(release.stall_seconds, 50.0)

    def test_download_progress_has_every_planned_tcp_gap(self) -> None:
        for mode in (soak.EvidenceMode.UNIT, soak.EvidenceMode.RELEASE):
            with self.subTest(mode=mode.value):
                profile = soak.timing_profile(mode)
                chunks = soak.download_progress_chunks(profile)
                timestamps = tuple(
                    index * profile.progress_gap_seconds
                    for index in range(len(chunks))
                )
                gaps = tuple(
                    later - earlier
                    for earlier, later in zip(timestamps, timestamps[1:])
                )
                self.assertEqual(timestamps[-1], profile.progress_total_seconds)
                self.assertGreater(timestamps[-1], profile.progress_lower_seconds)
                self.assertLess(timestamps[-1], profile.progress_upper_seconds)
                self.assertEqual(len(gaps), profile.progress_chunks - 1)
                self.assertTrue(all(0 < gap < profile.activity_seconds for gap in gaps))

                mismatched = replace(profile, progress_chunks=profile.progress_chunks - 1)
                with self.assertRaisesRegex(
                    soak.SoakError,
                    "download progress chunk count",
                ):
                    soak.download_progress_chunks(mismatched)

    def test_upload_header_terminator_is_progress_delayed_non_vacuously(self) -> None:
        for mode in (soak.EvidenceMode.UNIT, soak.EvidenceMode.RELEASE):
            with self.subTest(mode=mode.value):
                profile = soak.timing_profile(mode)
                chunks = soak.upload_progress_response_chunks(profile)
                timestamps = tuple(
                    index * profile.progress_gap_seconds
                    for index in range(len(chunks))
                )
                completed = soak.first_complete_header_seconds(chunks, timestamps)
                self.assertEqual(completed, timestamps[-1])
                self.assertGreater(completed, profile.progress_lower_seconds)
                self.assertLess(completed, profile.progress_upper_seconds)
                self.assertNotIn(b"\r\n\r\n", b"".join(chunks[:-1]))
                self.assertTrue(b"".join(chunks).endswith(b"\r\n\r\n"))
                gaps = tuple(
                    later - earlier
                    for earlier, later in zip(timestamps, timestamps[1:])
                )
                self.assertTrue(gaps)
                self.assertTrue(all(0 < gap < profile.activity_seconds for gap in gaps))

                early = list(chunks)
                early[3] += b"\r\n"
                early_completion = soak.first_complete_header_seconds(early, timestamps)
                self.assertLess(early_completion, profile.progress_lower_seconds)

    def test_scaled_fixture_runs_all_four_scenarios_non_vacuously(self) -> None:
        profile = soak.timing_profile(soak.EvidenceMode.UNIT)
        before = {thread.ident for thread in threading.enumerate()}
        rows = [soak.run_unit_scenario(scenario, profile) for scenario in soak.SCENARIOS]
        self.assertEqual([row.scenario for row in rows], [item.value for item in soak.SCENARIOS])
        for row in rows:
            self.assertEqual(row.verdict, "pass")
            self.assertEqual(row.batch_attempts, 1)
            self.assertEqual(row.active_connections, 0)
            self.assertGreater(row.maximum_active_connections, 0)
            if row.behavior == soak.Behavior.PROGRESS.value:
                self.assertEqual(row.action_attempts, 1)
                self.assertTrue(row.completed)
                self.assertEqual(row.completed_bytes, len(soak.OBJECT_BYTES))
            else:
                self.assertEqual(row.action_attempts, 2)
                self.assertEqual(row.failure_kind, soak.FailureKind.HTTP_CONTROL.value)
                self.assertEqual(
                    row.timeout_evidence,
                    soak.TimeoutEvidence.ACTIVITY_RETRY_BEFORE_CONTROL.value,
                )
                self.assertIsNotNone(row.retry_after_seconds)
                self.assertFalse(row.completed)
        after = {thread.ident for thread in threading.enumerate()}
        self.assertEqual(after, before)

    def test_wrong_upload_bytes_cannot_satisfy_exact_object_evidence(self) -> None:
        profile = soak.timing_profile(soak.EvidenceMode.UNIT)
        scenario = soak.Scenario.UPLOAD_PROGRESS
        state = soak.FixtureState(scenario, profile)
        wrong = b"BADLFS!!"
        self.assertEqual(len(wrong), len(soak.OBJECT_BYTES))
        with soak.FixtureServer(state) as fixture:
            action = soak.batch_action(fixture.endpoint, scenario, profile.activity_seconds)
            parsed = urllib.parse.urlsplit(action)
            connection = http.client.HTTPConnection(
                parsed.hostname,
                parsed.port,
                timeout=profile.activity_seconds,
            )
            try:
                connection.request(
                    "PUT",
                    parsed.path,
                    body=wrong,
                    headers={"Content-Length": str(len(wrong))},
                )
                response = connection.getresponse()
                response.read()
                self.assertEqual(response.status, 422)
            finally:
                connection.close()
        evidence = state.snapshot()
        child = soak.ChildEvidence(
            exit_code=0,
            elapsed_seconds=profile.progress_total_seconds,
            failure_kind=soak.FailureKind.NONE,
            stdout_sha256=hashlib.sha256(b"").hexdigest(),
            stderr_sha256=hashlib.sha256(b"").hexdigest(),
        )
        verdict, errors = soak.validate_scenario(scenario, profile, child, evidence)
        self.assertEqual(verdict, "fail")
        self.assertTrue(any("OID" in error for error in errors))

    def test_missing_attempt_and_early_completion_cannot_pass(self) -> None:
        profile = soak.timing_profile(soak.EvidenceMode.UNIT)
        scenario = soak.Scenario.DOWNLOAD_PROGRESS
        fixture = soak.FixtureEvidence(
            batch_attempts=1,
            action_attempts=0,
            active_connections=0,
            maximum_active_connections=0,
            completed=True,
            completed_bytes=len(soak.OBJECT_BYTES),
            upload_attempt_bytes=(),
            upload_attempt_oids=(),
            action_start_seconds=(),
            control_statuses=(),
            protocol_errors=(),
        )
        child = soak.ChildEvidence(
            exit_code=0,
            elapsed_seconds=profile.progress_lower_seconds / 2,
            failure_kind=soak.FailureKind.NONE,
            stdout_sha256="0" * 64,
            stderr_sha256="0" * 64,
        )
        verdict, errors = soak.validate_scenario(scenario, profile, child, fixture)
        self.assertEqual(verdict, "fail")
        self.assertTrue(any("attempt" in error for error in errors))
        self.assertTrue(any("elapsed" in error for error in errors))

    def test_wrong_failure_kind_cannot_pass_a_stall(self) -> None:
        profile = soak.timing_profile(soak.EvidenceMode.UNIT)
        scenario = soak.Scenario.DOWNLOAD_STALL
        fixture = soak.FixtureEvidence(
            batch_attempts=1,
            action_attempts=2,
            active_connections=0,
            maximum_active_connections=1,
            completed=False,
            completed_bytes=0,
            upload_attempt_bytes=(),
            upload_attempt_oids=(),
            action_start_seconds=(0.0, profile.activity_seconds),
            control_statuses=((1, 408), (2, 400)),
            protocol_errors=(),
        )
        child = soak.ChildEvidence(
            exit_code=2,
            elapsed_seconds=profile.activity_seconds,
            failure_kind=soak.FailureKind.OTHER,
            stdout_sha256="0" * 64,
            stderr_sha256="0" * 64,
        )
        verdict, errors = soak.validate_scenario(scenario, profile, child, fixture)
        self.assertEqual(verdict, "fail")
        self.assertTrue(any("HTTP control" in error for error in errors))

    def test_wrong_retry_timing_cannot_be_labeled_an_activity_timeout(self) -> None:
        profile = soak.timing_profile(soak.EvidenceMode.UNIT)
        fixture = soak.FixtureEvidence(
            batch_attempts=1,
            action_attempts=2,
            active_connections=0,
            maximum_active_connections=2,
            completed=False,
            completed_bytes=0,
            upload_attempt_bytes=(),
            upload_attempt_oids=(),
            action_start_seconds=(0.0, profile.activity_seconds / 3),
            control_statuses=((1, 408), (2, 400)),
            protocol_errors=(),
        )
        child = soak.ChildEvidence(
            exit_code=2,
            elapsed_seconds=profile.activity_seconds,
            failure_kind=soak.FailureKind.HTTP_CONTROL,
            stdout_sha256="0" * 64,
            stderr_sha256="0" * 64,
        )
        result, errors = soak.scenario_result(
            soak.Scenario.DOWNLOAD_STALL,
            profile,
            child,
            fixture,
        )
        self.assertEqual(result.verdict, "fail")
        self.assertEqual(result.timeout_evidence, soak.TimeoutEvidence.NONE.value)
        self.assertTrue(any("activity-timeout evidence window" in error for error in errors))

    def test_output_is_bounded_canonical_and_never_claims_performance(self) -> None:
        profile = soak.timing_profile(soak.EvidenceMode.UNIT)
        rows = [soak.run_unit_scenario(scenario, profile) for scenario in soak.SCENARIOS]
        metadata = {
            "schema_version": soak.SCHEMA_VERSION,
            "claim": "fixture-self-test-no-performance-claim",
            "mode": "unit",
        }
        with tempfile.TemporaryDirectory() as directory:
            output = pathlib.Path(directory)
            soak.write_evidence(output, metadata, rows)
            tsv = (output / "results.tsv").read_bytes()
            encoded_json = (output / "metadata.json").read_bytes()
            self.assertLessEqual(len(tsv), soak.MAX_OUTPUT_BYTES)
            self.assertLessEqual(len(encoded_json), soak.MAX_OUTPUT_BYTES)
            self.assertNotIn(b"strict-within-host-superiority", encoded_json)
            self.assertEqual(tsv.count(b"\n"), len(soak.SCENARIOS) + 1)
            parsed = json.loads(encoded_json)
            self.assertEqual(parsed["metadata"]["claim"], metadata["claim"])

    def test_release_mode_requires_exact_binary_inputs_before_running(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            args = soak.parse_args(
                [
                    "--mode",
                    "release",
                    "--output-dir",
                    directory,
                    "--source-commit",
                    "a" * 40,
                ]
            )
            with self.assertRaisesRegex(soak.SoakError, "exact source, Zmin, and Git"):
                soak.run(args)

    def test_profile_validation_rejects_vacuous_progress_and_stall(self) -> None:
        profile = soak.timing_profile(soak.EvidenceMode.UNIT)
        with self.assertRaises(soak.SoakError):
            replace(profile, progress_gap_seconds=profile.activity_seconds).validate()
        with self.assertRaises(soak.SoakError):
            replace(profile, stall_seconds=profile.activity_seconds).validate()
        with self.assertRaises(soak.SoakError):
            replace(profile, stall_seconds=profile.stall_upper_seconds).validate()

    def test_batch_rejects_missing_or_wrong_transfer_hash_ref_and_media_type(self) -> None:
        profile = soak.timing_profile(soak.EvidenceMode.UNIT)
        exact = {
            "operation": "download",
            "transfers": ["basic"],
            "objects": [{"oid": soak.OBJECT_OID, "size": len(soak.OBJECT_BYTES)}],
            "ref": {"name": soak.EXPECTED_BATCH_REF},
            "hash_algo": "sha256",
        }
        cases = []
        missing_transfer = dict(exact)
        missing_transfer.pop("transfers")
        cases.append((missing_transfer, soak.LFS_MEDIA_TYPE))
        wrong_hash = dict(exact)
        wrong_hash["hash_algo"] = "sha1"
        cases.append((wrong_hash, soak.LFS_MEDIA_TYPE))
        wrong_ref = dict(exact)
        wrong_ref["ref"] = {"name": "refs/heads/wrong"}
        cases.append((wrong_ref, soak.LFS_MEDIA_TYPE))
        cases.append((exact, "application/json"))
        for payload, content_type in cases:
            with self.subTest(payload=payload, content_type=content_type):
                state = soak.FixtureState(soak.Scenario.DOWNLOAD_PROGRESS, profile)
                with soak.FixtureServer(state) as fixture:
                    parsed = urllib.parse.urlsplit(fixture.endpoint)
                    connection = http.client.HTTPConnection(
                        parsed.hostname,
                        parsed.port,
                        timeout=profile.child_timeout_seconds,
                    )
                    body = json.dumps(payload, separators=(",", ":")).encode("ascii")
                    try:
                        connection.request(
                            "POST",
                            f"{parsed.path}/objects/batch",
                            body=body,
                            headers={
                                "Accept": content_type,
                                "Content-Type": content_type,
                                "Content-Length": str(len(body)),
                            },
                        )
                        response = connection.getresponse()
                        response.read()
                        self.assertEqual(response.status, 500)
                    finally:
                        connection.close()
                self.assertEqual(state.snapshot().batch_attempts, 0)

    def test_fake_exact_hashed_release_binary_without_sidecar_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            binary = soak.contract.canonical_release_binary(root)
            binary.parent.mkdir(parents=True)
            shutil.copy2(sys.executable, binary)
            binary.chmod(binary.stat().st_mode | 0o100)
            digest, _ = soak.hash_file(binary)
            zmin = soak.ExecutableIdentity(
                binary.resolve(),
                digest,
                f"{soak.ZMIN_VERSION_PREFIX}0.1.0)",
            )
            git = soak.ExecutableIdentity(
                pathlib.Path(sys.executable).resolve(),
                "0" * 64,
                "git version fixture",
            )
            with self.assertRaisesRegex(soak.SoakError, "identity sidecar"):
                soak.release_source_identity(root, "a" * 40, git, zmin)

    def test_clean_archive_sidecar_is_bound_without_using_dirty_live_tree(self) -> None:
        git_path = pathlib.Path(shutil.which("git") or "").resolve()
        self.assertTrue(git_path.is_file())
        with tempfile.TemporaryDirectory() as directory:
            temporary = pathlib.Path(directory)
            live = temporary / "dirty-live"
            live.mkdir()
            (live / "unrelated-modification").write_text("dirty", encoding="ascii")
            archive = temporary / "archive"
            archive.mkdir()
            environment = os.environ.copy()
            environment.update(
                {
                    "GIT_AUTHOR_NAME": "Timeout Soak",
                    "GIT_AUTHOR_EMAIL": "timeout@example.invalid",
                    "GIT_COMMITTER_NAME": "Timeout Soak",
                    "GIT_COMMITTER_EMAIL": "timeout@example.invalid",
                    "GIT_CONFIG_GLOBAL": str(temporary / "empty.gitconfig"),
                    "GIT_CONFIG_NOSYSTEM": "1",
                    "GIT_OPTIONAL_LOCKS": "0",
                    "GIT_TERMINAL_PROMPT": "0",
                }
            )
            (temporary / "empty.gitconfig").write_bytes(b"")

            def git(*arguments: str) -> str:
                completed = subprocess.run(
                    [str(git_path), "-C", str(archive), *arguments],
                    env=environment,
                    stdin=subprocess.DEVNULL,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                    check=True,
                    text=True,
                )
                return completed.stdout.strip()

            git("init", "--quiet", "--initial-branch=main", "--template=")
            (archive / ".gitignore").write_text("/target/\n", encoding="ascii")
            (archive / "Cargo.lock").write_text("lock\n", encoding="ascii")
            git("add", "--", ".gitignore", "Cargo.lock")
            git("commit", "--quiet", "-m", "archive source")
            source_commit = git("rev-parse", "HEAD")
            expected_tree = git("rev-parse", "HEAD^{tree}")
            binary = soak.contract.canonical_release_binary(archive)
            binary.parent.mkdir(parents=True)
            shutil.copy2(sys.executable, binary)
            binary.chmod(binary.stat().st_mode | 0o100)
            digest, _ = soak.hash_file(binary)
            zmin = soak.ExecutableIdentity(
                binary.resolve(),
                digest,
                f"{soak.ZMIN_VERSION_PREFIX}0.1.0)",
            )
            git_identity = soak.ExecutableIdentity(
                git_path,
                "0" * 64,
                "git version fixture",
            )
            sidecar = soak.contract.identity_sidecar(binary)
            sidecar.write_text(
                json.dumps(
                    {
                        "python": {"path": str(pathlib.Path(sys.executable).resolve())},
                        "make": {"path": str(pathlib.Path(sys.executable).resolve())},
                    }
                ),
                encoding="ascii",
            )
            with mock.patch.object(
                soak.contract,
                "sidecar_matches",
                return_value=(True, "matched"),
            ), mock.patch.object(
                soak.contract,
                "load_json",
                side_effect=AssertionError("release identity must parse its one snapshot"),
            ):
                identity = soak.release_source_identity(archive, source_commit, git_identity, zmin)
            self.assertEqual(identity.source_commit, source_commit)
            self.assertEqual(identity.source_tree, expected_tree)
            self.assertEqual(identity.sidecar_sha256, soak.hash_file(sidecar)[0])
            self.assertTrue((live / "unrelated-modification").is_file())

            (archive / "new-source").write_text("different tree\n", encoding="ascii")
            git("add", "--", "new-source")
            git("commit", "--quiet", "-m", "different tree")
            with self.assertRaisesRegex(soak.SoakError, "clean pinned commit"):
                soak.release_source_identity(
                    archive,
                    source_commit,
                    git_identity,
                    zmin,
                )
            new_commit = git("rev-parse", "HEAD")
            with mock.patch.object(
                soak.contract,
                "sidecar_matches",
                return_value=(False, "forged"),
            ):
                with self.assertRaisesRegex(soak.SoakError, "does not match"):
                    soak.release_source_identity(
                        archive,
                        new_commit,
                        git_identity,
                        zmin,
                    )

    def test_process_helper_caps_noise_and_reaps_lingering_descendant(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            environment = os.environ.copy()
            environment["PYTHONDONTWRITEBYTECODE"] = "1"
            noisy = soak.run_bounded_process(
                root / "noise",
                root,
                environment,
                2.0,
                (
                    sys.executable,
                    "-B",
                    "-c",
                    "import sys;sys.stdout.buffer.write(b'x'*"
                    f"{soak.MAX_CAPTURE_BYTES + 1});sys.stdout.flush()",
                ),
            )
            self.assertEqual(noisy.exit_code, 125)
            self.assertEqual((root / "noise" / "stdout").stat().st_size, soak.MAX_CAPTURE_BYTES)

            linger = soak.run_bounded_process(
                root / "linger",
                root,
                environment,
                2.0,
                (
                    sys.executable,
                    "-B",
                    "-c",
                    "import subprocess,sys;"
                    "p=subprocess.Popen([sys.executable,'-B','-c','import time;time.sleep(60)']);"
                    "print(p.pid,flush=True)",
                ),
            )
            expected_lingering_status = 124 if os.name == "nt" else 126
            self.assertEqual(linger.exit_code, expected_lingering_status)
            pid = int((root / "linger" / "stdout").read_text(encoding="ascii").strip())
            deadline = time.monotonic() + 2.0
            while process_exists(pid):
                if time.monotonic() >= deadline:
                    self.fail("bounded helper left its descendant alive")
                time.sleep(0.01)

    @unittest.skipIf(os.name == "nt", "POSIX process-group contract")
    def test_process_group_eperm_requires_proven_quiescence(self) -> None:
        completed = mock.Mock()
        completed.pid = 4242
        completed.poll.return_value = 0
        completed.wait.return_value = 0

        def absent_group(process_group, signal_number):
            self.assertEqual(process_group, completed.pid)
            if signal_number == process_runner.signal.SIGKILL:
                raise PermissionError("signal raced with child exit")
            if signal_number == 0:
                raise ProcessLookupError
            self.fail(f"unexpected signal: {signal_number}")

        with mock.patch.object(process_runner.os, "killpg", side_effect=absent_group):
            process_runner.terminate_process_group(completed, graceful=False)
        completed.poll.assert_called_once_with()
        completed.wait.assert_called_once_with()

        for poll_result, group_probe in (
            (None, None),
            (0, None),
            (0, PermissionError("group identity is not observable")),
        ):
            with self.subTest(poll_result=poll_result, group_probe=group_probe):
                unsafe = mock.Mock()
                unsafe.pid = 4343
                unsafe.poll.return_value = poll_result

                def unresolved_group(_process_group, signal_number):
                    if signal_number == process_runner.signal.SIGKILL:
                        raise PermissionError("signal was not authorized")
                    if signal_number == 0 and group_probe is not None:
                        raise group_probe
                    return None

                with mock.patch.object(
                    process_runner.os,
                    "killpg",
                    side_effect=unresolved_group,
                ), self.assertRaises(PermissionError):
                    process_runner.terminate_process_group(unsafe, graceful=False)
                unsafe.wait.assert_not_called()

    def test_nested_python_runner_keeps_clean_source_bytecode_free(self) -> None:
        git_path = pathlib.Path(shutil.which("git") or "").resolve()
        self.assertTrue(git_path.is_file())
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            repository = root / "clean-source"
            repository.mkdir()
            probe_module = repository / "nested_probe_module.py"
            probe_script = repository / "nested_version_probe.py"
            probe_module.write_text("VALUE = 'nested'\n", encoding="ascii")
            probe_script.write_text(
                "import os\n"
                "import nested_probe_module\n"
                "print('probe:' + os.environ.get('PYTHONDONTWRITEBYTECODE', '') + ':' "
                "+ nested_probe_module.VALUE)\n",
                encoding="ascii",
            )
            git_environment = os.environ.copy()
            git_environment.update(
                {
                    "GIT_AUTHOR_NAME": "Timeout Soak",
                    "GIT_AUTHOR_EMAIL": "timeout@example.invalid",
                    "GIT_COMMITTER_NAME": "Timeout Soak",
                    "GIT_COMMITTER_EMAIL": "timeout@example.invalid",
                    "GIT_CONFIG_GLOBAL": str(root / "empty.gitconfig"),
                    "GIT_CONFIG_NOSYSTEM": "1",
                }
            )
            (root / "empty.gitconfig").write_bytes(b"")

            def git(*arguments: str) -> bytes:
                completed = subprocess.run(
                    [str(git_path), "-C", str(repository), *arguments],
                    env=git_environment,
                    stdin=subprocess.DEVNULL,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                    check=True,
                )
                return completed.stdout

            git("init", "--quiet", "--initial-branch=main", "--template=")
            git("add", "--", probe_module.name, probe_script.name)
            git("commit", "--quiet", "-m", "clean source")
            self.assertEqual(git("status", "--porcelain=v1", "--untracked-files=all"), b"")

            python_path = pathlib.Path(sys.executable).resolve()
            python_sha256, _ = soak.hash_file(python_path)
            for inherited in (None, "0"):
                with self.subTest(inherited=inherited):
                    environment = os.environ.copy()
                    if inherited is None:
                        environment.pop("PYTHONDONTWRITEBYTECODE", None)
                    else:
                        environment["PYTHONDONTWRITEBYTECODE"] = inherited
                    artifacts = root / f"runner-{inherited or 'unset'}"
                    actual_run = subprocess.run
                    with mock.patch.object(
                        soak.subprocess,
                        "run",
                        wraps=actual_run,
                    ) as runner_call:
                        process = soak.run_bounded_process(
                            artifacts,
                            repository,
                            environment,
                            10.0,
                            (sys.executable, str(probe_script)),
                        )
                    self.assertEqual(process.exit_code, 0)
                    self.assertEqual(
                        (artifacts / "stdout").read_bytes(),
                        b"probe:1:nested\n",
                    )
                    invocation = runner_call.call_args.args[0]
                    self.assertEqual(invocation[0:2], [sys.executable, "-B"])
                    self.assertEqual(
                        pathlib.Path(invocation[2]).name,
                        "git-bench-process.py",
                    )
                    self.assertEqual(
                        runner_call.call_args.kwargs["env"]["PYTHONDONTWRITEBYTECODE"],
                        "1",
                    )
                    self.assertEqual(
                        environment.get("PYTHONDONTWRITEBYTECODE"),
                        inherited,
                    )
                    with mock.patch.dict(os.environ, {}, clear=False):
                        if inherited is None:
                            os.environ.pop("PYTHONDONTWRITEBYTECODE", None)
                        else:
                            os.environ["PYTHONDONTWRITEBYTECODE"] = inherited
                        identity = soak.executable_identity(
                            python_path,
                            python_sha256,
                            ("-B", str(probe_script)),
                            "probe:1:nested",
                        )
                    self.assertEqual(identity.version, "probe:1:nested")
                    cache_paths = [
                        path
                        for path in repository.rglob("*")
                        if path.name == "__pycache__" or path.suffix == ".pyc"
                    ]
                    self.assertEqual(cache_paths, [])
                    self.assertEqual(
                        git("status", "--porcelain=v1", "--untracked-files=all"),
                        b"",
                    )

    def test_server_close_runs_when_handler_quiescence_bound_fails(self) -> None:
        state = soak.FixtureState(
            soak.Scenario.DOWNLOAD_STALL,
            soak.timing_profile(soak.EvidenceMode.UNIT),
        )
        fixture = soak.FixtureServer(state)
        fixture.__enter__()
        with mock.patch.object(
            state,
            "wait_for_actions",
            return_value=False,
        ), mock.patch.object(
            fixture.server,
            "server_close",
            wraps=fixture.server.server_close,
        ) as close:
            with self.assertRaisesRegex(soak.SoakError, "did not terminate"):
                fixture.__exit__(None, None, None)
            close.assert_called_once_with()
        self.assertFalse(fixture.thread.is_alive())

    def test_failed_release_scenario_retains_bounded_negative_diagnostics(self) -> None:
        profile = soak.timing_profile(soak.EvidenceMode.UNIT)
        completed = replace(
            soak.run_unit_scenario(soak.Scenario.DOWNLOAD_PROGRESS, profile),
            mode=soak.EvidenceMode.RELEASE.value,
        )
        runner_metrics = (
            "0.125000000\tunsupported\tunsupported\t1024\tunsupported\t0\t0\t7\t7\t"
            "peak_rss_bytes\tworking_set_peak\twaited_child_processes\tbytes\t"
            "wall_seconds=available\n"
        )
        stdout = b"child output /private/hidden token=SECRET\n"
        stderr = b"Authorization: Bearer SECRET\n/private/hidden\n"
        child = soak.ChildEvidence(
            exit_code=2,
            elapsed_seconds=profile.progress_lower_seconds / 2,
            failure_kind=soak.FailureKind.OTHER,
            stdout_sha256=hashlib.sha256(stdout).hexdigest(),
            stderr_sha256=hashlib.sha256(stderr).hexdigest(),
            runner_metrics_sha256=hashlib.sha256(runner_metrics.encode("ascii")).hexdigest(),
            runner_metrics=runner_metrics,
            stdout_excerpt=stdout.decode("ascii"),
            stderr_excerpt=stderr.decode("ascii"),
        )

        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            output = root / "output"
            output.mkdir()
            archive = root / "archive"
            archive.mkdir()
            zmin = soak.ExecutableIdentity(
                root / "zmin",
                "a" * 64,
                f"{soak.ZMIN_VERSION_PREFIX}fixture)",
            )
            git = soak.ExecutableIdentity(root / "git", "b" * 64, "git version fixture")
            source = soak.ReleaseSourceIdentity(
                archive,
                "c" * 40,
                "d" * 40,
                archive / "target" / "release" / "zmin.identity.json",
                "e" * 64,
            )

            def release_scenario(
                scenario,
                _profile,
                _root,
                _git,
                _zmin,
                diagnostics,
            ):
                if scenario == soak.Scenario.DOWNLOAD_PROGRESS:
                    return completed
                diagnostics.child = child
                diagnostics.residual_processes = 0
                diagnostics.residual_lfs_temp_files = 0
                raise soak.SoakError(
                    "progress scenario elapsed before /private/hidden "
                    "Authorization: Bearer SECRET"
            )

            stderr_capture = io.StringIO()
            with mock.patch.object(
                soak,
                "executable_identity",
                side_effect=(zmin, git),
            ), mock.patch.object(
                soak,
                "release_source_identity",
                return_value=source,
            ), mock.patch.object(
                soak,
                "run_release_scenario",
                side_effect=release_scenario,
            ), contextlib.redirect_stderr(stderr_capture):
                status = soak.main(
                    [
                        "--mode",
                        "release",
                        "--output-dir",
                        str(output),
                        "--source-commit",
                        source.source_commit,
                        "--repo-root",
                        str(archive),
                        "--zmin-bin",
                        str(zmin.path),
                        "--zmin-sha256",
                        zmin.sha256,
                        "--git-bin",
                        str(git.path),
                        "--git-sha256",
                        git.sha256,
                    ]
                )

            self.assertEqual(status, 2)
            self.assertEqual({path.name for path in output.iterdir()}, {"failure.json"})
            encoded = (output / "failure.json").read_bytes()
            self.assertLessEqual(len(encoded), soak.MAX_OUTPUT_BYTES)
            self.assertNotIn(b"SECRET", encoded)
            self.assertNotIn(b"/private/hidden", encoded)
            self.assertNotIn(b"timeout-correctness-only-no-speed-or-memory-claim", encoded)
            failure = json.loads(encoded)
            self.assertEqual(failure["claim"], "timeout-contract-not-established")
            self.assertEqual(failure["verdict"], "failure")
            self.assertEqual(failure["scenario"], soak.Scenario.UPLOAD_PROGRESS.value)
            self.assertEqual(
                [row["scenario"] for row in failure["completed_scenarios"]],
                [soak.Scenario.DOWNLOAD_PROGRESS.value],
            )
            self.assertEqual(failure["identity"]["source_commit"], source.source_commit)
            self.assertEqual(failure["identity"]["zmin_sha256"], zmin.sha256)
            self.assertEqual(
                failure["identity"]["identity_sidecar_sha256"],
                source.sidecar_sha256,
            )
            self.assertEqual(failure["child_runner"]["exit_code"], child.exit_code)
            self.assertEqual(
                failure["child_runner"]["elapsed_seconds"], child.elapsed_seconds
            )
            self.assertEqual(failure["child_runner"]["runner_metrics"], runner_metrics)
            self.assertEqual(
                failure["child_runner"]["runner_metrics_sha256"],
                child.runner_metrics_sha256,
            )
            self.assertEqual(
                failure["cleanup"],
                {
                    "residual_lfs_temp_files": 0,
                    "residual_processes": 0,
                    "temporary_root_removed": True,
                },
            )
            self.assertNotIn("SECRET", stderr_capture.getvalue())

    def test_failure_publish_cleans_partial_and_full_prepublish_failures(self) -> None:
        data = b'{"claim":"timeout-contract-not-established","verdict":"failure"}\n'
        for failure_point in ("partial-write", "before-publish"):
            with self.subTest(failure_point=failure_point), tempfile.TemporaryDirectory() as directory:
                output = pathlib.Path(directory)
                if failure_point == "partial-write":
                    original_write = soak.os.write
                    calls = 0

                    def fail_after_partial(descriptor, remaining):
                        nonlocal calls
                        calls += 1
                        if calls == 1:
                            return original_write(descriptor, bytes(remaining[:7]))
                        raise OSError("injected write failure")

                    injection = mock.patch.object(
                        soak.os,
                        "write",
                        side_effect=fail_after_partial,
                    )
                else:
                    injection = mock.patch.object(
                        soak,
                        "_link_failure_temp",
                        side_effect=soak.FailureArtifactPublicationError(
                            "injected pre-publish failure"
                        ),
                    )
                with injection, self.assertRaises(soak.FailureArtifactPublicationError):
                    soak.publish_failure_artifact(output, data)
                self.assertFalse((output / soak.FAILURE_ARTIFACT_NAME).exists())
                self.assertEqual(list(output.glob(f"{soak.FAILURE_TEMP_PREFIX}*")), [])

    def test_failure_publish_never_overwrites_existing_final(self) -> None:
        original = b'{"claim":"existing","verdict":"failure"}\n'
        replacement = b'{"claim":"timeout-contract-not-established"}\n'
        with tempfile.TemporaryDirectory() as directory:
            output = pathlib.Path(directory)
            final = output / soak.FAILURE_ARTIFACT_NAME
            final.write_bytes(original)
            with self.assertRaisesRegex(
                soak.FailureArtifactPublicationError,
                "already exists",
            ):
                soak.publish_failure_artifact(output, replacement)
            self.assertEqual(final.read_bytes(), original)
            self.assertEqual(list(output.glob(f"{soak.FAILURE_TEMP_PREFIX}*")), [])

    def test_failure_publish_success_is_exact_atomic_json(self) -> None:
        data = b'{"claim":"timeout-contract-not-established","verdict":"failure"}\n'
        with tempfile.TemporaryDirectory() as directory:
            output = pathlib.Path(directory)
            soak.publish_failure_artifact(output, data)
            final = output / soak.FAILURE_ARTIFACT_NAME
            self.assertEqual(final.read_bytes(), data)
            self.assertEqual(json.loads(final.read_bytes())["verdict"], "failure")
            self.assertEqual({path.name for path in output.iterdir()}, {final.name})
            self.assertIn(
                "same-UID mutation after a successful return",
                soak.ATOMIC_PUBLICATION_THREAT_BOUNDARY,
            )

    def test_concurrent_failure_publish_has_exactly_one_winner(self) -> None:
        payloads = (
            b'{"claim":"timeout-contract-not-established","writer":1}\n',
            b'{"claim":"timeout-contract-not-established","writer":2}\n',
        )
        with tempfile.TemporaryDirectory() as directory:
            output = pathlib.Path(directory)
            barrier = threading.Barrier(3)
            outcomes = []
            outcome_lock = threading.Lock()

            def writer(data):
                barrier.wait()
                try:
                    soak.publish_failure_artifact(output, data)
                except soak.FailureArtifactPublicationError as error:
                    outcome = ("lost", type(error).__name__)
                else:
                    outcome = ("won", hashlib.sha256(data).hexdigest())
                with outcome_lock:
                    outcomes.append(outcome)

            threads = [
                threading.Thread(target=writer, args=(payload,)) for payload in payloads
            ]
            for thread in threads:
                thread.start()
            barrier.wait()
            for thread in threads:
                thread.join(timeout=5.0)
                self.assertFalse(thread.is_alive())
            self.assertEqual([kind for kind, _value in outcomes].count("won"), 1)
            self.assertEqual([kind for kind, _value in outcomes].count("lost"), 1)
            final = (output / soak.FAILURE_ARTIFACT_NAME).read_bytes()
            self.assertIn(final, payloads)
            self.assertEqual(list(output.glob(f"{soak.FAILURE_TEMP_PREFIX}*")), [])

    def test_failure_publish_rejects_replaced_temp_symlink(self) -> None:
        if not hasattr(os, "symlink"):
            self.skipTest("platform does not expose symlink creation")
        data = b'{"claim":"timeout-contract-not-established"}\n'
        with tempfile.TemporaryDirectory() as directory:
            output = pathlib.Path(directory)
            victim = output / "victim"
            victim.write_bytes(b"do not alter")
            original_link = soak._link_failure_temp

            def replace_with_symlink(temporary_name, final_name, directory_descriptor):
                os.unlink(temporary_name, dir_fd=directory_descriptor)
                os.symlink(victim.name, temporary_name, dir_fd=directory_descriptor)
                return original_link(temporary_name, final_name, directory_descriptor)

            with mock.patch.object(
                soak,
                "_link_failure_temp",
                side_effect=replace_with_symlink,
            ), self.assertRaisesRegex(
                soak.FailureArtifactPublicationError,
                "opened safely",
            ):
                soak.publish_failure_artifact(output, data)
            self.assertEqual(victim.read_bytes(), b"do not alter")
            self.assertFalse((output / soak.FAILURE_ARTIFACT_NAME).exists())
            self.assertEqual(list(output.glob(f"{soak.FAILURE_TEMP_PREFIX}*")), [])

    def test_failure_publish_rolls_back_directory_path_swap(self) -> None:
        data = b'{"claim":"timeout-contract-not-established"}\n'
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            output = root / "output"
            output.mkdir()
            moved = root / "moved"
            original_revalidate = soak._revalidate_pinned_directory
            calls = 0

            def swap_directory(path, descriptor, identity):
                nonlocal calls
                calls += 1
                if calls == 2:
                    path.rename(moved)
                    path.mkdir()
                return original_revalidate(path, descriptor, identity)

            with mock.patch.object(
                soak,
                "_revalidate_pinned_directory",
                side_effect=swap_directory,
            ), self.assertRaisesRegex(
                soak.FailureArtifactPublicationError,
                "pinned directory",
            ):
                soak.publish_failure_artifact(output, data)
            self.assertFalse((output / soak.FAILURE_ARTIFACT_NAME).exists())
            self.assertFalse((moved / soak.FAILURE_ARTIFACT_NAME).exists())
            self.assertEqual(list(moved.glob(f"{soak.FAILURE_TEMP_PREFIX}*")), [])

    def test_failure_publish_rolls_back_same_inode_content_mutation(self) -> None:
        data = b'{"claim":"timeout-contract-not-established"}\n'
        with tempfile.TemporaryDirectory() as directory:
            output = pathlib.Path(directory)
            original_verify = soak._verify_published_failure
            mutated = False

            def mutate_then_verify(final_name, descriptor, identity, expected, links):
                nonlocal mutated
                if not mutated:
                    mutated = True
                    file_descriptor = os.open(
                        final_name,
                        os.O_WRONLY | os.O_TRUNC,
                        dir_fd=descriptor,
                    )
                    try:
                        os.write(file_descriptor, b"mutated")
                        os.fsync(file_descriptor)
                    finally:
                        os.close(file_descriptor)
                return original_verify(final_name, descriptor, identity, expected, links)

            with mock.patch.object(
                soak,
                "_verify_published_failure",
                side_effect=mutate_then_verify,
            ), self.assertRaisesRegex(
                soak.FailureArtifactPublicationError,
                "identity changed|bytes changed",
            ):
                soak.publish_failure_artifact(output, data)
            self.assertFalse((output / soak.FAILURE_ARTIFACT_NAME).exists())
            self.assertEqual(list(output.glob(f"{soak.FAILURE_TEMP_PREFIX}*")), [])

    def test_failure_publish_rolls_back_wrong_link_target(self) -> None:
        data = b'{"claim":"timeout-contract-not-established"}\n'
        with tempfile.TemporaryDirectory() as directory:
            output = pathlib.Path(directory)
            victim = output / "victim"
            victim.write_bytes(b"do not alter")

            def link_victim(_temporary_name, final_name, descriptor):
                os.link(
                    victim.name,
                    final_name,
                    src_dir_fd=descriptor,
                    dst_dir_fd=descriptor,
                    follow_symlinks=False,
                )

            with mock.patch.object(
                soak,
                "_link_failure_temp",
                side_effect=link_victim,
            ), self.assertRaisesRegex(
                soak.FailureArtifactPublicationError,
                "identity changed",
            ):
                soak.publish_failure_artifact(output, data)
            self.assertEqual(victim.read_bytes(), b"do not alter")
            self.assertFalse((output / soak.FAILURE_ARTIFACT_NAME).exists())
            self.assertEqual(list(output.glob(f"{soak.FAILURE_TEMP_PREFIX}*")), [])

    def test_failure_publish_rolls_back_strict_directory_fsync_failure(self) -> None:
        data = b'{"claim":"timeout-contract-not-established"}\n'
        with tempfile.TemporaryDirectory() as directory:
            output = pathlib.Path(directory)
            with mock.patch.object(
                soak,
                "_fsync_pinned_directory",
                side_effect=soak.FailureArtifactPublicationError("injected directory fsync"),
            ), self.assertRaisesRegex(
                soak.FailureArtifactPublicationError,
                "directory fsync",
            ):
                soak.publish_failure_artifact(output, data)
            self.assertFalse((output / soak.FAILURE_ARTIFACT_NAME).exists())
            self.assertEqual(list(output.glob(f"{soak.FAILURE_TEMP_PREFIX}*")), [])

    def test_failure_publish_rolls_back_post_link_path_swap(self) -> None:
        data = b'{"claim":"timeout-contract-not-established"}\n'
        with tempfile.TemporaryDirectory() as directory:
            output = pathlib.Path(directory)
            victim = output / "victim"
            victim.write_bytes(b"do not alter")
            original_verify = soak._verify_published_failure
            calls = 0

            def swap_final_then_verify(final_name, descriptor, identity, expected, links):
                nonlocal calls
                calls += 1
                if calls == 2:
                    os.unlink(final_name, dir_fd=descriptor)
                    os.symlink(victim.name, final_name, dir_fd=descriptor)
                return original_verify(final_name, descriptor, identity, expected, links)

            with mock.patch.object(
                soak,
                "_verify_published_failure",
                side_effect=swap_final_then_verify,
            ), self.assertRaisesRegex(
                soak.FailureArtifactPublicationError,
                "opened safely",
            ):
                soak.publish_failure_artifact(output, data)
            self.assertEqual(victim.read_bytes(), b"do not alter")
            self.assertFalse((output / soak.FAILURE_ARTIFACT_NAME).exists())
            self.assertEqual(list(output.glob(f"{soak.FAILURE_TEMP_PREFIX}*")), [])

    def test_windows_publication_is_typed_unsupported_before_any_write(self) -> None:
        data = b'{"claim":"timeout-contract-not-established"}\n'
        with tempfile.TemporaryDirectory() as directory:
            output = pathlib.Path(directory)
            with mock.patch.object(soak, "IS_WINDOWS", True), self.assertRaisesRegex(
                soak.AtomicEvidencePublicationUnsupported,
                "unsupported on Windows",
            ):
                soak.publish_failure_artifact(output, data)
            self.assertEqual(list(output.iterdir()), [])
            stderr = io.StringIO()
            with mock.patch.object(soak, "IS_WINDOWS", True), mock.patch.object(
                soak,
                "run",
                side_effect=soak.SoakError("injected unexpected run failure"),
            ), contextlib.redirect_stderr(stderr):
                status = soak.main(
                    [
                        "--mode",
                        "unit",
                        "--output-dir",
                        str(output),
                    ]
                )
            self.assertEqual(status, 2)
            self.assertEqual(list(output.iterdir()), [])
            self.assertIn("injected unexpected run failure", stderr.getvalue())
            self.assertIn("AtomicEvidencePublicationUnsupported", stderr.getvalue())
            self.assertNotIn("timeout-correctness-only", stderr.getvalue())


if __name__ == "__main__":
    unittest.main()
