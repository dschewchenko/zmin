# Upstream Git Compatibility Baseline

Date: 2026-06-18

This document tracks compatibility against selected upstream Git test-suite files.
It is intentionally stricter than the local command inventory and smoke tests:
command presence is not counted as behavior parity.

## Local runners

macOS:

```bash
ZMIN_BIN=target/debug/zmin \
ZMIN_UPSTREAM_ALLOW_FAILURES=1 \
tools/git-upstream-compat-suite.sh standard
```

Windows / Git for Windows through Parallels:

```bash
tools/parallels-windows-runner.sh upstream standard
```

For long or Windows/MSYS-sensitive focused chunks, prefer the detached runner
path so the Parallels host session cannot be the source of truth:

```bash
ZMIN_PARALLELS_UPSTREAM_DETACH=1 \
tools/parallels-windows-runner.sh upstream exhaustive

tools/parallels-windows-runner.sh upstream-poll \
  'C:\Users\skron\<zmin-upstream-...-out>'
```

After a fresh `zmin.exe` has already been built in the shared guest target,
prefer the fast rerun path for iterative upstream parity work:

```bash
tools/parallels-windows-runner.sh upstream-fast exhaustive
tools/parallels-windows-runner.sh upstream-poll \
  'C:\Users\skron\<zmin-upstream-...-out>'
```

`upstream-fast` is zmin-only tooling. It reuses
`C:\Users\skron\zmin-target\release\zmin.exe`, skips the Windows native
preflight, and detaches the upstream task for polling. Use the stricter
`upstream` command when a fresh binary/preflight proof is required.

For behavior-only upstream iteration, the local harness can build the faster
Cargo compatibility profile instead of the production release profile:

```bash
ZMIN_UPSTREAM_CARGO_PROFILE=compat tools/git-upstream-compat-suite.sh exhaustive
```

The Parallels runner also has a zmin-only compat path:

```bash
tools/parallels-windows-runner.sh upstream-compat exhaustive
```

`compat` keeps the release profile untouched for performance gates. On macOS
the first cold `cargo build -p zmin-cli --bin zmin --profile compat --timings`
completed in `2m53s`, while a no-op rebuild completed in `2.82s`; a focused
upstream smoke with `ZMIN_UPSTREAM_CARGO_PROFILE=compat`,
`tools/git-upstream-compat-tests-basic.txt`, and `--run=1 -q` passed `1/1`.
On Windows/Git-for-Windows, the runner now moves detached compat builds into
the Scheduled Task and sets `CARGO_BUILD_JOBS=2`, but cold `zmin-cli` compile
still disappeared without sentinel before producing
`C:\Users\skron\zmin-target\compat\zmin.exe`. Treat that as build runner /
crate-boundary work, not behavior parity evidence.

The first crate-boundary cleanup makes `zmin-cli` a library-backed package:
`src/lib.rs` owns the CLI module graph and both `zmin` and `git-http-backend`
are thin binary wrappers around `zmin_cli::run_cli()`. This removes the
`include!("../main.rs")` duplication in the helper binary and creates the
boundary needed for later command-domain splits. It does not by itself reduce
the cold compile cost of the large `zmin-cli` library crate: macOS `compat`
`--bins` cold rebuild after the boundary change was `2m50s`, while no-op
`compat --bins` was `0.27s`. A Parallels Windows `build-release` after this
change eventually produced `C:\Users\skron\zmin-target\release\zmin.exe`
(`7,355,904` bytes, `2026-06-18 21:47:36`), but the foreground build took
roughly 9-10 minutes, so the Windows build-loop problem remains open.

The next build-loop slice split the Clap CLI schema into a dedicated
`zmin-cli-schema` crate and left `crates/zmin-cli/src/cli/schema.rs` as an
internal re-export. This keeps Git behavior unchanged while giving Cargo a
stable crate boundary for the large derive/schema surface. The post-split
macOS `cargo build -p zmin-cli --bins --profile compat --timings` run completed
in `1m24s` after the prior compat check, with timing report rows showing
`zmin-cli-schema` at `83.2s`, the main `zmin-cli` library at `35.9s`, and each
thin bin at `0.7s`; an immediate no-op compat `--bins` rebuild completed in
`0.59s`. Focused validation passed for the v2.47 compatibility acceptance gate,
the `git show` root-commit regression test, `cargo test -p zmin-cli --lib
--no-run`, direct `rustfmt --edition 2024 --check` on the touched schema/compat
files, `git diff --check` on touched files, and `target/compat/zmin version`.
This confirms crate boundaries are the right build-speed direction, but the
next compile-time reduction still needs command/runtime domain crates rather
than another worker-count or global profile tweak.

The following build-architecture cleanup removed the broad hidden
`runtime.rs -> crate::cli::commands::*` re-export. Thin command dispatch files
now route to sibling `super::*_commands` modules directly, commit-message
whitespace helpers moved into runtime-owned `commit_meta`, and `PatchIdMode`
now lives beside the runtime patch-id algorithm instead of in
`reference_impl`. This did not change Git behavior, but it makes the remaining
runtime-to-command coupling explicit: only `runtime/submodule.rs` and
`runtime/primitive_adapters.rs` still import `transport_commands`. A post-change
macOS `cargo build -p zmin-cli --bins --profile compat --timings` rebuild took
`45.31s` with the main `zmin-cli` unit at `44.0s`, and the immediate no-op
compat rebuild took `0.20s`. Validation passed for `cargo check -p zmin-cli
--bin zmin --profile compat`, focused patch-id parity, commit cleanup parity,
two submodule clone/update parity tests, scoped rustfmt on touched dispatch and
runtime files, and scoped `git diff --check`. The next build-speed slice should
extract a transport clone service/options boundary so submodule and primitive
adapters no longer depend on the command implementation crate.

That transport service boundary is now in place. Runtime owns a small
`clone_service` registry for clone, upload-pack request, and receive-pack
request services; CLI startup and command dispatch register the concrete
transport implementations, while runtime callers use the registered service
functions and fail hard if registration is missing. This removes the remaining
runtime-to-transport-command imports from `runtime/submodule.rs` and
`runtime/primitive_adapters.rs` without adding fallback behavior. Direct unit
tests register the services explicitly. Post-change macOS validation passed
for `cargo check -p zmin-cli --bin zmin --profile compat`, focused
upload-pack/receive-pack primitive adapter tests, focused primitive runtime
transport mode-stability tests, the two submodule parity tests, patch-id
parity, commit cleanup parity, scoped rustfmt with `skip_children=true`, and
scoped `git diff --check`. A post-change `cargo build -p zmin-cli --bins
--profile compat --timings` rebuild completed in `19.85s`, with an immediate
no-op compat rebuild at `0.15s`. The next build-speed slice should extract
transport/runtime domains into crates instead of changing global Cargo worker
counts.

## Current measured baseline

Last full selected macOS `standard` run:
`/var/folders/l3/y2d_2zz51z731b86_sstzz0h0000gn/T/zmin-upstream-compat.wIhuNm/summary.tsv`

Last full selected Windows/Git-for-Windows `standard` run:
`C:\Users\zmin\zmin-upstream-20260615T043109Z-18392-out\summary.tsv`

Latest expanded macOS `exhaustive` supported-surface run with unsupported
reftable assertions skipped:
`/var/folders/l3/y2d_2zz51z731b86_sstzz0h0000gn/T//zmin-upstream-compat.SUcvtW/summary.tsv`

Latest expanded Windows/Git-for-Windows `exhaustive` supported-surface run with
unsupported reftable assertions skipped:
`C:\Users\skron\zmin-upstream-20260618T210215Z-22587-out\summary.tsv`

Latest targeted `t1500-rev-parse.sh` macOS run:
`/var/folders/l3/y2d_2zz51z731b86_sstzz0h0000gn/T/zmin-upstream-compat.rikbTw/summary.tsv`

Latest targeted `t1500-rev-parse.sh` Windows/Git-for-Windows run:
`C:\Users\zmin\zmin-upstream-20260614T162938Z-65298-out\summary.tsv`

