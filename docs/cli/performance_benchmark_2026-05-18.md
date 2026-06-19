# Git CLI Performance Benchmark

Date: 2026-06-12

Host: MacBook Pro M1, 16 GB RAM, macOS 26.4, Darwin 25.4.0, arm64.

Tools:

- Git: upstream/Homebrew Git `2.53.0` from `/usr/local/bin/git`, not Apple Git.
- Gitoxide: `gix 0.54.0`.
- Zmin: `zmin-cli 0.1.0`, release build.

Method: `tools/git-performance-bench.sh` with default representative fixtures and 10 repeats:

- history fixture: 90 commits, 25 files per commit, 2,520 reachable objects;
- write fixture: 1,800 initial files, then 200 dirty files;
- transport batch fixtures: 2,400 file updates for fetch and push;
- execution order is randomized per operation group with a fixed seed;
- Git and Zmin outputs/state are validated before or after measured operations;
- no CPU or I/O throttling was used for this run.

Validation checks covered:

- exact Git/Zmin output match for `status`, `log`, `rev-list`, and `merge-base`;
- Git `index-pack` acceptance for both Git-generated and Zmin-generated packs;
- matching Git/Zmin tree ids for write and push preparation;
- matching Git/Zmin refs for clone and fetch results;
- zero non-zero command exits in the measured rows.

Values are medians over 10 runs. Time is wall seconds, RSS is MiB. Lower is better. Raw data for this run is in `target/bench/git-performance-2026-06-12.tsv`.

`Zmin vs Git` and `Zmin vs Gitoxide` show the percentage wall-time improvement for Zmin against each baseline. `n/a` means the local `gix` CLI did not provide a comparable command in this benchmark. Gitoxide rows are CLI-adjacent comparisons, not strict output-equivalence checks.

| Operation | Fixture | Git sec / MiB | Gitoxide sec / MiB | Zmin sec / MiB | Zmin vs Git | Zmin vs Gitoxide |
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

In this local benchmark on a MacBook Pro M1, Zmin measured faster than upstream Git on every measured operation, from `14%` faster for `add` to `93%` faster for `push-noop`.

Against Gitoxide, Zmin had the same median as `gix` for `log` and `merge-base`, and measured faster on the other comparable CLI-adjacent operations by `50%` to `82%`.

Zmin also used less median RSS than upstream Git and Gitoxide in this run.

## 2026-06-16 add-dirty follow-up

The 2026-06-16 macOS smoke run caught a regression in the dirty tracked-file
case: `zmin add -A` over a fixture with 1,800 tracked files and 200 dirty
files measured around `0.46-0.49s` while upstream Git measured around
`0.14-0.15s`.

The fix keeps Git racy-index semantics by trusting stat metadata only when the
index mtime is strictly newer than the entry mtime, then lets `add -A` process
tracked changes in the tracked pass and skip already tracked regular/symlink
paths in the file staging loop. Same-size rapid rewrites still hash content
instead of relying on stat-only metadata.

Validation after the fix:

- `cargo fmt --all -- --check`
- `cargo test -p zmin-cli --test git_index_mutation_compat add_ -- --nocapture`
  (`30/30` passing)
- targeted upstream `t3700-add.sh` on macOS:
  `/var/folders/l3/y2d_2zz51z731b86_sstzz0h0000gn/T//zmin-upstream-compat.PjbgmS/summary.tsv`
- `ZMIN_BENCH_REPEATS=3 tools/git-performance-bench.sh` using
  `/private/tmp/zmin-codex-target/release/zmin`

Key median wall times from that smoke:

| Operation | Git median | Zmin median | Note |
| --- | ---: | ---: | --- |
| `add` | `1.19s` | `1.15s` | initial 1,800-file add remains slightly faster than Git |
| `add-dirty` | `0.16s` | `0.18s` | regression reduced from roughly 3x slower to near parity |
| `commit-dirty` | `0.14s` | `0.06s` | still faster than Git |

This closes the coarse `add-dirty` regression but does not yet prove the final
performance target. The next performance pass should keep reducing the remaining
syscall-heavy `add-dirty` gap and repeat the same gate on Windows/Git-for-
Windows through Parallels.

Windows/Git-for-Windows follow-up through Parallels after forcing rebuilds of
`C:\Users\zmin\zmin-target\release\zmin.exe`:

- `tools/parallels-windows-runner.sh benchmark 3`
- rows:
  `C:\Users\zmin\zmin-bench-20260616T071816Z-67664-out\bench.csv`
- checks:
  `C:\Users\zmin\zmin-bench-20260616T071816Z-67664-out\checks.csv`
- targeted upstream add compatibility:
  `/c/Users/zmin/zmin-upstream-20260616T072423Z-68770-out/summary.tsv`
  (`t3700-add.sh`, `1/1` passing)

Key Windows means from the 3-repeat smoke:

| Operation | Git mean | Zmin mean | Note |
| --- | ---: | ---: | --- |
| `add` | `0.89s` | `1.26s` | improved from the previous rebuilt Zmin `1.46s`, still a Windows gap |
| `add-dirty` | `0.21s` | `0.29s` | much better than the stale-binary `0.97s`, still behind Git |
| `commit-dirty` | `0.27s` | `0.11s` | faster than Git |

The Windows result validates correctness after the fast path but keeps Windows
`add` and `add-dirty` in the active performance burn-down.

Tooling invariant added after the stale-binary finding:
`tools/windows-native-benchmark.ps1` now always runs
`cargo build -p zmin-cli --release --bin zmin` before timed operations
instead of trusting an existing `zmin.exe`. A Parallels smoke run confirmed
the build path and benchmark checks:

- `tools/parallels-windows-runner.sh benchmark 1`
- rows:
  `C:\Users\zmin\zmin-bench-20260616T074825Z-6431-out\bench.csv`
- checks:
  `C:\Users\zmin\zmin-bench-20260616T074825Z-6431-out\checks.csv`
  (`status-output`, `log-output`, `rev-list-output`, `commit-dirty-1`,
  `clone-1`, `fetch-noop`, and `fetch-incremental-1` all `ok`)

Performance-gate output format follow-up:
`tools/windows-native-benchmark.ps1` now includes `median_seconds` in
`summary.csv` and writes `comparison.csv` with per-operation Git-vs-Zmin
mean/median ratios. This keeps the existing `bench.csv`, `checks.csv`, and
summary mean/min/max fields, while making Windows outlier-heavy runs easier to
judge without manual spreadsheet work.

Validation:

- `tools/parallels-windows-runner.sh benchmark 1`
- rows:
  `C:\Users\zmin\zmin-bench-20260616T155755Z-44314-out\bench.csv`
- checks:
  `C:\Users\zmin\zmin-bench-20260616T155755Z-44314-out\checks.csv`
  (`status-output`, `log-output`, `rev-list-output`, `commit-dirty-1`,
  `clone-1`, `fetch-noop`, and `fetch-incremental-1` all `ok`)
- summary with `median_seconds`:
  `C:\Users\zmin\zmin-bench-20260616T155755Z-44314-out\summary.csv`
- comparison ratios:
  `C:\Users\zmin\zmin-bench-20260616T155755Z-44314-out\comparison.csv`

Follow-up cache slice: `add`, tracked worktree refresh, and `status` now load
root `.gitattributes` once per command path through `WorktreeContentRules`
instead of re-reading/parsing it for every candidate file. This keeps the same
ident/eol/filter checks, including the Windows-sensitive newline path, but
removes repeated metadata and parse work from the hot loops.

Validation after the cache slice:

- `cargo fmt --all -- --check`
- `cargo test -p zmin-cli --test git_index_mutation_compat add_ -- --nocapture`
  (`30/30` passing)
- `cargo test -p zmin-cli --test git_worktree_state_compat -- --nocapture`
  (`21/21` passing)
- `cargo test -p zmin-cli --test git_status_compat -- --nocapture`
  (`4/4` passing)
- targeted macOS upstream `t3700-add.sh` and `t0021-conversion.sh`:
  `/var/folders/l3/y2d_2zz51z731b86_sstzz0h0000gn/T//zmin-upstream-compat.P0VkpR/summary.tsv`
  (`2/2` passing)
- macOS 3-repeat smoke log:
  `/tmp/zmin-macos-attrs-cache-bench-20260616.tsv`
  (`27` validation checks, no failures)
- Windows/Git-for-Windows Parallels smoke:
  `C:\Users\zmin\zmin-bench-20260616T081318Z-61641-out\bench.csv`
  with checks at
  `C:\Users\zmin\zmin-bench-20260616T081318Z-61641-out\checks.csv`
  (all `ok`)

Key follow-up timings:

| Platform | Operation | Git | Zmin | Note |
| --- | --- | ---: | ---: | --- |
| macOS | `add` median | `1.40s` | `1.11s` | still faster than Git in the 3-repeat smoke |
| macOS | `add-dirty` median | `0.15s` | `0.16s` | near parity, still slightly behind |
| macOS | `status` median | `0.02s` | `0.03s` | still a small status gap |
| Windows | `add` one-run mean | `0.93s` | `1.45s` | still a Windows gap |
| Windows | `add-dirty` one-run mean | `0.22s` | `0.30s` | still a Windows gap |
| Windows | `status` one-run mean | `0.06s` | `0.07s` | improved vs the earlier smoke, still behind Git |

The cache slice is accepted as a correctness-preserving hot-loop cleanup, not as
closure of the Windows performance goal.

Follow-up profiling slice: `add` now has `ZMIN_PHASE_TRACE` labels for setup,
file collection, tracked staging, already-tracked filtering, file staging, and
index writing. `tools/windows-native-benchmark.ps1` can write per-command Zmin
phase logs when `-ZminPhaseTraceDir` is set; the Parallels runner passes this
when `ZMIN_WINDOWS_BENCH_PHASE_TRACE=1`.

Validation/profiling run:

- `cargo fmt --all -- --check`
- `cargo test -p zmin-cli --test git_index_mutation_compat add_ -- --nocapture`
  (`30/30` passing)
- targeted macOS upstream `t3700-add.sh`:
  `/var/folders/l3/y2d_2zz51z731b86_sstzz0h0000gn/T//zmin-upstream-compat.sXC1yI/summary.tsv`
  (`1/1` passing)
- `ZMIN_WINDOWS_BENCH_PHASE_TRACE=1 tools/parallels-windows-runner.sh benchmark 1`
- rows:
  `C:\Users\zmin\zmin-bench-20260616T083424Z-76780-out\bench.csv`
- checks:
  `C:\Users\zmin\zmin-bench-20260616T083424Z-76780-out\checks.csv`
  (all `ok`)
- phase traces:
  `C:\Users\zmin\zmin-bench-20260616T083424Z-76780-out\phase-traces`

Key Windows phase evidence:

| Scenario | Total | Dominant phase | Phase time | Meaning |
| --- | ---: | --- | ---: | --- |
| initial `add -A` over 800 files | `1.53s` | `add.stage_files` | `1.47s` | object write/hash/index-entry upsert path dominates |
| dirty `add -A` over 100 files | `0.27s` | `add.stage_tracked` | `0.22s` | tracked-pass stat/hash scan dominates |

Next Windows add work should focus on those two paths rather than more broad
pathspec or ignore changes.

Follow-up staging-options cache: `add` and status-like worktree checks now load
`core.filemode` and `core.symlinks` once through `WorktreeStageOptions` instead
of consulting repo config during every file-mode comparison. This keeps the
same platform semantics (`core.filemode=false` on Windows, symlink handling,
and existing attributes/clean-filter checks), but removes repeated config reads
from the two traced add hot loops.

Validation:

- `cargo fmt --all -- --check`
- `cargo test -p zmin-cli --test git_index_mutation_compat add_ -- --nocapture`
  (`30/30` passing)
- `cargo test -p zmin-cli --test git_status_compat -- --nocapture`
  (`4/4` passing)
- targeted macOS upstream `t3700-add.sh`:
  `/var/folders/l3/y2d_2zz51z731b86_sstzz0h0000gn/T//zmin-upstream-compat.2097YU/summary.tsv`
  (`1/1` passing)
- `ZMIN_WINDOWS_BENCH_PHASE_TRACE=1 tools/parallels-windows-runner.sh benchmark 1`
- rows:
  `C:\Users\zmin\zmin-bench-20260616T085534Z-99002-out\bench.csv`
- checks:
  `C:\Users\zmin\zmin-bench-20260616T085534Z-99002-out\checks.csv`
  (all `ok`)
- phase traces:
  `C:\Users\zmin\zmin-bench-20260616T085534Z-99002-out\phase-traces`

Key Windows movement compared with the previous trace run:

| Scenario | Previous total / dominant phase | Current total / dominant phase | Result |
| --- | ---: | ---: | --- |
| initial `add -A` over 800 files | `1.53s` / `add.stage_files=1.47s` | `1.48s` / `add.stage_files=1.43s` | small improvement; object write/upsert still dominates |
| dirty `add -A` over 100 files | `0.27s` / `add.stage_tracked=0.22s` | `0.22s` / `add.stage_tracked=0.17s` | config-cache win in tracked pass |

The next Windows `add` optimization should target `stage_files` internals:
streamed blob write, loose-object path creation/write, and index-entry upsert
cost for initial adds.

Follow-up Windows initial-add hot-path slice: staging now avoids embedded-repo
warning probes for non-directory paths, uses point lookups for unmerged index
stage detection instead of scanning the full index, and caches successful
creation of the loose object directory inside `LooseObjectStore` so streamed
blob writes do not call `create_dir_all(.git/objects)` once per added file.
The object directory is still created lazily on the first streamed write.

Validation:

- `cargo fmt --all -- --check`
- `cargo test -p zmin-git-core streamed_blob -- --nocapture` (`5/5`
  passing)
- `cargo test -p zmin-cli --test git_index_mutation_compat add_ --
  --nocapture` (`30/30` passing)
- targeted macOS upstream `t3700-add.sh`:
  `/var/folders/l3/y2d_2zz51z731b86_sstzz0h0000gn/T//zmin-upstream-compat.DuMfU6/summary.tsv`
  (`1/1` passing)
- `ZMIN_WINDOWS_BENCH_PHASE_TRACE=1 tools/parallels-windows-runner.sh benchmark 1`
- rows:
  `C:\Users\zmin\zmin-bench-20260616T100713Z-35573-out\bench.csv`
- checks:
  `C:\Users\zmin\zmin-bench-20260616T100713Z-35573-out\checks.csv`
  (all `ok`)
- phase traces:
  `C:\Users\zmin\zmin-bench-20260616T100713Z-35573-out\phase-traces`

Key Windows movement:

| Scenario | Previous trace after staging-options cache | Current trace | Result |
| --- | ---: | ---: | --- |
| initial `add -A` over 800 files | `1.48s` / `add.stage_files=1.43s` | `0.79s` / `add.stage_files=0.75s` | initial add is now faster than stock Git in this one-repeat Windows run (`0.85s` Zmin vs `0.92s` Git) |
| dirty `add -A` over 100 files | `0.22s` / `add.stage_tracked=0.17s` | `0.22s` / `add.stage_tracked=0.16s` | dirty add remains near parity but still regressed in the tool-level row (`0.29s` Zmin vs `0.23s` Git) |

The performance goal is still open: the one-repeat Windows run is useful
directional evidence, but dirty add, status, clone, and broader multi-repeat
performance gates still need burn-down.

Follow-up tracked-pass cleanup and detail trace:
`stage_tracked_worktree_changes_matching` now reuses the `symlink_metadata`
result it already fetched while deciding whether a tracked worktree entry
changed. It also avoids pre-hashing stat-unsafe tracked files before staging:
when stat data is not safe, the tracked pass goes directly through the normal
staging path, which still hashes/writes the blob once and refreshes metadata
when the object id is unchanged. The racy-index guard is unchanged: stat data is
trusted only when the index mtime is strictly newer than the entry mtime.

`ZMIN_PHASE_TRACE` now emits one aggregate `add.stage_tracked.detail` row with
entry counts and timing buckets for metadata reads, stat-safe entries, direct
restaging, and content/conversion checks. The detail row is intentionally
aggregate-only to avoid per-file trace overhead.

Validation:

- `cargo fmt --all -- --check`
- `cargo test -p zmin-cli --test git_index_mutation_compat add_ --
  --nocapture` (`30/30` passing)
- `cargo test -p zmin-cli --test git_status_compat -- --nocapture` (`4/4`
  passing)
- targeted macOS upstream `t3700-add.sh`:
  `/var/folders/l3/y2d_2zz51z731b86_sstzz0h0000gn/T//zmin-upstream-compat.dOXThy/summary.tsv`
  (`1/1` passing)
- `ZMIN_WINDOWS_BENCH_PHASE_TRACE=1 tools/parallels-windows-runner.sh benchmark 1`
- rows:
  `C:\Users\zmin\zmin-bench-20260616T115537Z-6821-out\bench.csv`
- checks:
  `C:\Users\zmin\zmin-bench-20260616T115537Z-6821-out\checks.csv`
  (all `ok`)
- phase traces:
  `C:\Users\zmin\zmin-bench-20260616T115537Z-6821-out\phase-traces`

Key Windows detail:

| Scenario | Previous detail trace | Current detail trace | Result |
| --- | ---: | ---: | --- |
| dirty `add -A` over 100 files | `add.stage_tracked=0.178s`, `content_hashes=100`, `stage_file_seconds=0.138s` | `add.stage_tracked=0.148s`, `content_hashes=0`, `stat_unsafe=100`, `stage_file_seconds=0.120s` | prehash removal reduced tracked-pass time; restaging 100 dirty files is now the dominant cost |

The one-repeat tool row was still noisy (`0.234s` Zmin vs `0.158s` Git for
`add-dirty` in this run), so do not treat the Windows `add-dirty` gate as
closed. The next useful optimization target is the 100-file restaging path:
small blob write/compression/object install and index upsert during
`stage_file_with_mode_and_index_mtime_and_options`.

Follow-up guarded small-blob restage slice: regular files at or below 64 KiB now
use the in-memory `write_object` path only when the path already has an exact
index entry. New initial-add files still use the streamed blob path, preserving
the earlier initial-add architecture and avoiding the all-small-file regression
seen when this was tried for every small file. The stage path captures the exact
existing index entry once and reuses it for object-id equality refreshes, so the
guard does not add an extra lookup to initial adds.

Validation:

- `cargo fmt --all -- --check`
- `cargo test -p zmin-cli --test git_index_mutation_compat add_ --
  --nocapture` (`30/30` passing)
- `cargo test -p zmin-cli --test git_status_compat -- --nocapture` (`4/4`
  passing)
- targeted macOS upstream `t3700-add.sh`:
  `/var/folders/l3/y2d_2zz51z731b86_sstzz0h0000gn/T//zmin-upstream-compat.CSAqvv/summary.tsv`
  (`1/1` passing)
- `ZMIN_WINDOWS_BENCH_PHASE_TRACE=1 tools/parallels-windows-runner.sh benchmark 1`
  with rows/checks/traces at
  `C:\Users\zmin\zmin-bench-20260616T131229Z-63007-out`
  (all benchmark checks `ok`)

Windows phase evidence remains noisy. A cleaner pre-lookup-reuse run of the same
guarded small-blob path showed dirty restage `stage_file_seconds` moving from
`0.120s` to `0.106s` (`C:\Users\zmin\zmin-bench-20260616T125209Z-49446-out`),
while the final lookup-reuse run was system-noisy across the benchmark
(`add-dirty` `0.458s` Zmin vs `0.437s` Git, with `stage_file_seconds=0.246s`).
Keep this slice as a narrow correctness-preserving dirty-restage improvement,
not as closure of the Windows add performance gate. Next work should reduce the
remaining per-file object write/install cost and collect less noisy multi-repeat
Windows evidence.

Follow-up loose-object fanout cache slice: `LooseObjectStore` now caches created
loose-object fanout directories for the lifetime of the store, using the first
object-id byte as the 256-way fanout key. This avoids repeated
`create_dir_all(.git/objects/xx)` calls in `write_object`, known-id streamed
blob writes, unknown-id streamed blob installs, and loose-object copies. The
object write format and atomic temp-file install behavior are unchanged.

Validation:

- `cargo fmt --all -- --check`
- `cargo test -p zmin-git-core streamed_blob -- --nocapture` (`5/5`
  passing)
- `cargo test -p zmin-git-core loose_object -- --nocapture` (`9/9`
  passing)
- `cargo test -p zmin-cli --test git_index_mutation_compat add_ --
  --nocapture` (`30/30` passing)
- `cargo test -p zmin-cli --test git_status_compat -- --nocapture` (`4/4`
  passing)
- targeted macOS upstream `t3700-add.sh`:
  `/var/folders/l3/y2d_2zz51z731b86_sstzz0h0000gn/T//zmin-upstream-compat.OJL2DR/summary.tsv`
  (`1/1` passing)
- `ZMIN_WINDOWS_BENCH_PHASE_TRACE=1 tools/parallels-windows-runner.sh benchmark 3`
  with rows/checks/traces at
  `C:\Users\zmin\zmin-bench-20260616T134215Z-90870-out`
  (all benchmark checks `ok`)

Key Windows 3-repeat rows after fanout caching:

| Operation | Git mean | Zmin mean | Note |
| --- | ---: | ---: | --- |
| `add` | `1.109s` | `1.174s` | still behind Git on mean, but much closer; Zmin per-repeat `stage_files` was `0.869s`, `1.166s`, `0.791s` |
| `add-dirty` | `0.227s` | `0.363s` | not closed; one Zmin outlier had `stage_file_seconds=0.379s`, clean repeats were `0.117s` and `0.155s` |

The fanout cache is accepted as a low-level object-store cleanup that mostly
helps initial add. Windows `add-dirty` remains open; the next useful work is
inside the 100-file restaging path, with more detail around small object
compression/write/install and per-file index refresh cost.

Follow-up loose-object fast-compression slice: loose object writes now use fast
zlib compression. Git object identity is unchanged because object ids hash the
uncompressed Git object header and content, not the zlib byte stream. Pack
compression remains unchanged.

Validation:

- `cargo fmt --all -- --check`
- `cargo test -p zmin-git-core streamed_blob -- --nocapture` (`5/5`
  passing)
- `cargo test -p zmin-git-core loose_object -- --nocapture` (`9/9`
  passing)
- `cargo test -p zmin-cli --test git_index_mutation_compat add_ --
  --nocapture` (`30/30` passing)
- `cargo test -p zmin-cli --test git_status_compat -- --nocapture` (`4/4`
  passing)
- targeted macOS upstream `t3700-add.sh`:
  `/var/folders/l3/y2d_2zz51z731b86_sstzz0h0000gn/T//zmin-upstream-compat.LcmV4y/summary.tsv`
  (`1/1` passing)
- `ZMIN_WINDOWS_BENCH_PHASE_TRACE=1 tools/parallels-windows-runner.sh benchmark 3`
  with rows/checks/traces at
  `C:\Users\zmin\zmin-bench-20260616T135411Z-1874-out`
  (all benchmark checks `ok`)

Key Windows 3-repeat rows after fast loose compression:

| Operation | Git mean | Zmin mean | Note |
| --- | ---: | ---: | --- |
| `add` | `0.899s` | `1.020s` | still behind Git on mean, but one repeat beat Git (`0.816s` Zmin vs `0.830s` Git); Zmin `stage_files` was `1.041s`, `0.677s`, `0.818s` |
| `add-dirty` | `0.198s` | `0.190s` | green for this Windows fixture; Zmin `stage_file_seconds` dropped to `0.099s`, `0.081s`, `0.085s` |

This closes the immediate Windows `add-dirty` fixture gap for the current
benchmark, but not the broader performance goal. Initial Windows `add`, noisy
`status`, clone, larger repositories, and cross-platform multi-run gates remain
active.

Rejected follow-up experiment: after fast loose compression, the small-file
in-memory path was briefly widened from exact-existing index entries to all
regular files at or below 64 KiB. Correctness stayed green (`t3700-add.sh`
passed at
`/var/folders/l3/y2d_2zz51z731b86_sstzz0h0000gn/T//zmin-upstream-compat.FeAyyL/summary.tsv`),
but Windows `benchmark 3` regressed initial `add` (`1.118s` Zmin vs `0.876s`
Git, traces at `C:\Users\zmin\zmin-bench-20260616T140543Z-15360-out`).
The condition was reverted to the accepted exact-existing guard. Final
post-revert validation:

- `cargo fmt --all -- --check`
- `cargo test -p zmin-git-core streamed_blob -- --nocapture` (`5/5`
  passing)
- `cargo test -p zmin-git-core loose_object -- --nocapture` (`9/9`
  passing)
- `cargo test -p zmin-cli --test git_index_mutation_compat add_ --
  --nocapture` (`30/30` passing)
- `cargo test -p zmin-cli --test git_status_compat -- --nocapture` (`4/4`
  passing)
- targeted macOS upstream `t3700-add.sh`:
  `/var/folders/l3/y2d_2zz51z731b86_sstzz0h0000gn/T//zmin-upstream-compat.WgJ67q/summary.tsv`
  (`1/1` passing)

Follow-up stage-files detail trace and production Windows gate:
`ZMIN_PHASE_TRACE` now has an aggregate `add.stage_files.detail` row for the
file-staging loop. The row is emitted only when phase tracing is enabled, so the
normal add path does not pay the extra timing/counter cost. The detail buckets
separate metadata reads, object writes, parent cleanup, index upserts, and file
kind/count decisions.

Diagnostic Windows trace evidence:

- `ZMIN_WINDOWS_BENCH_PHASE_TRACE=1 tools/parallels-windows-runner.sh benchmark 3`
  with rows/checks/traces at
  `C:\Users\zmin\zmin-bench-20260616T141837Z-30949-out`
  (all benchmark checks `ok`)
- initial `add -A` staged `800` regular files, all on the streamed path
  (`streamed_files=800`, `small_existing_files=0`)
- `add.stage_files.detail` showed object writes dominating the initial-add
  stage: representative `object_write_seconds` values were `1.617s`, `0.837s`,
  and `0.874s`, while metadata, parent cleanup, and upsert buckets were much
  smaller
- dirty `add -A` had `files=0` in `add.stage_files.detail`; its remaining work
  is in the tracked-pass/restage path, not the initial file loop

Production, non-trace validation after the conditional trace path:

- targeted macOS upstream `t3700-add.sh`:
  `/var/folders/l3/y2d_2zz51z731b86_sstzz0h0000gn/T//zmin-upstream-compat.6sTRix/summary.tsv`
  (`1/1` passing)
- `tools/parallels-windows-runner.sh benchmark 3`
- rows:
  `C:\Users\zmin\zmin-bench-20260616T143025Z-43123-out\bench.csv`
- checks:
  `C:\Users\zmin\zmin-bench-20260616T143025Z-43123-out\checks.csv`
  (`status-output`, `log-output`, `rev-list-output`, `commit-dirty`, `clone`,
  `fetch-noop`, and `fetch-incremental` checks all `ok`)

Key Windows non-trace means:

| Operation | Git mean | Zmin mean | Note |
| --- | ---: | ---: | --- |
| `add` | `1.377s` | `1.484s` | still behind Git on mean; first repeat was faster (`1.144s` Zmin vs `1.426s` Git), second repeat was a Zmin outlier |
| `add-dirty` | `0.221s` | `0.243s` | close but not closed; third repeat beat Git (`0.232s` Zmin vs `0.245s` Git) |
| `status` | `0.091s` | `0.110s` | still a small Windows gap |
| `clone` | `0.776s` | `1.696s` | mean dominated by first Zmin outlier (`3.460s`); repeats 2 and 3 beat or matched Git closely |
| `fetch-noop` | `0.850s` | `0.077s` | still substantially faster than Git |
| `fetch-incremental` | `1.228s` | `0.487s` | still faster than Git |
| `commit-dirty` | `0.157s` | `0.076s` | still faster than Git |

The current next target is evidence-led: reduce initial-add streamed loose object
write cost inside `write_streamed_blob_content`, and separately investigate the
Windows clone/status outliers. Do not retry the rejected all-small in-memory add
path without new evidence, because it already regressed initial add.

Follow-up streamed loose-object install slice: unknown-id streamed blob writes
now skip the post-compression `path.exists()` check and rely on the existing
atomic `install_temp_object_file` `AlreadyExists` branch to remove the unknown
temp file when the object is already present. This removes one per-file stat
from initial add without changing object ids, zlib bytes, fanout paths, or the
hard-link based install semantics. A focused test covers duplicate streamed
content and verifies that the temporary root object file is removed.

Rejected in the same slice: wrapping streamed loose-object temp files in
`BufWriter`. Correctness stayed green, but the Windows non-trace benchmark at
`C:\Users\zmin\zmin-bench-20260616T144618Z-66561-out` had an initial-add
Zmin outlier (`4.514s`) and worse `add` mean (`2.584s` Zmin vs `1.815s`
Git), so the buffering change was removed.

Validation for the final no-`exists()` variant:

- `cargo fmt --all -- --check`
- `cargo test -p zmin-git-core streamed_blob -- --nocapture` (`6/6`
  passing)
- `cargo test -p zmin-git-core loose_object -- --nocapture` (`9/9`
  passing)
- `cargo test -p zmin-cli --test git_index_mutation_compat add_ --
  --nocapture` (`30/30` passing)
- `cargo test -p zmin-cli --test git_status_compat -- --nocapture` (`4/4`
  passing)
- targeted macOS upstream `t3700-add.sh`:
  `/var/folders/l3/y2d_2zz51z731b86_sstzz0h0000gn/T//zmin-upstream-compat.xNwUKb/summary.tsv`
  (`1/1` passing)
