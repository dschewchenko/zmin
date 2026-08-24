#!/usr/bin/env python3
"""Aggregate three retained authoritative performance evidence bundles.

Each input root must contain exactly ``standard/`` and ``observed/`` bundle
directories.  The two bundle types retain the existing performance contract;
this tool revalidates their raw rows, equivalence manifests, and recomputed
superiority summaries before it can publish a universal claim.
"""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import pathlib
import sys
from typing import Any, Iterable, Mapping

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import performance_contract as contract


AGGREGATE_SCHEMA_VERSION = 1
PLATFORMS = ("Darwin", "Linux", "Windows")
MANIFESTS = ("standard", "observed")
CONTROL_FILES = {"evidence.json", "metadata.json"}

# Aggregation is an input parser, not a general-purpose artifact importer.
# Keep these limits comfortably above the authoritative seven/ten-lane output
# while bounding memory and parser work before any untrusted bytes are parsed.
MAX_BUNDLE_FILES = 32
MAX_FILE_BYTES = 32 * 1024 * 1024
MAX_BUNDLE_BYTES = 96 * 1024 * 1024
MAX_ROOT_BYTES = 192 * 1024 * 1024
MAX_TSV_ROWS = 100_000


class AggregateError(contract.ContractError):
    """An input bundle cannot support a universal performance claim."""


@dataclass(frozen=True)
class FileSnapshot:
    relative_path: str
    path: pathlib.Path
    data: bytes
    sha256: str
    signature: tuple[int, int, int, int]


@dataclass(frozen=True)
class DirectorySnapshot:
    path: pathlib.Path
    identity: dict[str, Any]
    signature: tuple[int, int, int, int]
    entries: tuple[str, ...]


@dataclass(frozen=True)
class RootSnapshot:
    root: DirectorySnapshot
    bundles: Mapping[str, DirectorySnapshot]
    files: Mapping[str, Mapping[str, FileSnapshot]]


@dataclass
class BundleReport:
    platform: str
    manifest: str
    root_manifest_sha256: str | None = None
    evidence_sha256: str | None = None
    rows_sha256: str | None = None
    equivalence_sha256: str | None = None
    superiority_sha256: str | None = None
    row_count: int = 0
    summary_row_count: int = 0
    verdict: str = "non-authoritative"
    identity: dict[str, Any] | None = None
    reasons: list[str] | None = None

    def __post_init__(self) -> None:
        if self.reasons is None:
            self.reasons = []


def _snapshot(path: pathlib.Path, relative_path: str) -> FileSnapshot:
    if path.is_symlink() or not path.is_file():
        raise AggregateError(f"bundle artifact is not a regular non-symlink file: {path}")
    before = contract.file_signature(path)
    if before[2] > MAX_FILE_BYTES:
        raise AggregateError(
            f"bundle artifact exceeds the {MAX_FILE_BYTES}-byte per-file limit: {path}"
        )
    data = path.read_bytes()
    after = contract.file_signature(path)
    if before != after:
        raise AggregateError(f"bundle artifact changed while being read: {path}")
    if len(data) != before[2]:
        raise AggregateError(f"bundle artifact length changed while being read: {path}")
    return FileSnapshot(
        relative_path=relative_path,
        path=path,
        data=data,
        sha256=contract.sha256_bytes(data),
        signature=after,
    )


def _directory_snapshot(path: pathlib.Path, label: str) -> DirectorySnapshot:
    if path.is_symlink() or not path.is_dir():
        raise AggregateError(f"{label} must be a direct non-symlink directory: {path}")
    entries = tuple(sorted(entry.name for entry in path.iterdir()))
    return DirectorySnapshot(
        path=path,
        identity=contract.path_identity(path),
        signature=contract.file_signature(path),
        entries=entries,
    )


