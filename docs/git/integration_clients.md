# Client Integration Guide

This repository can be used as a library without shelling out to `git`.

## Minimal model

1. Use `zmin-git-core` for repository-format operations.
2. Use `zmin-primitives::git_runtime` when you need pluggable transport/object/refs/worktree adapters.
3. Keep any custom ids or metadata outside the repository object model.

## Small custom runtime example

```rust
use zmin_primitives::git_runtime::{
    GitObjectEnvelope, GitPrimitiveRuntime, GitPrimitiveRuntimeFactory, GitRuntimeMode,
};
use std::collections::BTreeMap;

let runtime = GitPrimitiveRuntimeFactory::in_memory(GitRuntimeMode::Public);
let blob_id = runtime
    .objects()
    .write_object_content(
        &GitObjectEnvelope {
            id: "0".repeat(40),
            size: 2,
            object_type: "blob".into(),
            metadata: BTreeMap::new(),
        },
        b"ok",
    )
    .expect("write");

assert_eq!(runtime.objects().read_object_content(&blob_id).unwrap(), b"ok");
```

## Practical guidance

- If you need stock Git interoperability, keep repository object ids and on-disk structure in Git-compatible mode.
- If you need custom metadata ids, generate them separately with `zmin_primitives::id`.
- If you need a different hashing policy for application data, apply it outside the `.git` object graph.

## Local Git Replacement Checks

Preview dogfood uses a local `git` shim that dispatches to `zmin`.

On macOS, do not force `GIT_CONFIG_SYSTEM` inside the shim unless it points to
the same system config the stock Git installation uses. Overriding it with a
bundled Dugite-style config changes `git config --null -l` output and can break
client/plugin parity even when the `zmin` binary itself matches stock Git.

Keep these checks green before asking an IDE or GUI client to use the binary:

```bash
zmin --version
zmin version --build-options
git --version
git status
git fetch --prune --no-tags
```

All dated benchmark, compatibility, differential, and replay results below are
retained historical evidence, including later reruns in this document; they are
non-authoritative for current HEAD and do not establish current compatibility,
parity, speed, or RSS claims. Current authoritative claims require the exact
contract and retained evidence described in
[`performance_evidence_contract.md`](performance_evidence_contract.md).

Historical pre-W5 macOS checkpoint (2026-07-17; non-authoritative for current
HEAD): release SHA-256
`f789a0204b00d3e96fff0ad885e34305c99cc8bf572d11a81e94e0c6b069c632`
passes the replacement smoke and the observed-client differential tests.
The 20-repeat real-workspace replay is byte-exact for exit status, stdout, and
stderr across all ten GUI lanes. It uses less p95 RSS in all ten lanes; on this
dirty workspace median wall time is lower in `9/10` lanes, with the unbounded
`log` lane at `1.183x` median and `1.374x` p95 wall time.
In that historical corpus, the normal seven-operation performance result was
below Git for both median time and p95 RSS in all seven operations.

The same historical release replay (20 repeats, `init`, `status`, `log`, `rev-list`,
`merge-base`, `pack-objects`, and `index-pack`) remained semantically exact and
below Git on both gates in all seven lanes; the slowest median-time ratio was
`0.990x` for `index-pack`, and the worst p95 RSS ratio was `0.985x`.

These pre-W5 results are retained for history only and do not establish a
current performance claim. Current authoritative claims require the exact
sampling, equivalence, provenance, and strict-gate contract in
[`performance_evidence_contract.md`](performance_evidence_contract.md).

The version line must start with the Git 2.47 compatibility baseline, currently
`git version 2.47.1.zmin`, and include the real Zmin package version after it. Some
clients reject tools below their minimum Git version before running any other
command.

Run the replacement smoke before local IDE dogfood:

```bash
cargo test -q -p zmin-cli --test git_replacement_dogfood_compat -- --nocapture
tools/git-replacement-dogfood-smoke.sh
```

Historical durable replace-git gate snapshot (2026-07-02; non-authoritative for
current HEAD):