Latest targeted `t2000-conflict-when-checking-files-out.sh` macOS run:
`/var/folders/l3/y2d_2zz51z731b86_sstzz0h0000gn/T/zmin-upstream-compat.rEistl/summary.tsv`

Latest targeted `t2000-conflict-when-checking-files-out.sh`
Windows/Git-for-Windows run:
`C:\Users\zmin\zmin-upstream-20260614T165421Z-77841-out\summary.tsv`

Latest targeted `t3700-add.sh` macOS run:
`/var/folders/l3/y2d_2zz51z731b86_sstzz0h0000gn/T/zmin-upstream-compat.tHEW6w/summary.tsv`

Latest targeted `t3700-add.sh` Windows/Git-for-Windows run:
`C:\Users\zmin\zmin-upstream-20260614T193027Z-12419-out\summary.tsv`

Latest targeted `t1006-cat-file.sh` macOS run:
`/var/folders/l3/y2d_2zz51z731b86_sstzz0h0000gn/T//zmin-upstream-compat.jlbZGN/summary.tsv`

Latest targeted `t1006-cat-file.sh` Windows/Git-for-Windows run:
`C:\Users\zmin\zmin-upstream-20260614T212550Z-28219-out\summary.tsv`

Latest targeted `t3200-branch.sh` macOS run:
`/var/folders/l3/y2d_2zz51z731b86_sstzz0h0000gn/T//zmin-upstream-compat.WciM4a/summary.tsv`

Latest targeted `t3200-branch.sh` Windows/Git-for-Windows run:
`C:\Users\zmin\zmin-upstream-20260615T003906Z-30087-out\summary.tsv`

Latest targeted `t3903-stash.sh` macOS run:
`/var/folders/l3/y2d_2zz51z731b86_sstzz0h0000gn/T/zmin-upstream-compat.XW3xzR/summary.tsv`

Latest targeted `t3903-stash.sh` Windows/Git-for-Windows run:
`C:\Users\zmin\zmin-upstream-20260615T042229Z-8798-out\summary.tsv`

Latest targeted `t2020-checkout-detach.sh` macOS run:
`/var/folders/l3/y2d_2zz51z731b86_sstzz0h0000gn/T/zmin-upstream-compat.LTwocn/summary.tsv`

Latest targeted `t4013-diff-various.sh` macOS run:
`/tmp/zmin-macos-t4013-afterfix-20260618T183410Z/summary.tsv`

Latest targeted Windows/Git-for-Windows CLI regression for the `t4013`
empty-root `git show` fix:
`ZMIN_WINDOWS_VALIDATE_NO_FMT=1 tools/parallels-windows-runner.sh validate targeted git_history_query_compat show_empty_root_commit_does_not_print_empty_patch_separator`
passed in guest copy `C:\Users\skron\zmin-20260618T191006Z-64719`
with `1 passed; 0 failed`.

Latest targeted `t5510-fetch.sh` macOS supported-surface run with unsupported
reftable assertions skipped:
`/var/folders/l3/y2d_2zz51z731b86_sstzz0h0000gn/T//zmin-upstream-compat.aJDOaU/summary.tsv`

Latest targeted `t1410-reflog.sh` macOS run:
`/var/folders/l3/y2d_2zz51z731b86_sstzz0h0000gn/T//zmin-upstream-compat.Jz5EIT/summary.tsv`

Latest targeted `t0000-basic.sh` macOS run:
`/var/folders/l3/y2d_2zz51z731b86_sstzz0h0000gn/T//zmin-upstream-compat.otzjP9/summary.tsv`

Latest targeted `t0021-conversion.sh` macOS run:
`/var/folders/l3/y2d_2zz51z731b86_sstzz0h0000gn/T//zmin-upstream-compat.Z0GjZS/summary.tsv`

Latest targeted `t0027-auto-crlf.sh` macOS run:
`/var/folders/l3/y2d_2zz51z731b86_sstzz0h0000gn/T//zmin-upstream-compat.QV7Slc/summary.tsv`

| Mode | Test | Status | Upstream scope | Result |
| --- | --- | --- | --- | --- |
| quick | `t0001-init.sh` | pass | repository creation, gitdir layout, init options | `102/102` |
| quick | `t0002-gitfile.sh` | pass | gitfile discovery and redirected gitdir behavior | `14/14` |
| quick | `t0008-ignores.sh` | pass | ignore matching and status-adjacent path behavior | `398/398` |
| standard | `t1006-cat-file.sh` | pass | object inspection and batch plumbing | macOS `290/290`; Windows/Git-for-Windows `290/290` |
| standard | `t1500-rev-parse.sh` | pass | revision/path parsing plumbing | `81/81` |
| standard | `t2000-conflict-when-checking-files-out.sh` | pass | checkout/index path collision behavior | `14/14` |
| standard | `t3200-branch.sh` | pass | branch porcelain compatibility | macOS `167/167`; Windows/Git-for-Windows `167/167` |
| standard | `t3700-add.sh` | pass | index mutation and add porcelain compatibility | macOS `58/58`; Windows/Git-for-Windows `58/58` |
| standard | `t3903-stash.sh` | pass | stash porcelain compatibility | macOS and Windows/Git-for-Windows selected suite pass; upstream records its own known breakage assertions as expected xfail |

File-level score for `standard`: `9/9` selected test files pass on macOS and
`9/9` selected test files pass on Windows/Git-for-Windows.

Known passing upstream assertions counted from this selected set on macOS: at
least `1264/1264` non-xfail assertions in the selected standard set. This count
is only a burn-down signal for the selected upstream files; it must not be used
as a claim of full Git parity.

Current expanded `exhaustive` supported-surface burn-down: `16/16` selected
files pass on macOS and Windows/Git-for-Windows with
`ZMIN_UPSTREAM_SKIP_UNSUPPORTED_REFTABLE=1`. Latest Windows/Git-for-Windows
evidence is
`C:\Users\skron\zmin-upstream-20260618T210215Z-22587-out\summary.tsv`: the
detached `upstream-fast exhaustive` run wrote `upstream-runner.exit=0`,
`total=16`, `passed=16`, `failed=0`, and post-run cleanup showed `tasks=0`,
`procs=0`.

Earlier 2026-06-18 Windows integrated replay retries are retained only as
runner-history evidence. Detached `upstream-fast exhaustive` runs at
`C:\Users\skron\zmin-upstream-20260618T191751Z-77416-out`,
`C:\Users\skron\zmin-upstream-20260618T192030Z-78138-out`, and
`C:\Users\skron\zmin-upstream-20260618T194759Z-8937-out` stopped without an
`upstream-runner.exit` sentinel before the later clean signoff above. The
matching stock-Git quick control at
`C:\Users\skron\zmin-upstream-20260618T195534Z-22444-out` also lost its
scheduled task before a sentinel. Treat those older artifacts as
Git-for-Windows/MSYS lifecycle noise, not Zmin assertion failures. The runner
now refuses to start a new upstream run when another `ZminUpstream-*` task or
`zmin-upstream-*` process is active; validation covered a dummy
`ZminUpstream-guard-probe` refusal and a real Windows smoke at
`C:\Users\skron\zmin-upstream-20260618T193333Z-95929-out` (`1/1`).

Focused `t0008-ignores.sh` follow-up: `tools/git-upstream-compat-tests-ignores.txt`
now selects only that file for standalone reruns. The macOS Zmin standalone run
passed `1/1` at
`/var/folders/l3/y2d_2zz51z731b86_sstzz0h0000gn/T//zmin-upstream-compat.JrrUMq/summary.tsv`.
The Windows release binary was rebuilt at
`C:\Users\skron\zmin-build-20260618T200521Z-40198-out` and produced
`C:\Users\skron\zmin-target\release\zmin.exe` (`7,361,536` bytes,
LastWriteTime `2026-06-18 22:15:57`). A first detached Zmin standalone run at
`C:\Users\skron\zmin-upstream-20260618T200347Z-34454-out` disappeared during
guest-side release build before any test evidence, because the shared release
binary was absent. After the explicit build, detached Zmin standalone
`t0008` at `C:\Users\skron\zmin-upstream-20260618T201604Z-58224-out` and
detached stock-Git standalone `t0008` at
`C:\Users\skron\zmin-upstream-20260618T201750Z-59901-out` both started the
test, then lost the scheduled task before `upstream-runner.exit`; each artifact
has only the `summary.tsv` header and a zero-byte `t0008-ignores.log`. This is
not green parity evidence, but the matching stock-Git standalone control kept
those older artifacts classified as Windows/Git-for-Windows/MSYS harness
lifecycle instability rather than a Zmin behavior failure. `upstream-poll` now
prints artifact inventory, summary tail, and zero-byte log names for this
missing-task/no-sentinel case.

