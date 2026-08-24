# Current Git compatibility CI

The workflow supports a one-time bootstrap push, one controlled retry update,
and normal manual (`workflow_dispatch`) runs. The first authoritative run is
created by pushing a new branch named exactly
`compat/current-git-v2.55-replay`. The controlled retry is a single
non-forced push from snapshot tip
`6ae3fa78e2f4499bc1ca2a5b511fcde27b9605c2`; its tip commit must be signed and
contain the exact `Replay-Current-Git: true` marker. Push-run reruns are
skipped because `github.run_attempt` must be `1`. Preserve the branch after
creation: deleting and recreating it could create another accepted run.
Normal `workflow_dispatch` is available only after this workflow reaches the
repository's default branch. The
authoritative `full1045` scope runs on native `ubuntu-24.04` x86_64. It checks out
`github.sha` exactly, uses `contents: read`, has no secrets/release/publish
permissions, queues concurrent runs (`cancel-in-progress: false`), and allows
360 minutes for the complete replay. The pinned actions are
`actions/checkout@11bd71901bbe5b1630ceea73d27597364c9af683` (v4.2.2) and
`actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02` (v4.6.2).

Creating and pushing this exact replay branch also triggers the existing
`.github/workflows/compatibility-contract.yml` because its `push` trigger is
broad. That companion check is read-only and non-authoritative: it emits
`compatibility_claim=unverified` and cannot satisfy or replace the 1,045-test
current-Git replay evidence described here.

The committed `tools/git-current-compat-contract.json` is parsed with duplicate
key and unknown-key rejection before any network, build, or harness work. It
binds Git v2.55.0's archive URL/SHA-256, annotated tag object, peeled commit,
and the exact 1,046/1,045 top-level/selected name lists. The only excluded file
is `t5323-pack-redundant.sh`; git-svn, git-cvsserver, gitweb, cvsimport, and
git-p4 top-level families remain selected. The generated manifest and sorted
name-list digests are checked against the contract.

After archive SHA/tag/source validation, one exclusive fresh manifest cache is
prepared at `manifest-cache/v2.55.0.tar.gz`; the archive must be a regular
non-symlink file. Its extracted `git-v2.55.0` source receives the exact
`.zmin-pristine-source.sha256` marker used by the live harness (SHA-256 plus one
newline, read-only mode, and file/directory `fsync`) before either manifest or
deprecated audit runs. The same cache-preparation function is exercised by the
offline self-test, which uses the cached authority archive and runs the real
manifest/deprecated-audit helpers to prove 1,045/1,046 and the sole t5323
exclusion.

The runner installs the complete Git test dependency set with
`Acquire::Retries=0`; the cvs/cvsps, subversion/libsvn-perl, CGI, DBI, SQLite,
and other listed packages are required and a missing package fails preflight.
Package and executable versions/absolute paths are recorded. Perforce r23.2
`p4`/`p4d` and the pinned JGit 6.8.0 launcher are downloaded into a task-owned
tool directory, verified against frozen HTTPS SHA-256 and exact-size values
(`p4` 10,587,624 bytes; `p4d` 18,181,064 bytes), and never
uploaded as replay artifacts. The JGit launcher is the official Maven Central
6.8.0.202311291450-r `.sh` artifact (size 27,037,916 bytes; SHA-256
`764f272c3857da1acaa26689f187a5b38d6597f766c91db649300fd0b8225fa9`); its
HTTPS origin and frozen digest are the provenance/identity check. Apache/SVN, locales, gettext, OpenSSL, Java,
Perl modules, cvsps, and passwordless non-root sudo are preflighted before
the lanes start; failures are infrastructure failures, not test skips. The
Git-p4 family remains in the denominator. Each
lane's logs are checked for missing files and an exact top-level TAP
`1..0 # SKIP`; fully skipped retained tests (including t91xx, t94xx, t95xx,
t96xx, and t98xx) make the outcome incomplete. Assertion-level `# SKIP`
counts are recorded separately and do not turn platform skips into failures or
passes. The artifact files `optional-skips.tsv` and `assertion-skips.tsv` carry
the per-lane test names, classification, required profile, exact reason, log
hashes, and separate assertion counts.
The replay lane does not use a runner's preinstalled Rust toolchain and has no
fallback. `rust-toolchain.toml` continues to express the repository's
`stable` policy, while the replay lane freezes the observed native toolchain
at `1.98.0-x86_64-unknown-linux-gnu`, installed with the minimal profile and
`--no-self-update`. It verifies `rustc 1.98.0 (88d9e12ae 2026-08-18)`, the
`x86_64-unknown-linux-gnu` host, and `cargo 1.98.0 ...`. The manifest's
`rust-version = 1.95` remains the MSRV and is unchanged. `RUSTUP_MAX_RETRIES=0`
plus `CARGO_NET_RETRY=0` makes installation or dependency fetch failure
explicit rather than silently retrying. Cargo builds both `zmin` and
`zmin-git-remote-http` with `--locked --release`.