- `tools/parallels-windows-runner.sh benchmark 3`
- rows:
  `C:\Users\zmin\zmin-bench-20260616T145805Z-76918-out\bench.csv`
- checks:
  `C:\Users\zmin\zmin-bench-20260616T145805Z-76918-out\checks.csv`
  (`status-output`, `log-output`, `rev-list-output`, `commit-dirty`, `clone`,
  `fetch-noop`, and `fetch-incremental` checks all `ok`)

Key Windows non-trace means for the final no-`exists()` variant:

| Operation | Git mean | Zmin mean | Note |
| --- | ---: | ---: | --- |
| `add` | `1.017s` | `1.136s` | still behind Git on mean; repeat 2 beat Git (`0.837s` Zmin vs `0.924s` Git) |
| `add-dirty` | `0.316s` | `0.270s` | faster than Git in this run |
| `status` | `0.080s` | `0.076s` | faster than Git in this run |
| `clone` | `0.733s` | `1.520s` | still the main Windows performance gap |
| `fetch-noop` | `0.519s` | `0.063s` | still substantially faster than Git |
| `fetch-incremental` | `0.599s` | `0.217s` | still faster than Git |
| `commit-dirty` | `0.195s` | `0.125s` | still faster than Git |

The Windows add path is improved but not complete: initial `add` remains behind
Git on mean, and clone is now the clearest local benchmark gap. The next
performance pass should profile local clone and continue reducing streamed
object write/install cost for initial add.

Follow-up local clone smudge-filter pass slice: fresh checkout already applies
Git-compatible ident/eol smudge while writing files, so the post-checkout pass
only needs to handle external `filter=<name>` smudge drivers. The pass now loads
root `.gitattributes` once, returns immediately when there are no attributes,
and checks for a path-specific `filter` before reading the checked-out file. This
removes a full second read of every checkout file for the common no-filter local
clone case without changing filter semantics.

Diagnostic evidence before the change:

- Windows trace:
  `C:\Users\zmin\zmin-bench-20260616T151458Z-92433-out\phase-traces\clone-1_local-37ad3bf3cc224f46b1db2bbbb684fc46.log`
- `checkout_fresh.smudge_filters` was `1.061642s`, dominating the `1.689174s`
  `clone_local` phase

Validation for the cached/no-filter smudge pass:

- `cargo fmt --all -- --check`
- `cargo test -p zmin-cli --test git_clone_compat clone_ -- --nocapture`
  (`7/7` selected clone tests passing)
- `cargo test -p zmin-cli --test git_worktree_state_compat checkout_ --
  --nocapture` (`13/13` selected checkout tests passing)
- targeted macOS upstream `t0021-conversion.sh`:
  `/var/folders/l3/y2d_2zz51z731b86_sstzz0h0000gn/T//zmin-upstream-compat.mdLzl2/summary.tsv`
  (`1/1` passing)
- targeted Windows/Git-for-Windows upstream `t0021-conversion.sh`:
  `/c/Users/zmin/zmin-upstream-20260616T153545Z-13383-out/summary.tsv`
  (`1/1` passing)
- Windows trace after the change:
  `C:\Users\zmin\zmin-bench-20260616T152623Z-10861-out\phase-traces\clone-1_local-2e9d2e129afa42238f97677cd19f0c10.log`
- `checkout_fresh.smudge_filters` dropped to `0.000981s`; the trace run itself
  was a noisy checkout outlier, so production judgment uses the non-trace run
  below
- `tools/parallels-windows-runner.sh benchmark 3`
- rows:
  `C:\Users\zmin\zmin-bench-20260616T153332Z-12352-out\bench.csv`
- checks:
  `C:\Users\zmin\zmin-bench-20260616T153332Z-12352-out\checks.csv`
  (`status-output`, `log-output`, `rev-list-output`, `commit-dirty`, `clone`,
  `fetch-noop`, and `fetch-incremental` checks all `ok`)

Key Windows non-trace means after the clone smudge-filter pass change:

| Operation | Git mean | Zmin mean | Note |
| --- | ---: | ---: | --- |
| `clone` | `0.511s` | `0.499s` | local clone fixture is faster than Git on mean |
| `add` | `1.015s` | `1.114s` | still behind Git on mean |
| `add-dirty` | `0.267s` | `0.219s` | faster than Git in this run |
| `status` | `0.069s` | `0.077s` | small Windows gap remains |
| `fetch-noop` | `0.902s` | `0.058s` | still substantially faster than Git |
| `fetch-incremental` | `0.341s` | `0.179s` | still faster than Git |

The immediate Windows local clone fixture gap is closed by this slice, but the
broader performance gate remains open: initial `add`, status noise, larger
repositories, HTTP clone/fetch, sparse/worktree-first scenarios, and repeated
cross-platform benchmark runs still need evidence.

Follow-up loose-object header write cleanup: streamed loose blob writes and
in-memory loose object encoding now write the loose-object header (`"<kind>
<size>\0"`) through one stack-buffered `write_all` instead of several small
writes plus formatting. This is a narrow per-object overhead cleanup for the
initial-add hot path; it does not change Git object ids, zlib settings,
temporary object installation, or the existing streamed-vs-in-memory staging
policy.

Validation:

- `cargo fmt --all -- --check`
- `cargo test -p zmin-git-core streamed_blob -- --nocapture` (`6/6`
  passing)
- `cargo test -p zmin-git-core loose_object -- --nocapture` (`9/9`
  passing)
- `cargo test -p zmin-cli --test git_index_mutation_compat add_ --
  --nocapture` (`30/30` passing)
- `cargo test -p zmin-cli --test git_status_compat -- --nocapture` (`4/4`
  passing)
- targeted macOS upstream `t3700-add.sh`:
  `/var/folders/l3/y2d_2zz51z731b86_sstzz0h0000gn/T//zmin-upstream-compat.Z5Hzh8/summary.tsv`
  (`1/1` passing)
- targeted Windows/Git-for-Windows upstream `t3700-add.sh`:
  `/c/Users/zmin/zmin-upstream-20260616T155408Z-37038-out/summary.tsv`
  (`1/1` passing)

Windows benchmark evidence:

- `tools/parallels-windows-runner.sh benchmark 3`
  rows/checks:
  `C:\Users\zmin\zmin-bench-20260616T154308Z-28326-out\bench.csv`,
  `C:\Users\zmin\zmin-bench-20260616T154308Z-28326-out\checks.csv`
- cached follow-up `tools/parallels-windows-runner.sh benchmark 3`
  rows/checks:
  `C:\Users\zmin\zmin-bench-20260616T155144Z-34177-out\bench.csv`,
  `C:\Users\zmin\zmin-bench-20260616T155144Z-34177-out\checks.csv`
- all benchmark checks were `ok` in both runs

Key Windows observations across the two 3-repeat runs:

| Operation | Evidence |
| --- | --- |
| `add` | first run: Zmin `0.885s` vs Git `0.925s`; second run: Zmin `1.433s` vs Git `1.370s`, with one Zmin outlier (`1.904s`) and 2/3 repeats faster than Git |
| `add-dirty` | first run: Zmin `0.247s` vs Git `0.226s`; second run: Zmin `0.221s` vs Git `0.219s`; still effectively noisy/even, not closed |
| `status` | first run: Zmin `0.090s` vs Git `0.083s`; second run: Zmin `0.078s` vs Git `0.108s`; still noise-sensitive |
| `clone` | first run had a Zmin clone outlier; second run was close but Zmin still behind on mean (`0.498s` vs `0.438s`) |

Keep this as an accepted object-store micro-cleanup, not as a closed performance
gate. The next meaningful add work still needs either less noisy evidence or a
larger reduction in the streamed object write/install path.

Follow-up local clone fresh-checkout threshold slice: after the smudge-filter
pass, the remaining Windows local clone regression moved into fresh checkout.
The benchmark fixture has 480 worktree entries, but the existing parallel fresh
checkout path only enabled at 512 entries. Lowering the threshold to 256 lets
medium worktrees use the already-existing parallel checkout implementation
without changing checkout semantics, filter handling, or the repository format.

Diagnostic evidence before the threshold change:

- Windows 5-repeat comparison:
  `C:\Users\zmin\zmin-bench-20260616T160118Z-53898-out\comparison.csv`
- `clone` was still behind Git: mean ratio `1.521070`, median ratio
  `1.532775`
- Windows phase trace:
  `C:\Users\zmin\zmin-bench-20260616T160440Z-57383-out\phase-traces\clone-1_local-165999b4d88249988166c5d5060c668d.log`
- `checkout_index_fresh_into_metadata` had `entries=480`; the old 512-entry
  threshold kept this fixture on the serial fresh-checkout path

Validation for the threshold change:

- `cargo fmt --all -- --check`
- `cargo test -p zmin-cli --test git_clone_compat clone_ -- --nocapture`
  (`7/7` selected clone tests passing)
- `cargo test -p zmin-cli --test git_worktree_state_compat checkout_ --
  --nocapture` (`13/13` selected checkout tests passing)
- targeted macOS upstream `t2000-conflict-when-checking-files-out.sh` and
  `t0021-conversion.sh`:
  `/var/folders/l3/y2d_2zz51z731b86_sstzz0h0000gn/T//zmin-upstream-compat.8E4YHA/summary.tsv`
  (`2/2` passing)
- targeted Windows/Git-for-Windows upstream
  `t2000-conflict-when-checking-files-out.sh`:
  `/c/Users/zmin/zmin-upstream-20260616T162013Z-70062-out/summary.tsv`
  (`1/1` passing)
- `tools/parallels-windows-runner.sh benchmark 5`
- rows/checks/comparison:
  `C:\Users\zmin\zmin-bench-20260616T160912Z-64776-out\bench.csv`,
  `C:\Users\zmin\zmin-bench-20260616T160912Z-64776-out\checks.csv`,
  `C:\Users\zmin\zmin-bench-20260616T160912Z-64776-out\comparison.csv`
- all benchmark checks were `ok`

Key Windows 5-repeat ratios after the threshold change:

| Operation | Git median | Zmin median | Zmin/Git median ratio | Zmin/Git mean ratio | Note |
| --- | ---: | ---: | ---: | ---: | --- |
| `clone` | `0.624501s` | `0.452820s` | `0.725091` | `0.697418` | local clone fixture is now faster than Git on mean and median |
| `add` | `1.236434s` | `1.319451s` | `1.067142` | `1.654963` | median remains slightly behind; mean has a Zmin outlier |
| `add-dirty` | `0.262768s` | `0.299813s` | `1.140980` | `0.629616` | median remains behind; Git had one large outlier on mean |
| `status` | `0.099910s` | `0.080331s` | `0.804034` | `0.788044` | faster than Git in this run |
| `rev-list` | `0.066000s` | `0.105173s` | `1.593530` | `1.379389` | still a clear local benchmark gap |

This closes the immediate Windows local clone fixture gap with repeated
evidence. It does not close the broader performance goal: initial `add`,
`add-dirty` median behavior, `rev-list`, larger repositories, HTTP clone/fetch,
sparse/worktree-first scenarios, and cross-platform benchmark gates remain open.

Follow-up `rev-list --objects --all` commit-tree reuse slice: the Windows
benchmark showed a clear `rev-list` median gap after the clone threshold work.
For the normal `--objects` output path without `--parents` or `--children`,
Zmin was collecting commits once for history traversal and then reading the
same commits again while walking object paths only to recover each commit tree
id. The new path reuses `CollectedCommitTree` values for object-line traversal,
so commit ids are still printed in the same order and object path output uses
the same tree walk, but the second commit-object read is removed for this common
plumbing case.

Validation:

- `cargo fmt --all -- --check`
- `cargo test -p zmin-cli rev_list -- --nocapture` (`9/9` focused unit tests
  passing across the CLI binaries plus selected compat tests)
- `cargo test -p zmin-cli --test git_history_query_compat rev_list --
  --nocapture` (`3/3` passing)
- `cargo test -p zmin-cli --test git_pack_integrity_compat rev_list --
  --nocapture` found no tests for that filter (`0` selected)
- `tools/parallels-windows-runner.sh benchmark 5`
- rows/checks/comparison:
  `C:\Users\zmin\zmin-bench-20260616T162711Z-82605-out\bench.csv`,
  `C:\Users\zmin\zmin-bench-20260616T162711Z-82605-out\checks.csv`,
  `C:\Users\zmin\zmin-bench-20260616T162711Z-82605-out\comparison.csv`
- all benchmark checks were `ok`, including exact `rev-list-output`
  comparison against Git

Key Windows 5-repeat ratios after the `rev-list` reuse change:

| Operation | Git median | Zmin median | Zmin/Git median ratio | Zmin/Git mean ratio | Note |
| --- | ---: | ---: | ---: | ---: | --- |
| `rev-list` | `0.060632s` | `0.047215s` | `0.778714` | `0.903492` | now faster than Git on mean and median |
| `add` | `0.973845s` | `1.062131s` | `1.090657` | `1.073809` | still behind Git |
| `add-dirty` | `0.197094s` | `0.202360s` | `1.026718` | `1.018957` | close but still behind on this run |
| `status` | `0.058939s` | `0.062650s` | `1.062963` | `1.057122` | small gap remains |
| `clone` | `0.395150s` | `0.481817s` | `1.219327` | `1.315260` | variance reopened; repeat 1 had a Zmin outlier, repeats 4 and 5 were near/faster than Git |

This closes the immediate `rev-list` fixture gap while preserving exact output
compatibility. The broader performance gate remains open: `add`, `add-dirty`,
status variance, clone variance, larger repositories, HTTP clone/fetch,
sparse/worktree-first scenarios, and cross-platform benchmark gates still need
burn-down.

Follow-up clean `status` stat-safety slice: the next trace showed clean
porcelain status spending most of its command time in tracked worktree
inspection. `add` already used the racy-index-safe stat fast path by comparing
entry metadata against the index mtime; `status` was passing no index mtime, so
clean regular files could fall through to content hashing. `worktree_status`
now reads the index mtime once, uses the same strict "entry mtime older than
index mtime" guard as `add`, and reuses the first `symlink_metadata` result
instead of probing existence and then statting the same path again. Missing or
stat-error paths keep the previous deleted-path behavior.

Diagnostic evidence before the change:

- Windows trace:
  `C:\Users\zmin\zmin-bench-20260616T163927Z-95853-out\phase-traces\status-1_clean-9fd2e855b4244afbb31b8ffe7e627b56.log`
- `status.worktree_status=0.036759s`, `status.total=0.047536s`
- same run comparison:
  `C:\Users\zmin\zmin-bench-20260616T163927Z-95853-out\comparison.csv`
- `status` was behind Git in that repeat: Zmin `0.065026s` vs Git
  `0.061108s`

Validation:

- `cargo fmt --all -- --check`
- `cargo test -p zmin-cli --test git_status_compat -- --nocapture` (`4/4`
  passing, including same-size/same-mtime content-change detection)
- `cargo test -p zmin-cli --test git_index_mutation_compat add_ --
  --nocapture` (`30/30` passing)
- `cargo test -p zmin-cli --test git_worktree_state_compat status_ --
  --nocapture` selected no tests (`0` selected)
- post-change Windows trace:
  `C:\Users\zmin\zmin-bench-20260616T164710Z-1609-out\phase-traces`
- post-change status trace:
  `status.worktree_status=0.012354s`, `status.total=0.023350s`
- `tools/parallels-windows-runner.sh benchmark 5`
- rows/checks/comparison:
  `C:\Users\zmin\zmin-bench-20260616T165321Z-3734-out\bench.csv`,
  `C:\Users\zmin\zmin-bench-20260616T165321Z-3734-out\checks.csv`,
  `C:\Users\zmin\zmin-bench-20260616T165321Z-3734-out\comparison.csv`
- all benchmark checks were `ok`, including exact `status-output`
  comparison against Git

Key Windows 5-repeat ratios after the status stat-safety change:

| Operation | Git median | Zmin median | Zmin/Git median ratio | Zmin/Git mean ratio | Note |
| --- | ---: | ---: | ---: | ---: | --- |
| `status` | `0.061532s` | `0.037452s` | `0.608659` | `0.567639` | clean status fixture is now faster than Git on mean and median |
| `clone` | `0.619177s` | `0.454583s` | `0.734173` | `0.664477` | local clone fixture faster than Git in this run |
| `add-dirty` | `0.263662s` | `0.225953s` | `0.856980` | `0.778280` | dirty add faster than Git in this run |
| `rev-list` | `0.058636s` | `0.047184s` | `0.804693` | `0.851910` | still faster than Git after the previous slice |
| `add` | `1.157697s` | `1.206349s` | `1.042025` | `1.088446` | remaining immediate local benchmark gap |

This closes the immediate clean `status` fixture gap while preserving the racy
index safety covered by the focused status regression tests. The broader
performance gate remains open: initial `add`, larger repositories, HTTP
clone/fetch, sparse/worktree-first scenarios, and broader cross-platform gates
still need burn-down.

Follow-up initial `add -A` parallel staging slice: after status became faster
than Git, the remaining local Windows fixture gap was initial `add` over 800
new regular files. Trace evidence showed the command was dominated by streamed
loose-object writes (`add.stage_files.detail` had `streamed_files=800` and
`object_write_seconds` around `0.99s` in the pre-slice run). The accepted path
keeps index mutation sequential, but computes loose blob objects in parallel
for the narrow safe case: `add -A` without chmod, intent-to-add, ignored-error
handling, symlinks, content conversion, existing stage-zero entries, or
unmerged stages. Any unsupported candidate falls back to the existing
sequential staging path.

Validation:

- `cargo fmt --all -- --check`
- `cargo test -p zmin-cli --test git_index_mutation_compat add_ --
  --nocapture` (`30/30` passing)
- `cargo test -p zmin-cli --test git_status_compat -- --nocapture` (`4/4`
  passing)
- trace run with `ZMIN_WINDOWS_BENCH_PHASE_TRACE=1
  tools/parallels-windows-runner.sh benchmark 1`
- trace rows/checks/phase traces:
  `C:\Users\zmin\zmin-bench-20260616T170606Z-20291-out\bench.csv`,
  `C:\Users\zmin\zmin-bench-20260616T170606Z-20291-out\checks.csv`,
  `C:\Users\zmin\zmin-bench-20260616T170606Z-20291-out\phase-traces`
- trace evidence for initial add:
  `add.stage_files.detail=0.624718s`, `files=800`,
  `streamed_files=800`, `object_write_seconds=0.606909s`,
  `add.total=0.715382s`; the timed row was Zmin `0.782197s` vs Git
  `1.128684s`
- `tools/parallels-windows-runner.sh benchmark 5`
- rows/checks/comparison:
  `C:\Users\zmin\zmin-bench-20260616T171240Z-21594-out\bench.csv`,
  `C:\Users\zmin\zmin-bench-20260616T171240Z-21594-out\checks.csv`,
  `C:\Users\zmin\zmin-bench-20260616T171240Z-21594-out\comparison.csv`
- all benchmark checks were `ok`, including exact `status-output`,
  `log-output`, `rev-list-output`, and per-repeat clone/fetch/commit checks

Key Windows 5-repeat ratios after the parallel staging change:

| Operation | Git median | Zmin median | Zmin/Git median ratio | Zmin/Git mean ratio | Note |
| --- | ---: | ---: | ---: | ---: | --- |
| `add` | `1.173683s` | `1.126044s` | `0.959412` | `0.849406` | initial add fixture is now faster than Git on mean and median |
| `add-dirty` | `0.322575s` | `0.236419s` | `0.732911` | `0.788300` | still faster after the initial-add change |
| `status` | `0.062871s` | `0.037768s` | `0.600718` | `0.561391` | clean status remains faster than Git |
| `clone` | `0.470137s` | `0.443322s` | `0.942963` | `0.744471` | local clone remains faster than Git |
| `rev-list` | `0.072051s` | `0.058551s` | `0.812632` | `0.930080` | still faster than Git |

macOS follow-up smoke:

- `ZMIN_BENCH_REPEATS=3 tools/git-performance-bench.sh >
  /tmp/zmin-macos-parallel-add-bench-20260616.tsv`
- parsed locally from the TSV: `129` timing rows, `27` validation rows, `0`
  validation failures

Key macOS 3-repeat ratios:

| Operation | Git median | Zmin median | Zmin/Git median ratio | Zmin/Git mean ratio | Note |
| --- | ---: | ---: | ---: | ---: | --- |
| `add` | `1.390000s` | `0.390000s` | `0.280576` | `0.291866` | parallel staging is a clear win on macOS too |
| `add-dirty` | `0.150000s` | `0.120000s` | `0.800000` | `0.800000` | still faster than Git |
| `status` | `0.020000s` | `0.010000s` | `0.500000` | `0.500000` | clean status remains faster than Git |
| `clone` | `0.120000s` | `0.120000s` | `1.000000` | `1.057143` | effectively tied at this small scale |
| `rev-list` | `0.020000s` | `0.030000s` | `1.500000` | `1.333333` | small absolute gap remains |
| `fetch-incremental` | `0.060000s` | `0.080000s` | `1.333333` | `1.388889` | small absolute gap remains |
| `commit` | `0.060000s` | `0.080000s` | `1.333333` | `1.222222` | small absolute gap remains |

This closes the immediate local Windows benchmark gaps in `add`, `add-dirty`,
`status`, `clone`, and `rev-list` for the current fixture and confirms that the
initial-add staging change is also a macOS win. The broader performance gate
remains open: macOS short-operation gaps (`rev-list`, incremental fetch,
commit), larger repositories, HTTP clone/fetch, sparse and worktree-first
scenarios, macOS-vs-Windows repeated gates, memory/RSS, and the planned
worktree-first, built-in hooks, and CMS-like porcelain work still need
implementation and evidence.

Follow-up smart HTTP performance gate slice: the local benchmark suite now has a
dedicated `tools/git-http-performance-bench.sh` gate. It starts a local CGI
`git-http-backend`, serves the same bare repository to upstream Git and
`zmin`, measures smart-HTTP clone, noop fetch, incremental fetch, and a
many-file batch fetch, and writes `bench.tsv`, `checks.tsv`, `summary.tsv`, and
`comparison.tsv`.

The first smoke run exposed a real compatibility bug: smart-HTTP incremental
fetch could receive a thin pack with ref-delta bases that already existed in
the client repository, then fail during pack indexing with
`ref-delta base object not found`. The fix teaches the smart HTTP, SSH, and
git-daemon negotiated fetch install paths to use the existing thin-pack repair
path when the upload-pack request includes `have` lines. Clone/no-have paths
still use the direct index path.

Validation:

- `cargo fmt --all -- --check`
- `cargo build --release -p zmin-cli --bin zmin`
- `cargo test -p zmin-cli --test git_transport_http_compat
  fetch_smart_http_incremental_thin_pack_repairs_existing_bases_like_stock_git
  -- --nocapture` (`1/1` passing)
- `cargo test -p zmin-cli --test git_transport_http_compat fetch --
  --nocapture` (`20/20` passing)
- `cargo test -p zmin-cli --test git_transport_local_compat fetch_ --
  --nocapture` (`58/58` passing)
- Windows/Git-for-Windows targeted validation:
  `tools/parallels-windows-runner.sh validate targeted git_transport_http_compat
  fetch_smart_http_incremental_thin_pack_repairs_existing_bases_like_stock_git`
  (`1/1` passing; Git for Windows `2.54.0.windows.1`)
- Windows/Git-for-Windows targeted validation for smart HTTP noop pack skipping:
  `tools/parallels-windows-runner.sh validate targeted git_transport_http_compat
  fetch_smart_http_noop_skips_upload_pack_when_roots_exist_locally`
  (`1/1` passing; Git for Windows `2.54.0.windows.1`)
- HTTP smoke after the fix:
  `/tmp/zmin-http-bench-smoke-20260616` with clone, noop fetch, incremental
  fetch, and batch fetch checks all `ok`
- default 3-repeat HTTP gate after the thin-pack fix:
  `/tmp/zmin-http-bench-20260616/bench.tsv`,
  `/tmp/zmin-http-bench-20260616/checks.tsv`,
  `/tmp/zmin-http-bench-20260616/comparison.tsv`
- 3-repeat HTTP gate after noop roots filtering:
  `/tmp/zmin-http-noop-skip-20260616/bench.tsv`,
  `/tmp/zmin-http-noop-skip-20260616/checks.tsv`,
  `/tmp/zmin-http-noop-skip-20260616/comparison.tsv`
- HTTP gate checks all `ok` for `clone-http-1..3`, `fetch-http-noop`,
  `fetch-http-incremental-1..3`, and `fetch-http-batch-1..3`

Follow-up noop optimization: configured smart HTTP fetch now filters wanted
roots that already exist in the local object store before opening upload-pack.
The existing discovery, ref update, and validation flow remains in place, but a
true noop fetch no longer asks the server for an empty pack.

Key macOS smart-HTTP 3-repeat ratios after the noop optimization:

| Operation | Git median | Zmin median | Zmin/Git median ratio | Zmin/Git mean ratio | Note |
| --- | ---: | ---: | ---: | ---: | --- |
| `clone-http` | `0.200000s` | `0.150000s` | `0.750000` | `0.790323` | smart HTTP clone is faster than Git on this local CGI fixture |
| `fetch-http-incremental` | `0.140000s` | `0.090000s` | `0.642857` | `0.707317` | incremental thin-pack fetch is now correct and faster |
| `fetch-http-batch` | `0.150000s` | `0.120000s` | `0.800000` | `0.795455` | many-file HTTP fetch is faster |
| `fetch-http-noop` | `0.080000s` | `0.050000s` | `0.625000` | `0.625000` | noop fetch skips upload-pack when advertised roots already exist locally |

Windows/Git-for-Windows smart HTTP automation follow-up:
`tools/windows-native-http-benchmark.ps1` now runs the same local smart HTTP
shape without requiring Python. It uses a small PowerShell TCP loopback server
that invokes `git http-backend`, writes `bench.csv`, `checks.csv`,
`summary.csv`, `comparison.csv`, and `http-server.log`, and is exposed through
`tools/parallels-windows-runner.sh http-benchmark [repeats]`.

Validation:

- `bash -n tools/parallels-windows-runner.sh`
- first Windows smoke found the old bash/Python path was not portable because
  the guest only had Microsoft Store Python aliases, not a real `python3`
- `tools/parallels-windows-runner.sh http-benchmark 1` after the PowerShell
  server change passed and wrote:
  `C:\Users\zmin\zmin-http-bench-20260616T182644Z-14223-out`
- `tools/parallels-windows-runner.sh http-benchmark 3` passed with empty
  `http-server.log`; rows/checks/comparison:
  `C:\Users\zmin\zmin-http-bench-20260616T182744Z-14487-out\bench.csv`,
  `C:\Users\zmin\zmin-http-bench-20260616T182744Z-14487-out\checks.csv`,
  `C:\Users\zmin\zmin-http-bench-20260616T182744Z-14487-out\comparison.csv`
- Windows checks were all `ok` for `clone-http-1..3`, `fetch-http-noop`,
  `fetch-http-incremental-1..3`, and `fetch-http-batch-1..3`

Key Windows/Git-for-Windows smart-HTTP 3-repeat ratios:

| Operation | Git median | Zmin median | Zmin/Git median ratio | Zmin/Git mean ratio | Note |
| --- | ---: | ---: | ---: | ---: | --- |
| `clone-http` | `2.291041s` | `1.418489s` | `0.619148` | `0.737387` | smart HTTP clone is faster than Git on the local Windows loopback fixture |
| `fetch-http-incremental` | `0.938210s` | `0.749552s` | `0.798916` | `0.647534` | incremental smart HTTP fetch is faster |
| `fetch-http-batch` | `1.561567s` | `0.652611s` | `0.417920` | `0.451473` | many-file HTTP fetch is faster |
| `fetch-http-noop` | `0.605755s` | `0.204490s` | `0.337579` | `0.388221` | noop fetch remains faster after upload-pack skipping |

This adds the missing local smart-HTTP performance gate and closes the
incremental thin-pack correctness hole it exposed. The noop follow-up closes the
small local smart-HTTP noop fixture gap. The Windows follow-up adds native
Git-for-Windows HTTP benchmark automation and shows the local loopback fixture
faster than Git on all measured smart HTTP operations. The broader performance
goal remains open: larger real HTTP repositories, auth/proxy/network variants,
sparse and remote worktree-first scenarios, memory/RSS, broader hook UX, and
broader CMS-like porcelain still need implementation or evidence. The first
local-only `clone --worktree-first` / `clone --instant` correctness slice and
the first CMS-like slices (`save`, `changes`, `publish`, `update`, `undo`,
`timeline`, and `recover`) are correctness-validated separately in
`docs/cli/command_compatibility_audit.md`; they are not a performance gate.

Follow-up local worktree-first performance-gate slice: the macOS and Windows
local benchmark gates now include a dedicated `clone-instant` operation. The
operation compares stock `git clone` against `zmin clone --instant` on the
same local source repository and validates `HEAD`, `HEAD^{tree}`, and
`zmin.worktreeFirst=true` in the Zmin clone.

Validation:

- `bash -n tools/git-performance-bench.sh`
- macOS smoke:
  `ZMIN_BENCH_REPEATS=1 ZMIN_BENCH_COMMITS=4
  ZMIN_BENCH_FILES_PER_COMMIT=3 ZMIN_BENCH_WRITE_FILES=20
  ZMIN_BENCH_DIRTY_FILES=5 ZMIN_BENCH_FETCH_BATCH_FILES=20
  ZMIN_BENCH_PUSH_BATCH_FILES=20 tools/git-performance-bench.sh`
  with `clone-instant-1`, `clone-instant-1-tree`, and
  `clone-instant-1-marker` checks all `ok`