- `cargo test -q -p zmin-cli --test git_runtime_dependency_audit -- --nocapture`
  passed in that snapshot, confirming the audited stock-Git dependency patterns
  remained confined to test-only `src` zones.
- `cargo test -q -p zmin-cli --test git_replacement_dogfood_compat -- --nocapture`
  passed `5/5` in that snapshot; the shell shim smoke, the focused shim-routed
  built-in LFS discovery lane, both stock-style LFS hook takeover lanes
  (default hooks directory and custom `core.hooksPath`), and the publish/pull
  repository-state comparison were green there.
- `cargo test -q -p zmin-cli --test git_observed_client_compat -- --nocapture`
  passed `16/16` in that snapshot; the observed IDE/client command families
  were green on the focused fixture set, including history queries, commit-detail
  `show --stdin` shapes, including the exact fuller/decorated
  `raw + numstat + shortstat` stdin family, Local Changes status/ls-files
  lanes, and poison-git checks that verify the exercised observed commands do
  not shell back out to a stock `git`.

The smoke creates a temporary `git` shim that dispatches to `zmin`, then checks
the IDE-shaped surfaces that usually run first: version probes, build-option
version output, invalid version-option shape, `status -z`, porcelain v2 branch
status, `check-ignore --stdin`, `rev-parse`, `config`,
`cat-file --batch-check`, `ls-files -z`, `diff -z`, `log -z`, observed
`show --stdin` commit-detail families, and
`fetch --prune --no-tags`. It also checks that a `git` shim can answer the
common LFS discovery probes many clients/plugins issue separately:
`git lfs version`, `git lfs env`, empty-repo `git lfs ls-files`,
`git lfs install --manual`,
`git lfs update`, `git lfs update --manual`, invalid-flag shape for
`git lfs update --bogus`, and the standard installed-hook callback entrypoints
`git lfs post-commit`,
`git lfs post-checkout ...`, `git lfs post-merge ...`, and
`git lfs pre-push` stdin-shape validation. It also exercises the practical
upgrade path where a repository already has stock `git-lfs` local hooks and the
shim reruns `git lfs install --local --skip-smudge`, proving Zmin can take over
that existing hook lane instead of treating the hook files as foreign. The Rust
integration tests were the durable gate for CI/local verification in that
snapshot; they also
assert that a local `git` shim routing into Zmin answers `git lfs version`,
`git lfs env`, empty-repo `git lfs ls-files`, and the manual install/update
discovery commands without relying on the shell smoke alone, and that rerunning
`git lfs install --local --skip-smudge` through the shim can take over existing
stock-style hooks in both the default hooks directory and a custom
`core.hooksPath`.
stock-style LFS hook files both under the default `.git/hooks` location and a
custom relative `core.hooksPath`. The shell entry point remains useful for
direct manual dogfood. This is a dogfood gate, not a complete Git or Git LFS
compatibility claim, and the observed-client gate does not widen the supported
parity claim beyond the specific command families modeled in that fixture.

To discover the real command families issued by a GUI instead of guessing them,
configure the GUI's Git executable to
`tools/git-gui-capture-wrapper.py` and set both required variables:

```bash
export ZMIN_GUI_CAPTURE_TARGET=/absolute/path/to/zmin
export ZMIN_GUI_CAPTURE_LOG=/absolute/private/path/gui-git.jsonl
```

The wrapper records a private (`0700` directory, `0600` JSONL) command corpus
and then replaces itself with the selected executable, preserving the original
stdin, stdout, stderr, and exit behavior. Credentials, URL user information and
queries, absolute paths, and sensitive config values are not stored. Parse the
first JSONL rows locally before retaining or sharing a capture.

The safe, read-only subset of a capture can be replayed against both binaries:

```bash
python3 tools/git-gui-capture-replay.py \
  /absolute/private/path/gui-git.jsonl /path/to/repo \
  --stock-git /usr/bin/git --zmin /absolute/path/to/zmin
```

