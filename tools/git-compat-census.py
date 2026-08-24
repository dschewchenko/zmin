#!/usr/bin/env python3
"""Generate the frozen current Git compatibility census/checklist.

The census starts from independent source layers:

- the committed Git v2.55.0 compatibility contract
- the current Git command and documentation option seeds
- the historical Zmin CLI schema and behavior matrices as evidence inputs
- the existing stock-oracle test inventory as an evidence layer
- the committed Zmin extension contract
- source hard-fail guard scans

It deliberately does not use `existing_oracle_test_inventory.tsv` as the
primary backlog. That TSV is only used to connect reviewed tests to evidence
status after the command/docs/schema/matrix surfaces are known.
"""

from __future__ import annotations

import argparse
import csv
import hashlib
import json
import os
import re
import subprocess
import sys
from collections import Counter, defaultdict
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable, Mapping


MATRIX_COLUMNS = [
    "group",
    "command",
    "option",
    "value",
    "combination",
    "repo_state",
    "transport",
    "platform",
    "stock_git_case",
    "zmin_status",
    "evidence",
    "notes",
]

MATRIX_COLUMN_ALIASES = {
    "reference_group": "group",
    "repository_state": "repo_state",
    "example": "stock_git_case",
    "status": "zmin_status",
}

OUTPUT_COLUMNS = [
    "item_id",
    "bucket",
    "item_kind",
    "command",
    "option",
    "value",
    "combination",
    "repo_state",
    "transport",
    "platform",
    "implementation_source",
    "evidence_source",
    "evidence_kind",
    "source_detail",
    "next_action",
    "notes",
]

HARD_FAIL_PATTERN = re.compile(r"unsupported|not supported yet|not implemented yet")
LONG_OPTION_PATTERN = re.compile(r"(?<!\S)(--[A-Za-z0-9][A-Za-z0-9-]*)(?:[=\s]|$)")
SHORT_OPTION_PATTERN = re.compile(r"(?<!\S)(-[A-Za-z0-9?])(?:[=\s]|$)")
IDENTIFIER_PATTERN = re.compile(r"\b[A-Za-z_][A-Za-z0-9_]*\b")

UPSTREAM_CONTRACT_COLUMNS = ["key", "value"]
EXTENSION_CONTRACT_COLUMNS = [
    "row_type",
    "id",
    "kind",
    "parent",
    "surface",
    "evidence",
    "status",
    "anchor",
    "scope",
]
CURRENT_GIT_NON_EXTENSION_NAMES = {
    "backfill",
    "diff-pairs",
    "format-rev",
    "history",
    "last-modified",
    "repo",
    "url-parse",
}
EXPECTED_PRIMARY_EXTENSION_IDS = {
    "command.hooks",
    "command.save",
    "command.changes",
    "command.publish",
    "command.update",
    "command.undo",
    "command.timeline",
    "command.recover",
    "command.compatibility",
    "command.lfs",
    "subcommand.hooks.init",
    "subcommand.hooks.add",
    "subcommand.hooks.list",
    "subcommand.hooks.remove",
    "subcommand.hooks.run",
    "option.clone.worktree-first",
    "option.clone.instant",
    "option.clone.background-fetch",
    "option.clone.demand-hydrate",
    "option.cat-file.type",
    "option.cat-file.size",
    "option.cat-file.exists",
    "option.cat-file.pretty",
    "option.imap-send.folder",
    "option.imap-send.list",
    "option.imap-send.short-folder",
    "option.credential-cache.daemon-internal",
    "option.instaweb.daemon-internal",
    "option.instaweb.git-dir",
    "option.instaweb.work-tree",
    "hook-option.hook-run.ignore-missing",
    "hook-option.hook-run.to-stdin",
    "hook-option.hooks-add.force",
    "hook-option.hooks-add.staged-runner",
    "hook-option.hooks-add.ext",
    "hook-option.hooks-run.staged",
    "hook-option.hooks-run.ext",
    "hook-option.hooks-run.list",
    "hook-option.hooks-run.dry-run",
    "environment.ZMIN_GIT_HTTP_VERSION",
}
EXPECTED_RELATIONSHIP_IDS = {
    "relationship.hooks.init",
    "relationship.hooks.add",
    "relationship.hooks.list",
    "relationship.hooks.remove",
    "relationship.hooks.run",
    "relationship.repo.info",
    "relationship.repo.structure",
}


@dataclass(frozen=True)
class CurrentContract:
    values: dict[str, str]
    tag: str
    commit: str
    archive_url: str
    archive_sha256: str
    authoritative_denominator: int


@dataclass(frozen=True)
class ExtensionContract:
    primary: tuple[dict[str, str], ...]
    relationships: tuple[dict[str, str], ...]


@dataclass(frozen=True)
class CurrentSource:
    cache_dir: Path
    command_list: Path
    archive_sha256: str

    def environment(self, tag: str) -> dict[str, str]:
        return {
            "ZMIN_GIT_BASELINE": tag,
            "ZMIN_GIT_DOC_CACHE": str(self.cache_dir),
            "ZMIN_GIT_COMMAND_LIST": str(self.command_list),
            "ZMIN_GIT_SOURCE_ARCHIVE_SHA256": self.archive_sha256,
        }


def die(message: str) -> None:
    print(f"error: {message}", file=sys.stderr)
    raise SystemExit(1)