- Windows/Git-for-Windows smoke:
  `tools/parallels-windows-runner.sh benchmark 1`
- Windows rows/checks/comparison:
  `C:\Users\zmin\zmin-bench-20260616T211043Z-77868-out\bench.csv`,
  `C:\Users\zmin\zmin-bench-20260616T211043Z-77868-out\checks.csv`,
  `C:\Users\zmin\zmin-bench-20260616T211043Z-77868-out\comparison.csv`
- Windows checks for `clone-instant-1`, `clone-instant-1-tree`, and
  `clone-instant-1-marker` were all `ok`

Key Windows one-repeat smoke ratio:

| Operation | Git median | Zmin median | Zmin/Git median ratio | Zmin/Git mean ratio | Note |
| --- | ---: | ---: | ---: | ---: | --- |
| `clone-instant` | `0.367737s` | `0.343539s` | `0.934198` | `0.934198` | local `--instant` clone is present in the benchmark gate and faster in this smoke |

This closes the missing local worktree-first benchmark coverage gap. It does
not close the broader worktree-first performance goal: remote partial
hydration, demand hydration for missing objects, background history fetch, and
larger repeated worktree-first benchmark gates remain open.

Follow-up remote worktree-first correctness slice: smart HTTP `clone
--instant` now supports a first remote path without changing standard `clone`.
The smart HTTP instant path requests only the selected `HEAD` target initially,
materializes the worktree, writes only refs for objects it requested, records
`zmin.worktreeFirst=true`, and leaves additional branches/tags for a later
normal `fetch origin`. Validation: `cargo fmt --all -- --check`; macOS
`git_transport_http_compat
clone_instant_smart_http_materializes_head_then_fetch_hydrates_refs`; macOS
`git_transport_http_compat clone_reads_smart_http_pack_like_stock_git`; macOS
`git_clone_compat clone_worktree_first_rejects_non_worktree_or_remote_modes`;
Windows/Git-for-Windows targeted validation for the smart HTTP instant test;
and Windows/Git-for-Windows targeted validation for the unsupported
git-daemon regression. This does not close remote worktree-first performance:
SSH/git-daemon support decisions, demand hydration, background history fetch,
and repeated remote benchmark gates remain open.

Follow-up remote worktree-first performance-gate smoke: the macOS and
Windows/Git-for-Windows smart HTTP benchmark gates now include
`clone-http-instant`. The operation compares stock `git clone` against
`zmin clone --instant` on the same local smart HTTP source and validates
`HEAD`, `HEAD^{tree}`, and `zmin.worktreeFirst=true` in the Zmin clone.

Validation:

- `bash -n tools/git-http-performance-bench.sh`
- `cargo fmt --all -- --check`
- macOS smoke:
  `ZMIN_HTTP_BENCH_REPEATS=1 ZMIN_HTTP_BENCH_COMMITS=4
  ZMIN_HTTP_BENCH_FILES_PER_COMMIT=3 ZMIN_HTTP_BENCH_BATCH_FILES=20
  ZMIN_HTTP_BENCH_OUT_DIR=/tmp/zmin-http-instant-bench-smoke-20260616
  tools/git-http-performance-bench.sh`
- macOS rows/checks/comparison:
  `/tmp/zmin-http-instant-bench-smoke-20260616/bench.tsv`,
  `/tmp/zmin-http-instant-bench-smoke-20260616/checks.tsv`,
  `/tmp/zmin-http-instant-bench-smoke-20260616/comparison.tsv`
- Windows/Git-for-Windows smoke:
  `tools/parallels-windows-runner.sh http-benchmark 1`
- Windows rows/checks/comparison:
  `C:\Users\zmin\zmin-http-bench-20260616T215058Z-15171-out\bench.csv`,
  `C:\Users\zmin\zmin-http-bench-20260616T215058Z-15171-out\checks.csv`,
  `C:\Users\zmin\zmin-http-bench-20260616T215058Z-15171-out\comparison.csv`
- Windows checks for `clone-http-instant-1`,
  `clone-http-instant-1-tree`, and `clone-http-instant-1-marker` were all
  `ok`

Key one-repeat smoke ratios:

| Platform | Operation | Git median | Zmin median | Zmin/Git median ratio | Note |
| --- | --- | ---: | ---: | ---: | --- |
| macOS | `clone-http-instant` | `0.180000s` | `0.100000s` | `0.555556` | local smart HTTP instant clone is present in the benchmark gate and faster in this smoke |
| Windows/Git-for-Windows | `clone-http-instant` | `0.759531s` | `0.677775s` | `0.892360` | local smart HTTP instant clone is present in the Windows benchmark gate and faster in this smoke |

This closes the missing smart HTTP worktree-first benchmark coverage gap for
the local loopback fixture. It does not close the broader remote
worktree-first performance goal: repeated larger runs, real network/auth/proxy
variants, SSH/git-daemon support decisions, demand hydration, and background
history fetch remain open.

Follow-up Windows Gitoxide comparison tooling slice:
`tools/windows-native-benchmark.ps1` now detects `GIX_BIN` or `gix` on PATH and
records optional `tool=gix` rows for comparable CLI-adjacent operations:
`status`, `log`, `merge-base`, local `clone`, `fetch-noop`, and
`fetch-incremental`. `comparison.csv` now includes
`gix_mean_seconds`, `zmin_vs_gix_mean_ratio`, `gix_median_seconds`, and
`zmin_vs_gix_median_ratio` in addition to the existing Git-vs-Zmin columns.
When Gitoxide is not installed in the Windows guest, the benchmark prints
`gix=not-found (skipping Gitoxide rows)` and preserves the existing Git-vs-Zmin
gate behavior.

Validation:

- Windows/Git-for-Windows scoped no-Gitoxide smoke:
  `tools/parallels-windows-runner.sh benchmark 1 'status,log,merge-base'`
- Windows output:
  `C:\Users\zmin\zmin-bench-20260617T082128Z-72364-out\bench.csv`,
  `C:\Users\zmin\zmin-bench-20260617T082128Z-72364-out\checks.csv`,
  `C:\Users\zmin\zmin-bench-20260617T082128Z-72364-out\comparison.csv`
- The run printed `gix=not-found (skipping Gitoxide rows)`, selected only
  `log,merge-base,status`, and completed successfully. One-repeat Zmin/Git
  ratios in that no-gix smoke were `log` `0.468721`, `merge-base` `0.541372`,
  and `status` `0.580352`.

Follow-up Windows Gitoxide validation slice: the Parallels guest now has a
real Windows `gix.exe` installed from `gitoxide v0.54.0`. Direct guest commands
must add `C:\Users\zmin\.cargo\bin` to `PATH`, while the benchmark runner
already does that through its PowerShell environment setup. The first real-gix
run exposed that `gix fetch` requires a local committer identity for reflog
updates; the benchmark now configures the Git-format `gix-fetch` fixture with
the same local `user.name`, `user.email`, `commit.gpgsign=false`, and
`core.autocrlf=false` values used by the other fixture repositories.

Validation:

- Windows/Git-for-Windows one-repeat real-Gitoxide smoke:
  `tools/parallels-windows-runner.sh benchmark 1 'status,log,merge-base,clone,fetch-noop,fetch-incremental'`
- Windows one-repeat output:
  `C:\Users\zmin\zmin-bench-20260617T084941Z-85804-out\comparison.csv`
- Windows/Git-for-Windows three-repeat real-Gitoxide run:
  `tools/parallels-windows-runner.sh benchmark 3 'status,log,merge-base,clone,fetch-noop,fetch-incremental'`
- Windows three-repeat output:
  `C:\Users\zmin\zmin-bench-20260617T085033Z-86095-out\bench.csv`,
  `C:\Users\zmin\zmin-bench-20260617T085033Z-86095-out\checks.csv`,
  `C:\Users\zmin\zmin-bench-20260617T085033Z-86095-out\comparison.csv`
- The three-repeat run printed
  `gix=C:\Users\zmin\.cargo\bin\gix.exe`; all checks were `ok`, including
  exact `status-output`, exact `log-output`, `clone-*`, `clone-*-tree`,
  `fetch-noop`, and `fetch-incremental-*`.

Key Windows three-repeat real-Gitoxide ratios:

| Operation | Zmin/Git mean | Zmin/Git median | Zmin/Gitoxide mean | Zmin/Gitoxide median | Note |
| --- | ---: | ---: | ---: | ---: | --- |
| `clone` | `1.170528` | `1.228311` | `0.968114` | `1.146125` | mean is slightly faster than Gitoxide, but median is slower and clone remains a Windows gap |
| `fetch-incremental` | `0.420883` | `0.507099` | `0.774962` | `0.930816` | faster than Git and Gitoxide |
| `fetch-noop` | `0.140295` | `0.139300` | `0.263415` | `0.269569` | faster than Git and Gitoxide |
| `log` | `0.557696` | `0.624758` | `1.120625` | `1.098660` | faster than Git, slower than Gitoxide |
| `merge-base` | `0.463602` | `0.520787` | `0.967531` | `1.082647` | mean near Gitoxide, median slower than Gitoxide |
| `status` | `0.590609` | `0.594974` | `0.282032` | `0.231549` | faster than Git and Gitoxide |

This closes the missing Windows real-Gitoxide benchmark proof for the scoped
CLI-adjacent operations. It does not close the broader "better than Git and
Gitoxide" goal: Windows local `clone`, `log`, and `merge-base` still need
optimization or stronger repeated evidence, and the larger/full benchmark plus
remote/auth/proxy network variants remain open.

Follow-up Windows scoped log/merge-base real-Gitoxide refresh:
`log_with_options` now emits `ZMIN_PHASE_TRACE` rows for `log.total`,
`log.collect_revs`, `log.collect_commits`, `log.default_abbrev_len`, and
`log.render`, matching the existing performance-trace pattern used by status,
add, clone, checkout, fetch, and diff paths. The scoped Windows run below
showed that the earlier three-way `log` and `merge-base` Gitoxide gaps were not
stable in a focused run.

Validation:

- `cargo fmt --all -- --check`
- `cargo test -p zmin-git-core checkout -- --nocapture`
- Windows/Git-for-Windows scoped real-Gitoxide trace:
  `ZMIN_WINDOWS_BENCH_PHASE_TRACE=1 tools/parallels-windows-runner.sh benchmark 3 'log,merge-base'`
- Windows output:
  `C:\Users\zmin\zmin-bench-20260617T090422Z-97719-out\bench.csv`,
  `C:\Users\zmin\zmin-bench-20260617T090422Z-97719-out\checks.csv`,
  `C:\Users\zmin\zmin-bench-20260617T090422Z-97719-out\comparison.csv`,
  `C:\Users\zmin\zmin-bench-20260617T090422Z-97719-out\phase-traces`
- Checks: exact `log-output` was `ok`.

Key Windows three-repeat scoped ratios:

| Operation | Zmin/Git mean | Zmin/Git median | Zmin/Gitoxide mean | Zmin/Gitoxide median | Note |
| --- | ---: | ---: | ---: | ---: | --- |
| `log` | `0.558069` | `0.562375` | `0.672024` | `0.393903` | faster than Git and Gitoxide in the scoped run |
| `merge-base` | `0.399662` | `0.352668` | `0.829149` | `0.735039` | faster than Git and Gitoxide in the scoped run |

Trace details for `log`: `log.total` was `0.009865s` to `0.011735s`,
`log.collect_commits` was `0.007299s` to `0.008558s`,
`log.collect_revs` was below `0.001s`, `log.default_abbrev_len` was below
`0.001s`, and `log.render` was below `0.001s`.

Rejected local clone header-gated stream-probe experiment: a follow-up attempt
to avoid small/delta packed-object duplicate reads by checking
`object_header_hint` before `try_write_blob_to_path` did not hold up on
Windows. It eliminated stream attempts but moved the cost into header probing:
`stream_attempts=0`, `stream_write` `0.155335s` to `0.178890s`, and clone
regressed to Zmin/Gitoxide median ratio `4.239137` in
`C:\Users\zmin\zmin-bench-20260617T091411Z-1810-out`. The experiment was
removed from the worktree. Keep the next clone optimization focused on
checkout object-read/materialize behavior or fixture-aware design, not
pre-header probing and not a global checkout worker-cap increase.

Follow-up fresh-checkout empty-attributes micro-slice: fresh checkout now
builds a command-scoped `CheckoutContentRules` view and skips per-entry
`ident` / `eol=crlf` lookups when the root `.gitattributes` load produced no
rules. This is a narrow hot-path cleanup for clone fixtures without attributes;
repositories with attributes still run the existing per-path rules, and the
conversion guard remains upstream `t0021-conversion.sh`.

Validation:

- `cargo fmt --all -- --check`
- `cargo test -p zmin-git-core checkout -- --nocapture`
- `cargo test -p zmin-cli --test git_transport_http_compat clone_instant_ -- --nocapture`
- macOS upstream conversion guard:
  `ZMIN_UPSTREAM_TEST_LIST=tools/git-upstream-compat-tests-conversion.txt ZMIN_UPSTREAM_SKIP_UNSUPPORTED_REFTABLE=1 tools/git-upstream-compat-suite.sh exhaustive`
- macOS upstream output:
  `/var/folders/l3/y2d_2zz51z731b86_sstzz0h0000gn/T//zmin-upstream-compat.u2YqIX/summary.tsv`
- Windows/Git-for-Windows scoped clone trace:
  `ZMIN_WINDOWS_BENCH_PHASE_TRACE=1 tools/parallels-windows-runner.sh benchmark 3 'clone,clone-instant'`
- Windows scoped clone/instant output:
  `C:\Users\zmin\zmin-bench-20260617T093354Z-17476-out\bench.csv`,
  `C:\Users\zmin\zmin-bench-20260617T093354Z-17476-out\checks.csv`,
  `C:\Users\zmin\zmin-bench-20260617T093354Z-17476-out\comparison.csv`,
  `C:\Users\zmin\zmin-bench-20260617T093354Z-17476-out\phase-traces`
- Windows warm scoped clone rerun:
  `C:\Users\zmin\zmin-bench-20260617T094003Z-19029-out\bench.csv`,
  `C:\Users\zmin\zmin-bench-20260617T094003Z-19029-out\comparison.csv`,
  `C:\Users\zmin\zmin-bench-20260617T094003Z-19029-out\phase-traces`

Key Windows evidence after the empty-attributes cleanup:

| Operation | Zmin/Git mean | Zmin/Git median | Zmin/Gitoxide mean | Zmin/Gitoxide median | Note |
| --- | ---: | ---: | ---: | ---: | --- |
| `clone-instant` | `0.669496` | `0.665084` | n/a | n/a | local instant clone is faster than Git in the scoped run |
| `clone` | `1.440026` | `1.417411` | `1.160842` | `1.214765` | warm scoped rerun; normal local clone remains slower than Git and Gitoxide |

The `clone,clone-instant` run had all checks `ok`. Its normal `clone`
stopwatch included a large outlier, but the follow-up warm `clone` rerun still
showed a real local clone gap. Phase traces keep pointing at
`checkout_fresh.checkout_index`, especially packed object read and materialize
time, as the next optimization target. This slice does not close the normal
local clone Git/Gitoxide gate.

Rejected stream-probe threshold experiment: lowering
`STREAM_CHECKOUT_DISABLE_AFTER_MISSES` from `128` to `32` reduced failed stream
probe attempts (`stream_attempts=32`, `stream_written=0`) but did not improve
the clone gate and produced worse/noisier Windows results in
`C:\Users\zmin\zmin-bench-20260617T094316Z-21347-out` (`clone` Zmin/Git
median ratio `2.966388`, Zmin/Gitoxide median ratio `1.575210`). The
threshold was restored to `128`. Do not retry a smaller global stream-probe
miss threshold without fixture-aware evidence, because it can also hurt
repositories where the first streamable blob appears later in checkout order.

Rejected local clone pack-hash shortcut direction: temporary per-object pack
read instrumentation on Windows
`C:\Users\zmin\zmin-bench-20260617T095700Z-36014-out` split packed object
reads into lookup/decode/hash. The instrumentation itself was intentionally
removed because per-object trace-file writes made the stopwatch unusable
(`clone` Zmin `11.046386s`). The diagnostic aggregate still showed the useful
ordering: `packed_read.hash` was only about `0.004999s` across 500 reads, while
`packed_read.decode` was much larger and the checkout wall time remained
dominated by object decode plus materialize. Do not skip packed-object hash
verification as the next clone optimization; it is not the measured bottleneck
and would weaken corrupt-object diagnostics.

Rejected serial-checkout threshold experiment: raising
`PARALLEL_FRESH_CHECKOUT_MIN_ENTRIES` from `256` back to `512` forced the
480-entry Windows clone fixture through the serial fresh-checkout path. It made
the Windows local clone gate worse in
`C:\Users\zmin\zmin-bench-20260617T100843Z-45240-out`: Zmin/Git median
ratio `4.688508`, Zmin/Gitoxide median ratio `3.410763`, with
`checkout_fresh.checkout_index` around `0.403530s` to `0.440677s`. The
threshold was restored to `256`. Do not retry serializing the small Windows
clone fixture as a shortcut; the next useful path remains checkout
object-decode/materialize design.

Follow-up decoded pack-object cache slice: `PackedObjectStore` now keeps a
bounded decoded-object read cache on the store instance (`4096` entries,
`8 MiB` content budget). It is shared through the existing cloned store handle,
does not bypass final object hash verification, and is intended to avoid
re-inflating the same pack object when checkout delta chains share bases. The
cache is bounded and skips objects larger than the byte budget.

Validation:

- `cargo fmt --all -- --check`
- `cargo test -p zmin-git-core packed_store_caches_decoded_objects_for_repeated_reads -- --nocapture`
- `cargo test -p zmin-git-core checkout -- --nocapture`
- `cargo test -p zmin-git-core pack -- --nocapture`
- `cargo test -p zmin-cli --test git_transport_http_compat clone_instant_ -- --nocapture`
- Windows/Git-for-Windows scoped clone cold build run:
  `C:\Users\zmin\zmin-bench-20260617T102336Z-57244-out`
- Windows/Git-for-Windows scoped clone warm run:
  `C:\Users\zmin\zmin-bench-20260617T103220Z-61790-out`
- Windows/Git-for-Windows scoped clone second warm run:
  `C:\Users\zmin\zmin-bench-20260617T103353Z-62930-out`

Key Windows evidence after the decoded-object cache:

| Run | Zmin/Git mean | Zmin/Git median | Zmin/Gitoxide mean | Zmin/Gitoxide median | Note |
| --- | ---: | ---: | ---: | ---: | --- |
| cold build scoped clone | `2.304515` | `3.456782` | `1.919730` | `3.068852` | rejected as cold/noisy for gate; phase trace still showed lower `object_read` |
| warm scoped clone | `0.931212` | `1.030192` | `0.517556` | `0.469266` | faster than Git on mean and Gitoxide on mean/median; Git median near parity |
| second warm scoped clone | `1.016298` | `0.879663` | `0.861868` | `0.988847` | faster than Git on median and faster than Gitoxide on mean/median; Git mean near parity |

The cache reduced warm-run `checkout_index_fresh_into_metadata` `object_read`
phase traces to roughly `0.046755s` to `0.068903s`; the remaining local clone
variance is now mostly materialization and Windows stopwatch noise. This is a
measured move toward the normal local clone gate, but it does not by itself
close the broader clone performance requirement across cold/warm, larger
fixtures, and remote/auth/proxy scenarios.

Rejected delta-only decoded-cache refinement: narrowing the decoded object
cache to recursive delta/base reads only avoided top-level checkout blob
caching, but Windows scoped clone evidence was weaker than the accepted
all-object cache. The cold/warm runs
`C:\Users\zmin\zmin-bench-20260617T104052Z-75573-out` and
`C:\Users\zmin\zmin-bench-20260617T104915Z-82162-out` kept checks `ok`, but
the warm run had Zmin/Git median ratio `1.190899` and Zmin/Gitoxide median
ratio `0.851787`; `object_read` varied around `0.050959s` to `0.091450s`.
The refinement was reverted. Keep the accepted all-object bounded decoded
cache unless a future fixture-aware design proves that top-level cache traffic
is the actual bottleneck.

Follow-up zmin rename and checkout materialization trace slice: after the
repository/package rename, Windows benchmarking now builds `zmin-cli` and runs
`zmin.exe` from `C:\Users\skron\zmin-target`. The Parallels runner deliberately
keeps the existing local infrastructure defaults (`Skron Windows Runner`, guest
user `skron`, and `~/.skron-parallels-cache`) so the rename does not create a
duplicate VM or duplicate llvm-mingw cache. New repository artifacts and output
directories use `zmin-*`; legacy `skron-*` paths remain historical evidence
only.

Fresh-checkout tracing now splits `materialize_write` into
`materialize_file_open`, `materialize_file_bytes`, `materialize_file_close`, and
`materialize_chmod` when `ZMIN_CHECKOUT_PHASE_TRACE` is enabled. The non-trace
runtime path still uses the existing file-write path, so the accepted change is
diagnostic and does not add clean-run overhead.

Validation:

- `bash -n tools/parallels-windows-runner.sh`
- `tools/parallels-windows-runner.sh tools`
- `rustfmt --edition 2024 --check crates/zmin-git-core/src/checkout.rs`
- `cargo test -p zmin-git-core checkout -- --nocapture`
- `cargo test -p zmin-cli --test git_transport_http_compat clone_instant_ -- --nocapture`
- Windows/Git-for-Windows phase-traced scoped clone:
  `ZMIN_WINDOWS_BENCH_PHASE_TRACE=1 tools/parallels-windows-runner.sh benchmark 3 'clone'`
- Windows phase-trace output:
  `C:\Users\skron\zmin-bench-20260617T114114Z-54661-out`
- Windows/Git-for-Windows warm clean scoped clone:
  `tools/parallels-windows-runner.sh benchmark 3 'clone'`
- Windows clean output:
  `C:\Users\skron\zmin-bench-20260617T115125Z-63024-out`

Key Windows trace evidence:

| Repeat | `clone_local` | `object_read` | `materialize` | `file_open` | `file_bytes` | `file_close` | Stream attempts |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | `0.611357s` | `0.228782s` | `0.649531s` | `0.399527s` | `0.184761s` | `0.064843s` | `128` |
| 2 | `0.335094s` | `0.059343s` | `0.333231s` | `0.234562s` | `0.061016s` | `0.037061s` | `128` |
| 3 | `0.263806s` | `0.092301s` | `0.264493s` | `0.199540s` | `0.043486s` | `0.021198s` | `129` |

Key Windows clean-run evidence:

| Operation | Zmin/Git mean | Zmin/Git median | Zmin/Gitoxide mean | Zmin/Gitoxide median | Checks |
| --- | ---: | ---: | ---: | ---: | --- |
| `clone` | `0.692625` | `0.541301` | `0.655580` | `0.650121` | all `ok` |

The clean warm clone run is faster than both Git and Gitoxide on mean and
median for this scoped fixture. The remaining measured local clone target is
checkout materialization variance, especially Windows file open/create cost.
A Windows `FILE_FLAG_SEQUENTIAL_SCAN` fresh-checkout open hint was tested and
removed because clean benchmark evidence did not justify keeping it; do not
retry that hint without stronger clean-run evidence.

Windows push/pull benchmark coverage slice: `tools/windows-native-benchmark.ps1`
now includes scoped local-bare-remote `push-noop`, `push-incremental`,
`push-batch`, `pull-noop`, and `pull-incremental` operations. This closes a
Windows measurement gap; it does not change CLI push or pull behavior. The
push fixtures validate pushed refs and matching preparation trees, while the
pull fixtures validate fast-forwarded `HEAD` against both the Git baseline and
the source repository. Gitoxide rows are intentionally absent for these
operations because this gate does not have a comparable `gix` push/pull CLI
surface.

Validation:

- `bash -n tools/parallels-windows-runner.sh`
- `git diff --check -- tools/windows-native-benchmark.ps1 tools/parallels-windows-runner.sh`
- Windows/Git-for-Windows smoke:
  `tools/parallels-windows-runner.sh benchmark 1 'push-noop,push-incremental,push-batch,pull-noop,pull-incremental'`
- Windows smoke output:
  `C:\Users\skron\zmin-bench-20260617T115732Z-77294-out`
- Windows/Git-for-Windows 3-repeat gate:
  `tools/parallels-windows-runner.sh benchmark 3 'push-noop,push-incremental,push-batch,pull-noop,pull-incremental'`
- Windows 3-repeat output:
  `C:\Users\skron\zmin-bench-20260617T115943Z-78907-out`
- Windows/Git-for-Windows pull-incremental clean rerun:
  `tools/parallels-windows-runner.sh benchmark 3 'pull-incremental'`
- Windows pull-incremental rerun output:
  `C:\Users\skron\zmin-bench-20260617T120402Z-82024-out`

Key Windows 3-repeat evidence:

| Operation | Zmin/Git mean | Zmin/Git median | Checks | Note |
| --- | ---: | ---: | --- | --- |
| `push-noop` | `0.172105` | `0.184620` | `ok` | faster than Git |
| `push-incremental` | `0.441063` | `0.391675` | `ok` | faster than Git |
| `push-batch` | `0.468073` | `0.536127` | `ok` | 2,400-file fixture, faster than Git |
| `pull-noop` | `0.251267` | `0.308744` | `ok` | faster than Git |
| `pull-incremental` | `0.916210` | `1.174448` | `ok` | mixed/noisy in this combined run |

The combined push/pull run had all checks `ok`, but its `pull-incremental`
median had one Zmin outlier. A clean scoped rerun of only `pull-incremental`
produced mean ratio `0.722868` and median ratio `0.552164`, with all
fast-forward checks `ok`, so the combined-run median gap is treated as Windows
benchmark variance rather than accepted regression evidence. Broader push/pull
work still needs larger repositories and non-local transport/auth/proxy gates.

macOS pull benchmark and fast-forward metadata-preservation slice:
`tools/git-performance-bench.sh` now includes `pull-noop` and
`pull-incremental` local-bare-remote operations alongside the existing push and
fetch rows. The first production-size macOS 3-repeat run exposed
`pull-incremental` as a real local gap: Zmin/Git mean ratio `1.440000` and
median ratio `1.117647` in `/tmp/zmin-macos-pull-bench-20260617.tsv`, while
all pull checks were `ok`.

Trace on the same shape of 90-commit / 25-files-per-commit fixture showed the
gap in fast-forward checkout/index metadata work, not in config resolution:
before the fix `fast_forward.checkout` was about `0.071180s` and `pull.total`
about `0.188851s`. Fast-forward now uses a checkout transition variant only
after the existing explicit `worktree_clean` check: unchanged stage-0 index
entries keep their old stat metadata, and only changed/new paths are rehashed
for refreshed metadata. The general checkout transition keeps the previous
full-refresh behavior.

Validation:

- `rustfmt --edition 2024 --check crates/zmin-cli/src/runtime/worktree_index.rs crates/zmin-cli/src/runtime/merge_worktree.rs crates/zmin-cli/src/cli/commands/transport_impl.rs`
- `cargo test -p zmin-cli --test git_clone_compat clone_instant_local_repo_fetch_and_pull_remain_canonical_git_operations -- --nocapture`
- `git diff --check -- crates/zmin-cli/src/runtime/worktree_index.rs crates/zmin-cli/src/runtime/merge_worktree.rs crates/zmin-cli/src/cli/commands/transport_impl.rs tools/git-performance-bench.sh`
- macOS small smoke:
  `/tmp/zmin-macos-pull-bench-smoke-20260617.tsv`
- macOS pre-fix production-size gate:
  `/tmp/zmin-macos-pull-bench-20260617.tsv`
- macOS post-fix production-size gate:
  `/tmp/zmin-macos-pull-fast-forward-bench-20260617.tsv`
- Windows/Git-for-Windows post-fix scoped gate:
  `tools/parallels-windows-runner.sh benchmark 3 'pull-incremental'`
- Windows post-fix output:
  `C:\Users\skron\zmin-bench-20260617T122819Z-42889-out`

Post-fix trace on the same fixture dropped `fast_forward.checkout` to about
`0.006364s` and `pull.total` to about `0.130996s`.

Key post-fix evidence:

| Platform | Operation | Zmin/Git mean | Zmin/Git median | Checks |
| --- | --- | ---: | ---: | --- |
| macOS | `pull-noop` | `0.296296` | `0.333333` | `ok` |
| macOS | `pull-incremental` | `0.816327` | `0.812500` | `ok` |
| Windows/Git-for-Windows | `pull-incremental` | `0.616095` | `0.513132` | `ok` |

This closes the local-bare `pull-noop` and `pull-incremental` benchmark gap for
the current macOS and Windows fixtures. Larger repositories and non-local
transport/auth/proxy pull gates remain open.

macOS scoped benchmark selection slice: `tools/git-performance-bench.sh` now
accepts `ZMIN_BENCH_OPS` as a comma, space, or semicolon separated operation
allowlist. Default behavior remains the full benchmark. Unknown operation names
fail before fixture setup, and selected runs still perform the setup and
validation needed by dependent rows, for example untimed pack generation before
`index-pack` or untimed commit setup before `commit-dirty`.

Useful examples:

- `ZMIN_BENCH_REPEATS=3 ZMIN_BENCH_OPS='clone,clone-instant,clone-instant-git-daemon,clone-instant-ssh' tools/git-performance-bench.sh`
- `ZMIN_BENCH_REPEATS=3 ZMIN_BENCH_OPS='push-noop,push-incremental,push-batch,pull-noop,pull-incremental' tools/git-performance-bench.sh`
- `ZMIN_BENCH_REPEATS=3 ZMIN_BENCH_OPS='fetch-noop fetch-incremental fetch-batch' tools/git-performance-bench.sh`

Validation:

- `bash -n tools/git-performance-bench.sh`
- `git diff --check -- tools/git-performance-bench.sh`
- unknown-op parser probe:
  `ZMIN_BENCH_OPS='no-such-op' ZMIN_BENCH_REPEATS=1 tools/git-performance-bench.sh`
- macOS scoped smoke:
  `ZMIN_BENCH_REPEATS=1 ZMIN_BENCH_COMMITS=6 ZMIN_BENCH_FILES_PER_COMMIT=3 ZMIN_BENCH_OPS='clone-instant, pull-noop' tools/git-performance-bench.sh`
- macOS remote/push scoped smoke:
  `ZMIN_BENCH_REPEATS=1 ZMIN_BENCH_COMMITS=6 ZMIN_BENCH_FILES_PER_COMMIT=3 ZMIN_BENCH_PUSH_BATCH_FILES=5 ZMIN_BENCH_OPS='clone-instant-git-daemon clone-instant-ssh;push-incremental' tools/git-performance-bench.sh`

The first scoped smoke emitted only `clone-instant` and `pull-noop` rows, with
checks for matching `HEAD`, tree, the `zmin.worktreeFirst=true` marker, and
`pull-noop`. The second scoped smoke emitted only `clone-instant-git-daemon`,
`clone-instant-ssh`, and `push-incremental`, with the expected remote instant
clone checks and pushed-ref validation.

macOS scoped phase-trace follow-up: `tools/git-performance-bench.sh` now accepts
`ZMIN_BENCH_PHASE_TRACE_DIR`. When set, only measured `tool=zmin` rows run with
`ZMIN_PHASE_TRACE=1`, `ZMIN_CHECKOUT_PHASE_TRACE=1`, and a per-command
`ZMIN_PHASE_TRACE_FILE` under that directory. Git and Gitoxide rows keep the
previous timing path. This makes the macOS scoped loop usable for checkout,
pull, git-daemon, and SSH phase analysis without manually reconstructing each
benchmark command.

Validation:

- `bash -n tools/git-performance-bench.sh`
- `git diff --check -- tools/git-performance-bench.sh`
- macOS scoped trace smoke:
  `ZMIN_BENCH_REPEATS=1 ZMIN_BENCH_COMMITS=6 ZMIN_BENCH_FILES_PER_COMMIT=3 ZMIN_BENCH_OPS='clone-instant, pull-noop' ZMIN_BENCH_PHASE_TRACE_DIR=/tmp/zmin-macos-trace-smoke tools/git-performance-bench.sh`
- macOS remote-instant trace smoke:
  `ZMIN_BENCH_REPEATS=1 ZMIN_BENCH_COMMITS=6 ZMIN_BENCH_FILES_PER_COMMIT=3 ZMIN_BENCH_OPS='clone-instant-git-daemon clone-instant-ssh' ZMIN_BENCH_PHASE_TRACE_DIR=/tmp/zmin-macos-remote-trace-smoke tools/git-performance-bench.sh`
- tiny full-default regression smoke without trace-dir:
  `ZMIN_BENCH_REPEATS=1 ZMIN_BENCH_COMMITS=6 ZMIN_BENCH_FILES_PER_COMMIT=3 ZMIN_BENCH_WRITE_FILES=5 ZMIN_BENCH_DIRTY_FILES=2 ZMIN_BENCH_FETCH_BATCH_FILES=5 ZMIN_BENCH_PUSH_BATCH_FILES=5 tools/git-performance-bench.sh`

The first trace smoke wrote `clone-instant` and `pull-noop` trace files with
`checkout_fresh.*`, `clone_local.*`, `fast_forward.*`, and `pull.*` labels. The
remote-instant smoke wrote separate git-daemon and SSH trace files with
`clone_git_daemon.*`, `clone_ssh.*`, `daemon_fetch_pack.*`, `ssh_fetch_pack.*`,
and `checkout_fresh.*` labels. The tiny full-default run still emitted all 23
benchmark operation groups and had no failed checks.

macOS benchmark output follow-up: `tools/git-performance-bench.sh` now accepts
`ZMIN_BENCH_OUT_DIR`. When set, the script preserves the existing stdout stream
and additionally writes:

- `bench.tsv` with the raw measured rows;
- `checks.tsv` with validation rows;
- `summary.csv` with per-operation/tool run count, mean, median, min, and max
  wall seconds;
- `comparison.csv` with Git-vs-Zmin mean/median ratios and Gitoxide ratios when
  a comparable `gix` row exists.

This gives the macOS benchmark loop the same ratio-first evidence shape used by
the Windows native benchmark.

Validation:

- `bash -n tools/git-performance-bench.sh`
- `git diff --check -- tools/git-performance-bench.sh`
- scoped output smoke:
  `ZMIN_BENCH_REPEATS=1 ZMIN_BENCH_COMMITS=6 ZMIN_BENCH_FILES_PER_COMMIT=3 ZMIN_BENCH_OPS='clone-instant,pull-noop' ZMIN_BENCH_OUT_DIR=/tmp/zmin-macos-out-smoke ZMIN_BENCH_PHASE_TRACE_DIR=/tmp/zmin-macos-out-traces tools/git-performance-bench.sh`
- tiny full-default output smoke:
  `ZMIN_BENCH_REPEATS=1 ZMIN_BENCH_COMMITS=6 ZMIN_BENCH_FILES_PER_COMMIT=3 ZMIN_BENCH_WRITE_FILES=5 ZMIN_BENCH_DIRTY_FILES=2 ZMIN_BENCH_FETCH_BATCH_FILES=5 ZMIN_BENCH_PUSH_BATCH_FILES=5 ZMIN_BENCH_OUT_DIR=/tmp/zmin-full-out-smoke tools/git-performance-bench.sh`

The scoped smoke wrote all four output files and only `clone-instant` /
`pull-noop` comparison rows. The tiny full-default output smoke wrote all four
output files, produced 23 comparison operation groups, and had no failed checks.

macOS benchmark precision and stale-binary guard follow-up:
`tools/git-performance-bench.sh` now records `real` wall time with nanosecond
timestamps around each measured command instead of using the rounded
`/usr/bin/time -lp` `real` field. It still uses `/usr/bin/time -lp` for
`user`, `sys`, and `rss`. This matters for the subsecond clone/push/pull rows,
where centisecond rounding can move ratios by double-digit percentages.

The same slice now builds the default release binary
(`cargo build --manifest-path Cargo.toml --release -p zmin-cli --bin zmin`)
before timed rows when `ZMIN_BIN` is not explicitly set. This matches the
Windows native benchmark stale-binary invariant. If `ZMIN_BIN` is explicitly
set, the caller-owned binary is used and must already be executable.

Local push also has phase labels for evidence-led follow-up:
`push.total`, `push.find_repo`, `push.resolve_remote`, `push.remote_url`,
`push.local.open_destination`, `push.local.setup_stores`,
`push.local.collect_destination_roots`, `push.local.default_refspec`, `push.local.parse_refspec`,
`push.local.destination_has_object`, `push.local.copy_reachable_objects`,
`push.local.validate_update`, `push.local.update_ref`,
`push.local.set_upstream`, and `push.local.render`. The shared phase-trace
helper no longer creates an `Instant` when `ZMIN_PHASE_TRACE` is disabled, so
disabled instrumentation is cheaper across existing phase-traced commands.

Validation:

- `bash -n tools/git-performance-bench.sh`
- `git diff --check -- tools/git-performance-bench.sh crates/zmin-cli/src/runtime/phase_trace.rs crates/zmin-cli/src/cli/commands/transport_impl.rs`
- `rustfmt --edition 2024 --check crates/zmin-cli/src/runtime/phase_trace.rs crates/zmin-cli/src/cli/commands/transport_impl.rs`
- `cargo build --manifest-path Cargo.toml --release -p zmin-cli --bin zmin`
- `cargo test -p zmin-cli --test git_transport_local_compat push_ -- --nocapture`
  (`4/4` passing)
- precise scoped smoke:
  `ZMIN_BENCH_REPEATS=1 ZMIN_BENCH_COMMITS=6 ZMIN_BENCH_FILES_PER_COMMIT=3 ZMIN_BENCH_OPS='clone-instant,pull-noop' ZMIN_BENCH_OUT_DIR=/tmp/zmin-precision-smoke tools/git-performance-bench.sh`
- fair macOS scoped gate without phase tracing:
  `/tmp/zmin-macos-precision-fair-gate-20260617T135538Z`
- push-incremental 10-repeat fair rerun after disabled-trace optimization:
  `/tmp/zmin-macos-push-incremental-fair-after-tracefix-20260617T140749Z`
- broader fair macOS scoped gate after the trace fix:
  `/tmp/zmin-macos-scoped-fair-after-tracefix-20260617T140812Z`
- Windows/Git-for-Windows scoped fair push/pull gate after the same runtime
  changes:
  `C:\Users\skron\zmin-bench-20260617T141309Z-49421-out`
- Windows/Git-for-Windows scoped traced push/pull run for phase evidence only:
  `C:\Users\skron\zmin-bench-20260617T142441Z-59901-out`

Important evidence rule: use `ZMIN_BENCH_PHASE_TRACE_DIR` runs for phase
analysis, not as fair ratio gates, because traced `tool=zmin` rows do extra
trace-file work while Git/Gitoxide rows do not.

Key fair evidence after the precision/build/trace changes:

| Run | Operation | Zmin/Git mean | Zmin/Git median | Note |
| --- | --- | ---: | ---: | --- |
| `/tmp/zmin-macos-precision-fair-gate-20260617T135538Z` | `clone` | `0.950869` | `0.933419` | faster than Git; also faster than Gitoxide |
| same | `clone-instant` | `0.771035` | `0.799523` | faster than Git |
| same | `clone-instant-git-daemon` | `0.857781` | `0.869233` | faster than Git |
| same | `clone-instant-ssh` | `0.901152` | `1.281012` | mean faster, median noisy because Git had one very fast run |
| same | `pull-incremental` | `0.861021` | `0.960489` | faster/near parity |
| same | `pull-noop` | `0.389841` | `0.404467` | faster than Git |
| same | `push-batch` | `0.769367` | `0.441626` | faster than Git |
| `/tmp/zmin-macos-push-incremental-fair-after-tracefix-20260617T140749Z` | `push-incremental` | `0.995599` | `1.022536` | mean parity/faster; remaining median gap about `2.3%` on a short row |
| `/tmp/zmin-macos-scoped-fair-after-tracefix-20260617T140812Z` | `push-incremental` | `1.039765` | `1.044717` | small noisy gap remains in the broader run |

Key Windows fair evidence from
`C:\Users\skron\zmin-bench-20260617T141309Z-49421-out`:

| Operation | Zmin/Git mean | Zmin/Git median | Checks | Note |
| --- | ---: | ---: | --- | --- |
| `pull-incremental` | `0.872795` | `0.744628` | `ok` | faster than Git |
| `pull-noop` | `0.214254` | `0.169273` | `ok` | faster than Git |
| `push-batch` | `0.320459` | `0.259727` | `ok` | faster than Git |
| `push-incremental` | `0.469977` | `0.460995` | `ok` | faster than Git |
| `push-noop` | `0.177780` | `0.200561` | `ok` | faster than Git |

Push-incremental trace evidence with tracing enabled at
`/tmp/zmin-push-trace-fresh` showed the local push time dominated by
`push.local.copy_reachable_objects` (`~0.058-0.067s`). Windows traced evidence
at `C:\Users\skron\zmin-bench-20260617T142441Z-59901-out` showed the same local
push shape: `push.local.copy_reachable_objects` took `0.187395s` of
`push.total=0.201475s` for the one-repeat traced `push-incremental` row. The
same traced Windows run kept `pull-incremental` checks green and showed
`fast_forward.worktree_clean=1.268198s`, `fast_forward.checkout=0.017478s`, and
`fast_forward.update_refs=0.287755s` inside `pull.total=1.817730s`. Keep future
push work focused on reachable-object copy, and treat pull follow-up as
worktree-clean/ref-update investigation rather than checkout metadata work.

Local push negotiation follow-up:
local push now reuses the remote-push pack id collection path with destination
refs as excluded roots before copying objects into the local bare remote. This
keeps the ref update behavior unchanged, but avoids collecting the whole source
history and then probing the destination for every reachable object during an
incremental push.

Validation:

- `rustfmt --edition 2024 crates/zmin-cli/src/cli/commands/transport_impl.rs`
- `cargo test -p zmin-cli transport_commands::transport_request_tests::collect_push_pack_ids_excludes_objects_reachable_from_remote_roots -- --nocapture`
  (passes in both zmin-cli binary test targets)
- `cargo test -p zmin-cli --test git_transport_local_compat push_ -- --nocapture`
  (`4/4` passing)
- macOS fair scoped run:
  `/tmp/zmin-macos-push-negotiation-20260617T143022Z`
- macOS traced phase run:
  `/tmp/zmin-macos-push-negotiation-trace-20260617T143419Z`
- Windows/Git-for-Windows fair scoped push run:
  `C:\Users\skron\zmin-bench-20260617T143438Z-72736-out`
- Windows/Git-for-Windows traced `push-incremental` run:
  `C:\Users\skron\zmin-bench-20260617T144618Z-81442-out`

Key post-optimization fair evidence:

| Platform | Operation | Zmin/Git mean | Zmin/Git median | Checks | Note |
| --- | --- | ---: | ---: | --- | --- |
| macOS | `push-incremental` | `0.364313` | `0.324592` | `ok` | fixed the prior small median gap |
| Windows | `push-noop` | `0.174491` | `0.216372` | `ok` | no regression |
| Windows | `push-incremental` | `0.073975` | `0.063631` | `ok` | much faster than Git |
| Windows | `push-batch` | `0.034839` | `0.036812` | `ok` | much faster than Git |

Phase evidence moved in the expected direction. On macOS,
`push.local.copy_reachable_objects` dropped to `0.012487s` in the traced
`push-incremental` row. On Windows, the same phase dropped from the previous
`0.187395s` traced row to `0.000485s`, with `push.total=0.009966s`.

Post-pushfix macOS broad gate:

- broad scoped fair run:
  `/tmp/zmin-macos-post-pushfix-broad-20260617T145513Z`
- focused `push-batch` rerun:
  `/tmp/zmin-macos-push-batch-rerun-20260617T150004Z`
- focused `pull-incremental` rerun:
  `/tmp/zmin-macos-pull-incremental-rerun-20260617T150100Z`
- focused `clone-instant-ssh` rerun:
  `/tmp/zmin-macos-ssh-instant-rerun-20260617T150126Z`
- focused `clone-instant-ssh` traced run:
  `/tmp/zmin-macos-ssh-instant-trace-20260617T150149Z`

The broad run had all checks ok. Focused reruns showed the broad `push-batch`
and `pull-incremental` slow rows were noise: `push-batch` was near parity/faster
(`0.886213` mean / `0.982051` median), and `pull-incremental` was faster
(`0.858338` / `0.824430`). The stable macOS gap is now `clone-instant-ssh`:
focused 5-repeat evidence measured `1.418090` mean and `1.351125` median
Zmin/Git ratios with all clone checks ok.

The traced SSH instant row is phase evidence only. It showed
`clone_ssh.discovery=0.081614s`, `clone_ssh.fetch_objects=0.088845s`,
`clone_ssh.write_refs_config=0.037570s`, and `clone_ssh.checkout=0.062985s`.
A trial that increased the SSH stdout `BufReader` capacity to `64 KiB` was
rejected and reverted: correctness tests stayed green, but the focused fair
run `/tmp/zmin-macos-ssh-buffer-rerun-20260617T150340Z` did not improve the
median (`1.363938`) and had a worse mean (`2.132687`). Do not retry plain SSH
reader-buffer tuning without stronger evidence; next SSH work should target
process/discovery variance, ref/config writes, or checkout phases.

Remote instant clone config batching follow-up:
smart HTTP, git-daemon, and SSH worktree-first clone paths now share a helper
that builds the remote clone config key/value list and writes it with
`set_config_values`. This keeps the same config surface (`remote.<name>.url`,
`remote.<name>.fetch`, `remote.<name>.mirror`, `remote.<name>.tagOpt`,
`zmin.worktreeFirst`, and the opt-in demand-hydrate keys when enabled), but
avoids repeated config file rewrites. Branch config setup in those remote clone
paths also uses batched config writes.

Validation:

- `rustfmt --edition 2024 crates/zmin-cli/src/cli/commands/transport_impl.rs`
- `cargo test -p zmin-cli --test git_transport_http_compat clone_instant_ -- --nocapture`
  (`9/9` passing)
- macOS focused fair `clone-instant-ssh` rerun:
  `/tmp/zmin-macos-ssh-config-batch-rerun-20260617T151153Z`
- macOS focused traced `clone-instant-ssh` run:
  `/tmp/zmin-macos-ssh-config-batch-trace-20260617T151505Z`
- Windows/Git-for-Windows scoped remote instant run:
  `C:\Users\skron\zmin-bench-20260617T151539Z-21260-out`
- Windows/Git-for-Windows focused `clone-instant-git-daemon` rerun:
  `C:\Users\skron\zmin-bench-20260617T152257Z-26609-out`

Trace evidence confirms the intended phase win: macOS
`clone_ssh.write_refs_config` dropped from `0.037570s` to `0.011346s`.
Fair macOS ratio evidence improved only slightly and remains slower than Git
for the SSH loopback fixture (`1.455226` mean / `1.338546` median after the
batching change, compared with the previous `1.418090` / `1.351125`). Treat
the remaining macOS SSH gap as discovery/fetch/checkout/process variance rather
than config-write overhead. Windows remote instant checks stayed green; the
combined run had SSH faster than Git (`0.628981` mean / `0.733210` median) and
a noisy git-daemon outlier, while the focused git-daemon rerun cleared that
noise (`0.431233` mean / `0.421193` median).

Remote instant branch config batching follow-up:
branch tracking config (`branch.<name>.remote` and `branch.<name>.merge`) is now
included in the same `clone_remote_config_values` batch for non-bare branch
targets. This removes the separate branch-config rewrite from the smart HTTP,
git-daemon, and SSH clone paths while preserving the required initial
`refs/remotes/origin/<branch>` state for instant clones. The local clone path was
left unchanged in this slice because the current evidence target is remote
instant SSH variance.

Validation:

- `rustfmt --edition 2024 crates/zmin-cli/src/cli/commands/transport_impl.rs`
- `cargo test -p zmin-cli transport_commands::transport_request_tests::clone_remote_config_values_batches_branch_tracking_config -- --nocapture`
  (passes in both zmin-cli binary test targets)
- `cargo test -p zmin-cli --test git_transport_http_compat clone_instant_ -- --nocapture`
  (`9/9` passing, before the added helper unit test)
- macOS fair focused `clone-instant-ssh` rerun:
  `/tmp/zmin-macos-ssh-branch-config-batch-rerun-20260617T152944Z`
- macOS traced focused `clone-instant-ssh` run:
  `/tmp/zmin-macos-ssh-branch-config-batch-trace-20260617T153255Z`
- Windows/Git-for-Windows scoped remote instant run:
  `C:\Users\skron\zmin-bench-20260617T153313Z-40255-out`
- Windows/Git-for-Windows focused `clone-instant-git-daemon` rerun:
  `C:\Users\skron\zmin-bench-20260617T153851Z-41245-out`
- Windows/Git-for-Windows focused `clone-instant-ssh` rerun:
  `C:\Users\skron\zmin-bench-20260617T153953Z-41605-out`

Key fair evidence:

| Platform | Operation | Zmin/Git mean | Zmin/Git median | Checks | Note |
| --- | --- | ---: | ---: | --- | --- |
| macOS | `clone-instant-ssh` | `1.433677` | `1.300083` | `ok` | small median improvement, still slower than Git |
| Windows focused | `clone-instant-git-daemon` | `0.781916` | `0.752020` | `ok` | focused rerun cleared combined outlier |
| Windows focused | `clone-instant-ssh` | `0.396248` | `0.320241` | `ok` | faster than Git |

The traced macOS row showed `clone_ssh.write_refs_config=0.012123s`, with the
remaining time mostly in `clone_ssh.fetch_objects=0.139572s` and
`clone_ssh.checkout=0.103088s` for that noisy trace. This confirms the branch
config rewrite is no longer the main SSH instant clone bottleneck. Next macOS
SSH work should target fetch sideband/index-pack variance or checkout
materialization, not additional config batching.

Remote upload-pack sideband/no-progress follow-up:
the sideband pack parser now emits `upload_pack.sideband` phase telemetry with
pack/progress/error packet counts, payload byte counts, and split read/write
timings. The client upload-pack request now asks for `no-progress` alongside
`side-band-64k`, `thin-pack`, `ofs-delta`, and `include-tag`, matching the
intent of instant clone paths that discard progress channel data.

Validation:

- `cargo test -p zmin-cli sideband_pack_stream_uses_caller_buffer -- --nocapture`
  (passes in both zmin-cli binary test targets)
- `cargo test -p zmin-cli transport_commands::transport_request_tests::upload_pack_request_matches_git_pkt_line_shape -- --nocapture`
  (passes in both zmin-cli binary test targets)
- `cargo test -p zmin-cli --test git_transport_http_compat http_backend_upload_pack_filter_blob_none_omits_blob_objects -- --nocapture`
- `cargo test -p zmin-cli --test git_transport_http_compat clone_instant_ -- --nocapture`
  (`9/9` passing)
- macOS traced focused `clone-instant-ssh` run:
  `/tmp/zmin-macos-ssh-no-progress-trace-20260617T155632Z`
- macOS fair focused `clone-instant-ssh` rerun:
  `/tmp/zmin-macos-ssh-no-progress-rerun-20260617T160009Z`
- Windows/Git-for-Windows scoped remote instant run:
  `C:\Users\skron\zmin-bench-20260617T160031Z-61056-out`
- Windows/Git-for-Windows focused `clone-instant-git-daemon` rerun:
  `C:\Users\skron\zmin-bench-20260617T160703Z-63200-out`

Key evidence:

| Platform | Operation | Zmin/Git mean | Zmin/Git median | Checks | Note |
| --- | --- | ---: | ---: | --- | --- |
| macOS trace | `clone-instant-ssh` | `1.639782` | `1.639782` | `ok` | phase evidence only; tracing adds Zmin-only overhead |
| macOS fair | `clone-instant-ssh` | `1.545057` | `1.233402` | `ok` | better than the prior `1.300083` median, still slower than Git |
| Windows combined | `clone-instant-ssh` | `0.460539` | n/a | `ok` | faster than Git; stdout table did not expose median ratio |
| Windows focused | `clone-instant-git-daemon` | `0.773875` | n/a | `ok` | focused rerun cleared the combined git-daemon outlier |

The macOS trace proves the protocol change is effective:
`progress_packets=0` and `progress_bytes=0`, compared with the previous traced
rows that had more than 160 progress packets. The same row still spent
`0.053524s` in frame reads while pack payload read/write stayed tiny
(`read_seconds=0.000025`, `write_seconds=0.000122`), so `no-progress` is
accepted as a correctness-preserving protocol cleanup and telemetry improvement,
not as full closure of the macOS SSH instant clone gap. Remaining SSH work
should investigate discovery/process startup variance, remote pack delivery,
and checkout materialization.

SSH discovery split and protocol-v2 rejection follow-up:
`ssh_open_advertised_upload_pack` now splits SSH instant clone discovery into
`ssh_upload_pack.open.spawn` and `ssh_upload_pack.open.advertisement`, with the
advertisement row count and HEAD symref presence attached to the trace. This
keeps the protocol-v0 behavior, but makes the previous `clone_ssh.discovery`
variance actionable.

Validation:

- `cargo test -p zmin-cli --test git_transport_http_compat clone_instant_ -- --nocapture`
  (`9/9` passing)
- macOS final traced focused `clone-instant-ssh` run:
  `/tmp/zmin-macos-ssh-open-split-final-trace-20260617T162756Z`
- rejected macOS protocol-v2 traced run:
  `/tmp/zmin-macos-ssh-v2-trace-20260617T162134Z`
- rejected macOS protocol-v2 fair run:
  `/tmp/zmin-macos-ssh-v2-fair-20260617T162428Z`
- Windows/Git-for-Windows focused `clone-instant-ssh` run:
  `C:\Users\skron\zmin-bench-20260617T163041Z-81371-out`
- Windows/Git-for-Windows focused `clone-instant-ssh` rerun:
  `C:\Users\skron\zmin-bench-20260617T163538Z-82015-out`

Key evidence:

| Platform | Operation | Zmin/Git mean | Zmin/Git median | Checks | Note |
| --- | --- | ---: | ---: | --- | --- |
| macOS final trace | `clone-instant-ssh` | `1.990654` | `1.990654` | `ok` | phase evidence only; `ssh_upload_pack.open.spawn=0.003789s`, `ssh_upload_pack.open.advertisement=0.082532s` |
| macOS rejected v2 fair | `clone-instant-ssh` | `1.388523` | `1.344309` | `ok` | protocol v2 made the fair median worse than the accepted no-progress run |
| Windows first focused | `clone-instant-ssh` | `1.086886` | n/a | `ok` | noisy Zmin outlier at `1.648020s` |
| Windows focused rerun | `clone-instant-ssh` | `0.676947` | n/a | `ok` | warm rerun cleared the outlier and stayed faster than Git |

A minimal SSH protocol-v2 instant-clone experiment was tested and removed. Git
does use `GIT_PROTOCOL=version=2` in this fake-SSH benchmark, but the Zmin v2
experiment did not reduce the measured discovery wait: the traced v2 row still
had `ssh_upload_pack.open.advertisement=0.324048s`, and the fair median ratio
worsened to `1.344309`. Do not retry a narrow SSH v2 conversion as a clone
speed fix without new evidence; a real v2 transport implementation should be
justified as a broader compatibility feature, not as this macOS loopback
performance fix. The retained split telemetry shows current discovery variance
is dominated by waiting for upload-pack advertisement, not local SSH process
spawn (`0.003789s` in the final trace).

SSH refs/config split and packed-refs rejection follow-up:
the retained code now traces the SSH instant clone ref/config phase as
`clone_ssh.write_refs_config.write_refs`, optional
`clone_ssh.write_refs_config.prune_missing_tag_refs`, and
`clone_ssh.write_refs_config.set_config`. This is telemetry only; clone still
writes the initial loose refs through the normal ref store path.

Validation:

- `cargo test -p zmin-cli --test git_transport_http_compat clone_instant_ssh_ -- --nocapture`
  (`3/3` passing)
- `cargo test -p zmin-cli --test git_transport_http_compat clone_instant_ -- --nocapture`
  (`9/9` passing after reverting the packed-refs experiment)
- macOS traced focused `clone-instant-ssh` run with retained split telemetry:
  `/tmp/zmin-macos-ssh-ref-config-split-trace-20260617T164019Z`
- rejected macOS packed-refs traced run:
  `/tmp/zmin-macos-ssh-packed-refs-trace-20260617T164447Z`
- rejected macOS packed-refs fair run:
  `/tmp/zmin-macos-ssh-packed-refs-fair-20260617T164748Z`
- rejected Windows/Git-for-Windows packed-refs scoped remote run:
  `C:\Users\skron\zmin-bench-20260617T164808Z-93801-out`
- final retained-code macOS fair focused `clone-instant-ssh` run:
  `/tmp/zmin-macos-ssh-ref-config-split-final-fair-20260617T165335Z`
- final retained-code Windows/Git-for-Windows warm focused
  `clone-instant-ssh` rerun:
  `C:\Users\skron\zmin-bench-20260617T170127Z-97514-out`

Key evidence:

| Platform | Operation | Zmin/Git mean | Zmin/Git median | Checks | Note |
| --- | --- | ---: | ---: | --- | --- |
| macOS retained trace | `clone-instant-ssh` | n/a | n/a | `ok` | `write_refs=0.037509s`, `set_config=0.001953s`, total `0.044511s` |
| macOS rejected packed-refs trace | `clone-instant-ssh` | n/a | n/a | `ok` | targeted phase improved: `write_refs=0.018915s`, total `0.025391s` |
| macOS rejected packed-refs fair | `clone-instant-ssh` | `1.426575` | `1.345815` | `ok` | worse than the retained no-progress/split direction |
| Windows rejected packed-refs combined | `clone-instant-git-daemon` | `1.805112` | n/a | `ok` | git-daemon outlier made the change unsuitable despite SSH improving |
| Windows rejected packed-refs combined | `clone-instant-ssh` | `0.895764` | n/a | `ok` | faster than Git, but not enough to accept the layout change |
| macOS retained fair | `clone-instant-ssh` | `1.844999` | `1.276756` | `ok` | one cold Zmin outlier; warm Zmin rows were around `0.20s`, still slower than Git |
| Windows retained warm rerun | `clone-instant-ssh` | `0.822471` | n/a | `ok` | raw medians: Git `0.866106s`, Zmin `0.754173s` |

