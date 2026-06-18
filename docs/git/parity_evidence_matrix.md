# Git Compatibility Evidence Matrix

Date: 2026-06-18

## Reading this matrix

This matrix records evidence for command presence, local scenario coverage, and
repository-state handoff. It does not by itself prove full upstream Git behavior
parity. Upstream Git test-suite status is tracked separately in
`docs/git/upstream_compatibility_baseline.md`.

## Baseline and command coverage

| Surface | Command | Result |
| --- | --- | --- |
| Command baseline (v2.32.0) | `ZMIN_GIT_GAP_STRICT=1 ./tools/git-command-gap.sh` | `145/145`, `100.0%`, `0` gaps |
| Command baseline (v2.47.1) | `ZMIN_GIT_BASELINE=v2.47.1 ./tools/git-command-gap.sh` | `150/150`, `100.0%`, `0` gaps |
| CLI compatibility suite | `cargo test -p zmin-cli --all-targets` | `486/486` passing tests |
| Core primitive suite | `cargo test -p zmin-git-core --all-targets` | `66/66` passing tests |

## Upstream Git test-suite status

As of 2026-06-16:

- upstream quick suite: green on macOS and Windows/Git-for-Windows
- upstream standard suite: green; `9/9` selected files pass on macOS and
  `9/9` selected files pass on Windows/Git-for-Windows; `t1006-cat-file.sh`,
  `t1500-rev-parse.sh`, `t2000-conflict-when-checking-files-out.sh`,
  `t3200-branch.sh`, `t3700-add.sh`, and `t3903-stash.sh` are green on both
  platforms
- targeted `t1006-cat-file.sh` is green at `290/290` on macOS and
  Windows/Git-for-Windows
- targeted `t3200-branch.sh` is green at `167/167` on macOS and
  Windows/Git-for-Windows
- targeted `t3700-add.sh` is green at `58/58` on macOS and
  Windows/Git-for-Windows
- targeted `t3903-stash.sh` is green on macOS and Windows/Git-for-Windows;
  upstream records its own known breakage assertions as expected xfail
- expanded `exhaustive` supported-surface burn-down passes `16/16` selected
  files on macOS with `ZMIN_UPSTREAM_SKIP_UNSUPPORTED_REFTABLE=1`. Windows
  has the previous integrated `15/15` supported-surface run plus a clean
  full-file `t0027-auto-crlf.sh` signoff; rerun the integrated 16-file Windows
  list before claiming Windows `16/16`. A 2026-06-18 retry first refreshed the
  Windows release `zmin.exe`, then started detached `upstream-fast exhaustive`,
  but the artifacts
  `C:\Users\skron\zmin-upstream-20260618T191751Z-77416-out` and
  `C:\Users\skron\zmin-upstream-20260618T192030Z-78138-out` stopped without an
  `upstream-runner.exit` sentinel after delayed `upstream-compat` /
  compat-profile runner contention. Do not count those retries as integrated
  Windows evidence. The runner now fail-fast blocks a new upstream start while
  another `ZminUpstream-*` task/process is active; validation covered a dummy
  `ZminUpstream-guard-probe` refusal and a real Windows smoke at
  `C:\Users\skron\zmin-upstream-20260618T193333Z-95929-out` (`1/1`).
  A clean post-guard integrated retry at
  `C:\Users\skron\zmin-upstream-20260618T194759Z-8937-out` used a freshly
  rebuilt release `zmin.exe` (`2026-06-18 21:47:36`) and reached `t0008`, but
  stopped before `upstream-runner.exit` after Git-for-Windows/MSYS reported
  `couldn't create signal pipe` / Win32 error `5`; keep it inconclusive.
  A stock-Git control quick replay at
  `C:\Users\skron\zmin-upstream-20260618T195534Z-22444-out` likewise passed
  `t0001`/`t0002`, started `t0008`, then lost the scheduled task before
  `upstream-runner.exit` or a `t0008` TAP log, so the blocker is harness/MSYS
  lifecycle stability rather than a confirmed Zmin assertion.
  Focused `t0008` standalone reruns keep that classification. The macOS Zmin
  standalone run selected by `tools/git-upstream-compat-tests-ignores.txt`
  passed `1/1` at
  `/var/folders/l3/y2d_2zz51z731b86_sstzz0h0000gn/T//zmin-upstream-compat.JrrUMq/summary.tsv`.
  Windows explicit `build-release` then produced
  `C:\Users\skron\zmin-target\release\zmin.exe` (`7,361,536` bytes,
  `2026-06-18 22:15:57`), but detached Zmin standalone `t0008` at
  `C:\Users\skron\zmin-upstream-20260618T201604Z-58224-out` and detached
  stock-Git standalone `t0008` at
  `C:\Users\skron\zmin-upstream-20260618T201750Z-59901-out` both lost the
  scheduled task before `upstream-runner.exit`; each artifact contains only a
  header summary and zero-byte `t0008-ignores.log`. `upstream-poll` now reports
  those missing-task/no-sentinel artifact inventories directly.
  The cleanup regex no longer deletes the shared
  `C:\Users\skron\zmin-target` cache; after a fresh `build-release`
  (`C:\Users\skron\zmin-build-20260618T202339Z-69008-out`, `zmin.exe`
  `7,379,456` bytes, `2026-06-18 22:32:21`), bounded Windows Zmin `t0008`
  passed `--run=1-50`, `1-100`, `1-200`, and `1-300` at
  `C:\Users\skron\zmin-upstream-20260618T203227Z-78826-out`,
  `C:\Users\skron\zmin-upstream-20260618T203336Z-79428-out`,
  `C:\Users\skron\zmin-upstream-20260618T203535Z-80785-out`, and
  `C:\Users\skron\zmin-upstream-20260618T203756Z-82113-out`. Wider bounded
  `1-306`, `1-312`, `1-325`, `1-350`, and `1-398` retries are inconclusive:
  most show the same no-sentinel/header-only/zero-byte-log lifecycle pattern,
  while `1-306` wrote a `t0008-ignores.log` with Git-for-Windows/MSYS `sed`
  fork/signal-pipe errors before the parent task disappeared. Current accepted
  Windows `t0008` bounded coverage stops at `1-300`. A stock-Git control for
  the same `1-306` bounded selector at
  `C:\Users\skron\zmin-upstream-20260618T205844Z-14158-out` also stopped before
  `upstream-runner.exit` with header-only summary and zero-byte
  `t0008-ignores.log`.
