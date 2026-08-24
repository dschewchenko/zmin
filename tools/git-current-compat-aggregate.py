#!/usr/bin/env python3
"""Fail-closed aggregation of independently produced compatibility profiles."""

from __future__ import annotations

import argparse
import csv
import hashlib
import json
import os
import re
import stat
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path


EXPECTED_UPSTREAM = {
    "tag": "v2.55.0",
    "archive_url": "https://github.com/git/git/archive/refs/tags/v2.55.0.tar.gz",
    "archive_sha256": "72923418db7b26dfddc21e2268660c5118e560bdfaa09b4489b67b38e9b69c49",
    "tag_object": "5ce91c059e41090e7d2cffad39c04af8acf98dc1",
    "commit": "e9019fcafe0040228b8631c30f97ae1adb61bcdc",
}
EXPECTED_MANIFEST = {"mode": "all-nondeprecated", "top_level_count": 1046, "selected_count": 1045, "sole_exclusion": "t5323-pack-redundant.sh"}
EXPECTED_MANIFEST_HASHES = {
    "top_level_names_sha256": "fcaaaf275db6a4e0b86ac623549371b2cd7438eef92ba677791f55d37c9f7c90",
    "selected_names_sha256": "b49ad3a4b94a93d77109da4060671ee5ba0fa92fd1b45885023bd5b358ed34a6",
    "generated_tsv_sha256": "ba7f8c03b683eb984970c103f791e65a77c5664101ba8ca61d3c5d80ab4bafcb",
    "names_encoding": "LC_ALL=C sorted basenames, one LF-terminated name per line",
}
EXPECTED_POLICY = (
    ("t0029-core-unsetenvvars.sh", "platform", "windows-x86_64", "skipping Windows-specific tests"),
    ("t0051-windows-named-pipe.sh", "platform", "windows-x86_64", "skipping Windows-specific tests"),
    ("t1509-root-work-tree.sh", "dedicated-host", "linux-privileged-root", "Test requiring writable / skipped. Read this test if you want to run it"),
    ("t3910-mac-os-precompose.sh", "platform", "macos-native", "filesystem does not corrupt utf-8"),
    ("t5580-unc-paths.sh", "platform", "windows-x86_64", "skipping Windows-only path tests"),
    ("t5608-clone-2gb.sh", "dedicated-host", "linux-high-disk", "expensive 2GB clone test; enable with GIT_TEST_CLONE_2GB=true"),
    ("t6419-merge-ignorecase.sh", "platform", "linux-case-insensitive-fs", "skipping case insensitive tests - case sensitive file system"),
)
BASE_PROFILE = "linux-ubuntu-24.04-x86_64"
OPTIONAL_HEADER = ["lane", "test", "classification", "required_run_profile", "reason", "log_sha256"]
SUMMARY_HEADER = ["mode", "test", "status", "reason", "log"]
REQUIRED_METADATA = {
    "workflow_commit", "scope", "profile", "expected_tests", "jobs", "per_test_timeout_seconds",
    "classification", "authority_rule", "upstream_tag", "upstream_commit", "upstream_tag_object",
    "upstream_archive_sha256", "no_retries_or_rerolls", "repository_writes", "secrets", "runner",
    "contract_sha256", "manifest_sha256",
}
HEX64 = re.compile(r"^[0-9a-f]{64}$")
HEX40 = re.compile(r"^[0-9a-f]{40}$")
TAP_SKIP = re.compile(r"^[ \t]*1[ \t]*\.\.[ \t]*0[ \t]*#[ \t]*[Ss][Kk][Ii][Pp](?:[ \t]+(.*?))?[ \t]*$", re.MULTILINE)


@dataclass(frozen=True)
class PolicyEntry:
    test: str
    classification: str
    required_run_profile: str
    reason: str


@dataclass(frozen=True)
class Contract:
    sha256: str
    generated_tsv_sha256: str
    selected_count: int
    policy: tuple[PolicyEntry, ...]


@dataclass(frozen=True)
class ProfileArtifact:
    name: str
    root: Path
    tests: frozenset[str]
    coverage: frozenset[tuple[str, str]]
    skipped: tuple[tuple[str, str], ...]
    workflow_commit: str


def fail(message: str) -> None:
    raise ValueError(message)


def regular_no_symlink(path: Path, label: str) -> os.stat_result:
    try:
        value = os.lstat(path)
    except OSError as error:
        fail(f"missing {label}: {path}: {error}")
    if stat.S_ISLNK(value.st_mode) or not stat.S_ISREG(value.st_mode):
        fail(f"{label} is not a regular non-symlink file: {path}")
    return value