Bounded `t0008` follow-up: the Parallels cleanup pattern no longer removes the
shared `C:\Users\skron\zmin-target` cache, because deleting it made
`upstream-fast` rebuild inside the Scheduled Task. A fresh `build-release`
produced `C:\Users\skron\zmin-target\release\zmin.exe` (`7,379,456` bytes,
LastWriteTime `2026-06-18 22:32:21`) at
`C:\Users\skron\zmin-build-20260618T202339Z-69008-out`; a dry-run regex probe
confirmed `zmin-target` no longer matches the cleanup deletion pattern. With
that binary reused, detached Windows Zmin bounded `t0008-ignores.sh` passed
`--run=1-50 -q` at
`C:\Users\skron\zmin-upstream-20260618T203227Z-78826-out`, `--run=1-100 -q`
at `C:\Users\skron\zmin-upstream-20260618T203336Z-79428-out`,
`--run=1-200 -q` at
`C:\Users\skron\zmin-upstream-20260618T203535Z-80785-out`, and
`--run=1-300 -q` at
`C:\Users\skron\zmin-upstream-20260618T203756Z-82113-out`; each wrote
`upstream-runner.exit=0`, `passed=1`, and a `summary.tsv` pass row. Wider
bounded attempts `--run=1-398 -q` at
`C:\Users\skron\zmin-upstream-20260618T204127Z-83865-out` and
`--run=1-350 -q` at
`C:\Users\skron\zmin-upstream-20260618T204428Z-86913-out` are inconclusive:
both lost the scheduled task before `upstream-runner.exit`, with header-only
summary and zero-byte `t0008-ignores.log`. Follow-up boundary probes also keep
the accepted frontier at `1-300`: `--run=1-325 -q` at
`C:\Users\skron\zmin-upstream-20260618T204951Z-97199-out` and
`--run=1-312 -q` at
`C:\Users\skron\zmin-upstream-20260618T205317Z-6063-out` stopped with the same
header-only/zero-byte-log pattern, while `--run=1-306 -q` at
`C:\Users\skron\zmin-upstream-20260618T205412Z-7101-out` wrote a
`t0008-ignores.log` containing Git-for-Windows/MSYS `sed` fork/signal-pipe
errors (`child_copy` Win32 error `299`, `couldn't create signal pipe` Win32
error `5`) before the parent task disappeared without `upstream-runner.exit`.
Stock-Git control for the same bounded `--run=1-306 -q` at
`C:\Users\skron\zmin-upstream-20260618T205844Z-14158-out` also lost the
scheduled task before `upstream-runner.exit`, with header-only summary and a
zero-byte `t0008-ignores.log`.
Treat `1-300` as historical bounded `t0008` evidence only; it is superseded by
the clean full Windows integrated `16/16` run above.

A current dirty-worktree macOS integrated rerun after the `t4013` separator fix
is inconclusive, not green evidence:
`/tmp/zmin-macos-current-exhaustive-afterfix-20260618T183428Z/summary.tsv`
stopped with exit `143` while starting `t0027-auto-crlf.sh`; the summary had
only `11` passing files through `t0021-conversion.sh`, and
`t0027-auto-crlf.log` was zero bytes.

`t0000-basic.sh` is now green on macOS. The burn-down matched stock Git for
basic harness invocation behavior, `ls-tree -r -t`, `write-tree --prefix`,
`write-tree --missing-ok`, `update-index --index-info` with missing object ids,
`diff-files` and `diff-index` stale-stat behavior before and after
`update-index --refresh`, `show --pretty=raw`, duplicate-parent omission in
`commit-tree`, and `update-index --replace` / `--cacheinfo` D/F conflict
handling.

`t0021-conversion.sh` is now green on macOS. The burn-down matched stock Git
for `:path` blob objectish resolution across diff/cat-file/rev-parse, ident
clean/smudge canonicalization, `eol=crlf` checkout behavior, one-shot and
long-running clean/smudge filters, `%f` shell quoting, required missing
clean/smudge errors, BrokenPipe-tolerant filters, and `checkout-index --prefix`
smudge behavior.

`t0027-auto-crlf.sh` is now green on macOS for the targeted
Windows-sensitive newline/autocrlf burn-down. The accepted slices fixed
`ls-files --eol -o` text/binary classification for control characters,
`commit .` repository-root pathspec handling, config-driven CRLF clean and
round-trip warnings for `add`, checkout CRLF output conversion for
`core.autocrlf` / `core.eol`, `text=auto eol=crlf` mixed/binary preservation,
and `ls-files --eol` implicit `text` display for bare `eol=lf/crlf`
attributes. Failure progression was `1362/2600` to `1361/2600`, `361/2600`,
`257/2600`, `252/2600`, `104/2600`, `8/2600`, then pass at
`/var/folders/l3/y2d_2zz51z731b86_sstzz0h0000gn/T//zmin-upstream-compat.QV7Slc/summary.tsv`.
Windows/Git-for-Windows focused regression proof passed for the affected
`ls-files --eol`, `-text eol=crlf`, and `text=auto eol=crlf` checkout cases.
The Windows upstream replay now forces a fresh release rebuild before the suite,
avoiding stale `C:\Users\skron\zmin-target\release\zmin.exe` evidence. A fresh
Windows run moved past the earlier `commit files attr=text` warning mismatch
after `TextCrlf` warning selection was changed to prefer `LF -> CRLF` when the
checkout side writes CRLF; the partial log reached the checkout/`ls-files --eol`
block around assertion `2150/2600`. Follow-up runner probes showed that a
Windows Scheduled Task can keep a Git-Bash script alive past the Parallels Tools
session limit (`35s` sentinel probe passed), and direct scheduled
`t0027-auto-crlf.sh --run=1 -q` completed with `exit=0` at
`C:\Users\skron\zmin-direct-scheduled-run1b` (`t0027.log` 215322 bytes). The
upstream runner no longer schedules a second automatic run five minutes after
manual `Start-ScheduledTask`; it registers the task with a far-future trigger
and starts it explicitly. A clean full runner attempt after that fix produced a
real Windows `t0027-auto-crlf.log` at
`C:\Users\skron\zmin-upstream-20260617T210250Z-2150-out`: it failed around
checkout/`ls-files --eol` assertions `532`, `537-542`, and `549-553`, while the
parent suite still stopped before writing `upstream-runner.exit` and left only a
header `summary.tsv`. Treat those assertion numbers as the current parity
frontier and the missing sentinel as a separate Git-for-Windows/MSYS harness
stability issue. A later direct focused replay is not accepted as parity
evidence because it hit MSYS fork/load-address errors (`uniq.exe` fatal,
`test-lib-functions.sh: fork: Read-only file system`) with stale direct-run
processes present. The runner cleanup path now also stops stale host-side
`prlctl exec` sessions for old `t0027`/`zmin-*` direct runs before cleaning
guest tasks and MSYS helper processes; a cleanup probe finished with `tasks=0`
and `procs=0` after removing a stale `zmin-stockgit-t0027` run. A later clean
Windows upstream micro-replay with `t0027-auto-crlf.sh --run=1-2 -q` completed
through the runner at `C:\Users\skron\zmin-upstream-20260617T212507Z-32151-out`:
`upstream-runner.exit` was `0`, `summary.tsv` recorded `passed=1`, and the TAP
tail ended with `# passed all 2600 test(s)` / `1..2600`. This proves the
Scheduled Task parent can now return and write the sentinel for a narrow
selected range, but it still does not prove full Windows `t0027` parity because
assertions outside `1-2` were skipped. A later runner hardening pass moved the
upstream release build onto the same logged `Start-Process` path used by
`build-release` (`CARGO_BUILD_JOBS=1`, stdout/stderr files) and tightened stale
host-side `.zmin-parallels-script.*.ps1` cleanup by inspecting the script
contents for `t0027`/`zmin-upstream` markers before killing. Validation:
`tools/parallels-windows-runner.sh build-release` produced
`C:\Users\skron\zmin-target\release\zmin.exe` (`7348736` bytes), and a patched
micro-replay with `t0027-auto-crlf.sh --run=1-2 -q` completed at
`C:\Users\skron\zmin-upstream-20260617T220659Z-61903-out` with
`upstream-runner.exit=0`, `passed=1`, `failed=0`. A patched full run at
`C:\Users\skron\zmin-upstream-20260617T220311Z-58174-out` built successfully but
the upstream Bash task stopped before `upstream-runner.exit`, leaving a
header-only summary and zero-byte `t0027-auto-crlf.log`. A bounded Zmin replay
for `--run=520-560 -v` at
`C:\Users\skron\zmin-upstream-20260617T221456Z-66776-out` showed the same
header-only/no-log stop. A stock Git control for the same bounded range at
`C:\Users\skron\zmin-upstream-20260617T221602Z-68046-out` was inconclusive: it
progressed into TAP skip output around assertion `366` and then stalled without
a sentinel. Cleanup after the stock probe finished with `procs=0`. Treat the
current Windows `t0027` gap as unresolved full-range Git-for-Windows/MSYS
harness instability plus the earlier real parity frontier around assertions
`532`, `537-542`, and `549-553`. Those assertions map to the checkout and
`ls-files --eol` block for `core.autocrlf=false`, `core.eol=lf`, and
`-text eol=crlf` / `text eol=lf` / `text eol=crlf` attributes. Focused local
regression proof now covers that block through
`checkout_core_autocrlf_false_core_eol_lf_text_attribute_matrix_matches_stock_git`:
macOS `cargo test -p zmin-cli --test git_worktree_state_compat
checkout_core_autocrlf_false_core_eol_lf_text_attribute_matrix_matches_stock_git
-- --nocapture` passed (`1/1`), and Windows/Git-for-Windows
`ZMIN_WINDOWS_VALIDATE_NO_FMT=1 tools/parallels-windows-runner.sh validate
targeted git_worktree_state_compat
checkout_core_autocrlf_false_core_eol_lf_text_attribute_matrix_matches_stock_git`
passed (`1/1`). The whole focused checkout/worktree state file also passes on
both platforms: macOS `cargo test -p zmin-cli --test git_worktree_state_compat
-- --nocapture` passed `26/26`, and Windows/Git-for-Windows
`ZMIN_WINDOWS_VALIDATE_NO_FMT=1 tools/parallels-windows-runner.sh validate file
git_worktree_state_compat` passed `26/26`. This reduces the likely Zmin
behavior risk in the known frontier block, but do not count the upstream file
green on Windows until a clean full or accepted chunked strategy writes summary
and sentinel.