- targeted `t0021-conversion.sh` is green on macOS after fixes for `:path`
  blob objectish resolution, ident clean/smudge canonicalization, `eol=crlf`,
  one-shot and long-running clean/smudge filters, `%f` shell quoting, required
  missing clean/smudge errors, BrokenPipe-tolerant filters, and
  `checkout-index --prefix` smudge behavior
- targeted `t0027-auto-crlf.sh` is green on macOS for the
  Windows-sensitive newline/autocrlf burn-down after fixes for
  `ls-files --eol -o` text/binary classification, `commit .` root pathspec
  handling, config-driven CRLF clean/warnings for `add`, checkout CRLF output
  conversion, `text=auto eol=crlf` mixed/binary preservation, and
  `ls-files --eol` implicit `text` display for bare `eol=lf/crlf`
  attributes. Failure progression: `1362/2600` to `1361/2600`, `361/2600`,
  `257/2600`, `252/2600`, `104/2600`, `8/2600`, then pass. Latest summary:
  `/var/folders/l3/y2d_2zz51z731b86_sstzz0h0000gn/T//zmin-upstream-compat.QV7Slc/summary.tsv`.
  Windows/Git-for-Windows focused regression proof passed for the affected
  `ls-files --eol`, `-text eol=crlf`, and `text=auto eol=crlf` checkout
  cases. A fresh Windows upstream replay now rebuilds `zmin.exe` before the
  suite and moved past the earlier `commit files attr=text` warning mismatch
  after the `TextCrlf` warning-priority fix. Windows/Git-for-Windows clean
  full-file replay is now green at
  `C:\Users\skron\zmin-upstream-20260618T174128Z-51900-out` with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..2600`, and cleanup ending at
  `tasks=0`, `procs=0`; broader Git compatibility still depends on the other
  upstream files and scenario gates.
  Scheduled Task execution now avoids the earlier duplicate auto-trigger,
  and direct scheduled `t0027-auto-crlf.sh --run=1 -q` passed at
  `C:\Users\skron\zmin-direct-scheduled-run1b`. The latest clean full runner
  attempt produced real TAP failures at
  `C:\Users\skron\zmin-upstream-20260617T210250Z-2150-out`, around assertions
  `532`, `537-542`, and `549-553`, but the parent suite still stopped before
  writing `upstream-runner.exit`. Runner cleanup now also stops stale host-side
  `prlctl exec` sessions for old `t0027`/`zmin-*` direct runs; the latest
  cleanup probe finished with `tasks=0` and `procs=0`. A clean Windows upstream
  micro-replay with `t0027-auto-crlf.sh --run=1-2 -q` completed through the
  runner at `C:\Users\skron\zmin-upstream-20260617T212507Z-32151-out`, wrote
  `upstream-runner.exit=0`, and recorded `passed=1` in `summary.tsv`. A clean
  patched runner micro-replay with the logged upstream build path also passed at
  `C:\Users\skron\zmin-upstream-20260617T220659Z-61903-out`
  (`upstream-runner.exit=0`, `passed=1`, `failed=0`), after
  `tools/parallels-windows-runner.sh build-release` produced
  `C:\Users\skron\zmin-target\release\zmin.exe` (`7348736` bytes). The patched
  full run at `C:\Users\skron\zmin-upstream-20260617T220311Z-58174-out` and
  bounded Zmin range `--run=520-560 -v` at
  `C:\Users\skron\zmin-upstream-20260617T221456Z-66776-out` still stopped before
  the parent wrote `upstream-runner.exit`, leaving header-only summaries. Stock
  Git control for the same bounded range at
  `C:\Users\skron\zmin-upstream-20260617T221602Z-68046-out` was inconclusive:
  it reached TAP skip output around assertion `366` and then stalled without a
  sentinel. The known frontier block now has focused cross-platform local proof:
  `checkout_core_autocrlf_false_core_eol_lf_text_attribute_matrix_matches_stock_git`
  passed on macOS (`cargo test -p zmin-cli --test git_worktree_state_compat ...`,
  `1/1`) and Windows/Git-for-Windows
  (`ZMIN_WINDOWS_VALIDATE_NO_FMT=1 tools/parallels-windows-runner.sh validate
  targeted git_worktree_state_compat ...`, `1/1`). The full
  `git_worktree_state_compat` file also passed on macOS (`26/26`) and
  Windows/Git-for-Windows (`ZMIN_WINDOWS_VALIDATE_NO_FMT=1
  tools/parallels-windows-runner.sh validate file git_worktree_state_compat`,
  `26/26`). A clean full or accepted chunked Windows replay remains required
  before this is counted as complete cross-platform upstream file parity.
  Follow-up bounded-run harness proof is now accepted for small chunks:
  `ZMIN_UPSTREAM_BOUNDED_RUN=1` stops upstream `test-lib.sh` after the max
  numeric `--run` selector, local stock Git control ended at `1..2` in
  `/var/folders/l3/y2d_2zz51z731b86_sstzz0h0000gn/T//zmin-upstream-compat.xqPouU/summary.tsv`,
  and Parallels Windows stock/Zmin controls ended at `1..2` with
  `upstream-runner.exit=0` in
  `C:\Users\skron\zmin-upstream-20260617T223731Z-20089-out` and
  `C:\Users\skron\zmin-upstream-20260617T223848Z-20422-out`. This proves the
  chunking mechanism, not full `t0027`; isolated ranges still need setup-aware
  chunks or comparable stock Git controls. A setup-aware `--run=1-560 -q` chunk
  now passes on macOS for stock Git and Zmin, and on Windows for stock Git at
  `C:\Users\skron\zmin-upstream-20260617T224512Z-35957-out` (`1..560`). Earlier
  attached Windows Zmin `1-560` and `1-315` attempts stopped before sentinel
  with header-only summaries, so they are runner/Parallels instability evidence
  rather than accepted behavior results. The runner no-sentinel cleanup path now removes
  non-running stale tasks/processes; a full or accepted Windows Zmin chunk still
  remains required for this part of `t0027`. After a clean VM restart, attached
  stock micro-run output was valid but the host wrapper still returned `127`;
  detached/poll stock micro-run at
  `C:\Users\skron\zmin-upstream-20260617T231125Z-60086-out` returned `0`, TAP
  `1..2`, and cleaned `tasks=0` / `procs=0`. Prefer detached/poll for the next
  Windows chunks. The setup-aware Windows Zmin `--run=1-560 -q` chunk then
  passed through detached/poll at
  `C:\Users\skron\zmin-upstream-20260617T231344Z-62525-out` using
  `ZMIN_BIN=/c/Users/skron/zmin-target/release/zmin.exe`, with
  `upstream-runner.exit=0`, `passed=1`, and TAP `1..560`. Detached Windows
  stock Git also passed `--run=1-700 -q` at
  `C:\Users\skron\zmin-upstream-20260617T232529Z-87470-out` (`1..700`), and
  detached Windows Zmin passed the narrower `--run=1-620 -q` chunk at
  `C:\Users\skron\zmin-upstream-20260617T234902Z-97648-out` using
  `ZMIN_BIN=/c/Users/skron/zmin-target/release/zmin.exe`, with
  `upstream-runner.exit=0`, `passed=1`, and TAP `1..620`. A narrower follow-up
  detached Windows Zmin `--run=1-635 -q` chunk also passed at
  `C:\Users\skron\zmin-upstream-20260618T000446Z-6436-out` with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..635`, and cleanup ending at
  `tasks=0`, `procs=0`. A later clean detached Windows Zmin prefix advanced the
  accepted frontier to `--run=1-850 -q` at
  `C:\Users\skron\zmin-upstream-20260617T233435Z-90976-out`, with
  `upstream-runner.exit=0`, `passed=1`, and TAP `1..850`. A clean follow-up
  split advanced the accepted frontier to `--run=1-875 -q` at
  `C:\Users\skron\zmin-upstream-20260618T002019Z-26048-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..875`, and cleanup ending at
  `tasks=0`, `procs=0`. A follow-up clean split advanced it to
  `--run=1-888 -q` at
  `C:\Users\skron\zmin-upstream-20260618T003134Z-32543-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..888`, and cleanup ending at
  `tasks=0`, `procs=0`. A subsequent clean retry advanced the accepted frontier
  to `--run=1-894 -q` at
  `C:\Users\skron\zmin-upstream-20260618T004310Z-40055-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..894`, and cleanup ending at
  `tasks=0`, `procs=0`. Follow-up clean splits advanced the accepted frontier
  to `--run=1-897 -q` at
  `C:\Users\skron\zmin-upstream-20260618T005056Z-45629-out` (`1..897`), then to
  `--run=1-900 -q` at
  `C:\Users\skron\zmin-upstream-20260618T005853Z-51322-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..900`, and cleanup ending at
  `tasks=0`, `procs=0`, and then to `--run=1-925 -q` at
  `C:\Users\skron\zmin-upstream-20260618T010923Z-62781-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..925`, and cleanup ending at
  `tasks=0`, `procs=0`, then to `--run=1-950 -q` at
  `C:\Users\skron\zmin-upstream-20260618T011831Z-68910-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..950`, and cleanup ending at
  `tasks=0`, `procs=0`, then to `--run=1-1000 -q` at
  `C:\Users\skron\zmin-upstream-20260618T012842Z-70601-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..1000`, and cleanup ending at
  `tasks=0`, `procs=0`, then to `--run=1-1040 -q` at
  `C:\Users\skron\zmin-upstream-20260618T014147Z-81896-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..1040`, and cleanup ending at
  `tasks=0`, `procs=0`, then to `--run=1-1070 -q` at
  `C:\Users\skron\zmin-upstream-20260618T015516Z-84211-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..1070`, and cleanup ending at
  `tasks=0`, `procs=0`, then to `--run=1-1200 -q` at
  `C:\Users\skron\zmin-upstream-20260618T020544Z-344-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..1200`, and cleanup ending at
  `tasks=0`, `procs=0`, then to `--run=1-1250 -q` at
  `C:\Users\skron\zmin-upstream-20260618T022223Z-7616-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..1250`, and cleanup ending at
  `tasks=0`, `procs=0`, then to `--run=1-1275 -q` at
  `C:\Users\skron\zmin-upstream-20260618T023429Z-13076-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..1275`, and cleanup ending at
  `tasks=0`, `procs=0`, then to `--run=1-1288 -q` at
  `C:\Users\skron\zmin-upstream-20260618T024856Z-20561-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..1288`, and cleanup ending at
  `tasks=0`, `procs=0`, then to `--run=1-1294 -q` at
  `C:\Users\skron\zmin-upstream-20260618T030032Z-28814-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..1294`, and cleanup ending at
  `tasks=0`, `procs=0`, then to `--run=1-1295 -q` at
  `C:\Users\skron\zmin-upstream-20260618T031356Z-40205-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..1295`, and cleanup ending at
  `tasks=0`, `procs=0`, then to `--run=1-1296 -q` at
  `C:\Users\skron\zmin-upstream-20260618T032527Z-55624-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..1296`, and cleanup ending at
  `tasks=0`, `procs=0`, then to manual clean `--run=1-1297 -q` retry at
  `C:\Users\skron\zmin-upstream-20260618T033909Z-63329-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..1297`, and cleanup ending at
  `tasks=0`, `procs=0`, then to `--run=1-1300 -q` at
  `C:\Users\skron\zmin-upstream-20260618T035820Z-80453-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..1300`, and cleanup ending at
  `tasks=0`, `procs=0`, then to `--run=1-1312 -q` at
  `C:\Users\skron\zmin-upstream-20260618T041817Z-93781-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..1312`, and cleanup ending at
  `tasks=0`, `procs=0`, then to delayed `--run=1-1325 -q` at
  `C:\Users\skron\zmin-upstream-20260618T043809Z-6476-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..1325`, and cleanup ending at
  `tasks=0`, `procs=0`, then to clean `--run=1-1350 -q` retry at
  `C:\Users\skron\zmin-upstream-20260618T045357Z-18050-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..1350`, and cleanup ending at
  `tasks=0`, `procs=0`, then to `--run=1-1375 -q` at
  `C:\Users\skron\zmin-upstream-20260618T050651Z-29377-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..1375`, and cleanup ending at
  `tasks=0`, `procs=0`, then to `--run=1-1400 -q` at
  `C:\Users\skron\zmin-upstream-20260618T051953Z-36712-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..1400`, and cleanup ending at
  `tasks=0`, `procs=0`, then to `--run=1-1425 -q` at
  `C:\Users\skron\zmin-upstream-20260618T053502Z-49040-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..1425`, and cleanup ending at
  `tasks=0`, `procs=0`, then to a clean `--run=1-1450 -q` retry at
  `C:\Users\skron\zmin-upstream-20260618T055336Z-73874-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..1450`, and cleanup ending at
  `tasks=0`, `procs=0`, then to a clean `--run=1-1475 -q` retry at
  `C:\Users\skron\zmin-upstream-20260618T061631Z-90649-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..1475`, and cleanup ending at
  `tasks=0`, `procs=0`, then to `--run=1-1500 -q` at
  `C:\Users\skron\zmin-upstream-20260618T063008Z-9868-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..1500`, and cleanup ending at
  `tasks=0`, `procs=0`, then to `--run=1-1525 -q` at
  `C:\Users\skron\zmin-upstream-20260618T064247Z-27409-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..1525`, and cleanup ending at
  `tasks=0`, `procs=0`, then to `--run=1-1550 -q` at
  `C:\Users\skron\zmin-upstream-20260618T065647Z-41899-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..1550`, and cleanup ending at
  `tasks=0`, `procs=0`, then to `--run=1-1575 -q` at
  `C:\Users\skron\zmin-upstream-20260618T071034Z-51080-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..1575`, and cleanup ending at
  `tasks=0`, `procs=0`, then to `--run=1-1600 -q` at
  `C:\Users\skron\zmin-upstream-20260618T072406Z-63964-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..1600`, and cleanup ending at
  `tasks=0`, `procs=0`, then to `--run=1-1625 -q` at
  `C:\Users\skron\zmin-upstream-20260618T074102Z-81140-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..1625`, and cleanup ending at
  `tasks=0`, `procs=0`, then to a clean `--run=1-1650 -q` retry at
  `C:\Users\skron\zmin-upstream-20260618T080117Z-240-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..1650`, and cleanup ending at
  `tasks=0`, `procs=0`, then to `--run=1-1675 -q` at
  `C:\Users\skron\zmin-upstream-20260618T082115Z-13584-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..1675`, and cleanup ending at
  `tasks=0`, `procs=0`, then to `--run=1-1700 -q` at
  `C:\Users\skron\zmin-upstream-20260618T084253Z-22306-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..1700`, and cleanup ending at
  `tasks=0`, `procs=0`, then to `--run=1-1725 -q` at
  `C:\Users\skron\zmin-upstream-20260618T090236Z-43539-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..1725`, and cleanup ending at
  `tasks=0`, `procs=0`, then to `--run=1-1750 -q` at
  `C:\Users\skron\zmin-upstream-20260618T092047Z-61541-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..1750`, and cleanup ending at
  `tasks=0`, `procs=0`, then to `--run=1-1775 -q` at
  `C:\Users\skron\zmin-upstream-20260618T093838Z-75531-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..1775`, and cleanup ending at
  `tasks=0`, `procs=0`, then to `--run=1-1800 -q` at
  `C:\Users\skron\zmin-upstream-20260618T100508Z-263-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..1800`, and cleanup ending at
  `tasks=0`, `procs=0`, then to `--run=1-1825 -q` at
  `C:\Users\skron\zmin-upstream-20260618T102240Z-9595-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..1825`, and cleanup ending at
  `tasks=0`, `procs=0`, then to `--run=1-1850 -q` at
  `C:\Users\skron\zmin-upstream-20260618T104030Z-25358-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..1850`, and cleanup ending at
  `tasks=0`, `procs=0`, then to `--run=1-1875 -q` at
  `C:\Users\skron\zmin-upstream-20260618T105728Z-32384-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..1875`, and cleanup ending at
  `tasks=0`, `procs=0`, then to `--run=1-1900 -q` at
  `C:\Users\skron\zmin-upstream-20260618T111511Z-49378-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..1900`, and cleanup ending at
  `tasks=0`, `procs=0`, then to `--run=1-1925 -q` at
  `C:\Users\skron\zmin-upstream-20260618T113825Z-69888-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..1925`, and cleanup ending at
  `tasks=0`, `procs=0`, then to `--run=1-1950 -q` at
  `C:\Users\skron\zmin-upstream-20260618T115401Z-80980-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..1950`, and cleanup ending at
  `tasks=0`, `procs=0`, then to `--run=1-1975 -q` at
  `C:\Users\skron\zmin-upstream-20260618T121220Z-97816-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..1975`, and cleanup ending at
  `tasks=0`, `procs=0`, then to `--run=1-2050 -q` at
  `C:\Users\skron\zmin-upstream-20260618T123448Z-16338-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..2050`, and cleanup ending at
  `tasks=0`, `procs=0`, then to `--run=1-2100 -q` at
  `C:\Users\skron\zmin-upstream-20260618T125156Z-29217-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..2100`, and cleanup ending at
  `tasks=0`, `procs=0`, then to `--run=1-2150 -q` at
  `C:\Users\skron\zmin-upstream-20260618T131021Z-43572-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..2150`, and cleanup ending at
  `tasks=0`, `procs=0`, then to `--run=1-2200 -q` at
  `C:\Users\skron\zmin-upstream-20260618T133232Z-64811-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..2200`, and cleanup ending at
  `tasks=0`, `procs=0`, then to `--run=1-2250 -q` at
  `C:\Users\skron\zmin-upstream-20260618T135252Z-75703-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..2250`, and cleanup ending at
  `tasks=0`, `procs=0`, then to `--run=1-2300 -q` at
  `C:\Users\skron\zmin-upstream-20260618T141651Z-99429-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..2300`, and cleanup ending at
  `tasks=0`, `procs=0`, then to `--run=1-2350 -q` at
  `C:\Users\skron\zmin-upstream-20260618T143554Z-15165-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..2350`, and cleanup ending at
  `tasks=0`, `procs=0`, then to `--run=1-2375 -q` at
  `C:\Users\skron\zmin-upstream-20260618T145804Z-30606-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..2375`, and cleanup ending at
  `tasks=0`, `procs=0`, then to `--run=1-2400 -q` at
  `C:\Users\skron\zmin-upstream-20260618T152301Z-50906-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..2400`, and cleanup ending at
  `tasks=0`, `procs=0`, then to `--run=1-2450 -q` at
  `C:\Users\skron\zmin-upstream-20260618T154504Z-70558-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..2450`, and cleanup ending at
  `tasks=0`, `procs=0`, then to `--run=1-2500 -q` at
  `C:\Users\skron\zmin-upstream-20260618T160926Z-95571-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..2500`, and cleanup ending at
  `tasks=0`, `procs=0`, then to `--run=1-2550 -q` at
  `C:\Users\skron\zmin-upstream-20260618T163810Z-10494-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..2550`, and cleanup ending at
  `tasks=0`, `procs=0`, then to `--run=1-2600 -q` at
  `C:\Users\skron\zmin-upstream-20260618T170832Z-26268-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..2600`, and cleanup ending at
  `tasks=0`, `procs=0`, then to a clean full-file replay at
  `C:\Users\skron\zmin-upstream-20260618T174128Z-51900-out`, with
  `upstream-runner.exit=0`, `passed=1`, TAP `1..2600`, and cleanup ending at
  `tasks=0`, `procs=0`; the earlier `--run=1-700 -q` prefix also passed at
  `C:\Users\skron\zmin-upstream-20260617T232529Z-87470-out`. Full Windows
  `t0027` is now counted green. Older `1-900`, previous `1-950`, `1-938`,
  previous `1-1040`, `1-1100`, `1-1297`, and `1-1300` attempts stopped before
  sentinel, disappeared, or stayed Ready with header-only/zero-byte logs; the
  stale `1-1297` retry at
  `C:\Users\skron\zmin-upstream-20260618T033549Z-57583-out` had no sentinel,
  header-only summary, zero-byte TAP log, empty stderr, and cleanup at
  `tasks=0`, `procs=0`. A clean `1-1300` retry at
  `C:\Users\skron\zmin-upstream-20260618T035316Z-74900-out` started Running
  after accepted `1-1297`, then disappeared without sentinel with a header-only
  summary, zero-byte TAP log, empty stderr, and a clean guest probe. A queued
  earlier `1-1325` artifact at
  `C:\Users\skron\zmin-upstream-20260618T041641Z-88144-out` only reached
  Scheduled Task `Ready` state with a header-only summary, zero-byte TAP log,
  empty stderr, and no sentinel before cleanup; the delayed clean `1-1325`
  retry later passed. A queued `1-1350` artifact at
  `C:\Users\skron\zmin-upstream-20260618T045047Z-12532-out` also only reached
  Scheduled Task `Ready` state with a header-only summary, zero-byte TAP log,
  empty stderr, and no sentinel before cleanup. A `1-1450` attempt at
  `C:\Users\skron\zmin-upstream-20260618T054709Z-62165-out` also reached
  Scheduled Task `Ready` state without `upstream-runner.exit`, with only the
  `summary.tsv` header and no TAP tail before cleanup; it is superseded by the
  later clean `1-1450` retry. A `1-1475` attempt at
  `C:\Users\skron\zmin-upstream-20260618T061048Z-83023-out` likewise reached
  Scheduled Task `Ready` state without `upstream-runner.exit`, with only the
  `summary.tsv` header and no TAP tail before cleanup; the accepted frontier
  remained `1-1450` until the later clean `1-1475` retry. A clean retry at
  `C:\Users\skron\zmin-upstream-20260618T061608Z-90369-out` lost its Scheduled
  Task while MSYS child processes continued, then ended without
  `upstream-runner.exit`, with only the `summary.tsv` header and no TAP tail;
  this is also lifecycle noise, not accepted product evidence, and is
  superseded by the later clean `1-1475` retry, which is now superseded by the
  clean `1-1625` retry. A queued `1-1650` artifact at
  `C:\Users\skron\zmin-upstream-20260618T075633Z-93328-out` reached Scheduled
  Task `Ready` state without accepted sentinel/TAP evidence after the clean
  `1-1625` pass; it is superseded by the later clean `1-1650` retry. An
  earlier `1-894` attempt at
  `C:\Users\skron\zmin-upstream-20260618T004040Z-38931-out` left
  `tasks=0`, `procs=0`. Earlier wider retries were contaminated by host-side
  queued polls plus stray smaller delayed chunks (`1-620`, `1-635`, `1-642`,
  `1-650`). An attempted `1-2050` preflight start at
  `C:\Users\skron\zmin-upstream-20260618T122927Z-9649` stopped before upstream
  execution when the Windows release build returned exit `-1`; the accepted
  retry reused the unchanged release binary after docs/knowledge-only edits.
  The `1-2200` launch printed a host-side `Canceling the job/session` message
  before detached output, but the guest Scheduled Task existed, was polled
  directly, and produced accepted summary/sentinel/TAP evidence. A stale
  `1-2400` artifact at
  `C:\Users\skron\zmin-upstream-20260618T145616Z-29241-out` reached Scheduled
  Task `Ready` state with no sentinel, a header-only summary, and a zero-byte
  TAP log after the clean `1-2350` pass; cleanup then confirmed `tasks=0`,
  `procs=0`, and it is not accepted evidence. Treat the clean full-file replay
  at `C:\Users\skron\zmin-upstream-20260618T174128Z-51900-out` as the accepted
  Windows `t0027` file signoff; the stale lifecycle starts remain runner noise,
  not Zmin failures.
- targeted `t0000-basic.sh` is green on macOS after fixes for basic harness
  invocation behavior, tree/index plumbing, stale-stat `diff-files` /
  `diff-index`, raw commit display, duplicate `commit-tree` parents, and
  `update-index` D/F conflict handling
- targeted `t2020-checkout-detach.sh` is green on macOS after fixes for
  detached HEAD state transitions, detached-head advice/output, orphan warnings,
  checkout branch upstream tracking output, and `checkout --orphan` avoiding
  zero-new-id HEAD reflog entries
- targeted `t4013-diff-various.sh` is green on macOS after fixes for merge diff
  modes, `diff-tree --stdin`, multi-commit `show` pathspecs, `-I`/blank-line
  filtering across patch/stat/raw/name formats, conflicted-index raw/name
  metadata, file-to-directory worktree diff handling, stat-only refresh
  behavior, `diff.noPrefix`, post-checkout index stat metadata refresh, and
  default `git show <empty-root-ref>` separator handling when the root diff has
  no entries. Latest targeted proof:
  `/tmp/zmin-macos-t4013-afterfix-20260618T183410Z/summary.tsv`.
  Windows/Git-for-Windows targeted CLI regression proof also passed via
  `ZMIN_WINDOWS_VALIDATE_NO_FMT=1 tools/parallels-windows-runner.sh validate targeted git_history_query_compat show_empty_root_commit_does_not_print_empty_patch_separator`
  in guest copy `C:\Users\skron\zmin-20260618T191006Z-64719` (`1/1`).
  The broader current dirty-worktree macOS integrated rerun
  `/tmp/zmin-macos-current-exhaustive-afterfix-20260618T183428Z/summary.tsv`
  stopped with exit `143` at `t0027-auto-crlf.sh` before writing TAP output for
  that file, so it is not accepted as a refreshed 16-file proof.
- targeted `t5510-fetch.sh` supported-surface burn-down is green with
  `ZMIN_UPSTREAM_SKIP_UNSUPPORTED_REFTABLE=1`; coverage now includes direct
  URL `fetch --tags file://<repo>`, smart HTTP `git-http-backend` ScriptAlias
  execution, non-bare smart HTTP export, multiple explicit smart HTTP tag
  refspecs under `protocol.version=2`, configured fetch, `FETCH_HEAD`,
  `remote.origin.followRemoteHEAD`, prune, namespace prune, and overlapping
  wildcard refspec prune; Windows coverage includes Git-for-Windows
  `file:///c/...` local repository URLs and D/F remote-tracking ref conflicts
  returning the stock prune hint; focused smart-HTTP incremental fetch coverage
  now also verifies thin packs with ref-delta bases that already exist in the
  client repository on macOS and Windows/Git-for-Windows; focused smart-HTTP
  noop coverage verifies that a second fetch with all advertised roots already
  present locally preserves refs and avoids a redundant upload-pack request on
  macOS and Windows/Git-for-Windows
