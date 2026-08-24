#!/usr/bin/env python3
"""Focused archive permission tests for tools/release-package.py."""

from __future__ import annotations

import importlib.util
import io
from pathlib import Path
import sys
import tarfile
import tempfile
import unittest


PACKAGER_PATH = Path(__file__).with_name("release-package.py")
SPEC = importlib.util.spec_from_file_location("release_package", PACKAGER_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"cannot load {PACKAGER_PATH}")
PACKAGER = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = PACKAGER
SPEC.loader.exec_module(PACKAGER)


class ReleasePackagePermissionTest(unittest.TestCase):
    def test_non_executable_unix_binary_fails_check_and_e2e(self) -> None:
        with tempfile.TemporaryDirectory(prefix="zmin-release-package-test-") as raw_root:
            root = Path(raw_root)
            binary_names = PACKAGER.BINARY_NAMES
            for name in binary_names:
                (root / name).write_bytes(name.encode("ascii"))
            PACKAGER.write_manifest(root, binary_names)
            archive = root / "broken.tar.gz"
            with tarfile.open(archive, mode="w:gz") as package:
                for name in (*binary_names, PACKAGER.MANIFEST_NAME):
                    payload = (root / name).read_bytes()
                    info = tarfile.TarInfo(name)
                    info.size = len(payload)
                    info.mode = 0o644 if name == binary_names[0] else 0o755
                    package.addfile(info, io.BytesIO(payload))

            with self.assertRaisesRegex(PACKAGER.PackageError, "not executable"):
                PACKAGER.check_archive(archive)
            with self.assertRaisesRegex(PACKAGER.PackageError, "not executable"):
                PACKAGER.run_e2e(archive)

    def test_windows_zip_does_not_claim_unix_modes(self) -> None:
        with tempfile.TemporaryDirectory(prefix="zmin-release-package-test-") as raw_root:
            root = Path(raw_root)
            binary_names = ("zmin.exe", "zmin-git-remote-http.exe")
            for name in binary_names:
                (root / name).write_bytes(name.encode("ascii"))
            PACKAGER.write_manifest(root, binary_names)
            archive = root / "windows.zip"
            PACKAGER.write_zip_archive(archive, root, binary_names)

            entries = PACKAGER.check_archive(archive)
            for name in binary_names:
                self.assertIsNone(entries[name].mode)


if __name__ == "__main__":
    unittest.main()