Follow-up bounded-run harness proof: `ZMIN_UPSTREAM_BOUNDED_RUN=1` now patches
upstream `t/test-lib.sh` to stop after the max numeric `--run` selector instead
of running the skip-heavy remainder of `t0027`. Local stock Git control with
`--run=1-2 -q` passed at
`/var/folders/l3/y2d_2zz51z731b86_sstzz0h0000gn/T//zmin-upstream-compat.xqPouU/summary.tsv`;
the TAP log ended with `# passed all 2 test(s)` / `1..2`. The Parallels runner
now forwards `ZMIN_UPSTREAM_BOUNDED_RUN` into the guest upstream script. Windows
stock Git bounded control passed at
`C:\Users\skron\zmin-upstream-20260617T223731Z-20089-out` and Windows Zmin
bounded control passed at
`C:\Users\skron\zmin-upstream-20260617T223848Z-20422-out`; both wrote
`upstream-runner.exit=0`, recorded `passed=1`, and ended TAP at `1..2`.
Post-run guest state was clean (`procs=0`) and the host stale-process check only
matched the current `rg` probe. This accepts the bounded runner mechanics for
small focused chunks, not full Windows `t0027` parity. Isolated ranges such as
`--run=520-560` remain invalid as behavior proof unless their setup dependency
is included or a stock Git control demonstrates the same isolated-range result.

Setup-aware bounded chunk follow-up: local stock Git and local Zmin both passed
`t0027-auto-crlf.sh --run=1-560 -q` with bounded stop at `1..560`. Evidence:
stock Git
`/tmp/zmin-upstream-t0027-stock-1-560-20260617T224356Z/summary.tsv`, Zmin
`/tmp/zmin-upstream-t0027-zmin-1-560-20260617T224431Z/summary.tsv`. Windows
stock Git also passed the same setup-aware chunk at
`C:\Users\skron\zmin-upstream-20260617T224512Z-35957-out` with
`upstream-runner.exit=0`, `passed=1`, and TAP `1..560`. Earlier attached
Windows Zmin attempts for `--run=1-560 -q` and the smaller `--run=1-315 -q` are
not accepted as behavior evidence:
`C:\Users\skron\zmin-upstream-20260617T225203Z-37834-out`
and `C:\Users\skron\zmin-upstream-20260617T225746Z-41469-out` stopped before
sentinel with header-only summaries / zero-byte TAP logs after host-side
Parallels session cancellation or Scheduled Task `Ready` state. Treat those as
runner/Parallels instability, not Zmin parity failure. The runner cleanup path
now unregisters no-sentinel tasks and kills MSYS helper processes when the task
is no longer running; if it is still running with an attached runner process, it
continues to leave it alone for later inspection. The current guest/host cleanup
ended with `tasks=0`, `procs=0`, and only the current `rg` process matching the
host stale-process probe. After a clean VM stop/start cycle, a bounded stock Git
micro-run wrote valid output at
`C:\Users\skron\zmin-upstream-20260617T230754Z-57431-out` (`exit=0`, TAP
`1..2`), but the attached host wrapper returned `127` after printing the
successful output. The detached/poll path avoided that host-session ambiguity:
`C:\Users\skron\zmin-upstream-20260617T231125Z-60086-out` was polled with
`tools/parallels-windows-runner.sh upstream-poll`, returned `0`, recorded
`passed=1`, ended TAP at `1..2`, and cleaned to `tasks=0`, `procs=0`. Use
detached/poll for future Windows `t0027` chunks. The previously inconclusive
setup-aware Windows Zmin chunk then passed through detached/poll at
`C:\Users\skron\zmin-upstream-20260617T231344Z-62525-out`: the generated runner
script used `ZMIN_BIN=/c/Users/skron/zmin-target/release/zmin.exe`, no stock Git
control override, wrote `upstream-runner.exit=0`, recorded `passed=1`, and the
TAP log ended at `# passed all 560 test(s)` / `1..560`. A stale ready task was
removed afterwards and cleanup ended with `tasks=0`, `procs=0`.