- targeted `t1410-reflog.sh` is green on macOS after fixes for fast-import
  reflog creation, branch/HEAD reflog recording, reflog delete/drop, expire
  no-op/stale/timestamp handling, linked-worktree reflog listing, reflog
  pattern config, `log -g --branches=<glob> --format=%gD`, and `log -g`
  ordinal behavior around hidden zero-new-id entries
- current detailed baseline: `docs/git/upstream_compatibility_baseline.md`

## Repository-state proof

The repository handoff proof lives primarily in:

- `crates/zmin-cli/tests/git_repository_state_compat.rs`
- `crates/zmin-cli/tests/git_worktree_state_compat.rs`
- `crates/zmin-cli/tests/git_transport_local_compat.rs`

These checks cover:

- worktree state
- index state
- refs and reflogs
- loose objects and packfiles
- local clone, fetch, push, and pull handoff

## Provider smoke

Real-repository smoke and provider checks are driven by:

- `tools/git-provider-smoke.sh`
- `tools/git-real-repo-smoke.sh`

They are used as extra proof on top of the local compatibility suites, not as a substitute for them.

## Additive Zmin surface

These commands are additive Zmin porcelain or planning modes. They must not be
counted as upstream Git parity, but they are tracked here because they share the
same canonical `.git` repository state.