The packed-refs experiment proved that the loose-ref write subphase can be made
smaller in a trace, but the fair macOS and combined Windows gates did not
justify changing the initial ref layout for this performance loop. Keep the
split telemetry; do not retry packed refs as the loopback SSH clone fix without
new evidence. The next useful SSH instant clone work should target advertisement
wait variance and checkout materialization, or a broader transport design, not
another narrow packed-refs or protocol-v2 trial.

Duplicate clone HEAD write cleanup and top-level remote clone trace follow-up:
successful branch clone paths no longer rewrite `HEAD` after
`init_repository`, because repository initialization already writes
`HEAD -> refs/heads/<initial_branch>` for both bare and worktree clones. The
local branch ref, remote refs, branch config, detached/tag `HEAD` handling, and
missing-branch error path are unchanged. Git-daemon and SSH clone paths now also
emit top-level `clone_git_daemon` and `clone_ssh` phase labels, matching the
existing `clone_local` and `clone_http` labels.

Validation:

- `rustfmt --edition 2024 --check crates/zmin-cli/src/cli/commands/transport_impl.rs`
- `cargo test -p zmin-cli --test git_transport_http_compat clone_instant_ -- --nocapture`
  (`9/9` passing; an earlier full filtered run had one background-fetch timing
  miss that passed on immediate focused rerun)
- `cargo test -p zmin-cli --test git_clone_compat clone_ -- --nocapture`
  (`10/10` passing)
- `cargo test -p zmin-cli --test git_transport_local_compat clone_ -- --nocapture`
  (`2/2` passing)
- macOS traced focused `clone-instant-ssh` run:
  `/tmp/zmin-macos-ssh-top-trace-20260617T171326Z`
- macOS fair focused `clone-instant-ssh` run:
  `/tmp/zmin-macos-ssh-skip-head-final-fair-20260617T172235Z`
- macOS fair focused `clone-instant-git-daemon` smoke:
  `/tmp/zmin-macos-git-daemon-skip-head-fair-20260617T172255Z`
- Windows/Git-for-Windows first focused `clone-instant-ssh` run:
  `C:\Users\skron\zmin-bench-20260617T171633Z-17719-out`
- Windows/Git-for-Windows warm focused `clone-instant-ssh` rerun:
  `C:\Users\skron\zmin-bench-20260617T172124Z-19205-out`

Key evidence:

| Platform | Operation | Zmin/Git mean | Zmin/Git median | Checks | Note |
| --- | --- | ---: | ---: | --- | --- |
| macOS trace | `clone-instant-ssh` | `1.949392` | `1.949392` | `ok` | phase evidence only; `write_refs_config.write_refs=0.022228s`, `clone_ssh=0.305454s`, stopwatch `0.759068s` |
| macOS fair | `clone-instant-ssh` | `1.189884` | `1.266655` | `ok` | still slower than Git on median; small/noisy improvement vs prior retained-code median |
| macOS fair | `clone-instant-git-daemon` | `1.830022` | `0.894109` | `ok` | one cold Zmin outlier; median faster than Git |
| Windows first focused | `clone-instant-ssh` | `1.088539` | n/a | `ok` | one Zmin outlier (`1.524251s`) made the run slower |
| Windows warm focused | `clone-instant-ssh` | `0.738614` | `0.708343` | `ok` | warm rerun faster than Git; raw medians Git `1.192680s`, Zmin `0.844827s` |

The duplicate-HEAD cleanup is accepted as a low-risk ref-write cleanup with a
measured trace reduction in the SSH ref subphase, not as closure of the macOS
SSH instant clone gap. The top-level trace also exposed a tracing caveat:
`clone_ssh=0.305454s` did not cover the full traced stopwatch row
(`0.759068s`), so future macOS trace work should reduce or account for phase
trace overhead before using traced stopwatch ratios as optimization proof. The
remaining product gap is still advertisement wait variance, sideband delivery,
and checkout materialization.

Phase-trace RSS overhead cleanup follow-up:
`ZMIN_PHASE_TRACE=1` no longer samples RSS by default. RSS fields remain in
`zmin-phase` rows for parser compatibility, but they are emitted as
`rss_bytes=0` and `rss_delta_bytes=0` unless `ZMIN_PHASE_TRACE_RSS=1` is set.
This keeps normal phase traces focused on timing; RSS sampling can still be
enabled explicitly when memory evidence is the goal.

Validation:

- `rustfmt --edition 2024 --check crates/zmin-cli/src/runtime/phase_trace.rs crates/zmin-cli/src/cli/commands/transport_impl.rs`
- `cargo test -p zmin-cli --test git_transport_http_compat clone_instant_ssh_materializes_head_then_fetch_hydrates_refs -- --nocapture`
- `cargo test -p zmin-cli --test git_clone_compat clone_local_repo_matches_stock_git_state -- --nocapture`
- Windows/Git-for-Windows targeted validation with the unrelated whole-tree fmt
  preflight skipped:
  `ZMIN_WINDOWS_VALIDATE_NO_FMT=1 tools/parallels-windows-runner.sh validate targeted git_transport_http_compat clone_instant_ssh_materializes_head_then_fetch_hydrates_refs`
  (`1/1` passing)
- local `status` trace smoke without `ZMIN_PHASE_TRACE_RSS`:
  `/tmp/zmin-phase-trace-smoke.4D86SR/no-rss.log`
- local `status` trace smoke with `ZMIN_PHASE_TRACE_RSS=1`:
  `/tmp/zmin-phase-trace-smoke.4D86SR/rss.log`
- macOS traced focused `clone-instant-ssh` run after adding `clone.total`:
  `/tmp/zmin-macos-ssh-clone-total-trace-20260617T173013Z`

Key evidence:

| Scenario | Representative phase | Seconds | RSS fields | Note |
| --- | --- | ---: | --- | --- |
| no RSS status smoke | `status.find_repo` | `0.000146` | `0 / 0` | cheap timing trace, same row shape |
| RSS status smoke | `status.find_repo` | `0.002175` | non-zero | macOS `/bin/ps` sampling adds milliseconds per phase |
| no RSS SSH trace | `clone-instant-ssh` | `1.830026` ratio | `0 / 0` | phase evidence only; checks ok |

The follow-up trace showed `clone.total=0.206985s` and
`clone_ssh=0.206743s` against a measured process row of `0.646539s`. That means
the remaining untraced stopwatch gap in this setup is outside the Rust clone
body reached by `run_clone` (process startup, CLI parsing before dispatch,
benchmark wrapper, or OS scheduling), not hidden inside the SSH transport
subphases. For the product gap, the useful in-process targets remain
`ssh_upload_pack.open.advertisement` (`0.075753s` in this trace),
`upload_pack.sideband` frame reads (`0.051544s`), and checkout materialization
(`materialize_file_open=0.036204s` in aggregate worker timing).

Early CLI trace classification follow-up:
The CLI wrapper now emits top-level phase labels for `cli.process`,
`cli.total`, `cli.parse`, `cli.dispatch`, and `cli.cleanup`. This is telemetry
only; the release build attempts for the follow-up benchmark were terminated by
SIGTERM 15 / exit 143 without a Rust diagnostic, so this slice does not make a
release performance-ratio claim.

Validation:

- `rustfmt --edition 2024 --check --config skip_children=true crates/zmin-cli/src/cli/mod.rs`
- `rustfmt --edition 2024 --check crates/zmin-cli/src/runtime/phase_trace.rs crates/zmin-cli/src/cli/commands/transport_impl.rs`
- `cargo test -p zmin-cli --test git_transport_http_compat clone_instant_ssh_materializes_head_then_fetch_hydrates_refs -- --nocapture`
- `cargo test -p zmin-cli --test git_clone_compat clone_local_repo_matches_stock_git_state -- --nocapture`
- debug-only traced focused `clone-instant-ssh` run:
  `/tmp/zmin-macos-ssh-cli-total-debug-trace-20260617T174528Z`

Key debug evidence:

| Scenario | Representative phase | Seconds | Note |
| --- | --- | ---: | --- |
| debug external row | `clone-instant-ssh` | `1.829223` | debug build only, classification evidence |
| debug CLI wrapper | `cli.process` | `0.936069` | earliest traced Rust wrapper label |
| debug CLI total | `cli.total` | `0.935712` | includes parse, dispatch, cleanup |
| debug CLI parse | `cli.parse` | `0.007548` | not the product gap |
| debug clone body | `clone.total` | `0.927203` | matches `cli.dispatch=0.927247` |
| debug SSH discovery | `ssh_upload_pack.open.advertisement` | `0.300634` | still a meaningful in-process target |
| debug SSH fetch | `clone_ssh.fetch_objects` | `0.467258` | includes slow debug `index_pack=0.394928` |

The debug trace confirms that parse/dispatch wrappers do not explain the
external stopwatch gap: even `cli.process` covers only about half of the debug
measured row. Treat the remaining difference as outside the traced Rust CLI
section (pre-main process startup, dynamic loader, OS scheduling, or benchmark
wrapper). Product work should return to in-process release targets already
identified by the release trace, or use a dedicated process-startup profiler
before optimizing CLI parse/dispatch.

Release CLI trace follow-up after successful rebuild:
The current dirty worktree release binary now builds successfully again:

- `/usr/bin/time -lp cargo build --manifest-path Cargo.toml --release -p zmin-cli --bin zmin`
  completed in `155.22s` real time with no Rust diagnostic.
- focused fair macOS `clone-instant-ssh` run:
  `/tmp/zmin-macos-ssh-release-fair-20260617T175149Z`
- focused release trace:
  `/tmp/zmin-macos-ssh-release-trace-20260617T175232Z`

Fair evidence:

| Operation | Runs | Git mean | Zmin mean | Mean ratio | Git median | Zmin median | Median ratio | Checks |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| `clone-instant-ssh` | `3` | `0.222254` | `0.214349` | `0.964430` | `0.164305` | `0.205576` | `1.251185` | ok |

Release trace evidence:

| Phase | Seconds | Note |
| --- | ---: | --- |
| external `clone-instant-ssh` row | `0.240702` | one-repeat trace row, phase evidence only |
| `cli.process` | `0.220409` | release now accounts for almost all Zmin row time |
| `cli.parse` | `0.002801` | still not the gap |
| `clone.total` | `0.216262` | clone body dominates CLI dispatch |
| `ssh_upload_pack.open.advertisement` | `0.056893` | remote upload-pack wait |
| `upload_pack.sideband` | `0.051970` | `frame_read_seconds=0.051738` |
| `ssh_fetch_pack.index_pack` | `0.019767` | local pack indexing |
| `clone_ssh.write_refs_config.write_refs` | `0.023415` | retained loose-ref layout |
| `clone_ssh.checkout` | `0.055041` | checkout wall phase |

The release trace supersedes the debug-only process-gap classification for the
current worktree: `cli.process` now covers nearly all of the measured Zmin row,
so a broad untraced Rust startup gap is not visible in this release run. The
fair gate still leaves a median gap because Git has very fast warm rows, so the
macOS SSH instant clone target remains open. The next evidence-led work should
target one of the measured release phases (`ssh_upload_pack.open.advertisement`,
sideband frame waits, checkout materialization, or a narrowly justified
ref-write change) and rerun the same fair gate before accepting it.

Windows/Git-for-Windows SSH instant clone refresh:
The same scoped operation was rerun through the Parallels Windows runner. The
first 3-repeat run had a single cold Zmin outlier, so a warm rerun was used for
the fair Windows decision. No runtime change was accepted in this pass.

Validation/evidence:

- first scoped Windows fair run:
  `tools/parallels-windows-runner.sh benchmark 3 clone-instant-ssh`
  with output at `C:\Users\skron\zmin-bench-20260617T175626Z-89538-out`
- warm scoped Windows fair rerun:
  `C:\Users\skron\zmin-bench-20260617T180113Z-92828-out`
- scoped Windows phase trace:
  `C:\Users\skron\zmin-bench-20260617T180304Z-93263-out`

Fair evidence:

| Run | Operation | Runs | Git mean | Zmin mean | Mean ratio | Git median | Zmin median | Median ratio | Checks |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| first | `clone-instant-ssh` | `3` | `0.860051` | `1.036992` | `1.205733` | `0.922095` | `1.658067` | `1.798152` | ok |
| warm rerun | `clone-instant-ssh` | `3` | `0.962840` | `0.693923` | `0.720704` | `1.136883` | `0.802489` | `0.705868` | ok |

The first run's raw rows were Git `0.922095/0.857340/0.800718` and Zmin
`1.658067/0.685316/0.767594`, so the slower aggregate was driven by one cold
Zmin row. The warm rerun cleared that outlier and was faster on both mean and
median with the same `HEAD`, `HEAD^{tree}`, and `zmin.worktreeFirst=true`
checks passing.

Windows phase evidence from the one-repeat trace:

| Phase | Seconds | Note |
| --- | ---: | --- |
| external `clone-instant-ssh` row | `0.633882` | trace row, phase evidence only |
| `cli.process` | `0.593658` | most measured Zmin row time is inside CLI process |
| `clone.total` | `0.571750` | clone body dominates |
| `ssh_upload_pack.open.advertisement` | `0.292834` | largest single traced phase |
| `upload_pack.sideband` | `0.085090` | `frame_read_seconds=0.084946` |
| `clone_ssh.checkout` | `0.155611` | checkout wall phase |
| `checkout_index_fresh_into_metadata.materialize_file_open` | `0.137991` | aggregate worker timing |
| `stream_attempts / stream_written / skipped` | `128 / 0 / 352` | stream-probe cutoff behaved as intended |

This refresh keeps Windows ahead of Git on the current scoped fake-SSH loopback
fixture after a warm rerun, while macOS still has the open median gap. Future
SSH instant clone work should be judged by both: a macOS fair improvement and
no Windows regression in the same scoped gate.

macOS 10-repeat SSH instant clone baseline refresh:
The macOS gap was rerun with more repeats to separate the stable median from
small-sample ordering noise. No runtime change was accepted from this pass.

Validation/evidence:

- fair scoped macOS run:
  `/tmp/zmin-macos-ssh-release-fair-10x-20260617T180654Z`
- 3-repeat phase trace:
  `/tmp/zmin-macos-ssh-release-trace-3x-20260617T180732Z`
- all `clone-instant-ssh` `HEAD`, `HEAD^{tree}`, and
  `zmin.worktreeFirst=true` checks were `ok`

Fair evidence:

| Operation | Runs | Git mean | Zmin mean | Mean ratio | Git median | Zmin median | Median ratio | Checks |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| `clone-instant-ssh` | `10` | `0.209034` | `0.230780` | `1.104032` | `0.161808` | `0.217298` | `1.342937` | ok |

The first Git row was cold (`0.607202s`), but the 10-repeat median still shows
Zmin slower than Git. Zmin rows were tighter (`0.191655s` to `0.295417s`) than
Git rows (`0.140975s` to `0.607202s`), so the remaining target is not only a
single Zmin outlier.

3-repeat release trace summary:

| Phase | Mean seconds | Median seconds | Note |
| --- | ---: | ---: | --- |
| external `clone-instant-ssh` row | `0.224401` | `0.204589` | trace rows only |
| `cli.process` | `0.208782` | `0.190122` | most measured Zmin row time is inside CLI process |
| `clone.total` | `0.206027` | `0.189017` | clone body dominates |
| `ssh_upload_pack.open.advertisement` | `0.052541` | `0.047592` | upload-pack wait |
| `upload_pack.sideband` | `0.051382` | `0.047998` | mostly frame read wait |
| `ssh_fetch_pack.index_pack` | `0.019174` | `0.018957` | stable local pack indexing |
| `clone_ssh.write_refs_config` | `0.018270` | `0.017793` | retained loose-ref layout |
| `clone_ssh.checkout` | `0.056088` | `0.050550` | checkout wall phase |
| checkout `materialize_file_open` | `0.045071` | `0.036851` | aggregate worker timing |
| stream metrics | `128 / 0 / 472` | `128 / 0 / 472` | cutoff behaved as intended |

This confirms the macOS SSH instant clone gap is stable enough to keep working,
but the trace still splits time across upload-pack advertisement wait, sideband
frame wait, and checkout materialization. The next accepted runtime experiment
should target one of those measured phases and show both a macOS 10-repeat
movement and no regression in the scoped Windows fake-SSH gate.

Paired-ratio benchmark reporting follow-up:
`tools/git-performance-bench.sh` and `tools/windows-native-benchmark.ps1` now
append paired-ratio fields to `comparison.csv`, matching rows by each measured
row's `extra` value such as `1/ssh`. Existing aggregate mean/median columns are
unchanged.

New comparison fields:

- `zmin_vs_git_pair_count`
- `zmin_vs_git_pair_mean_ratio`
- `zmin_vs_git_pair_median_ratio`
- `zmin_vs_git_pair_min_ratio`
- `zmin_vs_git_pair_max_ratio`
- `zmin_vs_gix_pair_count`
- `zmin_vs_gix_pair_mean_ratio`
- `zmin_vs_gix_pair_median_ratio`

Validation/evidence:

- `bash -n tools/git-performance-bench.sh tools/parallels-windows-runner.sh`
- macOS scoped smoke:
  `/tmp/zmin-macos-paired-ratio-smoke-20260617T181223Z`
- Windows/Git-for-Windows scoped smoke:
  `C:\Users\skron\zmin-bench-20260617T181240Z-25641-out`

The macOS smoke produced paired Git ratios for both repeats
(`pair_count=2`, pair median `0.937755`) while the aggregate median ratio was
`0.726666`, proving the new columns capture per-repeat movement separately from
sorted aggregate medians. The Windows smoke produced `pair_count=1` and matching
aggregate/paired ratios (`0.875889`) with the usual `HEAD`, `HEAD^{tree}`, and
`zmin.worktreeFirst=true` checks ok.

Paired-ratio macOS SSH baseline:
After adding paired ratios, the current macOS `clone-instant-ssh` gate was
rerun with 10 repeats:

- output:
  `/tmp/zmin-macos-ssh-paired-10x-20260617T181505Z`
- all `HEAD`, `HEAD^{tree}`, and `zmin.worktreeFirst=true` checks were `ok`

Key ratios:

| Metric | Ratio |
| --- | ---: |
| aggregate mean | `1.209660` |
| aggregate median | `1.258879` |
| paired mean | `1.232935` |
| paired median | `1.238333` |
| paired min | `1.014657` |
| paired max | `1.346750` |

Every matched repeat was slower for Zmin than Git in this run. The cold first
pair was nearly even (`Git 0.354645s`, `Zmin 0.359843s`, ratio `1.014657`), but
warm pairs stayed around `1.11x` to `1.35x` slower. This makes the macOS SSH
instant clone gap stronger than the earlier aggregate-only evidence: the next
runtime experiment should improve paired ratios as well as aggregate mean/median,
and must still preserve the scoped Windows gate.

Paired macOS git-daemon control and open-temp-file rejection:
A matching 10-repeat git-daemon control confirmed the remote clone/check-out
path is not generally slower on macOS:

- output:
  `/tmp/zmin-macos-git-daemon-paired-10x-20260617T181804Z`
- all `HEAD`, `HEAD^{tree}`, and `zmin.worktreeFirst=true` checks were `ok`

Key git-daemon ratios:

| Metric | Ratio |
| --- | ---: |
| aggregate mean | `0.853809` |
| aggregate median | `0.835432` |
| paired mean | `0.854506` |
| paired median | `0.860001` |
| paired min | `0.787771` |
| paired max | `0.931399` |

That result keeps the current macOS gap scoped to SSH advertisement/upload-pack
delivery rather than the shared remote clone/check-out machinery. A small SSH
and git-daemon fetch experiment that reused the already-open temporary pack file
for sideband parsing, matching the HTTP path, was tested and reverted because it
regressed the macOS SSH gate:

- rejected output:
  `/tmp/zmin-macos-ssh-open-tempfile-10x-20260617T182259Z`
- all `clone-instant-ssh` checks were `ok`
- rejected ratios: aggregate mean `1.425546`, aggregate median `1.288827`,
  paired mean `1.352521`, paired median `1.298316`, paired min `1.114903`,
  paired max `2.026822`

The trial was worse than the paired baseline (`1.238333` paired median), so it
was removed. Do not retry open-temp-file sideband parsing as the macOS SSH
instant clone fix unless new evidence points at file reopen cost specifically.

Fake SSH wrapper timing:
The benchmark tooling now has opt-in fake SSH wrapper timing to separate the
measured clone command stopwatch from the remote `git-upload-pack` process
lifetime inside the benchmark's fake SSH command.

New controls:

- macOS/Linux benchmark:
  `ZMIN_BENCH_SSH_TRACE_DIR=/path/to/ssh-traces`
- Windows benchmark:
  `-SshTraceDir C:\path\to\ssh-traces`
- Parallels runner:
  `ZMIN_WINDOWS_BENCH_SSH_TRACE=1`

Trace files are per measured `clone-instant-ssh` row and include
`tool`, `op`, `extra`, `git_protocol`, `start_ns`, `end_ns`, `real_seconds`,
`exit`, and the remote command. The default benchmark path is unchanged unless
the trace directory is set.

Validation/evidence:

- syntax:
  `bash -n tools/git-performance-bench.sh tools/parallels-windows-runner.sh`
- macOS smoke:
  `/tmp/zmin-macos-ssh-protocol-trace-smoke-20260617T183307Z`
- macOS phase + fake SSH trace:
  `/tmp/zmin-macos-ssh-wrapper-phase-3x-20260617T183209Z`
- Windows/Git-for-Windows smoke:
  `C:\Users\skron\zmin-bench-20260617T183331Z-87276-out`

The macOS 3-repeat trace had all checks ok and showed the fake SSH remote
process lifetime was much longer for Zmin than Git:

| Tool | Count | Mean | Median | Min | Max |
| --- | ---: | ---: | ---: | ---: | ---: |
| Git | `3` | `0.028451s` | `0.028026s` | `0.026946s` | `0.030380s` |
| Zmin | `3` | `0.111649s` | `0.103586s` | `0.103052s` | `0.128309s` |

The same macOS run had Zmin phase means `clone.total=0.223376s`,
`ssh_upload_pack.open.advertisement=0.061772s`,
`ssh_fetch_pack.sideband_to_pack=0.063376s`, and
`clone_ssh.checkout=0.052932s`. The wrapper trace also proved `GIT_PROTOCOL`
was empty for both Git and Zmin in the fake SSH command, so this specific
difference is not explained by Git passing `GIT_PROTOCOL=version=2`.

The Windows one-repeat smoke validated the new `-SshTraceDir` path and runner
flag. It had all SSH clone checks ok, but showed a different profile from
macOS: Git's fake SSH remote lifetime was `0.403205s`, Zmin's was `0.292111s`,
while the external stopwatch still had a slower Zmin row (`1.278015` ratio).
Treat fake SSH lifetime as a diagnostic dimension, not an acceptance gate by
itself. The next macOS SSH work should target client-side upload-pack delivery
or request/read flow with paired fair evidence, while Windows must still be
checked for stopwatch regressions separately.

Sequential SSH instant clone follow-up:
A fresh scoped release run kept the macOS `clone-instant-ssh` gap reproducible
after the hooks/CMS validation slice. No runtime change was accepted in this
pass because the phase evidence still points at several small contributors
rather than one safe fix.

Validation/evidence:

- fair scoped macOS run:
  `ZMIN_BENCH_REPEATS=3 ZMIN_BENCH_OPS=clone-instant-ssh ZMIN_BENCH_OUT_DIR=/tmp/zmin-macos-ssh-fair-20260617Tseq tools/git-performance-bench.sh`
- trace scoped macOS run:
  `ZMIN_BENCH_REPEATS=1 ZMIN_BENCH_OPS=clone-instant-ssh ZMIN_BENCH_OUT_DIR=/tmp/zmin-macos-ssh-trace-20260617Tseq ZMIN_BENCH_PHASE_TRACE_DIR=/tmp/zmin-macos-ssh-trace-20260617Tseq/traces tools/git-performance-bench.sh`
- all `clone-instant-ssh` checks were `ok`

Key evidence:

| Scenario | Git | Zmin | Zmin/Git ratio | Note |
| --- | ---: | ---: | ---: | --- |
| fair mean | `0.259830s` | `0.353173s` | `1.359247` | still slower than Git |
| fair median | `0.164339s` | `0.220020s` | `1.338818` | still slower than Git |
| trace row | `0.367343s` | `0.519215s` | `1.413553` | phase evidence only |

The trace has `cli.process=0.231693s`, `clone.total=0.229851s`,
`ssh_upload_pack.open.advertisement=0.061500s`,
`upload_pack.sideband=0.053407s`, `clone_ssh.write_refs_config=0.027194s`,
and `checkout_fresh.checkout_index=0.057230s`. Do not repeat the already
rejected global worker-cap, stream-threshold, stdout-buffer, packed-refs, or
narrow SSH protocol-v2 experiments for this gap. The next accepted SSH work
needs either a dedicated process-startup/wrapper profile or a narrower
checkout/materialization or upload-pack delivery fix with clean fair evidence.

Current dirty-worktree SSH instant clone refresh:
After the Windows `t0027` full-file signoff and the repository rename worktree
state, the same scoped macOS fake-SSH gate was rerun to refresh the baseline
before any new runtime experiment. No runtime change was accepted in this pass.

Validation/evidence:

- fair scoped macOS run:
  `/tmp/zmin-macos-ssh-current-fair-3x-20260618T180242Z`
- trace scoped macOS run:
  `/tmp/zmin-macos-ssh-current-trace-1x-20260618T180304Z`
- all `clone-instant-ssh` `HEAD`, `HEAD^{tree}`, and
  `zmin.worktreeFirst=true` checks were `ok`

Key fair ratios:

| Metric | Ratio |
| --- | ---: |
| aggregate mean | `1.267754` |
| aggregate median | `1.360408` |
| paired mean | `1.318018` |
| paired median | `1.312756` |
| paired min | `1.214692` |
| paired max | `1.426605` |

The 3-repeat fake-SSH wrapper traces still show a longer remote
`git-upload-pack` lifetime for Zmin than Git on macOS:

| Tool | Count | Mean | Median | Min | Max |
| --- | ---: | ---: | ---: | ---: | ---: |
| Git | `3` | `0.027642s` | `0.024099s` | `0.023954s` | `0.034873s` |
| Zmin | `3` | `0.114577s` | `0.096128s` | `0.095589s` | `0.152015s` |

The one-repeat trace row was intentionally used only for phase diagnosis:
external ratio `1.520792`, `cli.process=0.259948s`, `clone.total=0.255004s`,
`ssh_upload_pack.open.advertisement=0.089031s`,
`upload_pack.sideband=0.067104s`, `clone_ssh.write_refs_config=0.007703s`,
and `checkout_fresh.checkout_index=0.042506s`. The clone path already reuses
the advertised SSH upload-pack session; there is no second SSH advertisement
round trip to remove. Keep the macOS SSH instant clone gap open and require any
next runtime experiment to improve paired ratios while preserving the scoped
Windows fake-SSH stopwatch gate.

Current dirty-worktree Windows SSH instant clone refresh:
The matching scoped Windows/Git-for-Windows fake-SSH gate was rerun through the
Parallels benchmark runner with SSH wrapper tracing enabled. This validates the
current dirty worktree as a Windows preservation point for future macOS SSH
experiments; no runtime change was accepted in this pass.

Validation/evidence:

- scoped Windows run:
  `C:\Users\skron\zmin-bench-20260618T180552Z-70636-out`
- command:
  `ZMIN_WINDOWS_BENCH_OPS=clone-instant-ssh ZMIN_WINDOWS_BENCH_SSH_TRACE=1 tools/parallels-windows-runner.sh benchmark 1 clone-instant-ssh`
- all `clone-instant-ssh` `HEAD`, `HEAD^{tree}`, and
  `zmin.worktreeFirst=true` checks were `ok`

Key ratios:

| Metric | Ratio |
| --- | ---: |
| aggregate mean | `0.955303` |
| aggregate median | `0.955303` |
| paired mean | `0.955303` |
| paired median | `0.955303` |

Raw stopwatch rows were Git `1.949853s` and Zmin `1.862700s`. The Windows fake
SSH wrapper traces also favored Zmin in this run: Git remote
`git-upload-pack` lifetime `0.382498s`, Zmin `0.312930s`, both with empty
`GIT_PROTOCOL`. Keep using this scoped Windows gate as the regression check for
the next macOS SSH instant clone runtime experiment.

Fake SSH exec-path parity follow-up:
The macOS fake-SSH lifetime gap above was partly a benchmark fixture issue, not
a Zmin protocol regression. `tools/git-performance-bench.sh` and
`tools/windows-native-benchmark.ps1` now set
`ZMIN_BENCH_FAKE_SSH_GIT_EXEC_PATH` from the benchmark Git executable's
`git --exec-path`, and the fake SSH wrapper prepends that path before invoking
`git-upload-pack`. This forces Git and Zmin rows to talk to the same local
upload-pack binary. The benchmark tooling also gained opt-in per-row packet
logs: `ZMIN_BENCH_SSH_PACKET_TRACE_DIR` on macOS, `-SshPacketTraceDir` on
Windows, and `ZMIN_WINDOWS_BENCH_SSH_PACKET_TRACE=1` through the Parallels
runner.

Diagnostic packet traces:

- shared macOS packet trace:
  `/tmp/zmin-macos-ssh-packet-trace-1x-20260618T180902Z`
- per-row macOS packet trace before the exec-path fix:
  `/tmp/zmin-macos-ssh-packet-per-row-1x-20260618T181103Z`