Post-560 extension attempt: detached Windows stock Git passed
`t0027-auto-crlf.sh --run=1-700 -q` at
`C:\Users\skron\zmin-upstream-20260617T232529Z-87470-out` with
`upstream-runner.exit=0`, `passed=1`, and TAP `1..700`. Detached Windows Zmin
passed the narrower `--run=1-620 -q` chunk at
`C:\Users\skron\zmin-upstream-20260617T234902Z-97648-out`: the generated runner
script used `ZMIN_BIN=/c/Users/skron/zmin-target/release/zmin.exe`, no stock Git
control override, wrote `upstream-runner.exit=0`, recorded `passed=1`, and the
TAP log ended at `# passed all 620 test(s)` / `1..620`. A narrower follow-up
detached Windows Zmin `--run=1-635 -q` chunk passed at
`C:\Users\skron\zmin-upstream-20260618T000446Z-6436-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 635 test(s)` / `1..635`, and cleanup confirmed `tasks=0`,
`procs=0`. A later clean detached Windows Zmin prefix advanced the accepted
frontier to `--run=1-850 -q` at
`C:\Users\skron\zmin-upstream-20260617T233435Z-90976-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, and the TAP log ended at
`# passed all 850 test(s)` / `1..850`. A clean follow-up split advanced the
accepted frontier to `--run=1-875 -q` at
`C:\Users\skron\zmin-upstream-20260618T002019Z-26048-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 875 test(s)` / `1..875`, and cleanup confirmed `tasks=0`,
`procs=0`. A follow-up clean split advanced the accepted frontier to
`--run=1-888 -q` at
`C:\Users\skron\zmin-upstream-20260618T003134Z-32543-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 888 test(s)` / `1..888`, and cleanup confirmed `tasks=0`,
`procs=0`. A subsequent clean retry advanced the accepted frontier to
`--run=1-894 -q` at
`C:\Users\skron\zmin-upstream-20260618T004310Z-40055-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 894 test(s)` / `1..894`, and cleanup confirmed `tasks=0`,
`procs=0`. Follow-up clean splits advanced the accepted frontier first to
`--run=1-897 -q` at
`C:\Users\skron\zmin-upstream-20260618T005056Z-45629-out` (`passed=1`, TAP
`1..897`), then to `--run=1-900 -q` at
`C:\Users\skron\zmin-upstream-20260618T005853Z-51322-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 900 test(s)` / `1..900`, and cleanup confirmed `tasks=0`,
`procs=0`, and then to `--run=1-925 -q` at
`C:\Users\skron\zmin-upstream-20260618T010923Z-62781-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 925 test(s)` / `1..925`, and cleanup confirmed `tasks=0`,
`procs=0`, then to `--run=1-950 -q` at
`C:\Users\skron\zmin-upstream-20260618T011831Z-68910-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 950 test(s)` / `1..950`, and cleanup confirmed `tasks=0`,
`procs=0`, then to `--run=1-1000 -q` at
`C:\Users\skron\zmin-upstream-20260618T012842Z-70601-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 1000 test(s)` / `1..1000`, and cleanup confirmed `tasks=0`,
`procs=0`, then to `--run=1-1040 -q` at
`C:\Users\skron\zmin-upstream-20260618T014147Z-81896-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 1040 test(s)` / `1..1040`, and cleanup confirmed `tasks=0`,
`procs=0`, then to `--run=1-1070 -q` at
`C:\Users\skron\zmin-upstream-20260618T015516Z-84211-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 1070 test(s)` / `1..1070`, and cleanup confirmed `tasks=0`,
`procs=0`, then to `--run=1-1200 -q` at
`C:\Users\skron\zmin-upstream-20260618T020544Z-344-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 1200 test(s)` / `1..1200`, and cleanup confirmed `tasks=0`,
`procs=0`, then to `--run=1-1250 -q` at
`C:\Users\skron\zmin-upstream-20260618T022223Z-7616-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 1250 test(s)` / `1..1250`, and cleanup confirmed `tasks=0`,
`procs=0`, then to `--run=1-1275 -q` at
`C:\Users\skron\zmin-upstream-20260618T023429Z-13076-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 1275 test(s)` / `1..1275`, and cleanup confirmed `tasks=0`,
`procs=0`, then to `--run=1-1288 -q` at
`C:\Users\skron\zmin-upstream-20260618T024856Z-20561-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 1288 test(s)` / `1..1288`, and cleanup confirmed `tasks=0`,
`procs=0`, then to `--run=1-1294 -q` at
`C:\Users\skron\zmin-upstream-20260618T030032Z-28814-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 1294 test(s)` / `1..1294`, and cleanup confirmed `tasks=0`,
`procs=0`, then to `--run=1-1295 -q` at
`C:\Users\skron\zmin-upstream-20260618T031356Z-40205-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 1295 test(s)` / `1..1295`, and cleanup confirmed `tasks=0`,
`procs=0`, then to `--run=1-1296 -q` at
`C:\Users\skron\zmin-upstream-20260618T032527Z-55624-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 1296 test(s)` / `1..1296`, and cleanup confirmed `tasks=0`,
`procs=0`, then to a manual clean `--run=1-1297 -q` retry at
`C:\Users\skron\zmin-upstream-20260618T033909Z-63329-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 1297 test(s)` / `1..1297`, and cleanup confirmed `tasks=0`,
`procs=0`, then to `--run=1-1300 -q` at
`C:\Users\skron\zmin-upstream-20260618T035820Z-80453-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 1300 test(s)` / `1..1300`, and cleanup confirmed `tasks=0`,
`procs=0`, then to `--run=1-1312 -q` at
`C:\Users\skron\zmin-upstream-20260618T041817Z-93781-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 1312 test(s)` / `1..1312`, and cleanup confirmed `tasks=0`,
`procs=0`, then to delayed `--run=1-1325 -q` at
`C:\Users\skron\zmin-upstream-20260618T043809Z-6476-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 1325 test(s)` / `1..1325`, and cleanup confirmed `tasks=0`,
`procs=0`, then to a clean `--run=1-1350 -q` retry at
`C:\Users\skron\zmin-upstream-20260618T045357Z-18050-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 1350 test(s)` / `1..1350`, and cleanup confirmed `tasks=0`,
`procs=0`, then to `--run=1-1375 -q` at
`C:\Users\skron\zmin-upstream-20260618T050651Z-29377-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 1375 test(s)` / `1..1375`, and cleanup confirmed `tasks=0`,
`procs=0`, then to `--run=1-1400 -q` at
`C:\Users\skron\zmin-upstream-20260618T051953Z-36712-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 1400 test(s)` / `1..1400`, and cleanup confirmed `tasks=0`,
`procs=0`, then to `--run=1-1425 -q` at
`C:\Users\skron\zmin-upstream-20260618T053502Z-49040-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 1425 test(s)` / `1..1425`, and cleanup confirmed `tasks=0`,
`procs=0`, then to a clean `--run=1-1450 -q` retry at
`C:\Users\skron\zmin-upstream-20260618T055336Z-73874-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 1450 test(s)` / `1..1450`, and cleanup confirmed `tasks=0`,
`procs=0`, then to a clean `--run=1-1475 -q` retry at
`C:\Users\skron\zmin-upstream-20260618T061631Z-90649-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 1475 test(s)` / `1..1475`, and cleanup confirmed `tasks=0`,
`procs=0`, then to `--run=1-1500 -q` at
`C:\Users\skron\zmin-upstream-20260618T063008Z-9868-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 1500 test(s)` / `1..1500`, and cleanup confirmed `tasks=0`,
`procs=0`, then to `--run=1-1525 -q` at
`C:\Users\skron\zmin-upstream-20260618T064247Z-27409-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 1525 test(s)` / `1..1525`, and cleanup confirmed `tasks=0`,
`procs=0`, then to `--run=1-1550 -q` at
`C:\Users\skron\zmin-upstream-20260618T065647Z-41899-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 1550 test(s)` / `1..1550`, and cleanup confirmed `tasks=0`,
`procs=0`, then to `--run=1-1575 -q` at
`C:\Users\skron\zmin-upstream-20260618T071034Z-51080-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 1575 test(s)` / `1..1575`, and cleanup confirmed `tasks=0`,
`procs=0`, then to `--run=1-1600 -q` at
`C:\Users\skron\zmin-upstream-20260618T072406Z-63964-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 1600 test(s)` / `1..1600`, and cleanup confirmed `tasks=0`,
`procs=0`, then to `--run=1-1625 -q` at
`C:\Users\skron\zmin-upstream-20260618T074102Z-81140-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 1625 test(s)` / `1..1625`, and cleanup confirmed `tasks=0`,
`procs=0`, then to a clean `--run=1-1650 -q` retry at
`C:\Users\skron\zmin-upstream-20260618T080117Z-240-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 1650 test(s)` / `1..1650`, and cleanup confirmed `tasks=0`,
`procs=0`, then to `--run=1-1675 -q` at
`C:\Users\skron\zmin-upstream-20260618T082115Z-13584-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 1675 test(s)` / `1..1675`, and cleanup confirmed `tasks=0`,
`procs=0`, then to `--run=1-1700 -q` at
`C:\Users\skron\zmin-upstream-20260618T084253Z-22306-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 1700 test(s)` / `1..1700`, and cleanup confirmed `tasks=0`,
`procs=0`, then to `--run=1-1725 -q` at
`C:\Users\skron\zmin-upstream-20260618T090236Z-43539-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 1725 test(s)` / `1..1725`, and cleanup confirmed `tasks=0`,
`procs=0`, then to `--run=1-1750 -q` at
`C:\Users\skron\zmin-upstream-20260618T092047Z-61541-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 1750 test(s)` / `1..1750`, and cleanup confirmed `tasks=0`,
`procs=0`, then to `--run=1-1775 -q` at
`C:\Users\skron\zmin-upstream-20260618T093838Z-75531-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 1775 test(s)` / `1..1775`, and cleanup confirmed `tasks=0`,
`procs=0`, then to `--run=1-1800 -q` at
`C:\Users\skron\zmin-upstream-20260618T100508Z-263-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 1800 test(s)` / `1..1800`, and cleanup confirmed `tasks=0`,
`procs=0`, then to `--run=1-1825 -q` at
`C:\Users\skron\zmin-upstream-20260618T102240Z-9595-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 1825 test(s)` / `1..1825`, and cleanup confirmed `tasks=0`,
`procs=0`, then to `--run=1-1850 -q` at
`C:\Users\skron\zmin-upstream-20260618T104030Z-25358-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 1850 test(s)` / `1..1850`, and cleanup confirmed `tasks=0`,
`procs=0`, then to `--run=1-1875 -q` at
`C:\Users\skron\zmin-upstream-20260618T105728Z-32384-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 1875 test(s)` / `1..1875`, and cleanup confirmed `tasks=0`,
`procs=0`, then to `--run=1-1900 -q` at
`C:\Users\skron\zmin-upstream-20260618T111511Z-49378-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 1900 test(s)` / `1..1900`, and cleanup confirmed `tasks=0`,
`procs=0`, then to `--run=1-1925 -q` at
`C:\Users\skron\zmin-upstream-20260618T113825Z-69888-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 1925 test(s)` / `1..1925`, and cleanup confirmed `tasks=0`,
`procs=0`, then to `--run=1-1950 -q` at
`C:\Users\skron\zmin-upstream-20260618T115401Z-80980-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 1950 test(s)` / `1..1950`, and cleanup confirmed `tasks=0`,
`procs=0`, then to `--run=1-1975 -q` at
`C:\Users\skron\zmin-upstream-20260618T121220Z-97816-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 1975 test(s)` / `1..1975`, and cleanup confirmed `tasks=0`,
`procs=0`, then to `--run=1-2050 -q` at
`C:\Users\skron\zmin-upstream-20260618T123448Z-16338-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 2050 test(s)` / `1..2050`, and cleanup confirmed `tasks=0`,
`procs=0`, then to `--run=1-2100 -q` at
`C:\Users\skron\zmin-upstream-20260618T125156Z-29217-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 2100 test(s)` / `1..2100`, and cleanup confirmed `tasks=0`,
`procs=0`, then to `--run=1-2150 -q` at
`C:\Users\skron\zmin-upstream-20260618T131021Z-43572-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 2150 test(s)` / `1..2150`, and cleanup confirmed `tasks=0`,
`procs=0`, then to `--run=1-2200 -q` at
`C:\Users\skron\zmin-upstream-20260618T133232Z-64811-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 2200 test(s)` / `1..2200`, and cleanup confirmed `tasks=0`,
`procs=0`, then to `--run=1-2250 -q` at
`C:\Users\skron\zmin-upstream-20260618T135252Z-75703-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 2250 test(s)` / `1..2250`, and cleanup confirmed `tasks=0`,
`procs=0`, then to `--run=1-2300 -q` at
`C:\Users\skron\zmin-upstream-20260618T141651Z-99429-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 2300 test(s)` / `1..2300`, and cleanup confirmed `tasks=0`,
`procs=0`, then to `--run=1-2350 -q` at
`C:\Users\skron\zmin-upstream-20260618T143554Z-15165-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 2350 test(s)` / `1..2350`, and cleanup confirmed `tasks=0`,
`procs=0`, then to `--run=1-2375 -q` at
`C:\Users\skron\zmin-upstream-20260618T145804Z-30606-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 2375 test(s)` / `1..2375`, and cleanup confirmed `tasks=0`,
`procs=0`, then to `--run=1-2400 -q` at
`C:\Users\skron\zmin-upstream-20260618T152301Z-50906-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 2400 test(s)` / `1..2400`, and cleanup confirmed `tasks=0`,
`procs=0`, then to `--run=1-2450 -q` at
`C:\Users\skron\zmin-upstream-20260618T154504Z-70558-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 2450 test(s)` / `1..2450`, and cleanup confirmed `tasks=0`,
`procs=0`, then to `--run=1-2500 -q` at
`C:\Users\skron\zmin-upstream-20260618T160926Z-95571-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 2500 test(s)` / `1..2500`, and cleanup confirmed `tasks=0`,
`procs=0`, then to `--run=1-2550 -q` at
`C:\Users\skron\zmin-upstream-20260618T163810Z-10494-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 2550 test(s)` / `1..2550`, and cleanup confirmed `tasks=0`,
`procs=0`, then to `--run=1-2600 -q` at
`C:\Users\skron\zmin-upstream-20260618T170832Z-26268-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 2600 test(s)` / `1..2600`, and cleanup confirmed `tasks=0`,
`procs=0`. A clean full-file replay then passed at
`C:\Users\skron\zmin-upstream-20260618T174128Z-51900-out`: it wrote
`upstream-runner.exit=0`, recorded `passed=1`, ended TAP at
`# passed all 2600 test(s)` / `1..2600`, and cleanup confirmed `tasks=0`,
`procs=0`. A later cleanup probe found an orphaned guest
`cargo build -p zmin-cli --release --bins` / `rustc git_http_backend` process
tree without a `ZminUpstream-*` owner task; it was stopped before any next
Windows upstream run. The run also confirmed the earlier
`--run=1-700 -q` prefix at
`C:\Users\skron\zmin-upstream-20260617T232529Z-87470-out` with
`upstream-runner.exit=0`, `passed=1`, and TAP `1..700`. Wider lifecycle retries
are not accepted as product evidence yet: the previous `--run=1-1040 -q`
(`C:\Users\skron\zmin-upstream-20260617T232031Z-73186-out`), two
previous `--run=1-950 -q` attempts
(`C:\Users\skron\zmin-upstream-20260617T234503Z-92740-out` and
`C:\Users\skron\zmin-upstream-20260618T000306Z-5380-out`), and
`--run=1-900 -q` (`C:\Users\skron\zmin-upstream-20260618T001047Z-9407-out`)
stopped before sentinel with header-only summaries and zero-byte TAP logs. An
attempted `--run=1-938 -q` split at
`C:\Users\skron\zmin-upstream-20260618T011821Z-68734-out` also remained
header-only without sentinel before the clean `1-950` retry superseded it.
Attempted `--run=1-1100 -q` at
`C:\Users\skron\zmin-upstream-20260618T015230Z-83215-out` also remained
header-only without sentinel or stderr before the clean `1-1070` split.
Attempted `--run=1-1300 -q` at
`C:\Users\skron\zmin-upstream-20260618T021857Z-6321-out` also remained
header-only without sentinel or stderr before the clean `1-1250` split. A
later `--run=1-1300 -q` retry at
`C:\Users\skron\zmin-upstream-20260618T022215Z-7429-out` stopped before
sentinel with a header-only summary and zero-byte TAP log before the clean
`1-1275` split completed. Attempted `--run=1-1297 -q` at
`C:\Users\skron\zmin-upstream-20260618T031211Z-38655-out` stayed in
Scheduled Task `Ready` state with a header-only summary, zero-byte TAP log, no
sentinel, and an empty stderr before the clean `1-1295` split; a second
`--run=1-1297 -q` attempt at
`C:\Users\skron\zmin-upstream-20260618T033549Z-57583-out` disappeared without
sentinel, kept a header-only summary and zero-byte TAP log, had empty stderr,
and cleaned up to `tasks=0`, `procs=0` after the clean `1-1296` split. A clean
`--run=1-1300 -q` artifact at
`C:\Users\skron\zmin-upstream-20260618T035316Z-74900-out` started Running after
accepted `1-1297`, then disappeared without sentinel with a header-only
summary, zero-byte TAP log, empty stderr, and a clean guest probe. A queued
`--run=1-1325 -q` artifact at
`C:\Users\skron\zmin-upstream-20260618T041641Z-88144-out` only reached
Scheduled Task `Ready` state with a header-only summary, zero-byte TAP log,
empty stderr, and no sentinel before cleanup; the delayed clean `1-1325`
retry later passed. A queued `--run=1-1350 -q` artifact at
`C:\Users\skron\zmin-upstream-20260618T045047Z-12532-out` also only reached
Scheduled Task `Ready` state with a header-only summary, zero-byte TAP log,
empty stderr, and no sentinel before cleanup. A `--run=1-1450 -q` attempt at
`C:\Users\skron\zmin-upstream-20260618T054709Z-62165-out` also reached
Scheduled Task `Ready` state without `upstream-runner.exit`, with only the
`summary.tsv` header and no TAP tail before cleanup; it is superseded by the
later clean `1-1450` retry. A `--run=1-1475 -q` attempt at
`C:\Users\skron\zmin-upstream-20260618T061048Z-83023-out` likewise reached
Scheduled Task `Ready` state without `upstream-runner.exit`, with only the
`summary.tsv` header and no TAP tail before cleanup, so the accepted frontier
remained `1-1450` until the later clean `1-1475` retry. A clean retry at
`C:\Users\skron\zmin-upstream-20260618T061608Z-90369-out` lost its Scheduled
Task while MSYS child processes continued, then ended without
`upstream-runner.exit`, with only the `summary.tsv` header and no TAP tail;
this is also lifecycle noise, not accepted product evidence, and is superseded
by the later clean `1-1475` retry, which is now superseded by the clean
`1-1625` retry. A queued `--run=1-1650 -q` artifact at
`C:\Users\skron\zmin-upstream-20260618T075633Z-93328-out` reached Scheduled
Task `Ready` state with no accepted sentinel or TAP evidence after the clean
`1-1625` pass; treat it as lifecycle noise superseded by the later clean
`1-1650` retry. An
earlier `--run=1-894 -q` attempt at
`C:\Users\skron\zmin-upstream-20260618T004040Z-38931-out` hit the same
lifecycle pattern: missing sentinel, header-only summary, zero-byte TAP log,
empty stderr, and cleanup at `tasks=0`, `procs=0`. The
latest wider retries were also contaminated by host-side queued
`sleep && upstream-poll` processes and stray smaller delayed chunks
(`1-620`, `1-635`, `1-642`, `1-650`), so treat them as runner/MSYS lifecycle
instability, not Zmin assertion failures. Cleanup stopped the queued host polls,
unregistered `ZminUpstream-*` tasks, killed MSYS helper processes, and left the
guest probe empty. An attempted `1-2050` preflight start at
`C:\Users\skron\zmin-upstream-20260618T122927Z-9649` stopped before upstream
test execution when the Windows release build returned exit `-1`; treat this as
build runner noise because the accepted retry reused the unchanged release
binary after docs/knowledge-only edits. The `1-2200` launch printed a host-side
`Canceling the job/session` message before detached output, but the guest
Scheduled Task existed, was polled directly, and produced accepted
summary/sentinel/TAP evidence. A stale `--run=1-2400 -q` artifact at
`C:\Users\skron\zmin-upstream-20260618T145616Z-29241-out` later reached
Scheduled Task `Ready` state with no `upstream-runner.exit`, a header-only
summary, and a zero-byte TAP log after the clean `1-2350` pass; cleanup then
confirmed `tasks=0`, `procs=0`, and it is not accepted
evidence. The accepted Windows/Git-for-Windows bounded replay reaches
`1..2600`, and the clean full-file replay now confirms final Windows `t0027`
file signoff; broader supported-surface parity remains separate.