- `clone --worktree-first` / `clone --instant` is supported for local
  repositories, smart HTTP remotes, git-daemon remotes, and SSH remotes. The
  remote paths fetch the selected `HEAD` target first, materialize the working
  tree, write only refs for objects they requested, record
  `zmin.worktreeFirst=true`, and let a normal `fetch origin` hydrate
  additional branch and tag refs later.
- `clone --instant --background-fetch` is additive Zmin-only surface for
  remote worktree-first clones. It materializes the selected `HEAD` first, then
  starts a detached `fetch origin` process after checkout and records
  `zmin.worktreeFirstBackgroundFetch=true`,
  `zmin.worktreeFirstBackgroundFetchRemote=origin`, and the spawned pid in
  config. It is explicit opt-in; default `--instant` remains HEAD-only until a
  later foreground `fetch origin`.
- `clone --instant --demand-hydrate` is additive Zmin-only surface for remote
  worktree-first clones over smart HTTP, git-daemon, and SSH. It records
  `remote.origin.promisor=true`, `zmin.worktreeFirstDemandHydrate=true`, and
  `zmin.worktreeFirstDemandHydrateRemote=origin`, then lets promisor object
  reads hydrate missing `HEAD` objects on demand. It is explicit opt-in; default
  `--instant` remains non-promisor.
