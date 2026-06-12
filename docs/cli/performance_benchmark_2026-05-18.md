# Git CLI Performance Benchmark

Date: 2026-06-12

Host: MacBook Pro M1, 16 GB RAM, macOS 26.4, Darwin 25.4.0, arm64.

Tools:

- Git: upstream/Homebrew Git `2.53.0` from `/usr/local/bin/git`, not Apple Git.
- Gitoxide: `gix 0.54.0`.
- Skron: `skron-cli 0.1.0`, release build.

Method: `tools/git-performance-bench.sh` with default representative fixtures and 10 repeats:

- history fixture: 90 commits, 25 files per commit, 2,520 reachable objects;
- write fixture: 1,800 initial files, then 200 dirty files;
- transport batch fixtures: 2,400 file updates for fetch and push;
- execution order is randomized per operation group with a fixed seed;
- Git and Skron outputs/state are validated before or after measured operations;
- no CPU or I/O throttling was used for this run.

Validation checks covered:

- exact Git/Skron output match for `status`, `log`, `rev-list`, and `merge-base`;
- Git `index-pack` acceptance for both Git-generated and Skron-generated packs;
- matching Git/Skron tree ids for write and push preparation;
- matching Git/Skron refs for clone and fetch results;
- zero non-zero command exits in the measured rows.

Values are medians over 10 runs. Time is wall seconds, RSS is MiB. Lower is better. Raw data for this run is in `target/bench/git-performance-2026-06-12.tsv`.

`Skron vs Git` and `Skron vs Gitoxide` show the percentage wall-time improvement for Skron against each baseline. `n/a` means the local `gix` CLI did not provide a comparable command in this benchmark. Gitoxide rows are CLI-adjacent comparisons, not strict output-equivalence checks.

| Operation | Fixture | Git sec / MiB | Gitoxide sec / MiB | Skron sec / MiB | Skron vs Git | Skron vs Gitoxide |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| `init` | single repo init | `0.05 / 6.2` | `n/a` | `0.01 / 5.0` | `80% faster` | `n/a` |
| `status` | 90 commits / 2,520 objects | `0.05 / 7.5` | `0.02 / 16.2` | `0.01 / 6.4` | `80% faster` | `50% faster` |
| `log` | 90 commits / 2,520 objects | `0.05 / 7.0` | `0.01 / 13.9` | `0.01 / 6.1` | `80% faster` | `0%` |
| `rev-list` | 90 commits / 2,520 objects | `0.05 / 7.3` | `n/a` | `0.03 / 6.6` | `40% faster` | `n/a` |
| `merge-base` | 90 commits / 2,520 objects | `0.05 / 6.7` | `0.01 / 13.7` | `0.01 / 5.9` | `80% faster` | `0%` |
| `pack-objects` | 2,520 objects | `0.05 / 6.8` | `n/a` | `0.01 / 5.8` | `80% faster` | `n/a` |
| `index-pack` | pack from 2,520 objects | `0.11 / 6.7` | `n/a` | `0.08 / 6.2` | `27% faster` | `n/a` |
| `add` | 1,800 files | `1.19 / 7.1` | `n/a` | `1.02 / 6.4` | `14% faster` | `n/a` |
| `commit` | 1,800 files | `0.14 / 7.9` | `n/a` | `0.05 / 6.5` | `64% faster` | `n/a` |
| `add-dirty` | 200 files | `0.15 / 7.6` | `n/a` | `0.12 / 6.3` | `20% faster` | `n/a` |
| `commit-dirty` | 200 files | `0.15 / 8.6` | `n/a` | `0.05 / 6.6` | `67% faster` | `n/a` |
| `clone` | local repository | `0.23 / 8.1` | `0.27 / 21.2` | `0.10 / 6.5` | `57% faster` | `63% faster` |
| `push-noop` | local bare remote | `0.15 / 6.3` | `n/a` | `0.01 / 6.0` | `93% faster` | `n/a` |
| `push-incremental` | local bare remote | `0.44 / 7.1` | `n/a` | `0.08 / 6.9` | `82% faster` | `n/a` |
| `push-batch` | 2,400 files | `0.67 / 8.9` | `n/a` | `0.13 / 7.8` | `81% faster` | `n/a` |
| `fetch-noop` | local bare remote | `0.26 / 7.1` | `0.11 / 15.8` | `0.02 / 6.0` | `92% faster` | `82% faster` |
| `fetch-incremental` | local bare remote | `0.35 / 7.4` | `0.21 / 17.1` | `0.08 / 7.0` | `77% faster` | `62% faster` |
| `fetch-batch` | 2,400 files | `0.36 / 7.4` | `0.21 / 17.7` | `0.09 / 6.9` | `75% faster` | `57% faster` |
## Summary

In this local benchmark on a MacBook Pro M1, Skron measured faster than upstream Git on every measured operation, from `14%` faster for `add` to `93%` faster for `push-noop`.

Against Gitoxide, Skron had the same median as `gix` for `log` and `merge-base`, and measured faster on the other comparable CLI-adjacent operations by `50%` to `82%`.

Skron also used less median RSS than upstream Git and Gitoxide in this run.