`t2020-checkout-detach.sh` is now green on macOS. The burn-down matched stock
Git for `checkout HEAD` / `checkout @` no-op behavior, `checkout --detach`
defaulting to current `HEAD`, full `refs/heads/<name>` checkout detaching
instead of switching, detached-head advice/output, orphan warnings, and checkout
branch upstream tracking output separation between stdout and stderr. A final
reflog regression fix matched stock Git by making `checkout --orphan` update
symbolic `HEAD` without writing a zero-new-id HEAD reflog entry.

`t4013-diff-various.sh` is now green on macOS. The final burn-down matched
stock Git for merge diff modes, `diff-tree --stdin`, multi-commit `show`
pathspecs, `--line-prefix`, `diff-index -m`, `-I`/`--ignore-matching-lines`
with `--ignore-blank-lines` across patch/stat/raw/name formats, malformed `-I`
diagnostics, conflicted-index raw/name metadata, file-to-directory worktree
diff handling, stat-only refresh behavior, `diff.noPrefix` config parsing, and
index stat metadata refresh after branch checkout so raw diff uses materialized
index blob ids. The 2026-06-18 regression fix also matches stock Git for
`git show <empty-root-ref>` with default `log.showroot=true`: Zmin no longer
prints a patch separator when the root diff has no entries, while explicit
`--root` and non-empty root diffs still show the root diff.
Windows/Git-for-Windows targeted CLI validation passed for the same empty-root regression in
guest copy `C:\Users\skron\zmin-20260618T191006Z-64719`; earlier foreground
runner and manual Task Scheduler attempts that stopped with exit `143` or
before task creation remain invalid runner lifecycle evidence.