- The background-fetch slice also fixed remote fetch update ordering for
  non-depth HTTP, git-daemon, and SSH fetches: refs are written only after
  objects are hydrated, so failed/background fetches do not leave bad
  remote-tracking refs.
- Promisor-only demand hydration now covers ordinary `cat-file` object reads:
  when a repository has `remote.<name>.promisor=true` and a requested object is
  missing locally, `cat-file -t <object>` and typed-object reads hydrate that
  object from a local or HTTP promisor remote before retrying the read. Normal
  repositories without promisor remotes keep the previous missing-object
  behavior.
- `zmin hooks` is additive Zmin-managed hook porcelain over standard Git hook
  files. It supports `hooks init`, `hooks add [--force] <hook> <command>`,
  `hooks list`, and `hooks remove <hook>` for `pre-commit`, `commit-msg`,
  `pre-push`, `post-checkout`, and `post-merge`. It stores multi-value commands
  in `.git/config` under `zmin.hooks.<hook>`, generates an executable
  `.git/hooks/<hook>` shell runner, preserves manual hooks unless `--force` is
  requested, forwards normal hook arguments, and stops at the first failing
  managed command.
- CMS-like Zmin porcelain is additive and currently covers `save`, `changes`,
  `publish`, `update`, `undo`, `timeline`, and `recover`. These commands compose
  existing Git-compatible repository operations (`add`, `commit`, `status`,
  `push`, `pull --ff-only`, `reset`, `log`, and `restore`) while preserving the
  canonical `.git` state and refusing unsafe dirty-worktree cases for remote or
  undo operations.
