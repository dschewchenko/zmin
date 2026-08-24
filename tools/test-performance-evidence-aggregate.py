#!/usr/bin/env python3
"""Focused fail-closed tests for the cross-platform evidence aggregator."""

from __future__ import annotations

import csv
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


TOOLS = Path(__file__).resolve().parent


def load_module(name: str, path: Path):
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


contract = load_module("performance_contract_aggregate_test", TOOLS / "performance_contract.py")
fixtures = load_module("performance_contract_fixture_test", TOOLS / "test-performance-contract.py")
aggregate = load_module("performance_evidence_aggregate_test", TOOLS / "performance-evidence-aggregate.py")

# The production contract retains 20,000 bootstrap resamples.  These synthetic
# tests exercise bundle authentication and cross-platform gating, not the
# already-covered statistics implementation; use a smaller authenticated test
# policy so six complete bundles stay fast and deterministic.
for _module in (contract, aggregate.contract):
    _module.STATISTICS_BOOTSTRAP_RESAMPLES = 200
    _module.STATISTICS_POLICY["bootstrap_resamples"] = 200


def write_tsv(path: Path, fields: tuple[str, ...], rows: list[dict[str, str]]) -> None:
    with path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(
            handle,
            fieldnames=list(fields),
            delimiter="\t",
            lineterminator="\n",
            extrasaction="raise",
        )
        writer.writeheader()
        writer.writerows({field: row.get(field, "") for field in fields} for row in rows)


def standard_rows() -> list[dict[str, str]]:
    rows = []
    for row in fixtures.rows_for_manifest(list(contract.STANDARD_MANDATORY_LANES)):
        row = dict(row)
        if row["tool"] == "git":
            row["real"] = "1.000000"
            row["rss_bytes"] = "4096"
        else:
            row["real"] = "0.500000"
            row["rss_bytes"] = "2048"
        rows.append(
            {
                **row,
                "op": row["lane"],
                "extra": "",
            }
        )
        rows[-1].pop("lane")
    return rows


def observed_rows() -> list[dict[str, str]]:
    per_lane = {
        lane: fixtures.observed_rows_for_lane(lane)
        for lane in contract.OBSERVED_MANDATORY_LANES
    }
    rows: list[dict[str, str]] = []
    for kind, count in (("warmup", 3), ("measured", 30), ("cold", 10)):
        for index in range(1, count + 1):
            for lane in contract.OBSERVED_MANDATORY_LANES:
                pair_id = f"{lane}-{kind}-{index}"
                for row in per_lane[lane]:
                    if row["pair_id"] != pair_id:
                        continue
                    row = dict(row)
                    if row["tool"] == "stock":
                        row["real_seconds"] = "1.000000"
                        row["max_rss_bytes"] = "4096"
                    else:
                        row["real_seconds"] = "0.500000"
                        row["max_rss_bytes"] = "2048"
                    rows.append(row)
    return rows


def complete_metadata(manifest: str, platform: str, bundle: Path, fixture_root: Path, template: Path) -> dict:
    metadata = (
        fixtures.complete_standard_metadata()
        if manifest == "standard"
        else fixtures.complete_observed_metadata()
    )
    metadata.update(
        {
            "binary_path_profile": "release",
            "build_marker": contract.RELEASE_BUILD_MARKER,
            "results_dir": str(bundle),
            "results_dir_identity": contract.path_identity(bundle),
            "durability": contract.durability_capability(bundle),
            "fixture_root": str(fixture_root),
            "host": {"os": platform},
            "code_commit": "synthetic-commit",
            "cargo_lock_sha256": "synthetic-lock",
            "code_status_sha256": "synthetic-status",
            "statistics_verdict": "pass",
            "claim_status": "authoritative",
            "authoritative_reasons": [],
            "metrics": {
                "required": list(contract.REQUIRED_METRICS) + list(contract.MEMORY_METRICS),
                "optional": list(contract.OPTIONAL_METRICS),
                "memory_contracts": contract.MEMORY_CONTRACTS,
                "unsupported_value": "unsupported",
            },
        }
    )
    if manifest == "standard":
        binding = contract.benchmark_template_binding(fixture_root, template)
        metadata["benchmark_init_template"] = binding
        metadata["environment"]["values"]["GIT_TEMPLATE_DIR"] = binding["path"]
    return metadata