The latest targeted `t5510-fetch.sh` supported-surface run skips only
reftable-dependent upstream assertions through
`ZMIN_UPSTREAM_SKIP_UNSUPPORTED_REFTABLE=1` and now passes. The burn-down
covered direct URL `fetch --tags file://<repo>`, configured fetch, `FETCH_HEAD`
for-merge ordering, remote HEAD change, `followRemoteHEAD` modes (`never`,
`warn`, `warn-if-not-*`, `always`), explicit fetch refspecs, dangling remote
HEAD setup, `fetch --prune`, namespace pruning, tag-preserving prune modes,
overlapping wildcard refspec pruning, smart HTTP `git-http-backend` ScriptAlias
execution, non-bare smart HTTP export, and multiple explicit smart HTTP tag
refspecs under `protocol.version=2`. The Windows/Git-for-Windows burn-down also
covered Git-for-Windows `file:///c/...` local repository URLs for submodule
clone/fetch setup and branch-name directory/file remote-tracking ref conflicts
returning the stock Git prune hint instead of a raw Windows `Access is denied`
filesystem error.

`t1410-reflog.sh` is now green on macOS. The final burn-down covered
fast-import reflog creation, branch/HEAD reflog recording, reflog delete/drop,
expire no-op/stale/timestamp handling, linked-worktree reflog listing, reflog
pattern config such as `gc.refs/heads/root2/*.reflogExpire`,
`log -g --branches=<glob> --format=%gD`, and `log -g` ordinal behavior across
hidden zero-new-id entries without adding holes for orphan checkout histories.

The latest Windows targeted `t1006-cat-file.sh` run confirms quick preflight,
provider smoke, real-repository smoke, and the full selected cat-file suite
under Git-for-Windows. The previous Windows-only blockers in this file were fixed:
symlink-challenged index mode preservation, `git add --refresh` stat refresh,
the installed-binary upstream `test_cmp` harness override, and `--chmod` on
symlink index entries materialized as regular files when `core.symlinks=false`.
Additional installed-binary upstream harness shims now cover `test-tool
genrandom`, raw `sha1`, `zlib deflate`, and `path-utils file-size` for Windows.
The previous selected `standard` blockers in `t3200-branch.sh` and
`t3903-stash.sh` are now green on both platforms. The `t3903` burn-down fixed a
Windows-sensitive racy index/staging path where same-size rapid rewrites could
be skipped after reset/stash when the file metadata matched.