Replay verifies that the supplied repository matches the capture's private root
hash, deduplicates invocations, and compares exit code, stdout, stderr, and a
semantic repository-state snapshot from the same temporary path. It
intentionally skips mutating commands, stdin-driven commands, nested working
directories, and arguments that were redacted or replaced by path placeholders;
those families need an explicit deterministic fixture before they become a
compatibility gate. Mismatch artifacts remain private in the reported output
directory.

Historical live replay on the dirty main workspace (2026-07-14;
non-authoritative for current HEAD) passed `1/1` captured
read-only invocations with identical exit code, stdout, stderr, and semantic
state. The workspace contained an IDE Unix socket; replay materialized that
special file as an inert regular placeholder in the private fixture instead of
failing the repository copy.

When the dogfood gate is green but an IDE still feels slower than stock Git,
measure the exact observed hot lanes on the current repository:

```bash
tools/git-observed-client-bench.sh /path/to/repo
```

The script compares stock Git and the selected `ZMIN_BIN` (otherwise the newest
available local binary) on the current practical
problem lanes:
machine-readable Local Changes `status`, the `.idea`-scoped `ls-files` lane
that drives nested Local Changes trees, commit-details `show --numstat`,
stdin-fed commit-details `show --name-status`, and the unbounded observed
`log --decorate=full ... --date-order --` history query.
The fallback binary selection is exploratory only. For an authoritative run,
set `ZMIN_OBSERVED_BENCH_EVIDENCE_MODE=authoritative`, provide an explicit
release binary with a matching identity sidecar, set
`ZMIN_OBSERVED_BENCH_MAKE=/absolute/trusted/path/make`, and retain the output
directory. The Make path, first version line, and SHA-256 are authenticated at
both start and finish. See
[`performance_evidence_contract.md`](performance_evidence_contract.md) for the
exact identity, sample-count, pairing, metric, and supported-platform
requirements. The current descriptor-bound Make runner supports Linux only;
it opens the sealed Make object read-only through /proc/self/fd before launch.
Darwin and Windows authoritative runs fail closed before even make --version.

The default exploratory run warms each tool/lane pair once before recording
samples, and each measured repeat alternates stock-Git/Zmin execution order.
Authoritative process-cold pairs use fresh child processes while recording
`filesystem_cache=warm` and `filesystem_cache_drop=no-drop`; they are not disk-
cold measurements. A
small process runner
measures wall time at nanosecond resolution and normalizes `ru_maxrss` to bytes
on macOS and Linux. The TSV output and terminal summary report median and p95
wall time plus p95 maximum resident bytes, and optional ratio gates can fail a
regression run. Set
`ZMIN_OBSERVED_BENCH_PHASE_TRACE=1` only for a separate diagnostic run; phase
tracing is excluded from normal timing by default. Those traces can tie a slow
lane back to labels such as
`log.collect_commits`, `show.commit_diff`, or `status.head_index_diff`.
The current trace split for the metadata-only observed `log` path also breaks
the commit-graph hint lane into `commit_graph_hints[_exact_roots].roots`,
`commit_graph_hints[_exact_roots].walk`, and the subsequent
`author_metadata_from_hints` / `render_metadata_from_hints` hydration phase, so
future regressions can distinguish graph traversal cost from commit metadata
materialization cost before changing behavior.

Latest measured rerun on the dirty main workspace as of 2026-07-16, using
release SHA-256
`b0333dfbbda18427b909348ebeeb63025c5a52a4c5a2088c09923d468468b907`:

- A final 20-repeat run of all ten observed client lanes matched stock Git
  exactly for exit status, stdout, and stderr. Median Zmin/Git wall-time ratios
  were `0.429` (`branch --show-current`), `0.428`
  (`config --null --list`), `0.425` (`for-each-ref`), `0.832` (unbounded
  decorated `log`), `0.591` (`ls-files` for `.idea`), `0.466` (`ls-tree`),
  `0.440` (`rev-parse`), `0.819` (`show --numstat`), `0.695` (stdin-fed
  `show --name-status`), and `0.740`
  (machine-readable `status`). No measured observed-client lane has a median
  wall-time regression on this corpus.