def run_text(
    command: list[str], cwd: Path, environment: Mapping[str, str] | None = None
) -> str:
    env = os.environ.copy()
    if environment:
        env.update(environment)
    result = subprocess.run(
        command,
        cwd=cwd,
        check=False,
        env=env,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    if result.returncode != 0:
        sys.stderr.write(result.stderr)
        die(f"command failed: {' '.join(command)}")
    return result.stdout


def stable_id(prefix: str, parts: Iterable[str]) -> str:
    payload = "\t".join(parts).encode()
    return f"{prefix}:{hashlib.sha1(payload).hexdigest()[:16]}"


def nested_parent_statuses(
    command: str,
    command_set: set[str],
    matrix_primary_by_status: dict[tuple[str, str], Counter[str]],
) -> Counter[str]:
    parts = command.split("-")
    for split in range(len(parts) - 1, 0, -1):
        parent = "-".join(parts[:split])
        subcommand = "-".join(parts[split:])
        statuses = matrix_primary_by_status.get((parent, subcommand), Counter())
        if statuses:
            return statuses
        if parent not in command_set:
            continue
    return Counter()


def nested_schema_refs(
    parent_command: str,
    option: str,
    schema_options: dict[tuple[str, str], list[dict[str, str]]],
    command_set: set[str],
) -> list[dict[str, str]]:
    refs: list[dict[str, str]] = []
    prefix = f"{parent_command}-"
    for (command, schema_option), arg_refs in schema_options.items():
        if (
            schema_option != option
            or not command.startswith(prefix)
            or command in command_set
        ):
            continue
        refs.extend(arg_refs)
    return refs


def read_tsv(path: Path) -> list[dict[str, str]]:
    if not path.exists():
        die(f"required TSV is missing: {path}")
    with path.open(newline="") as handle:
        return list(csv.DictReader(handle, delimiter="\t"))


def write_tsv(path: Path, columns: list[str], rows: list[dict[str, str]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", newline="") as handle:
        writer = csv.DictWriter(handle, delimiter="\t", fieldnames=columns, lineterminator="\n")
        writer.writeheader()
        for row in rows:
            writer.writerow({column: row.get(column, "") for column in columns})


def reviewed_complete_commands(root: Path) -> set[str]:
    path = root / "docs/cli/census/reviewed_complete_command_matrices.tsv"
    if not path.exists():
        return set()
    return {row["command"] for row in read_tsv(path) if row.get("command")}


def reviewed_complete_option_pairs(root: Path) -> set[tuple[str, str]]:
    path = root / "docs/cli/census/reviewed_complete_doc_option_pairs.tsv"
    if not path.exists():
        return set()
    return {
        (row["command"], row["option"])
        for row in read_tsv(path)
        if row.get("command") and row.get("option")
    }


def deferred_doc_option_pairs(
    root: Path,
) -> tuple[set[tuple[str, str]], list[dict[str, str]]]:
    path = root / "docs/cli/census/deferred_doc_option_pairs.tsv"
    if not path.exists():
        return set(), []
    pairs: set[tuple[str, str]] = set()
    rows: list[dict[str, str]] = []
    for row in read_tsv(path):
        command = row.get("command", "")
        option = row.get("option", "")
        if not command or not option:
            continue
        evidence = row.get("evidence", "")
        notes = row.get("notes", "")
        pairs.add((command, option))
        rows.append(
            {
                "item_id": stable_id("deferred-option-doc", [command, option, evidence, notes]),
                "bucket": "Zmin-only extension or deferred/non-Git-2.47.1 scope",
                "item_kind": "doc_option_oracle_deferral",
                "command": command,
                "option": option,
                "value": "<deferred>",
                "combination": "<none>",
                "repo_state": "<deferred>",
                "transport": "<deferred>",
                "platform": "all",
                "implementation_source": "doc option deferral inventory",
                "evidence_source": evidence,
                "evidence_kind": "deferral",
                "source_detail": str(path.relative_to(root)),
                "next_action": "revisit only with a real Git 2.47.1 stock-helper oracle or an explicit scope change",
                "notes": notes,
            }
        )
    return pairs, rows


def _contract_path(root: Path) -> Path:
    return root / "tools/git-upstream-compat-contract.tsv"


def _extension_contract_path(root: Path) -> Path:
    return root / "tools/zmin-extensions-contract.tsv"


def _read_strict_key_value_tsv(path: Path) -> dict[str, str]:
    if not path.exists():
        die(f"required current contract is missing: {path}")
    values: dict[str, str] = {}
    with path.open(newline="") as handle:
        reader = csv.reader(handle, delimiter="\t")
        try:
            header = next(reader)
        except StopIteration:
            die(f"current contract is empty: {path}")
        if header != UPSTREAM_CONTRACT_COLUMNS:
            die(f"current contract header drift: {path}")
        for line_number, row in enumerate(reader, start=2):
            if len(row) != 2 or not row[0] or not row[1]:
                die(f"malformed current contract row at {path}:{line_number}")
            if row[0] in values:
                die(f"duplicate current contract key: {row[0]}")
            values[row[0]] = row[1]
    return values


def _parse_contract_int(values: dict[str, str], key: str) -> int:
    value = values[key]
    if not re.fullmatch(r"[0-9]+", value):
        die(f"current contract integer drift: {key}")
    return int(value)


def load_current_contract(root: Path) -> CurrentContract:
    values = _read_strict_key_value_tsv(_contract_path(root))
    current_keys = {
        "contract_id",
        "upstream_git_tag",
        "upstream_git_repo",
        "upstream_git_commit",
        "source_identity_policy",
        "upstream_archive_url",
        "upstream_archive_sha256",
        "authoritative_manifest",
        "deprecated_removed_groups",
        "external_current_groups",
        "upstream_top_level_shell_tests",
        "excluded_deprecated_removed_shell_tests",
        "authoritative_upstream_test_denominator",
        "core_only_manifest",
        "core_only_shell_tests",
        "external_current_shell_tests",
        "required_evidence",
        "current_git_non_extension_count",
        "current_git_non_extension_backfill",
        "current_git_non_extension_diff_pairs",
        "current_git_non_extension_format_rev",
        "current_git_non_extension_history",
        "current_git_non_extension_last_modified",
        "current_git_non_extension_repo",
        "current_git_non_extension_url_parse",
    }
    if set(values) != current_keys:
        missing = sorted(current_keys - set(values))
        extra = sorted(set(values) - current_keys)
        die(f"current contract key set drift: missing={missing} extra={extra}")

    expected = {
        "contract_id": "git-current-nondeprecated-v2.55.0",
        "upstream_git_tag": "v2.55.0",
        "upstream_git_repo": "https://github.com/git/git.git",
        "upstream_git_commit": "e9019fcafe0040228b8631c30f97ae1adb61bcdc",
        "source_identity_policy": "archive_sha256_exact; tag_commit_declared; source_manifest_exact; no_checkout_fallback",
        "upstream_archive_url": "https://github.com/git/git/archive/refs/tags/v2.55.0.tar.gz",
        "upstream_archive_sha256": "72923418db7b26dfddc21e2268660c5118e560bdfaa09b4489b67b38e9b69c49",
        "authoritative_manifest": "all-nondeprecated",
        "deprecated_removed_groups": "t5323-pack-redundant|t5323|1",
        "external_current_groups": "git-svn|t91|69;git-cvsserver|t94|3;gitweb|t95|3;cvsimport|t96|5;git-p4|t98[0-3]|37",
        "core_only_manifest": "full-core",
        "required_evidence": "upstream shell suite; stock Git differential; macOS; Linux; Windows",
        "current_git_non_extension_backfill": "backfill|Documentation/git-backfill.adoc|t/t5620-backfill.sh|current-git-v2.55.0",
        "current_git_non_extension_diff_pairs": "diff-pairs|Documentation/git-diff-pairs.adoc|t/t4070-diff-pairs.sh|current-git-v2.55.0",
        "current_git_non_extension_format_rev": "format-rev|Documentation/git-format-rev.adoc|t/t6120-describe.sh|current-git-v2.55.0",
        "current_git_non_extension_history": "history|Documentation/git-history.adoc|t/t3450-history.sh,t/t3451-history-reword.sh,t/t3452-history-split.sh,t/t3453-history-fixup.sh|current-git-v2.55.0",
        "current_git_non_extension_last_modified": "last-modified|Documentation/git-last-modified.adoc|t/t8020-last-modified.sh|current-git-v2.55.0",
        "current_git_non_extension_repo": "repo|Documentation/git-repo.adoc|t/t1900-repo-info.sh,t/t1901-repo-structure.sh|current-git-v2.55.0",
        "current_git_non_extension_url_parse": "url-parse|Documentation/git-url-parse.adoc|t/t9904-url-parse.sh|current-git-v2.55.0",
    }
    for key, expected_value in expected.items():
        if values[key] != expected_value:
            die(f"current contract value drift: {key}")

    total = _parse_contract_int(values, "upstream_top_level_shell_tests")
    excluded = _parse_contract_int(values, "excluded_deprecated_removed_shell_tests")
    denominator = _parse_contract_int(values, "authoritative_upstream_test_denominator")
    core = _parse_contract_int(values, "core_only_shell_tests")
    external = _parse_contract_int(values, "external_current_shell_tests")
    non_extension_count = _parse_contract_int(values, "current_git_non_extension_count")
    expected_metrics = {
        "upstream_top_level_shell_tests": (total, 1046),
        "excluded_deprecated_removed_shell_tests": (excluded, 1),
        "authoritative_upstream_test_denominator": (denominator, 1045),
        "core_only_shell_tests": (core, 928),
        "external_current_shell_tests": (external, 117),
        "current_git_non_extension_count": (non_extension_count, 7),
    }
    for key, (actual, expected_value) in expected_metrics.items():
        if actual != expected_value:
            die(f"current contract metric drift: {key}")
    if total - excluded != denominator or core + external != denominator:
        die("current contract denominator arithmetic drift")

    removed_parts = values["deprecated_removed_groups"].split("|")
    if len(removed_parts) != 3 or removed_parts[0] != "t5323-pack-redundant" or removed_parts[1] != "t5323":
        die("deprecated exclusion metadata drift")
    if int(removed_parts[2]) != excluded:
        die("deprecated exclusion count drift")
    external_total = 0
    for group in values["external_current_groups"].split(";"):
        parts = group.split("|")
        if len(parts) != 3 or not parts[0] or not parts[1] or not parts[2].isdigit():
            die("external current group metadata drift")
        external_total += int(parts[2])
    if external_total != external:
        die("external current group arithmetic drift")

    return CurrentContract(
        values=values,
        tag=values["upstream_git_tag"],
        commit=values["upstream_git_commit"],
        archive_url=values["upstream_archive_url"],
        archive_sha256=values["upstream_archive_sha256"],
        authoritative_denominator=denominator,
    )


def load_extension_contract(root: Path) -> ExtensionContract:
    path = _extension_contract_path(root)
    if not path.exists():
        die(f"required extension contract is missing: {path}")
    rows: list[dict[str, str]] = []
    seen_ids: set[str] = set()
    with path.open(newline="") as handle:
        reader = csv.reader(handle, delimiter="\t")
        try:
            header = next(reader)
        except StopIteration:
            die(f"extension contract is empty: {path}")
        if header != EXTENSION_CONTRACT_COLUMNS:
            die(f"extension contract header drift: {path}")
        for line_number, values in enumerate(reader, start=2):
            if len(values) != len(EXTENSION_CONTRACT_COLUMNS) or any(value == "" for value in values):
                die(f"malformed extension contract row at {path}:{line_number}")
            row = dict(zip(EXTENSION_CONTRACT_COLUMNS, values))
            if row["id"] in seen_ids:
                die(f"duplicate extension contract id: {row['id']}")
            seen_ids.add(row["id"])
            if row["row_type"] not in {"primary", "relationship"}:
                die(f"invalid extension row kind: {row['row_type']}")
            if row["kind"] not in {"command", "subcommand", "option", "environment", "relationship"}:
                die(f"invalid extension kind: {row['kind']}")
            if row["status"] not in {"stable", "deferred"}:
                die(f"invalid extension status: {row['status']}")
            if row["row_type"] == "relationship" and row["kind"] != "relationship":
                die(f"relationship row has invalid kind: {row['id']}")
            if row["row_type"] == "primary" and row["kind"] == "relationship":
                die(f"primary row has relationship kind: {row['id']}")
            if row["kind"] in {"command", "environment"} and row["parent"] != "-":
                die(f"root extension row has unexpected parent: {row['id']}")
            if (
                row["kind"] == "subcommand" and not row["parent"]
                or row["kind"] == "option" and not row["parent"]
            ):
                die(f"nested extension row has missing parent: {row['id']}")
            rows.append(row)

    primary = tuple(row for row in rows if row["row_type"] == "primary")
    relationships = tuple(row for row in rows if row["row_type"] == "relationship")
    for row in primary:
        if row["kind"] == "command" and row["surface"] in CURRENT_GIT_NON_EXTENSION_NAMES:
            die(f"current Git command cannot be a Zmin extension: {row['surface']}")
    if {row["id"] for row in primary} != EXPECTED_PRIMARY_EXTENSION_IDS:
        die("primary extension id set drift")
    if {row["id"] for row in relationships} != EXPECTED_RELATIONSHIP_IDS:
        die("extension relationship id set drift")
    if len(primary) != 40 or len(relationships) != 7:
        die("extension contract count drift")
    for row in rows:
        row_id = row["id"]
        if row_id.startswith("command."):
            expected_kind, expected_parent, expected_surface = "command", "-", row_id.removeprefix("command.")
        elif row_id.startswith("subcommand.hooks."):
            expected_kind, expected_parent, expected_surface = "subcommand", "hooks", row_id.rsplit(".", 1)[1]
        elif row_id.startswith("option."):
            _, expected_parent, suffix = row_id.split(".", 2)
            expected_kind = "option"
            expected_surface = "-f" if suffix == "short-folder" else f"--{suffix}"
        elif row_id.startswith("hook-option.hook-run."):
            expected_kind, expected_parent = "option", "hook"
            expected_surface = f"--{row_id.rsplit('.', 1)[1]}"
        elif row_id.startswith("hook-option.hooks-add.") or row_id.startswith("hook-option.hooks-run."):
            expected_kind, expected_parent = "option", "hooks"
            expected_surface = f"--{row_id.rsplit('.', 1)[1]}"
        elif row_id.startswith("environment."):
            expected_kind, expected_parent, expected_surface = "environment", "-", row_id.removeprefix("environment.")
        elif row_id.startswith("relationship.hooks."):
            expected_kind, expected_parent, expected_surface = "relationship", "hooks", row_id.rsplit(".", 1)[1]
        elif row_id.startswith("relationship.repo."):
            expected_kind, expected_parent, expected_surface = "relationship", "repo", row_id.rsplit(".", 1)[1]
        else:
            die(f"unknown extension id shape: {row_id}")
        if (row["kind"], row["parent"], row["surface"]) != (
            expected_kind,
            expected_parent,
            expected_surface,
        ):
            die(f"extension row shape drift: {row_id}")
        if row["status"] != "stable":
            die(f"extension row status drift: {row_id}")
    for row in relationships:
        expected_parent = "hooks" if row["id"].startswith("relationship.hooks.") else "repo"
        if row["parent"] != expected_parent:
            die(f"relationship parent drift: {row['id']}")
    return ExtensionContract(primary=primary, relationships=relationships)


def resolve_current_source(root: Path, contract: CurrentContract) -> CurrentSource:
    cache_value = os.environ.get("ZMIN_GIT_DOC_CACHE")
    cache_dir = Path(cache_value) if cache_value else root / "target" / "git-doc-cache" / contract.tag
    cache_dir = cache_dir.resolve()
    command_value = os.environ.get("ZMIN_GIT_COMMAND_LIST")
    command_path = Path(command_value) if command_value else cache_dir / "command-list.txt"
    if command_path.is_symlink():
        die(f"Git {contract.tag} command-list must be the validated source command-list.txt: {command_path}")
    command_list = command_path.resolve()
    expected_basename = f"git-{contract.tag}"
    if cache_dir.name != expected_basename:
        die(f"Git source root basename does not match {contract.tag}: {cache_dir}")
    expected_command_list = (cache_dir / "command-list.txt").resolve()
    if command_list != expected_command_list or command_list.is_symlink():
        die(f"Git {contract.tag} command-list must be the validated source command-list.txt: {command_list}")
    if not command_list.is_file():
        die(f"Git {contract.tag} command-list cache is missing: {command_list}")
    marker = cache_dir / ".zmin-pristine-source.sha256"
    if not marker.is_file():
        die(f"Git {contract.tag} source identity marker is missing: {marker}")
    actual_marker = marker.read_text(encoding="utf-8").strip()
    if actual_marker != contract.archive_sha256:
        die(
            f"Git {contract.tag} source identity mismatch: "
            f"expected {contract.archive_sha256}, got {actual_marker or '<empty>'}"
        )
    documentation = cache_dir / "Documentation"
    if not documentation.is_dir():
        die(f"Git {contract.tag} source Documentation directory is missing: {documentation}")
    return CurrentSource(
        cache_dir=cache_dir,
        command_list=command_list,
        archive_sha256=contract.archive_sha256,
    )


def command_list_from_cache(source: CurrentSource) -> list[str]:
    commands = []
    for line in source.command_list.read_text().splitlines():
        fields = line.split()
        if not fields or not fields[0].startswith("git-"):
            continue
        commands.append(fields[0][4:])
    if not commands:
        die(f"Git v2.55.0 command-list cache has no commands: {source.command_list}")
    return sorted(set(commands))


def option_seed_from_docs(root: Path, source: CurrentSource, contract: CurrentContract) -> list[dict[str, str]]:
    output = run_text(
        [str(root / "tools/git-compat-option-inventory.sh")],
        root,
        source.environment(contract.tag),
    )
    rows = list(csv.DictReader(output.splitlines(), delimiter="\t"))
    required = {"command", "option", "doc"}
    if not rows or set(rows[0]) != required:
        die("Git documentation option seed has an unexpected shape")
    return rows


def load_historical_zmin_schema(root: Path, schema_json: Path | None) -> dict:
    if schema_json is not None:
        if not schema_json.is_absolute():
            schema_json = root / schema_json
        if not schema_json.exists():
            die(f"Zmin schema JSON is missing: {schema_json}")
        return json.loads(schema_json.read_text())

    output = run_text(
        [
            "cargo",
            "run",
            "-q",
            "-p",
            "zmin-cli",
            "--bin",
            "zmin",
            "--",
            "compat",
            "--profile",
            "v2-47",
            "--format",
            "json",
        ],
        root,
    )
    return json.loads(output)


def normalize_schema(schema: dict) -> tuple[set[str], set[str], dict[tuple[str, str], list[dict[str, str]]]]:
    command_names: set[str] = set()
    additional: set[str] = set()
    schema_options: dict[tuple[str, str], list[dict[str, str]]] = defaultdict(list)

    for name in schema.get("additional", []):
        if name.startswith("git-"):
            additional.add(name[4:])

    for command in schema.get("commands", []):
        raw_name = command.get("name", "")
        if not raw_name.startswith("git-"):
            continue
        command_name = raw_name[4:]
        command_names.add(command_name)
        for arg in command.get("args", []):
            candidates = []
            if arg.get("long"):
                candidates.append(arg["long"])
            if arg.get("short"):
                candidates.append(arg["short"])
            if arg.get("positional"):
                candidates.append(f"<positional:{arg.get('id', 'arg')}>")
            for option in candidates:
                schema_options[(command_name, option)].append(
                    {
                        "arg_id": str(arg.get("id", "")),
                        "num_args": str(arg.get("num_args", "")),
                        "action": str(arg.get("action", "")),
                        "required": str(arg.get("required", "")),
                        "positional": str(arg.get("positional", "")),
                    }
                )

    return command_names, additional, schema_options


def historical_matrix_rows(root: Path) -> list[dict[str, str]]:
    rows: list[dict[str, str]] = []
    matrix_dir = root / "docs/cli/matrices"
    if not matrix_dir.exists():
        die(f"matrix directory is missing: {matrix_dir}")
    for matrix in sorted(matrix_dir.glob("*_v2_47.tsv")):
        with matrix.open(newline="") as handle:
            reader = csv.DictReader(handle, delimiter="\t")
            normalized_fieldnames = [
                MATRIX_COLUMN_ALIASES.get(field, field) for field in (reader.fieldnames or [])
            ]
            if normalized_fieldnames != MATRIX_COLUMNS:
                die(f"unexpected matrix columns in {matrix}")
            for line_number, row in enumerate(reader, start=2):
                row = {
                    MATRIX_COLUMN_ALIASES.get(key, key): value
                    for key, value in dict(row).items()
                }
                row["matrix_file"] = str(matrix.relative_to(root))
                row["matrix_line"] = str(line_number)
                rows.append(row)
    return rows


def evidence_kind(evidence: str) -> str:
    if evidence.startswith("tools/") or "dogfood" in evidence or "trace" in evidence:
        return "real_tool_trace"
    if "::" in evidence:
        return "stock_git_oracle_test"
    if evidence.startswith("t") or "upstream" in evidence:
        return "upstream_git_test"
    if evidence:
        return "matrix_row_evidence"
    return "missing_evidence"


def row_from_matrix(row: dict[str, str], bucket: str, item_kind: str, next_action: str) -> dict[str, str]:
    return {
        "item_id": stable_id(
            "matrix",
            [
                row["matrix_file"],
                row["matrix_line"],
                row["command"],
                row["option"],
                row["value"],
                row["combination"],
                row["repo_state"],
                row["transport"],
                row["platform"],
            ],
        ),
        "bucket": bucket,
        "item_kind": item_kind,
        "command": row["command"],
        "option": row["option"],
        "value": row["value"],
        "combination": row["combination"],
        "repo_state": row["repo_state"],
        "transport": row["transport"],
        "platform": row["platform"],
        "implementation_source": "behavior matrix",
        "evidence_source": row["evidence"],
        "evidence_kind": evidence_kind(row["evidence"]),
        "source_detail": f"{row['matrix_file']}:{row['matrix_line']}",
        "next_action": next_action,
        "notes": row["notes"],
    }


def matrix_option_spellings(row: dict[str, str]) -> set[str]:
    """Return option spellings evidenced by a matrix row.

    The matrix `option` column is authoritative for the primary row shape, but
    many rows verify option combinations in `stock_git_case`. Extract only
    unambiguous spellings so the census can avoid false backlog without
    treating compact short-option clusters as proof for every possible alias.
    """

    spellings = set()
    option = row["option"]
    if option.startswith("-"):
        spellings.add(option.split("=", 1)[0])

    text = " ".join([row["stock_git_case"], row["combination"]])
    spellings.update(match.group(1) for match in LONG_OPTION_PATTERN.finditer(text))
    spellings.update(match.group(1) for match in SHORT_OPTION_PATTERN.finditer(text))
    return spellings


def normalized_placeholder_names(option: str) -> set[str]:
    if not (option.startswith("<") and option.endswith(">")):
        return set()
    raw = option[1:-1].lower()
    normalized = re.sub(r"[^a-z0-9]+", "_", raw).strip("_")
    if not normalized or normalized == "none":
        return set()

    names = {normalized}
    if normalized.startswith("positional_"):
        names.add(normalized.removeprefix("positional_"))
    if normalized.endswith("s"):
        names.add(normalized[:-1])
    aliases = {
        "pathspec": {"paths", "path"},
        "tree_ish": {"treeish"},
        "tree": {"treeish"},
        "pattern": {"patterns"},
        "revision_range": {"revs", "revision_ranges"},
        "repository": {"remote"},
        "refspec": {"refspecs"},
        "basename": {"base_name"},
    }
    names.update(aliases.get(normalized, set()))
    return names


def hard_fail_is_documented(path: Path, stripped: str, docs_text: str) -> bool:
    quoted = re.findall(r'"([^"]*(?:unsupported|not supported yet|not implemented yet)[^"]*)"', stripped)
    if stripped in docs_text or any(fragment in docs_text for fragment in quoted):
        return True

    basename = path.name
    if basename not in docs_text:
        return False
    identifiers = [
        identifier
        for identifier in IDENTIFIER_PATTERN.findall(stripped)
        if any(fragment in identifier for fragment in ["unsupported", "not_supported", "not_implemented"])
    ]
    return any(
        identifier in docs_text
        for identifier in identifiers
    )


def hard_fail_scan(root: Path, extension_contract: ExtensionContract) -> list[dict[str, str]]:
    rows = []
    scan_roots = [
        root / "crates/zmin-cli/src",
        root / "crates/zmin-git-core/src",
    ]
    for scan_root in scan_roots:
        if not scan_root.exists():
            die(f"source scan root is missing: {scan_root}")
        for path in sorted(scan_root.rglob("*.rs")):
            for line_number, line in enumerate(path.read_text(errors="replace").splitlines(), start=1):
                if not HARD_FAIL_PATTERN.search(line):
                    continue
                stripped = line.strip()
                if ".expect_err(" in stripped:
                    continue
                classification_status = "unclassified"
                relative_path = str(path.relative_to(root))
                for extension in extension_contract.primary:
                    if extension["kind"] != "environment":
                        continue
                    if extension["surface"] not in stripped or extension["status"] != "stable":
                        continue
                    evidence_paths = extension["evidence"].split(";")
                    if relative_path not in evidence_paths:
                        continue
                    if not all((root / evidence_path).is_file() for evidence_path in evidence_paths):
                        continue
                    classification_status = "zmin-only-evidenced"
                    break
                rows.append(
                    {
                        "file": str(path.relative_to(root)),
                        "line": str(line_number),
                        "text": stripped,
                        "classification_status": classification_status,
                    }
                )
    return rows


def zmin_extension_command_names(contract: ExtensionContract) -> set[str]:
    names = {
        row["surface"]
        for row in contract.primary
        if row["kind"] == "command"
    }
    names.update(
        f"{row['parent']}-{row['surface']}"
        for row in contract.primary
        if row["kind"] == "subcommand"
    )
    return names


def zmin_extension_option_keys(contract: ExtensionContract) -> set[tuple[str, str]]:
    return {
        (row["parent"], row["surface"])
        for row in contract.primary
        if row["kind"] == "option"
    }


def zmin_extension_rows(contract: ExtensionContract) -> list[dict[str, str]]:
    rows: list[dict[str, str]] = []
    for source_row in contract.primary:
        if source_row["kind"] == "command":
            command = source_row["surface"]
            option = "<command>"
            item_kind = "zmin_extension_surface"
        elif source_row["kind"] == "subcommand":
            command = f"{source_row['parent']}-{source_row['surface']}"
            option = "<command>"
            item_kind = "zmin_extension_surface"
        elif source_row["kind"] == "option":
            command = source_row["parent"]
            option = source_row["surface"]
            item_kind = "zmin_extension_surface"
        elif source_row["kind"] == "environment":
            command = source_row["surface"]
            option = "<environment>"
            item_kind = "zmin_extension_environment"
        rows.append(
            {
                "item_id": stable_id("extension-contract", [source_row["id"]]),
                "bucket": "Zmin-only extension surface",
                "item_kind": item_kind,
                "command": command,
                "option": option,
                "value": source_row["status"],
                "combination": source_row["kind"],
                "repo_state": "<not-applicable>",
                "transport": "<not-applicable>",
                "platform": "all",
                "implementation_source": "committed Zmin extension contract",
                "evidence_source": source_row["evidence"],
                "evidence_kind": "extension_contract",
                "source_detail": f"id={source_row['id']} scope={source_row['scope']}",
                "next_action": "keep outside the upstream compatibility denominator",
                "notes": source_row["anchor"],
            }
        )
    return rows


def zmin_relationship_rows(contract: ExtensionContract) -> list[dict[str, str]]:
    rows: list[dict[str, str]] = []
    for source_row in contract.relationships:
        rows.append(
            {
                "item_id": source_row["id"],
                "bucket": "API relationship evidence",
                "item_kind": "api_relationship",
                "command": source_row["parent"],
                "option": source_row["surface"],
                "value": source_row["status"],
                "combination": source_row["kind"],
                "repo_state": "<not-applicable>",
                "transport": "<not-applicable>",
                "platform": "all",
                "implementation_source": "committed Zmin API relationship contract",
                "evidence_source": source_row["evidence"],
                "evidence_kind": "relationship_contract",
                "source_detail": f"id={source_row['id']} scope={source_row['scope']}",
                "next_action": "relationship metadata only; not a scope classification",
                "notes": source_row["anchor"],
            }
        )
    return rows


def make_census(
    root: Path,
    contract: CurrentContract,
    source: CurrentSource,
    schema_json: Path | None,
) -> dict[str, list[dict[str, str]]]:
    commands = command_list_from_cache(source)
    command_set = set(commands)
    options = option_seed_from_docs(root, source, contract)
    documented_option_pairs = {(row["command"], row["option"]) for row in options}
    schema = load_historical_zmin_schema(root, schema_json)
    zmin_commands, additional_commands, zmin_options = normalize_schema(schema)
    matrices = historical_matrix_rows(root)
    extension_contract = load_extension_contract(root)
    hard_fails = hard_fail_scan(root, extension_contract)
    extension_commands = zmin_extension_command_names(extension_contract)
    extension_options = zmin_extension_option_keys(extension_contract)
    complete_commands = reviewed_complete_commands(root)
    complete_option_pairs = reviewed_complete_option_pairs(root) & documented_option_pairs
    deferred_option_pairs, deferred_option_rows = deferred_doc_option_pairs(root)

    matrix_options_by_status: dict[tuple[str, str], Counter[str]] = defaultdict(Counter)
    matrix_primary_by_status: dict[tuple[str, str], Counter[str]] = defaultdict(Counter)
    matrix_rows_by_command: Counter[str] = Counter()
    for row in matrices:
        matrix_primary_by_status[(row["command"], row["option"])][row["zmin_status"]] += 1
        for option in matrix_option_spellings(row):
            matrix_options_by_status[(row["command"], option)][row["zmin_status"]] += 1
        placeholder_names = normalized_placeholder_names(row["option"])
        if placeholder_names:
            for (command, option), arg_refs in zmin_options.items():
                if command != row["command"] or not option.startswith("<positional:"):
                    continue
                for arg_ref in arg_refs:
                    if arg_ref["arg_id"] in placeholder_names:
                        matrix_options_by_status[(command, option)][row["zmin_status"]] += 1
        matrix_rows_by_command[row["command"]] += 1

    verified = [
        row_from_matrix(row, "verified", "exact_behavior_variant", "safe to skip unless code or evidence changes")
        for row in matrices
        if row["zmin_status"] == "closed"
    ]
    invalid_input = [
        row_from_matrix(row, "invalid-input parity", "exact_invalid_input_variant", "safe to skip unless parser/error behavior changes")
        for row in matrices
        if row["zmin_status"] == "invalid-input"
    ]
    open_exact = [
        row_from_matrix(row, "not implemented / broken / open", "exact_behavior_variant", "fix implementation or evidence, then rerun stock-Git parity")
        for row in matrices
        if row["zmin_status"] in {"open", "partial"}
    ]
    exact_open_oracle_gaps = [
        {
            **row,
            "item_kind": "exact_open_local_oracle_unavailable",
            "next_action": "rerun on an environment with the missing stock Git command/tool, or keep open without claiming compatibility",
        }
        for row in open_exact
        if any(
            marker in f"{row['evidence_source']} {row['notes']}".lower()
            for marker in [
                "lacks",
                "unavailable",
                "oracle unavailable",
                "does not dispatch",
            ]
        )
    ]

    implemented_unverified = []
    for (command, option), arg_refs in sorted(zmin_options.items()):
        if command not in command_set:
            continue
        if command in complete_commands:
            continue
        if (command, option) in extension_options:
            continue
        statuses = matrix_options_by_status.get((command, option), Counter())
        for arg_ref in arg_refs:
            if (
                statuses["closed"]
                or statuses["invalid-input"]
                or statuses["open"]
                or statuses["partial"]
            ):
                continue
            implemented_unverified.append(
                {
                    "item_id": stable_id("schema-arg", [command, option, arg_ref["arg_id"]]),
                    "bucket": "implemented but unverified",
                    "item_kind": "zmin_schema_argument",
                    "command": command,
                    "option": option,
                    "value": arg_ref["num_args"],
                    "combination": f"action={arg_ref['action']}",
                    "repo_state": "<unclassified>",
                    "transport": "<unclassified>",
                    "platform": "all",
                    "implementation_source": "zmin compat schema",
                    "evidence_source": "historical zmin compat --profile v2-47 --format json",
                    "evidence_kind": "zmin_schema",
                    "source_detail": f"arg_id={arg_ref['arg_id']} required={arg_ref['required']} positional={arg_ref['positional']}",
                    "next_action": "add stock-Git oracle evidence before counting this parser/handler surface",
                    "notes": "schema presence is not compatibility evidence",
                }
            )

    for command in sorted(additional_commands):
        if command in extension_commands:
            continue
        statuses = matrix_primary_by_status.get((command, "<command>"), Counter())
        statuses += nested_parent_statuses(command, command_set | additional_commands, matrix_primary_by_status)
        if statuses["closed"] or statuses["invalid-input"] or statuses["open"] or statuses["partial"]:
            continue
        implemented_unverified.append(
            {
                "item_id": stable_id("schema-additional", [command]),
                "bucket": "implemented but unverified",
                "item_kind": "zmin_schema_additional_or_nested_command",
                "command": command,
                "option": "<command>",
                "value": "<empty>",
                "combination": "<none>",
                "repo_state": "<unclassified>",
                "transport": "<unclassified>",
                "platform": "all",
                "implementation_source": "zmin compat schema",
                    "evidence_source": "historical zmin compat --profile v2-47 --format json",
                "evidence_kind": "zmin_schema",
                "source_detail": "schema entry outside top-level Git command-list",
                "next_action": "map to parent Git command matrix, Zmin extension inventory or explicit deferral before counting",
                "notes": "additional schema entries include nested Git subcommands as well as Zmin-only surface",
            }
        )

    remaining = list(open_exact)
    for command in commands:
        if matrix_rows_by_command[command] == 0:
            remaining.append(
                {
                    "item_id": stable_id("command-matrix", [command]),
                    "bucket": "not implemented / broken / open",
                    "item_kind": "command_matrix_not_started",
                    "command": command,
                    "option": "<command>",
                    "value": "<empty>",
                    "combination": "<none>",
                    "repo_state": "<unexpanded>",
                    "transport": "<unexpanded>",
                    "platform": "all",
                    "implementation_source": "upstream Git command-list",
                    "evidence_source": "command-list.txt",
                    "evidence_kind": "upstream_command_seed",
                    "source_detail": contract.tag,
                    "next_action": "seed the command matrix from docs/options/states/transports before fixing behavior",
                    "notes": "entrypoint presence alone is not compatibility",
                }
            )

    for option in options:
        command = option["command"]
        spelling = option["option"]
        statuses = matrix_options_by_status.get((command, spelling), Counter())
        schema_refs = zmin_options.get((command, spelling), []) or nested_schema_refs(
            command, spelling, zmin_options, command_set
        )
        if (command, spelling) in complete_option_pairs:
            continue
        if (command, spelling) in deferred_option_pairs:
            continue
        if statuses["closed"] or statuses["invalid-input"]:
            next_action = "expand remaining values, negations, repeated forms, combinations, states, transports and platforms for this documented option"
            bucket = "not implemented / broken / open"
            kind = "doc_option_expansion_required"
        elif statuses["open"] or statuses["partial"]:
            next_action = "fix implementation or evidence for the existing exact rows, then rerun stock-Git parity before expanding this documented option"
            bucket = "not implemented / broken / open"
            kind = "doc_option_exact_rows_open"
        elif schema_refs:
            next_action = "write exact stock-Git rows for the implemented parser/handler surface"
            bucket = "implemented but unverified"
            kind = "doc_option_implemented_without_matrix_evidence"
        else:
            next_action = "decide whether to implement, explicitly defer, or prove stock-compatible rejection"
            bucket = "not implemented / broken / open"
            kind = "doc_option_not_in_zmin_schema"
        remaining.append(
            {
                "item_id": stable_id("doc-option", [command, spelling, option["doc"], kind]),
                "bucket": bucket,
                "item_kind": kind,
                "command": command,
                "option": spelling,
                "value": "<unexpanded>",
                "combination": "<unexpanded>",
                "repo_state": "<unexpanded>",
                "transport": "<unexpanded>",
                "platform": "<unexpanded>",
                "implementation_source": "upstream Git docs plus zmin schema",
                "evidence_source": option["doc"],
                "evidence_kind": "upstream_git_docs",
                "source_detail": f"{option['doc']} status_counts={dict(statuses)} schema_args={len(schema_refs)}",
                "next_action": next_action,
                "notes": "documented option seed is not a complete behavior denominator",
            }
        )

    for guard in hard_fails:
        if guard["classification_status"] == "unclassified":
            remaining.append(
                {
                    "item_id": stable_id("hard-fail", [guard["file"], guard["line"], guard["text"]]),
                    "bucket": "not implemented / broken / open",
                    "item_kind": "source_hard_fail_unclassified",
                    "command": "<unknown>",
                    "option": "<source-guard>",
                    "value": "<unclassified>",
                    "combination": "<unclassified>",
                    "repo_state": "<unclassified>",
                    "transport": "<unclassified>",
                    "platform": "<unclassified>",
                    "implementation_source": "source hard-fail scan",
                    "evidence_source": f"{guard['file']}:{guard['line']}",
                    "evidence_kind": "source_scan",
                    "source_detail": guard["text"],
                    "next_action": "classify as Git-supported gap, invalid-input parity, deferral or Zmin-only extension before adding rows",
                    "notes": "unclassified guard mapping",
                }
            )

    extension_deferred = zmin_extension_rows(extension_contract) + deferred_option_rows
    relationship_rows = zmin_relationship_rows(extension_contract)

    oracle_inventory_path = root / "docs/cli/existing_oracle_test_inventory.tsv"
    oracle_inventory = read_tsv(oracle_inventory_path)
    oracle_rows = []
    for row in oracle_inventory:
        oracle_rows.append(
            {
                "item_id": stable_id("oracle", [row["evidence"], row["inventory_status"]]),
                "bucket": "oracle evidence layer",
                "item_kind": "stock_oracle_test_function",
                "command": row["command_hints"] or "<none>",
                "option": row["evidence"],
                "value": row["inventory_status"],
                "combination": "<test-function>",
                "repo_state": "<from-test>",
                "transport": "<from-test>",
                "platform": "all",
                "implementation_source": "existing oracle inventory",
                "evidence_source": row["evidence"],
                "evidence_kind": "stock_git_oracle_test",
                "source_detail": f"{row['file']} body_lines={row['body_lines']} evidence_refs={row['evidence_refs']}",
                "next_action": "use only after independent command/docs/schema census identifies the row shape",
                "notes": "not a primary backlog source",
            }
        )

    hard_fail_rows = []
    for guard in hard_fails:
        hard_fail_rows.append(
            {
                "item_id": stable_id("hard-fail-all", [guard["file"], guard["line"], guard["text"]]),
                "bucket": "source hard-fail scan",
                "item_kind": "source_hard_fail",
                "command": "<unknown>",
                "option": "<source-guard>",
                "value": guard["classification_status"],
                "combination": "<source>",
                "repo_state": "<source>",
                "transport": "<source>",
                "platform": "<source>",
                "implementation_source": "source hard-fail scan",
                "evidence_source": f"{guard['file']}:{guard['line']}",
                "evidence_kind": "source_scan",
                "source_detail": guard["text"],
                "next_action": "verify documented classifications and classify unclassified hits",
                "notes": f"{guard['classification_status']} guard mapping",
            }
        )

    all_items = verified + invalid_input + implemented_unverified + remaining + extension_deferred + oracle_rows + hard_fail_rows

    summary_counter = Counter()
    summary_counter["complete_command_matrices"] = len(complete_commands)
    summary_counter["complete_doc_option_pairs"] = len(complete_option_pairs)
    summary_counter["current_upstream_command_seed_count"] = len(commands)
    summary_counter["authoritative_upstream_shell_test_denominator"] = contract.authoritative_denominator
    summary_counter["zmin_primary_extension_rows"] = len(extension_contract.primary)
    summary_counter["zmin_relationship_rows"] = len(extension_contract.relationships)
    summary_counter["git_doc_option_seed_rows"] = len(options)
    summary_counter["historical_zmin_schema_commands"] = len(zmin_commands)
    summary_counter["historical_schema_current_command_overlap"] = len(command_set & zmin_commands)
    summary_counter["historical_schema_additional_commands"] = len(additional_commands)
    summary_counter["matrix_rows"] = len(matrices)
    summary_counter["verified_rows"] = len(verified)
    summary_counter["invalid_input_rows"] = len(invalid_input)
    summary_counter["open_or_partial_matrix_rows"] = len(open_exact)
    summary_counter["exact_open_local_oracle_unavailable_rows"] = len(exact_open_oracle_gaps)
    summary_counter["implemented_but_unverified_rows"] = len(implemented_unverified)
    summary_counter["remaining_to_fix_or_verify_rows"] = len(remaining)
    summary_counter["extension_or_deferred_rows"] = len(extension_deferred)
    summary_counter["oracle_evidence_layer_rows"] = len(oracle_rows)
    summary_counter["hard_fail_scan_rows"] = len(hard_fail_rows)
    summary_counter["hard_fail_scan_unclassified_rows"] = sum(
        1 for guard in hard_fails if guard["classification_status"] == "unclassified"
    )
    summary_counter["all_census_rows"] = len(all_items)

    summary_rows = [
        {
            "metric": metric,
            "count": str(count),
            "note": note,
        }
        for metric, count, note in [
            ("complete_command_matrices", summary_counter["complete_command_matrices"], "reviewed commands whose full behavior matrix is finished"),
            ("complete_doc_option_pairs", summary_counter["complete_doc_option_pairs"], "reviewed documented command-option pairs whose full behavior matrix is finished"),
            ("upstream_git_tag", contract.tag, "frozen current Git tag from the committed contract"),
            ("upstream_git_commit", contract.commit, "frozen current Git commit from the committed contract"),
            ("current_upstream_command_seed_count", summary_counter["current_upstream_command_seed_count"], "current command-list seed count"),
            ("authoritative_upstream_shell_test_denominator", summary_counter["authoritative_upstream_shell_test_denominator"], "current nondeprecated upstream shell-test denominator"),
            ("zmin_primary_extension_rows", summary_counter["zmin_primary_extension_rows"], "primary Zmin extension rows only"),
            ("zmin_relationship_rows", summary_counter["zmin_relationship_rows"], "relationship evidence rows, never extension denominator"),
            ("git_doc_option_seed_rows", summary_counter["git_doc_option_seed_rows"], "documented option spelling seed, not final denominator"),
            ("historical_zmin_schema_commands", summary_counter["historical_zmin_schema_commands"], "commands emitted by the historical Zmin schema evidence input"),
            ("historical_schema_current_command_overlap", summary_counter["historical_schema_current_command_overlap"], "historical schema names overlapping the current command seed"),
            ("historical_schema_additional_commands", summary_counter["historical_schema_additional_commands"], "historical schema names outside the current command seed"),
            ("matrix_rows", summary_counter["matrix_rows"], "existing behavior rows used as evidence layer"),
            ("verified_rows", summary_counter["verified_rows"], "closed exact behavior rows"),
            ("invalid_input_rows", summary_counter["invalid_input_rows"], "stock-compatible rejection rows"),
            ("open_or_partial_matrix_rows", summary_counter["open_or_partial_matrix_rows"], "exact rows still open or partial"),
            ("exact_open_local_oracle_unavailable_rows", summary_counter["exact_open_local_oracle_unavailable_rows"], "exact open rows blocked by missing local stock Git oracle command/tool"),
            ("implemented_but_unverified_rows", summary_counter["implemented_but_unverified_rows"], "schema args without exact matrix evidence"),
            ("remaining_to_fix_or_verify_rows", summary_counter["remaining_to_fix_or_verify_rows"], "doc-option expansion, exact opens and unclassified guards"),
            ("extension_or_deferred_rows", summary_counter["extension_or_deferred_rows"], "Zmin-only or deferred/non-Git scope items"),
            ("oracle_evidence_layer_rows", summary_counter["oracle_evidence_layer_rows"], "existing oracle inventory rows, not primary backlog"),
            ("hard_fail_scan_rows", summary_counter["hard_fail_scan_rows"], "source guard hits"),
            ("hard_fail_scan_unclassified_rows", summary_counter["hard_fail_scan_unclassified_rows"], "source guard hits not matched to classification docs"),
            ("all_census_rows", summary_counter["all_census_rows"], "union of generated census output rows"),
        ]
    ]

    return {
        "summary": summary_rows,
        "verified_behavior": verified,
        "invalid_input_parity": invalid_input,
        "exact_open_oracle_gaps": exact_open_oracle_gaps,
        "implemented_but_unverified": implemented_unverified,
        "remaining_to_fix_or_verify": remaining,
        "zmin_extension_or_deferred": extension_deferred,
        "zmin_api_relationships": relationship_rows,
        "oracle_evidence_layer": oracle_rows,
        "hard_fail_scan": hard_fail_rows,
        "all_items": all_items,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", default=".", help="repository root")
    parser.add_argument(
        "--historical-zmin-schema-json",
        type=Path,
        help="historical v2-47 Zmin schema evidence input",
    )
    parser.add_argument(
        "--validate-contracts",
        action="store_true",
        help="validate current contracts and v2.55 source identity without generating census output",
    )
    parser.add_argument(
        "--out-dir",
        default="docs/cli/census",
        help="output directory relative to the repository root",
    )
    args = parser.parse_args()

    root = Path(args.root).resolve()
    contract = load_current_contract(root)
    extension_contract = load_extension_contract(root)
    source = resolve_current_source(root, contract)
    if args.validate_contracts:
        command_seed_count = len(command_list_from_cache(source))
        print(f"upstream_git_tag={contract.tag}")
        print(f"upstream_git_commit={contract.commit}")
        print(f"current_upstream_command_seed_count={command_seed_count}")
        print(f"authoritative_upstream_shell_test_denominator={contract.authoritative_denominator}")
        print(f"zmin_primary_extension_rows={len(extension_contract.primary)}")
        print(f"zmin_relationship_rows={len(extension_contract.relationships)}")
        print("contracts=pass")
        return 0
    out_dir = root / args.out_dir
    census = make_census(root, contract, source, args.historical_zmin_schema_json)
    command_progress = run_text(
        [str(root / "tools/git-compat-command-summary.sh"), "--tsv"],
        root,
        source.environment(contract.tag),
    )

    write_tsv(out_dir / "summary.tsv", ["metric", "count", "note"], census["summary"])
    (out_dir / "command_progress.tsv").write_text(command_progress)
    for name in [
        "verified_behavior",
        "invalid_input_parity",
        "exact_open_oracle_gaps",
        "implemented_but_unverified",
        "remaining_to_fix_or_verify",
        "zmin_extension_or_deferred",
        "oracle_evidence_layer",
        "hard_fail_scan",
        "all_items",
    ]:
        write_tsv(out_dir / f"{name}.tsv", OUTPUT_COLUMNS, census[name])
    write_tsv(
        out_dir / "zmin_api_relationships.tsv",
        OUTPUT_COLUMNS,
        census["zmin_api_relationships"],
    )

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