The replay setup resolves absolute `rustc`, `rustdoc`, and `cargo` paths from
the pinned toolchain with `rustup which --toolchain`, validates their shared
expected toolchain directory, binds `RUSTUP_TOOLCHAIN`, `RUSTC`, `RUSTDOC`, and
the replay cargo path, and prepends that directory to `PATH`. It unsets
`RUSTC_WRAPPER` and `CARGO_BUILD_RUSTC_WRAPPER`; isolated `CARGO_HOME` cannot
make Cargo fall back to a rustup proxy or the repository's `stable` override.

The v2 contract has one frozen Linux base profile and seven explicit
platform/dedicated-host assignments: Windows `t0029`, `t0051`, and `t5580`;
macOS `t3910`; case-insensitive Linux `t6419`; privileged Linux `t1509`; and
high-disk Linux `t5608`. Their exact source TAP reasons are required in
`optional-skips.tsv`; unknown tests, reason changes, profile mismatches, and
missing logs fail closed. The Linux artifact therefore reports
`cross_platform_status=pending` with seven coverage gaps, even when both
1,045-row lanes pass. `tools/git-current-compat-aggregate.py` accepts a
cross-platform pass only after the assigned profiles close those gaps and
both control and Zmin cover every selected test; it never treats Linux alone
as a 100% claim. Aggregation verifies the artifact checksum manifest, metadata
and lane status, every real TAP log and its skip reason, exact manifest/contract
identity, and rejects symlinked/traversal paths, duplicate keys, unknown
profiles, and synthetic TSV-only evidence. Its output is atomically replaced
only after validation.

Run `32741972934` for snapshot `369ac8b56f6c8cdee6e7057df7b77712b6007380`
is classified `HARNESS INVALID`: the runner had no preinstalled `stable`
toolchain, dependency setup stopped before the replay, and zero tests ran.
Its artifact is not current-Git evidence and must not be counted as either a
pass or a test failure. The controlled retry must use the frozen toolchain
above; no manual rerun of the failed push is authoritative.

Run `32746703223` for snapshot `d8f756d1f5eb0abd377f9939524cd33ab2f939ad`
is also classified `HARNESS INVALID`: the authoritative build was blocked by
Rust E0308 before tests ran (zero tests). Its artifact is not current-Git
evidence and must not be counted as either a pass or a test failure. The next
controlled retry must use a signed, non-forced commit whose parent is that
snapshot tip and whose message contains the exact `Replay-Current-Git: true`
marker.

Run `32748958014` for snapshot
`18a0f0d455337385e1b6a25312fb15879cdf5145` is also classified
`HARNESS INVALID`: the pinned Rust installation and release build passed, but
the runtime executable preflight rejected the runner's symlinked Perl path
before either lane ran (zero tests). Its artifact is not current-Git evidence
and must not be counted as either a pass or a test failure. The next controlled
retry must use a signed, non-forced commit whose parent is that snapshot tip
and whose message contains the exact `Replay-Current-Git: true` marker. The
workflow now resolves and exports canonical, regular executable paths for
Perl, Git, Python, and Make, while the replay helper revalidates those exact
paths and records their command identities and canonical paths.

Run `32764105728` for snapshot
`9ac8723b671d845cb6181b6b0b88e6bb93a97531` is classified `HARNESS INVALID`:
dependency provisioning failed before its completion marker, and the replay
step was correctly skipped; zero tests ran. Its artifact records the setup
stage, line, and shell command without credentials. The next controlled retry
must use a signed, non-forced commit whose parent is that snapshot tip and
whose message contains the exact `Replay-Current-Git: true` marker.

