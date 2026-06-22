#!/usr/bin/env python3
"""Generate the Git 2.47.1 compatibility census/checklist.

The census starts from independent source layers:

- upstream Git command and documentation option seeds
- the Zmin CLI schema produced by `zmin compat --profile v2-47 --format json`
- existing behavior matrices
- the existing stock-oracle test inventory as an evidence layer
- Zmin extension and deferral docs
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
from pathlib import Path
from typing import Iterable


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
EVIDENCE_REF_PATTERN = re.compile(r"[A-Za-z0-9_]+::[A-Za-z0-9_]+")
LONG_OPTION_PATTERN = re.compile(r"(?<!\S)(--[A-Za-z0-9][A-Za-z0-9-]*)(?:[=\s]|$)")
SHORT_OPTION_PATTERN = re.compile(r"(?<!\S)(-[A-Za-z])(?:[=\s]|$)")
IDENTIFIER_PATTERN = re.compile(r"\b[A-Za-z_][A-Za-z0-9_]*\b")


def die(message: str) -> None:
    print(f"error: {message}", file=sys.stderr)
    raise SystemExit(1)


def run_text(command: list[str], cwd: Path) -> str:
    result = subprocess.run(
        command,
        cwd=cwd,
        check=False,
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


def command_list_from_cache(root: Path, baseline: str) -> list[str]:
    cache_dir = root / "target/git-doc-cache" / baseline
    command_list = cache_dir / "command-list.txt"
    if not command_list.exists():
        run_text([str(root / "tools/git-compat-option-inventory.sh")], root)
    if not command_list.exists():
        die(f"upstream command-list cache was not created: {command_list}")

    commands = []
    for line in command_list.read_text().splitlines():
        fields = line.split()
        if not fields or not fields[0].startswith("git-"):
            continue
        commands.append(fields[0][4:])
    return sorted(set(commands))


def option_seed_from_docs(root: Path) -> list[dict[str, str]]:
    output = run_text([str(root / "tools/git-compat-option-inventory.sh")], root)
    rows = list(csv.DictReader(output.splitlines(), delimiter="\t"))
    required = {"command", "option", "doc"}
    if not rows or set(rows[0]) != required:
        die("Git documentation option seed has an unexpected shape")
    return rows


def load_zmin_schema(root: Path, schema_json: Path | None) -> dict:
    if schema_json is not None:
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


def matrix_rows(root: Path) -> list[dict[str, str]]:
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


def hard_fail_scan(root: Path) -> list[dict[str, str]]:
    docs_text = "\n".join(
        path.read_text(errors="replace")
        for path in [
            root / "docs/cli/variant_compatibility_plan.md",
            root / "docs/cli/matrix_row_growth_audit.md",
            root / "docs/cli/zmin_extensions_inventory.md",
            root / "docs/cli/oracle_test_deferrals.md",
        ]
        if path.exists()
    )
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
                documented = hard_fail_is_documented(path, stripped, docs_text)
                rows.append(
                    {
                        "file": str(path.relative_to(root)),
                        "line": str(line_number),
                        "text": stripped,
                        "classification_status": "documented" if documented else "unclassified",
                    }
                )
    return rows


def markdown_cells(line: str) -> list[str]:
    return [
        cell.strip().strip("`")
        for cell in line.strip().strip("|").split("|")
    ]


def zmin_extension_command_names(root: Path) -> set[str]:
    extension_doc = root / "docs/cli/zmin_extensions_inventory.md"
    if not extension_doc.exists():
        die(f"extension inventory is missing: {extension_doc}")

    commands = set()
    for line in extension_doc.read_text(errors="replace").splitlines():
        if not line.startswith("| `zmin "):
            continue
        cells = markdown_cells(line)
        if not cells or cells[0] == "Command":
            continue
        parts = cells[0].split()
        if len(parts) >= 2:
            commands.add(parts[1])
    return commands


def zmin_extension_rows(root: Path) -> list[dict[str, str]]:
    rows = []
    extension_doc = root / "docs/cli/zmin_extensions_inventory.md"
    deferral_doc = root / "docs/cli/oracle_test_deferrals.md"
    if not extension_doc.exists():
        die(f"extension inventory is missing: {extension_doc}")
    if not deferral_doc.exists():
        die(f"oracle deferral inventory is missing: {deferral_doc}")

    for line in extension_doc.read_text(errors="replace").splitlines():
        if not line.startswith("| `"):
            continue
        cells = markdown_cells(line)
        if len(cells) < 4:
            continue
        command = cells[0]
        if command in {"Command", "Layer", "Variable"}:
            continue
        if command.startswith("zmin "):
            item_kind = "zmin_extension_surface"
            option = cells[1] if cells[1].startswith("-") else "<command>"
        elif command.startswith("ZMIN_"):
            item_kind = "zmin_extension_environment"
            option = "<environment>"
        else:
            continue
        evidence = cells[2] if len(cells) > 2 else ""
        rows.append(
            {
                "item_id": stable_id("extension-doc", [command, option, evidence]),
                "bucket": "Zmin-only extension or deferred/non-Git-2.47.1 scope",
                "item_kind": item_kind,
                "command": command,
                "option": option,
                "value": "<empty>",
                "combination": "<none>",
                "repo_state": "<not-applicable>",
                "transport": "<not-applicable>",
                "platform": "all",
                "implementation_source": "zmin extension inventory",
                "evidence_source": evidence,
                "evidence_kind": "classification_evidence",
                "source_detail": str(extension_doc.relative_to(root)),
                "next_action": "keep outside Git compatibility denominator unless explicitly reclassified",
                "notes": cells[-1] if cells else "",
            }
        )

    for line in deferral_doc.read_text(errors="replace").splitlines():
        if not line.startswith("| `"):
            continue
        cells = markdown_cells(line)
        if len(cells) < 3 or cells[0] == "Evidence":
            continue
        rows.append(
            {
                "item_id": stable_id("deferral-doc", [cells[0], cells[1], cells[2]]),
                "bucket": "Zmin-only extension or deferred/non-Git-2.47.1 scope",
                "item_kind": "oracle_deferral",
                "command": "<deferred>",
                "option": cells[0],
                "value": cells[1],
                "combination": "<none>",
                "repo_state": "<deferred>",
                "transport": "<deferred>",
                "platform": "all",
                "implementation_source": "oracle deferral inventory",
                "evidence_source": cells[0],
                "evidence_kind": "deferral",
                "source_detail": str(deferral_doc.relative_to(root)),
                "next_action": "do not count until a Git 2.47.1 oracle row is available",
                "notes": cells[2],
            }
        )

    for doc_path, classification in [
        (extension_doc, "Zmin-only extension or deferred/non-Git-2.47.1 scope"),
        (deferral_doc, "Zmin-only extension or deferred/non-Git-2.47.1 scope"),
    ]:
        text = doc_path.read_text(errors="replace")
        for evidence_ref in sorted(set(EVIDENCE_REF_PATTERN.findall(text))):
            rows.append(
                {
                    "item_id": stable_id("classification", [str(doc_path), evidence_ref]),
                    "bucket": classification,
                    "item_kind": "classified_evidence",
                    "command": "<evidence>",
                    "option": evidence_ref,
                    "value": "<empty>",
                    "combination": "<none>",
                    "repo_state": "<classified>",
                    "transport": "<classified>",
                    "platform": "all",
                    "implementation_source": "classification doc",
                    "evidence_source": evidence_ref,
                    "evidence_kind": "classification_evidence",
                    "source_detail": str(doc_path.relative_to(root)),
                    "next_action": "do not count as Git 2.47.1 behavior row unless reclassified",
                    "notes": "",
                }
            )
    return rows


def make_census(root: Path, baseline: str, schema_json: Path | None) -> dict[str, list[dict[str, str]]]:
    commands = command_list_from_cache(root, baseline)
    command_set = set(commands)
    options = option_seed_from_docs(root)
    schema = load_zmin_schema(root, schema_json)
    zmin_commands, additional_commands, zmin_options = normalize_schema(schema)
    matrices = matrix_rows(root)
    hard_fails = hard_fail_scan(root)
    extension_commands = zmin_extension_command_names(root)

    matrix_options_by_status: dict[tuple[str, str], Counter[str]] = defaultdict(Counter)
    matrix_rows_by_command: Counter[str] = Counter()
    for row in matrices:
        for option in matrix_option_spellings(row):
            matrix_options_by_status[(row["command"], option)][row["zmin_status"]] += 1
        matrix_rows_by_command[row["command"]] += 1

    schema_arg_statuses: dict[tuple[str, str], Counter[str]] = defaultdict(Counter)
    for (command, option), statuses in matrix_options_by_status.items():
        for arg_ref in zmin_options.get((command, option), []):
            schema_arg_statuses[(command, arg_ref["arg_id"])].update(statuses)

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

    implemented_unverified = []
    for (command, option), arg_refs in sorted(zmin_options.items()):
        if command not in command_set:
            continue
        statuses = matrix_options_by_status.get((command, option), Counter())
        for arg_ref in arg_refs:
            arg_statuses = statuses + schema_arg_statuses.get((command, arg_ref["arg_id"]), Counter())
            if (
                arg_statuses["closed"]
                or arg_statuses["invalid-input"]
                or arg_statuses["open"]
                or arg_statuses["partial"]
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
                    "evidence_source": "zmin compat --profile v2-47 --format json",
                    "evidence_kind": "zmin_schema",
                    "source_detail": f"arg_id={arg_ref['arg_id']} required={arg_ref['required']} positional={arg_ref['positional']}",
                    "next_action": "add stock-Git oracle evidence before counting this parser/handler surface",
                    "notes": "schema presence is not compatibility evidence",
                }
            )

    for command in sorted(additional_commands):
        if command in extension_commands:
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
                "evidence_source": "zmin compat --profile v2-47 --format json",
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
                    "source_detail": baseline,
                    "next_action": "seed the command matrix from docs/options/states/transports before fixing behavior",
                    "notes": "entrypoint presence alone is not compatibility",
                }
            )

    for option in options:
        command = option["command"]
        spelling = option["option"]
        statuses = matrix_options_by_status.get((command, spelling), Counter())
        schema_refs = zmin_options.get((command, spelling), [])
        if statuses["closed"] or statuses["invalid-input"]:
            next_action = "expand remaining values, negations, repeated forms, combinations, states, transports and platforms for this documented option"
            bucket = "not implemented / broken / open"
            kind = "doc_option_expansion_required"
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
                    "notes": "",
                }
            )

    extension_deferred = zmin_extension_rows(root)

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
    summary_counter["git_2_47_commands"] = len(commands)
    summary_counter["git_doc_option_seed_rows"] = len(options)
    summary_counter["zmin_schema_commands"] = len(zmin_commands)
    summary_counter["zmin_schema_baseline_commands"] = len(command_set & zmin_commands)
    summary_counter["zmin_schema_additional_commands"] = len(additional_commands)
    summary_counter["matrix_rows"] = len(matrices)
    summary_counter["verified_rows"] = len(verified)
    summary_counter["invalid_input_rows"] = len(invalid_input)
    summary_counter["open_or_partial_matrix_rows"] = len(open_exact)
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
            ("git_2_47_commands", summary_counter["git_2_47_commands"], "upstream command-list seed"),
            ("git_doc_option_seed_rows", summary_counter["git_doc_option_seed_rows"], "documented option spelling seed, not final denominator"),
            ("zmin_schema_commands", summary_counter["zmin_schema_commands"], "commands emitted by zmin compat schema"),
            ("zmin_schema_baseline_commands", summary_counter["zmin_schema_baseline_commands"], "baseline command names present in schema"),
            ("zmin_schema_additional_commands", summary_counter["zmin_schema_additional_commands"], "schema commands outside Git 2.47.1 baseline"),
            ("matrix_rows", summary_counter["matrix_rows"], "existing behavior rows used as evidence layer"),
            ("verified_rows", summary_counter["verified_rows"], "closed exact behavior rows"),
            ("invalid_input_rows", summary_counter["invalid_input_rows"], "stock-compatible rejection rows"),
            ("open_or_partial_matrix_rows", summary_counter["open_or_partial_matrix_rows"], "exact rows still open or partial"),
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
        "implemented_but_unverified": implemented_unverified,
        "remaining_to_fix_or_verify": remaining,
        "zmin_extension_or_deferred": extension_deferred,
        "oracle_evidence_layer": oracle_rows,
        "hard_fail_scan": hard_fail_rows,
        "all_items": all_items,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", default=".", help="repository root")
    parser.add_argument("--baseline", default="v2.47.1", help="Git baseline")
    parser.add_argument("--zmin-schema-json", type=Path, help="precomputed zmin compat JSON")
    parser.add_argument(
        "--out-dir",
        default="docs/cli/census",
        help="output directory relative to the repository root",
    )
    args = parser.parse_args()

    root = Path(args.root).resolve()
    out_dir = root / args.out_dir
    census = make_census(root, args.baseline, args.zmin_schema_json)

    write_tsv(out_dir / "summary.tsv", ["metric", "count", "note"], census["summary"])
    for name in [
        "verified_behavior",
        "invalid_input_parity",
        "implemented_but_unverified",
        "remaining_to_fix_or_verify",
        "zmin_extension_or_deferred",
        "oracle_evidence_layer",
        "hard_fail_scan",
        "all_items",
    ]:
        write_tsv(out_dir / f"{name}.tsv", OUTPUT_COLUMNS, census[name])

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