## What is covered now

- macOS and Windows/Git-for-Windows can run the same upstream compatibility
  harness locally.
- The quick suite is green on both platforms.
- The selected standard suite is green on both platforms.
- The expanded supported-surface suite is green on both platforms:
  `15/15` selected files on macOS and `15/15` selected files on
  Windows/Git-for-Windows with reftable assertions skipped as unsupported.
- Native Windows extended smoke covers build, status/diff/log/rev-list/ls-tree,
  local clone/fetch/push/pull, provider remote smoke, and a real repository
  mutation workflow.
- Additive Zmin `clone --worktree-first` / `clone --instant` coverage now
  includes local repositories, smart HTTP remotes, git-daemon remotes, and SSH
  remotes on macOS and Windows/Git-for-Windows. This is not upstream Git parity
  surface. The remote slices materialize the selected `HEAD` worktree first,
  write only refs for requested objects, record `zmin.worktreeFirst=true`, and
  validate that a later normal `fetch origin` hydrates additional branch and tag
  refs.
- Additive Zmin `clone --instant --background-fetch` is now explicit opt-in
  for remote worktree-first clones. It preserves the default `--instant`
  HEAD-only behavior, starts a detached `fetch origin` after checkout, records
  background-fetch config markers, and validates smart HTTP, git-daemon, and
  SSH branch/tag hydration on macOS and Windows/Git-for-Windows loopback or
  fake-SSH fixtures. The same slice changed
  non-depth HTTP, git-daemon, and SSH fetches to write remote refs only after
  object hydration, matching safer Git semantics for failed/background fetches.
- Additive Zmin `clone --instant --demand-hydrate` is now explicit opt-in for
  remote worktree-first clones over smart HTTP, git-daemon, and SSH. It records
  `remote.origin.promisor=true`, `zmin.worktreeFirstDemandHydrate=true`, and
  the demand-hydrate remote marker, then validates that missing local `HEAD`
  objects are hydrated by `cat-file` through the configured promisor remote on
  macOS and Windows/Git-for-Windows loopback/fake-SSH fixtures.
- Promisor-only demand hydration now has focused object-plumbing coverage:
  `cat-file -t <object>` and `cat-file <type> <object>` hydrate a missing local
  object from a configured local or HTTP promisor remote before retrying the
  read. This is guarded by `remote.<name>.promisor=true`; normal repositories
  keep the previous missing-object behavior. The same focused object-plumbing
  slice fixed single-object root-commit `show HEAD` to include the root patch
  like stock Git.
- Worktree-first clone performance gates now cover local, smart HTTP,
  git-daemon, and SSH loopback/fake-SSH scenarios on macOS and
  Windows/Git-for-Windows. They are correctness-plus-timing gates, not a closed
  performance claim: the latest macOS one-repeat remote instant smoke reuses the
  advertised upload-pack session and is faster than stock Git for git-daemon and
  fake-SSH on that small fixture. The refreshed Windows/Git-for-Windows
  three-repeat gate after the same optimization has all clone-instant checks ok
  and closes the git-daemon instant median gap, but fake-SSH instant still has
  noisy slower median behavior.

## Remaining compatibility work

Priority order:

1. Keep every supported command surface tied to either upstream-file coverage,
   local parity tests, or an explicit unsupported/out-of-scope decision.
2. Broaden real-repository scale and long-running transport scenarios on the
   local Windows runner before expanding the supported parity claim.
3. Keep Windows-only newline, executable-bit, symlink, path, quoting, and
   process-behavior differences under upstream or focused local coverage as the
   supported surface expands.
4. Treat authenticated transport, proxy, and non-loopback network variants as
   separate scenario gates rather than implied coverage from local loopback
   transport tests.

## Unsupported / out-of-scope status

No unexplained failures are allowed for completion. A failing upstream assertion
must either be fixed or moved to an explicit unsupported/out-of-scope list with
the affected command, option, upstream test name, and product decision.

Current broader burn-down status as of 2026-06-18:

- Command inventory is green for the tracked baselines. Validation:
  `ZMIN_GIT_GAP_STRICT=1 ./tools/git-command-gap.sh` reported v2.32 baseline
  `145/145`, `missing_command_baseline=0`; `ZMIN_GIT_BASELINE=v2.47.1
  ./tools/git-command-gap.sh` reported raw v2.47 including help `151/151`,
  `raw_missing_upstream_commands_including_help=0`; and
  `cargo test -p zmin-cli --test compatibility_command -- --nocapture` passed
  `4/4`.
- Behavior parity is green only for the selected supported upstream surface:
  current expanded `exhaustive` runs are `16/16` selected files on macOS and
  Windows/Git-for-Windows with unsupported reftable assertions skipped.
- Additive Zmin CLI surface is not counted as upstream parity, but its
  canonical `.git` behavior is currently covered by focused macOS and Windows
  tests for managed hooks, CMS porcelain, local/remote `clone --instant`,
  `--background-fetch`, and `--demand-hydrate`.
- Performance gates are collected and correctness-clean on macOS and Windows
  with real Gitoxide where comparable. They do not close all optimization work;
  the remaining measured gaps are tracked in
  `docs/cli/performance_benchmark_2026-05-18.md`.

The following surfaces are not approved as complete Git parity:

- `cat-file` is green for the selected `t1006-cat-file.sh` file on macOS and
  Windows/Git-for-Windows; additional object-plumbing behavior outside this
  selected file remains subject to later exhaustive coverage.
- `rev-parse` behavior beyond the now-green selected `t1500-rev-parse.sh`
  file remains subject to additional upstream files and explicit scope review.
- `checkout-index`, `read-tree`, and `write-tree` behavior beyond the now-green
  selected `t2000-conflict-when-checking-files-out.sh` file remains subject to
  additional upstream files and explicit scope review.
- `branch` is green for the selected `t3200-branch.sh` file on macOS and
  Windows/Git-for-Windows; additional branch behavior outside this selected file
  remains subject to later exhaustive coverage.
- `add` is green for the selected `t3700-add.sh` file on macOS and
  Windows/Git-for-Windows; additional add-path behavior outside this selected
  file remains subject to later exhaustive coverage.
- `stash` is green for the selected `t3903-stash.sh` file on macOS and
  Windows/Git-for-Windows; stash behavior outside this selected file remains
  subject to later exhaustive coverage.
- Reftable ref storage (`--ref-format=reftable`,
  `extensions.refStorage=reftable`) is explicitly unsupported until a real
  reftable backend exists. Supported-surface upstream runs may set
  `ZMIN_UPSTREAM_SKIP_UNSUPPORTED_REFTABLE=1`; full upstream runs without that
  flag must still report reftable-dependent assertions as outside current
  parity.
- Authenticated HTTPS/SSH transport, credential-helper flows, corporate proxy
  handling, custom enterprise TLS/proxy environments, and long-running
  non-loopback network scenarios are outside the current supported parity claim
  until each has a dedicated local and Windows scenario gate. Existing
  unauthenticated public-provider and loopback transport smokes do not imply
  parity for these environments.
- Git LFS, partial clone filter negotiation beyond the explicit
  demand-hydration surface, sparse-checkout expansion, signed commit/tag
  verification workflows, and platform-specific file watcher / daemon behavior
  remain outside the current parity claim unless a later test slice explicitly
  adds them.
- Larger real-repository scale scenarios are not complete. Existing real-repo
  smokes are useful preservation evidence, but they are not a substitute for a
  documented scale matrix with repository size, object count, ref count,
  transport, auth/proxy mode, and macOS/Windows results.

## Completion rule

This goal is not complete until:

- selected upstream quick, standard, and agreed exhaustive files are green on
  macOS and Windows/Git-for-Windows, or every remaining failure has an explicit
  unsupported/out-of-scope decision;
- local scenario tests still pass;
- Windows validation runs locally through Parallels without GitHub Actions;
- the compatibility evidence matrix links to the latest passing local outputs.