def refresh_raw_results(bundle: Path) -> None:
    evidence = bundle / "evidence.json"
    metadata = json.loads(evidence.read_text(encoding="utf-8"))
    entries = []
    for path in sorted(bundle.iterdir(), key=lambda item: item.name):
        if path.name in {"evidence.json", "metadata.json"}:
            continue
        data = path.read_bytes()
        entries.append(
            {"path": str(path), "bytes": len(data), "sha256": contract.sha256_bytes(data)}
        )
    metadata["raw_results"] = entries
    metadata["raw_results_sha256"] = contract.sha256_bytes(contract.canonical_json(entries))
    evidence.write_bytes(contract.canonical_json(metadata) + b"\n")


def build_root(root: Path, platform: str, fixture_root: Path, template: Path) -> None:
    root.mkdir()
    for manifest in aggregate.MANIFESTS:
        bundle = root / manifest
        bundle.mkdir()
        metadata = complete_metadata(manifest, platform, bundle, fixture_root, template)
        rows = standard_rows() if manifest == "standard" else observed_rows()
        if platform == "Windows":
            for row in rows:
                row["rss_bytes" if manifest == "standard" else "max_rss_bytes"] = "unsupported"
                row["job_commit_bytes"] = "4096" if row["tool"] in {"git", "stock"} else "2048"
                row["memory_metric"] = "peak_job_commit_bytes"
                row["memory_semantics"] = "job_commit_peak"
                row["memory_scope"] = "job_process_tree"
                row["metrics_availability"] = row["metrics_availability"].replace(
                    "peak_rss_bytes=available;peak_job_commit_bytes=unsupported",
                    "peak_rss_bytes=unsupported;peak_job_commit_bytes=available",
                )
        fields = contract.STANDARD_RESULT_FIELDS if manifest == "standard" else contract.OBSERVED_RESULT_FIELDS
        write_tsv(bundle / ("bench.tsv" if manifest == "standard" else "observed.tsv"), fields, rows)
        lanes = list(contract.MANDATORY_LANE_MANIFESTS[manifest])
        write_tsv(bundle / "equivalence.tsv", contract.EQUIVALENCE_MANIFEST_FIELDS, fixtures.equivalence_rows_for_manifest(lanes))
        summary, reasons = contract.compute_superiority_summary(rows, metadata)
        if reasons:
            raise AssertionError(f"synthetic {manifest} summary is invalid: {reasons[:3]}")
        (bundle / "superiority.tsv").write_bytes(contract.serialize_superiority_summary(summary))
        metadata["raw_results"] = []
        metadata["raw_results_sha256"] = contract.sha256_bytes(contract.canonical_json([]))
        (bundle / "evidence.json").write_bytes(contract.canonical_json(metadata) + b"\n")
        refresh_raw_results(bundle)


class PerformanceEvidenceAggregateTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.fixture_root = self.root / "fixture"
        self.fixture_root.mkdir()
        self.template = self.fixture_root / "template"
        self.template.mkdir()
        self.roots: dict[str, str] = {}
        for platform in aggregate.PLATFORMS:
            path = self.root / platform.lower()
            build_root(path, platform, self.fixture_root, self.template)
            self.roots[platform] = str(path)

    def tearDown(self) -> None:
        self.temp.cleanup()

    def test_all_platforms_pass_without_pooling(self) -> None:
        payload = aggregate.aggregate_roots(self.roots)
        self.assertEqual(payload["claim_status"], "authoritative")
        self.assertEqual(payload["universal_claim"], "pass")
        self.assertEqual(payload["pooling"], "none")
        self.assertEqual(len(payload["bundles"]), 6)
        self.assertEqual(payload["reasons"], [])
        digest = payload.pop("manifest_sha256")
        self.assertEqual(digest, contract.sha256_bytes(contract.canonical_json(payload)))

    def test_missing_os_is_rejected_by_cli(self) -> None:
        output = self.root / "aggregate.json"
        command = [
            sys.executable,
            "-B",
            str(TOOLS / "performance-evidence-aggregate.py"),
            "aggregate",
            "--darwin",
            self.roots["Darwin"],
            "--linux",
            self.roots["Linux"],
            "--output",
            str(output),
        ]
        completed = subprocess.run(command, capture_output=True, text=True)
        self.assertNotEqual(completed.returncode, 0)
        self.assertFalse(output.exists())

    def test_duplicate_os_or_wrong_host_is_not_universal(self) -> None:
        payload = aggregate.aggregate_roots(
            {**self.roots, "Windows": self.roots["Linux"]}
        )
        self.assertEqual(payload["claim_status"], "non-authoritative")
        self.assertTrue(any("roots must be distinct" in reason for reason in payload["reasons"]))

    def test_identity_mismatch_is_not_universal(self) -> None:
        evidence = Path(self.roots["Linux"]) / "standard" / "evidence.json"
        metadata = json.loads(evidence.read_text(encoding="utf-8"))
        metadata["fixture_sha256"] = "different-fixture"
        evidence.write_bytes(contract.canonical_json(metadata) + b"\n")
        payload = aggregate.aggregate_roots(self.roots)
        self.assertEqual(payload["universal_claim"], "not-established")
        self.assertTrue(any("identities differ" in reason for reason in payload["reasons"]))

    def test_forged_summary_is_not_trusted(self) -> None:
        bundle = Path(self.roots["Darwin"]) / "observed"
        summary = bundle / "superiority.tsv"
        lines = summary.read_text(encoding="utf-8").splitlines()
        cells = lines[1].split("\t")
        cells[4] = "999999999"
        lines[1] = "\t".join(cells)
        summary.write_text("\n".join(lines) + "\n", encoding="utf-8")
        refresh_raw_results(bundle)
        payload = aggregate.aggregate_roots(self.roots)
        self.assertEqual(payload["claim_status"], "non-authoritative")
        self.assertTrue(any("superiority summary" in reason for reason in payload["reasons"]))

    def test_inconclusive_bundle_cannot_make_universal_claim(self) -> None:
        evidence = Path(self.roots["Windows"]) / "observed" / "evidence.json"
        metadata = json.loads(evidence.read_text(encoding="utf-8"))
        metadata["statistics_verdict"] = "inconclusive"
        evidence.write_bytes(contract.canonical_json(metadata) + b"\n")
        payload = aggregate.aggregate_roots(self.roots)
        self.assertEqual(payload["claim_status"], "non-authoritative")
        self.assertTrue(any("statistics_verdict is not pass" in reason for reason in payload["reasons"]))

    def test_raw_manifest_byte_length_is_authenticated(self) -> None:
        evidence = Path(self.roots["Darwin"]) / "standard" / "evidence.json"
        metadata = json.loads(evidence.read_text(encoding="utf-8"))
        metadata["raw_results"][0]["bytes"] += 1
        metadata["raw_results_sha256"] = contract.sha256_bytes(
            contract.canonical_json(metadata["raw_results"])
        )
        evidence.write_bytes(contract.canonical_json(metadata) + b"\n")
        payload = aggregate.aggregate_roots(self.roots)
        self.assertEqual(payload["claim_status"], "non-authoritative")
        self.assertTrue(any("byte length does not match" in reason for reason in payload["reasons"]))

    def test_input_caps_fail_closed_before_parsing(self) -> None:
        original_file_limit = aggregate.MAX_FILE_BYTES
        original_row_limit = aggregate.MAX_TSV_ROWS
        try:
            aggregate.MAX_FILE_BYTES = 128
            payload = aggregate.aggregate_roots(self.roots)
            self.assertEqual(payload["claim_status"], "non-authoritative")
            self.assertTrue(any("per-file limit" in reason for reason in payload["reasons"]))

            aggregate.MAX_FILE_BYTES = original_file_limit
            aggregate.MAX_TSV_ROWS = 4
            payload = aggregate.aggregate_roots(self.roots)
            self.assertEqual(payload["claim_status"], "non-authoritative")
            self.assertTrue(any("row limit" in reason for reason in payload["reasons"]))
        finally:
            aggregate.MAX_FILE_BYTES = original_file_limit
            aggregate.MAX_TSV_ROWS = original_row_limit

    def test_file_count_and_total_byte_caps_fail_closed(self) -> None:
        standard = Path(self.roots["Darwin"]) / "standard"
        for index in range(aggregate.MAX_BUNDLE_FILES):
            (standard / f"extra-{index}.txt").write_bytes(b"x")
        payload = aggregate.aggregate_roots(self.roots)
        self.assertEqual(payload["claim_status"], "non-authoritative")
        self.assertTrue(any("files, exceeding" in reason for reason in payload["reasons"]))

        for path in standard.glob("extra-*.txt"):
            path.unlink()
        original_bundle_limit = aggregate.MAX_BUNDLE_BYTES
        try:
            aggregate.MAX_BUNDLE_BYTES = 1
            payload = aggregate.aggregate_roots(self.roots)
            self.assertEqual(payload["claim_status"], "non-authoritative")
            self.assertTrue(any("bundle limit" in reason for reason in payload["reasons"]))
        finally:
            aggregate.MAX_BUNDLE_BYTES = original_bundle_limit

    def test_root_total_cap_fails_closed(self) -> None:
        original_root_limit = aggregate.MAX_ROOT_BYTES
        try:
            aggregate.MAX_ROOT_BYTES = 1
            payload = aggregate.aggregate_roots(self.roots)
            self.assertEqual(payload["claim_status"], "non-authoritative")
            self.assertTrue(any("total limit" in reason for reason in payload["reasons"]))
        finally:
            aggregate.MAX_ROOT_BYTES = original_root_limit

    def test_late_entry_and_replacement_are_detected_by_pinned_snapshot(self) -> None:
        _, _, snapshot = aggregate._load_root("Darwin", self.roots["Darwin"])
        late = Path(self.roots["Darwin"]) / "standard" / "late.tsv"
        late.write_bytes(b"unexpected\n")
        with self.assertRaises(aggregate.AggregateError):
            aggregate._recheck_root_snapshot(snapshot)
        late.unlink()

        target = Path(self.roots["Darwin"]) / "standard" / "equivalence.tsv"
        original = target.read_bytes()
        target.unlink()
        with self.assertRaises(aggregate.AggregateError):
            aggregate._recheck_root_snapshot(snapshot)
        target.write_bytes(original)
        replacement = target.with_name("equivalence.replacement.tsv")
        replacement.write_bytes(target.read_bytes())
        os.replace(replacement, target)
        with self.assertRaises(aggregate.AggregateError):
            aggregate._recheck_root_snapshot(snapshot)

    def test_cli_rechecks_all_inputs_at_atomic_publication_boundary(self) -> None:
        output = self.root / "aggregate.json"
        target = Path(self.roots["Darwin"]) / "standard" / "equivalence.tsv"
        original_atomic_write = aggregate.contract.atomic_write_bytes

        def replace_then_publish(path, data, **kwargs):
            replacement = target.with_name("equivalence.late.tsv")
            replacement.write_bytes(target.read_bytes())
            os.replace(replacement, target)
            return original_atomic_write(path, data, **kwargs)

        with mock.patch.object(
            aggregate.contract,
            "atomic_write_bytes",
            side_effect=replace_then_publish,
        ):
            with self.assertRaisesRegex(aggregate.AggregateError, "changed during aggregation"):
                aggregate.main(
                    [
                        "aggregate",
                        "--darwin",
                        self.roots["Darwin"],
                        "--linux",
                        self.roots["Linux"],
                        "--windows",
                        self.roots["Windows"],
                        "--output",
                        str(output),
                    ]
                )
        self.assertFalse(output.exists())

    def test_symlinked_direct_artifact_is_rejected(self) -> None:
        if not hasattr(os, "symlink"):
            self.skipTest("symlinks unavailable")
        outside = self.root / "outside.tsv"
        outside.write_bytes(b"outside\n")
        link = Path(self.roots["Linux"]) / "observed" / "escaped.tsv"
        try:
            os.symlink(outside, link)
            payload = aggregate.aggregate_roots(self.roots)
            self.assertEqual(payload["claim_status"], "non-authoritative")
            self.assertTrue(any("regular file" in reason for reason in payload["reasons"]))
        finally:
            if link.is_symlink() or link.exists():
                link.unlink()

    def test_raw_manifest_traversal_is_rejected(self) -> None:
        evidence = Path(self.roots["Windows"]) / "observed" / "evidence.json"
        bundle = evidence.parent
        metadata = json.loads(evidence.read_text(encoding="utf-8"))
        metadata["raw_results"][0]["path"] = str(bundle / ".." / "outside.tsv")
        metadata["raw_results_sha256"] = contract.sha256_bytes(
            contract.canonical_json(metadata["raw_results"])
        )
        evidence.write_bytes(contract.canonical_json(metadata) + b"\n")
        payload = aggregate.aggregate_roots(self.roots)
        self.assertEqual(payload["claim_status"], "non-authoritative")
        self.assertTrue(any("retained direct file" in reason for reason in payload["reasons"]))


if __name__ == "__main__":
    unittest.main()