def _bundle_inventory(
    bundle: DirectorySnapshot,
) -> tuple[tuple[pathlib.Path, ...], int]:
    if len(bundle.entries) > MAX_BUNDLE_FILES:
        raise AggregateError(
            f"{bundle.path} contains {len(bundle.entries)} files, exceeding the "
            f"{MAX_BUNDLE_FILES}-file bundle limit"
        )
    entries: list[pathlib.Path] = []
    total_bytes = 0
    for name in bundle.entries:
        entry = bundle.path / name
        if entry.is_symlink() or not entry.is_file():
            raise AggregateError(f"{bundle.path}/{name} must be a direct regular file")
        signature = contract.file_signature(entry)
        if signature[2] > MAX_FILE_BYTES:
            raise AggregateError(
                f"bundle artifact exceeds the {MAX_FILE_BYTES}-byte per-file limit: {entry}"
            )
        total_bytes += signature[2]
        if total_bytes > MAX_BUNDLE_BYTES:
            raise AggregateError(
                f"{bundle.path} exceeds the {MAX_BUNDLE_BYTES}-byte bundle limit"
            )
        entries.append(entry)
    return tuple(entries), total_bytes


def _bundle_files(
    root: pathlib.Path,
    manifest: str,
    inventory: tuple[pathlib.Path, ...],
) -> dict[str, FileSnapshot]:
    bundle = root / manifest
    if bundle.is_symlink() or not bundle.is_dir():
        raise AggregateError(f"missing {manifest} evidence bundle: {bundle}")
    snapshots: dict[str, FileSnapshot] = {}
    for entry in inventory:
        relative = f"{manifest}/{entry.name}"
        if entry.is_symlink() or not entry.is_file():
            raise AggregateError(f"{relative} must be a direct regular file")
        snapshots[entry.name] = _snapshot(entry, relative)
    if "evidence.json" not in snapshots:
        raise AggregateError(f"{manifest} bundle is missing evidence.json")
    if "equivalence.tsv" not in snapshots:
        raise AggregateError(f"{manifest} bundle is missing equivalence.tsv")
    if "superiority.tsv" not in snapshots:
        raise AggregateError(f"{manifest} bundle is missing superiority.tsv")
    return snapshots


def _root_manifest(
    root: pathlib.Path,
    files_by_manifest: Mapping[str, Mapping[str, FileSnapshot]],
) -> tuple[list[dict[str, Any]], str]:
    entries: list[dict[str, Any]] = []
    for manifest in MANIFESTS:
        for name, snapshot in sorted(files_by_manifest[manifest].items()):
            entries.append(
                {
                    "path": f"{manifest}/{name}",
                    "bytes": len(snapshot.data),
                    "sha256": snapshot.sha256,
                }
            )
    return entries, contract.sha256_bytes(contract.canonical_json(entries))


def _verify_raw_results(
    metadata: Mapping[str, Any],
    snapshots: Mapping[str, FileSnapshot],
    bundle: pathlib.Path,
) -> list[str]:
    reasons: list[str] = []
    raw = metadata.get("raw_results")
    if not isinstance(raw, list) or not raw or not all(isinstance(item, dict) for item in raw):
        return ["raw_results is missing or malformed"]
    if metadata.get("raw_results_sha256") != contract.sha256_bytes(
        contract.canonical_json(raw)
    ):
        reasons.append("raw_results_sha256 is not canonical")
    expected: dict[str, str] = {}
    for item in raw:
        path = item.get("path")
        digest = item.get("sha256")
        declared_bytes = item.get("bytes")
        if not isinstance(path, str) or not isinstance(digest, str):
            reasons.append("raw_results contains a malformed path or digest")
            continue
        if type(declared_bytes) is not int or declared_bytes < 0:
            reasons.append(f"raw_results has an invalid byte length for {path}")
            continue
        raw_path = pathlib.Path(path)
        name = pathlib.PurePath(path.replace("\\", "/")).name
        if not name or name in CONTROL_FILES or name in expected:
            reasons.append(f"raw_results has an invalid or duplicate basename: {name}")
            continue
        if not raw_path.is_absolute():
            reasons.append(f"raw_results path is not absolute: {path}")
            continue
        snapshot = snapshots.get(name)
        if (
            snapshot is None
            or ".." in raw_path.parts
            or raw_path.resolve(strict=False).parent != bundle.resolve(strict=False)
            or raw_path.resolve(strict=False) != snapshot.path.resolve(strict=False)
        ):
            reasons.append(f"raw_results path is not the retained direct file: {path}")
            continue
        if declared_bytes != len(snapshot.data):
            reasons.append(
                f"raw_results byte length does not match retained file: {path}"
            )
        expected[name] = digest
    actual = {
        name: snapshot.sha256
        for name, snapshot in snapshots.items()
        if name not in CONTROL_FILES
    }
    if expected != actual:
        reasons.append("retained raw result set does not match authenticated raw_results")
    return reasons