Run `32767635984` for snapshot
`45108021d7883ec8011b4d03bdd0aec0606851b7` is classified `HARNESS INVALID`:
ambient executable preflight rejected the runner's symlinked `awk` alternative
before either lane ran (zero tests). Ambient command spellings are now recorded
alongside their canonical regular executable targets; only final bound
executables and downloaded tools require the spelling itself to be canonical.
The next controlled retry must use a signed, non-forced commit whose parent is
that snapshot tip and whose message contains the exact
`Replay-Current-Git: true` marker.

Run `32773393010` for snapshot
`6ae3fa78e2f4499bc1ca2a5b511fcde27b9605c2` is classified `HARNESS INVALID`:
the isolated Cargo home left the nested rustup proxy without the pinned
toolchain bin directory, causing `rustc -vV` to fail with `ENOENT` before
either lane ran (zero tests). Its artifact is not current-Git evidence and
must not be counted as either a pass or a test failure. The next controlled
retry must use a signed, non-forced commit whose parent is that snapshot tip
and whose message contains the exact `Replay-Current-Git: true` marker.

Run `32769621191` for snapshot
`0e1f17ac245e306fb90462be38d4091c72a089bd` is classified `HARNESS INVALID`:
the canonical-runtime Git lookup preserved the `command -v` line terminator,
so preflight rejected the path before either lane ran (zero tests). Its
artifact is not current-Git evidence and must not be counted as either a pass
or a test failure. The next controlled retry must use a signed, non-forced
commit whose parent is that snapshot tip and whose message contains the exact
`Replay-Current-Git: true` marker.

Stock and Zmin runs use separate, newly-created lane caches, homes, temp
directories, and output directories. The stock lane builds Git from the exact
archive, then resolves exactly one executable matching
`harness-v2.55.0-*/git` under that lane cache, canonicalizes its path, verifies
`git version 2.55.0`, and records its path/hash/version. No fingerprint or host
path is hardcoded. The Zmin lane uses the release binary and remote HTTP helper
built from the triggering commit.

Normal Make is authoritative for the prepared stock tree: its
`GIT-BUILD-OPTIONS` must be present, must show CURL, EXPAT, gettext, Perl,
Python, Gitweb, and PCRE2 enabled, and no synthetic NO_* fallback is allowed.
The prepared tree includes generated `git-svn`, `git-cvsserver`,
`git-cvsimport`, `git-p4`, and Gitweb helpers. Both lane shims receive the same
source/options/helper set; only the Git executable and remote-HTTP target
differ.

`per_test_timeout=0` is authoritative. Any nonzero timeout is diagnostic and
is labelled in metadata; it cannot be reported as authoritative. The `jobs`
input controls build/preparation concurrency only. The helper
uploads raw per-test logs, role summaries, manifests, dependency/tool/binary
hashes, runner OS/kernel data, and checksums. A bounded universal EXIT trap
writes an outcome, minimal metadata, and checksums even on unexpected
termination. The workflow's always-run infrastructure collector supplies a
minimal artifact if setup fails, then the pinned upload step runs, and the final
gate reads the stored outcome rather than masking a failed replay.

The checkout is task-owned and ephemeral. The helper requires the checkout's
`.tmp` path to be absent, creates exactly that path for the existing harness,
and removes that owned path plus its replay work tree on exit. No cache action,
ref mutation, retry/reroll, release call, or network credential is used.

The cached compatibility state is 358/1045 pass, 687 fail, with two timeout
markers; it is unverified evidence, not a current-Linux result. There is no
current Linux compatibility claim until a complete `per_test_timeout=0` replay
finishes in both lanes with zero fully skipped retained top-level tests.

Before merging this proposal, run `bash -n` on both replay scripts, parse the
workflow with a YAML parser while checking dispatch-only triggers, immutable
action SHAs, read-only permissions, and the native runner, then run the strict
contract parser and `tools/git-current-compat-replay-selftest.sh`. The offline
self-test invokes the production `--selftest-parse-lane` mode for a clean log,
the retained git-p4 `1..0 # SKIP` fixture, a missing log, and an
assertion-only skip. The full-skip case must be nonzero while the assertion-only
case remains green with its count recorded; no network or dispatch is needed
for these checks. Set `ZMIN_CURRENT_GIT_AUTHORITY_ARCHIVE` to the cached,
already-validated authority archive when running that prep case; set
`ZMIN_COMPAT_REPO_ROOT` only when the helper scripts are outside the proposal
checkout.