- Validation for managed hooks and CMS porcelain: macOS `cargo test -p zmin-cli
  --test git_admin_tools_compat hook -- --nocapture` (`2/2`), macOS
  `cargo test -p zmin-cli --test git_cms_porcelain_compat -- --nocapture`
  (`4/4`), Windows/Git-for-Windows
  `ZMIN_WINDOWS_VALIDATE_NO_FMT=1 tools/parallels-windows-runner.sh validate
  targeted git_admin_tools_compat
  managed_hooks_add_list_remove_and_protect_manual_hooks` (`1/1`), and
  Windows/Git-for-Windows
  `ZMIN_WINDOWS_VALIDATE_NO_FMT=1 tools/parallels-windows-runner.sh validate
  file git_cms_porcelain_compat` (`4/4`).
- Validation for the smart HTTP, git-daemon, and SSH slices:
  `cargo fmt --all -- --check`,
  `cargo test -p zmin-cli --test git_transport_http_compat
  clone_instant_smart_http_materializes_head_then_fetch_hydrates_refs --
  --nocapture`, `cargo test -p zmin-cli --test git_transport_http_compat
  clone_reads_smart_http_pack_like_stock_git -- --nocapture`, `cargo test -p
  zmin-cli --test git_transport_http_compat
  clone_instant_git_daemon_materializes_head_then_fetch_hydrates_refs --
  --nocapture`, `cargo test -p zmin-cli --test git_transport_http_compat
  clone_reads_git_daemon_remote_like_stock_git -- --nocapture`, `cargo test -p
  zmin-cli --test git_transport_http_compat
  clone_instant_ssh_materializes_head_then_fetch_hydrates_refs -- --nocapture`,
  `cargo test -p zmin-cli --test git_transport_http_compat
  clone_reads_ssh_remote_like_stock_git -- --nocapture`, `cargo test -p
  zmin-cli --test git_clone_compat
  clone_worktree_first_rejects_non_worktree_or_remote_modes -- --nocapture`,
  Windows/Git-for-Windows `tools/parallels-windows-runner.sh validate targeted
  git_transport_http_compat
  clone_instant_smart_http_materializes_head_then_fetch_hydrates_refs`, and
  Windows/Git-for-Windows `tools/parallels-windows-runner.sh validate targeted
  git_transport_http_compat
  clone_instant_git_daemon_materializes_head_then_fetch_hydrates_refs`, and
  Windows/Git-for-Windows `tools/parallels-windows-runner.sh validate targeted
  git_transport_http_compat
  clone_instant_ssh_materializes_head_then_fetch_hydrates_refs`, and
  Windows/Git-for-Windows `tools/parallels-windows-runner.sh validate targeted
  git_clone_compat clone_worktree_first_rejects_non_worktree_or_remote_modes`.