def _find_rows(
    manifest: str,
    snapshots: Mapping[str, FileSnapshot],
) -> tuple[str, list[dict[str, str]]]:
    expected_fields = (
        contract.STANDARD_RESULT_FIELDS
        if manifest == "standard"
        else contract.OBSERVED_RESULT_FIELDS
    )
    candidates: list[tuple[str, list[dict[str, str]]]] = []
    for name, snapshot in snapshots.items():
        if name in CONTROL_FILES:
            continue
        _ensure_tsv_row_cap(snapshot)
        try:
            rows = contract.read_strict_tsv_bytes(
                snapshot.data,
                snapshot.path,
                expected_fields=expected_fields,
            )
        except contract.ContractError:
            continue
        candidates.append((name, rows))
    if len(candidates) != 1:
        raise AggregateError(
            f"{manifest} bundle must contain exactly one raw result TSV with the canonical schema"
        )
    return candidates[0]


def _ensure_tsv_row_cap(snapshot: FileSnapshot) -> None:
    row_count = snapshot.data.count(b"\n")
    if snapshot.data and not snapshot.data.endswith(b"\n"):
        row_count += 1
    if row_count > MAX_TSV_ROWS:
        raise AggregateError(
            f"{snapshot.relative_path} contains {row_count} rows, exceeding the "
            f"{MAX_TSV_ROWS}-row limit"
        )


def _identity_projection(metadata: Mapping[str, Any]) -> dict[str, Any]:
    comparator = metadata.get("git_comparator")
    if isinstance(comparator, dict):
        comparator_identity = {
            key: comparator.get(key)
            for key in ("status", "tag", "commit", "archive_sha256")
        }
    else:
        comparator_identity = comparator
    harnesses = metadata.get("harnesses")
    if isinstance(harnesses, list):
        harness_identity = []
        for item in harnesses:
            if not isinstance(item, dict):
                harness_identity.append(item)
                continue
            path = item.get("path")
            name = (
                pathlib.PurePath(str(path).replace("\\", "/")).name
                if isinstance(path, str)
                else path
            )
            harness_identity.append({"name": name, "sha256": item.get("sha256")})
    else:
        harness_identity = harnesses
    return {
        "source": {
            "schema_version": metadata.get("schema_version"),
            "code_commit": metadata.get("code_commit"),
            "code_dirty": metadata.get("code_dirty"),
            "code_status_sha256": metadata.get("code_status_sha256"),
            "cargo_lock_sha256": metadata.get("cargo_lock_sha256"),
        },
        "contract": {
            "comparator": comparator_identity,
            "harnesses": harness_identity,
            "sample_phase_policy": metadata.get("sample_phase_policy"),
            "environment_policy": metadata.get("environment_policy"),
            "mandatory_manifest": metadata.get("mandatory_manifest"),
            "mandatory_lanes": metadata.get("mandatory_lanes"),
            "mandatory_manifest_sha256": metadata.get("mandatory_manifest_sha256"),
            "equivalence_plan_sha256": metadata.get("equivalence_plan_sha256"),
        },
        "fixture": {
            "fixture_sha256": metadata.get("fixture_sha256"),
            "git_config_sha256": metadata.get("git_config_sha256"),
        },
        "workload": {
            "command_corpus": metadata.get("command_corpus"),
            "command_corpus_sha256": metadata.get("command_corpus_sha256"),
            "policy": metadata.get("policy"),
        },
        "statistics": metadata.get("statistics_policy"),
        "memory": metadata.get("metrics", {}).get("memory_contracts")
        if isinstance(metadata.get("metrics"), dict)
        else None,
    }