- per-row macOS packet trace after the exec-path fix:
  `/tmp/zmin-macos-ssh-execpath-fair-1x-20260618T181236Z`
- Windows packet trace validation:
  `C:\Users\skron\zmin-bench-20260618T181522Z-25510-out`

The first two macOS traces confirmed the unfair fixture: the Git row used
`agent=git/2.50.1-Darwin`, while the Zmin fake-SSH remote used
`agent=git/2.53.0-Darwin`. After the wrapper fix, both macOS rows used
`agent=git/2.50.1-Darwin`; the one-repeat trace checks were ok and fake-SSH
lifetime was Git `0.032547s` vs Zmin `0.027394s`. The Windows packet trace also
confirmed both rows use `agent=git/2.54.0.windows.1-Windows`. Packet tracing is
diagnostic only because it perturbs the stopwatch rows.

Clean fair macOS evidence after the fixture fix:

- 3-repeat run:
  `/tmp/zmin-macos-ssh-execpath-fair-3x-20260618T181303Z`
- 10-repeat run:
  `/tmp/zmin-macos-ssh-execpath-fair-10x-20260618T181328Z`
- all `clone-instant-ssh` `HEAD`, `HEAD^{tree}`, and
  `zmin.worktreeFirst=true` checks were `ok`

| Run | Aggregate mean | Aggregate median | Paired mean | Paired median |
| --- | ---: | ---: | ---: | ---: |
| macOS 3x | `1.144648` | `0.989566` | `1.076771` | `1.032245` |
| macOS 10x | `1.074071` | `1.004285` | `1.043337` | `1.028492` |

The 10-repeat fake-SSH remote lifetime summary now favors Zmin: Git
mean/median `0.029194s` / `0.026138s`, Zmin `0.025369s` / `0.024816s`. This
supersedes the earlier wrapper-lifetime diagnosis and materially reduces the
macOS 10-repeat paired gap from `1.238333` to `1.028492`, but it does not close
the macOS SSH instant clone performance target. Remaining work should target
the small non-remote-lifetime overhead that keeps the paired median above Git.

Clean Windows preservation evidence after the fixture fix:

- packet-trace validation:
  `C:\Users\skron\zmin-bench-20260618T181522Z-25510-out`
- clean 3-repeat run without packet tracing:
  `C:\Users\skron\zmin-bench-20260618T181638Z-28896-out`
- all `clone-instant-ssh` `HEAD`, `HEAD^{tree}`, and
  `zmin.worktreeFirst=true` checks were `ok`

The clean Windows 3-repeat gate stayed faster than Git: aggregate mean ratio
`0.874218`, aggregate median ratio `0.805154`, paired mean ratio `0.885576`,
and paired median ratio `0.959802`. Fake SSH remote lifetime also stayed
slightly faster for Zmin: Git mean/median `0.421419s` / `0.422468s`, Zmin
`0.392450s` / `0.383335s`. Keep Windows as preserved after the exec-path
fixture fix, while treating the packet-traced one-repeat stopwatch
(`1.448177`) as diagnostic overhead/noise rather than acceptance evidence.

Current macOS scoped SSH refresh after rebuilding the release binary:

- 10-repeat fair run:
  `/tmp/zmin-macos-ssh-current-fair-10x-20260618T210441Z`
- one-repeat diagnostic trace:
  `/tmp/zmin-macos-ssh-current-trace-1x-20260618T210909Z`
- all `clone-instant-ssh` `HEAD`, `HEAD^{tree}`, and
  `zmin.worktreeFirst=true` checks were `ok`

| Run | Aggregate mean | Aggregate median | Paired mean | Paired median |
| --- | ---: | ---: | ---: | ---: |
| macOS 10x current release | `0.725666` | `0.695147` | `0.756060` | `0.740152` |
| macOS larger 3x current release | `0.667712` | `0.715473` | `0.748495` | `0.784204` |

This supersedes the earlier macOS SSH instant-clone gap on the scoped fixture:
the current rebuilt release binary is faster than Git on mean, median, paired
mean, and paired median. The diagnostic trace is not acceptance timing because
it enables phase, fake-SSH, and packet logging; it still usefully confirms both
rows use `agent=git/2.50.1-Darwin`, fake-SSH lifetime favors Zmin (Git
`0.047380s`, Zmin `0.029343s`), and the remaining Zmin time is mostly local
checkout/materialization (`checkout_fresh.checkout_index=0.097589s` for 600
entries, with `materialize_file_open=0.048000s`).

The larger scoped macOS fixture used
`ZMIN_BENCH_COMMITS=180 ZMIN_BENCH_FILES_PER_COMMIT=40` with three repeats:
`/tmp/zmin-macos-ssh-large-fair-3x-20260618T211317Z`. It kept all checks ok and
also favored Zmin despite one paired outlier. Keep broader clone performance
open for still-larger fixtures, real network/auth/proxy variants, and repeated
cross-platform gates.

Windows SSH preservation retry caveat:
Follow-up Windows/Git-for-Windows `clone-instant-ssh` retries after the macOS
closure are not accepted as clean preservation evidence. Correctness checks
stayed ok in the produced benchmark artifacts, but the VM was concurrently
running a focused `git_transport_http_compat` test and an isolated
`zmin-git-remote-http` helper build under
`C:\Users\skron\zmin-20260618T221408Z-5443\target\test-remote-http-helper`.
That guest build/test contention makes the stopwatch ratios unsuitable for a
product regression or preservation claim.

Artifacts to treat as noisy/contended:

- traced failed build attempt:
  `C:\Users\skron\zmin-bench-20260618T215515Z-94520` (removed before out dir)
- traced 3-repeat run:
  `C:\Users\skron\zmin-bench-20260618T220543Z-3057-out`
- traced warm 3-repeat run:
  `C:\Users\skron\zmin-bench-20260618T221053Z-3916-out`
- untraced 5-repeat run:
  `C:\Users\skron\zmin-bench-20260618T221238Z-4289-out`
- clean untraced retry after guest processes cleared:
  `C:\Users\skron\zmin-bench-20260618T222044Z-11414-out` (failed before
  timing; `work\ssh-remote.git` was absent and no CSVs were produced)

The traced 3-repeat runs were slower/noisy despite ok checks, and the untraced
5-repeat run had a faster aggregate median but slower paired/mean ratios while
the guest was building the remote HTTP helper. A later full 3-repeat Windows
benchmark did complete cleanly before guest cleanup removed its artifact; the
captured output for `C:\Users\skron\zmin-bench-20260618T222653Z-41023-out`
had no failed checks, and its `clone-instant-ssh` row was mean/paired-mean
faster than Git but median noisy (`0.974159` mean ratio, `1.046030` median
ratio, `0.971601` paired mean, `1.046030` paired median).

The current accepted scoped Windows SSH preservation point is the clean
5-repeat rerun `C:\Users\skron\zmin-bench-20260618T223331Z-42606-out`.
It produced `bench.csv`, `checks.csv`, `summary.csv`, and `comparison.csv`,
had `15` checks with no failures, and left no matching guest processes. Zmin
was faster than Git on aggregate and paired ratios:

| Metric | Ratio |
| --- | ---: |
| aggregate mean | `0.979017` |
| aggregate median | `0.932111` |
| paired mean | `0.984014` |
| paired median | `0.860411` |

Test-helper stabilization note:
`ensure_remote_http_helper()` now builds `zmin-git-remote-http` into an isolated
`target/test-remote-http-helper` directory and copies the helper beside the
current test executable when needed, so transport tests do not contend with the
parent test package target. Focused macOS validation passed:
`cargo test -p zmin-cli --test git_transport_http_compat clone_instant_smart_http_demand_hydrate_recovers_missing_head_objects -- --exact --test-threads=1 --nocapture`.

Current cross-platform performance gate refresh:

- macOS full local 3-repeat gate:
  `/tmp/zmin-macos-performance-gate-20260618T222426Z`
- Windows/Git-for-Windows full 3-repeat gate with real Gitoxide:
  host-captured output from `C:\Users\skron\zmin-bench-20260618T222653Z-41023-out`
  before guest cleanup removed the artifact
- macOS failed-check filter returned only the header; Windows failed-check
  filter returned no rows. Treat both gates as correctness-clean.

macOS headline ratios from `comparison.csv`:

| Operation | Zmin/Git mean | Zmin/Git median | Zmin/Gitoxide mean | Zmin/Gitoxide median | Note |
| --- | ---: | ---: | ---: | ---: | --- |
| `add` | `0.677622` | `0.663224` | n/a | n/a | faster than Git |
| `clone` | `0.969930` | `0.969726` | `0.381058` | `0.403940` | near Git parity, faster than Gitoxide |
| `clone-instant` | `0.930454` | `0.896764` | n/a | n/a | faster than Git |
| `clone-instant-git-daemon` | `0.925723` | `0.905785` | n/a | n/a | faster than Git |
| `clone-instant-ssh` | `0.544516` | `0.824004` | n/a | n/a | faster than Git; one slow Git outlier affects mean |
| `fetch-incremental` | `1.275221` | `1.335211` | `0.404165` | `0.420169` | slower than Git, faster than Gitoxide |
| `fetch-batch` | `1.356012` | `1.213176` | `0.440008` | `0.440983` | slower than Git, faster than Gitoxide |
| `log` | `0.575197` | `0.790373` | `0.987337` | `0.978569` | faster than Git, near Gitoxide parity |
| `merge-base` | `0.603762` | `0.672728` | `0.740512` | `0.780194` | faster than Git and Gitoxide |
| `pull-incremental` | `1.202770` | `0.830179` | n/a | n/a | mean slower from one Zmin outlier; median faster |
| `push-batch` | `1.134906` | `1.093725` | n/a | n/a | slower than Git |
| `status` | `0.906159` | `0.807158` | `0.557811` | `0.697167` | faster than Git and Gitoxide |

Windows headline ratios from `comparison.csv`:

| Operation | Zmin/Git mean | Zmin/Git median | Zmin/Gitoxide mean | Zmin/Gitoxide median | Note |
| --- | ---: | ---: | ---: | ---: | --- |
| `add` | `1.147391` | `1.264337` | n/a | n/a | still slower than Git |
| `add-dirty` | `0.745180` | `0.690004` | n/a | n/a | faster than Git |
| `clone` | `2.234593` | `3.669840` | `0.969706` | `1.238878` | slower than Git; mean near Gitoxide, median slower |
| `clone-instant` | `1.066094` | `1.028556` | n/a | n/a | slightly slower than Git |
| `clone-instant-git-daemon` | `1.258647` | `1.266704` | n/a | n/a | slower than Git |
| `clone-instant-ssh` | `0.974159` | `1.046030` | n/a | n/a | mean slightly faster, median slightly slower |
| `fetch-incremental` | `0.495087` | `0.514856` | `0.628276` | `0.675862` | faster than Git and Gitoxide |
| `fetch-noop` | `0.151291` | `0.129791` | `0.283590` | `0.265583` | faster than Git and Gitoxide |
| `log` | `0.539253` | `0.553138` | `0.862014` | `0.693965` | faster than Git and Gitoxide on aggregate |
| `merge-base` | `0.446136` | `0.471417` | `0.779132` | `0.703876` | faster than Git and Gitoxide on aggregate |
| `pull-incremental` | `0.660155` | `0.448627` | n/a | n/a | faster than Git |
| `push-noop` | `0.527369` | `0.797983` | n/a | n/a | faster than Git, but noisy |
| `status` | `0.589863` | `0.556501` | `0.249768` | `0.182660` | faster than Git and Gitoxide |

Gate conclusion:
The current macOS and Windows performance gates are correctness-clean and include
real Gitoxide where the benchmark has comparable operations. The remaining
measured gaps are now explicit: macOS `fetch-incremental`, `fetch-batch`, and
`push-batch`; Windows `add`, default `clone`, `clone-instant`, and
`clone-instant-git-daemon`. Windows `clone-instant-ssh` was median-slower in
the full gate but is superseded for the scoped SSH preservation check by
`C:\Users\skron\zmin-bench-20260618T223331Z-42606-out`, where mean, median,
paired mean, and paired median all favor Zmin. Do not claim all performance
work is complete until the remaining non-SSH gaps are either improved or
explicitly accepted as out of scope for the current release.

MacOS fetch/push gap rerun and fetch telemetry:
The explicit macOS gaps from the full gate were rerun with a scoped 7-repeat
benchmark to separate stable gaps from 3-repeat noise. All checks were `ok`.

- scoped fair rerun:
  `/tmp/zmin-macos-fetch-push-gaps-20260619T223845Z`
- phase trace before fetch-detail labels:
  `/tmp/zmin-macos-fetch-push-trace-20260619T224020Z`
- phase trace after fetch-detail labels:
  `/tmp/zmin-macos-fetch-trace-detail-20260619T224530Z`

| Operation | Aggregate mean | Aggregate median | Paired mean | Paired median | Zmin vs Gix median |
| --- | ---: | ---: | ---: | ---: | ---: |
| `fetch-incremental` | `1.459156` | `1.227862` | `1.440862` | `1.249049` | `0.393288` |
| `fetch-batch` | `1.187526` | `1.146005` | `1.186970` | `1.141461` | `0.425308` |
| `push-batch` | `1.190289` | `1.168394` | `1.191435` | `1.162718` | n/a |

The new local fetch phase labels keep behavior unchanged and split the local
configured-fetch path into source/setup, root collection, reachable-object
copying, ref updates, `FETCH_HEAD`, and tag copying. One-repeat trace evidence
shows the current fetch time is mostly reachable-object copy plus ref update:

| Operation | `copy_reachable_objects` | `apply_refspecs` | `fetch.total` |
| --- | ---: | ---: | ---: |
| `fetch-incremental` | `0.045428s` | `0.015859s` | `0.066502s` |
| `fetch-batch` | `0.059083s` | `0.018920s` | `0.083179s` |

For `push-batch`, the prior trace in
`/tmp/zmin-macos-fetch-push-trace-20260619T224020Z` showed
`push.local.copy_reachable_objects=0.314928s` out of `push.total=0.321103s`.
The next performance slice should target the shared reachable-object copy path
and local ref update path before retesting the macOS fetch/push gate.

MacOS local-fetch destination-have exclusion:
The next scoped slice made local fetch use destination refs as `have` roots for
the local object graph walk, matching the already-existing push-side idea of
excluding objects reachable from the other side before collecting pack ids. This
keeps the same object-store validation and missing-object copy rules, but avoids
walking the full old history for incremental local fetch.

- scoped fair rerun:
  `/tmp/zmin-macos-fetch-exclude-have-20260618T225748Z`
- phase trace:
  `/tmp/zmin-macos-fetch-exclude-have-trace-20260618T230048Z`
- validation:
  all benchmark checks were `ok`; `git_transport_local_compat` passed `88/88`.

| Operation | Aggregate mean | Aggregate median | Paired mean | Paired median | Zmin vs Gix median |
| --- | ---: | ---: | ---: | ---: | ---: |
| `fetch-incremental` | `0.787816` | `0.752560` | `0.775390` | `0.759823` | `0.234301` |
| `fetch-batch` | `1.181593` | `1.173324` | `1.183456` | `1.177318` | `0.417504` |
| `push-batch` | `1.191205` | `1.193044` | `1.191399` | `1.197094` | n/a |

The `fetch-incremental` scoped macOS gap is closed for this fixture: median
Zmin time moved from `0.081296s` before the exclusion slice to `0.048422s`,
and phase trace shows `fetch.local.copy_reachable_objects` moving from
`0.045428s` to `0.011425s` with one destination excluded root. `fetch-batch`
still has a material gap on the same gate, with trace time mostly in copying
the new batch objects (`fetch.local.copy_reachable_objects=0.055430s`) and
ref updates (`fetch.local.apply_refspecs=0.013499s`). `push-batch` remains
unchanged by this fetch-side slice and should stay on the open macOS
performance list.

MacOS local-fetch pack-sized missing-object filtering:
The next fetch-batch slice split the local multi-root copy trace and found the
remaining batch time was dominated by checking every new tree/blob id against
the destination object database before writing a pack. For pack-sized local
fetches, graph exclusion already removes objects reachable from destination
refs and an indexed pack may safely contain an object id that is already present
elsewhere in the object database. The implementation now keeps per-object
destination checks for the small loose-copy path, but extends the missing list
directly when the operation is already going to write a pack.

- scoped fair rerun:
  `/tmp/zmin-macos-fetch-pack-skip-contains-20260618T230628Z`
- phase trace:
  `/tmp/zmin-macos-fetch-pack-skip-contains-trace-20260618T230837Z`
- validation:
  all benchmark checks were `ok`; `git_transport_local_compat` passed `88/88`.

| Operation | Aggregate mean | Aggregate median | Paired mean | Paired median | Zmin vs Gix median |
| --- | ---: | ---: | ---: | ---: | ---: |
| `fetch-incremental` | `0.684219` | `0.754282` | `0.693591` | `0.684217` | `0.229391` |
| `fetch-batch` | `0.649863` | `0.637276` | `0.650488` | `0.633885` | `0.229342` |

Both scoped macOS fetch gaps are now closed for this fixture. The batch trace
shows `fetch.local.copy.record_tree_objects` dropping from `0.034992s` to
`0.000016s`, `fetch.local.copy_reachable_objects` dropping from `0.055881s` to
`0.020765s`, and `fetch.total=0.035314s` on the one-repeat diagnostic trace.
`push-batch` remains the explicit open macOS local-transport gap.

MacOS local-push undeltified pack write:
The push-batch trace showed the remaining local push gap was not pack-id
collection. It was local pack writing: `push.local.copy.write_missing_objects`
was `0.307829s` out of `push.total=0.327097s` in
`/tmp/zmin-macos-push-batch-detail-20260618T231149Z`. Local `send-pack`
already uses undeltified packs, so the local filesystem porcelain push path now
uses the same pack option. Network push paths are unchanged.

- scoped fair rerun:
  `/tmp/zmin-macos-push-undeltified-20260618T231429Z`
- phase trace:
  `/tmp/zmin-macos-push-undeltified-trace-20260618T231700Z`
- combined fetch/push confirmation:
  `/tmp/zmin-macos-fetch-push-closed-20260618T231739Z`
- validation:
  all benchmark checks were `ok`; `git_transport_local_compat` passed `88/88`.

| Operation | Aggregate mean | Aggregate median | Paired mean | Paired median |
| --- | ---: | ---: | ---: | ---: |
| `push-batch` | `0.883090` | `0.843625` | `0.881934` | `0.849670` |

The push trace after the change shows
`push.local.copy.write_missing_objects=0.200639s`,
`push.local.copy_reachable_objects=0.210805s`, and `push.total=0.219258s`.
The combined 7-repeat confirmation kept every validation check `ok` and had
median ratios favoring Zmin for all three scoped local-transport rows:
`fetch-incremental=0.627965`, `fetch-batch=0.959612`, and
`push-batch=0.831581`. The combined `fetch-batch` mean stayed noisy
(`1.039550`) because of two early Zmin outliers; the dedicated fetch-only gate
above remains the cleaner fetch-batch timing evidence (`0.649863` mean,
`0.637276` median).

