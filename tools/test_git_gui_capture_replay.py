#!/usr/bin/env python3

from __future__ import annotations

import json
import os
import pathlib
import socket
import subprocess
import sys
import tempfile
import unittest

CAPTURE = pathlib.Path(__file__).with_name("git-gui-capture-wrapper.py")
REPLAY = pathlib.Path(__file__).with_name("git-gui-capture-replay.py")
STOCK_GIT = pathlib.Path("/usr/bin/git")


class GitGuiCaptureReplayTest(unittest.TestCase):
    def capture_status(self, repository: pathlib.Path, log: pathlib.Path) -> None:
        environment = os.environ.copy()
        environment.update(
            {
                "ZMIN_GUI_CAPTURE_TARGET": str(STOCK_GIT),
                "ZMIN_GUI_CAPTURE_LOG": str(log),
            }
        )
        completed = subprocess.run(
            [sys.executable, str(CAPTURE), "status", "--porcelain=v2", "-z"],
            cwd=repository,
            env=environment,
            capture_output=True,
            check=False,
        )
        self.assertEqual(completed.returncode, 0, completed.stderr.decode())

    def replay(
        self,
        repository: pathlib.Path,
        log: pathlib.Path,
        out: pathlib.Path,
        zmin: pathlib.Path,
    ) -> subprocess.CompletedProcess[bytes]:
        return subprocess.run(
            [
                sys.executable,
                str(REPLAY),
                str(log),
                str(repository),
                "--stock-git",
                str(STOCK_GIT),
                "--zmin",
                str(zmin),
                "--out",
                str(out),
            ],
            capture_output=True,
            check=False,
        )

    def test_replay_matches_identical_binary(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            repository = root / "repo"
            repository.mkdir()
            subprocess.run([str(STOCK_GIT), "init", "-q"], cwd=repository, check=True)
            (repository / "untracked.txt").write_text("content\n", encoding="utf-8")
            if os.name != "nt" and hasattr(socket, "AF_UNIX"):
                unix_socket = socket.socket(socket.AF_UNIX)
                unix_socket.bind(str(repository / "untracked.sock"))
                unix_socket.close()
            log = root / "capture.jsonl"
            self.capture_status(repository, log)

            completed = self.replay(repository, log, root / "out", STOCK_GIT)

            self.assertEqual(completed.returncode, 0, completed.stderr.decode())
            summary = json.loads(completed.stdout)
            self.assertEqual(summary["passed"], 1)
            self.assertEqual(summary["mismatched"], 0)

    def test_replay_preserves_mismatch_artifacts(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            repository = root / "repo"
            repository.mkdir()
            subprocess.run([str(STOCK_GIT), "init", "-q"], cwd=repository, check=True)
            log = root / "capture.jsonl"
            self.capture_status(repository, log)

            completed = self.replay(repository, log, root / "out", pathlib.Path("/bin/echo"))

            self.assertEqual(completed.returncode, 1)
            summary = json.loads(completed.stdout)
            self.assertEqual(summary["mismatched"], 1)
            case_dirs = [path for path in (root / "out").iterdir() if path.is_dir() and path.name != "work"]
            self.assertEqual(len(case_dirs), 1)
            self.assertTrue((case_dirs[0] / "zmin.stdout").is_file())


if __name__ == "__main__":
    unittest.main()