def _validate_bundle(
    platform_name: str,
    root: pathlib.Path,
    manifest: str,
    files: Mapping[str, FileSnapshot],
    root_manifest_sha256: str,
) -> BundleReport:
    report = BundleReport(
        platform=platform_name,
        manifest=manifest,
        root_manifest_sha256=root_manifest_sha256,
        evidence_sha256=files["evidence.json"].sha256,
        equivalence_sha256=files["equivalence.tsv"].sha256,
        superiority_sha256=files["superiority.tsv"].sha256,
    )
    try:
        metadata = contract.load_json_bytes(files["evidence.json"].data, files["evidence.json"].path)
        if metadata.get("mode") != "authoritative":
            report.reasons.append("evidence mode is not authoritative")
        if metadata.get("claim_status") != "authoritative":
            report.reasons.append("evidence claim_status is not authoritative")
        if metadata.get("authoritative_reasons") not in ([], None):
            report.reasons.append("evidence contains authoritative rejection reasons")
        if metadata.get("statistics_verdict") != "pass":
            report.reasons.append("evidence statistics_verdict is not pass")
        if metadata.get("mandatory_manifest") != manifest:
            report.reasons.append("evidence mandatory manifest does not match bundle directory")
        host = metadata.get("host")
        if not isinstance(host, dict) or host.get("os") != platform_name:
            report.reasons.append(f"evidence host OS is not {platform_name}")
        expected_memory = contract.memory_contract_for_metadata(metadata)
        metrics = metadata.get("metrics")
        if expected_memory is None or not isinstance(metrics, dict):
            report.reasons.append("evidence memory metric contract is missing")
        elif metrics.get("memory_contracts") != contract.MEMORY_CONTRACTS:
            report.reasons.append("evidence memory metric contract is not canonical")
        report.reasons.extend(contract.policy_reasons(metadata))
        report.reasons.extend(_verify_raw_results(metadata, files, root / manifest))
        rows_name, rows = _find_rows(manifest, files)
        report.rows_sha256 = files[rows_name].sha256
        _ensure_tsv_row_cap(files["equivalence.tsv"])
        _ensure_tsv_row_cap(files["superiority.tsv"])
        equivalence = contract.read_equivalence_bytes(
            files["equivalence.tsv"].data,
            files["equivalence.tsv"].path,
        )
        superiority = contract.read_superiority_summary_bytes(
            files["superiority.tsv"].data,
            files["superiority.tsv"].path,
        )
        report.row_count = len(rows)
        report.summary_row_count = len(superiority)
        report.reasons.extend(contract.validate_rows(rows, metadata, equivalence))
        report.reasons.extend(contract.validate_equivalence(equivalence, metadata))
        report.reasons.extend(
            contract.validate_equivalence_against_rows(equivalence, rows, metadata)
        )
        expected_summary, recompute_reasons = contract.compute_superiority_summary(rows, metadata)
        report.reasons.extend(recompute_reasons)
        report.reasons.extend(
            contract.validate_superiority_summary_artifact(
                files["superiority.tsv"].data,
                files["superiority.tsv"].path,
                superiority,
                expected_summary,
            )
        )
        expected_count = len(contract.MANDATORY_LANE_MANIFESTS[manifest]) * 2
        if len(expected_summary) != expected_count:
            report.reasons.append(
                f"recomputed {manifest} superiority row count is {len(expected_summary)}, expected {expected_count}"
            )
        if any(row.get("verdict") != "pass" for row in expected_summary):
            report.reasons.append("recomputed superiority contains a non-pass verdict")
        report.identity = _identity_projection(metadata)
        report.verdict = "pass" if not report.reasons else "non-authoritative"
    except (KeyError, TypeError, contract.ContractError) as error:
        report.reasons.append(str(error))
        report.verdict = "non-authoritative"
    return report


def _recheck_directory(snapshot: DirectorySnapshot, label: str) -> None:
    current = _directory_snapshot(snapshot.path, label)
    if (
        current.identity != snapshot.identity
        or current.signature != snapshot.signature
        or current.entries != snapshot.entries
    ):
        raise AggregateError(f"{label} identity or exact entry set changed during aggregation")


def _recheck_root_snapshot(snapshot: RootSnapshot) -> None:
    _recheck_directory(snapshot.root, "bundle root")
    for manifest, bundle in snapshot.bundles.items():
        _recheck_directory(bundle, f"{manifest} bundle")
        for name, file_snapshot in snapshot.files[manifest].items():
            path = file_snapshot.path
            if path.is_symlink() or not path.is_file():
                raise AggregateError(f"{manifest}/{name} was replaced by a non-regular file")
            if (
                contract.file_signature(path) != file_snapshot.signature
                or contract.sha256_file(path) != file_snapshot.sha256
            ):
                raise AggregateError(f"{manifest}/{name} changed during aggregation")