- Lightweight schema-equivalent parsers now cover the exact common `show` and
  `ls-files` GUI shapes while deferring unfamiliar or order-sensitive forms to
  the full parser. The `.idea` `ls-files` p95 RSS ratio moved from `1.145x` to
  `1.004x`. The stdin-fed named `show` path streams tree differences, uses a
  transient tree cache, and reads only commit links for the exact `%H %P`
  header; its p95 RSS ratio moved from `1.219x` to `1.056x`.
- Normal pack-index lookup now validates structure without hashing the complete
  `.idx` trailer on every process start, matching stock Git's behavior when
  only that trailer checksum is corrupt. Explicit pack-index decoding and
  `verify-pack` remain checksum-strict. This reduced the dedicated packed
  `show --stdin` p95 RSS ratio from `1.217x` to `1.056x`.
- The unbounded history lane now uses independent mutable pack-reader caches
  per parallel reader while sharing immutable indexes and mappings. Its median
  moved from roughly `49 ms` before that change to `26 ms`; author identities
  are interned per worker to bound retained metadata. Pack-reader buffers are
  configurable and this metadata-only lane uses 512-byte buffers plus bounded
  worker stacks. The latest p95 RSS is `8.68 MB` versus stock Git's `8.54 MB`
  (`1.017x`); the observed multi-pack history tail is much smaller but remains
  a GUI-corpus memory follow-up. In the latest run, `status` and `ls-tree`
  remained below stock Git p95 RSS and `for-each-ref` was equal; the remaining
  short-lived GUI lanes were between `1.015x` and `1.086x`, so GUI-process RSS
  remains an explicit follow-up rather than a passed all-lane memory gate.
  Ordinary oneline history uses a compact
  traversal retaining only id, parents, and subject; on the standard corpus it
  reduced p95 RSS to `5.05 MB` versus stock Git's `5.19 MB`.
- Status attribute lookup now caches the global/system and info attribute
  layers for a scan and only resolves worktree-local layers per path. A new
  differential test proves that the cache preserves global, root, nested, and
  `$GIT_DIR/info/attributes` precedence while matching stock Git's clean and
  modified results after content hashing is forced. Repositories with at least
  512 tracked regular files use a bounded two-worker status scan, while staging
  keeps its existing larger-repository policy. Porcelain v2 now writes its body
  through one buffered stdout lock instead of relocking for every path. A
  stat-size mismatch fast path skips both attribute resolution and SHA-1 when
  the index stat data already proves the worktree file changed; same-size files
  retain the exact raw or converted comparison path. On this workspace that
  reduced attribute-rule lookups and SHA-1 candidates from `115` to `6`, and
  immutable per-directory attribute rules are shared rather than deeply cloned.
  The observed status median improved from about `2.04x` stock Git to `0.75x`;
  p95 RSS is lower than stock Git (`7.41 MB` versus `7.57 MB`).
- The final ten-repeat standard seven-operation corpus remained green for exact
  result checks. With the release above, Zmin median wall time and p95 RSS were
  lower on all seven operations. Median-time ratios ranged from `0.498x`
  (`init`) through `0.946x` (`index-pack`); p95 RSS ratios ranged from `0.873x`
  through `0.997x`. Status measured `0.744x` median time and `0.950x` p95 RSS,
  while ordinary log measured `0.748x` and `0.963x` respectively.

Post-corruption-closure release check on 2026-07-16:

- Release SHA-256
  `4245de5d38d8638aae2e729bcdf50f18254a661bace6eebe0a8b771b6fe57272`
  passed the complete pinned `t1060-object-corruption.sh` file and a
  five-repeat standard seven-operation benchmark.
- All standard semantic checks remained exact. Median Zmin/Git ratios ranged
  from `0.543x` (`init`) to `0.987x` (`index-pack`), while p95 RSS ratios
  ranged from `0.864x` to `0.973x`; every standard lane remained below Git for
  both time and memory.