Windows scoped fetch/push preservation after local-transport changes:
The Windows-native benchmark now includes a `fetch-batch` operation matching the
macOS local batch fixture shape: a separate bare remote, a base clone for each
tool, a 2400-file batch commit, repeated cloned fetch worktrees, `fsck` for
Zmin, and remote-tracking ref checks. The first Windows `fetch-batch` smoke
found a benchmark fixture bug: the new bare batch remote did not point `HEAD`
at `refs/heads/main`, so Zmin's base clone had no local branch and `fetch
origin` failed with `error: ref not found`. The benchmark fixture now sets the
bare remote HEAD before cloning, matching the other Windows local-remote
fixtures; the failed artifact is not timing evidence.

- failed fixture-diagnosis run:
  `C:\Users\skron\zmin-bench-20260618T232145Z-40835-out`
- fixed one-repeat `fetch-batch` smoke:
  `C:\Users\skron\zmin-bench-20260618T233019Z-41897-out`
- fixed three-repeat scoped gate:
  `C:\Users\skron\zmin-bench-20260618T233157Z-42084-out`
- validation:
  `10` checks, no non-`ok` rows; post-run guest process probe `process_count=0`.

| Operation | Mean ratio | Median ratio | Paired mean | Paired median | Zmin vs Gix median |
| --- | ---: | ---: | ---: | ---: | ---: |
| `fetch-incremental` | `0.158460` | `0.131595` | `0.168698` | `0.232668` | `0.244922` |
| `fetch-batch` | `0.151639` | `0.126801` | `0.158933` | `0.194469` | `0.243275` |
| `push-batch` | `0.037337` | `0.032621` | `0.038090` | `0.041526` | n/a |

This preserves the local fetch/push performance work on Windows/Git-for-Windows
for the scoped fixture and adds the missing Windows `fetch-batch` coverage. It
does not close the broader Windows performance goal from the full gate; older
full-gate gaps such as `add`, default `clone`, `clone-instant`, and
`clone-instant-git-daemon` still need their own current evidence or follow-up
work.

Cross-platform full-gate refresh after local-transport changes:
After the local fetch/push slices, both full local performance gates were rerun
to replace the older open-gap list with current evidence.

- macOS 3-repeat full gate:
  `/tmp/zmin-macos-current-full-gate-20260619T001`
- Windows/Git-for-Windows 3-repeat full gate:
  `C:\Users\skron\zmin-bench-20260618T234709Z-71238-out`
- validation:
  macOS had no non-`ok` rows in `checks.tsv`; Windows had no non-`ok` rows in
  `checks.csv`. A post-run Windows `tasklist` probe showed no lingering
  benchmark `git`, `git-daemon`, `cargo`, `rustc`, or `zmin` processes; only
  the probe's own `cmd.exe` / `tasklist.exe` were present.

Current macOS ratios:

| Operation | Mean ratio | Median ratio | Paired median | Zmin vs Gix median |
| --- | ---: | ---: | ---: | ---: |
| `commit` | `1.122916` | `1.143321` | `1.144384` | n/a |
| `clone-instant-ssh` | `0.459977` | `1.038870` | `0.947099` | n/a |
| `push-batch` | `1.052396` | `0.827278` | `0.827278` | n/a |
| `fetch-incremental` | `0.708960` | `0.706985` | `0.709188` | `0.229131` |
| `fetch-batch` | `0.665497` | `0.663059` | `0.689810` | `0.249595` |

The macOS local-transport gaps are closed in the full gate as well as the
scoped gates. `push-batch` has one cold Zmin outlier on mean (`0.466829s`) but
the aggregate and paired medians favor Zmin. The clearest remaining macOS
full-gate gap is `commit`; `clone-instant-ssh` is mixed/noisy because aggregate
median is slightly slower while paired median and mean favor Zmin.

Current Windows/Git-for-Windows ratios:

| Operation | Mean ratio | Median ratio | Paired median | Zmin vs Gix median |
| --- | ---: | ---: | ---: | ---: |
| `clone` | `2.298324` | `3.755054` | `4.687418` | `2.639025` |
| `clone-instant` | `1.136215` | `1.021717` | `1.252733` | n/a |
| `clone-instant-git-daemon` | `1.345728` | `1.149305` | `1.501317` | n/a |
| `clone-instant-ssh` | `0.925775` | `0.798440` | `1.042287` | n/a |
| `add` | `0.682281` | `0.551357` | `0.904493` | n/a |
| `fetch-incremental` | `0.201034` | `0.186062` | `0.212325` | `0.314815` |
| `fetch-batch` | `0.224081` | `0.221841` | `0.246636` | `0.286218` |
| `push-batch` | `0.054304` | `0.062775` | `0.073006` | n/a |

This supersedes the older Windows open-gap list: `add` and the local
fetch/push rows are now faster than Git on the full gate. The current stable
Windows performance work is clone-focused: default `clone` is slower than both
Git and Gitoxide, while `clone-instant` and `clone-instant-git-daemon` remain
behind Git. `clone-instant-ssh` is mixed but much closer; it should be preserved
while targeting the default/local and git-daemon clone paths.

Windows fresh-clone no-smudge fast path:
The next Windows trace slice targeted the remaining clone gaps. A scoped
phase-traced diagnostic run showed the fresh clone path was spending hundreds
of milliseconds in a post-checkout smudge scan even for benchmark repositories
with no root attributes and no checkout CRLF conversion:

- before trace:
  `C:\Users\skron\zmin-bench-20260618T235808Z-76515-out`
- after trace:
  `C:\Users\skron\zmin-bench-20260619T000315Z-79379-out`
- fair Windows 3-repeat scoped rerun:
  `C:\Users\skron\zmin-bench-20260619T000715Z-79879-out`
- macOS preservation scoped rerun:
  `/tmp/zmin-macos-clone-smudge-skip-20260619T001`
- validation:
  `cargo check -p zmin-cli --bin zmin --profile compat`;
  focused unit guard for `may_smudge_checkout_entries`;
  `git_worktree_state_compat checkout_core_autocrlf` (`3/3`);
  local `git_clone_compat clone_instant` (`2/2`);
  remote `git_transport_http_compat clone_instant_` (`9/9`);
  touched-file rustfmt; `git diff --check`.

The implementation now uses loaded `WorktreeStageOptions` / `WorktreeContentRules`
to skip the full post-checkout smudge pass only when the content rules prove
that checkout cannot rewrite content: no root attributes and `core.autocrlf`
does not output CRLF. Repositories with attributes or `core.autocrlf=true`
keep the existing smudge/filter behavior.

Trace deltas:

| Operation | Before `smudge_filters` | After `smudge_filters` | Before `clone.total` | After `clone.total` |
| --- | ---: | ---: | ---: | ---: |
| `clone` | `0.290217s` | `0.001640s` | `0.671065s` | `0.353670s` |
| `clone-instant` | `0.241594s` | `0.001683s` | `0.407266s` | `0.279059s` |
| `clone-instant-git-daemon` | `0.260921s` | `0.001331s` | `0.708242s` | `0.532808s` |

Fair Windows scoped ratios after the change:

| Operation | Mean ratio | Median ratio | Paired median | Zmin vs Gix median |
| --- | ---: | ---: | ---: | ---: |
| `clone` | `1.121092` | `1.160308` | `1.524839` | `0.525196` |
| `clone-instant` | `0.747176` | `1.116850` | `1.147372` | n/a |
| `clone-instant-git-daemon` | `1.156264` | `1.184291` | `1.227312` | n/a |

The default Windows clone gap is much smaller and Zmin is now faster than
Gitoxide on the scoped local clone fixture, but default `clone` is still behind
Git. `clone-instant` is faster on mean but still slower on aggregate/paired
median, so it remains near-parity rather than closed. `clone-instant-git-daemon`
remains a clear Windows gap.

macOS preservation ratios after the same change:

| Operation | Mean ratio | Median ratio | Paired median | Zmin vs Gix median |
| --- | ---: | ---: | ---: | ---: |
| `clone` | `2.219934` | `0.834371` | `0.825621` | `0.361715` |
| `clone-instant` | `0.831471` | `0.842320` | `0.813188` | n/a |
| `clone-instant-git-daemon` | `0.652645` | `0.841330` | `0.937316` | n/a |

The macOS scoped rerun had one cold Zmin `clone` outlier (`0.611844s`) that
only affects the mean; medians and paired medians preserve the clone rows.
Next Windows clone work should target fresh checkout materialization, especially
file open/write/metadata behavior, while preserving CRLF/attribute semantics and
the already-good macOS medians.

Windows fresh-checkout metadata syscall skip:
The follow-up Windows materialization slice removed one more avoidable fresh
checkout syscall. On non-Unix platforms, checkout metadata currently records
only the file size in the index, so freshly written regular files can update
`IndexEntry::size` from the final checkout content length instead of reopening
the path with `symlink_metadata`. Unix keeps the existing metadata path because
ctime, mtime, device, inode, uid, and gid are still recorded there.

- phase-traced Windows diagnostic:
  `C:\Users\skron\zmin-bench-20260619T001426Z-88032-out`
- fair Windows 3-repeat scoped rerun:
  `C:\Users\skron\zmin-bench-20260619T001757Z-88483-out`
- macOS preservation scoped rerun:
  `/tmp/zmin-macos-clone-metadata-skip-20260619T001`
- validation:
  `cargo test -p zmin-git-core checkout -- --nocapture` (`10/10`);
  `cargo check -p zmin-cli --bin zmin --profile compat`;
  `git_worktree_state_compat checkout_core_autocrlf` (`3/3`);
  `git_clone_compat clone_instant` (`2/2`);
  `git_transport_http_compat clone_instant_` (`9/9`);
  touched-file rustfmt; `git diff --check`.

Trace result:

| Operation | `metadata` | `checkout_index` | `clone.total` |
| --- | ---: | ---: | ---: |
| `clone` | `0.000000s` | `0.158755s` | `0.202451s` |
| `clone-instant` | `0.000000s` | `0.127807s` | `0.157194s` |
| `clone-instant-git-daemon` | `0.000000s` | `0.122158s` | `0.249098s` |

Fair Windows scoped ratios after the metadata skip:

| Operation | Mean ratio | Median ratio | Paired median | Zmin vs Gix median |
| --- | ---: | ---: | ---: | ---: |
| `clone` | `0.993263` | `1.286148` | `1.650266` | `0.517605` |
| `clone-instant` | `0.656808` | `0.830552` | `0.831308` | n/a |
| `clone-instant-git-daemon` | `0.765858` | `0.732455` | `0.913540` | n/a |

This closes the scoped Windows `clone-instant` and `clone-instant-git-daemon`
rows for this fixture. Default Windows `clone` is now mean parity/faster and
still faster than Gitoxide, but aggregate and paired medians remain behind Git
because one Zmin row was still slower; keep default clone open and target the
remaining fresh checkout file open/write variance rather than metadata.

macOS preservation after the same code change:

| Operation | Mean ratio | Median ratio | Paired median | Zmin vs Gix median |
| --- | ---: | ---: | ---: | ---: |
| `clone` | `2.194284` | `0.846192` | `0.846192` | `0.355775` |
| `clone-instant` | `0.598572` | `0.667473` | `0.671085` | n/a |
| `clone-instant-git-daemon` | `0.803564` | `0.779869` | `0.759583` | n/a |

The macOS rerun again had one cold local clone mean outlier, but medians and
paired medians preserve the clone rows.

Windows clone 7-repeat follow-up:
A stronger Windows/Git-for-Windows scoped rerun after the metadata syscall skip
replaced the noisy 3-repeat clone judgment with a 7-repeat gate:

- scoped Windows 7-repeat:
  `C:\Users\skron\zmin-bench-20260619T002430Z-94637-out`
- phase-traced default clone diagnostic:
  `C:\Users\skron\zmin-bench-20260619T002656Z-95010-out`
- validation:
  all `HEAD`, `HEAD^{tree}`, and `zmin.worktreeFirst=true` checks were `ok`
  for every repeat. A post-run process probe found no lingering benchmark
  `git`, `git-daemon`, `cargo`, `rustc`, or `zmin` processes.

Seven-repeat ratios:

| Operation | Mean ratio | Median ratio | Paired median | Zmin vs Gix median |
| --- | ---: | ---: | ---: | ---: |
| `clone` | `1.110021` | `1.083673` | `1.333045` | `0.854759` |
| `clone-instant` | `0.522979` | `0.552358` | `0.556192` | n/a |
| `clone-instant-git-daemon` | `0.791166` | `0.904394` | `0.834051` | n/a |

This confirms the scoped Windows `clone-instant` and
`clone-instant-git-daemon` rows remain closed. Default Windows `clone` is now
close to Git and faster than Gitoxide, but aggregate and paired medians are
still behind Git, so default clone remains the active Windows clone gap.

The clone-only trace showed `cli.process=0.352082s` while the external
stopwatch row was `0.478133s`; inside Zmin, the remaining time is dominated by
fresh checkout materialization for 480 entries:

| Phase | Seconds |
| --- | ---: |
| `checkout_fresh.checkout_index` | `0.263257` |
| `materialize_file_open` | `0.143172` |
| `materialize_file_bytes` | `0.074171` |
| `materialize_file_close` | `0.026391` |
| `metadata` | `0.000000` |

The stream path did not write any file in this fixture (`stream_attempts=128`,
`stream_written=0`, `stream_skipped_after_disable=352`), because the remaining
files are small enough to fall below the streaming threshold. The next accepted
default-clone experiment should therefore target Windows fresh-checkout file
open/write variance or object-read-to-write flow, not metadata.

A trial that raised the non-Unix fresh-checkout worker cap from 2 to 4 was
rejected and reverted. Its scoped Windows run
`C:\Users\skron\zmin-bench-20260619T002859Z-95491-out` kept checks ok and
preserved instant rows, but worsened default `clone` mean ratio to `1.455604`
and median ratio to `1.152745` due to a large Zmin outlier. Do not retry a
broad worker-count increase without new evidence that the bottleneck is worker
starvation rather than Windows file creation variance.

A later trial that lowered the non-Unix stream threshold for fresh checkout
small blobs and let `PackedFirstObjectStore` stream loose blobs directly to the
worktree path was also rejected and reverted. Diagnostic run
`C:\Users\skron\zmin-bench-20260619T003755Z-1656-out` kept the clone checks ok
but made default `clone` much slower (`4.297091` mean ratio vs Git and
`1.570534` vs Gix). The trace showed why: the experiment wrote all 480 files
through the stream path (`stream_attempts=480`, `stream_written=480`) and
eliminated `object_read` / `materialize`, but `stream_write` alone took
`0.871846s` and `checkout_fresh.checkout_index` rose to `0.451276s`. Do not
retry small loose direct streaming without a different implementation that
avoids per-file loose-object hash verification and proves a lower `stream_write`
phase than the read-object path.

Windows stream-probe miss cutoff:
The accepted follow-up keeps the existing streaming implementation and 1 MiB
blob threshold, but reduces the non-Unix fresh-checkout stream miss cutoff from
128 to 16. Unix keeps the previous 128-miss cutoff. This targets the wasted
small-blob probe path from the default Windows clone trace without changing
checkout content rules or CRLF/filter behavior.

- phase-traced Windows diagnostic:
  `C:\Users\skron\zmin-bench-20260619T004435Z-7311-out`
- scoped Windows 7-repeat:
  `C:\Users\skron\zmin-bench-20260619T004758Z-7636-out`
- validation:
  `cargo test -p zmin-git-core checkout -- --nocapture` (`10/10`);
  `cargo check -p zmin-cli --bin zmin --profile compat`;
  touched-file rustfmt; Windows clone checks had no non-`ok` rows; the post-run
  process probe found no lingering benchmark `git`, `git-daemon`, `cargo`,
  `rustc`, or `zmin` processes.

Trace movement:

| Metric | Before | After |
| --- | ---: | ---: |
| `stream_attempts` | `128` | `16` |
| `stream_skipped_after_disable` | `352` | `464` |
| `stream_write` | `0.052893s` | `0.006718s` |
| `checkout_fresh.checkout_index` | `0.263257s` | `0.174360s` |
| `metadata` | `0.000000s` | `0.000000s` |

Seven-repeat Windows ratios after the cutoff:

| Operation | Mean ratio | Median ratio | Paired median | Zmin vs Gix median |
| --- | ---: | ---: | ---: | ---: |
| `clone` | `0.640274` | `0.773980` | `0.810366` | `0.549127` |
| `clone-instant` | `0.576865` | `0.505314` | `0.595544` | n/a |
| `clone-instant-git-daemon` | `0.757973` | `0.822713` | `0.821221` | n/a |

This closes the scoped Windows default `clone` gap for this fixture while
preserving the already-green instant clone rows. Broader clone performance still
needs full-gate refreshes and larger/real repository fixtures.

Windows full-gate refresh after clone fixes:
The full Windows/Git-for-Windows 3-repeat gate was rerun after the stream-probe
cutoff:

- full Windows 3-repeat:
  `C:\Users\skron\zmin-bench-20260619T005143Z-12449-out`
- validation:
  `checks.csv` had no non-`ok` rows; the post-run process probe found no
  lingering benchmark `git`, `git-daemon`, `cargo`, `rustc`, or `zmin`
  processes.

The full gate confirms the non-clone rows are green: `add`, `add-dirty`,
`commit`, `commit-dirty`, `status`, `rev-list`, `merge-base`, pulls, local
fetches, and local pushes all favor Zmin against Git on mean, median, and paired
median. `log` remains faster than Git but slower than Gix in this CLI-adjacent
comparison.

Selected full-gate ratios:

| Operation | Mean ratio | Median ratio | Paired median | Zmin vs Gix median |
| --- | ---: | ---: | ---: | ---: |
| `add` | `0.806096` | `0.793515` | `0.820090` | n/a |
| `add-dirty` | `0.594674` | `0.494559` | `0.806009` | n/a |
| `status` | `0.586979` | `0.573781` | `0.596218` | `0.208959` |
| `fetch-batch` | `0.225013` | `0.225632` | `0.310967` | `0.313763` |
| `push-batch` | `0.052701` | `0.060649` | `0.060649` | n/a |
| `clone-instant` | `0.626157` | `0.663972` | `0.834046` | n/a |
| `clone-instant-git-daemon` | `0.591135` | `0.506673` | `0.782324` | n/a |
| `clone-instant-ssh` | `0.565773` | `0.506528` | `0.697860` | n/a |

Default `clone` remains noisy in the broader full gate despite the clean scoped
7-repeat closure. The full-gate row was slower than Git and Gix because repeat
`1/local` hit a Zmin outlier (`git=0.454510s`, `gix=0.675368s`,
`zmin=1.130403s`), while repeats `2/local` and `3/local` were much closer
(`zmin=0.442741s` and `0.414866s`). Full-gate default `clone` ratios were
mean `1.721274`, median `2.487081`, paired median `2.487081`, and Gix median
`1.673759`. Keep the scoped 7-repeat gate as the cleaner default-clone timing
proof for this fixture, but leave broader default clone performance open until
a full-gate rerun is stable without the local-clone outlier and larger/real
repository fixtures are covered.

macOS full-gate refresh after clone fixes:
The macOS 3-repeat full gate was rerun after the Windows-only stream-probe
cutoff to confirm the Unix checkout path was preserved:

- full macOS 3-repeat:
  `/tmp/zmin-macos-current-full-gate-20260619T006`
- validation:
  `checks.tsv` had no non-`ok` rows.

The run keeps clone and local-transport rows green on macOS. The stream-probe
cutoff remains Windows-only, so this serves as a current Unix preservation gate
after the clone-focused Windows changes.

Selected full-gate ratios:

| Operation | Mean ratio | Median ratio | Paired median | Zmin vs Gix median |
| --- | ---: | ---: | ---: | ---: |
| `clone` | `0.810820` | `0.791332` | `0.797377` | `2.203258` |
| `clone-instant` | `0.807407` | `0.796202` | `0.796202` | n/a |
| `clone-instant-git-daemon` | `0.886479` | `0.925952` | `0.891929` | n/a |
| `clone-instant-ssh` | `0.437521` | `0.741846` | `0.701449` | n/a |
| `fetch-incremental` | `0.666965` | `0.675218` | `0.666543` | n/a |
| `fetch-batch` | `0.936461` | `0.962664` | `0.952645` | `2.599574` |
| `push-batch` | `0.811725` | `0.806676` | `0.804909` | n/a |
| `status` | `0.761898` | `0.738272` | `0.738272` | `1.019519` |

The only notable macOS median gap left in this full gate is `commit`
(`1.143562` median ratio, paired median `1.015880`). `status` is still faster
than Git by median but effectively parity with Gix in this comparison. Keep the
macOS commit row as the next local CLI gap; do not reopen the Windows-only
stream cutoff unless new Unix evidence shows a regression.

macOS commit ref-write gap:
A scoped 7-repeat macOS `commit` rerun with phase tracing confirmed the full
gate `commit` gap was mostly loose-ref durability overhead rather than tree or
commit-object construction:

- baseline scoped run:
  `/tmp/zmin-macos-commit-scope-20260619T-next`
- detailed update-ref trace:
  `/tmp/zmin-macos-commit-update-ref-trace-20260619T-next3`
- accepted no-loose-ref-fsync scoped run:
  `/tmp/zmin-macos-commit-no-ref-fsync-20260619T-next4`

Before the change, scoped `commit` was stable slower than Git (`1.161869` mean
ratio, `1.132603` median ratio, `1.159314` paired median). Trace showed
`commit.update_ref` dominated the command (`0.036735s` median), with
`commit.update_ref.write_ref` taking `0.022843s` median. `write_tree` was the
next largest phase at about `0.016813s` median.

The accepted change removes `sync_all()` from the generic loose-ref lock-file
write path while keeping create-new lock semantics and rename replacement.
This matches the benchmark's stock Git behavior more closely for ordinary loose
ref updates; packed-refs writer behavior was left unchanged.

Post-change scoped ratios:

| Operation | Mean ratio | Median ratio | Paired median |
| --- | ---: | ---: | ---: |
| `commit` | `0.671556` | `0.636282` | `0.650646` |

Post-change trace shows `commit.update_ref` down to `0.001850s` median and
`commit.update_ref.write_ref` down to `0.000173s` median. The remaining
largest commit phase is `commit.write_tree` at `0.017007s` median.

Validation:

- `cargo test -p zmin-git-core refs -- --nocapture` (`18/18`)
- `cargo test -p zmin-cli --test git_commit_compat -- --nocapture` (`26/26`)
- `cargo test -p zmin-cli --test git_refs_compat -- --nocapture` (`15/15`)
- `cargo test -p zmin-cli --test git_reflog_compat -- --nocapture` (`11/11`)
- `cargo test -p zmin-cli --test git_ref_resolution_compat -- --nocapture`
  (`6/6`)
- `cargo check -p zmin-cli --bin zmin --profile compat`

The refs validation also exposed and fixed an adjacent parity bug: default
`update-ref HEAD <oid>` treated `HEAD` as a pseudoref before the deref path and
wrote a direct `.git/HEAD`, while stock Git updates the symbolic branch unless
`--no-deref` is used. The fix leaves `HEAD` out of the generic pseudoref branch
for update/ref-log old-id handling, preserving stock behavior for both default
deref and explicit `--no-deref` modes.

macOS full-gate refresh after ref-write fix:
The full macOS 3-repeat gate was rerun after the loose-ref write change:

- full macOS 3-repeat:
  `/tmp/zmin-macos-full-gate-refwrite-20260619T-next`
- validation:
  `checks.tsv` had no non-`ok` rows.

The full gate confirms the scoped `commit` improvement carries through the
broader local matrix. `commit` and `commit-dirty` are now green in the full gate,
and clone/local-transport rows remain green against Git by median.

Selected full-gate ratios:

| Operation | Mean ratio | Median ratio | Paired median | Zmin vs Gix median |
| --- | ---: | ---: | ---: | ---: |
| `add` | `0.734858` | `0.719517` | `0.724928` | n/a |
| `add-dirty` | `0.841213` | `0.817494` | `0.804452` | n/a |
| `commit` | `0.592748` | `0.585314` | `0.591651` | n/a |
| `commit-dirty` | `0.563509` | `0.598578` | `0.600885` | n/a |
| `status` | `0.776831` | `0.783520` | `0.778239` | `0.801335` |
| `clone` | `0.477180` | `0.477040` | `0.519513` | `0.211477` |
| `clone-instant` | `0.586160` | `0.579312` | `0.579312` | n/a |
| `clone-instant-git-daemon` | `0.813067` | `0.743024` | `0.720249` | n/a |
| `clone-instant-ssh` | `0.344639` | `0.544331` | `0.544331` | n/a |
| `fetch-incremental` | `0.460137` | `0.460095` | `0.460095` | `0.155077` |
| `fetch-batch` | `0.528423` | `0.474976` | `0.495487` | `0.186758` |
| `push-batch` | `0.785267` | `0.746822` | `0.771364` | n/a |

`index-pack` had a noisy mean (`1.114553`) from one Zmin outlier but stayed
green by median (`0.986697`) and paired median (`0.960930`). `rev-list` remains
near parity (`0.922157` median, `0.955336` paired median). The next
cross-platform risk for the ref-write change is Windows preservation, especially
commit/ref/reflog behavior and the Windows full-gate rows.

Windows focused commit/ref preservation after ref-write fix:
The focused Windows/Git-for-Windows commit rows were rerun after the loose-ref
write and `update-ref HEAD` deref changes:

- focused Windows 3-repeat:
  `C:\Users\skron\zmin-bench-20260619T013144Z-69118-out`
- rows:
  `bench.csv`
- comparison:
  `comparison.csv`
- validation:
  `checks.csv` had `commit-dirty` tree checks for all three repeats; this
  selected operation set does not emit separate plain `commit` tree rows, so
  the plain `commit` benchmark row is timing/process-exit evidence and parity
  is covered by the focused tests below.
- post-run process probe:
  no `git`, `git-daemon`, `cargo`, `rustc`, or `zmin` processes were returned.

Focused benchmark ratios:

| Operation | Mean ratio | Median ratio | Paired median |
| --- | ---: | ---: | ---: |
| `commit` | `0.024351` | `0.021210` | `0.032325` |
| `commit-dirty` | `0.505536` | `0.543572` | `0.543572` |

The raw rows had Git plain-commit times of `5.445789s`, `3.359833s`, and
`5.981778s`, while Zmin was `0.126872s`, `0.108605s`, and `0.124608s`. That is
a Windows/Git-for-Windows process-timing artifact in stock Git's plain commit
path for this fixture, not a reason to generalize the ratio beyond this gate.
The `commit-dirty` row is the cleaner Windows timing comparison and is still
green.

Focused Windows parity tests were also run from the same copied snapshot with
the GNU Windows toolchain:

- `cargo test -p zmin-cli --test git_refs_compat
  update_ref_no_deref_modes_match_stock_git_head_storage -- --nocapture`
  (`1/1`)
- `cargo test -p zmin-cli --test git_reflog_compat
  commit_records_branch_and_head_reflog -- --nocapture` (`1/1`)
- `cargo test -p zmin-cli --test git_commit_compat
  commit_messages_match_stock_git_object -- --nocapture` (`1/1`)

This preserves the ref-write change on Windows for the affected ref, reflog,
and commit-object behavior. The broader Windows performance work remains the
default `clone` full-gate stability and larger/real repository evidence, not
the commit/ref path.

Windows clone full-gate and larger-fixture follow-up:
The full Windows/Git-for-Windows 3-repeat gate was rerun after the ref-write
fixes:

- full Windows 3-repeat:
  `C:\Users\skron\zmin-bench-20260619T014310Z-74670-out`
- validation:
  `checks.csv` had no non-`ok` rows; the post-run process probe returned no
  `git`, `git-daemon`, `cargo`, `rustc`, or `zmin` processes.

The non-clone rows remained green, and instant clone rows were mostly green or
near parity. Default local `clone` remains open in the broader full gate:

| Operation | Mean ratio | Median ratio | Paired median | Zmin vs Gix median |
| --- | ---: | ---: | ---: | ---: |
| `clone` | `1.302746` | `1.611725` | `1.611725` | `0.986275` |
| `clone-instant` | `0.739518` | `1.035247` | `1.035247` | n/a |
| `clone-instant-git-daemon` | `0.890358` | `0.866720` | `1.179618` | n/a |
| `clone-instant-ssh` | `0.441035` | `0.505479` | `0.589863` | n/a |

Raw default clone rows were Zmin `0.660040s`, `0.351822s`, `0.555803s` vs Git
`0.409524s`, `0.390924s`, `0.402906s`. A focused traced 3-repeat clone run:

- focused trace:
  `C:\Users\skron\zmin-bench-20260619T015029Z-75569-out`

had no failed checks and showed mixed timing (`0.943575` mean ratio,
`1.097100` median ratio, `1.556261` paired median). The trace localized slow
Zmin rows to fresh checkout materialization and object reads, with the slowest
row showing `checkout_fresh.checkout_index=0.448311s`, `object_read=0.140409s`,
`materialize_file_open=0.362468s`, `materialize_file_bytes=0.163204s`, and
`materialize_file_close=0.060619s` for 480 entries. Treat the trace as
diagnostic only because trace mode uses the split writer path for phase
measurement.

A larger clone-only Windows fixture was added as ad hoc evidence by invoking
`tools/windows-native-benchmark.ps1` directly from the copied guest snapshot
with `-Commits 120 -FilesPerCommit 80 -Ops clone` (about 1920 final worktree
files):

- larger clone baseline:
  `C:\Users\skron\zmin-bench-20260619T-large-clone-out`
- larger clone trace:
  `C:\Users\skron\zmin-bench-20260619T-large-clone-trace-out`

The larger 3-repeat fixture passed checks but showed a real open gap:

| Operation | Mean ratio | Median ratio | Paired median | Zmin vs Gix median |
| --- | ---: | ---: | ---: | ---: |
| `clone` | `1.848973` | `1.752087` | `2.313287` | `1.033359` |

Raw larger rows were Zmin `2.548632s`, `2.827041s`, `2.457708s` vs Git
`1.101736s`, `1.613528s`, `1.521348s`. The one-repeat larger trace reproduced
the gap (`2.400961` vs Git, `0.942203` vs Gix) and localized it to checkout:
`checkout_fresh.checkout_index=2.129546s`, `object_read=0.536086s`, and
aggregate worker materialization time `3.568772s` for 1920 entries. The largest
aggregate subphase was file close (`2.456853s`), followed by file open
(`0.823042s`) and file bytes (`0.287522s`); aggregate worker timings can exceed
wall time because checkout is parallel.

A conditional non-Unix 4-worker checkout experiment for larger checkouts was
tested and reverted. It did not improve the larger fixture:

- rejected worker experiment:
  `C:\Users\skron\zmin-bench-20260619T-large-clone-workers4-out`
- ratios:
  mean `2.020522`, median `2.110726`, paired median `2.252501`, and Zmin vs Gix
  median `1.166641`

It also produced an unacceptable small-fixture scoped outlier in
`C:\Users\skron\zmin-bench-20260619T020038Z-76895-out` (`zmin=1.955448s` for
repeat `1/local`). Do not retry broader worker-count increases or conditional
worker caps without new evidence that avoids both the 480-entry regression and
the 1920-entry file close/open bottleneck. The next Windows default clone work
should target checkout materialization/file close/open behavior or object
read-to-write flow with a traced larger fixture, then rerun both the default
scoped gate and the larger clone fixture before any full-gate claim.

A source-store checkout experiment was also tested and reverted. Local clone
already builds a `source_store` for target resolution, so the experiment changed
local clone checkout to read through that existing source object store instead
of creating a fresh destination object store after hardlink/copy. It preserved
local macOS clone compatibility tests, but the Windows scoped default clone gate
was not acceptable:

- rejected source-store checkout experiment:
  `C:\Users\skron\zmin-bench-20260619T021224Z-83938-out`
- ratios:
  mean `2.251246`, median `3.457229`, paired median `3.457229`, and Zmin vs Gix
  median `2.857743`
- raw rows:
  Zmin `1.870876s`, `0.475994s`, `0.502301s` vs Git `0.541149s`,
  `0.360187s`, `0.364261s`

The experiment reproduced the same unacceptable first-row Zmin outlier pattern
without enough later-row improvement to justify keeping it. Do not retry
source-store-vs-destination-store checkout cache reuse as the Windows default
clone fix unless new trace data shows destination object-store initialization,
not materialization/file close/open, is the bottleneck.

A fresh-checkout `create_new` writer experiment was also tested and reverted.
The hypothesis was that fresh checkout can avoid overwrite/truncate semantics
because target regular files should not exist. Local validation stayed green,
but the Windows scoped default clone gate was worse than the accepted baseline:

- rejected fresh writer experiment:
  `C:\Users\skron\zmin-bench-20260619T022026Z-92107-out`
- ratios and summary:
  mean ratio `2.088477` vs Git and `1.493963` vs Gix; Git median
  `0.432925s`, Gix median `0.621198s`, Zmin median `1.550569s`

The experiment was reverted. Do not retry replacing the normal fresh checkout
write path with `create_new`/manual writes unless new trace evidence shows the
existing `fs::write` path is the measured bottleneck rather than Windows file
creation/close variance or object-read-to-write flow.

A non-Unix thread-local pack-file cache experiment was tested and reverted. The
hypothesis was that parallel checkout workers were serializing packed object
reads through the shared cached pack-file mutex. The experiment kept Unix on the
old shared cache and moved non-Unix pack-file reads to a thread-local cached
handle, but Windows scoped default clone was still worse than the accepted
baseline and the branch also introduced a non-Unix unused-field warning:

- rejected thread-local pack cache experiment:
  `C:\Users\skron\zmin-bench-20260619T022935Z-98831-out`
- ratios and summary:
  mean ratio `1.979965` vs Git and `1.209705` vs Gix; Git median
  `0.428126s`, Gix median `0.657105s`, Zmin median `1.613190s`

The experiment was reverted. Do not retry per-thread pack-file cache handles as
the Windows default clone fix without trace data showing the shared pack-file
mutex is the bottleneck and without preserving warning-free non-Unix builds.

Checkout object storage telemetry:
The next accepted diagnostic slice added checkout trace-only object storage
classification. `GitObjectStore::object_storage_hint` reports `loose`,
`packed`, or `unknown`; fresh checkout calls it only when
`ZMIN_CHECKOUT_PHASE_TRACE` is enabled and emits `object_locate`,
`object_read_loose`, `object_read_packed`, `object_read_unknown`, and matching
count metrics. The non-trace checkout path does not call the hint. The
diagnostic was later extended with trace-only materialization size metrics:
`materialized_regular_files`, `materialized_executable_files`,
`materialized_file_bytes`, and `materialized_file_max_bytes`.

Validation:

- `rustfmt --edition 2024 --check` on touched core files
- `cargo test -p zmin-git-core checkout -- --nocapture` (`10/10`)
- `cargo test -p zmin-git-core pack::tests:: -- --nocapture` (`109/109`)
- `cargo check -p zmin-cli --bin zmin --profile compat`
- `cargo test -p zmin-cli --test git_clone_compat clone_ -- --nocapture`
  (`10/10`)
- `git diff --check`
- Windows process probe after traced runs: `process_count=0`
- size-metric follow-up validation:
  `cargo test -p zmin-git-core checkout -- --nocapture` (`10/10`),
  `cargo check -p zmin-cli --bin zmin --profile compat`,
  `cargo test -p zmin-cli --test git_clone_compat clone_ -- --nocapture`
  (`10/10`), and touched-file rustfmt

Windows trace evidence:

- default 480-entry clone trace:
  `C:\Users\skron\zmin-bench-20260619T023952Z-6776-out`
- larger 1920-entry clone trace:
  `C:\Users\skron\zmin-bench-20260619T-large-clone-storage-trace-out`
- default 480-entry size-metric trace:
  `C:\Users\skron\zmin-bench-20260619T030459Z-21829-out`
- larger 1920-entry size-metric trace:
  `C:\Users\skron\zmin-bench-20260619T-large-size-metrics-out`

Both traces passed clone checks. The default fixture had 480 packed reads and no
loose/unknown reads: `object_read_packed=480`, `object_read=0.055091s`, and
`object_locate=0.023424s`. Its dominant checkout work was still materialization:
`materialize_write=0.178536s`, mostly file open (`0.130252s`).

The larger fixture had the same shape at scale: `object_read_packed=1920`,
`object_read=0.333212s`, `object_locate=0.143202s`, and no loose/unknown reads.
The reproduced gap remained dominated by materialization, especially file close:
`materialize_write=3.301539s`, `materialize_file_close=2.181284s`,
`materialize_file_open=0.630518s`, and `materialize_file_bytes=0.489104s`
while external timing was Zmin `2.556621s` vs Git `0.956642s`.

The size-metric follow-up confirmed this is a tiny-file materialization
problem, not a data-throughput problem. The default trace wrote 480 regular
files, `506184` bytes total, and a max file size of `1055` bytes; in-process
`clone.total` was `0.144189s` even though the traced external stopwatch row was
noisy (`1.360853s`). The larger trace wrote 1920 regular files, `2027064` bytes
total, and a max file size of `1056` bytes; `checkout_fresh.checkout_index` was
`2.542155s`, with aggregate worker timing `materialize_file_close=2.681224s`,
`materialize_file_open=1.206953s`, and `materialize_file_bytes=0.332118s`.

Parallel checkout trace completeness follow-up:

- default 480-entry path-prep trace:
  `C:\Users\skron\zmin-bench-20260619T031330Z-28596-out`

The parallel fresh-checkout worker loops now account for target path
construction in `path_prep`, matching the serial trace shape. The Windows
default clone trace passed checks and showed path construction is not the
remaining bottleneck: `path_prep=0.000798s` for 480 entries, while
`materialize_file_open=0.152694s`, `materialize_file_bytes=0.037029s`, and
`materialize_file_close=0.021956s`.

Windows `clone-large` benchmark op:

- first-class `clone-large` validation:
  `C:\Users\skron\zmin-bench-20260619T031928Z-33885-out`
- first-class `clone-large` 3-repeat baseline:
  `C:\Users\skron\zmin-bench-20260619T032455Z-38782-out`
- first-class `clone-large` phase trace:
  `C:\Users\skron\zmin-bench-20260619T033732Z-56852-out`
- default clone refactor smoke:
  `C:\Users\skron\zmin-bench-20260619T-clone-op-refactor-smoke-out`

`tools/windows-native-benchmark.ps1` now has a first-class `clone-large`
operation using a separate larger source fixture (`CloneLargeCommits=120`,
`CloneLargeFilesPerCommit=80` by default). The script now builds benchmark
source repositories through one helper so the default and larger clone fixtures
use the same setup rules. The `clone-large` smoke passed `HEAD` and tree checks
and reproduced the larger-fixture gap without the previous ad hoc direct
PowerShell invocation: Git `0.971349s`, Gix `2.356681s`, Zmin `2.916709s`
(`3.002741` vs Git, `1.237634` vs Gix). The default `clone` operation was
rerun from the same copied snapshot after the fixture-builder refactor and
also passed checks, with Zmin faster in that one-repeat smoke (`0.901862` vs
Git and `0.821960` vs Gix).

The 3-repeat `clone-large` baseline passed all six HEAD/tree checks and had no
lingering benchmark process rows after completion. It is the current
first-class larger-clone comparison point: mean ratio `2.396071`, median ratio
`2.390069`, paired median `3.148522` vs Git; and mean ratio `1.163089`,
median ratio `1.038241`, paired median `1.499950` vs Gix. Raw rows were Git
`0.872730s`, `1.251959s`, `1.248083s`; Gix `2.882055s`, `2.071250s`,
`1.994913s`; and Zmin `2.747810s`, `2.341322s`, `2.992269s`.

The first-class `clone-large` phase trace passed clone checks and confirmed the
same root-cause profile under the standardized op. It wrote 1920 regular files,
`2027064` bytes total, max file `1056` bytes, all from packed objects
(`object_read_packed=1920`). `path_prep` was only `0.004262s`; checkout time
was dominated by Windows file materialization with `materialize_file_close`
`3.646950s`, `materialize_file_open=0.945190s`, and
`materialize_file_bytes=0.174350s`, while `checkout_fresh.checkout_index` was
`2.715903s`. Treat the traced external stopwatch row as diagnostic only.

macOS `clone-large` benchmark op:

- first-class `clone-large` smoke:
  `/tmp/zmin-macos-clone-large-smoke-20260619Tnext`
- first-class `clone-large` 3-repeat baseline:
  `/tmp/zmin-macos-clone-large-3x-20260619Tnext`
- default clone refactor smoke:
  `/tmp/zmin-macos-clone-op-refactor-smoke-20260619Tnext`

`tools/git-performance-bench.sh` now mirrors the Windows benchmark with a
first-class `clone-large` operation and default knobs
`ZMIN_BENCH_CLONE_LARGE_COMMITS=120` and
`ZMIN_BENCH_CLONE_LARGE_FILES_PER_COMMIT=80`. Source fixture creation now uses
one helper for both the default and larger clone fixtures. The macOS
`clone-large` smoke passed `HEAD` and tree checks and showed a larger-fixture
gap vs Git in the one-repeat row: Git `0.308667s`, Zmin `0.647097s`
(`2.096424`), while Zmin stayed faster than Gix `0.802181s` (`0.806672`).
The default `clone` refactor smoke also passed checks and kept Zmin faster in
that one-repeat row (`0.652178` vs Git, `0.217018` vs Gix).

The 3-repeat macOS `clone-large` baseline supersedes the one-repeat smoke for
timing. It passed all six HEAD/tree checks and favored Zmin against both
comparators: mean ratio `0.620689`, median ratio `0.546510`, paired median
`0.643759` vs Git; and mean ratio `0.320103`, median ratio `0.313390`, paired
median `0.313390` vs Gix. Raw rows were Git `0.221128s`, `0.293400s`,
`0.288260s`; Gix `0.567794s`, `0.502686s`, `0.486148s`; and Zmin
`0.155175s`, `0.157537s`, `0.185570s`. This means the larger clone gap is
currently Windows-specific for this fixture.

Windows default gate after adding `clone-large`:

- full default Windows/Git-for-Windows one-repeat gate:
  `C:\Users\skron\zmin-bench-20260619T034410Z-61834-out`

The default Windows benchmark operation set now includes `clone-large`. The
one-repeat full gate passed all validation checks (`28 ok`) and the post-run
process probe found no lingering `git`, `git-daemon`, `cargo`, `rustc`, or
`zmin` processes. This is a tooling/gate validation point, not a performance
closure: most non-clone rows favored Zmin against Git, including `add`
(`0.702141`), `add-dirty` (`0.684878`), `status` (`0.473731`), fetch/push
rows, pull rows, `commit`, and `commit-dirty`; `log` was effectively parity
with Gix (`1.020809`) while faster than Git. The open Windows clone rows
remained visible in the same default gate: default `clone` was `1.906781` vs
Git and `1.314845` vs Gix, and `clone-large` was `2.855882` vs Git and
`1.207647` vs Gix. Keep using focused 3-repeat/trace runs for optimization
decisions, and treat this full one-repeat as proof that the broader gate now
carries the larger clone fixture.

Windows checkout materialization max-latency telemetry:

- traced `clone-large` with file-operation max timings:
  `C:\Users\skron\zmin-bench-20260619T035330Z-67254-out`
- traced default `clone` with file-operation max timings:
  `C:\Users\skron\zmin-bench-20260619T040103Z-68101-out`

Fresh checkout trace now emits max per-file timing for the traced regular-file
open, byte write, and close phases (`materialize_file_open_max`,
`materialize_file_bytes_max`, and `materialize_file_close_max`) in addition to
the existing aggregate phase sums. This is trace-only and does not change the
non-traced checkout path. Validation: focused `zmin-git-core` checkout tests
(`10/10`), `cargo check -p zmin-cli --bin zmin --profile compat`, focused
`git_clone_compat clone_` tests (`10/10`), and both Windows traced clone
checks were `ok` with no lingering benchmark processes.

The larger trace wrote 1920 packed regular files (`2027064` bytes, max file
`1056` bytes). Its in-process `clone.total` was `1.389107s` and
`cli.process=1.412011s`, while the traced external stopwatch row was much
noisier (`3.892600s`); keep treating traced external stopwatch rows as
diagnostic. Within checkout, `checkout_fresh.checkout_index=1.326700s`,
`materialize_file_open=0.721807s` with max `0.044375s`,
`materialize_file_bytes=0.219779s` with max `0.012700s`, and
`materialize_file_close=1.077711s` with max `0.060952s`.

The default 480-entry trace wrote 480 packed regular files (`506184` bytes, max
file `1055` bytes). It had `clone.total=0.410515s`,
`cli.process=0.504815s`, and external stopwatch `0.691106s`. Its checkout
shape is smaller and less close-heavy:
`checkout_fresh.checkout_index=0.373311s`,
`materialize_file_open=0.199644s` with max `0.020486s`,
`materialize_file_bytes=0.044406s` with max `0.002144s`, and
`materialize_file_close=0.023294s` with max `0.001322s`.

The new max metrics show the 1920-entry fixture is not dominated by one single
giant close stall. It has both many small per-file costs and a modest Windows
close/open long tail compared with the 480-entry fixture. The next experiment
should therefore avoid another writer API swap and instead target the schedule
and batching shape of packed blob materialization, while continuing to compare
both the default and larger clone fixtures.

Windows checkout parallel-worker scheduling telemetry:

- traced `clone-large` with worker scheduling metrics:
  `C:\Users\skron\zmin-bench-20260619T040727Z-81008-out`
- traced default `clone` with worker scheduling metrics:
  `C:\Users\skron\zmin-bench-20260619T041249Z-81555-out`

Fresh checkout trace now also emits `parallel_worker_elapsed`,
`parallel_worker_elapsed_max`, `parallel_worker_count`,
`parallel_worker_entries_min`, and `parallel_worker_entries_max` for parallel
fresh checkout. This is trace-only and is intended to distinguish chunk skew
from per-worker file materialization cost. Validation: focused checkout tests
(`10/10`), compat build, focused clone compatibility tests (`10/10`), and both
Windows traced clone checks were `ok` with no lingering benchmark processes.

The larger 1920-entry trace used two equal workers (`960` entries each).
`checkout_fresh.checkout_index=1.603724s`, `parallel_worker_elapsed=2.979145s`,
and `parallel_worker_elapsed_max=1.592949s`, so checkout wall time is almost
exactly the slow worker time, not a scheduling gap after workers join. The
same trace had `materialize_file_open=0.789918s` with max `0.055172s`,
`materialize_file_bytes=0.244469s` with max `0.025761s`, and
`materialize_file_close=1.554152s` with max `0.184151s`; close still has a
larger long tail than the 480-entry fixture, but the chunks are balanced.

The default 480-entry trace also used two equal workers (`240` entries each).
`checkout_fresh.checkout_index=0.155039s`, `parallel_worker_elapsed=0.260296s`,
and `parallel_worker_elapsed_max=0.136489s`. Its file timing stayed much
smaller: `materialize_file_open=0.115267s` with max `0.018105s`,
`materialize_file_bytes=0.025204s` with max `0.000522s`, and
`materialize_file_close=0.023100s` with max `0.004738s`.

This rules out uneven chunk assignment as the Windows larger-clone root cause.
The next checkout experiment should target the per-worker materialization loop
or object-read-to-write sequencing inside each equal chunk, and must keep both
the 480-entry and 1920-entry fixtures in the evidence set.

Windows scoped non-trace clone preservation after checkout telemetry:

- scoped non-trace 3-repeat `clone,clone-large` gate:
  `C:\Users\skron\zmin-bench-20260619T041628Z-86310-out`

The trace-only checkout telemetry did not introduce validation failures in the
normal benchmark path. The scoped Windows run passed all `12` clone checks and
left no lingering benchmark processes. Default `clone` was green in this run:
mean ratio `0.869693`, median ratio `0.668421`, paired mean `0.927449`, and
paired median `1.176243` vs Git; it also favored Gix by aggregate mean and
median (`0.946600` and `0.907374`), with paired median mixed at `1.108087`.
Raw Zmin default clone rows were `0.743321s`, `0.808469s`, and `0.685740s`.

`clone-large` remains the current Windows clone performance gap against stock
Git: mean ratio `2.374197`, median ratio `2.299196`, paired mean `2.385995`,
and paired median `2.619137`. Against Gix it is near parity by aggregate mean
(`0.991462`) and slightly slower by aggregate median (`1.053992`) and paired
median (`1.200877`). Raw larger rows were Git `1.239143s`, `1.046534s`,
`0.954273s`; Gix `2.372460s`, `2.703089s`, `2.682970s`; and Zmin
`2.849033s`, `2.343871s`, `2.499372s`.

Windows checkout per-worker phase attribution:

- traced `clone-large` with per-worker phase max timings:
  `C:\Users\skron\zmin-bench-20260619T042330Z-93039-out`

Fresh checkout trace now emits per-worker max phase sums for object read,
materialization, file open, byte write, and file close. This remains trace-only
and is meant to distinguish object-read-to-write sequencing from the actual
file materialization loop. Validation: focused checkout tests (`10/10`),
compat build, focused clone compatibility tests (`10/10`), and the Windows
traced `clone-large` checks were `2 ok` with no lingering benchmark processes.

The traced larger clone again used two equal 960-entry workers. Its internal
timing was much lower than the traced external stopwatch:
`clone.total=1.148525s`, `cli.process=1.176366s`, and
`checkout_fresh.checkout_index=1.091618s` versus external stopwatch
`3.876761s`. The worker attribution shows the slow worker is dominated by
materialization rather than object reads:
`parallel_worker_elapsed_max=1.082258s`,
`parallel_worker_materialize_max=0.914644s`, and
`parallel_worker_object_read_max=0.087000s`. Inside materialization, the slow
worker's file phases were `parallel_worker_file_close_max=0.571424s`,
`parallel_worker_file_open_max=0.314260s`, and
`parallel_worker_file_bytes_max=0.111057s`.

The aggregate rows agree with that attribution: `object_read=0.172266s` for
all 1920 packed reads, while `materialize_file_close=0.981926s`,
`materialize_file_open=0.599073s`, and `materialize_file_bytes=0.168906s`.
This narrows the next Windows larger-clone optimization to the per-worker
regular-file materialization loop, especially close/open behavior. It argues
against spending the next experiment on packed object-read ordering for this
fixture.

Windows `clone-large` stronger non-trace baseline:

- scoped non-trace 5-repeat `clone-large` gate:
  `C:\Users\skron\zmin-bench-20260619T043028Z-98012-out`

The current dirty-worktree larger clone baseline passed all `10` validation
checks and left no lingering benchmark processes. This is the stronger current
timing point for the active Windows larger-clone gap: Zmin vs Git mean ratio
`1.789324`, median ratio `1.744893`, paired mean `1.829943`, and paired median
`1.671520`. Against Gix, Zmin is closer but still slower on this 5-repeat run:
mean ratio `1.104400`, median ratio `1.073279`, and paired median `1.080916`.

Raw rows were Git `1.095111s`, `1.259703s`, `1.231634s`, `1.213959s`,
`1.566662s`; Gix `2.425740s`, `2.002340s`, `2.156841s`, `1.742672s`,
`1.988194s`; and Zmin `2.777927s`, `2.023586s`, `2.058701s`, `2.383468s`,
`2.149070s`. Use this 5-repeat gate as the current non-trace acceptance
baseline for the next materialization-loop experiment.

Rejected large-checkout serialization experiment:

- default 480-entry preservation check:
  `C:\Users\skron\zmin-bench-20260619T-default-serial-cutoff-3x-out`
- larger 1920-entry trace:
  `C:\Users\skron\zmin-bench-20260619T-large-serial-cutoff-out`
- larger 1920-entry 3-repeat:
  `C:\Users\skron\zmin-bench-20260619T-large-serial-cutoff-3x-out`

The experiment disabled parallel fresh checkout on non-Unix once the entry
count reached 1024, based on the file-close-heavy larger trace. It did reduce
the traced larger aggregate file-close time from `2.181284s` to `0.436327s`
and `checkout_fresh.checkout_index` from `2.129546s` to `1.732048s`, but the
external large clone gate did not improve: mean ratio `1.917402`, median ratio
`1.675712`, paired median `2.962980`, and Zmin vs Gix median `1.305230`.
The 480-entry default fixture was only preserved, not improved (`1.070937`
mean, `1.189643` median, `1.189643` paired median, faster than Gix by mean).
The cutoff was reverted. Do not retry serializing large non-Unix fresh checkout
without new evidence that improves the external large fixture while preserving
the default fixture.

This evidence narrows the next Windows default/larger clone work: do not chase
loose-object checkout paths or shared pack-file mutex behavior for these
fixtures. Focus on packed-blob-to-worktree materialization and Windows file
close/open behavior, with both default and larger fixtures rerun before any
broader clone claim.

Windows phase-only clone checkout trace:

- phase-only larger `clone-large` trace:
  `C:\Users\skron\zmin-bench-20260619T044115Z-3734-out`
- phase-only default `clone` trace:
  `C:\Users\skron\zmin-bench-20260619T044447Z-4224-out`

`tools/windows-native-benchmark.ps1` now accepts
`-SkipCheckoutPhaseTrace`, and `tools/parallels-windows-runner.sh` exposes it
through `ZMIN_WINDOWS_BENCH_PHASE_TRACE_ONLY=1` when
`ZMIN_WINDOWS_BENCH_PHASE_TRACE=1` is set. This keeps the CLI phase log
enabled but does not set `ZMIN_CHECKOUT_PHASE_TRACE`, so fresh checkout uses the
normal non-trace `fs::write` path instead of the split diagnostic writer used
for open/write/close subphase attribution. The default remains unchanged:
normal Windows phase trace still includes checkout subphase rows unless the
phase-only env var is set.

Validation:

- `bash -n tools/parallels-windows-runner.sh`
- `git diff --check -- tools/windows-native-benchmark.ps1 tools/parallels-windows-runner.sh`
- Windows/Git-for-Windows phase-only `clone-large`:
  `ZMIN_WINDOWS_BENCH_PHASE_TRACE=1 ZMIN_WINDOWS_BENCH_PHASE_TRACE_ONLY=1 tools/parallels-windows-runner.sh benchmark 1 clone-large`
- Windows/Git-for-Windows phase-only default `clone`:
  `ZMIN_WINDOWS_BENCH_PHASE_TRACE=1 ZMIN_WINDOWS_BENCH_PHASE_TRACE_ONLY=1 tools/parallels-windows-runner.sh benchmark 1 clone`
- Both runs had `ok` HEAD/tree checks and no lingering `git`, `git-daemon`,
  `cargo`, `rustc`, or `zmin` process rows.

The larger phase-only run had external Zmin `3.665274s` versus Git `1.052089s`
and Gix `2.516289s`, with in-process `clone.total=2.659736s`,
`cli.process=2.791816s`, and `checkout_fresh.checkout_index=2.580531s`.
The default phase-only run was green externally (Zmin `0.336018s` versus Git
`0.513372s` and Gix `1.011193s`), with `clone.total=0.238613s`,
`cli.process=0.260839s`, and `checkout_fresh.checkout_index=0.133663s`.

This corrects the interpretation of the split checkout traces: the open/bytes/
close subphase rows are still useful for directional attribution, but they are
not exact acceptance timing for the runtime writer path because enabling
`ZMIN_CHECKOUT_PHASE_TRACE` changes fresh regular-file writes from `fs::write`
to a split `File::create`/`write_all`/drop sequence. The next larger-clone
experiment should be judged against phase-only or non-trace runs, and should
target the real `fs::write` materialization path at 1920 tiny files while
preserving the green 480-entry default fixture.

Direct-write checkout trace correction:

- direct-write checkout trace:
  `C:\Users\skron\zmin-bench-20260619T044943Z-10783-out`
- non-trace sanity run:
  `C:\Users\skron\zmin-bench-20260619T045602Z-11433-out`

Fresh checkout tracing now keeps the normal `fs::write` regular-file path even
when `ZMIN_CHECKOUT_PHASE_TRACE=1`. The old split
`File::create`/`write_all`/drop measurement was removed from the traced writer
path, and checkout trace now emits direct write timing through
`materialize_file_write_direct`, `materialize_file_write_direct_max`, and
`parallel_worker_file_write_direct_max`. The previous open/bytes/close rows are
left in the trace schema but are zero for regular files on this direct path.

Validation:

- `rustfmt --edition 2024 --check crates/zmin-git-core/src/checkout.rs`
- `cargo test -p zmin-git-core checkout -- --nocapture` (`10/10`)
- `cargo check -p zmin-cli --bin zmin --profile compat`
- `cargo test -p zmin-cli --test git_clone_compat clone_ -- --nocapture`
  (`10/10`)
- `git diff --check`
- Windows/Git-for-Windows direct-write trace:
  `ZMIN_WINDOWS_BENCH_PHASE_TRACE=1 tools/parallels-windows-runner.sh benchmark 1 clone,clone-large`
- Windows/Git-for-Windows non-trace sanity:
  `tools/parallels-windows-runner.sh benchmark 1 clone,clone-large`
- Both Windows runs had `ok` HEAD/tree checks and no lingering `git`,
  `git-daemon`, `cargo`, `rustc`, or `zmin` process rows.

The direct-write trace confirms the large-fixture checkout gap is on the actual
`fs::write` materialization path, not an artifact of the old split writer. The
480-entry default trace had `checkout_fresh.checkout_index=0.186804s`,
`materialize_file_write_direct=0.226393s`, direct-write max `0.004781s`,
`parallel_worker_file_write_direct_max=0.126995s`, and 480 packed regular
files totalling `506184` bytes. The 1920-entry larger trace had
`checkout_fresh.checkout_index=3.077314s`,
`materialize_file_write_direct=4.413223s`, direct-write max `0.146295s`,
`parallel_worker_file_write_direct_max=2.552462s`, and 1920 packed regular
files totalling `2027064` bytes.

The same run had noisy external stopwatch rows, especially default `clone`
(external Zmin `2.806436s` while in-process `clone.total=0.237501s`), so the
direct-write run is diagnostic rather than a clean performance gate. The
non-trace sanity run preserved correctness and showed `clone-large` near Gix
(Zmin `3.165417s`, Git `1.122614s`, Gix `3.044979s`), but default `clone`
again had an external outlier (`1.258814s` vs Git `0.503080s`). Treat the next
runtime experiment as targeting the 1920-entry `fs::write` materialization
shape while guarding against default-fixture regressions with non-trace
multi-repeat evidence.

Windows phase summary CSV:

- parser smoke output:
  `C:\Users\skron\zmin-bench-20260619T050344Z-16774-out`

When `-ZminPhaseTraceDir` is enabled, `tools/windows-native-benchmark.ps1` now
parses Zmin phase logs client-side and writes `phase_summary.csv` beside
`bench.csv`, `checks.csv`, `summary.csv`, and `comparison.csv`. The CSV has one
row per parsed `zmin-phase`, `zmin-checkout-phase`, and
`zmin-checkout-metric` record with normalized columns for trace file, row kind,
label, phase, metric, seconds, value, and entries. The Parallels runner prints
the path as `phase_summary=...` when the file is produced.

Validation:

- `git diff --check -- tools/windows-native-benchmark.ps1`
- `bash -n tools/parallels-windows-runner.sh`
- Windows/Git-for-Windows parser smoke:
  `ZMIN_WINDOWS_BENCH_PHASE_TRACE=1 tools/parallels-windows-runner.sh benchmark 1 clone`
- Smoke checks were `ok`; post-run process probe had no lingering benchmark
  processes.

The smoke proved the summary includes the new direct-write checkout rows without
manual log scraping: `checkout_fresh.checkout_index=0.222962s` and
`materialize_file_write_direct=0.215288s` for the 480-entry default clone trace.
Use `phase_summary.csv` as the first evidence artifact for future Windows phase
runs before reading raw trace logs.

Windows `clone-large` phase-summary baseline:

- phase-summary 3-repeat output:
  `C:\Users\skron\zmin-bench-20260619T050653Z-21519-out`

The first multi-run `phase_summary.csv` pass on `clone-large` kept the
correctness checks green (`6 ok`) and left no lingering benchmark processes.
External timing remains open against stock Git and close-but-slower against
Gix: Zmin/Git mean `1.920558`, median `1.999008`, paired median `2.372220`;
Zmin/Gix mean `1.123697`, median `1.140302`, paired median `1.227454`.

Parsed checkout evidence from `phase_summary.csv`:

| Repeat | `checkout_index` | `object_read` | `direct_write` | slow worker direct write |
| --- | ---: | ---: | ---: | ---: |
| 1 | `2.486245s` | `0.304164s` | `3.902865s` | `2.124110s` |
| 2 | `1.760553s` | `0.262276s` | `2.536619s` | `1.355050s` |
| 3 | `1.990851s` | `0.187964s` | `3.427560s` | `1.802575s` |

Every repeat materialized 1920 regular files, `2027064` total bytes, max file
`1056` bytes, with two equal checkout workers. This reinforces the current
runtime target: the Windows larger-clone gap is stable in the tiny-file
`fs::write` materialization path, while packed object read remains a smaller
secondary cost. The next code experiment should reduce this direct-write
materialization shape and must be accepted only with both larger `clone-large`
and default 480-entry clone evidence.

Rejected striped-worker checkout experiment:

- rejected striped run:
  `C:\Users\skron\zmin-bench-20260619T051429Z-28328-out`

A non-Unix experiment assigned large fresh-checkout entries round-robin across
the existing two workers instead of using two contiguous path-sorted chunks.
The hypothesis was that the direct-write slow-worker tail in `phase_summary.csv`
could be reduced without changing the writer API or worker count. Local
validation stayed green (`zmin-git-core checkout` `10/10`, compat build, and
`git_clone_compat clone_` `10/10`), and the Windows run kept all `6` clone
checks `ok`.

The internal phase rows improved, but the external acceptance gate got worse:
Zmin/Git mean `3.024027`, median `3.929956`, paired median `4.255828`; Zmin/Gix
mean `1.216979`, median `1.682570`, paired median `1.690022`. Parsed checkout
rows showed lower internal checkout times (`checkout_index` `1.240050s`,
`1.421786s`, `1.656919s`; direct write `1.852338s`, `2.072916s`, `2.468782s`),
but those did not translate to end-to-end clone performance. The experiment was
reverted. Do not retry striped/non-contiguous worker assignment unless new
evidence explains the external stopwatch regression and preserves both
`clone-large` and the 480-entry default clone.

Rejected batched read-then-write checkout experiment:

- rejected batched run:
  `C:\Users\skron\zmin-bench-20260619T070335Z-79027-out`

A non-Unix large fresh-checkout experiment changed the existing two-worker path
to read regular blobs into bounded per-worker batches and then materialize each
batch. The hypothesis was that grouping object reads ahead of direct writes
would reduce the 1920-file Windows materialization tail without changing the
public checkout behavior.

Local validation stayed green:

- `cargo check -p zmin-cli --bin zmin --profile compat`
- `cargo test -p zmin-git-core checkout -- --nocapture` (`10/10`)
- `cargo test -p zmin-cli --test git_clone_compat clone_ -- --nocapture`
  (`10/10`)

The Windows traced `clone-large` run regressed externally: Git `1.186045s`, Gix
`2.743261s`, Zmin `4.012232s`; Zmin/Git ratio `3.382867`, Zmin/Gix ratio
`1.462578`. Parsed phase rows still put the cost in direct materialization:
`object_read=0.136088s`, `materialize_file_write_direct=2.234422s`,
`parallel_worker_file_write_direct_max=1.195622s`, 1920 regular files,
`2027064` bytes, and two workers. The experiment was reverted. Do not retry
read-all-then-write batching for large Windows checkout unless new evidence
explains why it improves external stopwatch timing while preserving both
default `clone` and `clone-large` gates.

Windows balanced clone benchmark ordering:

- fixed-order baseline before the gate fix:
  `C:\Users\skron\zmin-bench-20260619T071508Z-87737-out`
- first helper validation attempt:
  `C:\Users\skron\zmin-bench-20260619T072514Z-90750`
- balanced-order validation:
  `C:\Users\skron\zmin-bench-20260619T072857Z-91389-out`

The Windows benchmark previously measured local `clone` and `clone-large` in a
fixed Git, Gitoxide, Zmin order for every repeat, unlike the macOS benchmark's
balanced/randomized operation ordering. This made clone rows more sensitive to
warm/cold filesystem and VM state, and it always placed Zmin last in the
three-way comparison. The benchmark now builds per-repeat measurement specs and
rotates the tool order: repeat 1 starts with Git, repeat 2 starts with Gitoxide,
and repeat 3 starts with Zmin when all three tools are present.

The first helper version failed in PowerShell with `Argument types do not
match` because it returned the generic list object instead of an array. The
helper now returns `ToArray()`, and the balanced scoped gate completed.

Validation:

- `git diff --check -- tools/windows-native-benchmark.ps1`
- `tools/parallels-windows-runner.sh benchmark 3 clone,clone-large`
- `checks.csv` had all `12` clone/tree checks `ok`
- post-run process probe returned no `git`, `gix`, `cargo`, `rustc`, or `zmin`
  processes

The balanced `bench.csv` confirmed the intended order:

| Repeat | `clone` order | `clone-large` order |
| --- | --- | --- |
| 1 | Git, Gix, Zmin | Git, Gix, Zmin |
| 2 | Gix, Zmin, Git | Gix, Zmin, Git |
| 3 | Zmin, Git, Gix | Zmin, Git, Gix |

Balanced scoped ratios:

| Operation | Mean ratio | Median ratio | Paired median | Zmin vs Gix mean | Zmin vs Gix median | Zmin vs Gix paired median |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `clone` | `0.716940` | `0.893475` | `1.394072` | `0.673909` | `0.662654` | `1.376148` |
| `clone-large` | `1.778496` | `2.028921` | `4.558178` | `0.641560` | `0.489970` | `1.658765` |

Treat the scoped default `clone` aggregate mean/median as green in this balanced
run but still noisy by paired median. Treat `clone-large` as still open against
stock Git, with better aggregate Gitoxide comparison but mixed paired evidence
because the third Gitoxide row was an outlier (`8.512475s`). Future Windows
clone work should use balanced-order artifacts when judging Git/Gitoxide
comparisons.

Windows balanced `clone-large` 5-repeat refresh:

- balanced 5-repeat output:
  `C:\Users\skron\zmin-bench-20260619T074456Z-7419-out`

The stronger balanced `clone-large` run narrows the current interpretation of
the Windows large-clone gap. Correctness remained green (`10` HEAD/tree checks
`ok`) and the post-run process probe returned no `git`, `gix`, `cargo`,
`rustc`, or `zmin` processes.

Summary ratios:

| Operation | Mean ratio | Median ratio | Paired median | Zmin vs Gix mean | Zmin vs Gix median | Zmin vs Gix paired median |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `clone-large` | `1.097463` | `1.138710` | `0.936113` | `0.996572` | `1.126623` | `0.850111` |

Raw balanced rows:

| Repeat | Git | Gix | Zmin |
| --- | ---: | ---: | ---: |
| 1 | `1.025198s` | `2.967985s` | `2.523116s` |
| 2 | `2.997299s` | `2.411252s` | `1.808740s` |
| 3 | `2.134825s` | `2.157729s` | `1.604448s` |
| 4 | `0.991854s` | `1.913653s` | `2.430947s` |
| 5 | `3.230858s` | `1.980261s` | `3.024447s` |

Treat Windows `clone-large` as near-parity/mixed on this fixture under balanced
ordering, not as the earlier fixed-order 2x+ regression. It is still not a
clean closure against stock Git because aggregate mean and median remain above
`1.00`, but the paired median favors Zmin. Against Gitoxide, aggregate mean is
at parity and paired median favors Zmin, while aggregate median is slower.
Future runtime work should target remaining fresh-checkout materialization only
when new balanced evidence shows a stable external gap, not from fixed-order or
trace-only artifacts.

Windows balanced `clone` plus `clone-large` 5-repeat refresh:

- balanced combined 5-repeat output:
  `C:\Users\skron\zmin-bench-20260619T081351Z-47593-out`

The combined balanced run rebuilt the current release binary and measured
`clone` and `clone-large` together after guest cleanup. Correctness stayed
green with all `20` HEAD/tree checks `ok`. Post-run cleanup reported no active
benchmark scheduled tasks or processes.

Summary ratios:

| Operation | Mean ratio | Median ratio | Paired median | Zmin vs Gix mean | Zmin vs Gix median | Zmin vs Gix paired median |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `clone` | `1.179949` | `0.804009` | `0.770573` | `1.239306` | `0.985925` | `0.823274` |
| `clone-large` | `1.522958` | `1.692979` | `2.266864` | `1.064369` | `1.545188` | `0.966347` |

Treat default `clone` as green by aggregate and paired medians in the combined
run, with noisy mean still affected by outliers. Treat `clone-large` as
unclosed against stock Git in this combined run and mixed against Gitoxide. The
latest evidence is not stable enough to claim large Windows clone closure; keep
the next work focused on balanced evidence before making another materialization
experiment.