def _load_root(
    platform_name: str,
    root_value: str,
) -> tuple[pathlib.Path, dict[str, BundleReport], RootSnapshot]:
    root = contract.absolute_directory(root_value, reject_symlinks=True)
    root_snapshot = _directory_snapshot(root, f"{platform_name} bundle root")
    if root_snapshot.entries != tuple(sorted(MANIFESTS)):
        raise AggregateError(
            f"{platform_name} root must contain exactly standard/ and observed/ directories"
        )
    bundle_snapshots = {
        manifest: _directory_snapshot(root / manifest, f"{platform_name}/{manifest}")
        for manifest in MANIFESTS
    }
    inventories = {
        manifest: _bundle_inventory(bundle_snapshots[manifest]) for manifest in MANIFESTS
    }
    root_bytes = sum(total for _, total in inventories.values())
    if root_bytes > MAX_ROOT_BYTES:
        raise AggregateError(
            f"{platform_name} root exceeds the {MAX_ROOT_BYTES}-byte total limit"
        )
    files_by_manifest = {
        manifest: _bundle_files(root, manifest, inventories[manifest][0])
        for manifest in MANIFESTS
    }
    _entries, root_manifest_sha256 = _root_manifest(root, files_by_manifest)
    reports = {
        manifest: _validate_bundle(
            platform_name,
            root,
            manifest,
            files_by_manifest[manifest],
            root_manifest_sha256,
        )
        for manifest in MANIFESTS
    }
    snapshot = RootSnapshot(root_snapshot, bundle_snapshots, files_by_manifest)
    try:
        _recheck_root_snapshot(snapshot)
    except AggregateError as error:
        for report in reports.values():
            report.reasons.append(str(error))
            report.verdict = "non-authoritative"
    return root, reports, snapshot


def _identity_digest(identity: dict[str, Any] | None) -> str | None:
    if identity is None:
        return None
    return contract.sha256_bytes(contract.canonical_json(identity))


def _aggregate_roots_with_snapshots(
    roots: Mapping[str, str],
) -> tuple[dict[str, Any], dict[str, RootSnapshot]]:
    if tuple(sorted(roots)) != tuple(sorted(PLATFORMS)):
        raise AggregateError("exactly Darwin, Linux, and Windows bundle roots are required")
    resolved: dict[str, pathlib.Path] = {}
    reports: dict[str, dict[str, BundleReport]] = {}
    snapshots: dict[str, RootSnapshot] = {}
    reasons: list[str] = []
    for platform_name in PLATFORMS:
        try:
            root, platform_reports, root_snapshot = _load_root(platform_name, roots[platform_name])
            resolved[platform_name] = root
            reports[platform_name] = platform_reports
            snapshots[platform_name] = root_snapshot
        except (OSError, contract.ContractError) as error:
            reasons.append(f"{platform_name}: {error}")
    if len({str(path) for path in resolved.values()}) != len(resolved):
        reasons.append("platform bundle roots must be distinct")

    identities: dict[str, dict[str, Any]] = {}
    for manifest in MANIFESTS:
        manifest_reports: list[BundleReport] = []
        for platform_name in PLATFORMS:
            report = reports.get(platform_name, {}).get(manifest)
            if report is None or report.identity is None:
                continue
            identities[f"{platform_name}/{manifest}"] = report.identity
            manifest_reports.append(report)
        values = list(identities[key] for key in sorted(identities) if key.endswith(f"/{manifest}"))
        if values and any(value != values[0] for value in values[1:]):
            identity_reason = f"{manifest} source/contract/fixture/workload/statistics identities differ across platforms"
            reasons.append(identity_reason)
            for report in manifest_reports:
                report.reasons.append(identity_reason)
                report.verdict = "non-authoritative"

    bundle_records: list[dict[str, Any]] = []
    all_pass = not reasons
    for platform_name, snapshot in snapshots.items():
        try:
            _recheck_root_snapshot(snapshot)
        except AggregateError as error:
            reasons.append(f"{platform_name}: {error}")
            for report in reports[platform_name].values():
                report.reasons.append(str(error))
                report.verdict = "non-authoritative"
    for platform_name in PLATFORMS:
        platform_reports = reports.get(platform_name, {})
        for manifest in MANIFESTS:
            report = platform_reports.get(manifest)
            if report is None:
                all_pass = False
                bundle_records.append(
                    {"platform": platform_name, "manifest": manifest, "verdict": "non-authoritative", "reasons": ["bundle was not loaded"]}
                )
                continue
            if report.reasons:
                all_pass = False
            bundle_records.append(
                {
                    "platform": platform_name,
                    "manifest": manifest,
                    "root_manifest_sha256": report.root_manifest_sha256,
                    "evidence_sha256": report.evidence_sha256,
                    "rows_sha256": report.rows_sha256,
                    "equivalence_sha256": report.equivalence_sha256,
                    "superiority_sha256": report.superiority_sha256,
                    "row_count": report.row_count,
                    "summary_row_count": report.summary_row_count,
                    "identity_sha256": _identity_digest(report.identity),
                    "verdict": report.verdict,
                    "reasons": sorted(set(report.reasons)),
                }
            )
    all_pass = all_pass and all(record["verdict"] == "pass" for record in bundle_records)
    for record in bundle_records:
        for reason in record["reasons"]:
            reasons.append(f"{record['platform']}/{record['manifest']}: {reason}")
    reasons = sorted(set(reasons))
    payload = {
        "schema_version": AGGREGATE_SCHEMA_VERSION,
        "evidence_scope": "cross-platform-performance",
        "platforms": list(PLATFORMS),
        "manifests": {
            "standard": list(contract.STANDARD_MANDATORY_LANES),
            "observed": list(contract.OBSERVED_MANDATORY_LANES),
        },
        "sample_classes": ["measured", "cold"],
        "claim_status": "authoritative" if all_pass else "non-authoritative",
        "universal_claim": "pass" if all_pass else "not-established",
        "pooling": "none",
        "bundles": bundle_records,
        "reasons": reasons,
    }
    payload["manifest_sha256"] = contract.sha256_bytes(contract.canonical_json(payload))
    return payload, snapshots