Historical measured rerun on the main workspace as of 2026-07-02:

- A fresh live-alias rerun with
  `ZMIN_OBSERVED_BENCH_REPEATS=1 ZMIN_BIN=/Users/dschewchenko/.local/bin/git.zmin-bin tools/git-observed-client-bench.sh .`
  now reports:
  `observed_log_unbounded` at
  `stock 0.06s / zmin 0.07s / ratio 1.17x`,
  `observed_ls_files_idea` at
  `stock 0.02s / zmin 0.01s / ratio 0.50x`,
  `observed_show_numstat` at
  `stock 0.06s / zmin 0.16s / ratio 2.67x`,
  `observed_show_stdin_name_status` at
  `stock 0.02s / zmin 0.14s / ratio 7.00x`,
  and machine-readable `observed_status` at
  `stock 0.04s / zmin 0.03s / ratio 0.75x`.
  The practical remaining developer-client gap on the current alias is therefore
  concentrated in the two `show` lanes, not in `status` or `.idea`-scoped
  `ls-files`.
- RSS is still materially above stock on every measured lane in that live-alias
  run: about `2.20x` on the unbounded `log` lane, `3.13x` on the `.idea`
  `ls-files` lane, `1.81x` on `show --numstat`, `3.45x` on
  `show --stdin --name-status`, and `2.39x` on machine-readable `status`.
- The current trace for `observed_show_stdin_name_status` still shows the raw
  diff body itself is small (two `show.commit_diff` phases at about `0.009s`
  each) while total `cli.dispatch` is about `0.134s`, so the remaining gap is
  now mostly outside the diff body and still needs another pass on surrounding
  renderer/setup overhead.
- The current trace for `observed_show_numstat` shows `show.commit_diff` at
  about `0.030s` inside `cli.dispatch` at about `0.150s`, with many
  `diff_stat.entry_content.index` subphases contributing to the remaining cost.
  This lane is improved relative to earlier much worse runs, but it is still
  meaningfully slower than stock on the live alias and remains an active
  replace-git performance target.
- A follow-up log-specific pass on 2026-07-02 kept the compat-safe
  `read_object_prefix_or_full(...)` author-hints path and lowered
  `PARALLEL_LOG_AUTHOR_METADATA_MIN_COMMITS` from `4096` to `1024`, which
  enables parallel author-metadata hydration for the measured `1577`-commit
  observed log lane on this workspace. The latest release-binary trace for that
  path dropped `log.collect_commits.author_metadata_from_hints` from about
  `0.0259s` to `0.0218s`, and `log.total` from about `0.0385s` to `0.0341s`.
  A three-run wall-clock rerun on the same lane then measured
  `stock: 0.03 / 0.02 / 0.02s` against `zmin: 0.02 / 0.03 / 0.02s`, so the
  best-of practical time gap is closed for this workspace, while RSS remains
  materially higher than stock on the same lane (about `19.9 MiB` vs
  `8.5 MiB`).
- A direct live-repo smoke rerun against the freshly rebuilt
  `cargo-target/release/zmin` on 2026-07-02 confirmed that the currently built
  binary matches stock Git for the previously problematic observed
  `show --name-status "--format=%H %P" --stdin` lane and for the machine
  readable `status --porcelain -z --no-renames --untracked-files=all
  --ignored=matching --` lane. The stale mismatch that had still been visible
  through `~/.local/bin/git.zmin-bin` was resolved by re-syncing that local
  alias to the rebuilt release binary with `tools/sync-local-git-alias.sh`.
- `tools/sync-local-git-alias.sh` now hard-fails if the just-installed local
  alias diverges from stock Git on those two observed client lanes for the
  selected verification repository, and
  `sync_local_git_alias_script_verifies_observed_client_flows` in
  `git_replacement_dogfood_compat.rs` covers that post-sync guard with a
  temporary alias path and a temporary two-commit repository fixture.
