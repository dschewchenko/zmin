#!/usr/bin/env python3
"""Inventory stock-Git oracle tests and their TSV evidence references."""

from __future__ import annotations

import argparse
import csv
import re
from collections import Counter
from pathlib import Path


ORACLE_PATTERNS = (
    "git(",
    "git_args(",
    "git_with_stdin",
    "git_with_env",
    'command_output_with_env("git"',
    'Command::new("git")',
    "stock_git",
    "stock ",
)

ZMIN_PATTERNS = (
    "run_zmin",
    "zmin_bin",
    "assert_zmin",
    "zmin_repo",
    "Zmin",
)

EVIDENCE_REF_PATTERN = re.compile(r"[A-Za-z0-9_]+::[A-Za-z0-9_]+")

def parse_test_functions(path: Path) -> list[tuple[str, str]]:
    text = path.read_text(errors="replace")
    functions: list[tuple[str, str]] = []
    for match in re.finditer(r"(?m)^\s*#\[test\]\s*\n\s*fn\s+([A-Za-z0-9_]+)\s*\(", text):
        name = match.group(1)
        start = text.find("{", match.end())
        if start < 0:
            continue
        depth = 0
        end = start
        for index, char in enumerate(text[start:], start):
            if char == "{":
                depth += 1
            elif char == "}":
                depth -= 1
                if depth == 0:
                    end = index + 1
                    break
        functions.append((name, text[start:end]))
    return functions


def collect_evidence_refs(root: Path) -> Counter[str]:
    refs: Counter[str] = Counter()
    for matrix in sorted((root / "docs/cli/matrices").glob("*_v2_47.tsv")):
        with matrix.open(newline="") as handle:
            for row in csv.DictReader(handle, delimiter="\t"):
                evidence = row.get("evidence", "")
                for evidence_ref in EVIDENCE_REF_PATTERN.findall(evidence):
                    refs[evidence_ref] += 1
                if "::" in evidence and not EVIDENCE_REF_PATTERN.search(evidence):
                    refs[evidence] += 1

    classification_docs = [
        root / "docs/cli/zmin_extensions_inventory.md",
        root / "docs/cli/oracle_test_deferrals.md",
    ]
    for classification_doc in classification_docs:
        if not classification_doc.exists():
            continue
        text = classification_doc.read_text(errors="replace")
        for evidence in EVIDENCE_REF_PATTERN.findall(text):
            refs[evidence] += 1
    return refs


def collect_commands(root: Path) -> list[str]:
    commands_path = root / "crates/zmin-cli/src/compat/v2_47_commands.txt"
    return [line.strip() for line in commands_path.read_text().splitlines() if line.strip()]


def command_hints(file_name: str, function_name: str, commands: list[str]) -> str:
    haystack = f"{file_name} {function_name}".replace("-", "_")
    hints = []
    for command in commands:
        token = command.replace("-", "_")
        if re.search(rf"(^|_)({re.escape(token)})(_|$)", haystack):
            hints.append(command)
    return ",".join(hints)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", default=".", help="repository root")
    args = parser.parse_args()

    root = Path(args.root).resolve()
    evidence_refs = collect_evidence_refs(root)
    commands = collect_commands(root)

    writer = csv.writer(__import__("sys").stdout, delimiter="\t", lineterminator="\n")
    writer.writerow(
        [
            "evidence",
            "file",
            "test",
            "body_lines",
            "command_hints",
            "evidence_refs",
            "inventory_status",
        ]
    )

    for path in sorted((root / "crates/zmin-cli/tests").glob("*.rs")):
        for test_name, body in parse_test_functions(path):
            has_oracle = any(pattern in body for pattern in ORACLE_PATTERNS)
            has_zmin = any(pattern in body for pattern in ZMIN_PATTERNS)
            if not (has_oracle and has_zmin):
                continue
            evidence = f"{path.stem}::{test_name}"
            evidence_ref_count = evidence_refs[evidence]
            status = "represented" if evidence_ref_count else "missing_or_unclassified"
            writer.writerow(
                [
                    evidence,
                    path.name,
                    test_name,
                    len(body.splitlines()),
                    command_hints(path.name, test_name, commands),
                    evidence_ref_count,
                    status,
                ]
            )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
