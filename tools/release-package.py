#!/usr/bin/env python3
"""Create, validate, and exercise Zmin release archives."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import gzip
import hashlib
import http.server
import io
import os
from pathlib import Path
import shutil
import socketserver
import ssl
import subprocess
import sys
import tarfile
import tempfile
import threading
from typing import Iterable, Optional
from urllib.parse import unquote, urlsplit
import zipfile


MANIFEST_NAME = "ZMIN-MANIFEST.tsv"
BINARY_NAMES = ("zmin", "zmin-git-remote-http")


@dataclass(frozen=True)
class ArchiveEntry:
    payload: bytes
    mode: Optional[int]


class PackageError(RuntimeError):
    """A release package is invalid or its E2E fixture failed."""


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def archive_binary_names(archive: Path) -> tuple[str, str]:
    if archive.suffix.lower() == ".zip":
        return ("zmin.exe", "zmin-git-remote-http.exe")
    return BINARY_NAMES


def read_archive(archive: Path) -> dict[str, ArchiveEntry]:
    if archive.suffix.lower() == ".zip":
        return read_zip_archive(archive)
    if archive.name.endswith(".tar.gz"):
        return read_tar_archive(archive)
    raise PackageError(f"unsupported release archive: {archive}")


def read_zip_archive(archive: Path) -> dict[str, ArchiveEntry]:
    files: dict[str, ArchiveEntry] = {}
    with zipfile.ZipFile(archive) as package:
        for info in package.infolist():
            if info.is_dir():
                raise PackageError(f"archive contains directory entry: {info.filename}")
            if info.filename in files:
                raise PackageError(f"archive contains duplicate entry: {info.filename}")
            if Path(info.filename).name != info.filename or info.filename in {"", ".", ".."}:
                raise PackageError(f"archive entry is not a root file: {info.filename}")
            files[info.filename] = ArchiveEntry(package.read(info), None)
    return files


def read_tar_archive(archive: Path) -> dict[str, ArchiveEntry]:
    files: dict[str, ArchiveEntry] = {}
    with tarfile.open(archive, mode="r:gz") as package:
        for info in package.getmembers():
            if not info.isfile():
                raise PackageError(f"archive contains non-file entry: {info.name}")
            if info.name in files:
                raise PackageError(f"archive contains duplicate entry: {info.name}")
            if Path(info.name).name != info.name or info.name in {"", ".", ".."}:
                raise PackageError(f"archive entry is not a root file: {info.name}")
            stream = package.extractfile(info)
            if stream is None:
                raise PackageError(f"cannot read archive entry: {info.name}")
            files[info.name] = ArchiveEntry(stream.read(), info.mode)
    return files


def parse_manifest(files: dict[str, ArchiveEntry], binary_names: tuple[str, str]) -> None:
    expected = set(binary_names) | {MANIFEST_NAME}
    if set(files) != expected:
        missing = sorted(expected - set(files))
        extra = sorted(set(files) - expected)
        raise PackageError(f"archive entries drifted: missing={missing} extra={extra}")

    rows = files[MANIFEST_NAME].payload.decode("ascii").splitlines()
    expected_rows = [
        "file\tsha256",
        *[f"{name}\t{sha256_bytes(files[name].payload)}" for name in binary_names],
    ]
    if rows != expected_rows:
        raise PackageError(f"{MANIFEST_NAME} does not validate both release binaries")


def validate_archive_permissions(
    files: dict[str, ArchiveEntry], binary_names: tuple[str, str], archive: Path
) -> None:
    if not archive.name.endswith(".tar.gz"):
        return
    for name in binary_names:
        mode = files[name].mode
        if mode is None or not mode & 0o111:
            raise PackageError(f"Unix archive binary is not executable: {name}")
        if mode & 0o7000:
            raise PackageError(f"Unix archive binary has special mode bits: {name}")


def check_archive(archive: Path) -> dict[str, ArchiveEntry]:
    if not archive.is_file():
        raise PackageError(f"release archive is missing: {archive}")
    files = read_archive(archive)
    binary_names = archive_binary_names(archive)
    parse_manifest(files, binary_names)
    validate_archive_permissions(files, binary_names, archive)
    print(f"release package check passed: {archive} ({len(files) - 1} binaries)")
    return files


def write_manifest(directory: Path, binary_names: tuple[str, str]) -> None:
    rows = ["file\tsha256"]
    for name in binary_names:
        rows.append(f"{name}\t{sha256_bytes((directory / name).read_bytes())}")
    (directory / MANIFEST_NAME).write_text("\n".join(rows) + "\n", encoding="ascii")


def package_files(directory: Path, binary_names: tuple[str, str]) -> Iterable[tuple[str, bytes]]:
    for name in (*binary_names, MANIFEST_NAME):
        yield name, (directory / name).read_bytes()


def write_tar_archive(archive: Path, directory: Path, binary_names: tuple[str, str]) -> None:
    with archive.open("wb") as raw:
        with gzip.GzipFile(fileobj=raw, mode="wb", mtime=0) as compressed:
            with tarfile.open(fileobj=compressed, mode="w", format=tarfile.PAX_FORMAT) as package:
                for name, payload in package_files(directory, binary_names):
                    info = tarfile.TarInfo(name)
                    info.size = len(payload)
                    info.mode = 0o755 if name != MANIFEST_NAME else 0o644
                    info.mtime = 0
                    info.uid = 0
                    info.gid = 0
                    info.uname = ""
                    info.gname = ""
                    package.addfile(info, io.BytesIO(payload))


def write_zip_archive(archive: Path, directory: Path, binary_names: tuple[str, str]) -> None:
    with zipfile.ZipFile(archive, mode="w", compression=zipfile.ZIP_DEFLATED) as package:
        for name, payload in package_files(directory, binary_names):
            info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            info.create_system = 0
            info.external_attr = 0
            info.compress_type = zipfile.ZIP_DEFLATED
            package.writestr(info, payload)


def create_archive(zmin: Path, helper: Path, archive: Path) -> None:
    binary_names = archive_binary_names(archive)
    if not zmin.is_file() or not helper.is_file():
        raise PackageError(f"both release binaries are required: {zmin} {helper}")
    archive.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="zmin-release-package-") as raw_directory:
        directory = Path(raw_directory)
        shutil.copyfile(zmin, directory / binary_names[0])
        shutil.copyfile(helper, directory / binary_names[1])
        os.chmod(directory / binary_names[0], 0o755)
        os.chmod(directory / binary_names[1], 0o755)
        write_manifest(directory, binary_names)
        if archive.suffix.lower() == ".zip":
            write_zip_archive(archive, directory, binary_names)
        else:
            write_tar_archive(archive, directory, binary_names)
    check_archive(archive)


def run_checked(command: list[str], *, cwd: Path | None = None, env: dict[str, str] | None = None) -> None:
    result = subprocess.run(command, cwd=cwd, env=env, capture_output=True, text=True)
    if result.returncode != 0:
        raise PackageError(
            f"command failed ({result.returncode}): {' '.join(command)}\n"
            f"stdout: {result.stdout}\nstderr: {result.stderr}"
        )


class GitBackendHandler(http.server.BaseHTTPRequestHandler):
    project_root: Path
    git_binary: str
    server_port: int

    def do_GET(self) -> None:  # noqa: N802
        self.serve_backend()

    def do_POST(self) -> None:  # noqa: N802
        self.serve_backend()

    def serve_backend(self) -> None:
        parsed = urlsplit(self.path)
        body = self.rfile.read(int(self.headers.get("Content-Length", "0")))
        environment = os.environ.copy()
        environment.update(
            {
                "CONTENT_LENGTH": str(len(body)),
                "CONTENT_TYPE": self.headers.get("Content-Type", ""),
                "GATEWAY_INTERFACE": "CGI/1.1",
                "GIT_HTTP_EXPORT_ALL": "1",
                "GIT_PROJECT_ROOT": str(self.project_root),
                "HTTP_HOST": self.headers.get("Host", ""),
                "PATH_INFO": unquote(parsed.path),
                "QUERY_STRING": parsed.query,
                "REMOTE_ADDR": "127.0.0.1",
                "REQUEST_METHOD": self.command,
                "SERVER_NAME": "127.0.0.1",
                "SERVER_PORT": str(self.server_port),
                "SERVER_PROTOCOL": "HTTP/1.1",
            }
        )
        result = subprocess.run(
            [self.git_binary, "http-backend"],
            input=body,
            capture_output=True,
            env=environment,
            check=False,
        )
        if result.returncode != 0:
            self.send_error(500, "git http-backend failed")
            return
        header_bytes, separator, response_body = result.stdout.partition(b"\r\n\r\n")
        if not separator:
            header_bytes, separator, response_body = result.stdout.partition(b"\n\n")
        if not separator:
            self.send_error(500, "git http-backend returned no headers")
            return
        status = 200
        has_content_length = False
        for raw_header in header_bytes.splitlines():
            name, _, value = raw_header.decode("latin-1").partition(":")
            if name.lower() == "status":
                status = int(value.strip().split(" ", 1)[0])
            elif name.lower() == "content-length":
                has_content_length = True
        self.send_response(status)
        for raw_header in header_bytes.splitlines():
            name, separator, value = raw_header.decode("latin-1").partition(":")
            if separator and name.lower() != "status":
                self.send_header(name, value.lstrip())
        if not has_content_length:
            self.send_header("Content-Length", str(len(response_body)))
        self.send_header("Connection", "close")
        self.end_headers()
        self.close_connection = True
        self.wfile.write(response_body)

    def log_message(self, _format: str, *_args: object) -> None:
        return


class ThreadingHTTPSServer(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True
    allow_reuse_address = True


def create_fixture_repository(root: Path, git_binary: str) -> Path:
    repos = root / "repos"
    work = root / "work"
    bare = repos / "repo.git"
    repos.mkdir()
    run_checked([git_binary, "init", "--bare", str(bare)])
    run_checked([git_binary, "init", "-b", "main", str(work)])
    run_checked([git_binary, "-C", str(work), "config", "user.name", "Release E2E"])
    run_checked([git_binary, "-C", str(work), "config", "user.email", "release-e2e@example.test"])
    run_checked([git_binary, "-C", str(work), "config", "commit.gpgsign", "false"])
    (work / "README.md").write_text("release archive HTTPS clone\n", encoding="utf-8")
    run_checked([git_binary, "-C", str(work), "add", "README.md"])
    run_checked([git_binary, "-C", str(work), "commit", "-m", "fixture"])
    run_checked([git_binary, "-C", str(work), "remote", "add", "origin", str(bare)])
    run_checked([git_binary, "-C", str(work), "push", "origin", "main"])
    return bare


def write_test_certificate(root: Path, openssl_binary: str) -> tuple[Path, Path, Path]:
    ca_config = root / "ca-openssl.cnf"
    ca_certificate = root / "ca.pem"
    ca_key = root / "ca.key"
    server_config = root / "server-openssl.cnf"
    certificate = root / "server.pem"
    key = root / "server.key"
    csr = root / "server.csr"
    ca_config.write_text(
        "[req]\n"
        "distinguished_name = dn\n"
        "x509_extensions = v3_ca\n"
        "prompt = no\n\n"
        "[dn]\n"
        "CN = Zmin release E2E test CA\n\n"
        "[v3_ca]\n"
        "basicConstraints = critical,CA:true,pathlen:1\n"
        "keyUsage = critical,keyCertSign,cRLSign\n"
        "subjectKeyIdentifier = hash\n",
        encoding="ascii",
    )
    server_config.write_text(
        "[req]\n"
        "distinguished_name = dn\n"
        "prompt = no\n\n"
        "[dn]\n"
        "CN = 127.0.0.1\n\n"
        "[v3_server]\n"
        "basicConstraints = critical,CA:false\n"
        "keyUsage = critical,digitalSignature,keyEncipherment\n"
        "extendedKeyUsage = serverAuth\n"
        "subjectAltName = IP:127.0.0.1\n",
        encoding="ascii",
    )
    run_checked(
        [
            openssl_binary,
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-keyout",
            str(ca_key),
            "-out",
            str(ca_certificate),
            "-days",
            "1",
            "-config",
            str(ca_config),
        ]
    )
    run_checked(
        [
            openssl_binary,
            "req",
            "-new",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-keyout",
            str(key),
            "-out",
            str(csr),
            "-config",
            str(server_config),
        ]
    )
    run_checked(
        [
            openssl_binary,
            "x509",
            "-req",
            "-in",
            str(csr),
            "-CA",
            str(ca_certificate),
            "-CAkey",
            str(ca_key),
            "-CAcreateserial",
            "-out",
            str(certificate),
            "-days",
            "1",
            "-sha256",
            "-extfile",
            str(server_config),
            "-extensions",
            "v3_server",
        ]
    )
    return certificate, key, ca_certificate


def extract_archive(
    files: dict[str, ArchiveEntry],
    destination: Path,
    binary_names: tuple[str, str],
    archive: Path,
) -> tuple[Path, Path]:
    is_unix_archive = archive.name.endswith(".tar.gz")
    for name, entry in files.items():
        path = destination / name
        path.write_bytes(entry.payload)
        if is_unix_archive:
            if entry.mode is None:
                raise PackageError(f"Unix archive entry has no mode: {name}")
            os.chmod(path, entry.mode & 0o777)
    return destination / binary_names[0], destination / binary_names[1]


def run_e2e(archive: Path) -> None:
    files = check_archive(archive)
    git_binary = shutil.which("git")
    openssl_binary = shutil.which("openssl")
    if git_binary is None or openssl_binary is None:
        raise PackageError("HTTPS package E2E requires git and openssl on PATH")
    with tempfile.TemporaryDirectory(prefix="zmin-release-e2e-") as raw_root:
        root = Path(raw_root)
        package_root = root / "package"
        package_root.mkdir()
        zmin_binary, helper_binary = extract_archive(
            files, package_root, archive_binary_names(archive), archive
        )
        if zmin_binary.parent != helper_binary.parent:
            raise PackageError("release helper is not a sibling of zmin after extraction")
        fixture_root = root / "fixture"
        fixture_root.mkdir()
        create_fixture_repository(fixture_root, git_binary)
        certificate, key, ca_certificate = write_test_certificate(fixture_root, openssl_binary)
        handler = GitBackendHandler
        handler.project_root = fixture_root / "repos"
        handler.git_binary = git_binary
        server = ThreadingHTTPSServer(("127.0.0.1", 0), handler)
        handler.server_port = server.server_address[1]
        tls = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        tls.load_cert_chain(certificate, key)
        server.socket = tls.wrap_socket(server.socket, server_side=True)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        empty_path = root / "empty-path"
        empty_path.mkdir()
        clone = root / "clone"
        (root / "home").mkdir()
        global_git_config = root / "home" / ".gitconfig"
        global_git_config.write_text(
            "[user]\n"
            "\tname = Release E2E\n"
            "\temail = release-e2e@example.test\n",
            encoding="ascii",
        )
        environment = {
            "GIT_SSL_CAINFO": str(ca_certificate),
            "GIT_CONFIG_GLOBAL": str(global_git_config),
            "HOME": str(root / "home"),
            "LANG": "C",
            "LC_ALL": "C",
            "NO_PROXY": "127.0.0.1,localhost",
            "PATH": str(empty_path),
            "TMPDIR": str(root / "tmp"),
        }
        (root / "tmp").mkdir()
        url = f"https://127.0.0.1:{handler.server_port}/repo.git"
        try:
            result = subprocess.run(
                [str(zmin_binary), "clone", url, str(clone)],
                cwd=package_root,
                env=environment,
                capture_output=True,
                text=True,
            )
        finally:
            server.shutdown()
            server.server_close()
            thread.join(timeout=5)
        if result.returncode != 0:
            raise PackageError(
                f"extracted HTTPS clone failed ({result.returncode})\n"
                f"stdout: {result.stdout}\nstderr: {result.stderr}"
            )
        if (clone / "README.md").read_text(encoding="utf-8") != "release archive HTTPS clone\n":
            raise PackageError("extracted HTTPS clone did not materialize fixture content")
        print(f"release package HTTPS E2E passed: {archive}")


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)

    create = subparsers.add_parser("create")
    create.add_argument("--zmin", type=Path, required=True)
    create.add_argument("--helper", type=Path, required=True)
    create.add_argument("--artifact", type=Path, required=True)

    check = subparsers.add_parser("check")
    check.add_argument("--artifact", type=Path, required=True)

    e2e = subparsers.add_parser("e2e")
    e2e.add_argument("--artifact", type=Path, required=True)
    return parser


def main() -> int:
    args = build_parser().parse_args()
    try:
        if args.command == "create":
            create_archive(args.zmin, args.helper, args.artifact)
        elif args.command == "check":
            check_archive(args.artifact)
        else:
            run_e2e(args.artifact)
    except (OSError, PackageError, subprocess.SubprocessError, zipfile.BadZipFile, tarfile.TarError) as error:
        print(f"release package check failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