- Validation for the explicit background-fetch slice: macOS `cargo test -p
  zmin-cli --test git_transport_http_compat
  clone_instant_smart_http_background_fetch_hydrates_refs -- --nocapture`,
  `cargo test -p zmin-cli --test git_transport_http_compat
  clone_instant_git_daemon_background_fetch_hydrates_refs -- --nocapture`, and
  `cargo test -p zmin-cli --test git_transport_http_compat
  clone_instant_ssh_background_fetch_hydrates_refs -- --nocapture`,
  macOS `cargo test -p zmin-cli --test git_transport_http_compat fetch_ --
  --nocapture` (`26/26`), Windows/Git-for-Windows
  `tools/parallels-windows-runner.sh validate targeted
  git_transport_http_compat
  clone_instant_smart_http_background_fetch_hydrates_refs`,
  Windows/Git-for-Windows
  `tools/parallels-windows-runner.sh validate targeted
  git_transport_http_compat
  clone_instant_git_daemon_background_fetch_hydrates_refs`, and
  Windows/Git-for-Windows `tools/parallels-windows-runner.sh validate targeted
  git_transport_http_compat clone_instant_ssh_background_fetch_hydrates_refs`,
  and
  Windows/Git-for-Windows `tools/parallels-windows-runner.sh validate targeted
  git_transport_http_compat fetch_reads_git_daemon_remote_like_stock_git`.