def secure_path(path: Path, label: str, directory: bool = False) -> Path:
    if not path.is_absolute():
        fail(f"{label} must be absolute")
    current = Path(path.anchor)
    for part in path.parts[1:]:
        current /= part
        try:
            value = os.lstat(current)
        except OSError as error:
            fail(f"missing {label} component {current}: {error}")
        if stat.S_ISLNK(value.st_mode):
            fail(f"{label} has symlink component: {current}")
    value = os.lstat(path)
    if directory and not stat.S_ISDIR(value.st_mode):
        fail(f"{label} is not a directory: {path}")
    return path


def unique_pairs(pairs: list[tuple[str, object]]) -> dict[str, object]:
    result: dict[str, object] = {}
    for key, value in pairs:
        if key in result:
            fail(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def load_contract(path: Path) -> Contract:
    secure_path(path, "contract")
    raw = path.read_bytes()
    try:
        value = json.loads(raw.decode("utf-8"), object_pairs_hook=unique_pairs)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        fail(f"invalid contract JSON: {error}")
    if set(value) != {"contract_version", "upstream", "manifest", "skip_policy"} or value["contract_version"] != 2:
        fail("contract root schema/version is not exact")
    if value["upstream"] != EXPECTED_UPSTREAM:
        fail("contract upstream binding is not exact")
    manifest = value["manifest"]
    required_manifest = {"mode", "top_level_count", "selected_count", "sole_exclusion", "top_level_names_sha256", "selected_names_sha256", "generated_tsv_sha256", "names_encoding"}
    if set(manifest) != required_manifest or any(manifest[key] != expected for key, expected in EXPECTED_MANIFEST.items()) or any(manifest[key] != expected for key, expected in EXPECTED_MANIFEST_HASHES.items()):
        fail("contract manifest schema/binding is not exact")
    for key in ("top_level_names_sha256", "selected_names_sha256", "generated_tsv_sha256"):
        if not isinstance(manifest[key], str) or not HEX64.fullmatch(manifest[key]):
            fail(f"contract manifest {key} is not SHA-256")
    policy = value["skip_policy"]
    if set(policy) != {"version", "base_profile", "entries"} or policy["version"] != 1 or policy["base_profile"] != BASE_PROFILE:
        fail("contract skip policy schema is not exact")
    entries = tuple(PolicyEntry(**entry) for entry in policy["entries"])
    if entries != tuple(PolicyEntry(*entry) for entry in EXPECTED_POLICY) or len({entry.test for entry in entries}) != 7:
        fail("contract skip policy entries are not exact sorted entries")
    return Contract(hashlib.sha256(raw).hexdigest(), manifest["generated_tsv_sha256"], 1045, entries)


def read_tsv(path: Path, label: str) -> list[list[str]]:
    regular_no_symlink(path, label)
    with path.open(newline="", encoding="utf-8") as handle:
        return list(csv.reader(handle, delimiter="\t"))


def read_metadata(path: Path, contract: Contract, name: str, expected_workflow: str | None) -> dict[str, str]:
    result: dict[str, str] = {}
    for row in read_tsv(path, "metadata"):
        if len(row) != 2 or not row[0] or row[0] in result:
            fail(f"metadata duplicate/malformed key for {name}")
        result[row[0]] = row[1]
    if set(result) != REQUIRED_METADATA:
        fail(f"metadata schema mismatch for {name}")
    if result["profile"] != name or result["scope"] != "full1045" or result["expected_tests"] != "1045":
        fail(f"metadata scope/profile mismatch for {name}")
    if result["contract_sha256"] != contract.sha256 or result["manifest_sha256"] != contract.generated_tsv_sha256:
        fail(f"metadata contract/manifest identity mismatch for {name}")
    if not HEX40.fullmatch(result["workflow_commit"]):
        fail(f"metadata workflow commit malformed for {name}")
    if expected_workflow is not None and result["workflow_commit"] != expected_workflow:
        fail(f"metadata workflow commit mismatch for {name}")
    return result


def validate_status(path: Path, name: str, lane: str) -> None:
    values: dict[str, str] = {}
    for row in read_tsv(path, "lane status"):
        if len(row) != 2 or not row[0] or row[0] in values:
            fail(f"lane status duplicate/malformed key for {name}/{lane}")
        values[row[0]] = row[1]
    required = {"role", "stock_control", "command_exit", "summary_present", "summary_rows", "summary_passes", "summary_failures", "expected_rows"}
    if set(values) != required or values["role"] != lane or values["summary_present"] != "true" or values["summary_rows"] != "1045" or values["expected_rows"] != "1045":
        fail(f"lane status schema/denominator mismatch for {name}/{lane}")
    if not re.fullmatch(r"[0-9]+", values["command_exit"]):
        fail(f"lane status exit malformed for {name}/{lane}")


def validate_checksums(root: Path) -> None:
    checksum_path = root / "checksums.sha256"
    regular_no_symlink(checksum_path, "checksums.sha256")
    expected: dict[str, str] = {}
    for line in checksum_path.read_text(encoding="utf-8").splitlines():
        match = re.fullmatch(r"([0-9a-f]{64})  (.+)", line)
        if not match:
            fail("malformed checksum entry")
        digest, relative = match.groups()
        relative_path = Path(relative)
        if relative_path.is_absolute() or ".." in relative_path.parts or relative in expected or relative == "checksums.sha256":
            fail("unsafe/duplicate checksum path")
        target = root / relative_path
        secure_path(target, "checksum target")
        expected[relative] = digest
    actual_paths: set[str] = set()
    for directory, directories, files in os.walk(root, followlinks=False):
        for directory_name in directories:
            if os.path.islink(Path(directory) / directory_name):
                fail("checksum tree contains symlink directory")
        for file_name in files:
            target = Path(directory) / file_name
            relative = target.relative_to(root).as_posix()
            if relative == "checksums.sha256":
                continue
            regular_no_symlink(target, "checksum tree file")
            actual_paths.add(relative)
    if actual_paths != set(expected):
        fail("checksum entries do not exactly cover artifact files")
    for relative, digest in expected.items():
        if hashlib.sha256((root / relative).read_bytes()).hexdigest() != digest:
            fail(f"checksum mismatch: {relative}")


def normalize_reason(value: str) -> str:
    return " ".join(value.split())


def tap_skips(log_path: Path) -> list[str]:
    text = log_path.read_text(encoding="utf-8", errors="strict")
    return [normalize_reason(match.group(1) or "") for match in TAP_SKIP.finditer(text)]


def validate_log_reference(raw_reference: str, lane_root: Path, lane: str, test_name: str) -> Path:
    if not raw_reference or "\\" in raw_reference:
        fail(f"unsafe {lane} log reference for {test_name}")
    try:
        raw_reference.encode("utf-8").decode("utf-8")
    except UnicodeError:
        fail(f"non-UTF-8 {lane} log reference for {test_name}")
    if any(ord(character) < 32 or ord(character) == 127 for character in raw_reference):
        fail(f"control character in {lane} log reference for {test_name}")
    if raw_reference.startswith("/"):
        fail(f"absolute {lane} log reference for {test_name}")
    components = raw_reference.split("/")
    if any(component in {"", ".", ".."} for component in components):
        fail(f"non-canonical {lane} log reference for {test_name}")
    expected_reference = f"{test_name[:-3]}.log"
    if raw_reference != expected_reference:
        fail(f"unexpected {lane} log reference for {test_name}: {raw_reference}")
    reference = Path(raw_reference)
    if reference.is_absolute() or tuple(reference.parts) != (expected_reference,):
        fail(f"ambiguous {lane} log reference for {test_name}")
    log_path = lane_root / reference
    try:
        log_path.relative_to(lane_root)
    except ValueError:
        fail(f"{lane} log reference escapes lane for {test_name}")
    secure_path(log_path, f"{lane} log")
    regular_no_symlink(log_path, f"{lane} log")
    return log_path


def validate_profile(name: str, root: Path, contract: Contract, expected_workflow: str | None) -> ProfileArtifact:
    secure_path(root, f"profile {name}", directory=True)
    validate_checksums(root)
    metadata = read_metadata(root / "metadata.tsv", contract, name, expected_workflow)
    manifest_path = root / "scope" / "selected-manifest.tsv"
    manifest = read_tsv(manifest_path, "selected manifest")
    if hashlib.sha256(manifest_path.read_bytes()).hexdigest() != contract.generated_tsv_sha256:
        fail(f"manifest digest mismatch for {name}")
    tests = [row[1] for row in manifest[1:] if len(row) == 3 and row[1]]
    if len(tests) != contract.selected_count or len(set(tests)) != contract.selected_count or any(".." in test or "/" in test or not re.fullmatch(r"t[0-9]{4}-[^/]+\.sh", test) for test in tests):
        fail(f"manifest identity/denominator mismatch for {name}")
    selected = set(tests)
    coverage: set[tuple[str, str]] = set()
    observed_skips: dict[tuple[str, str], str] = {}
    for lane in ("control", "zmin"):
        lane_root = root / lane
        secure_path(lane_root, f"{name}/{lane}", directory=True)
        validate_status(lane_root / "status.tsv", name, lane)
        summary = read_tsv(lane_root / "summary.tsv", "summary")
        if summary[:1] != [SUMMARY_HEADER] or len(summary[1:]) != contract.selected_count:
            fail(f"summary schema/denominator mismatch for {name}/{lane}")
        seen: set[str] = set()
        for row in summary[1:]:
            if len(row) != 5:
                fail(f"summary row malformed for {name}/{lane}")
            mode, test_name, status, _reason, raw_log = row
            if mode != "all-nondeprecated" or test_name not in selected or test_name in seen or status not in {"pass", "fail"}:
                fail(f"summary identity/status mismatch for {name}/{lane}")
            seen.add(test_name)
            log_path = validate_log_reference(raw_log, lane_root, lane, test_name)
            skips = tap_skips(log_path)
            key = (lane, test_name)
            if len(skips) > 1:
                fail(f"multiple top-level skips for {name}/{lane}/{test_name}")
            if skips:
                observed_skips[key] = skips[0]
            elif status == "pass":
                coverage.add(key)
        if seen != selected:
            fail(f"summary test set mismatch for {name}/{lane}")
    optional = read_tsv(root / "optional-skips.tsv", "optional skips")
    if optional[:1] != [OPTIONAL_HEADER]:
        fail(f"optional skip schema mismatch for {name}")
    policy = {entry.test: entry for entry in contract.policy}
    declared: dict[tuple[str, str], str] = {}
    for row in optional[1:]:
        if len(row) != 6:
            fail(f"optional skip row malformed for {name}")
        lane, test_name, classification, required_profile, reason, log_sha = row
        key = (lane, test_name)
        if lane not in {"control", "zmin"} or key in declared or key not in observed_skips or test_name not in policy or not HEX64.fullmatch(log_sha):
            fail(f"optional skip linkage/schema mismatch for {name}")
        entry = policy[test_name]
        log_path = validate_log_reference(f"{test_name[:-3]}.log", root / lane, lane, test_name)
        if log_sha != hashlib.sha256(log_path.read_bytes()).hexdigest() or classification != entry.classification or required_profile != entry.required_run_profile or reason != entry.reason or observed_skips[key] != entry.reason:
            fail(f"optional skip policy/hash mismatch for {name}")
        declared[key] = reason
    if set(declared) != set(observed_skips):
        fail(f"optional skip rows missing/extra for {name}")
    return ProfileArtifact(name, root, frozenset(selected), frozenset(coverage), tuple(sorted(declared)), metadata["workflow_commit"])


def atomic_write(path: Path, content: str) -> None:
    if not path.is_absolute():
        fail("output must be absolute")
    parent = secure_path(path.parent, "output parent", directory=True)
    if path.is_symlink():
        fail("output is symlink")
    with tempfile.NamedTemporaryFile("w", encoding="utf-8", dir=parent, prefix=".aggregate.", delete=False) as handle:
        temporary = Path(handle.name)
        handle.write(content)
        handle.flush()
        os.fsync(handle.fileno())
    os.replace(temporary, path)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--contract", required=True, type=Path)
    parser.add_argument("--profile", action="append", required=True, metavar="NAME=ROOT")
    parser.add_argument("--workflow-commit")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    try:
        contract = load_contract(args.contract)
        allowed_profiles = {BASE_PROFILE} | {entry.required_run_profile for entry in contract.policy}
        profiles: list[ProfileArtifact] = []
        names: set[str] = set()
        for value in args.profile:
            if "=" not in value:
                fail("profile must be NAME=ROOT")
            name, raw_root = value.split("=", 1)
            if name not in allowed_profiles or name in names:
                fail(f"unknown/duplicate profile: {name}")
            names.add(name)
            profiles.append(validate_profile(name, secure_path(Path(raw_root), "profile root", directory=True), contract, args.workflow_commit))
        if BASE_PROFILE not in names:
            fail("base Linux profile is required")
        workflow_commits = {profile.workflow_commit for profile in profiles}
        if len(workflow_commits) != 1:
            fail("profile workflow commits diverge")
        missing = sorted(allowed_profiles - names)
        status = "pending" if missing else "pass"
        gaps = {entry.test for entry in contract.policy} if missing else set()
        if not missing:
            for profile in profiles:
                assigned = {entry.test for entry in contract.policy if entry.required_run_profile == profile.name}
                if assigned & {test for _, test in profile.skipped}:
                    fail(f"assigned profile did not close its test: {profile.name}")
            covered = set().union(*(set(profile.coverage) for profile in profiles))
            expected = {(lane, test) for lane in ("control", "zmin") for test in next(iter(profiles)).tests}
            if covered != expected:
                fail("coverage closure is incomplete")
        rows = [("cross_platform_status", status), ("profiles_present", str(len(profiles))), ("required_profiles", str(len(allowed_profiles))), ("closure_gap_count", str(len(gaps))), ("selected_count", str(contract.selected_count)), ("workflow_commit", next(iter(workflow_commits)))]
        if missing:
            rows.append(("missing_profiles", ",".join(missing)))
        content = "\n".join("\t".join(row) for row in rows) + "\n"
        if args.output is not None:
            atomic_write(args.output, content)
        else:
            sys.stdout.write(content)
        return 0 if status == "pass" else 1
    except (OSError, ValueError, TypeError, KeyError, UnicodeError) as error:
        print(f"aggregate-error: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
