#!/usr/bin/env python3
"""Measure one child process with high-resolution time and peak RSS."""

from __future__ import annotations

import argparse
import os
import pathlib
import resource
import subprocess
import sys
import time
from typing import BinaryIO


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--stdout", required=True, type=pathlib.Path)
    parser.add_argument("--stderr", required=True, type=pathlib.Path)
    parser.add_argument("--metrics", required=True, type=pathlib.Path)
    parser.add_argument("--stdin", type=pathlib.Path)
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    if args.command[:1] == ["--"]:
        args.command = args.command[1:]
    if not args.command:
        parser.error("missing command after --")
    return args


def private_output(path: pathlib.Path) -> BinaryIO:
    descriptor = os.open(path, os.O_CREAT | os.O_TRUNC | os.O_WRONLY, 0o600)
    return os.fdopen(descriptor, "wb")


def main() -> int:
    args = parse_args()
    stdin = args.stdin.open("rb") if args.stdin is not None else subprocess.DEVNULL
    before = resource.getrusage(resource.RUSAGE_CHILDREN)
    started = time.perf_counter_ns()
    try:
        with private_output(args.stdout) as stdout, private_output(args.stderr) as stderr:
            completed = subprocess.run(
                args.command,
                stdin=stdin,
                stdout=stdout,
                stderr=stderr,
                check=False,
            )
    finally:
        if args.stdin is not None:
            stdin.close()
    elapsed_seconds = (time.perf_counter_ns() - started) / 1_000_000_000
    after = resource.getrusage(resource.RUSAGE_CHILDREN)
    max_rss_bytes = int(after.ru_maxrss if sys.platform == "darwin" else after.ru_maxrss * 1024)
    metrics = (
        f"{elapsed_seconds:.9f}\t"
        f"{after.ru_utime - before.ru_utime:.9f}\t"
        f"{after.ru_stime - before.ru_stime:.9f}\t"
        f"{max_rss_bytes}\n"
    )
    with private_output(args.metrics) as output:
        output.write(metrics.encode())
    return completed.returncode


if __name__ == "__main__":
    raise SystemExit(main())
