#!/usr/bin/env python3
"""Focused contract-authority tests for git-compat-census.py."""

from __future__ import annotations

import csv
import contextlib
import importlib.util
import io
import os
import re
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "tools/git-compat-census.py"

INDEPENDENT_CURRENT_GIT_NON_EXTENSION_NAMES = frozenset(
    {
        "backfill",
        "diff-pairs",
        "format-rev",
        "history",
        "last-modified",
        "repo",
        "url-parse",
    }
)


def load_census_module():
    spec = importlib.util.spec_from_file_location("git_compat_census_under_test", SCRIPT)
    if spec is None or spec.loader is None:
        raise RuntimeError("cannot import census module")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


CENSUS = load_census_module()


class ContractFixture:
    def __init__(self, temporary: tempfile.TemporaryDirectory[str]) -> None:
        self.temporary = temporary
        self.root = Path(temporary if isinstance(temporary, (str, Path)) else temporary.name)
        (self.root / "tools").mkdir()
        shutil.copy2(ROOT / "tools/git-upstream-compat-contract.tsv", self.root / "tools/git-upstream-compat-contract.tsv")
        shutil.copy2(ROOT / "tools/zmin-extensions-contract.tsv", self.root / "tools/zmin-extensions-contract.tsv")
        self.cache = self.root / "source" / "git-v2.55.0"
        self.cache.mkdir(parents=True)
        (self.cache / "Documentation").mkdir()
        self.archive_sha256 = "72923418db7b26dfddc21e2268660c5118e560bdfaa09b4489b67b38e9b69c49"
        (self.cache / ".zmin-pristine-source.sha256").write_text(
            f"{self.archive_sha256}\n",
            encoding="utf-8",
        )
        (self.cache / "command-list.txt").write_text(
            "git-log	common\ngit-rev-list	common\n",
            encoding="utf-8",
        )

    def environment(self) -> dict[str, str]:
        return {
            "ZMIN_GIT_DOC_CACHE": str(self.cache),
            "ZMIN_GIT_COMMAND_LIST": str(self.cache / "command-list.txt"),
            "ZMIN_GIT_SOURCE_ARCHIVE_SHA256": self.archive_sha256,
        }

    def mutate_contract(self, transform) -> None:
        path = self.root / "tools/git-upstream-compat-contract.tsv"
        with path.open(newline="") as handle:
            rows = list(csv.reader(handle, delimiter="\t"))
        transform(rows)
        with path.open("w", newline="") as handle:
            csv.writer(handle, delimiter="\t", lineterminator="\n").writerows(rows)

    def mutate_extensions(self, transform) -> None:
        path = self.root / "tools/zmin-extensions-contract.tsv"
        with path.open(newline="") as handle:
            rows = list(csv.reader(handle, delimiter="\t"))
        transform(rows)
        with path.open("w", newline="") as handle:
            csv.writer(handle, delimiter="\t", lineterminator="\n").writerows(rows)


class CensusAuthorityTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory(prefix="zmin-census-contract-")
        self.fixture = ContractFixture(self.temporary)

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def assert_failure(self, callback, diagnostic: str) -> None:
        stderr = io.StringIO()
        with contextlib.redirect_stderr(stderr):
            with self.assertRaises(SystemExit) as raised:
                callback()
        self.assertEqual(raised.exception.code, 1)
        self.assertIn(f"error: {diagnostic}", stderr.getvalue())

    def test_canonical_contracts_and_counts(self) -> None:
        contract = CENSUS.load_current_contract(self.fixture.root)
        extensions = CENSUS.load_extension_contract(self.fixture.root)
        self.assertEqual(contract.tag, "v2.55.0")
        self.assertEqual(contract.commit, "e9019fcafe0040228b8631c30f97ae1adb61bcdc")
        self.assertEqual(
            contract.values["source_identity_policy"],
            "archive_sha256_exact; tag_commit_declared; source_manifest_exact; no_checkout_fallback",
        )
        self.assertEqual(contract.authoritative_denominator, 1045)
        self.assertEqual(len(extensions.primary), 40)
        self.assertEqual(len(extensions.relationships), 7)

    def test_current_contract_missing_duplicate_and_malformed_rows(self) -> None:
        cases = [
            ("missing", lambda rows: rows.pop(2), "current contract key set drift"),
            ("duplicate", lambda rows: rows.append(rows[2][:]), "duplicate current contract key"),
            ("malformed", lambda rows: rows.append(["broken"]), "malformed current contract row"),
        ]
        for name, transform, diagnostic in cases:
            with self.subTest(name=name):
                with tempfile.TemporaryDirectory(prefix="zmin-census-contract-shape-") as temporary:
                    fixture = ContractFixture(temporary)
                    fixture.mutate_contract(transform)
                    self.assert_failure(
                        lambda: CENSUS.load_current_contract(fixture.root),
                        diagnostic,
                    )

    def test_current_contract_immutable_values_and_arithmetic(self) -> None:
        mutations = [
            ("tag", "upstream_git_tag", "v2.47.1", "current contract value drift: upstream_git_tag"),
            ("commit", "upstream_git_commit", "0" * 40, "current contract value drift: upstream_git_commit"),
            ("archive-url", "upstream_archive_url", "https://example.invalid/git.tar.gz", "current contract value drift: upstream_archive_url"),
            ("archive-sha", "upstream_archive_sha256", "0" * 64, "current contract value drift: upstream_archive_sha256"),
            ("source-policy", "source_identity_policy", "archive-only", "current contract value drift: source_identity_policy"),
            ("manifest", "authoritative_manifest", "core-only", "current contract value drift: authoritative_manifest"),
            ("exclusion", "deprecated_removed_groups", "t5322|t5322|1", "current contract value drift: deprecated_removed_groups"),
            ("total", "upstream_top_level_shell_tests", "1047", "current contract metric drift: upstream_top_level_shell_tests"),
            ("denominator", "authoritative_upstream_test_denominator", "1046", "current contract metric drift: authoritative_upstream_test_denominator"),
            ("evidence", "required_evidence", "upstream shell suite", "current contract value drift: required_evidence"),
        ]
        for name, key, value, diagnostic in mutations:
            with self.subTest(name=name):
                with tempfile.TemporaryDirectory(prefix="zmin-census-mutation-") as temporary:
                    fixture = ContractFixture(temporary)
                    fixture.mutate_contract(
                        lambda rows, key=key, value=value: next(row.__setitem__(1, value) for row in rows if row[0] == key)
                    )
                    self.assert_failure(
                        lambda: CENSUS.load_current_contract(fixture.root),
                        diagnostic,
                    )

    def test_external_groups_are_required_and_arithmetic_is_checked(self) -> None:
        self.fixture.mutate_contract(
            lambda rows: next(row.__setitem__(1, "git-svn|t91|68") for row in rows if row[0] == "external_current_groups")
        )
        self.assert_failure(
            lambda: CENSUS.load_current_contract(self.fixture.root),
            "current contract value drift: external_current_groups",
        )

    def test_extension_contract_positive_and_relationships_are_separate(self) -> None:
        contract = CENSUS.load_current_contract(self.fixture.root)
        extensions = CENSUS.load_extension_contract(self.fixture.root)
        names = CENSUS.zmin_extension_command_names(extensions)
        contract_current_names = {
            value.split("|", 1)[0]
            for key, value in contract.values.items()
            if key.startswith("current_git_non_extension_") and key != "current_git_non_extension_count"
        }
        self.assertEqual(CENSUS.CURRENT_GIT_NON_EXTENSION_NAMES, INDEPENDENT_CURRENT_GIT_NON_EXTENSION_NAMES)
        self.assertEqual(contract_current_names, INDEPENDENT_CURRENT_GIT_NON_EXTENSION_NAMES)
        self.assertEqual(
            {name for name in names if name.startswith("hooks-")},
            {"hooks-init", "hooks-add", "hooks-list", "hooks-remove", "hooks-run"},
        )
        self.assertTrue(names.isdisjoint(INDEPENDENT_CURRENT_GIT_NON_EXTENSION_NAMES))
        self.assertNotIn("repo-info", names)
        self.assertNotIn("repo-structure", names)
        self.assertNotIn(("repo", "info"), CENSUS.zmin_extension_option_keys(extensions))
        self.assertNotIn(("repo", "structure"), CENSUS.zmin_extension_option_keys(extensions))
        self.assertTrue(all(row["status"] == "stable" for row in extensions.primary))
        primary_output = CENSUS.zmin_extension_rows(extensions)
        relationship_output = CENSUS.zmin_relationship_rows(extensions)
        self.assertEqual(len(primary_output), 40)
        self.assertTrue(all(row["bucket"] == "Zmin-only extension surface" for row in primary_output))
        self.assertTrue(all(row["item_kind"] != "api_relationship" for row in primary_output))
        self.assertTrue(all(row["combination"] != "relationship" for row in primary_output))
        self.assertEqual(len(relationship_output), 7)
        self.assertEqual(
            {row["item_id"] for row in relationship_output},
            set(CENSUS.EXPECTED_RELATIONSHIP_IDS),
        )
        self.assertTrue(all(row["bucket"] == "API relationship evidence" for row in relationship_output))
        self.assertTrue(all("extension" not in row["bucket"] and "exclusion" not in row["bucket"] for row in relationship_output))
        relationships = {row["id"]: row for row in extensions.relationships}
        self.assertEqual(set(relationships), set(CENSUS.EXPECTED_RELATIONSHIP_IDS))
        for row_id, row in relationships.items():
            self.assertEqual(row["kind"], "relationship")
            self.assertEqual(row["parent"], "hooks" if row_id.startswith("relationship.hooks.") else "repo")
            self.assertEqual(row["status"], "stable")
        self.assertEqual(
            {row["id"] for row in extensions.relationships if row["parent"] == "hooks"},
            {
                "relationship.hooks.init",
                "relationship.hooks.add",
                "relationship.hooks.list",
                "relationship.hooks.remove",
                "relationship.hooks.run",
            },
        )
        self.assertEqual(
            {row["id"] for row in extensions.relationships if row["parent"] == "repo"},
            {"relationship.repo.info", "relationship.repo.structure"},
        )
        expected_relationship_sources = {
            "relationship.hooks.init": (
                "hooks",
                "init",
                "crates/zmin-cli-schema/src/lib.rs;crates/zmin-cli/tests/git_admin_tools_compat.rs",
                "Init,",
                "schema.managed-hooks",
            ),
            "relationship.hooks.add": (
                "hooks",
                "add",
                "crates/zmin-cli-schema/src/lib.rs;crates/zmin-cli/tests/git_admin_tools_compat.rs",
                "Add {",
                "schema.managed-hooks",
            ),
            "relationship.hooks.list": (
                "hooks",
                "list",
                "crates/zmin-cli-schema/src/lib.rs;crates/zmin-cli/tests/git_admin_tools_compat.rs",
                "List,",
                "schema.managed-hooks",
            ),
            "relationship.hooks.remove": (
                "hooks",
                "remove",
                "crates/zmin-cli-schema/src/lib.rs;crates/zmin-cli/tests/git_admin_tools_compat.rs",
                "Remove {",
                "schema.managed-hooks",
            ),
            "relationship.hooks.run": (
                "hooks",
                "run",
                "crates/zmin-cli-schema/src/lib.rs;crates/zmin-cli/tests/git_admin_tools_compat.rs",
                "Run {",
                "schema.managed-hooks",
            ),
            "relationship.repo.info": (
                "repo",
                "info",
                "tools/git-upstream-compat-contract.tsv",
                "current_git_non_extension_repo",
                "contract.current-git-repo",
            ),
            "relationship.repo.structure": (
                "repo",
                "structure",
                "tools/git-upstream-compat-contract.tsv",
                "current_git_non_extension_repo",
                "contract.current-git-repo",
            ),
        }
        expected_relationship_output = {}
        for relationship_id, (parent, surface, evidence, anchor, scope) in expected_relationship_sources.items():
            expected_relationship_output[relationship_id] = {
                "item_id": relationship_id,
                "bucket": "API relationship evidence",
                "item_kind": "api_relationship",
                "command": parent,
                "option": surface,
                "value": "stable",
                "combination": "relationship",
                "repo_state": "<not-applicable>",
                "transport": "<not-applicable>",
                "platform": "all",
                "implementation_source": "committed Zmin API relationship contract",
                "evidence_source": evidence,
                "evidence_kind": "relationship_contract",
                "source_detail": f"id={relationship_id} scope={scope}",
                "next_action": "relationship metadata only; not a scope classification",
                "notes": anchor,
            }
        self.assertEqual(
            {row["item_id"]: row for row in relationship_output},
            expected_relationship_output,
        )

    def test_extension_contract_malformed_duplicate_missing_and_count_drift(self) -> None:
        cases = [
            ("malformed", lambda rows: rows.append(["primary"]), "malformed extension contract row"),
            ("duplicate", lambda rows: rows.append(rows[1][:]), "duplicate extension contract id"),
            ("missing", lambda rows: rows.pop(1), "primary extension id set drift"),
            ("primary-39", lambda rows: rows.pop(1), "primary extension id set drift"),
            ("relationship-6", lambda rows: rows.pop(), "extension relationship id set drift"),
            ("primary-41", lambda rows: rows.append(["primary", "command.extra", "command", "-", "extra", "fixture", "stable", "Extra", "fixture"]), "primary extension id set drift"),
            ("relationship-8", lambda rows: rows.append(["relationship", "relationship.repo.extra", "relationship", "repo", "extra", "fixture", "stable", "extra", "fixture"]), "extension relationship id set drift"),
        ]
        for name, transform, diagnostic in cases:
            with self.subTest(name=name):
                with tempfile.TemporaryDirectory(prefix="zmin-census-extension-") as temporary:
                    fixture = ContractFixture(temporary)
                    fixture.mutate_extensions(transform)
                    self.assert_failure(
                        lambda: CENSUS.load_extension_contract(fixture.root),
                        diagnostic,
                    )

    def test_extension_wrong_kind_parent_status_and_current_command_rejected(self) -> None:
        mutations = [
            ("kind", lambda rows: next(row.__setitem__(2, "option") for row in rows if row[1] == "command.hooks"), "extension row shape drift: command.hooks"),
            ("parent", lambda rows: next(row.__setitem__(3, "repo") for row in rows if row[1] == "option.clone.instant"), "extension row shape drift: option.clone.instant"),
            ("status", lambda rows: next(row.__setitem__(6, "deferred") for row in rows if row[1] == "command.lfs"), "extension row status drift: command.lfs"),
        ]
        current_names = sorted(INDEPENDENT_CURRENT_GIT_NON_EXTENSION_NAMES)
        mutations.extend(
            (
                f"current-{name}",
                lambda rows, name=name: next(
                    (row.__setitem__(1, f"command.{name}"), row.__setitem__(4, name))
                    for row in rows
                    if row[1] == "command.save"
                ),
                f"current Git command cannot be a Zmin extension: {name}",
            )
            for name in current_names
        )
        for name, transform, diagnostic in mutations:
            with self.subTest(name=name):
                with tempfile.TemporaryDirectory(prefix="zmin-census-extension-") as temporary:
                    fixture = ContractFixture(temporary)
                    fixture.mutate_extensions(transform)
                    self.assert_failure(
                        lambda: CENSUS.load_extension_contract(fixture.root),
                        diagnostic,
                    )

    def test_markdown_is_irrelevant_and_no_cycle_is_static(self) -> None:
        docs = self.fixture.root / "docs/cli"
        docs.mkdir(parents=True)
        (docs / "zmin_extensions_inventory.md").write_text("| git repo | fake extension |\n")
        extensions = CENSUS.load_extension_contract(self.fixture.root)
        self.assertEqual(len(extensions.primary), 40)
        source = SCRIPT.read_text(encoding="utf-8")
        self.assertNotIn("zmin_extensions_inventory.md", source)
        for downstream in (
            "git-upstream-compat-contract-gate.sh",
            "git-upstream-compat-audit.sh",
            "git-upstream-compat-manifest.sh",
        ):
            self.assertNotIn(downstream, source)
        self.assertNotIn("git-upstream-compat-contract-gate.sh", source)
        self.assertNotIn("--baseline", source)
        self.assertNotIn("git_2_47", source)
        invocation = re.compile(r"(?:^|[\s\"'/])git-compat-census\.py(?:[\s\"']|$)")
        for caller_name in (
            "git-upstream-compat-contract-gate.sh",
            "git-upstream-compat-audit.sh",
            "git-upstream-compat-manifest.sh",
        ):
            caller = ROOT / "tools" / caller_name
            body = "\n".join(
                line for line in caller.read_text(encoding="utf-8").splitlines()
                if not line.lstrip().startswith("#")
            )
            self.assertIsNone(invocation.search(body), caller_name)
        self.assertIn('"zmin_api_relationships.tsv"', SCRIPT.read_text(encoding="utf-8"))

    def test_v247_only_cache_is_rejected_without_fallback(self) -> None:
        with tempfile.TemporaryDirectory(prefix="zmin-census-old-cache-") as temporary:
            old_cache = Path(temporary) / "git-v2.47.1"
            old_cache.mkdir()
            command_list = old_cache / "command-list.txt"
            command_list.write_text("git-log\tcommon\n")
            contract = CENSUS.load_current_contract(self.fixture.root)
            with patch.dict(os.environ, {"ZMIN_GIT_DOC_CACHE": str(old_cache), "ZMIN_GIT_COMMAND_LIST": str(command_list)}, clear=False):
                self.assert_failure(
                    lambda: CENSUS.resolve_current_source(self.fixture.root, contract),
                    "Git source root basename does not match v2.55.0",
                )

    def test_current_source_stale_basename_is_rejected_with_valid_identity(self) -> None:
        with tempfile.TemporaryDirectory(prefix="zmin-census-stale-basename-") as temporary:
            stale_cache = Path(temporary) / "git-v2.55.0-stale"
            shutil.copytree(self.fixture.cache, stale_cache)
            command_list = stale_cache / "command-list.txt"
            contract = CENSUS.load_current_contract(self.fixture.root)
            with patch.dict(
                os.environ,
                {"ZMIN_GIT_DOC_CACHE": str(stale_cache), "ZMIN_GIT_COMMAND_LIST": str(command_list)},
                clear=False,
            ):
                self.assert_failure(
                    lambda: CENSUS.resolve_current_source(self.fixture.root, contract),
                    "Git source root basename does not match v2.55.0",
                )

    def test_current_source_external_command_list_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory(prefix="zmin-census-external-command-list-") as temporary:
            external_command_list = Path(temporary) / "command-list.txt"
            external_command_list.write_text("git-log\tcommon\n", encoding="utf-8")
            contract = CENSUS.load_current_contract(self.fixture.root)
            with patch.dict(
                os.environ,
                {
                    "ZMIN_GIT_DOC_CACHE": str(self.fixture.cache),
                    "ZMIN_GIT_COMMAND_LIST": str(external_command_list),
                },
                clear=False,
            ):
                self.assert_failure(
                    lambda: CENSUS.resolve_current_source(self.fixture.root, contract),
                    "Git v2.55.0 command-list must be the validated source command-list.txt",
                )

    def test_stale_current_looking_cache_marker_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory(prefix="zmin-census-stale-current-") as temporary:
            stale_cache = Path(temporary) / "git-v2.55.0"
            stale_cache.mkdir()
            command_list = stale_cache / "command-list.txt"
            command_list.write_text("git-log\tcommon\n", encoding="utf-8")
            (stale_cache / ".zmin-pristine-source.sha256").write_text("0" * 64 + "\n", encoding="utf-8")
            contract = CENSUS.load_current_contract(self.fixture.root)
            with patch.dict(
                os.environ,
                {"ZMIN_GIT_DOC_CACHE": str(stale_cache), "ZMIN_GIT_COMMAND_LIST": str(command_list)},
                clear=False,
            ):
                self.assert_failure(
                    lambda: CENSUS.resolve_current_source(self.fixture.root, contract),
                    "Git v2.55.0 source identity mismatch",
                )

    def test_current_cache_and_command_seed_are_explicit(self) -> None:
        contract = CENSUS.load_current_contract(self.fixture.root)
        with patch.dict(os.environ, self.fixture.environment(), clear=False):
            source = CENSUS.resolve_current_source(self.fixture.root, contract)
        self.assertEqual(CENSUS.command_list_from_cache(source), ["log", "rev-list"])
        self.assertEqual(
            source.environment(contract.tag)["ZMIN_GIT_SOURCE_ARCHIVE_SHA256"],
            self.fixture.archive_sha256,
        )

    def test_option_seed_propagates_complete_current_source_environment(self) -> None:
        contract = CENSUS.load_current_contract(self.fixture.root)
        with patch.dict(os.environ, self.fixture.environment(), clear=False):
            source = CENSUS.resolve_current_source(self.fixture.root, contract)
        captured: dict[str, str] = {}

        def fake_run_text(command, cwd, environment=None):
            captured.update(environment or {})
            self.assertEqual(command, [str(self.fixture.root / "tools/git-compat-option-inventory.sh")])
            self.assertEqual(cwd, self.fixture.root)
            return "command\toption\tdoc\nlog\t--follow\tgit-log.adoc\n"

        with patch.object(CENSUS, "run_text", side_effect=fake_run_text):
            rows = CENSUS.option_seed_from_docs(self.fixture.root, source, contract)
        self.assertEqual(rows, [{"command": "log", "option": "--follow", "doc": "git-log.adoc"}])
        self.assertEqual(captured, source.environment(contract.tag))

    def test_cli_fixture_reports_distinct_current_metrics(self) -> None:
        environment = os.environ.copy()
        environment.update(self.fixture.environment())
        result = subprocess.run(
            [sys.executable, str(SCRIPT), "--root", str(self.fixture.root), "--validate-contracts"],
            cwd=ROOT,
            env=environment,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("upstream_git_tag=v2.55.0", result.stdout)
        self.assertIn("current_upstream_command_seed_count=2", result.stdout)
        self.assertIn("authoritative_upstream_shell_test_denominator=1045", result.stdout)
        self.assertIn("zmin_primary_extension_rows=40", result.stdout)
        self.assertIn("zmin_relationship_rows=7", result.stdout)
        self.assertNotIn("git_2_47", result.stdout)


if __name__ == "__main__":
    unittest.main(verbosity=2)