- Validation for the demand-hydration slice: macOS `cargo fmt --all --
  --check`, macOS `cargo test -p zmin-cli --test git_object_plumbing_compat
  -- --nocapture` (`21/21`), macOS `cargo test -p zmin-cli --test
  git_admin_tools_compat backfill_promisor_remote_recovers_missing_local_objects
  -- --nocapture`, macOS `cargo test -p zmin-cli --test
  git_transport_http_compat clone_instant_ -- --nocapture` (`9/9`), macOS
  `cargo test -p zmin-cli --test git_clone_compat
  clone_worktree_first_rejects_non_worktree_or_remote_modes -- --nocapture`,
  Windows/Git-for-Windows `tools/parallels-windows-runner.sh validate targeted
  git_object_plumbing_compat
  cat_file_promisor_remote_hydrates_missing_blob_on_demand`, and
  Windows/Git-for-Windows `tools/parallels-windows-runner.sh validate targeted
  git_object_plumbing_compat
  show_matches_stock_git_for_raw_commits_trees_blobs_and_tags`,
  Windows/Git-for-Windows `tools/parallels-windows-runner.sh validate targeted
  git_transport_http_compat
  clone_instant_smart_http_demand_hydrate_recovers_missing_head_objects`, and
  Windows/Git-for-Windows `tools/parallels-windows-runner.sh validate targeted
  git_transport_http_compat
  clone_instant_git_daemon_demand_hydrate_recovers_missing_head_objects`,
  Windows/Git-for-Windows `tools/parallels-windows-runner.sh validate targeted
  git_transport_http_compat
  clone_instant_ssh_demand_hydrate_recovers_missing_head_objects`, and
  Windows/Git-for-Windows `tools/parallels-windows-runner.sh validate targeted
  git_clone_compat clone_worktree_first_rejects_non_worktree_or_remote_modes`.
  The same object-plumbing validation fixed root-commit `show HEAD` so
  single-object `show` includes the root patch like stock Git.
- Performance-gate smoke for worktree-first clone now includes local
  `clone-instant`, loopback git-daemon `clone-instant-git-daemon`, and fake-SSH
  `clone-instant-ssh` in `tools/git-performance-bench.sh` and
  `tools/windows-native-benchmark.ps1`, plus smart HTTP `clone-http-instant` in
  `tools/git-http-performance-bench.sh` and `tools/windows-native-http-benchmark.ps1`.
  These gates validate `HEAD`, `HEAD^{tree}`, `zmin.worktreeFirst=true`, and
  `fsck --strict` for remote instant clones.
- Latest one-repeat smoke evidence: macOS
  `/tmp/zmin-remote-instant-bench-one-session-guard-20260617.tsv` has all new
  clone-instant remote checks ok after reusing the advertised upload-pack
  session for SSH and git-daemon instant clones. On this small fixture
  `clone-instant-git-daemon` is Zmin `0.06s` vs Git `0.08s`, and
  `clone-instant-ssh` is Zmin `0.14s` vs Git `0.30s`. Windows/Git-for-Windows
  `C:\Users\zmin\zmin-bench-20260616T233652Z-2239-out` is the refreshed
  three-repeat gate after the one-session optimization. All clone-instant checks
  are ok. `clone-instant-git-daemon` is now faster than Git on mean and median
  (ratio `0.958412` mean, `0.720567` median). `clone-instant-ssh` still has a
  Windows variance gap in this run (ratio `1.123351` mean, `1.326464` median).
  A follow-up phase-trace run
  `C:\Users\zmin\zmin-bench-20260616T234622Z-4223-out` shows no second
  advertisement round trip; remaining time is dominated by checkout index
  materialization plus SSH discovery variance.
- Latest macOS scoped fake-SSH refresh on the current dirty worktree is
  `/tmp/zmin-macos-ssh-current-fair-3x-20260618T180242Z`: all
  `clone-instant-ssh` checks are ok, but Zmin remains slower than Git
  (aggregate mean ratio `1.267754`, aggregate median `1.360408`, paired mean
  `1.318018`, paired median `1.312756`). The companion trace
  `/tmp/zmin-macos-ssh-current-trace-1x-20260618T180304Z` shows
  `cli.process=0.259948s`, `ssh_upload_pack.open.advertisement=0.089031s`,
  `upload_pack.sideband=0.067104s`, and `checkout_fresh.checkout_index=0.042506s`;
  the clone path already reuses the advertised SSH upload-pack session.
- Latest Windows/Git-for-Windows scoped fake-SSH refresh on the same dirty
  worktree is `C:\Users\skron\zmin-bench-20260618T180552Z-70636-out`: all
  `clone-instant-ssh` checks are ok and Zmin stayed faster than Git on the
  one-repeat gate (aggregate and paired ratio `0.955303`; raw rows Git
  `1.949853s`, Zmin `1.862700s`). Fake SSH remote lifetime was Git
  `0.382498s` vs Zmin `0.312930s`.
- Fake-SSH benchmark fixture follow-up fixed an unfair local upload-pack binary
  mismatch by prepending the benchmark Git executable's `git --exec-path` in
  the fake SSH wrapper. Packet traces before the fix showed macOS Git using
  `agent=git/2.50.1-Darwin` while the Zmin row used
  `agent=git/2.53.0-Darwin`; after the fix both macOS rows use
  `agent=git/2.50.1-Darwin`, and the Windows packet trace validates
  `agent=git/2.54.0.windows.1-Windows` for both rows. The clean macOS 10-repeat
  run `/tmp/zmin-macos-ssh-execpath-fair-10x-20260618T181328Z` has all checks
  ok and materially reduces the paired median gap from `1.238333` to
  `1.028492` (aggregate mean `1.074071`, aggregate median `1.004285`, paired
  mean `1.043337`). The clean Windows 3-repeat preservation run
  `C:\Users\skron\zmin-bench-20260618T181638Z-28896-out` has all checks ok and
  keeps Zmin faster than Git (aggregate mean `0.874218`, aggregate median
  `0.805154`, paired mean `0.885576`, paired median `0.959802`). Treat packet
  traces as protocol/tooling evidence only; they perturb stopwatch timings.
- Checkout variance, repeated larger remote worktree-first performance gates,
  and real network/auth/proxy variants remain open.
