#!/usr/bin/env python3
"""Focused fail-closed tests for tools/performance_contract.py."""

from __future__ import annotations

import argparse
import csv
from contextlib import ExitStack
import functools
import importlib.util
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
import unittest
from unittest import mock
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import performance_contract as contract


TRUSTED_GIT = Path("/usr/bin/git")


def load_process_module():
    process_path = Path(__file__).with_name("git-bench-process.py")
    spec = importlib.util.spec_from_file_location("git_bench_process_test", process_path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {process_path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def load_windows_metrics_module():
    module_path = Path(__file__).with_name("windows_child_metrics.py")
    spec = importlib.util.spec_from_file_location("windows_child_metrics_test", module_path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {module_path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def complete_metadata() -> dict[str, object]:
    metadata = {
        "mode": "exploratory",
        "git_comparator": {
            "status": "validated",
            "bundle": "/tmp/git-http-bundle-v2.55.0",
            **contract.AUTHORITATIVE_GIT_COMPARATOR,
        },
        "build_profile": "release",
        "code_dirty": False,
        "identity_complete": True,
        "python": {
            "path": "/usr/bin/python3",
            "sha256": "python",
            "version": "Python 3",
        },
        "command_corpus_sha256": "corpus",
        "fixture_sha256": "fixture",
        "policy": {
            "warmups": 3,
            "measured_pairs": 30,
            "cold_starts": 10,
            "ordering": "interleaved-paired",
            "random_seed": 1,
        },
        "statistics_policy": contract.statistics_policy_payload(),
        "sample_phase_policy": contract.SAMPLE_PHASE_POLICY,
        "environment_policy": contract.ENVIRONMENT_POLICY,
        "environment": {
            "values": {
                "PATH": "/usr/bin",
                "HOME": "/tmp/home",
                "TMPDIR": "/tmp",
                "LANG": "C",
                "LC_ALL": "C",
                "LC_CTYPE": "C",
                "TZ": "UTC",
                "GIT_CONFIG_GLOBAL": "/tmp/gitconfig",
                "GIT_CONFIG_NOSYSTEM": "1",
                "PYTHONHASHSEED": "0",
                "ZMIN_BENCH_REJECTED_ENV": "fixture",
                "ZMIN_BENCH_NETWORK_ENV_POLICY": "rejected",
            }
        },
        "host": {"os": "Darwin"},
    }
    metadata["mandatory_manifest"] = "pilot"
    metadata["mandatory_lanes"] = ["status"]
    metadata["mandatory_manifest_sha256"] = contract.mandatory_manifest_digest(
        "pilot", ["status"]
    )
    metadata["equivalence_manifest"] = "equivalence.tsv"
    metadata["equivalence_plan_sha256"] = contract.equivalence_plan_digest(
        "pilot", ["status"], metadata["policy"]
    )
    return metadata


def complete_standard_metadata() -> dict[str, object]:
    metadata = complete_metadata()
    metadata["mode"] = "authoritative"
    lanes = list(contract.STANDARD_MANDATORY_LANES)
    metadata["mandatory_manifest"] = "standard"
    metadata["mandatory_lanes"] = lanes
    metadata["mandatory_manifest_sha256"] = contract.mandatory_manifest_digest(
        "standard", lanes
    )
    metadata["equivalence_plan_sha256"] = contract.equivalence_plan_digest(
        "standard", lanes, metadata["policy"]
    )
    metadata["superiority_manifest"] = "superiority.tsv"
    return metadata


def complete_observed_metadata() -> dict[str, object]:
    metadata = complete_metadata()
    metadata["mode"] = "authoritative"
    lanes = list(contract.OBSERVED_MANDATORY_LANES)
    metadata["mandatory_manifest"] = "observed"
    metadata["mandatory_lanes"] = lanes
    metadata["mandatory_manifest_sha256"] = contract.mandatory_manifest_digest(
        "observed", lanes
    )
    metadata["equivalence_manifest"] = "equivalence.tsv"
    metadata["equivalence_plan_sha256"] = contract.equivalence_plan_digest(
        "observed", lanes, metadata["policy"]
    )
    metadata["superiority_manifest"] = "superiority.tsv"
    return metadata


def rows_for_lane(lane: str = "status") -> list[dict[str, str]]:
    rows: list[dict[str, str]] = []
    metrics = (
        "wall_seconds=available;user_seconds=available;sys_seconds=available;"
        "peak_rss_bytes=available;peak_job_commit_bytes=unsupported;"
        "major_page_faults=available;minor_page_faults=available;"
        "read_bytes=unsupported;write_bytes=unsupported"
    )
    for kind, count in (("warmup", 3), ("measured", 30), ("cold", 10)):
        for index in range(1, count + 1):
            pair_id = f"{lane}-{kind}-{index}"
            tool_order = ("git", "zmin") if index % 2 else ("zmin", "git")
            for order_index, tool in enumerate(tool_order, start=1):
                rows.append(
                    {
                        "tool": tool,
                        "lane": lane,
                        "sample_kind": kind,
                        "pair_id": pair_id,
                        "order_index": str(order_index),
                        "real": "0.125",
                        "user": "0.025",
                        "sys": "0.010",
                        "rss_bytes": "4096",
                        "job_commit_bytes": "unsupported",
                        "major_page_faults": "0",
                        "minor_page_faults": "2",
                        "read_bytes": "unsupported",
                        "write_bytes": "unsupported",
                        "memory_metric": "peak_rss_bytes",
                        "memory_semantics": "working_set_peak",
                        "memory_scope": "waited_child_processes",
                        "memory_unit": "bytes",
                        "metrics_availability": metrics,
                        "exit": "0",
                    }
                )
    return rows


def observed_rows_for_lane(lane: str = "observed_status") -> list[dict[str, str]]:
    rows = rows_for_lane(lane)
    for row in rows:
        if row["tool"] == "git":
            row["tool"] = "stock"
        row["real_seconds"] = row.pop("real")
        row["user_seconds"] = row.pop("user")
        row["sys_seconds"] = row.pop("sys")
        row["max_rss_bytes"] = row.pop("rss_bytes")
        row["exit"] = "0"
    return rows


def rows_for_manifest(lanes: list[str]) -> list[dict[str, str]]:
    rows: list[dict[str, str]] = []
    per_lane = {lane: rows_for_lane(lane) for lane in lanes}
    for kind, count in (("warmup", 3), ("measured", 30), ("cold", 10)):
        for index in range(1, count + 1):
            for lane in lanes:
                pair_id = f"{lane}-{kind}-{index}"
                rows.extend(
                    row
                    for row in per_lane[lane]
                    if row["pair_id"] == pair_id
                )
    return rows


def superiority_rows_for_manifest(manifest: str) -> list[dict[str, str]]:
    lanes = list(contract.MANDATORY_LANE_MANIFESTS[manifest])
    if manifest == "standard":
        rows = rows_for_manifest(lanes)
        stock_tool = "git"
        stock_field = "real"
        rss_field = "rss_bytes"
    else:
        rows = []
        for lane in lanes:
            rows.extend(observed_rows_for_lane(lane))
        stock_tool = "stock"
        stock_field = "real_seconds"
        rss_field = "max_rss_bytes"
    for row in rows:
        if row["tool"] == stock_tool:
            row[stock_field] = "1.000000"
            row[rss_field] = "4096"
        else:
            row[stock_field] = "0.500000"
            row[rss_field] = "2048"
    return rows


def equivalence_rows_for_manifest(lanes: list[str]) -> list[dict[str, str]]:
    rows: list[dict[str, str]] = []
    hashes = {
        "git_stdout_sha256": "0" * 64,
        "zmin_stdout_sha256": "0" * 64,
        "git_stderr_sha256": "1" * 64,
        "zmin_stderr_sha256": "1" * 64,
    }
    policy = {"warmup": 3, "measured": 30, "cold": 10}
    for sample_kind, count in policy.items():
        for index in range(1, count + 1):
            for lane in lanes:
                rows.append(
                    {
                        "lane": lane,
                        "sample_kind": sample_kind,
                        "phase": contract.SAMPLE_PHASE_POLICY[sample_kind]["phase"],
                        "pair_id": f"{lane}-{sample_kind}-{index}",
                        "pair_index": str(index),
                        "git_order_index": "1" if index % 2 else "2",
                        "zmin_order_index": "2" if index % 2 else "1",
                        "git_exit": "0",
                        "zmin_exit": "0",
                        **hashes,
                        "exit_equal": "true",
                        "stdout_equal": "true",
                        "stderr_equal": "true",
                    }
                )
    return rows


class PerformanceContractTests(unittest.TestCase):
    def test_strict_json_rejects_duplicate_keys_and_structural_overflow(self) -> None:
        path = Path("hostile.json")
        with self.assertRaisesRegex(contract.ContractError, "duplicate JSON key 'status'"):
            contract.load_json_bytes(
                b'{"status":"safe","nested":{"status":"first","status":"last"}}',
                path,
            )

        nested: object = 0
        for _ in range(contract.MAX_JSON_DEPTH):
            nested = {"child": nested}
        with self.assertRaisesRegex(contract.ContractError, "depth limit"):
            contract.load_json_bytes(contract.canonical_json(nested), path)

        with mock.patch.object(contract, "MAX_JSON_NODES", 3):
            with self.assertRaisesRegex(contract.ContractError, "node limit"):
                contract.load_json_bytes(b'{"items":[1,2,3]}', path)

        with mock.patch.object(contract, "MAX_JSON_BYTES", 2):
            with self.assertRaisesRegex(contract.ContractError, "byte limit"):
                contract.load_json_bytes(b'{"value":1}', path)

    def test_identity_sidecar_file_uses_strict_bounded_json_loader(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            sidecar = root / "zmin.identity.json"
            valid = {"marker": contract.RELEASE_BUILD_MARKER, "schema_version": 1}
            sidecar.write_bytes(contract.canonical_json(valid) + b"\n")
            self.assertEqual(contract.load_json(sidecar), valid)

            sidecar.write_bytes(b'{"marker":"first","marker":"last"}\n')
            with self.assertRaisesRegex(contract.ContractError, "duplicate JSON key"):
                contract.load_json(sidecar)

            nested: object = 0
            for _ in range(contract.MAX_JSON_DEPTH):
                nested = {"child": nested}
            sidecar.write_bytes(contract.canonical_json(nested) + b"\n")
            with self.assertRaisesRegex(contract.ContractError, "depth limit"):
                contract.load_json(sidecar)

            sidecar.write_bytes(b'{"items":[1,2,3]}\n')
            with mock.patch.object(contract, "MAX_JSON_NODES", 3):
                with self.assertRaisesRegex(contract.ContractError, "node limit"):
                    contract.load_json(sidecar)

            sidecar.write_bytes(b'{"value":1}\n')
            with mock.patch.object(contract, "MAX_JSON_BYTES", 2):
                with self.assertRaisesRegex(contract.ContractError, "byte limit"):
                    contract.load_json(sidecar)

            outside = root / "outside.json"
            outside.write_bytes(contract.canonical_json(valid) + b"\n")
            sidecar.unlink()
            sidecar.symlink_to(outside)
            with self.assertRaisesRegex(contract.ContractError, "symlink path component"):
                contract.load_json(sidecar)

    @staticmethod
    def init_fixture(root: Path) -> None:
        subprocess.run(["git", "init", "-q", "-b", "main", str(root)], check=True)
        subprocess.run(["git", "-C", str(root), "config", "user.name", "Benchmark"], check=True)
        subprocess.run(["git", "-C", str(root), "config", "user.email", "benchmark@example.invalid"], check=True)
        (root / "file.txt").write_text("initial\n", encoding="utf-8")
        subprocess.run(["git", "-C", str(root), "add", "file.txt"], check=True)
        subprocess.run(
            ["git", "-C", str(root), "-c", "commit.gpgsign=false", "commit", "-qm", "initial"],
            check=True,
            env={**os.environ, "GIT_AUTHOR_DATE": "1700000000 +0000", "GIT_COMMITTER_DATE": "1700000000 +0000"},
        )

    def test_complete_interleaved_rows_are_authoritative_ready(self) -> None:
        self.assertEqual(contract.validate_rows(rows_for_lane(), complete_metadata()), [])

    def test_optional_unsupported_bytes_are_not_a_zero_metric(self) -> None:
        metadata = complete_metadata()
        rows = rows_for_lane()
        self.assertTrue(all(row["metrics_availability"].endswith("unsupported") for row in rows))
        self.assertEqual(contract.validate_rows(rows, metadata), [])

    def test_observed_stock_and_zmin_pairing_is_authoritative_ready(self) -> None:
        self.assertEqual(
            contract.validate_rows(observed_rows_for_lane(), complete_metadata()), []
        )

    def test_authoritative_standard_manifest_requires_exact_lane_order(self) -> None:
        metadata = complete_standard_metadata()
        rows = rows_for_manifest(list(contract.STANDARD_MANDATORY_LANES))
        self.assertEqual(contract.validate_rows(rows, metadata), [])
        for lanes in (
            list(contract.STANDARD_MANDATORY_LANES[:-1]),
            list(contract.STANDARD_MANDATORY_LANES[1:])
            + [contract.STANDARD_MANDATORY_LANES[0]],
            list(contract.STANDARD_MANDATORY_LANES) + ["extra"],
        ):
            tampered = dict(metadata)
            tampered["mandatory_lanes"] = lanes
            tampered["mandatory_manifest_sha256"] = contract.mandatory_manifest_digest(
                "standard", lanes
            )
            reasons = contract.policy_reasons(tampered)
            self.assertIn(
                "authoritative mandatory lane manifest is incomplete or reordered",
                reasons,
            )
        missing_rows = rows_for_manifest(list(contract.STANDARD_MANDATORY_LANES[:-1]))
        self.assertTrue(
            any(
                "authoritative result lanes are missing" in reason
                for reason in contract.validate_rows(missing_rows, metadata)
            )
        )

    def test_authoritative_rows_use_sample_major_order_without_sorting_actual_rows(self) -> None:
        metadata = complete_standard_metadata()
        lanes = list(contract.STANDARD_MANDATORY_LANES)
        rows = rows_for_manifest(lanes)
        self.assertEqual(contract.validate_rows(rows, metadata), [])
        self.assertEqual(
            [
                (row["sample_kind"], row["pair_id"], row["lane"])
                for row in rows[:4]
            ],
            [
                ("warmup", "init-warmup-1", "init"),
                ("warmup", "init-warmup-1", "init"),
                ("warmup", "status-warmup-1", "status"),
                ("warmup", "status-warmup-1", "status"),
            ],
        )
        reordered = list(rows)
        reordered[2], reordered[4] = reordered[4], reordered[2]
        reasons = contract.validate_rows(reordered, metadata)
        self.assertIn("exact canonical order", " ".join(reasons))
        missing = reordered[:-1]
        reasons = contract.validate_rows(missing, metadata)
        self.assertTrue(reasons)
        duplicate = list(rows)
        duplicate[4] = dict(duplicate[2])
        reasons = contract.validate_rows(duplicate, metadata)
        self.assertIn("exact canonical order", " ".join(reasons))

    def test_per_pair_equivalence_requires_exact_authenticated_coverage(self) -> None:
        metadata = complete_standard_metadata()
        rows = equivalence_rows_for_manifest(list(contract.STANDARD_MANDATORY_LANES))
        self.assertEqual(contract.validate_equivalence(rows, metadata), [])

        cases = []
        mismatch = [dict(row) for row in rows]
        mismatch[0]["stdout_equal"] = "false"
        cases.append((mismatch, "does not prove stdout_equal"))

        stderr_mismatch = [dict(row) for row in rows]
        stderr_mismatch[0]["stderr_equal"] = "false"
        cases.append((stderr_mismatch, "does not prove stderr_equal"))

        exit_mismatch = [dict(row) for row in rows]
        exit_mismatch[0]["zmin_exit"] = "1"
        cases.append((exit_mismatch, "inconsistent exit_equal"))

        missing = rows[:-1]
        cases.append((missing, "missing pair IDs"))

        duplicate = [dict(row) for row in rows]
        duplicate[-1] = dict(duplicate[0])
        cases.append((duplicate, "duplicates pair ID"))

        lane_size = 3 + 30 + 10
        reordered = rows[lane_size : 2 * lane_size] + rows[:lane_size] + rows[2 * lane_size :]
        cases.append((reordered, "lane order is missing"))

        adjacent = [dict(row) for row in rows]
        adjacent[0], adjacent[1] = adjacent[1], adjacent[0]
        cases.append((adjacent, "canonical order"))

        for candidate, expected in cases:
            self.assertTrue(
                any(expected in reason for reason in contract.validate_equivalence(candidate, metadata)),
                expected,
            )

    def test_standard_exit_fields_and_equivalence_cross_check_are_required(self) -> None:
        metadata = complete_standard_metadata()
        rows = rows_for_manifest(list(contract.STANDARD_MANDATORY_LANES))
        equivalence = equivalence_rows_for_manifest(
            list(contract.STANDARD_MANDATORY_LANES)
        )
        self.assertEqual(contract.validate_rows(rows, metadata), [])
        self.assertEqual(
            contract.validate_equivalence_against_rows(equivalence, rows, metadata),
            [],
        )

        adjacent_tool_swap = [dict(row) for row in rows]
        adjacent_tool_swap[0], adjacent_tool_swap[1] = (
            adjacent_tool_swap[1],
            adjacent_tool_swap[0],
        )
        self.assertIn(
            "exact canonical order",
            " ".join(contract.validate_rows(adjacent_tool_swap, metadata)),
        )

        interleaved_pairs = [dict(row) for row in rows]
        interleaved_pairs[1], interleaved_pairs[2] = (
            interleaved_pairs[2],
            interleaved_pairs[1],
        )
        self.assertIn(
            "exact canonical order",
            " ".join(contract.validate_rows(interleaved_pairs, metadata)),
        )

        wrong_declared_order = [dict(row) for row in rows]
        wrong_declared_order[0]["order_index"], wrong_declared_order[1]["order_index"] = (
            wrong_declared_order[1]["order_index"],
            wrong_declared_order[0]["order_index"],
        )
        self.assertIn(
            "exact canonical order",
            " ".join(contract.validate_rows(wrong_declared_order, metadata)),
        )

        duplicate_key = [dict(row) for row in rows]
        duplicate_key[2]["pair_id"] = duplicate_key[0]["pair_id"]
        self.assertIn(
            "exact canonical order",
            " ".join(contract.validate_rows(duplicate_key, metadata)),
        )

        missing_exit = [dict(row) for row in rows]
        missing_exit[0].pop("exit")
        self.assertTrue(
            any("missing exit status" in reason for reason in contract.validate_rows(missing_exit, metadata))
        )

        malformed_exit = [dict(row) for row in rows]
        malformed_exit[0]["exit"] = "not-an-integer"
        self.assertTrue(
            any("invalid exit status" in reason for reason in contract.validate_rows(malformed_exit, metadata))
        )

        row_exit_mismatch = [dict(row) for row in rows]
        row_exit_mismatch[1]["exit"] = "1"
        self.assertTrue(
            any(
                "zmin exit differs from result row" in reason
                for reason in contract.validate_equivalence_against_rows(
                    equivalence, row_exit_mismatch, metadata
                )
            )
        )

        manifest_exit_mismatch = [dict(row) for row in equivalence]
        manifest_exit_mismatch[0]["zmin_exit"] = "1"
        self.assertTrue(
            any(
                "inconsistent exit_equal" in reason
                for reason in contract.validate_equivalence(manifest_exit_mismatch, metadata)
            )
        )

    def test_observed_equivalence_cross_check_includes_phase_and_exit(self) -> None:
        metadata = complete_observed_metadata()
        rows = []
        for lane in contract.OBSERVED_MANDATORY_LANES:
            rows.extend(observed_rows_for_lane(lane))
        equivalence = equivalence_rows_for_manifest(
            list(contract.OBSERVED_MANDATORY_LANES)
        )
        self.assertEqual(contract.validate_rows(rows, metadata), [])
        self.assertEqual(contract.validate_equivalence(equivalence, metadata), [])
        self.assertEqual(
            contract.validate_equivalence_against_rows(equivalence, rows, metadata),
            [],
        )

        wrong_phase = [dict(row) for row in equivalence]
        wrong_phase[0]["phase"] = "disk-cold"
        self.assertIn(
            "invalid phase",
            " ".join(contract.validate_equivalence(wrong_phase, metadata)),
        )
        wrong_exit = [dict(row) for row in rows]
        wrong_exit[0]["exit"] = "1"
        self.assertIn(
            "stock exit differs from result row",
            " ".join(
                contract.validate_equivalence_against_rows(
                    equivalence, wrong_exit, metadata
                )
            ),
        )

    def test_observed_authoritative_phase_order_and_mismatch_gate(self) -> None:
        repo = Path(__file__).resolve().parents[1]
        source = (repo / "tools/git-observed-client-bench.sh").read_text(encoding="utf-8")
        measured = source.index('for run in $(seq 1 "$repeats")')
        cold = source.index('if (( cold_starts > 0 )); then')
        self.assertLess(measured, cold)

        metadata = complete_metadata()
        metadata["mode"] = "authoritative"
        metadata["mandatory_manifest"] = "observed"
        lanes = ["observed_status"]
        metadata["mandatory_lanes"] = lanes
        metadata["mandatory_manifest_sha256"] = contract.mandatory_manifest_digest(
            "observed", lanes
        )
        metadata["equivalence_plan_sha256"] = contract.equivalence_plan_digest(
            "observed", lanes, metadata["policy"]
        )
        canonical = equivalence_rows_for_manifest(lanes)
        self.assertEqual(contract.validate_equivalence(canonical, metadata), [])

        phase_rows = {
            sample_kind: [row for row in canonical if row["sample_kind"] == sample_kind]
            for sample_kind in ("warmup", "measured", "cold")
        }
        old_emission = phase_rows["warmup"] + phase_rows["cold"] + phase_rows["measured"]
        self.assertIn(
            "canonical order",
            " ".join(contract.validate_equivalence(old_emission, metadata)),
        )

        mismatch = [dict(row) for row in canonical]
        mismatch[0]["stdout_equal"] = "false"
        self.assertIn(
            "does not prove stdout_equal",
            " ".join(contract.validate_equivalence(mismatch, metadata)),
        )

    def test_standard_equivalence_mismatch_retains_all_pair_artifact_bindings(self) -> None:
        repo = Path(__file__).resolve().parents[1]
        source = (repo / "tools/git-performance-bench.sh").read_text(encoding="utf-8")
        self.assertIn("retain_pair_artifacts", source)
        self.assertIn('pair_git_metrics_file', source)
        self.assertIn('pair_zmin_metrics_file', source)
        self.assertIn('artifact_copy_root "$out_dir" "$evidence_artifact_identity"', source)
        mismatch = source.index('if [[ "$exit_equal" != true')
        mismatch_end = source.index('local equivalence_row', mismatch)
        self.assertLess(
            source.index("retain_pair_artifacts", mismatch),
            mismatch_end,
        )

    def test_strict_tsv_rejects_duplicate_headers_extra_cells_short_rows(self) -> None:
        cases = (
            b"a\ta\n1\t2\n",
            b"a\tb\n1\t2\t3\n",
            b"a\tb\n1\n",
            b"a\t\n1\t2\n",
        )
        for data in cases:
            with self.assertRaises(contract.ContractError):
                contract.read_rows_bytes(data, Path("rows.tsv"))
        equivalence_header = "\t".join(contract.EQUIVALENCE_MANIFEST_FIELDS).encode()
        for suffix in (b"\tforged\n", b"\nshort\n"):
            with self.assertRaises(contract.ContractError):
                contract.read_equivalence_bytes(
                    equivalence_header + suffix,
                    Path("equivalence.tsv"),
                )
        duplicate_header = b"\t".join(
            [
                field.encode()
                for field in (*contract.EQUIVALENCE_MANIFEST_FIELDS[:-1], "stderr_equal", "stderr_equal")
            ]
        )
        with self.assertRaises(contract.ContractError):
            contract.read_equivalence_bytes(
                duplicate_header + b"\n",
                Path("equivalence.tsv"),
            )

    def test_equivalence_manifest_tamper_and_toctou_fail_closed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "equivalence.tsv"
            fields = "\t".join(contract.EQUIVALENCE_MANIFEST_FIELDS)
            path.write_text(fields + "\n", encoding="utf-8")
            snapshot = contract.read_file_snapshot(path)
            path.write_text(fields + "\tforged\n", encoding="utf-8")
            reasons = contract.verify_file_snapshots([snapshot])
            self.assertIn(f"input changed during finish: {path.resolve()}", reasons)
            forged_header = "\t".join(fields.split("\t")[:-1] + ["forged"])
            with self.assertRaisesRegex(contract.ContractError, "unexpected schema"):
                contract.read_equivalence_bytes(forged_header.encode() + b"\n", path)

    def test_pinned_equivalence_snapshot_rejects_late_file_and_directory_swaps(self) -> None:
        if os.name == "nt":
            self.skipTest("pinned directory fixture requires Unix descriptors")
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            retained = root / "retained"
            retained.mkdir()
            equivalence = retained / "equivalence.tsv"
            equivalence.write_text("captured\n", encoding="utf-8")
            sentinel = root / "external-sentinel"
            sentinel.write_text("must remain unchanged\n", encoding="utf-8")
            identity = contract.path_identity(retained)
            snapshot = contract.artifact_read_snapshot(
                retained,
                "equivalence.tsv",
                expected_directory_identity=identity,
            )

            replacement = retained / "replacement.tsv"
            replacement.write_text("late replacement\n", encoding="utf-8")
            equivalence.unlink()
            equivalence.symlink_to(sentinel)
            reasons = contract.verify_retained_file_snapshots(
                retained,
                identity,
                [snapshot],
            )
            self.assertTrue(any("retained input could not be revalidated" in reason for reason in reasons))
            self.assertEqual(sentinel.read_text(encoding="utf-8"), "must remain unchanged\n")

            equivalence.unlink()
            equivalence.write_text("captured\n", encoding="utf-8")
            snapshot = contract.artifact_read_snapshot(
                retained,
                "equivalence.tsv",
                expected_directory_identity=identity,
            )
            moved = root / "retained-before-swap"
            retained.rename(moved)
            retained.mkdir()
            reasons = contract.verify_retained_file_snapshots(
                retained,
                identity,
                [snapshot],
            )
            self.assertTrue(any("identity changed" in reason for reason in reasons))
            self.assertEqual(
                (moved / "equivalence.tsv").read_text(encoding="utf-8"),
                "captured\n",
            )

    def test_pinned_superiority_snapshot_rejects_late_substitution(self) -> None:
        if os.name == "nt":
            self.skipTest("pinned directory fixture requires Unix descriptors")
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            retained = root / "retained"
            retained.mkdir()
            sentinel = root / "external-sentinel"
            sentinel.write_text("must remain unchanged\n", encoding="utf-8")
            summary = retained / "superiority.tsv"
            summary.write_bytes(b"captured\n")
            identity = contract.path_identity(retained)
            snapshot = contract.artifact_read_snapshot(
                retained,
                "superiority.tsv",
                expected_directory_identity=identity,
            )
            summary.unlink()
            summary.symlink_to(sentinel)
            reasons = contract.verify_retained_file_snapshots(
                retained, identity, [snapshot]
            )
            self.assertTrue(any("retained input could not be revalidated" in reason for reason in reasons))
            self.assertEqual(sentinel.read_text(encoding="utf-8"), "must remain unchanged\n")

    def test_missing_required_metric_fails_closed(self) -> None:
        rows = rows_for_lane()
        measured_row = next(row for row in rows if row["sample_kind"] == "measured")
        measured_row["metrics_availability"] = measured_row["metrics_availability"].replace(
            "peak_rss_bytes=available", "peak_rss_bytes=unsupported"
        )
        reasons = contract.validate_rows(rows, complete_metadata())
        self.assertIn("required metric unavailable: peak_rss_bytes", reasons)

    def test_windows_uses_job_commit_identity_without_claiming_rss(self) -> None:
        metadata = complete_metadata()
        metadata["host"] = {"os": "Windows"}
        rows = rows_for_lane()
        for row in rows:
            row["rss_bytes"] = "unsupported"
            row["job_commit_bytes"] = "4096"
            row["memory_metric"] = "peak_job_commit_bytes"
            row["memory_semantics"] = "job_commit_peak"
            row["memory_scope"] = "job_process_tree"
            row["metrics_availability"] = row["metrics_availability"].replace(
                "peak_rss_bytes=available;peak_job_commit_bytes=unsupported",
                "peak_rss_bytes=unsupported;peak_job_commit_bytes=available",
            )
        self.assertEqual(contract.validate_rows(rows, metadata), [])

    def test_cross_semantic_memory_pair_fails_closed(self) -> None:
        metadata = complete_standard_metadata()
        rows = superiority_rows_for_manifest("standard")
        row = next(
            row
            for row in rows
            if row["tool"] == "zmin" and row["sample_kind"] == "measured"
        )
        row["memory_metric"] = "peak_job_commit_bytes"
        _summary, reasons = contract.compute_superiority_summary(rows, metadata)
        self.assertTrue(any("memory_metric must be peak_rss_bytes" in reason for reason in reasons))

    def test_non_zero_standard_result_fails_closed(self) -> None:
        rows = rows_for_lane()
        rows[0]["exit"] = "1"
        reasons = contract.validate_rows(rows, complete_metadata())
        self.assertIn("lane status row 1 has non-zero command exit: 1", reasons)

    def test_authoritative_gix_rows_are_rejected(self) -> None:
        rows = rows_for_lane()
        metadata = complete_metadata()
        metadata["mode"] = "authoritative"
        gix_row = dict(rows[0])
        gix_row["tool"] = "gix"
        rows.append(gix_row)
        reasons = contract.validate_rows(rows, metadata)
        self.assertIn("lane status has unexpected authoritative tool: gix", reasons)

    def test_numeric_and_availability_corruption_fails_closed(self) -> None:
        rows = rows_for_lane()
        rows[0]["real"] = "-1"
        rows[1]["read_bytes"] = "0"
        reasons = contract.validate_rows(rows, complete_metadata())
        self.assertTrue(any("invalid nonnegative wall_seconds" in reason for reason in reasons))
        self.assertTrue(any("encodes unsupported read_bytes as 0" in reason for reason in reasons))

    def test_strict_superiority_is_positive_for_standard_and_observed(self) -> None:
        for manifest, metadata_factory in (
            ("standard", complete_standard_metadata),
            ("observed", complete_observed_metadata),
        ):
            rows = superiority_rows_for_manifest(manifest)
            summary, reasons = contract.compute_superiority_summary(
                rows, metadata_factory()
            )
            self.assertEqual(reasons, [], manifest)
            self.assertEqual(len(summary), len(contract.MANDATORY_LANE_MANIFESTS[manifest]) * 2)
            self.assertTrue(all(row["median_wall_strict"] == "true" for row in summary))
            self.assertTrue(all(row["p95_wall_strict"] == "true" for row in summary))
            self.assertTrue(all(row["peak_memory_strict"] == "true" for row in summary))
            payload = contract.serialize_superiority_summary(summary)
            parsed = contract.read_superiority_summary_bytes(payload, Path("superiority.tsv"))
            self.assertEqual(
                contract.validate_superiority_summary_artifact(
                    payload, Path("superiority.tsv"), parsed, summary
                ),
                [],
            )

    def test_statistics_policy_is_authenticated_and_summary_fields_are_recomputed(self) -> None:
        metadata = complete_standard_metadata()
        self.assertNotIn(
            "authoritative statistics policy is missing or unauthenticated",
            contract.policy_reasons(metadata),
        )
        metadata["statistics_policy"] = dict(contract.statistics_policy_payload())
        metadata["statistics_policy"]["bootstrap_resamples"] = 10
        self.assertIn(
            "authoritative statistics policy is missing or unauthenticated",
            contract.policy_reasons(metadata),
        )
        rows = superiority_rows_for_manifest("standard")
        summary, reasons = contract.compute_superiority_summary(rows, complete_standard_metadata())
        self.assertEqual(reasons, [])
        forged = [dict(summary[0])]
        forged[0]["median_wall_ci_high"] = "0"
        payload = contract.serialize_superiority_summary(forged)
        self.assertTrue(
            contract.validate_superiority_summary_artifact(
                payload, Path("superiority.tsv"), forged, summary
            )
        )

    def test_paired_statistics_are_seeded_order_independently_and_cover_counts(self) -> None:
        measured_stock = [contract.Fraction((index + 1) * 10, 1) for index in range(30)]
        ratios = [
            contract.Fraction(((index * 17) % 29) + 1, 40 + index)
            for index in range(30)
        ]
        self.assertEqual(len(set(ratios)), 30)
        measured_zmin = [
            value * ratio for value, ratio in zip(measured_stock, ratios)
        ]
        metadata = complete_standard_metadata()
        result = contract._paired_log_ratio_statistics(
            measured_stock,
            measured_zmin,
            seed=contract._statistics_seed(
                metadata, "status", "measured", "median_wall"
            ),
        )
        reversed_result = contract._paired_log_ratio_statistics(
            list(reversed(measured_stock)),
            list(reversed(measured_zmin)),
            seed=123,
        )
        mispaired_result = contract._paired_log_ratio_statistics(
            measured_stock,
            list(reversed(measured_zmin)),
            seed=123,
        )
        self.assertEqual(result["wins"], "30")
        self.assertEqual(result["pvalue"], "0.000000000931322574615478515625")
        self.assertEqual(result["stable"], "true")
        self.assertEqual(
            reversed_result,
            contract._paired_log_ratio_statistics(
                measured_stock, measured_zmin, seed=123
            ),
        )
        self.assertNotEqual(mispaired_result["ratio"], reversed_result["ratio"])
        seed_cases = (
            ("lane", "other", "measured", "median_wall"),
            ("sample_kind", "status", "cold", "median_wall"),
            ("metric", "status", "measured", "median_memory"),
        )
        baseline_seed = contract._statistics_seed(
            metadata, "status", "measured", "median_wall"
        )
        for _label, lane, sample_kind, metric in seed_cases:
            self.assertNotEqual(
                baseline_seed,
                contract._statistics_seed(metadata, lane, sample_kind, metric),
            )
        changed_seed_metadata = complete_standard_metadata()
        changed_seed_metadata["policy"]["random_seed"] = 2
        self.assertNotEqual(
            baseline_seed,
            contract._statistics_seed(
                changed_seed_metadata, "status", "measured", "median_wall"
            ),
        )
        changed_fixture_metadata = complete_standard_metadata()
        changed_fixture_metadata["fixture_sha256"] = "fixture-other"
        self.assertNotEqual(
            baseline_seed,
            contract._statistics_seed(
                changed_fixture_metadata, "status", "measured", "median_wall"
            ),
        )
        cold_stock = [contract.Fraction(index + 1, 1) for index in range(10)]
        cold_zmin = [value / 2 for value in cold_stock]
        cold = contract._paired_log_ratio_statistics(cold_stock, cold_zmin, seed=123)
        self.assertEqual(cold["wins"], "10")
        self.assertEqual(cold["stable"], "true")

    def test_paired_statistics_ties_and_zero_are_not_claims(self) -> None:
        tied = contract._paired_log_ratio_statistics(
            [1, 2, 3], [1, 2, 3], seed=123
        )
        self.assertEqual(tied["ratio"], "1")
        self.assertEqual(tied["ci_low"], "1")
        self.assertEqual(tied["ci_high"], "1")
        self.assertEqual(tied["stable"], "false")
        one_tie = contract._paired_log_ratio_statistics([1, 2, 3], [0.5, 2, 6], seed=123)
        self.assertEqual(one_tie["wins"], "1")
        self.assertEqual(one_tie["pairs"], "2")
        self.assertEqual(one_tie["pvalue"], "0.75")
        with self.assertRaises(contract.ContractError):
            contract._paired_log_ratio_statistics([0], [1], seed=123)

    def test_raw_strict_pass_with_unstable_statistics_is_inconclusive(self) -> None:
        metadata = complete_standard_metadata()
        rows = superiority_rows_for_manifest("standard")
        for row in rows:
            if row["lane"] != "status" or row["sample_kind"] != "measured":
                continue
            index = int(row["pair_id"].rsplit("-", 1)[1])
            if row["tool"] == "zmin" and index > 16:
                row["real"] = "2.000000"
            if row["tool"] == "git" and index <= 5:
                row["real"] = "100.000000"
        summary, reasons = contract.compute_superiority_summary(rows, metadata)
        status = next(
            row for row in summary
            if row["lane"] == "status" and row["sample_kind"] == "measured"
        )
        self.assertEqual(status["median_wall_strict"], "true")
        self.assertEqual(status["p95_wall_strict"], "true")
        self.assertEqual(status["peak_memory_strict"], "true")
        self.assertEqual(status["verdict"], "inconclusive")
        self.assertEqual(status["statistics_stable"], "false")
        self.assertTrue(any("statistics are inconclusive" in reason for reason in reasons))

    def test_peak_rss_strict_gate_uses_raw_bytes_not_ceil_kib(self) -> None:
        rows = superiority_rows_for_manifest("standard")
        for row in rows:
            if row["lane"] != "status" or row["sample_kind"] != "measured":
                continue
            row["rss_bytes"] = "2048" if row["tool"] == "git" else "2047"
        summary, reasons = contract.compute_superiority_summary(
            rows, complete_standard_metadata()
        )
        status = next(
            row for row in summary
            if row["lane"] == "status" and row["sample_kind"] == "measured"
        )
        self.assertEqual(status["stock_peak_memory_bytes"], "2048")
        self.assertEqual(status["zmin_peak_memory_bytes"], "2047")
        self.assertEqual(status["stock_peak_memory_kb"], "2")
        self.assertEqual(status["zmin_peak_memory_kb"], "2")
        self.assertEqual(status["peak_memory_strict"], "true")
        self.assertFalse(
            any("status measured peak memory gate failed" in reason for reason in reasons)
        )

    def test_superiority_uses_even_median_and_nearest_rank_p95(self) -> None:
        metadata = complete_standard_metadata()
        rows = superiority_rows_for_manifest("standard")
        for row in rows:
            if (
                row["lane"] == "status"
                and row["sample_kind"] == "measured"
                and row["tool"] == "zmin"
                and row["pair_id"] == "status-measured-30"
            ):
                row["real"] = "2.000000"
        summary, reasons = contract.compute_superiority_summary(rows, metadata)
        self.assertEqual(reasons, [])
        status = next(
            row for row in summary if row["lane"] == "status" and row["sample_kind"] == "measured"
        )
        self.assertEqual(status["zmin_p95_wall_ns"], "500000000")
        self.assertEqual(status["zmin_median_wall_ns"], "500000000")

        rows = superiority_rows_for_manifest("standard")
        for row in rows:
            if (
                row["lane"] == "status"
                and row["sample_kind"] == "measured"
                and row["tool"] == "zmin"
                and row["pair_id"] in {"status-measured-29", "status-measured-30"}
            ):
                row["real"] = "2.000000"
        summary, reasons = contract.compute_superiority_summary(rows, metadata)
        self.assertIn("superiority status measured p95 wall gate failed", reasons)
        status = next(
            row for row in summary if row["lane"] == "status" and row["sample_kind"] == "measured"
        )
        self.assertEqual(status["zmin_p95_wall_ns"], "2000000000")

    def test_nearest_rank_p95_uses_exact_integer_ranks(self) -> None:
        for count, expected_rank in ((1, 1), (10, 10), (20, 19), (30, 29), (100, 95)):
            values = [contract.Fraction(index, 1) for index in range(1, count + 1)]
            self.assertEqual(contract._nearest_rank_p95(values), contract.Fraction(expected_rank, 1))

    def test_even_median_preserves_half_units_in_summary_and_thresholds(self) -> None:
        self.assertEqual(contract._even_median([1, 2]), contract.Fraction(3, 2))
        self.assertEqual(contract._canonical_fraction(contract.Fraction(3, 2)), "1.5")
        metric_reasons: list[str] = []
        subnanosecond = dict(rows_for_lane()[0])
        subnanosecond["real"] = "0.0000000005"
        self.assertEqual(
            contract._superiority_metric_value(
                subnanosecond, "wall_seconds", "subnanosecond", metric_reasons
            ),
            contract.Fraction(1, 2),
        )
        self.assertEqual(metric_reasons, [])
        metadata = complete_standard_metadata()
        metadata["environment"]["values"][
            "ZMIN_BENCH_MAX_ZMIN_VS_GIT_MEDIAN_RATIO"
        ] = "0.9999999996"
        rows = superiority_rows_for_manifest("standard")
        for row in rows:
            if (
                row["lane"] == "status"
                and row["sample_kind"] == "measured"
                and row["tool"] == "git"
            ):
                index = int(row["pair_id"].rsplit("-", 1)[1])
                row["real"] = "1.000000001" if index > 15 else "1.000000000"
            if (
                row["lane"] == "status"
                and row["sample_kind"] == "cold"
                and row["tool"] == "git"
            ):
                index = int(row["pair_id"].rsplit("-", 1)[1])
                row["real"] = "1.000000001" if index > 5 else "1.000000000"
        summary, reasons = contract.compute_superiority_summary(rows, metadata)
        self.assertEqual(reasons, [])
        status = next(
            row for row in summary if row["lane"] == "status" and row["sample_kind"] == "measured"
        )
        self.assertEqual(status["stock_median_wall_ns"], "1000000000.5")
        self.assertEqual(status["zmin_median_wall_ns"], "500000000")
        self.assertEqual(status["median_wall_strict"], "true")
        cold_status = next(
            row for row in summary if row["lane"] == "status" and row["sample_kind"] == "cold"
        )
        self.assertEqual(cold_status["stock_median_wall_ns"], "1000000000.5")
        self.assertEqual(cold_status["zmin_median_wall_ns"], "500000000")
        canonical = contract.serialize_superiority_summary(summary)
        self.assertIn(b"1000000000.5", canonical)
        self.assertEqual(
            contract.validate_superiority_summary_artifact(
                canonical,
                Path("superiority.tsv"),
                contract.read_superiority_summary_bytes(canonical, Path("superiority.tsv")),
                summary,
            ),
            [],
        )

        rows = superiority_rows_for_manifest("standard")
        for row in rows:
            if row["lane"] == "status" and row["sample_kind"] == "measured" and row["tool"] == "git":
                index = int(row["pair_id"].rsplit("-", 1)[1])
                row["real"] = "1.000000001" if index > 15 else "1.000000000"
            if row["lane"] == "status" and row["sample_kind"] == "measured" and row["tool"] == "zmin":
                index = int(row["pair_id"].rsplit("-", 1)[1])
                row["real"] = "1.000000001" if index > 15 else "1.000000000"
        summary, reasons = contract.compute_superiority_summary(rows, complete_standard_metadata())
        status = next(
            row for row in summary if row["lane"] == "status" and row["sample_kind"] == "measured"
        )
        self.assertEqual(status["stock_median_wall_ns"], "1000000000.5")
        self.assertEqual(status["zmin_median_wall_ns"], "1000000000.5")
        self.assertIn("superiority status measured median wall gate failed", reasons)

    def test_superiority_rejects_slow_median_p95_and_peak_rss(self) -> None:
        metadata = complete_standard_metadata()
        rows = superiority_rows_for_manifest("standard")
        for row in rows:
            if (
                row["lane"] == "status"
                and row["sample_kind"] == "measured"
                and row["tool"] == "zmin"
                and int(row["pair_id"].rsplit("-", 1)[1]) <= 16
            ):
                row["real"] = "2.000000"
        _summary, reasons = contract.compute_superiority_summary(rows, metadata)
        self.assertIn("superiority status measured median wall gate failed", reasons)

        rows = superiority_rows_for_manifest("standard")
        for row in rows:
            if (
                row["lane"] == "status"
                and row["sample_kind"] == "measured"
                and row["tool"] == "zmin"
                and int(row["pair_id"].rsplit("-", 1)[1]) >= 29
            ):
                row["real"] = "2.000000"
        _summary, reasons = contract.compute_superiority_summary(rows, metadata)
        self.assertIn("superiority status measured p95 wall gate failed", reasons)

        rows = superiority_rows_for_manifest("standard")
        next(
            row
            for row in rows
            if row["lane"] == "status"
            and row["sample_kind"] == "measured"
            and row["tool"] == "zmin"
        )["rss_bytes"] = "8192"
        _summary, reasons = contract.compute_superiority_summary(rows, metadata)
        self.assertIn("superiority status measured peak memory gate failed", reasons)

    def test_superiority_equality_fails_each_required_gate(self) -> None:
        metadata = complete_standard_metadata()
        rows = superiority_rows_for_manifest("standard")
        for row in rows:
            if row["lane"] == "status" and row["sample_kind"] == "measured" and row["tool"] == "zmin":
                row["real"] = "1.000000"
        _summary, reasons = contract.compute_superiority_summary(rows, metadata)
        self.assertIn("superiority status measured median wall gate failed", reasons)
        self.assertIn("superiority status measured p95 wall gate failed", reasons)

        rows = superiority_rows_for_manifest("standard")
        for row in rows:
            if (
                row["lane"] == "status"
                and row["sample_kind"] == "measured"
                and row["tool"] == "zmin"
                and int(row["pair_id"].rsplit("-", 1)[1]) >= 29
            ):
                row["real"] = "1.000000"
        _summary, reasons = contract.compute_superiority_summary(rows, metadata)
        self.assertIn("superiority status measured p95 wall gate failed", reasons)

        rows = superiority_rows_for_manifest("standard")
        next(
            row
            for row in rows
            if row["lane"] == "status"
            and row["sample_kind"] == "measured"
            and row["tool"] == "zmin"
        )["rss_bytes"] = "4096"
        _summary, reasons = contract.compute_superiority_summary(rows, metadata)
        self.assertIn("superiority status measured peak memory gate failed", reasons)

    def test_superiority_rejects_missing_unsupported_zero_negative_and_nan_metrics(self) -> None:
        metadata = complete_standard_metadata()
        mutations = (
            ("real", "0", "wall_seconds must be finite and positive"),
            ("real", "-1", "wall_seconds must be finite and positive"),
            ("real", "NaN", "wall_seconds must be finite and positive"),
            ("real", "1e10000", "wall_seconds must fit a finite positive float"),
            ("real", "1e-10000", "wall_seconds must fit a finite positive float"),
            ("rss_bytes", "0", "peak_rss_bytes must be a positive integer"),
            ("rss_bytes", "-1", "peak_rss_bytes must be a positive integer"),
            ("rss_bytes", "9" * 1025, "numeric value is too large"),
        )
        for field, value, expected in mutations:
            rows = superiority_rows_for_manifest("standard")
            row = next(row for row in rows if row["lane"] == "status" and row["sample_kind"] == "measured" and row["tool"] == "zmin")
            row[field] = value
            _summary, reasons = contract.compute_superiority_summary(rows, metadata)
            self.assertTrue(any(expected in reason for reason in reasons), (field, value, reasons))

        rows = superiority_rows_for_manifest("standard")
        row = next(row for row in rows if row["lane"] == "status" and row["sample_kind"] == "measured" and row["tool"] == "zmin")
        row["metrics_availability"] = row["metrics_availability"].replace(
            "wall_seconds=available", "wall_seconds=unsupported"
        )
        _summary, reasons = contract.compute_superiority_summary(rows, metadata)
        self.assertTrue(any("wall_seconds is missing or unsupported" in reason for reason in reasons))

    def test_superiority_rejects_insufficient_counts_and_summary_tampering(self) -> None:
        metadata = complete_standard_metadata()
        rows = superiority_rows_for_manifest("standard")
        rows = [
            row
            for row in rows
            if row["pair_id"] != "status-measured-30" or row["tool"] != "zmin"
        ]
        _summary, reasons = contract.compute_superiority_summary(rows, metadata)
        self.assertTrue(any("status measured zmin has 29 samples" in reason for reason in reasons))

        rows = superiority_rows_for_manifest("standard")
        expected, reasons = contract.compute_superiority_summary(rows, metadata)
        self.assertEqual(reasons, [])
        payload = contract.serialize_superiority_summary(expected)
        actual = contract.read_superiority_summary_bytes(payload, Path("superiority.tsv"))
        cases = (
            (actual[:-1], "missing or extra"),
            (actual + [dict(actual[-1])], "missing or extra"),
            (list(reversed(actual)), "exactly match"),
        )
        forged = [dict(row) for row in actual]
        forged[0]["zmin_median_wall_ns"] = "999"
        cases += ((forged, "exactly match"),)
        for candidate, expected_reason in cases:
            candidate_payload = contract.serialize_superiority_summary(candidate)
            candidate_reasons = contract.validate_superiority_summary_artifact(
                candidate_payload, Path("superiority.tsv"), candidate, expected
            )
            self.assertTrue(any(expected_reason in reason for reason in candidate_reasons))
        with self.assertRaises(contract.ContractError):
            contract.read_superiority_summary_bytes(
                b"forged\n", Path("superiority.tsv")
            )

    def test_optional_thresholds_only_tighten_and_exploratory_never_claims(self) -> None:
        metadata = complete_standard_metadata()
        rows = superiority_rows_for_manifest("standard")
        summary, reasons = contract.compute_superiority_summary(rows, metadata)
        self.assertEqual(reasons, [])
        metadata["environment"]["values"]["ZMIN_BENCH_MAX_ZMIN_VS_GIT_MEDIAN_RATIO"] = "0.9"
        _summary, reasons = contract.compute_superiority_summary(rows, metadata)
        self.assertEqual(reasons, [])
        metadata["environment"]["values"]["ZMIN_BENCH_MAX_ZMIN_VS_GIT_MEDIAN_RATIO"] = "0.4"
        _summary, reasons = contract.compute_superiority_summary(rows, metadata)
        self.assertTrue(any("optional superiority threshold" in reason for reason in reasons))
        metadata["environment"]["values"]["ZMIN_BENCH_MAX_ZMIN_VS_GIT_MEDIAN_RATIO"] = "1.1"
        _summary, reasons = contract.compute_superiority_summary(rows, metadata)
        self.assertTrue(any("must be finite and in (0,1]" in reason for reason in reasons))
        metadata["environment"]["values"]["ZMIN_BENCH_MAX_ZMIN_VS_GIT_MEDIAN_RATIO"] = "0"
        self.assertTrue(any("must be finite and in (0,1]" in reason for reason in contract.policy_reasons(metadata)))
        exploratory_rows, exploratory_reasons = contract.compute_superiority_summary(
            rows_for_manifest(list(contract.STANDARD_MANDATORY_LANES)), complete_metadata()
        )
        self.assertEqual(exploratory_rows, [])
        self.assertEqual(exploratory_reasons, [])
        self.assertIn("run mode is not authoritative", contract.policy_reasons(complete_metadata()))

    def test_duplicate_and_missing_pair_ids_fail_closed(self) -> None:
        rows = rows_for_lane()
        duplicate = dict(next(row for row in rows if row["sample_kind"] == "measured"))
        rows.append(duplicate)
        rows = [
            row
            for row in rows
            if row["pair_id"] != "status-cold-10"
        ]
        reasons = contract.validate_rows(rows, complete_metadata())
        self.assertTrue(any("cold missing pair IDs" in reason for reason in reasons))
        self.assertTrue(any("must have exactly one row per tool" in reason for reason in reasons))

    def test_fixture_fingerprint_binds_git_metadata(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.init_fixture(root)
            before = contract.fixture_fingerprint(root, TRUSTED_GIT)
            (root / ".git" / "HEAD").write_text("ref: refs/heads/other\n")
            self.assertNotEqual(before, contract.fixture_fingerprint(root, TRUSTED_GIT))

    def test_fixture_hash_anchor_binds_template_and_rejects_mutations(self) -> None:
        repo = Path(__file__).resolve().parents[1]
        source = (repo / "tools/git-performance-bench.sh").read_text(encoding="utf-8")
        self.assertIn(
            'fixture-hash --root "$src" --git-bin "$git_bin" --template-dir "$init_template_dir"',
            source,
        )
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "fixture"
            root.mkdir()
            self.init_fixture(root)
            template = root / ".zmin-bench-empty-template"
            template.mkdir()
            binding, identities = contract.benchmark_template_binding_and_identity_map(
                root, template
            )
            expected = contract.fixture_fingerprint(
                root,
                TRUSTED_GIT,
                bound_directory_identities=identities,
            )
            command = [
                sys.executable,
                str(repo / "tools/performance_contract.py"),
                "fixture-hash",
                "--root",
                str(root),
                "--git-bin",
                str(TRUSTED_GIT),
                "--template-dir",
                binding["path"],
                "--template-identity",
                contract.artifact_identity_token(identities[binding["path"]]),
            ]
            result = subprocess.run(
                command,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                check=False,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout.strip(), expected)

            template.rmdir()
            template.mkdir()
            replaced = subprocess.run(
                command,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                check=False,
            )
            self.assertNotEqual(replaced.returncode, 0)
            self.assertIn("identity changed", replaced.stderr)
            with self.assertRaisesRegex(contract.ContractError, "identity changed"):
                contract.fixture_fingerprint(
                    root,
                    TRUSTED_GIT,
                    bound_directory_identities=identities,
                )

            before_git_mutation = contract.fixture_fingerprint(
                root,
                TRUSTED_GIT,
                bound_directory_identities={
                    str(template.resolve()): contract.path_identity(template)
                },
            )
            (root / ".git" / "HEAD").write_text(
                "ref: refs/heads/mutated\n", encoding="utf-8"
            )
            after_git_mutation = contract.fixture_fingerprint(
                root,
                TRUSTED_GIT,
                bound_directory_identities={
                    str(template.resolve()): contract.path_identity(template)
                },
            )
            self.assertNotEqual(before_git_mutation, after_git_mutation)

    def test_fixture_hash_cli_preserves_exploratory_symlink_policy(self) -> None:
        repo = Path(__file__).resolve().parents[1]
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "fixture"
            root.mkdir()
            self.init_fixture(root)
            template = root / ".zmin-bench-empty-template"
            template.mkdir()
            identity = contract.path_identity(template)
            root_alias = Path(directory) / "fixture-alias"
            git_alias = Path(directory) / "git-alias"
            root_alias.symlink_to(root, target_is_directory=True)
            git_alias.symlink_to(TRUSTED_GIT)
            command = [
                sys.executable,
                str(repo / "tools/performance_contract.py"),
                "fixture-hash",
                "--root",
                str(root_alias),
                "--git-bin",
                str(git_alias),
                "--template-dir",
                str(template),
                "--template-identity",
                contract.artifact_identity_token(identity),
            ]
            exploratory = subprocess.run(
                command,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                check=False,
            )
            self.assertEqual(exploratory.returncode, 0, exploratory.stderr)

            authoritative = subprocess.run(
                [*command, "--strict-paths"],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                check=False,
            )
            self.assertNotEqual(authoritative.returncode, 0)
            self.assertIn("symlink path component", authoritative.stderr)

            missing_identity = subprocess.run(
                command[:-2],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                check=False,
            )
            self.assertNotEqual(missing_identity.returncode, 0)
            self.assertIn("requires --template-identity", missing_identity.stderr)

            identity_without_template = subprocess.run(
                [
                    *command[: command.index("--template-dir")],
                    "--template-identity",
                    contract.artifact_identity_token(identity),
                ],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                check=False,
            )
            self.assertNotEqual(identity_without_template.returncode, 0)
            self.assertIn("requires --template-dir", identity_without_template.stderr)

    def test_source_identity_uses_trusted_git_not_path_shadow(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "fixture"
            root.mkdir()
            self.init_fixture(root)
            fake_git = Path(directory) / "git"
            marker = Path(directory) / "fake-git-used"
            fake_git.write_text(
                f"#!/bin/sh\ntouch '{marker}'\nexit 99\n",
                encoding="utf-8",
            )
            fake_git.chmod(0o755)
            with mock.patch.dict(os.environ, {"PATH": str(fake_git.parent)}):
                state = contract.repo_state(root, TRUSTED_GIT)
                fingerprint = contract.fixture_fingerprint(root, TRUSTED_GIT)
            self.assertTrue(state["commit"])
            self.assertTrue(fingerprint)
            self.assertFalse(marker.exists())

    def test_fixture_fingerprint_binds_git_resolved_alternates(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.init_fixture(root)
            raw_path = subprocess.check_output(
                ["git", "-C", str(root), "rev-parse", "--git-path", "objects/info/alternates"],
                text=True,
            ).strip()
            alternates = Path(raw_path)
            if not alternates.is_absolute():
                alternates = (root / alternates).resolve()
            alternates.parent.mkdir(parents=True, exist_ok=True)
            before = contract.fixture_fingerprint(root, TRUSTED_GIT)
            alternates.write_text("/tmp/alternate-objects\n", encoding="utf-8")
            self.assertNotEqual(before, contract.fixture_fingerprint(root, TRUSTED_GIT))

    def test_empty_init_template_is_contained_and_fail_closed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "fixture"
            root.mkdir()
            self.init_fixture(root)
            template = root / ".zmin-bench-empty-template"
            template.mkdir()
            self.assertEqual(
                contract.validate_empty_template_directory(root, template),
                template.resolve(),
            )
            captured_identity = contract.path_identity(template)
            template.rmdir()
            template.mkdir()
            with self.assertRaisesRegex(contract.ContractError, "identity changed"):
                contract.validate_empty_template_directory(
                    root,
                    template,
                    expected_identity=captured_identity,
                )
            with self.assertRaisesRegex(contract.ContractError, "identity changed"):
                contract.fixture_fingerprint(
                    root,
                    TRUSTED_GIT,
                    bound_directory_identities={
                        str(template.resolve()): captured_identity
                    },
                )
            replacement = root / "replacement-template"
            replacement.mkdir()
            with self.assertRaisesRegex(contract.ContractError, "identity changed"):
                contract.validate_empty_template_directory(
                    root,
                    replacement,
                    expected_identity=captured_identity,
                )
            self.assertFalse((root / "evidence.json").exists())

            missing = root / "missing-template"
            with self.assertRaises(contract.ContractError):
                contract.validate_empty_template_directory(root, missing)

            (template / "unexpected").write_text("not empty\n", encoding="utf-8")
            with self.assertRaises(contract.ContractError):
                contract.validate_empty_template_directory(root, template)
            (template / "unexpected").unlink()

            before = contract.fixture_fingerprint(root, TRUSTED_GIT)
            external = Path(directory) / "external-template"
            external.mkdir()
            sentinel = external / "sentinel"
            sentinel.write_text("preserve\n", encoding="utf-8")
            template.rmdir()
            template.symlink_to(external, target_is_directory=True)
            with self.assertRaises(contract.ContractError):
                contract.validate_empty_template_directory(root, template)
            self.assertEqual(sentinel.read_text(encoding="utf-8"), "preserve\n")
            self.assertNotEqual(before, contract.fixture_fingerprint(root, TRUSTED_GIT))

    def test_standard_init_uses_identical_quiet_argv(self) -> None:
        source = (
            Path(__file__).resolve().parents[1] / "tools" / "git-performance-bench.sh"
        ).read_text(encoding="utf-8")
        self.assertIn(
            '$(shell_quote "$git_bin") init -q $(shell_quote "$tmp_dir/git-init-$n")',
            source,
        )
        self.assertIn(
            '$(shell_quote "$zmin_bin") init -q $(shell_quote "$tmp_dir/zmin-init-$n")',
            source,
        )
        self.assertIn('if [[ "$op" == "init" ]]', source)
        self.assertIn('--expected-identity "$init_template_identity"', source)
        self.assertIn('--template-identity "$init_template_identity"', source)
        self.assertIn("--strict-paths", source)
        self.assertIn('--bound-directory-env GIT_TEMPLATE_DIR', source)
        self.assertIn('export GIT_TEMPLATE_DIR="$init_template_dir"', source)
        self.assertNotIn("template-preflight +", source)
        self.assertNotRegex(source, r"\+\s+--fixture-root")
        self.assertIn('cd -- "$init_template_dir"', source)
        self.assertIn("pwd -P", source)
        self.assertNotIn("bound_directory_args", source)

    def test_bash32_init_status_smoke_passes_bound_and_ordinary_paths(self) -> None:
        repo = Path(__file__).resolve().parents[1]
        stock = Path(
            os.environ.get(
                "ZMIN_W51_STOCK_BIN",
                "/private/tmp/skron-git-w51-stock.vr2JAX/git-2.55.0/git",
            )
        )
        zmin = Path(
            os.environ.get(
                "ZMIN_W51_ZMIN_BIN",
                repo / "target/release/zmin",
            )
        )
        if not stock.is_file() or not zmin.is_file():
            self.skipTest("pinned stock and preserved clean Zmin binaries are required")
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "smoke-results"
            environment = os.environ.copy()
            for key in tuple(environment):
                if key.startswith("ZMIN_BENCH_"):
                    environment.pop(key)
            environment.update(
                {
                    "GIT_BIN": str(stock),
                    "ZMIN_BIN": str(zmin),
                    "GIX_BIN": "/nonexistent",
                    "ZMIN_BENCH_EVIDENCE_MODE": "smoke",
                    "ZMIN_BENCH_OPS": "init,status",
                    "ZMIN_BENCH_WARMUPS": "0",
                    "ZMIN_BENCH_REPEATS": "1",
                    "ZMIN_BENCH_COLD_STARTS": "0",
                    "ZMIN_BENCH_COMMITS": "2",
                    "ZMIN_BENCH_FILES_PER_COMMIT": "1",
                    "ZMIN_BENCH_OUT_DIR": str(output),
                }
            )
            result = subprocess.run(
                ["/bin/bash", str(repo / "tools/git-performance-bench.sh")],
                cwd=repo,
                env=environment,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                timeout=45,
                check=False,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertTrue((output / "evidence.json").is_file())
            with (output / "bench.tsv").open(newline="", encoding="utf-8") as handle:
                rows = list(csv.DictReader(handle, delimiter="\t"))
            self.assertEqual({row["op"] for row in rows}, {"init", "status"})
            metadata = json.loads((output / "metadata.json").read_text(encoding="utf-8"))
            self.assertEqual(metadata["mode"], "smoke")
            self.assertNotEqual(metadata.get("performance_claim"), "authoritative")

    def test_macos_tmp_alias_canonicalizes_template_and_preserves_identity(self) -> None:
        if sys.platform != "darwin":
            self.skipTest("macOS /tmp alias regression")
        with tempfile.TemporaryDirectory(dir="/tmp") as directory:
            raw_root = Path(directory)
            raw_template = raw_root / "empty-template"
            raw_template.mkdir()
            physical_root = Path(os.path.realpath(raw_root))
            physical_template = physical_root / "empty-template"
            self.assertEqual(Path("/tmp").resolve(), Path("/private/tmp").resolve())
            self.assertEqual(
                contract.benchmark_template_binding(raw_root, raw_template)["path"],
                str(physical_template),
            )

            canonicalized = subprocess.run(
                [
                    "bash",
                    "-c",
                    'cd -- "$1" && pwd -P',
                    "bash",
                    str(raw_template),
                ],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                check=True,
            )
            self.assertEqual(Path(canonicalized.stdout.strip()), physical_template)
            identity = contract.path_identity(physical_template)
            expected_identity = contract.artifact_identity_token(identity)
            cli = subprocess.run(
                [
                    sys.executable,
                    str(Path(__file__).resolve().parents[1] / "tools/performance_contract.py"),
                    "template-preflight",
                    "--fixture-root",
                    str(raw_root),
                    "--template-dir",
                    str(raw_template),
                    "--expected-identity",
                    expected_identity,
                ],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                check=False,
            )
            self.assertEqual(cli.returncode, 0, cli.stderr)
            self.assertEqual(cli.stdout.strip(), expected_identity)

            alias_root = raw_root.parent / f"{raw_root.name}-alias"
            alias_root.symlink_to(physical_root, target_is_directory=True)
            try:
                with self.assertRaises(contract.ContractError):
                    contract.validate_empty_template_directory(
                        physical_root,
                        alias_root / "empty-template",
                        expected_identity=identity,
                    )
            finally:
                alias_root.unlink()

            raw_template.rmdir()
            raw_template.mkdir()
            with self.assertRaisesRegex(contract.ContractError, "identity changed"):
                contract.validate_empty_template_directory(
                    physical_root,
                    raw_template,
                    expected_identity=identity,
                )

    def test_template_preflight_cli_accepts_valid_fixture(self) -> None:
        repo = Path(__file__).resolve().parents[1]
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "fixture"
            template = root / "empty-template"
            template.mkdir(parents=True)
            result = subprocess.run(
                [
                    sys.executable,
                    str(repo / "tools/performance_contract.py"),
                    "template-preflight",
                    "--fixture-root",
                    str(root),
                    "--template-dir",
                    str(template),
                ],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                check=False,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertRegex(result.stdout.strip(), r"^[0-9]+:[0-9]+$")

    def test_bound_directory_child_keeps_original_inode_after_path_swap(self) -> None:
        process = load_process_module()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            template = root / "template"
            template.mkdir()
            expected = contract.path_identity(template)
            binding = process.bind_directory_for_child(
                "GIT_TEMPLATE_DIR", template, expected
            )
            try:
                original = root / "original-template"
                template.rename(original)
                template.mkdir()
                child_environment = os.environ.copy()
                child_environment[binding.env_name] = binding.env_value
                observed = subprocess.run(
                    [
                        sys.executable,
                        "-c",
                        "import os; print(os.stat(os.environ['GIT_TEMPLATE_DIR']).st_ino)",
                    ],
                    env=child_environment,
                    pass_fds=(binding.fd,),
                    preexec_fn=functools.partial(os.fchdir, binding.fd),
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                    text=True,
                    check=True,
                )
                self.assertEqual(int(observed.stdout.strip()), expected["st_ino"])
                self.assertNotEqual(
                    int(observed.stdout.strip()), contract.path_identity(template)["st_ino"]
                )
            finally:
                os.close(binding.fd)

    def test_bound_directory_rejects_invalid_identity_paths_and_primitives(self) -> None:
        process = load_process_module()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            template = root / "template"
            template.mkdir()
            expected = contract.path_identity(template)

            wrong_identity = dict(expected)
            wrong_identity["st_ino"] = int(wrong_identity["st_ino"]) + 1
            with self.assertRaisesRegex(contract.ContractError, "identity changed"):
                process.bind_directory_for_child(
                    "GIT_TEMPLATE_DIR", template, wrong_identity
                )

            missing = root / "missing"
            with self.assertRaises(contract.ContractError):
                process.bind_directory_for_child("GIT_TEMPLATE_DIR", missing, expected)

            external = root / "external"
            external.mkdir()
            symlink = root / "symlink"
            symlink.symlink_to(external, target_is_directory=True)
            with self.assertRaises(contract.ContractError):
                process.bind_directory_for_child("GIT_TEMPLATE_DIR", symlink, expected)

            (template / "not-empty").write_text("x", encoding="utf-8")
            with self.assertRaisesRegex(contract.ContractError, "must be empty"):
                process.bind_directory_for_child(
                    "GIT_TEMPLATE_DIR", template, expected
                )
            (template / "not-empty").unlink()

            with mock.patch.object(process.os, "O_DIRECTORY", None):
                with self.assertRaisesRegex(contract.ContractError, "requires POSIX"):
                    process.bind_directory_for_child(
                        "GIT_TEMPLATE_DIR", template, expected
                    )
            with mock.patch.object(process.os, "O_NOFOLLOW", None):
                with self.assertRaisesRegex(contract.ContractError, "requires POSIX"):
                    process.bind_directory_for_child(
                        "GIT_TEMPLATE_DIR", template, expected
                    )

    def test_pinned_init_equivalence_uses_shared_empty_template(self) -> None:
        repo = Path(__file__).resolve().parents[1]
        stock = Path(
            os.environ.get(
                "ZMIN_W51_STOCK_BIN",
                "/private/tmp/skron-git-w51-stock.vr2JAX/git-2.55.0/git",
            )
        )
        zmin = Path(os.environ.get("ZMIN_W51_ZMIN_BIN", repo / "target/release/zmin"))
        if not stock.is_file() or not zmin.is_file():
            self.skipTest("pinned stock and clean Zmin binaries are required for the init probe")
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            template = root / "empty-template"
            template.mkdir()
            environment = os.environ.copy()
            environment.update(
                {
                    "GIT_CONFIG_GLOBAL": "/dev/null",
                    "GIT_CONFIG_NOSYSTEM": "1",
                    "GIT_TEMPLATE_DIR": str(template),
                    "LC_ALL": "C",
                }
            )
            warning = subprocess.run(
                [str(stock), "init", "-q", str(root / "without-template")],
                env={key: value for key, value in environment.items() if key != "GIT_TEMPLATE_DIR"},
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=True,
            )
            self.assertIn(b"templates not found", warning.stderr)

            artifact_root = root / "artifacts"
            artifact_root.mkdir()
            artifact_identity = contract.artifact_identity_token(
                contract.path_identity(artifact_root)
            )
            template_identity = contract.artifact_identity_token(
                contract.path_identity(template)
            )
            process_path = repo / "tools/git-bench-process.py"
            results = []
            for label, binary in (("stock", stock), ("zmin", zmin)):
                result = subprocess.run(
                    [
                        sys.executable,
                        str(process_path),
                        "--artifact-root",
                        str(artifact_root),
                        "--artifact-root-identity",
                        artifact_identity,
                        "--stdout",
                        str(artifact_root / f"{label}.stdout"),
                        "--stderr",
                        str(artifact_root / f"{label}.stderr"),
                        "--metrics",
                        str(artifact_root / f"{label}.metrics.tsv"),
                        "--bound-directory-env",
                        "GIT_TEMPLATE_DIR",
                        "--bound-directory",
                        str(template),
                        "--bound-directory-identity",
                        template_identity,
                        "--",
                        str(binary),
                        "init",
                        "-q",
                        str(root / f"{label}-init"),
                    ],
                    env=environment,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                    check=False,
                )
                self.assertEqual(result.returncode, 0, result.stderr.decode())
                results.append(
                    (
                        result.returncode,
                        (artifact_root / f"{label}.stdout").read_bytes(),
                        (artifact_root / f"{label}.stderr").read_bytes(),
                    )
                )
            self.assertEqual(results[0][0], results[1][0])
            self.assertEqual(results[0][1], results[1][1])
            self.assertEqual(results[0][2], results[1][2])

    def test_linked_worktree_state_is_bound_without_object_database_hashing(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "root"
            linked = Path(directory) / "linked"
            root.mkdir()
            self.init_fixture(root)
            subprocess.run(
                ["git", "-C", str(root), "worktree", "add", "-q", "-b", "linked", str(linked)],
                check=True,
            )
            before = contract.fixture_fingerprint(linked, TRUSTED_GIT)
            subprocess.run(["git", "-C", str(linked), "config", "benchmark.flag", "one"], check=True)
            self.assertNotEqual(before, contract.fixture_fingerprint(linked, TRUSTED_GIT))

    def test_fetch_snapshot_does_not_mutate_bound_source_identity(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "source"
            remote = Path(directory) / "remote.git"
            consumer = Path(directory) / "consumer"
            updater = Path(directory) / "updater"
            root.mkdir()
            self.init_fixture(root)
            subprocess.run(["git", "init", "-q", "--bare", str(remote)], check=True)
            subprocess.run(["git", "-C", str(root), "remote", "add", "origin", str(remote)], check=True)
            subprocess.run(["git", "-C", str(root), "push", "-q", "origin", "main"], check=True)
            before = contract.fixture_fingerprint(root, TRUSTED_GIT)
            subprocess.run(["git", "clone", "-q", str(remote), str(consumer)], check=True)
            subprocess.run(["git", "clone", "-q", str(remote), str(updater)], check=True)
            subprocess.run(["git", "-C", str(updater), "config", "user.name", "Benchmark"], check=True)
            subprocess.run(["git", "-C", str(updater), "config", "user.email", "benchmark@example.invalid"], check=True)
            (updater / "next.txt").write_text("next\n", encoding="utf-8")
            subprocess.run(["git", "-C", str(updater), "add", "next.txt"], check=True)
            subprocess.run(
                ["git", "-C", str(updater), "-c", "commit.gpgsign=false", "commit", "-qm", "next"],
                check=True,
            )
            subprocess.run(["git", "-C", str(updater), "push", "-q", "origin", "main"], check=True)
            subprocess.run(["git", "-C", str(consumer), "fetch", "origin"], check=True)
            self.assertEqual(before, contract.fixture_fingerprint(root, TRUSTED_GIT))

    def test_real_fetch_preparation_keeps_bound_fixture_stable(self) -> None:
        repo = Path(__file__).resolve().parents[1]
        zmin = repo / "target/release/zmin"
        if not zmin.is_file():
            self.skipTest("release zmin binary is required for the bounded harness regression")
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            environment = os.environ.copy()
            environment.update(
                {
                    "GIX_BIN": "/nonexistent",
                    "ZMIN_BIN": str(zmin),
                    "ZMIN_BENCH_EVIDENCE_MODE": "exploratory",
                    "ZMIN_BENCH_OPS": "fetch-noop,fetch-incremental",
                    "ZMIN_BENCH_REPEATS": "1",
                    "ZMIN_BENCH_WARMUPS": "0",
                    "ZMIN_BENCH_COLD_STARTS": "0",
                    "ZMIN_BENCH_COMMITS": "2",
                    "ZMIN_BENCH_FILES_PER_COMMIT": "1",
                    "ZMIN_BENCH_OUT_DIR": str(output),
                }
            )
            subprocess.run(
                ["bash", str(repo / "tools/git-performance-bench.sh")],
                cwd=repo,
                env=environment,
                check=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                timeout=30,
            )
            evidence = json.loads((output / "evidence.json").read_text(encoding="utf-8"))
            reasons = evidence["authoritative_reasons"]
            self.assertNotIn("fixture changed during benchmark", reasons)
            self.assertNotIn("fixture Git config changed during benchmark", reasons)
            with (output / "bench.tsv").open(newline="", encoding="utf-8") as handle:
                rows = list(csv.DictReader(handle, delimiter="\t"))
            self.assertTrue(rows)
            self.assertTrue(all(row.get("exit") == "0" for row in rows))
            with (output / "equivalence.tsv").open(newline="", encoding="utf-8") as handle:
                equivalence = list(csv.DictReader(handle, delimiter="\t"))
            self.assertEqual(len(equivalence), 2)
            self.assertTrue(
                all(
                    row.get(field) == "true"
                    for row in equivalence
                    for field in ("exit_equal", "stdout_equal", "stderr_equal")
                )
            )
            self.assertFalse(
                any("equivalence manifest" in reason for reason in reasons)
            )

    def test_authoritative_trace_and_observed_output_preflight(self) -> None:
        repo = Path(__file__).resolve().parents[1]
        zmin = repo / "target/release/zmin"
        if not zmin.is_file():
            self.skipTest("release zmin binary is required for the preflight regression")

        standard_environment = os.environ.copy()
        standard_environment.update(
            {
                "ZMIN_BIN": str(zmin),
                "ZMIN_BENCH_EVIDENCE_MODE": "authoritative",
                "ZMIN_BENCH_PHASE_TRACE_DIR": "/tmp/w5-forbidden-phase-trace",
            }
        )
        standard = subprocess.run(
            ["bash", str(repo / "tools/git-performance-bench.sh")],
            cwd=repo,
            env=standard_environment,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            timeout=10,
        )
        self.assertNotEqual(standard.returncode, 0)
        self.assertIn("forbids diagnostic tracing", standard.stderr)

        partial_environment = os.environ.copy()
        partial_environment.pop("ZMIN_BIN", None)
        partial_environment.update(
            {
                "ZMIN_BENCH_EVIDENCE_MODE": "authoritative",
                "ZMIN_BENCH_OPS": "status",
            }
        )
        partial = subprocess.run(
            ["bash", str(repo / "tools/git-performance-bench.sh")],
            cwd=repo,
            env=partial_environment,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            timeout=10,
        )
        self.assertNotEqual(partial.returncode, 0)
        self.assertIn("requires exactly these lanes in order", partial.stderr)

        for trace_name in ("ZMIN_BENCH_SSH_TRACE_DIR", "ZMIN_BENCH_SSH_PACKET_TRACE_DIR"):
            ssh_environment = os.environ.copy()
            ssh_environment.update(
                {
                    "ZMIN_BIN": str(zmin),
                    "ZMIN_BENCH_EVIDENCE_MODE": "authoritative",
                    trace_name: "/tmp/w5-forbidden-ssh-trace",
                }
            )
            ssh_attempt = subprocess.run(
                ["bash", str(repo / "tools/git-performance-bench.sh")],
                cwd=repo,
                env=ssh_environment,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                timeout=10,
            )
            self.assertNotEqual(ssh_attempt.returncode, 0)
            self.assertIn("forbids diagnostic tracing", ssh_attempt.stderr)

        observed_environment = os.environ.copy()
        observed_environment.update(
            {
                "ZMIN_BIN": str(zmin),
                "ZMIN_OBSERVED_BENCH_EVIDENCE_MODE": "authoritative",
                "ZMIN_OBSERVED_BENCH_PHASE_TRACE": "1",
            }
        )
        observed_environment.pop("ZMIN_OBSERVED_BENCH_OUT_DIR", None)
        observed = subprocess.run(
            ["bash", str(repo / "tools/git-observed-client-bench.sh"), str(repo)],
            cwd=repo,
            env=observed_environment,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            timeout=10,
        )
        self.assertNotEqual(observed.returncode, 0)
        self.assertIn("forbids diagnostic tracing", observed.stderr)

        observed_environment.pop("ZMIN_OBSERVED_BENCH_PHASE_TRACE", None)
        observed_without_output = subprocess.run(
            ["bash", str(repo / "tools/git-observed-client-bench.sh"), str(repo)],
            cwd=repo,
            env=observed_environment,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            timeout=10,
        )
        self.assertNotEqual(observed_without_output.returncode, 0)
        self.assertIn("explicit absolute ZMIN_OBSERVED_BENCH_OUT_DIR", observed_without_output.stderr)

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            retained = root / "retained"
            retained.mkdir()
            sentinel = retained / "sentinel"
            sentinel.write_text("untouched\n", encoding="utf-8")
            link = root / "retained-link"
            try:
                link.symlink_to(retained, target_is_directory=True)
            except OSError as error:
                self.skipTest(f"symlink creation unavailable: {error}")
            symlink_environment = os.environ.copy()
            symlink_environment.update(
                {
                    "ZMIN_BIN": str(zmin),
                    "ZMIN_OBSERVED_BENCH_EVIDENCE_MODE": "authoritative",
                    "ZMIN_OBSERVED_BENCH_OUT_DIR": str(link),
                }
            )
            symlink_attempt = subprocess.run(
                ["bash", str(repo / "tools/git-observed-client-bench.sh"), str(repo)],
                cwd=repo,
                env=symlink_environment,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                timeout=10,
            )
            self.assertNotEqual(symlink_attempt.returncode, 0)
            self.assertIn("symlink", symlink_attempt.stderr)
            self.assertEqual(sentinel.read_text(encoding="utf-8"), "untouched\n")
            self.assertFalse((retained / "summary.tsv").exists())

        observed_trace_environment = os.environ.copy()
        observed_trace_environment.update(
            {
                "ZMIN_BIN": str(zmin),
                "ZMIN_OBSERVED_BENCH_EVIDENCE_MODE": "authoritative",
                "GIT_TRACE_PACKET": "/tmp/w5-forbidden-observed-packet-trace",
            }
        )
        observed_trace_attempt = subprocess.run(
            ["bash", str(repo / "tools/git-observed-client-bench.sh"), str(repo)],
            cwd=repo,
            env=observed_trace_environment,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            timeout=10,
        )
        self.assertNotEqual(observed_trace_attempt.returncode, 0)
        self.assertIn("forbids diagnostic tracing", observed_trace_attempt.stderr)

    def test_python_resolver_ignores_path_shadow_and_rejects_relative(self) -> None:
        trusted = Path("/usr/bin/python3")
        if not trusted.is_file():
            self.skipTest("the trusted system Python path is unavailable")
        with tempfile.TemporaryDirectory() as directory:
            shadow = Path(directory) / "python3"
            marker = Path(directory) / "shadow-used"
            shadow.write_text(f"#!/bin/sh\ntouch '{marker}'\nexit 99\n", encoding="utf-8")
            shadow.chmod(0o755)
            environment = os.environ.copy()
            environment.update(
                {
                    "PATH": f"{directory}:/usr/bin:/bin",
                    "ZMIN_BENCH_PYTHON_BIN": str(trusted),
                }
            )
            resolved = subprocess.run(
                [
                    "bash",
                    "-c",
                    "source tools/benchmark-environment.sh; benchmark_resolve_python authoritative",
                ],
                cwd=Path(__file__).resolve().parents[1],
                env=environment,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                check=False,
            )
            self.assertEqual(resolved.returncode, 0, resolved.stderr)
            self.assertEqual(resolved.stdout.strip(), str(trusted))
            self.assertFalse(marker.exists())

            environment["ZMIN_BENCH_PYTHON_BIN"] = "python3"
            relative = subprocess.run(
                [
                    "bash",
                    "-c",
                    "source tools/benchmark-environment.sh; benchmark_resolve_python authoritative",
                ],
                cwd=Path(__file__).resolve().parents[1],
                env=environment,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                check=False,
            )
            self.assertNotEqual(relative.returncode, 0)
            self.assertIn("absolute path", relative.stderr)

    def test_authoritative_benchmark_rejects_auto_build(self) -> None:
        repo = Path(__file__).resolve().parents[1]
        trusted = Path("/usr/bin/python3")
        if not trusted.is_file():
            self.skipTest("the trusted system Python path is unavailable")
        with tempfile.TemporaryDirectory() as directory:
            fake_cargo = Path(directory) / "cargo"
            marker = Path(directory) / "cargo-used"
            fake_cargo.write_text(f"#!/bin/sh\ntouch '{marker}'\nexit 99\n", encoding="utf-8")
            fake_cargo.chmod(0o755)
            environment = os.environ.copy()
            environment.update(
                {
                    "PATH": f"{directory}:/usr/bin:/bin",
                    "GIT_BIN": "/usr/bin/git",
                    "GIX_BIN": "/nonexistent",
                    "ZMIN_BENCH_EVIDENCE_MODE": "authoritative",
                    "ZMIN_BENCH_PYTHON_BIN": str(trusted),
                    "ZMIN_BENCH_OUT_DIR": str(Path(directory) / "evidence"),
                }
            )
            environment.pop("ZMIN_BIN", None)
            rejected = subprocess.run(
                ["bash", str(repo / "tools/git-performance-bench.sh")],
                cwd=repo,
                env=environment,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                timeout=10,
                check=False,
            )
            self.assertNotEqual(rejected.returncode, 0)
            self.assertIn("explicit prebuilt ZMIN_BIN", rejected.stderr)
            self.assertFalse(marker.exists())

            observed_environment = dict(environment)
            observed_environment.pop("ZMIN_BIN", None)
            observed_environment["ZMIN_OBSERVED_BENCH_EVIDENCE_MODE"] = "authoritative"
            observed_rejected = subprocess.run(
                ["bash", str(repo / "tools/git-observed-client-bench.sh"), str(repo)],
                cwd=repo,
                env=observed_environment,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                timeout=10,
                check=False,
            )
            self.assertNotEqual(observed_rejected.returncode, 0)
            self.assertIn("explicit prebuilt ZMIN_BIN", observed_rejected.stderr)
            self.assertFalse(marker.exists())

    def test_authoritative_rows_must_be_retained_and_hashed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            metadata = root / "metadata.json"
            retained = root / "retained"
            retained.mkdir()
            rows = root / "external.tsv"
            raw = retained / "raw.tsv"
            metadata.write_text("{}\n", encoding="utf-8")
            rows.write_text("tool\n", encoding="utf-8")
            raw.write_text("raw\n", encoding="utf-8")
            with self.assertRaisesRegex(
                contract.ContractError,
                "inside the retained results directory",
            ):
                contract.finish_metadata(
                    argparse.Namespace(
                        metadata=str(metadata),
                        rows=str(rows),
                        output=str(root / "evidence.json"),
                        result=[str(raw)],
                        results_dir=str(retained),
                        require_authoritative=True,
                    )
                )

            retained_rows = retained / "rows.tsv"
            retained_rows.write_text("tool\n", encoding="utf-8")
            retained_metadata = retained / "retained-metadata.json"
            retained_metadata.write_text(
                json.dumps(
                    {
                        "results_dir": str(retained.resolve()),
                        "results_dir_identity": contract.path_identity(retained),
                    }
                )
                + "\n",
                encoding="utf-8",
            )
            evidence = contract.finish_metadata(
                argparse.Namespace(
                    metadata=str(retained_metadata),
                    rows=str(retained_rows),
                    output=str(retained / "retained-evidence.json"),
                    result=[str(raw)],
                    results_dir=str(retained),
                    require_authoritative=False,
                )
            )
            self.assertIn(
                str(retained_rows.resolve()),
                [item["path"] for item in evidence["raw_results"]],
            )

    def test_input_snapshots_reject_metadata_rows_and_raw_mutations(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for label in ("metadata", "rows", "raw"):
                path = root / f"{label}.input"
                path.write_bytes(f"initial-{label}\n".encode())
                snapshot = contract.read_file_snapshot(path)
                path.write_bytes(f"mutated-{label}\n".encode())
                reasons = contract.verify_file_snapshots([snapshot])
                self.assertIn(f"input changed during finish: {path.resolve()}", reasons)

            for label in ("metadata", "rows", "raw"):
                case = root / label
                case.mkdir()
                metadata_path = case / "metadata.json"
                rows_path = case / "rows.tsv"
                raw_path = case / "raw.tsv"
                metadata_path.write_text(
                    json.dumps(
                        {
                            "results_dir": str(case.resolve()),
                            "results_dir_identity": contract.path_identity(case),
                        }
                    )
                    + "\n",
                    encoding="utf-8",
                )
                rows_path.write_text("tool\n", encoding="utf-8")
                raw_path.write_text("raw\n", encoding="utf-8")
                target = {"metadata": metadata_path, "rows": rows_path, "raw": raw_path}[label]
                original_verify = contract.verify_retained_file_snapshots

                def mutate_after_snapshot(
                    retained_root: Path,
                    retained_identity: dict[str, object],
                    snapshots: list[dict[str, object]],
                ) -> list[str]:
                    target.write_text(f"mutated-{label}\n", encoding="utf-8")
                    return original_verify(retained_root, retained_identity, snapshots)

                with mock.patch.object(
                    contract,
                    "verify_retained_file_snapshots",
                    side_effect=mutate_after_snapshot,
                ):
                    evidence = contract.finish_metadata(
                        argparse.Namespace(
                            metadata=str(metadata_path),
                            rows=str(rows_path),
                            output=str(case / "evidence.json"),
                            result=[str(raw_path)],
                            results_dir=str(case),
                            require_authoritative=False,
                        )
                    )
                self.assertIn(
                    f"input changed during finish: {target.resolve()}",
                    evidence["authoritative_reasons"],
                )

    def test_results_directory_rejects_external_rows_raw_and_symlinks(self) -> None:
        if os.name == "nt":
            self.skipTest("no-follow retained-directory fixture requires Unix descriptors")
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            retained = root / "retained"
            retained.mkdir()
            metadata = retained / "metadata.json"
            metadata.write_text(
                json.dumps(
                    {
                        "results_dir": str(retained.resolve()),
                        "results_dir_identity": contract.path_identity(retained),
                    }
                )
                + "\n",
                encoding="utf-8",
            )
            retained_rows = retained / "rows.tsv"
            retained_rows.write_text("tool\n", encoding="utf-8")
            retained_raw = retained / "raw.tsv"
            retained_raw.write_text("retained\n", encoding="utf-8")
            external_rows = root / "external-rows.tsv"
            external_rows.write_text("external rows\n", encoding="utf-8")
            external_raw = root / "external-raw.tsv"
            external_raw.write_text("external raw\n", encoding="utf-8")
            sentinel = root / "sentinel"
            sentinel.write_text("preserve\n", encoding="utf-8")

            def finish(rows: Path, raw: Path) -> None:
                contract.finish_metadata(
                    argparse.Namespace(
                        metadata=str(metadata),
                        rows=str(rows),
                        output=str(retained / "evidence.json"),
                        result=[str(raw)],
                        results_dir=str(retained),
                        require_authoritative=False,
                    )
                )

            with self.assertRaisesRegex(contract.ContractError, "inside the retained"):
                finish(external_rows, retained_raw)
            with self.assertRaisesRegex(contract.ContractError, "inside the retained"):
                finish(retained_rows, external_raw)

            linked_rows = retained / "linked-rows.tsv"
            linked_rows.symlink_to(sentinel)
            with self.assertRaisesRegex(contract.ContractError, "symlink"):
                finish(linked_rows, retained_raw)
            self.assertEqual(sentinel.read_text(encoding="utf-8"), "preserve\n")
            self.assertFalse((retained / "evidence.json").exists())

    def test_exploratory_results_directory_swap_cannot_redirect_publication(self) -> None:
        if os.name == "nt":
            self.skipTest("pinned directory fixture requires Unix descriptors")
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            retained = root / "retained"
            retained.mkdir()
            sentinel = retained / "sentinel"
            sentinel.write_text("preserve\n", encoding="utf-8")
            metadata = retained / "metadata.json"
            rows = retained / "rows.tsv"
            raw = retained / "raw.tsv"
            metadata.write_text(
                json.dumps(
                    {
                        "results_dir": str(retained.resolve()),
                        "results_dir_identity": contract.path_identity(retained),
                    }
                )
                + "\n",
                encoding="utf-8",
            )
            rows.write_text("tool\n", encoding="utf-8")
            raw.write_text("raw\n", encoding="utf-8")

            original_publish = contract.atomic_write_bytes

            def replace_directory_before_publish(path: Path, data: bytes, **kwargs: object) -> None:
                backup = root / "retained-before-swap"
                retained.rename(backup)
                retained.mkdir()
                original_publish(path, data, **kwargs)

            with mock.patch.object(
                contract,
                "atomic_write_bytes",
                side_effect=replace_directory_before_publish,
            ):
                with self.assertRaisesRegex(contract.ContractError, "publication directory"):
                    contract.finish_metadata(
                        argparse.Namespace(
                            metadata=str(metadata),
                            rows=str(rows),
                            output=str(retained / "evidence.json"),
                            result=[str(raw)],
                            results_dir=str(retained),
                            require_authoritative=False,
                        )
                    )
            self.assertEqual(
                (root / "retained-before-swap" / "sentinel").read_text(encoding="utf-8"),
                "preserve\n",
            )
            self.assertFalse((retained / "evidence.json").exists())

    def test_publication_fails_closed_without_posix_no_follow_primitives(self) -> None:
        if os.name == "nt":
            self.skipTest("POSIX primitive simulation requires Unix publication")
        for missing_set in (("O_DIRECTORY",), ("O_NOFOLLOW",), ("O_DIRECTORY", "O_NOFOLLOW")):
            for require_directory_fsync in (False, True):
                with tempfile.TemporaryDirectory() as directory:
                    root = Path(directory)
                    retained = root / "retained"
                    retained.mkdir()
                    sentinel = retained / "sentinel"
                    sentinel.write_text("preserve\n", encoding="utf-8")
                    identity = contract.path_identity(retained)
                    with ExitStack() as patches:
                        for missing in missing_set:
                            patches.enter_context(mock.patch.object(os, missing, None))
                        capability = contract.durability_capability(retained)
                        self.assertFalse(capability["supported"])
                        with self.assertRaisesRegex(
                            contract.ContractError,
                            "POSIX primitives|unsupported",
                        ):
                            contract.publish_benchmark_artifact(
                                retained / "artifact.json",
                                b"{}\n",
                                results_dir=retained,
                                results_dir_identity=identity,
                                require_directory_fsync=require_directory_fsync,
                            )
                    self.assertEqual(sentinel.read_text(encoding="utf-8"), "preserve\n")
                    self.assertEqual(list(retained.iterdir()), [sentinel])

    def test_finish_recomputes_tampered_authoritative_reasons(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            repo = root / "repo"
            repo.mkdir()
            self.init_fixture(repo)
            (repo / "Cargo.toml").write_text(
                "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\n",
                encoding="utf-8",
            )
            (repo / "Cargo.lock").write_text("# fixture lock\n", encoding="utf-8")
            (repo / ".gitignore").write_text("target/\n", encoding="utf-8")
            subprocess.run(
                ["git", "-C", str(repo), "add", "Cargo.toml", "Cargo.lock", ".gitignore"],
                check=True,
            )
            subprocess.run(
                ["git", "-C", str(repo), "-c", "commit.gpgsign=false", "commit", "-qm", "build inputs"],
                check=True,
            )
            build_root = root / "build"
            build_root.mkdir()
            cargo = build_root / "cargo"
            rustc = build_root / "rustc"
            python = build_root / "python"
            make = build_root / "make"
            cargo.write_text(
                "#!/bin/sh\n"
                "if [ \"$1\" = \"--version\" ]; then echo 'cargo 1.95.0 fixture'; exit 0; fi\n"
                "case \" $* \" in *\" --locked \"*) ;; *) exit 98 ;; esac\n"
                f"mkdir -p '{repo / 'target' / 'release'}'\n"
                f"printf '#!/bin/sh\\n[ \"$1\" = \"--version\" ] && echo zmin-fixture\\n' > '{repo / 'target' / 'release' / 'zmin'}'\n"
                f"chmod 755 '{repo / 'target' / 'release' / 'zmin'}'\n",
                encoding="utf-8",
            )
            rustc.write_text(
                "#!/bin/sh\necho 'rustc 1.95.0 fixture'\n",
                encoding="utf-8",
            )
            python.write_text(
                "#!/bin/sh\necho 'Python 3.9 fixture'\n",
                encoding="utf-8",
            )
            make.write_text(
                "#!/bin/sh\necho 'GNU Make 4.4 fixture'\n",
                encoding="utf-8",
            )
            for tool in (cargo, rustc, python, make):
                tool.chmod(0o755)
            binary = repo / "target" / "release" / "zmin"
            marker = contract.build_release_identity(
                argparse.Namespace(
                    repo_root=str(repo),
                    git_bin=str(TRUSTED_GIT),
                    cargo_bin=str(cargo),
                    rustc_bin=str(rustc),
                    python_bin=str(python),
                    make_bin=str(make),
                )
            )
            self.assertTrue(binary.is_file())
            self.assertEqual(
                marker["sidecar_path"], str(contract.identity_sidecar(binary).resolve())
            )
            fixture = root / "fixture"
            fixture.mkdir()
            self.init_fixture(fixture)
            args = argparse.Namespace(
                repo_root=repo,
                git_bin=str(TRUSTED_GIT),
                zmin_bin=str(binary),
                python_bin=str(python),
                make_bin=str(make),
                fixture_root=str(fixture),
                output=str(root / "metadata-start.json"),
                mode="authoritative",
                build_profile="release",
                identity_sidecar="",
                command_corpus="tampered-metadata-test",
                warmups=3,
                measured_pairs=30,
                cold_starts=10,
                ordering="interleaved-paired",
                seed=1,
                harness=[str(Path(__file__).resolve())],
            )
            metadata = contract.start_metadata(args)
            rows_path = root / "rows.tsv"
            with rows_path.open("w", newline="", encoding="utf-8") as handle:
                writer = csv.DictWriter(handle, fieldnames=list(rows_for_lane()[0]), delimiter="\t")
                writer.writeheader()
                writer.writerows(rows_for_lane())
            raw_path = root / "raw.tsv"
            raw_path.write_text("raw\n", encoding="utf-8")
            start_metadata_path = root / "start.metadata.json"
            start_metadata_path.write_bytes(contract.canonical_json(metadata) + b"\n")
            start_metadata_sha256 = contract.sha256_file(start_metadata_path)

            cases = (
                ("exploratory", lambda item: item.update(mode="exploratory"), "run mode is not authoritative"),
                ("compat", lambda item: item.update(build_profile="compat"), "build profile is not release"),
                ("empty-reasons", lambda item: None, "authoritative scope requires a canonical mandatory manifest"),
                ("redirected-fixture", lambda item: item.update(fixture_root=str(root)), "finish anchor mismatch for fixture_root"),
                ("redirected-corpus", lambda item: item.update(command_corpus="other-corpus"), "finish anchor mismatch for command corpus"),
                ("empty-harnesses", lambda item: item.update(harnesses=[]), "finish anchor mismatch for harness list"),
                ("redirected-harness", lambda item: item["harnesses"].__setitem__(0, {"path": str(root), "sha256": "redirected"}), "finish anchor mismatch for harness list"),
                ("redirected-binary", lambda item: item["git"].update(path=item["zmin"]["path"]), "finish anchor mismatch for git binary path"),
            )
            for name, mutate, expected in cases:
                tampered = json.loads(json.dumps(metadata))
                mutate(tampered)
                tampered["authoritative_reasons"] = []
                metadata_path = root / f"{name}.metadata.json"
                output_path = root / f"{name}.evidence.json"
                metadata_path.write_text(json.dumps(tampered), encoding="utf-8")
                result = contract.finish_metadata(
                    argparse.Namespace(
                        metadata=str(metadata_path),
                        rows=str(rows_path),
                        output=str(output_path),
                        result=[str(raw_path)],
                        results_dir="",
                        require_authoritative=False,
                        anchor_repo_root=metadata["repo_root"],
                        anchor_fixture_root=metadata["fixture_root"],
                        anchor_command_corpus=metadata["command_corpus"],
                        anchor_fixture_sha256=metadata["fixture_sha256"],
                        anchor_git_bin=metadata["git"]["path"],
                        anchor_git_sha256=metadata["git"]["sha256"],
                        anchor_git_version=metadata["git"]["version"],
                        anchor_zmin_bin=metadata["zmin"]["path"],
                        anchor_zmin_sha256=metadata["zmin"]["sha256"],
                        anchor_zmin_version=metadata["zmin"]["version"],
                        anchor_python_bin=metadata["python"]["path"],
                        anchor_python_sha256=metadata["python"]["sha256"],
                        anchor_python_version=metadata["python"]["version"],
                        anchor_make_bin=metadata["make"]["path"],
                        anchor_make_sha256=metadata["make"]["sha256"],
                        anchor_make_version=metadata["make"]["version"],
                        anchor_build_profile=metadata["build_profile"],
                        anchor_identity_sidecar=metadata["identity_sidecar"],
                        anchor_identity_sidecar_sha256=metadata["identity_sidecar_sha256"] or "missing",
                        anchor_start_metadata_sha256=start_metadata_sha256,
                        anchor_harness=[item["path"] for item in metadata["harnesses"]],
                        anchor_harness_sha256=[item["sha256"] for item in metadata["harnesses"]],
                    )
                )
                self.assertIn(expected, result["authoritative_reasons"])

    def test_pairing_and_counts_fail_closed(self) -> None:
        rows = rows_for_lane()
        rows = [row for row in rows if not (row["sample_kind"] == "cold" and row["pair_id"].endswith("-10"))]
        next(row for row in rows if row["sample_kind"] == "measured" and row["tool"] == "git")["order_index"] = "2"
        reasons = contract.validate_rows(rows, complete_metadata())
        self.assertTrue(any("cold missing pair IDs" in reason for reason in reasons))
        self.assertTrue(any("order must be 1,2" in reason for reason in reasons))

    def test_identity_policy_rejects_dirty_compat_and_incomplete_runs(self) -> None:
        metadata = complete_metadata()
        metadata["build_profile"] = "compat"
        metadata["code_dirty"] = True
        metadata["identity_complete"] = False
        reasons = contract.policy_reasons(metadata)
        self.assertIn("build profile is not release", reasons)
        self.assertIn("working tree is dirty", reasons)
        self.assertIn("binary identity is incomplete or stale", reasons)

    def test_exploratory_mode_can_never_be_authoritative(self) -> None:
        metadata = complete_metadata()
        metadata["mode"] = "exploratory"
        self.assertIn("run mode is not authoritative", contract.policy_reasons(metadata))

    def test_sanitized_release_marker_binds_build_and_rejects_tampering(self) -> None:
        trusted_python = Path("/usr/bin/python3")
        if not trusted_python.is_file():
            self.skipTest("the trusted system Python path is unavailable")
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            repo = root / "repo"
            repo.mkdir()
            self.init_fixture(repo)
            (repo / "Cargo.toml").write_text(
                "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\n",
                encoding="utf-8",
            )
            (repo / "Cargo.lock").write_text("# fixture lock\n", encoding="utf-8")
            (repo / ".gitignore").write_text("target/\n", encoding="utf-8")
            subprocess.run(
                ["git", "-C", str(repo), "add", "Cargo.toml", "Cargo.lock", ".gitignore"],
                check=True,
            )
            subprocess.run(
                ["git", "-C", str(repo), "-c", "commit.gpgsign=false", "commit", "-qm", "build inputs"],
                check=True,
            )
            build_root = root / "build"
            cargo = build_root / "cargo"
            rustc = build_root / "rustc"
            make = build_root / "make"
            cargo_invocation = build_root / "cargo-invocation"
            target = repo / "target"
            binary = target / "release" / "zmin"
            sidecar = binary.with_name("zmin.identity.json")
            build_root.mkdir()
            cargo.write_text(
                "#!/bin/sh\n"
                "if [ \"$1\" = \"--version\" ]; then echo 'cargo 1.95.0 fixture'; exit 0; fi\n"
                f"printf '%s|%s\\n' \"$CARGO_NET_OFFLINE\" \"$*\" > '{cargo_invocation}'\n"
                "if [ \"$CARGO_NET_OFFLINE\" != \"true\" ]; then exit 97; fi\n"
                "case \" $* \" in *\" --release --locked --offline \"*) ;; *) exit 98 ;; esac\n"
                f"mkdir -p '{binary.parent}'\n"
                f"printf '#!/bin/sh\\n[ \"$1\" = \"--version\" ] && echo zmin-fixture\\n' > '{binary}'\n"
                f"chmod 755 '{binary}'\n",
                encoding="utf-8",
            )
            rustc.write_text(
                "#!/bin/sh\n"
                "echo 'rustc 1.95.0 fixture'\n",
                encoding="utf-8",
            )
            make.write_text(
                "#!/bin/sh\n"
                "echo 'GNU Make 4.4 fixture'\n",
                encoding="utf-8",
            )
            cargo.chmod(0o755)
            rustc.chmod(0o755)
            make.chmod(0o755)
            with mock.patch.dict(os.environ, {"CARGO_NET_OFFLINE": "0"}):
                marker = contract.build_release_identity(
                    argparse.Namespace(
                        repo_root=str(repo),
                        git_bin=str(TRUSTED_GIT),
                        cargo_bin=str(cargo),
                        rustc_bin=str(rustc),
                        python_bin=str(trusted_python),
                        make_bin=str(make),
                    )
                )
            self.assertEqual(marker["marker"], contract.RELEASE_BUILD_MARKER)
            self.assertEqual(marker["cargo_profile"], "release")
            canonical_command = [
                str(cargo.resolve()),
                "build",
                "--manifest-path",
                str(repo.resolve() / "Cargo.toml"),
                "--release",
                "--locked",
                "--offline",
                "-p",
                "zmin-cli",
                "--bin",
                "zmin",
            ]
            self.assertEqual(marker["build_command"], canonical_command)
            self.assertEqual(
                cargo_invocation.read_text(encoding="utf-8").split("|", 1)[0],
                "true",
            )
            self.assertEqual(marker["binary"]["path"], str(binary.resolve()))
            self.assertEqual(marker["make"]["path"], str(make.resolve()))
            self.assertEqual(marker["make"]["sha256"], contract.sha256_file(make))
            self.assertEqual(marker["make"]["version"], "GNU Make 4.4 fixture")
            self.assertEqual(
                marker["build_manifest"]["policy"],
                contract.RELEASE_BUILD_ENVIRONMENT_POLICY,
            )
            matched, detail = contract.sidecar_matches(
                marker,
                repo_root=repo.resolve(),
                git_bin=TRUSTED_GIT,
                binary=binary.resolve(),
                profile="release",
                state=contract.repo_state(repo.resolve(), TRUSTED_GIT),
                cargo_lock_sha256=contract.sha256_file(repo / "Cargo.lock"),
                python_bin=trusted_python,
                make_bin=make,
            )
            self.assertTrue(matched, detail)

            locked_index = canonical_command.index("--locked")
            invalid_commands = {
                "missing offline": [
                    item for item in canonical_command if item != "--offline"
                ],
                "reordered offline": [
                    *canonical_command[:locked_index],
                    "--offline",
                    "--locked",
                    *canonical_command[locked_index + 2 :],
                ],
                "extra frozen": [
                    *canonical_command[: locked_index + 2],
                    "--frozen",
                    *canonical_command[locked_index + 2 :],
                ],
            }
            for label, invalid_command in invalid_commands.items():
                invalid = json.loads(json.dumps(marker))
                invalid["build_command"] = invalid_command
                invalid["payload_sha256"] = contract.sha256_bytes(
                    contract.canonical_json(contract.marker_payload(invalid))
                )
                matched, detail = contract.sidecar_matches(
                    invalid,
                    repo_root=repo.resolve(),
                    git_bin=TRUSTED_GIT,
                    binary=binary.resolve(),
                    profile="release",
                    state=contract.repo_state(repo.resolve(), TRUSTED_GIT),
                    cargo_lock_sha256=contract.sha256_file(repo / "Cargo.lock"),
                    python_bin=trusted_python,
                    make_bin=make,
                )
                self.assertFalse(matched, label)
                self.assertIn("build command is not canonical", detail, label)

            self.assertEqual(marker["build_manifest"]["rejected_inherited_variables"], [])
            with mock.patch.dict(os.environ, {"ZMIN_BENCH_REJECTED_ENV": "measurement-only"}):
                matched, detail = contract.sidecar_matches(
                    marker,
                    repo_root=repo.resolve(),
                    git_bin=TRUSTED_GIT,
                    binary=binary.resolve(),
                    profile="release",
                    state=contract.repo_state(repo.resolve(), TRUSTED_GIT),
                    cargo_lock_sha256=contract.sha256_file(repo / "Cargo.lock"),
                    python_bin=trusted_python,
                    make_bin=make,
                )
            self.assertTrue(matched, detail)

            copied_binary = root / "copied-target" / "release" / "zmin"
            copied_binary.parent.mkdir(parents=True)
            shutil.copy2(binary, copied_binary)
            copied_binary.chmod(0o755)
            forged_copy = json.loads(json.dumps(marker))
            forged_copy["binary"]["path"] = str(copied_binary.resolve())
            forged_copy["binary"]["sha256"] = contract.sha256_file(copied_binary)
            forged_copy["payload_sha256"] = contract.sha256_bytes(
                contract.canonical_json(contract.marker_payload(forged_copy))
            )
            matched, detail = contract.sidecar_matches(
                forged_copy,
                repo_root=repo.resolve(),
                git_bin=TRUSTED_GIT,
                binary=copied_binary.resolve(),
                profile="release",
                state=contract.repo_state(repo.resolve(), TRUSTED_GIT),
                cargo_lock_sha256=contract.sha256_file(repo / "Cargo.lock"),
                python_bin=trusted_python,
                make_bin=make,
            )
            self.assertFalse(matched)
            self.assertIn("canonical Cargo release output", detail)

            relocated = json.loads(json.dumps(marker))
            relocated["build_manifest"]["values"]["CARGO_TARGET_DIR"] = str(
                root / "relocated-target"
            )
            relocated_manifest = {
                key: relocated["build_manifest"][key]
                for key in contract.RELEASE_ENVIRONMENT_FIELDS
                if key != "sha256"
            }
            relocated["build_manifest"]["sha256"] = contract.sha256_bytes(
                contract.canonical_json(relocated_manifest)
            )
            relocated["payload_sha256"] = contract.sha256_bytes(
                contract.canonical_json(contract.marker_payload(relocated))
            )
            matched, detail = contract.sidecar_matches(
                relocated,
                repo_root=repo.resolve(),
                git_bin=TRUSTED_GIT,
                binary=binary.resolve(),
                profile="release",
                state=contract.repo_state(repo.resolve(), TRUSTED_GIT),
                cargo_lock_sha256=contract.sha256_file(repo / "Cargo.lock"),
                python_bin=trusted_python,
                make_bin=make,
            )
            self.assertFalse(matched)
            self.assertIn("target directory is not canonical", detail)

            tampered = json.loads(json.dumps(marker))
            tampered["build_manifest"]["values"]["RUSTFLAGS"] = "-C opt-level=0"
            matched, detail = contract.sidecar_matches(
                tampered,
                repo_root=repo.resolve(),
                git_bin=TRUSTED_GIT,
                binary=binary.resolve(),
                profile="release",
                state=contract.repo_state(repo.resolve(), TRUSTED_GIT),
                cargo_lock_sha256=contract.sha256_file(repo / "Cargo.lock"),
                python_bin=trusted_python,
                make_bin=make,
            )
            self.assertFalse(matched)
            self.assertIn("payload digest", detail)

            previous_online = json.loads(json.dumps(marker))
            previous_online["build_manifest"]["values"]["CARGO_NET_OFFLINE"] = "false"
            policy_payload = {
                key: previous_online["build_manifest"][key]
                for key in contract.RELEASE_ENVIRONMENT_FIELDS
                if key != "sha256"
            }
            previous_online["build_manifest"]["sha256"] = contract.sha256_bytes(
                contract.canonical_json(policy_payload)
            )
            previous_online["payload_sha256"] = contract.sha256_bytes(
                contract.canonical_json(contract.marker_payload(previous_online))
            )
            matched, detail = contract.sidecar_matches(
                previous_online,
                repo_root=repo.resolve(),
                git_bin=TRUSTED_GIT,
                binary=binary.resolve(),
                profile="release",
                state=contract.repo_state(repo.resolve(), TRUSTED_GIT),
                cargo_lock_sha256=contract.sha256_file(repo / "Cargo.lock"),
                python_bin=trusted_python,
                make_bin=make,
            )
            self.assertFalse(matched)
            self.assertIn("canonical build manifest mismatch", detail)

            previous_sidecar = json.loads(json.dumps(previous_online))
            previous_sidecar["build_command"].remove("--offline")
            previous_sidecar["payload_sha256"] = contract.sha256_bytes(
                contract.canonical_json(contract.marker_payload(previous_sidecar))
            )
            matched, detail = contract.sidecar_matches(
                previous_sidecar,
                repo_root=repo.resolve(),
                git_bin=TRUSTED_GIT,
                binary=binary.resolve(),
                profile="release",
                state=contract.repo_state(repo.resolve(), TRUSTED_GIT),
                cargo_lock_sha256=contract.sha256_file(repo / "Cargo.lock"),
                python_bin=trusted_python,
                make_bin=make,
            )
            self.assertFalse(matched)
            self.assertIn("build command is not canonical", detail)

            unknown = json.loads(json.dumps(marker))
            unknown["unexpected"] = "forged"
            self.assertFalse(contract.sidecar_matches(
                unknown,
                repo_root=repo.resolve(),
                git_bin=TRUSTED_GIT,
                binary=binary.resolve(),
                profile="release",
                state=contract.repo_state(repo.resolve(), TRUSTED_GIT),
                cargo_lock_sha256=contract.sha256_file(repo / "Cargo.lock"),
                python_bin=trusted_python,
                make_bin=make,
            )[0])
            nested_unknown = json.loads(json.dumps(marker))
            nested_unknown["toolchain"]["cargo"]["unexpected"] = "forged"
            self.assertFalse(contract.sidecar_matches(
                nested_unknown,
                repo_root=repo.resolve(),
                git_bin=TRUSTED_GIT,
                binary=binary.resolve(),
                profile="release",
                state=contract.repo_state(repo.resolve(), TRUSTED_GIT),
                cargo_lock_sha256=contract.sha256_file(repo / "Cargo.lock"),
                python_bin=trusted_python,
                make_bin=make,
            )[0])
            missing = json.loads(json.dumps(marker))
            del missing["durability"]
            self.assertFalse(contract.sidecar_matches(
                missing,
                repo_root=repo.resolve(),
                git_bin=TRUSTED_GIT,
                binary=binary.resolve(),
                profile="release",
                state=contract.repo_state(repo.resolve(), TRUSTED_GIT),
                cargo_lock_sha256=contract.sha256_file(repo / "Cargo.lock"),
                python_bin=trusted_python,
                make_bin=make,
            )[0])
            rejected_tamper = json.loads(json.dumps(marker))
            rejected_tamper["build_manifest"]["rejected_inherited_variables"].append(
                "FORGED_BUILD_VARIABLE"
            )
            rejected_environment_payload = {
                key: rejected_tamper["build_manifest"][key]
                for key in contract.RELEASE_ENVIRONMENT_FIELDS
                if key != "sha256"
            }
            rejected_tamper["build_manifest"]["sha256"] = contract.sha256_bytes(
                contract.canonical_json(rejected_environment_payload)
            )
            rejected_tamper["payload_sha256"] = contract.sha256_bytes(
                contract.canonical_json(contract.marker_payload(rejected_tamper))
            )
            self.assertFalse(contract.sidecar_matches(
                rejected_tamper,
                repo_root=repo.resolve(),
                git_bin=TRUSTED_GIT,
                binary=binary.resolve(),
                profile="release",
                state=contract.repo_state(repo.resolve(), TRUSTED_GIT),
                cargo_lock_sha256=contract.sha256_file(repo / "Cargo.lock"),
                python_bin=trusted_python,
                make_bin=make,
            )[0])

            missing_make = json.loads(json.dumps(marker))
            del missing_make["make"]
            self.assertFalse(contract.sidecar_matches(
                missing_make,
                repo_root=repo.resolve(),
                git_bin=TRUSTED_GIT,
                binary=binary.resolve(),
                profile="release",
                state=contract.repo_state(repo.resolve(), TRUSTED_GIT),
                cargo_lock_sha256=contract.sha256_file(repo / "Cargo.lock"),
                python_bin=trusted_python,
                make_bin=make,
            )[0])
            for field, value, expected_detail in (
                ("path", str(root / "other-make"), "Make path mismatch"),
                ("sha256", "0" * 64, "Make sha256 mismatch"),
                ("version", "GNU Make forged", "Make version mismatch"),
            ):
                tampered_make = json.loads(json.dumps(marker))
                tampered_make["make"][field] = value
                tampered_make["payload_sha256"] = contract.sha256_bytes(
                    contract.canonical_json(contract.marker_payload(tampered_make))
                )
                matched, detail = contract.sidecar_matches(
                    tampered_make,
                    repo_root=repo.resolve(),
                    git_bin=TRUSTED_GIT,
                    binary=binary.resolve(),
                    profile="release",
                    state=contract.repo_state(repo.resolve(), TRUSTED_GIT),
                    cargo_lock_sha256=contract.sha256_file(repo / "Cargo.lock"),
                    python_bin=trusted_python,
                    make_bin=make,
                )
                self.assertFalse(matched)
                self.assertIn(expected_detail, detail)

            cargo.write_text(
                "#!/bin/sh\n"
                "if [ \"$1\" = \"--version\" ]; then echo 'cargo 1.95.0 fixture'; exit 0; fi\n"
                f"printf '#!/bin/sh\\necho rustc-swapped\\n' > '{rustc}'\n"
                f"chmod 755 '{rustc}'\n"
                f"mkdir -p '{binary.parent}'\n"
                f"printf '#!/bin/sh\\n[ \"$1\" = \"--version\" ] && echo zmin-fixture\\n' > '{binary}'\n"
                f"chmod 755 '{binary}'\n",
                encoding="utf-8",
            )
            with self.assertRaisesRegex(contract.ContractError, "cargo or rustc changed"):
                contract.build_release_identity(
                    argparse.Namespace(
                        repo_root=str(repo),
                        git_bin=str(TRUSTED_GIT),
                        cargo_bin=str(cargo),
                        rustc_bin=str(rustc),
                        python_bin=str(trusted_python),
                        make_bin=str(make),
                    )
                )

            cargo.write_text(
                "#!/bin/sh\n"
                "if [ \"$1\" = \"--version\" ]; then echo 'cargo 1.95.0 fixture'; exit 0; fi\n"
                f"mkdir -p '{binary.parent}'\n"
                f"printf '#!/bin/sh\\n[ \"$1\" = \"--version\" ] && echo zmin-fixture\\n' > '{binary}'\n"
                f"chmod 755 '{binary}'\n",
                encoding="utf-8",
            )
            rustc.write_text(
                "#!/bin/sh\n"
                "echo 'rustc 1.95.0 fixture'\n",
                encoding="utf-8",
            )
            cargo.chmod(0o755)
            rustc.chmod(0o755)
            sidecar.unlink()
            original_atomic_write = contract.atomic_write_bytes

            def mutate_source_before_publication(
                path: Path,
                data: bytes,
                **kwargs: object,
            ) -> None:
                (repo / "Cargo.lock").write_text(
                    "# mutated after build verification\n",
                    encoding="utf-8",
                )
                original_atomic_write(path, data, **kwargs)

            with mock.patch.object(
                contract,
                "atomic_write_bytes",
                side_effect=mutate_source_before_publication,
            ):
                with self.assertRaisesRegex(contract.ContractError, "input changed during finish"):
                    contract.build_release_identity(
                        argparse.Namespace(
                            repo_root=str(repo),
                            git_bin=str(TRUSTED_GIT),
                            cargo_bin=str(cargo),
                            rustc_bin=str(rustc),
                            python_bin=str(trusted_python),
                            make_bin=str(make),
                        )
                    )
            (repo / "Cargo.lock").write_text("# fixture lock\n", encoding="utf-8")

            manual = subprocess.run(
                [
                    sys.executable,
                    str(Path(__file__).with_name("performance_contract.py")),
                    "build-release",
                    "--repo-root",
                    str(repo),
                    "--git-bin",
                    str(TRUSTED_GIT),
                    "--cargo-bin",
                    str(cargo),
                    "--rustc-bin",
                    str(rustc),
                    "--python-bin",
                    str(trusted_python),
                    "--make-bin",
                    str(make),
                    "--target-dir",
                    str(target),
                    "--profile",
                    "compat",
                ],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                check=False,
            )
            self.assertNotEqual(manual.returncode, 0)
            self.assertIn("unrecognized arguments", manual.stderr)

    def test_result_manifest_rejects_nested_control_artifacts(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "retained"
            root.mkdir()
            (root / "metadata.json").write_text("{}\n", encoding="utf-8")
            raw = root / "raw.tsv"
            raw.write_text("raw\n", encoding="utf-8")
            self.assertEqual(
                contract.result_files(root, [], reject_symlinks=True),
                [raw.resolve()],
            )
            nested = root / "nested"
            nested.mkdir()
            for name in ("metadata.json", "evidence.json"):
                nested_path = nested / name
                nested_path.write_text("{}\n", encoding="utf-8")
                with self.assertRaisesRegex(
                    contract.ContractError, "nested control artifact is not allowed"
                ):
                    contract.result_files(root, [], reject_symlinks=True)
                nested_path.unlink()

    def test_authoritative_paths_reject_symlinks_and_directory_swaps(self) -> None:
        if os.name == "nt":
            self.skipTest("symlink fixture requires Unix permissions")
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            real_dir = root / "real"
            real_dir.mkdir()
            real_file = real_dir / "input"
            real_file.write_text("input\n", encoding="utf-8")
            file_link = root / "file-link"
            dir_link = root / "dir-link"
            try:
                file_link.symlink_to(real_file)
                dir_link.symlink_to(real_dir, target_is_directory=True)
            except OSError as error:
                self.skipTest(f"symlink creation unavailable: {error}")
            with self.assertRaisesRegex(contract.ContractError, "symlink"):
                contract.absolute_file(file_link, reject_symlinks=True)
            with self.assertRaisesRegex(contract.ContractError, "symlink"):
                contract.absolute_file(dir_link / "input", reject_symlinks=True)
            with self.assertRaisesRegex(contract.ContractError, "symlink"):
                contract.absolute_output_path(file_link, reject_symlinks=True)

            retained = root / "retained"
            retained.mkdir()
            (retained / "rows.tsv").write_text("tool\n", encoding="utf-8")
            (retained / "raw.tsv").write_text("raw\n", encoding="utf-8")
            result_link = retained / "raw-link.tsv"
            result_link.symlink_to(retained / "raw.tsv")
            with self.assertRaisesRegex(contract.ContractError, "symlink"):
                contract.result_files(retained, [], reject_symlinks=True)
            old_identity = contract.path_identity(retained)
            replacement = root / "replacement"
            replacement.mkdir()
            (replacement / "metadata.json").write_text(
                json.dumps({
                    "results_dir": str(retained.resolve()),
                    "results_dir_identity": old_identity,
                }),
                encoding="utf-8",
            )
            (replacement / "rows.tsv").write_text("tool\n", encoding="utf-8")
            (replacement / "raw.tsv").write_text("raw\n", encoding="utf-8")
            backup = root / "retained-old"
            retained.rename(backup)
            replacement.rename(retained)
            with self.assertRaisesRegex(contract.ContractError, "identity changed"):
                contract.finish_metadata(
                    argparse.Namespace(
                        metadata=str(retained / "metadata.json"),
                        rows=str(retained / "rows.tsv"),
                        output=str(retained / "evidence.json"),
                        result=[str(retained / "raw.tsv")],
                        results_dir=str(retained),
                        require_authoritative=True,
                    )
                )

    def test_results_directory_preflight_rejects_symlink_and_missing_authoritative_dir(self) -> None:
        if os.name == "nt":
            self.skipTest("symlink fixture requires Unix permissions")
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            retained = root / "retained"
            retained.mkdir()
            sentinel = retained / "sentinel"
            sentinel.write_text("untouched\n", encoding="utf-8")
            link = root / "retained-link"
            try:
                link.symlink_to(retained, target_is_directory=True)
            except OSError as error:
                self.skipTest(f"symlink creation unavailable: {error}")
            with self.assertRaisesRegex(contract.ContractError, "symlink"):
                contract.prepare_results_directory(link, require_existing=True)
            self.assertEqual(sentinel.read_text(encoding="utf-8"), "untouched\n")
            missing = root / "missing"
            with self.assertRaisesRegex(contract.ContractError, "must already exist"):
                contract.prepare_results_directory(missing, require_existing=True)
            self.assertFalse(missing.exists())
            created = contract.prepare_results_directory(
                root / "exploratory", require_existing=False
            )
            self.assertTrue(created.is_dir())

    def test_artifact_plan_rejects_file_symlinks_without_touching_sentinel(self) -> None:
        if os.name == "nt":
            self.skipTest("no-follow artifact fixture requires Unix permissions")
        artifact_names = (
            "summary.tsv",
            "rows.tsv",
            "results.tsv",
            "stdout.log",
            "stderr.log",
            "metrics.tsv",
            "copy.tsv",
        )
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "copy-source"
            source.write_text("copy source\n", encoding="utf-8")
            for name in artifact_names:
                artifact_root = root / name.replace(".", "-")
                artifact_root.mkdir()
                sentinel = artifact_root / "sentinel"
                sentinel.write_text("do not overwrite\n", encoding="utf-8")
                link = artifact_root / name
                try:
                    link.symlink_to(sentinel)
                except OSError as error:
                    self.skipTest(f"symlink fixture unavailable: {error}")
                with self.assertRaisesRegex(contract.ContractError, "symlink"):
                    contract.artifact_preflight_paths(artifact_root, [name])
                with self.assertRaisesRegex(contract.ContractError, "symlink"):
                    contract.artifact_write_bytes(artifact_root, name, b"replacement\n")
                with self.assertRaisesRegex(contract.ContractError, "symlink"):
                    contract.artifact_copy_file(artifact_root, name, source)
                self.assertEqual(sentinel.read_text(encoding="utf-8"), "do not overwrite\n")

    def test_artifact_process_preflight_rejects_all_child_outputs(self) -> None:
        if os.name == "nt":
            self.skipTest("no-follow artifact fixture requires Unix permissions")
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            sentinel = root / "sentinel"
            sentinel.write_text("preserve\n", encoding="utf-8")
            names = ("stdout.log", "stderr.log", "metrics.tsv", "copy.tsv")
            try:
                for name in names:
                    (root / name).symlink_to(sentinel)
            except OSError as error:
                self.skipTest(f"symlink fixture unavailable: {error}")
            identity = contract.artifact_identity_token(contract.path_identity(root))
            process = subprocess.run(
                [
                    sys.executable,
                    str(Path(__file__).with_name("git-bench-process.py")),
                    "--artifact-root",
                    str(root),
                    "--artifact-root-identity",
                    identity,
                    "--stdout",
                    str(root / "stdout.log"),
                    "--stderr",
                    str(root / "stderr.log"),
                    "--metrics",
                    str(root / "metrics.tsv"),
                    "--",
                    sys.executable,
                    "-c",
                    f"open({str(root / 'child-ran').__repr__()}, 'w').write('bad')",
                ],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
            )
            self.assertNotEqual(process.returncode, 0)
            self.assertIn("symlink", process.stderr)
            self.assertFalse((root / "child-ran").exists())
            self.assertEqual(sentinel.read_text(encoding="utf-8"), "preserve\n")

    def test_windows_child_command_and_environment_encoding(self) -> None:
        windows = load_windows_metrics_module()
        self.assertEqual(
            windows.windows_command_line([r"C:\Program Files\zmin.exe", "a b", 'quote"x']),
            subprocess.list2cmdline([r"C:\Program Files\zmin.exe", "a b", 'quote"x']),
        )
        self.assertEqual(
            windows.windows_environment_block({"z": "last", "A": "first"}),
            "A=first\x00z=last\x00\x00",
        )
        with self.assertRaisesRegex(windows.WindowsChildMetricsError, "invalid Windows environment"):
            windows.windows_environment_block({"bad=key": "value"})

    def test_windows_child_runner_assigns_before_resume_and_keeps_job_peak(self) -> None:
        windows = load_windows_metrics_module()

        class FakeApi:
            def __init__(self) -> None:
                self.events: list[object] = []

            def create_job(self) -> str:
                self.events.append("create_job")
                return "job"

            def set_kill_on_close(self, job: str) -> None:
                self.events.append(("kill_on_close", job))

            def create_process(self, command: str, cwd: str, environment: str, stdio: object):
                self.events.append(("create_process", command, cwd, environment, stdio))
                return "process", "thread"

            def assign_process(self, job: str, process: str) -> None:
                self.events.append(("assign", job, process))

            def resume_thread(self, thread: str) -> None:
                self.events.append(("resume", thread))

            def wait(self, process: str, timeout_ms: int) -> bool:
                self.events.append(("wait", timeout_ms))
                return True

            def wait_job(self, job: str, timeout_ms: int) -> bool:
                self.events.append(("wait_job", job, timeout_ms))
                return True

            def query_metrics(self, job: str):
                self.events.append(("query", job))
                return windows.JobMetrics(987654, 1.25, 0.5, 7, 11, 13)

            def exit_code(self, process: str) -> int:
                self.events.append(("exit", process))
                return 0

            def close(self, handle: str) -> None:
                self.events.append(("close", handle))

        api = FakeApi()
        result = windows.WindowsChildRunner(api=api, clock_ns=lambda: 0).run(
            ["zmin", "a b"],
            stdin=1,
            stdout=2,
            stderr=3,
            environment={"PATH": "C:\\bin"},
            cwd="C:\\work",
            timeout_seconds=1,
        )
        self.assertEqual(result.returncode, 0)
        self.assertEqual(result.metrics.peak_job_commit_bytes, 987654)
        self.assertLess(api.events.index(("assign", "job", "process")), api.events.index(("resume", "thread")))
        job_wait = next(
            event
            for event in api.events
            if isinstance(event, tuple) and event[0] == "wait_job"
        )
        self.assertLess(api.events.index(job_wait), api.events.index(("query", "job")))
        self.assertEqual(
            [event for event in api.events if isinstance(event, tuple) and event[0] == "close"],
            [("close", "thread"), ("close", "process"), ("close", "job")],
        )

    def test_windows_child_runner_timeout_terminates_job_and_returns_124(self) -> None:
        windows = load_windows_metrics_module()

        class FakeApi:
            def __init__(self) -> None:
                self.events: list[object] = []
                self.waits = [False, True]

            def create_job(self):
                return "job"

            def set_kill_on_close(self, job):
                pass

            def create_process(self, command, cwd, environment, stdio):
                return "process", "thread"

            def assign_process(self, job, process):
                self.events.append("assign")

            def resume_thread(self, thread):
                self.events.append("resume")

            def wait(self, process, timeout_ms):
                self.events.append(("wait", timeout_ms))
                return self.waits.pop(0)

            def wait_job(self, job, timeout_ms):
                self.events.append(("wait_job", job, timeout_ms))
                return True

            def terminate_job(self, job, exit_code):
                self.events.append(("terminate_job", job, exit_code))

            def query_metrics(self, job):
                self.events.append(("query", job))
                return windows.JobMetrics(123, 0.0, 0.0, 0, 0, 0)

            def close(self, handle):
                self.events.append(("close", handle))

        clock = iter((0, 1_000_000_000, 1_000_000_000))
        api = FakeApi()
        result = windows.WindowsChildRunner(api=api, clock_ns=lambda: next(clock)).run(
            ["child"],
            stdin=1,
            stdout=2,
            stderr=3,
            environment={},
            cwd="C:\\work",
            timeout_seconds=0.1,
        )
        self.assertEqual(result.returncode, 124)
        self.assertTrue(result.timed_out)
        self.assertIn(("terminate_job", "job", 124), api.events)
        self.assertLess(
            api.events.index(("terminate_job", "job", 124)),
            api.events.index(("wait_job", "job", windows.JOB_QUIESCENCE_TIMEOUT_MS)),
        )
        self.assertLess(
            api.events.index(("wait_job", "job", windows.JOB_QUIESCENCE_TIMEOUT_MS)),
            api.events.index(("query", "job")),
        )
        self.assertLess(
            api.events.index(("query", "job")),
            api.events.index(("close", "job")),
        )

    def test_windows_child_runner_times_out_descendants_after_parent_exit(self) -> None:
        windows = load_windows_metrics_module()

        class FakeApi:
            def __init__(self) -> None:
                self.events: list[object] = []
                self.job_waits = [False, True]

            def create_job(self):
                return "job"

            def set_kill_on_close(self, job):
                pass

            def create_process(self, command, cwd, environment, stdio):
                return "process", "thread"

            def assign_process(self, job, process):
                pass

            def resume_thread(self, thread):
                pass

            def wait(self, process, timeout_ms):
                self.events.append(("wait", process, timeout_ms))
                return True

            def wait_job(self, job, timeout_ms):
                self.events.append(("wait_job", job, timeout_ms))
                return self.job_waits.pop(0)

            def terminate_job(self, job, exit_code):
                self.events.append(("terminate_job", job, exit_code))

            def query_metrics(self, job):
                self.events.append(("query", job))
                return windows.JobMetrics(123, 0.0, 0.0, 0, 0, 0)

            def close(self, handle):
                self.events.append(("close", handle))

        clock = iter((0, 0, 100_000_000, 200_000_000))
        api = FakeApi()
        result = windows.WindowsChildRunner(api=api, clock_ns=lambda: next(clock)).run(
            ["child"],
            stdin=1,
            stdout=2,
            stderr=3,
            environment={},
            cwd="C:\\work",
            timeout_seconds=0.2,
        )
        self.assertEqual(result.returncode, windows.TIMEOUT_EXIT_STATUS)
        self.assertTrue(result.timed_out)
        deadline_wait = ("wait_job", "job", 100)
        cleanup_wait = ("wait_job", "job", windows.JOB_QUIESCENCE_TIMEOUT_MS)
        self.assertLess(api.events.index(deadline_wait), api.events.index(("terminate_job", "job", 124)))
        self.assertLess(api.events.index(("terminate_job", "job", 124)), api.events.index(cleanup_wait))
        self.assertLess(api.events.index(cleanup_wait), api.events.index(("query", "job")))

    def test_windows_child_runner_error_waits_for_assigned_job_quiescence(self) -> None:
        windows = load_windows_metrics_module()

        class FakeApi:
            def __init__(self) -> None:
                self.events: list[object] = []

            def create_job(self):
                return "job"

            def set_kill_on_close(self, job):
                pass

            def create_process(self, command, cwd, environment, stdio):
                return "process", "thread"

            def assign_process(self, job, process):
                self.events.append("assign")

            def resume_thread(self, thread):
                self.events.append("resume")
                raise RuntimeError("resume failed")

            def terminate_job(self, job, exit_code):
                self.events.append(("terminate_job", job, exit_code))

            def wait_job(self, job, timeout_ms):
                self.events.append(("wait_job", job, timeout_ms))
                return True

            def close(self, handle):
                self.events.append(("close", handle))

        api = FakeApi()
        with self.assertRaisesRegex(windows.WindowsChildMetricsError, "resume failed"):
            windows.WindowsChildRunner(api=api, clock_ns=lambda: 0).run(
                ["child"],
                stdin=1,
                stdout=2,
                stderr=3,
                environment={},
                cwd="C:\\work",
                timeout_seconds=1,
            )
        terminate = ("terminate_job", "job", 1)
        quiesce = ("wait_job", "job", windows.JOB_QUIESCENCE_TIMEOUT_MS)
        self.assertLess(api.events.index(terminate), api.events.index(quiesce))
        self.assertLess(api.events.index(quiesce), api.events.index(("close", "job")))

    def test_windows_child_runner_failure_cleans_unassigned_process(self) -> None:
        windows = load_windows_metrics_module()

        class FakeApi:
            def __init__(self) -> None:
                self.events: list[object] = []

            def create_job(self):
                return "job"

            def set_kill_on_close(self, job):
                pass

            def create_process(self, command, cwd, environment, stdio):
                return "process", "thread"

            def assign_process(self, job, process):
                self.events.append("assign")
                raise RuntimeError("assignment rejected")

            def terminate_process(self, process, exit_code):
                self.events.append(("terminate_process", process, exit_code))

            def wait(self, process, timeout_ms):
                self.events.append(("wait", process, timeout_ms))
                return True

            def close(self, handle):
                self.events.append(("close", handle))

        api = FakeApi()
        with self.assertRaisesRegex(windows.WindowsChildMetricsError, "assignment rejected"):
            windows.WindowsChildRunner(api=api, clock_ns=lambda: 0).run(
                ["child"],
                stdin=1,
                stdout=2,
                stderr=3,
                environment={},
                cwd="C:\\work",
                timeout_seconds=1,
            )
        self.assertIn(("terminate_process", "process", 1), api.events)
        self.assertIn(("wait", "process", windows.INFINITE), api.events)
        self.assertEqual(
            [event for event in api.events if isinstance(event, tuple) and event[0] == "close"],
            [("close", "thread"), ("close", "process"), ("close", "job")],
        )

    @unittest.skipUnless(os.name == "nt", "requires native Windows job objects")
    def test_windows_native_child_runner_smoke(self) -> None:
        windows = load_windows_metrics_module()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with open(os.devnull, "rb") as stdin, (root / "stdout.log").open("wb") as stdout, (root / "stderr.log").open("wb") as stderr:
                descendant_code = (
                    "import subprocess,sys; "
                    "child=subprocess.Popen([sys.executable,'-c',"
                    "'x=bytearray(16*1024*1024); import time; time.sleep(0.2)']); "
                    "child.wait()"
                )
                result = windows.WindowsChildRunner().run(
                    [sys.executable, "-c", descendant_code],
                    stdin=stdin,
                    stdout=stdout,
                    stderr=stderr,
                    environment=os.environ,
                    cwd=str(root),
                    timeout_seconds=5,
                )
            self.assertEqual(result.returncode, 0)
            self.assertGreater(result.metrics.peak_job_commit_bytes, 0)

    @unittest.skipUnless(os.name == "nt", "requires native Windows job objects")
    def test_process_runner_emits_the_windows_job_memory_contract(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            identity = contract.artifact_identity_token(contract.path_identity(root))
            result = subprocess.run(
                [
                    sys.executable,
                    str(Path(__file__).with_name("git-bench-process.py")),
                    "--artifact-root",
                    str(root),
                    "--artifact-root-identity",
                    identity,
                    "--stdout",
                    str(root / "stdout.log"),
                    "--stderr",
                    str(root / "stderr.log"),
                    "--metrics",
                    str(root / "metrics.tsv"),
                    "--",
                    sys.executable,
                    "-c",
                    "memory = bytearray(2 * 1024 * 1024); print(len(memory))",
                ],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                timeout=15,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual((root / "stdout.log").read_text(encoding="utf-8"), "2097152\n")
            fields = (root / "metrics.tsv").read_text(encoding="utf-8").split("\t")
            self.assertEqual(len(fields), 14)
            self.assertEqual(fields[3], "unsupported")
            self.assertGreater(int(fields[4]), 0)
            self.assertEqual(
                fields[9:13],
                ["peak_job_commit_bytes", "job_commit_peak", "job_process_tree", "bytes"],
            )
            self.assertIn("peak_job_commit_bytes=available", fields[13])
            self.assertIn("peak_rss_bytes=unsupported", fields[13])

    @unittest.skipUnless(os.name == "nt", "requires native Windows job objects")
    def test_windows_native_success_waits_for_descendant_quiescence(self) -> None:
        windows = load_windows_metrics_module()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            completed = root / "descendant-completed.txt"
            descendant_code = (
                "import pathlib,sys,time; "
                "memory=bytearray(16*1024*1024); time.sleep(0.2); "
                "pathlib.Path(sys.argv[1]).write_text(str(len(memory)))"
            )
            parent_code = (
                "import subprocess,sys; "
                "subprocess.Popen([sys.executable,'-c',sys.argv[1],sys.argv[2]])"
            )
            with open(os.devnull, "rb") as stdin, (root / "stdout.log").open("wb") as stdout, (root / "stderr.log").open("wb") as stderr:
                result = windows.WindowsChildRunner().run(
                    [sys.executable, "-c", parent_code, descendant_code, str(completed)],
                    stdin=stdin,
                    stdout=stdout,
                    stderr=stderr,
                    environment=os.environ,
                    cwd=str(root),
                    timeout_seconds=5,
                )
            self.assertEqual(result.returncode, 0)
            self.assertTrue(completed.is_file())
            self.assertEqual(completed.read_text(encoding="utf-8"), str(16 * 1024 * 1024))
            self.assertGreater(result.metrics.peak_job_commit_bytes, 0)

    @unittest.skipUnless(os.name == "nt", "requires native Windows job objects")
    def test_windows_native_child_runner_timeout_quiesces_descendants(self) -> None:
        windows = load_windows_metrics_module()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            alive = root / "descendant-survived.txt"
            child_code = (
                "import pathlib,sys,time; "
                "time.sleep(1.5); pathlib.Path(sys.argv[1]).write_text('alive')"
            )
            parent_code = (
                "import subprocess,sys,time; "
                "subprocess.Popen([sys.executable,'-c',sys.argv[1],sys.argv[2]]); "
                "time.sleep(30)"
            )
            with open(os.devnull, "rb") as stdin, (root / "stdout.log").open("wb") as stdout, (root / "stderr.log").open("wb") as stderr:
                result = windows.WindowsChildRunner().run(
                    [sys.executable, "-c", parent_code, child_code, str(alive)],
                    stdin=stdin,
                    stdout=stdout,
                    stderr=stderr,
                    environment=os.environ,
                    cwd=str(root),
                    timeout_seconds=0.5,
                )
            self.assertEqual(result.returncode, windows.TIMEOUT_EXIT_STATUS)
            self.assertTrue(result.timed_out)
            time.sleep(2.0)
            self.assertFalse(alive.exists(), "job descendant survived timeout cleanup")

    def test_process_timeout_terminates_the_entire_child_process_group(self) -> None:
        if os.name == "nt":
            self.skipTest("POSIX process-group cleanup is required")
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            identity = contract.artifact_identity_token(contract.path_identity(root))
            grandchild_pid = root / "grandchild.pid"
            child_code = (
                "import os, signal, subprocess, sys, time; "
                "grandchild=subprocess.Popen([sys.executable, '-c', "
                "\"import signal,time; signal.signal(signal.SIGTERM, signal.SIG_IGN); time.sleep(30)\"]); "
                "open(sys.argv[1], 'w').write(str(grandchild.pid)); "
                "time.sleep(30)"
            )
            result = subprocess.run(
                [
                    sys.executable,
                    str(Path(__file__).with_name("git-bench-process.py")),
                    "--artifact-root",
                    str(root),
                    "--artifact-root-identity",
                    identity,
                    "--stdout",
                    str(root / "stdout.log"),
                    "--stderr",
                    str(root / "stderr.log"),
                    "--metrics",
                    str(root / "metrics.tsv"),
                    "--timeout-seconds",
                    "0.2",
                    "--",
                    sys.executable,
                    "-c",
                    child_code,
                    str(grandchild_pid),
                ],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                timeout=10,
            )
            self.assertEqual(result.returncode, 124, result.stderr)
            self.assertIn("terminated its process group", result.stderr)
            self.assertIn("non-authoritative", result.stderr)
            self.assertTrue(grandchild_pid.is_file())
            descendant = int(grandchild_pid.read_text(encoding="utf-8"))
            for _ in range(40):
                try:
                    os.kill(descendant, 0)
                except ProcessLookupError:
                    break
                time.sleep(0.05)
            else:
                self.fail(f"timed-out process descendant survived: {descendant}")
            metrics = (root / "metrics.tsv").read_text(encoding="utf-8")
            self.assertEqual(len(metrics.splitlines()), 1)
            self.assertIn("wall_seconds=available", metrics)
            self.assertIn("peak_rss_bytes\tworking_set_peak\twaited_child_processes\tbytes", metrics)
            self.assertIn("peak_job_commit_bytes=unsupported", metrics)

    def test_process_output_limit_caps_artifacts_and_terminates_execution_unit(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            identity = contract.artifact_identity_token(contract.path_identity(root))
            child_code = (
                "import os; chunk=b'x'*4096; "
                "exec(compile('while True:\\n os.write(1,chunk)\\n os.write(2,chunk)',"
                "'<noisy-child>','exec'))"
            )
            result = subprocess.run(
                [
                    sys.executable,
                    str(Path(__file__).with_name("git-bench-process.py")),
                    "--artifact-root",
                    str(root),
                    "--artifact-root-identity",
                    identity,
                    "--stdout",
                    str(root / "stdout.log"),
                    "--stderr",
                    str(root / "stderr.log"),
                    "--metrics",
                    str(root / "metrics.tsv"),
                    "--max-output-bytes",
                    "4096",
                    "--timeout-seconds",
                    "5",
                    "--",
                    sys.executable,
                    "-c",
                    child_code,
                ],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                timeout=10,
            )
            self.assertEqual(result.returncode, 125, result.stderr)
            self.assertIn("bounded capture policy", result.stderr)
            sizes = [
                (root / "stdout.log").stat().st_size,
                (root / "stderr.log").stat().st_size,
            ]
            self.assertTrue(any(size == 4096 for size in sizes), sizes)
            self.assertTrue(all(0 <= size <= 4096 for size in sizes), sizes)
            self.assertEqual(len((root / "metrics.tsv").read_text().splitlines()), 1)

    def test_posix_success_rejects_and_reaps_lingering_descendant(self) -> None:
        if os.name == "nt":
            self.skipTest("POSIX process-group quiescence is required")
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            identity = contract.artifact_identity_token(contract.path_identity(root))
            marker = root / "descendant-survived.txt"
            descendant_code = (
                "import pathlib,sys,time; time.sleep(1); "
                "pathlib.Path(sys.argv[1]).write_text('survived')"
            )
            child_code = (
                "import subprocess,sys; "
                "subprocess.Popen([sys.executable,'-c',sys.argv[1],sys.argv[2]])"
            )
            result = subprocess.run(
                [
                    sys.executable,
                    str(Path(__file__).with_name("git-bench-process.py")),
                    "--artifact-root",
                    str(root),
                    "--artifact-root-identity",
                    identity,
                    "--stdout",
                    str(root / "stdout.log"),
                    "--stderr",
                    str(root / "stderr.log"),
                    "--metrics",
                    str(root / "metrics.tsv"),
                    "--timeout-seconds",
                    "5",
                    "--",
                    sys.executable,
                    "-c",
                    child_code,
                    descendant_code,
                    str(marker),
                ],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                timeout=10,
            )
            self.assertEqual(result.returncode, 126, result.stderr)
            self.assertIn("surviving process-group descendants", result.stderr)
            time.sleep(1.2)
            self.assertFalse(marker.exists())
            self.assertEqual(len((root / "metrics.tsv").read_text().splitlines()), 1)

    def test_authoritative_metadata_requires_validated_pinned_comparator(self) -> None:
        metadata = complete_standard_metadata()
        self.assertNotIn(
            "authoritative Git comparator",
            " ".join(contract.policy_reasons(metadata)),
        )
        metadata["git_comparator"]["status"] = "not-authoritative"
        self.assertIn(
            "provenance-validated",
            " ".join(contract.policy_reasons(metadata)),
        )
        metadata = complete_standard_metadata()
        metadata["git_comparator"]["commit"] = "wrong"
        self.assertIn("does not match the pinned contract", " ".join(contract.policy_reasons(metadata)))

    def test_standard_harness_rejects_rows_symlink_before_children(self) -> None:
        repo = Path(__file__).resolve().parents[1]
        zmin = repo / "target/release/zmin"
        if not zmin.is_file():
            self.skipTest("release zmin binary is required for the harness regression")
        with tempfile.TemporaryDirectory() as directory:
            retained = Path(directory) / "retained"
            retained.mkdir()
            sentinel = Path(directory) / "sentinel"
            sentinel.write_text("preserve\n", encoding="utf-8")
            try:
                (retained / "bench.tsv").symlink_to(sentinel)
            except OSError as error:
                self.skipTest(f"symlink fixture unavailable: {error}")
            environment = os.environ.copy()
            environment.update(
                {
                    "ZMIN_BIN": str(zmin),
                    "ZMIN_BENCH_EVIDENCE_MODE": "exploratory",
                    "ZMIN_BENCH_OPS": "fetch-noop",
                    "ZMIN_BENCH_OUT_DIR": str(retained),
                }
            )
            attempt = subprocess.run(
                ["bash", str(repo / "tools/git-performance-bench.sh")],
                cwd=repo,
                env=environment,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                timeout=10,
            )
            self.assertNotEqual(attempt.returncode, 0)
            self.assertIn("symlink", attempt.stderr)
            self.assertEqual(sentinel.read_text(encoding="utf-8"), "preserve\n")

    def test_authoritative_durability_fails_closed_when_unsupported(self) -> None:
        metadata = complete_metadata()
        metadata["mode"] = "authoritative"
        metadata.update(
            {
                "build_marker": contract.RELEASE_BUILD_MARKER,
                "results_dir": "/tmp/results",
                "results_dir_identity": {"platform": "posix", "st_dev": 1, "st_ino": 2},
                "durability": {
                    "supported": False,
                    "method": "windows-directory-fsync-not-implemented",
                    "platform": "nt",
                },
            }
        )
        self.assertIn("atomic evidence durability is unsupported", contract.policy_reasons(metadata))
        with tempfile.TemporaryDirectory() as directory:
            directory_path = Path(directory)
            with mock.patch.object(contract.os, "name", "nt"):
                capability = contract.durability_capability(directory_path)
                self.assertFalse(capability["supported"])
                self.assertIn("windows", capability["method"])
            with mock.patch.object(
                contract,
                "durability_capability",
                return_value={"supported": False, "method": "test-unsupported", "platform": "nt"},
            ):
                with self.assertRaisesRegex(contract.ContractError, "unsupported"):
                    contract.atomic_write_bytes(
                        Path(directory) / "evidence.json",
                        b"{}\n",
                        require_directory_fsync=True,
                    )

    def test_dirfd_publication_rejects_directory_replacement(self) -> None:
        if os.name == "nt":
            self.skipTest("dirfd publication requires POSIX")
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            pre_pin = root / "pre-pin"
            pre_pin.mkdir()
            pre_pin_identity = contract.path_identity(pre_pin)
            pre_pin.rename(root / "pre-pin-old")
            pre_pin.mkdir()
            with self.assertRaisesRegex(contract.ContractError, "changed while being pinned"):
                contract.atomic_write_bytes(
                    pre_pin / "evidence.json",
                    b"{}\n",
                    reject_symlinks=True,
                    require_directory_fsync=True,
                    expected_directory_identity=pre_pin_identity,
                )

            retained = root / "retained"
            retained.mkdir()
            destination = retained / "evidence.json"
            original_replace = contract.os.replace

            def replace_after_swap(source: object, target: object, **kwargs: object) -> None:
                backup = root / "retained-old"
                retained.rename(backup)
                retained.mkdir()
                original_replace(source, target, **kwargs)

            with mock.patch.object(contract.os, "replace", side_effect=replace_after_swap):
                with self.assertRaisesRegex(contract.ContractError, "replaced during publication"):
                    contract.atomic_write_bytes(
                        destination,
                        b"{}\n",
                        reject_symlinks=True,
                        require_directory_fsync=True,
                    )
            self.assertFalse(destination.exists())
            self.assertEqual((root / "retained-old" / "evidence.json").read_bytes(), b"{}\n")

    def test_authoritative_output_collisions_fail_before_input_validation(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            retained = root / "retained"
            retained.mkdir()
            metadata = retained / "metadata.json"
            rows = retained / "rows.tsv"
            raw = retained / "raw.tsv"
            metadata.write_text(
                json.dumps(
                    {
                        "results_dir": str(retained.resolve()),
                        "results_dir_identity": contract.path_identity(retained),
                        "durability": contract.durability_capability(retained),
                    }
                ),
                encoding="utf-8",
            )
            rows.write_text("not-a-valid-benchmark-row\n", encoding="utf-8")
            raw.write_text("raw\n", encoding="utf-8")
            with self.assertRaisesRegex(contract.ContractError, "collides with rows"):
                contract.finish_metadata(
                    argparse.Namespace(
                        metadata=str(metadata),
                        rows=str(rows),
                        output=str(rows),
                        result=[str(raw)],
                        results_dir=str(retained),
                        require_authoritative=True,
                    )
                )


if __name__ == "__main__":
    unittest.main()
