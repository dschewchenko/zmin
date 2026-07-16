#!/usr/bin/env python3

from __future__ import annotations

import json
import os
import pathlib
import stat
import subprocess
import sys
import tempfile
import unittest

WRAPPER = pathlib.Path(__file__).with_name("git-gui-capture-wrapper.py")


class GitGuiCaptureWrapperTest(unittest.TestCase):
    def test_capture_is_sanitized_private_and_transparent(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            log = pathlib.Path(directory) / "private" / "capture.jsonl"
            env = os.environ.copy()
            env.update(
                {
                    "ZMIN_GUI_CAPTURE_TARGET": "/usr/bin/true",
                    "ZMIN_GUI_CAPTURE_LOG": str(log),
                    "SSH_AUTH_SOCK": "/private/socket",
                }
            )
            result = subprocess.run(
                [
                    sys.executable,
                    str(WRAPPER),
                    "-c",
                    "credential.helper=top-secret",
                    "--format=%H:%P:%s",
                    "https://user:password@example.test/repo.git?token=secret",
                    "--git-dir=/Users/example/private/repo/.git",
                    "/Users/example/private/repo/file.txt",
                ],
                env=env,
                check=False,
                capture_output=True,
            )
            self.assertEqual(result.returncode, 0, result.stderr.decode())
            serialized = log.read_text(encoding="utf-8")
            self.assertNotIn("top-secret", serialized)
            self.assertNotIn("password", serialized)
            self.assertNotIn("token=secret", serialized)
            record = json.loads(serialized)
            self.assertEqual(record["schema"], 1)
            self.assertEqual(record["argv"][1], "credential.helper=<redacted>")
            self.assertEqual(record["argv"][2], "--format=%H:%P:%s")
            self.assertTrue(record["argv"][3].startswith("https://example.test/path-"))
            self.assertNotIn("repo.git", record["argv"][3])
            self.assertTrue(record["argv"][4].startswith("--git-dir=<path "))
            self.assertTrue(record["argv"][5].startswith("<path "))
            self.assertTrue(record["environment"]["SSH_AUTH_SOCK_present"])
            self.assertEqual(stat.S_IMODE(log.stat().st_mode), 0o600)

    def test_target_is_required_without_fallback(self) -> None:
        result = subprocess.run(
            [sys.executable, str(WRAPPER), "status"],
            env={},
            check=False,
            capture_output=True,
        )
        self.assertEqual(result.returncode, 2)
        self.assertIn(b"ZMIN_GUI_CAPTURE_TARGET", result.stderr)


if __name__ == "__main__":
    unittest.main()