def aggregate_roots(roots: Mapping[str, str]) -> dict[str, Any]:
    payload, _snapshots = _aggregate_roots_with_snapshots(roots)
    return payload


def _recheck_all_snapshots(snapshots: Mapping[str, RootSnapshot]) -> None:
    for platform_name, snapshot in snapshots.items():
        try:
            _recheck_root_snapshot(snapshot)
        except AggregateError as error:
            raise AggregateError(f"{platform_name}: {error}") from error


def parser() -> argparse.ArgumentParser:
    command = argparse.ArgumentParser(description=__doc__)
    subparsers = command.add_subparsers(dest="command", required=True)
    aggregate = subparsers.add_parser("aggregate")
    aggregate.add_argument("--darwin", required=True, metavar="ROOT")
    aggregate.add_argument("--linux", required=True, metavar="ROOT")
    aggregate.add_argument("--windows", required=True, metavar="ROOT")
    aggregate.add_argument("--output", required=True, metavar="PATH")
    return command


def main(argv: Iterable[str] | None = None) -> int:
    args = parser().parse_args(list(argv) if argv is not None else None)
    if args.command != "aggregate":
        raise AggregateError("aggregate command is required")
    output = contract.absolute_output_path(args.output, reject_symlinks=True)
    roots = {"Darwin": args.darwin, "Linux": args.linux, "Windows": args.windows}
    for root in roots.values():
        candidate = pathlib.Path(root).expanduser().resolve(strict=False)
        if output == candidate or contract.path_is_within(output, candidate):
            raise AggregateError("aggregate output must not be inside an input bundle root")
    payload, snapshots = _aggregate_roots_with_snapshots(roots)
    contract.atomic_write_bytes(
        output,
        contract.canonical_json(payload) + b"\n",
        reject_symlinks=True,
        prepublication_check=lambda: _recheck_all_snapshots(snapshots),
    )
    if payload["claim_status"] != "authoritative":
        print("cross-platform performance claim is non-authoritative: " + "; ".join(payload["reasons"]), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (AggregateError, OSError) as error:
        print(f"performance evidence aggregation failed: {error}", file=sys.stderr)
        raise SystemExit(2)
