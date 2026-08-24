# Performance Evidence Contract

W5.0 defines the boundary between a local benchmark measurement and an
authoritative performance result. The contract is implemented once in
`tools/performance_contract.py` and is used by both
`tools/git-performance-bench.sh` and `tools/git-observed-client-bench.sh`.

## Authoritative evidence

An authoritative run is accepted only when all of these facts are bound in the
metadata and raw evidence:

- absolute stock-Git and Zmin executable paths, versions, and SHA-256 hashes;
- an explicitly supplied stock comparator selected by `GIT_BIN` (standard) or
  `ZMIN_STOCK_GIT` (observed), which must be the canonical executable from the
  validated Git v2.55.0 HTTP bundle. The bundle is checked against the
  contract's `v2.55.0`, `e9019fcafe0040228b8631c30f97ae1adb61bcdc`, and
  `72923418db7b26dfddc21e2268660c5118e560bdfaa09b4489b67b38e9b69c49`
  tag/commit/archive identities, including the `git-remote-http` and
  `git-http-backend` helpers and bundle `GIT_EXEC_PATH`; authoritative mode
  never falls back to `PATH` or `/usr/bin/git`;
- an explicitly supplied prebuilt Zmin release binary; authoritative mode never
  auto-builds or generates an identity sidecar;
- the Zmin source commit, clean-tree status, release build profile, and a
  matching hash-bound `<zmin-binary>.identity.json` sidecar containing the
  canonical `<trusted repo_root>/target/release/zmin` output path, absolute Cargo/rustc
  paths, versions, and SHA-256 hashes from a separate sanitized build command;
- immutable Cargo.lock and Cargo config byte snapshots, source-state snapshot,
  canonical build-manifest and worker-environment facts, durability facts, and
  a canonical payload digest; unknown or missing marker fields are rejected;
- the exact trusted absolute Python interpreter path, version, and SHA-256;
- the exact trusted absolute Make executable path, normalized version line, and
  SHA-256; benchmark start/finish callers must pass these Make anchors explicitly;
- `Cargo.lock`, a separate fixture worktree/content fingerprint, and a logical
  Git-state fingerprint for the resolved worktree (`HEAD`, refs including
  `packed-refs`, index, config, `shallow`, alternates, and linked-worktree
  metadata); the Git object database is not bulk-hashed;
- host OS/kernel/architecture, CPU count, RAM, filesystem type, and mount
  capacity;
- a deterministic sanitized environment: explicit `PATH`, `HOME`, `TMPDIR`, C
  locale, UTC, Git config isolation, terminal/prompt settings, and hash seed;
  inherited variables are removed and their names are recorded in
  `ZMIN_BENCH_REJECTED_ENV`; CA/proxy variables are preserved only when the
  harness is explicitly opted into them, the choice is recorded as
  `ZMIN_BENCH_NETWORK_ENV_POLICY`, and preserved values are then bound in
  metadata. This rejection includes repository/index/object-directory and
  alternates variables, `GIT_SSH`/`GIT_SSH_COMMAND`, all `GIT_CONFIG_*` and
  `GIT_TRACE*` controls, `RUST_LOG`/`RUST_BACKTRACE`, and `LD_*`/`DYLD_*`
  injection variables;
- relevant Git/Zmin environment and fixture Git-config fingerprints;
- the harness files, command corpus, fixed random seed, and paired ordering;
- exactly 3 warmups, 30 measured Git/Zmin pairs, and 10 separate process-cold
  pairs per lane. Process-cold means a fresh child process with
  `filesystem_cache=warm` and `filesystem_cache_drop=no-drop`; these samples
  are not disk-cold and do not claim an OS filesystem-cache flush;
- the authenticated ordered standard mandatory manifest is exactly
  `init,status,log,rev-list,merge-base,pack-objects,index-pack`, while the
  observed mandatory manifest is exactly the existing ten `observed_*` lanes;
  authoritative mode rejects subsets, extras, duplicates, and reordered lanes;
- standard `init` pairs use the identical `init -q <dir>` argv for Git and
  Zmin, with the same absolute `GIT_TEMPLATE_DIR` pointing to a contained,
  no-symlink, empty fixture directory. Immediately before each child launch,
  the process wrapper pins that identity with `O_DIRECTORY|O_NOFOLLOW` and
  passes the open directory fd to the child; on macOS it changes the child to
  that fd with `fchdir` and uses descriptor-relative `GIT_TEMPLATE_DIR=.`
  because Git cannot `opendir` its `/dev/fd/<fd>` fdescfs path. That directory
  is included in the fixture identity; missing, non-empty, symlinked,
  unsupported, or swapped templates fail closed. Template warnings are not
  normalized after execution;
- standard and observed authoritative runs retain an authenticated
  `equivalence.tsv` with one exact exit/stdout/stderr hash comparison for every
  warmup, measured, and process-cold pair. The manifest records both the
  internal `sample_kind=cold` and the external `phase=process-cold` label.
  Missing, duplicate, reordered, tampered, or late substituted pair records
  fail closed before superiority is accepted. Successful pairs retain hashes
  and metadata only; full child streams are retained only for a command or
  equivalence failure.
- authoritative rows contain only Git and Zmin; Gitoxide/gix is disabled in
  authoritative mode;
- raw result paths and SHA-256 hashes; and
- metric availability for wall time, user time, system time, the authenticated
  platform-native peak-memory metric, page faults, and platform-supported
  read/write bytes. POSIX rows use `peak_rss_bytes` with `working_set_peak`;
  Windows rows use `peak_job_commit_bytes` with `job_commit_peak`.

Each standard/observed child measurement has a 120-second wall-time limit. On
POSIX the wrapper starts a dedicated process group and terminates the entire
group on timeout;
the wrapper returns status 124 and emits a `non-authoritative` diagnostic.
Platforms without the required process-group cleanup primitive fail closed
before spawning a child.

The harness invokes that exact absolute Python path for every contract,
measurement, and parser operation. It never resolves Python from `PATH`; a
relative or missing `ZMIN_BENCH_PYTHON_BIN` is rejected.

The required timing and platform-native peak-memory metric must be available.
Optional page-fault and read/write-byte fields use the literal value
`unsupported` when the host cannot provide them; unavailable values are never
encoded as zero. A Windows Job Object commit peak is never compared with POSIX
POSIX RSS, and a row with mismatched memory metric identity fails closed.

For each canonical standard or observed lane, finish independently recomputes
the measured-30 and process-cold-10 classes from the pinned raw rows. It records the
sorted even-N median (the two middle values), nearest-rank p95
(`ceil(0.95*N)-1`), and the platform-native peak memory as the maximum raw
sample. The summary retains both raw-byte peaks and a human-readable
`ceil(bytes/1024)` projection; the strict peak-memory gate compares raw bytes,
never the rounded projection. Every class must prove strict Zmin `<` stock for
median wall time, p95 wall time, and platform-native peak memory; equality,
missing/zero/negative/non-finite/unsupported metrics, or
insufficient samples fail closed. The authenticated `superiority.tsv` is a
readable projection only: finish recomputes it and requires exact canonical
schema, row order, counts, values, and bytes, so missing, extra, reordered, or
tampered summaries cannot create a claim. Optional ratio caps must be finite
values in `(0,1]` and can only tighten these mandatory gates; absent caps still
mean the strict `<1.0` policy.

The authenticated `statistics_policy` adds a deterministic stability check to
those raw gates. It uses 20,000 paired log-ratio bootstrap resamples at 95%
confidence and the exact one-sided paired sign test at alpha `0.05`; its seed is
derived from the evidence identity, lane, sample class, and metric. Median wall
time and median platform-native peak memory use the inferential interval.
Maximum platform-native peak memory remains the strict guardrail above, not an
inferential estimate. There is no mandatory
5% margin and measured and process-cold classes are never pooled across
operating systems. A row is `pass` only when both raw gates and stability pass;
raw-gate failure is `fail`; raw-gate success with unstable statistics is
`inconclusive` and cannot publish universal superiority; exploratory or other
non-authoritative evidence is `non-authoritative`. Finish recomputes every
statistics field from paired raw rows and rejects forged policy or summary
fields (ties are excluded from the sign-test denominator).

All positive timing and platform-native peak-memory inputs are validated as finite, positive,
float-representable values before inferential logarithms or exponentials are
evaluated. Malformed, overflowing, underflowing, or non-finite inputs fail
closed as non-authoritative evidence rather than raising an uncaught numeric
exception.

Authoritative mode requires interleaved paired order, a clean committed source
tree, a release binary, complete sidecar identity, retained output artifacts,
complete sample counts, and a successful final evidence validation. Compat,
debug, stale, uncommitted, partial, and missing-metric runs fail closed.
The main source fixture and all fetch remote/push/incremental-source setup
complete before metadata capture. Other benchmark lanes use separately bound
per-lane temporary fixtures prepared before their first measurement; they do
not mutate the captured source fixture. Fetch lanes then operate on immutable
snapshots or copied non-bound clones; mutation of any bound fixture identity
during measurement invalidates the run.

The finish step receives independent anchors from the invoking harness rather
than trusting editable metadata for repository and fixture paths, corpus,
fixture fingerprint, binary paths/versions/hashes, identity sidecar, or the
ordered harness list and hashes. Missing, redirected, or changed anchors fail
closed. The logical Git-state fingerprint resolves
`git rev-parse --git-path objects/info/alternates`, including linked worktrees
and alternate object directories, and hashes that file as metadata without
hashing the object database.

The invoking harness also passes a SHA-256 of the untouched start metadata
manifest. Finish rejects any edit to that manifest before recomputing policy,
host, environment, fixture, and source invariants.

Finish reads metadata, canonical rows, and every raw result into immutable
byte snapshots, hashes and parses those same bytes, then re-stats and
re-hashes all inputs immediately before publishing evidence. Evidence is
written through a same-directory temporary file, `fsync`, atomic rename, and
directory `fsync`. Authoritative `--rows` and every authoritative raw result
must be inside the explicit retained results directory and the rows file must
appear in `raw_results`.

When a retained results directory is discovered recursively, only its root
`metadata.json` and `evidence.json` control files are excluded from raw-result
discovery. A same-named file in any nested directory is rejected, not silently
omitted.
Authoritative metadata, rows, raw results, output, binaries, sidecars, and all
existing path components must not be symlinks. The retained results directory
identity is bound from start through publication; path swaps and output escapes
fail closed.

The cross-platform aggregator inventories size and entry caps before reading,
then applies bounded row parsing before parsing untrusted retained bundles: at
most 32 direct files and 96 MiB per `standard/`
or `observed/` bundle, 32 MiB per file, 192 MiB across one platform root, and
100,000 rows in any TSV candidate. JSON inputs additionally reject duplicate
keys and are bounded to 128 levels and 100,000 decoded nodes. These limits are
above the authoritative seven/ten-lane output and do not alter the
20,000-resample statistics policy.
The aggregator pins each platform root and both bundle directory identities and
their exact direct entry sets, then rechecks those identities, entry sets, file
signatures, and hashes immediately before publishing. Every `raw_results` entry
must authenticate both its SHA-256 and its declared byte length; late-added,
removed, replaced, symlinked, oversized, or traversal artifacts fail closed.

Authoritative preflight rejects diagnostic tracing requested through phase,
packet, SSH, or inherited `GIT_TRACE*`/Zmin trace controls in both harnesses.
Diagnostic tracing remains available only for exploratory runs. Both
authoritative harnesses also require an explicitly supplied absolute retained
output directory; they never silently place authoritative evidence in `/tmp`.
The harnesses pass that directory through the shared no-follow contract before
creating `summary.tsv` or metadata. Authoritative directories must already
exist and have a stable identity; exploratory directories are also created and
revalidated through the same helper. Symlinked or redirected directories are
rejected without touching their target.

Every planned benchmark artifact is also rooted in a pinned directory identity.
The shared artifact primitive preflights exact names before child processes run,
walks parent components with no-follow directory descriptors, and opens final
components with `O_NOFOLLOW|O_CREAT` under an explicit truncate, append, or
exclusive policy. This covers summary/rows/results, child stdout/stderr/metric
files, and retained copies. Shell redirection and `cp` are not used for those
artifacts; file-symlink swaps fail closed and cannot modify an external
sentinel. Evidence publication retains its separate temp-file, fsync, and
atomic-replace durability contract.

When `results_dir` is supplied in exploratory, smoke, or authoritative mode,
the start metadata captures that directory's identity and both metadata and
final evidence are published through the same pinned no-follow directory-fd
path. A directory replacement between identity capture and publication fails
closed without touching the replacement or any external sentinel. Platforms
without the required pinned publication primitives fail closed when a retained
directory is requested.

## Exploratory and smoke runs

The default harness mode is `exploratory`; `smoke` is also non-authoritative.
These modes may use a local alias, a stale or untracked binary, a dirty tree,
reduced repeats, or incomplete metrics, but their metadata explicitly records
`claim_status=non-authoritative` and the reasons. Their ratios must not be
presented as a release performance claim.

`ZMIN_BENCH_OPS` subsets are pilot scopes only and can never finish as
authoritative evidence. A universal mandatory performance claim requires both
complete canonical corpora: the seven-lane standard manifest and the ten-lane
observed-client manifest. One harness passing its integrity contract is not by
itself evidence of speed or platform-native peak-memory superiority.

Exploratory runs may resolve a local Git from `PATH`, but their result metadata
is non-authoritative. Only a run that passes the pinned-comparator preflight,
including bundle provenance and helper identities, may enter the authoritative
contract.

Create a binary identity sidecar after producing a release binary:

```bash
/usr/bin/python3 tools/performance_contract.py build-release \
  --repo-root /absolute/clean/repo \
  --cargo-bin /absolute/trusted/path/cargo \
  --rustc-bin /absolute/trusted/path/rustc \
  --python-bin /absolute/trusted/path/python3 \
  --git-bin /absolute/trusted/path/git \
  --make-bin /absolute/trusted/path/make
```

This sanitized command is the only supported producer of the release sidecar.
It requires a clean source tree, runs Cargo with `--release --locked --offline`,
and passes Cargo `CARGO_NET_OFFLINE=true`; the pinned registry sources must
already be available in its isolated Cargo home. It records the
Cargo.lock and Cargo config snapshots, exact binary hash, source commit/status,
the canonical sanitized build manifest and worker-environment values, durability
facts, and absolute Git/cargo/rustc/Python hashes and versions. The worker gets
only that explicit deterministic environment, so caller network overrides are
not inherited or serialized into the release marker. Measurement-time rejected variables remain
separate in benchmark metadata. There is no manual
`--profile` input: unknown markers, manual/compat profiles, incomplete
toolchain/environment records, or stale sidecars are rejected before an
authoritative claim. The sidecar path is always exactly
`${ZMIN_BIN}.identity.json`; a different output path is rejected. The matcher
reconstructs the canonical `<repo_root>/target/release/zmin` output path, build
policy, source state, and build manifest independently, so changing a policy or provenance field
and recomputing JSON does not authenticate it. Immediately before marker
publication it rechecks the original in-memory source snapshots; publication is
bound to the already-captured output-directory identity. Build the release binary and sidecar in a separate
sanitized step, then pass
`ZMIN_BIN=/absolute/path/to/target/release/zmin` and
`ZMIN_BENCH_PYTHON_BIN=/absolute/trusted/path/python3` to the authoritative
harness. The authoritative harness rejects an omitted `ZMIN_BIN` rather than
building through inherited Cargo/PATH/proxy settings.

Then use an explicit retained artifact directory for an authoritative run:

```bash
ZMIN_OBSERVED_BENCH_EVIDENCE_MODE=authoritative \
ZMIN_OBSERVED_BENCH_MAKE=/absolute/trusted/path/make \
ZMIN_OBSERVED_BENCH_OUT_DIR=/absolute/path/to/evidence \
tools/git-observed-client-bench.sh /absolute/path/to/fixture
```

The standard harness likewise requires `ZMIN_BENCH_MAKE` to name the same
trusted absolute Make executable. Both harnesses authenticate its path, first
version line, and SHA-256 at start and finish; a missing Make anchor is not a
v2-compatible invocation. The upstream compatibility runner holds the
authenticated Make source descriptor while it runs. On Linux it copies the
authenticated native ELF bytes into a sealed `memfd`, applies write/grow/shrink
and seal seals, then launches only that read/execute descriptor through
`execveat(..., AT_EMPTY_PATH)`; the source pathname is never reopened for
execution. Cache and preparation locks use fixed cache/.zmin-locks/
cache.lock and prepare.lock files published together by a no-replace directory
rename. Their descriptors are held by the shell and locked through inherited
fcntl.flock, so owner death releases the lock without stale directories or
takeover markers. On Linux the runner reopens the sealed object read-only
through /proc/self/fd before execveat; evaluated `.depend`/`po/build` outputs
and generated Perl modules are retained in the prepared tree and each regular
file is hashed in the authenticated prepared-artifact manifest. Manifest
generation uses one descriptor-relative, no-follow snapshot of each retained
root: node/byte accounting, regular-file hashing, and emitted rows all come
from the same opened descriptors, with identity and link-count checks before
and after each read. No generated artifact is reopened by pathname. The manifest
caps all generated entries at 65,536 nodes and 1 GiB of regular-file bytes;
these are deterministic fail-closed limits, not cleanup heuristics. Unexpected
generated paths fail closed instead of being deleted. The descriptor-bound
Make runner is currently supported only on Linux. Darwin and Windows fail
closed before even make --version because the supported runtimes do not
provide a verified descriptor-bound launch primitive; pathname execution is
not a permitted substitute. Unsupported platform behavior is therefore an
explicit harness blocker, not a compatibility result.

`tools/windows_child_metrics.py` is the native Windows child-measurement
foundation used by `tools/git-bench-process.py`: it passes only explicit stdio
handles to `CreateProcessW`, assigns the suspended child to a kill-on-close Job
Object before resuming it, terminates the whole Job Object on timeout, and
queries aggregate peak Job memory. After timeout or launch/unwind failure it
waits for the Job Object to become signaled (all assigned descendants exited)
before querying final metrics or closing handles; a quiescence timeout fails
closed. Its API is covered by non-Windows mocked tests and has Windows-only
smoke tests, but this does not make Windows-authoritative. Until
descriptor-bound artifact publication and durable retained
results are independently verified on Windows, the runner remains fail-closed
for authoritative evidence and no Windows speed or platform-native peak-memory claim is accepted.

`tools/local-run.sh benchmark` and `tools/benchmark-add-local.sh` may copy a
binary into a local alias without carrying this sidecar. Such runs remain
exploratory until identity is regenerated and validated.

Directory durability is recorded explicitly. Authoritative evidence fails
closed when directory `fsync` is unsupported, including Windows until a
verified Windows-safe directory flush exists; unsupported platforms are never
silently claimed. This contract records measurement integrity only. It does
not assert that Zmin is faster than Git or uses less memory, and it does not
establish cross-platform performance coverage.

## Strict Git LFS performance evidence

`tools/lfs-performance-bench.py` emits the separate
`zmin-lfs-performance-v4` TSV schema. The current writer and standalone
verifier accept v4 only. Retained v3 bundles remain immutable historical
evidence and must be checked with their original committed verifier; v4 never
reinterprets or upgrades their five-sample statistics. A run must provide `--repo-root` for the
clean source repository that produced the canonical
`target/release/zmin`. The absolute output path must be outside that repository
and must not exist before the run. The caller must also provide a distinct,
absolute, fresh `--failure-dir` outside the source repository. A clean run
leaves that path absent.
Before any transfer, and again before evidence publication, the harness uses
the shared sanitized-release matcher to verify the clean commit, HEAD tree,
Cargo.lock snapshot, canonical release binary, Git identity, sidecar schema,
sidecar payload digest, exact sidecar bytes, and the exact Python executable
path/content/version recorded by the release builder. The pinned Git LFS and
stock Git executables are also rehashed immediately before publication.
Missing, dirty, stale, or mismatched provenance fails closed.
At process start the harness resolves its own script path exactly once with a
strict filesystem lookup and imports both local support modules only from that
canonical tools directory. After release provenance is available, the script,
contract module, and host-monitor module must equal the corresponding regular,
non-symlink files under the authenticated source `tools/` directory. Invoking
the trusted script through an untrusted sibling symlink cannot redirect either
import.

`metadata.tsv` retains the source commit and tree, empty-status digest,
Cargo.lock SHA-256, sidecar SHA-256 and payload SHA-256, sidecar schema and
marker, Zmin/Git/Git-LFS/Python hashes and versions, and a canonical
`zmin-lfs-release-provenance-v2` digest over those fields. It also records the
schema version and verified release profile. Shareable evidence records fixed
tool roles rather than source, sidecar, executable, home-directory, or user
paths.

The four canonical TSV files are covered by `evidence-set.json`. Its
`zmin-lfs-performance-evidence-set-v4` root binds the exact bytes, columns,
record counts, per-artifact schemas, every metadata key/value (including claim,
workload, repeats, identities, and Git LFS version), and all four artifact
digests. Publication writes and `fsync`s a private sibling staging directory,
performs a full readback plus final provenance check, then uses an atomic
same-filesystem no-replace directory rename and applies the platform durability
barrier (directory `fsync` on POSIX, write-through move on Windows). A failed
readback, race, collision, or post-publication check rolls the owned directory
back; a successful partial set is never reported. Changing, removing, or
adding a metadata or manifest field invalidates this LFS schema v4; it remains
separate from the general evidence schema above.

If validation or publication fails after acquisition, the positive output
remains absent. Instead the harness atomically publishes a bounded, read-only
`zmin-lfs-performance-failed-evidence-v1` bundle to the explicit failure path.
That negative-only namespace uses renamed acquired/candidate artifacts and no
positive `evidence-set.json`; its manifest sets `publishable=false` and
`claim=not-established`. It binds the sanitized error digest, exact invocation,
schedule and per-sample stdout/stderr digest sets, source/tool identities, the
candidate root, and every retained artifact byte. A dedicated readback verifies
the no-replace publication, full hashes, canonical complete raw schedule, and
provenance bindings. The bundle is forensic custody only and cannot be consumed
as positive performance evidence.

Both strict claim states use one immutable canonical workload policy: exactly
eight 16 MiB objects, concurrency lanes `1,2,8`, two warmup block rounds and
40 measured block rounds for both download and upload. Each round contains all
six lanes exactly once. Their order is a deterministic SHA-256 permutation
bound to the authenticated source commit/tree, Cargo.lock, release sidecar and
payload, Python, Zmin, Git, Git LFS, and workload identities. There is no
operator seed and therefore no reroll. Each lane remains an adjacent pair and
uses exactly 20 stock-first and 20 Zmin-first measured pairs. Consequently a
strict evidence set has exactly six lanes, 504 raw rows, 12 summary rows, and
six comparisons.
Any smaller, larger, reordered, or duplicate policy is non-authoritative;
`functional-smoke` is a distinct policy and its comparisons are always
`not-evaluated`, so it cannot establish superiority.

The performance thresholds are unchanged and every inequality is strict:
Zmin's marginal median wall time, nearest-rank p95 wall time, and maximum
platform-native peak memory must each be lower than pinned Git LFS in every
lane. With 40 measured samples, nearest-rank p95 is rank 38 rather than the
maximum. The verifier also derives all 40 adjacent-pair wall-time ratios. The
26th sorted ratio is the conservative one-sided 95% upper confidence bound for
the population median (binomial upper tail `0.04035`) and must be below `1`.
The stock-first and Zmin-first paired-ratio medians must independently be below
`1`, preventing an order effect from establishing the claim. A value equal to
`1` fails. No row is trimmed, winsorized, replaced, retried, or excluded as an
outlier; all measured spikes remain in the p95, maximum-memory, paired, and
order-subset derivations.

A persistent bounded stdlib-only host monitor runs throughout acquisition.
Darwin, Linux, and Windows adapters bind monotonic scheduler lag, system CPU,
one-minute load where the OS supplies it, memory-pressure state, and thermal
state to each raw measurement interval. Monitor startup, sampling, bounded
history, or shutdown failure invalidates the run. An explicit warning,
critical, or unsupported memory-pressure or thermal state also invalidates the
run before publication. The system CPU ratio is a canonical decimal when the
OS counters advance; an exact zero counter delta is recorded as `unobserved`,
never misreported as idle CPU. Counter rollback or an impossible busy delta
invalidates the run. System CPU/load and scheduler-lag values are retained as
diagnostics only: v4 deliberately invents no external-CPU cutoff that could
discard an unfavorable product result. Adjacent pairing, the paired confidence
bound, and both order-subset medians are the claim-bearing temporal-noise
guards. Host telemetry never deletes a row or turns a failed lane into a pass.
Darwin requires `kern.memorystatus_vm_pressure_level=1` and `pmset` to report
no thermal or performance warning. The fixed `pmset` command has a two-second
deadline and a 16 KiB in-flight combined-output cap; timeout, overflow, decode,
or exit failure becomes unsupported telemetry and invalidates the run. Linux
requires zero ten-second full-memory
PSI and every observed thermal zone to remain below its `passive`, `hot`, or
`critical` trip; missing pressure or thermal telemetry is unsupported. Windows
requires system
memory load below 90 percent and active cooling mode; 90--96 percent or passive
cooling is warning, at least 97 percent is critical, and missing native power
telemetry is unsupported. Windows evidence remains non-authoritative for the
independent durable-publication reason above.

Standalone readback revalidates every raw row rather than trusting the root
digest alone. It requires the canonical block-round schedule, pair ID and
balanced adjacent order, an exit
code of zero, one Batch request, one completed transfer per object, exact
payload and completion bytes, observed concurrency equal to the lane,
zero residual transfers, finite process/timing metrics, throughput consistent
with payload and wall time, the platform's exact memory metric/semantics/scope,
normal interval-bound host pressure/thermal evidence, bounded finite host
telemetry, and lowercase SHA-256 output identities. Raw cells containing
controls, private
locations or identities, or secret-bearing text fail closed. Summary and
comparison bytes must be the exact deterministic derivation of those validated
raw rows even if an attacker coherently rebuilds every artifact digest and the
evidence-set root.
The writer first serializes and reparses all acquired raw rows through that
same canonical TSV contract (nine decimal places for wall time and six for
throughput), then derives every summary, comparison, verdict, and claim only
from those canonical typed rows. It never summarizes higher-precision process
values and later compares them with rounded readback values.

The same bounded UTF-8 privacy validator covers every metadata key and value as
well as every raw cell. Only the schema's enumerated fixed roles and policy
values are deliberately allowlisted; dynamic text rejects case-insensitive
local usernames, control bytes, oversized cells, private POSIX/Windows paths,
credential-bearing URIs, and secret/authentication/cookie tokens. Readback also
reconstructs `metadata.tsv` in canonical sorted-key order and requires exact
bytes. Every integer cell uses canonical unsigned decimal notation, including
the selected platform-memory value; signs, whitespace, and leading zeroes fail
even when an attacker recomputes the complete evidence root.
The per-child timeout is independently validated before fixture creation and
bound in canonical form into metadata and the evidence root. A strict LFS run
uses exactly the 180-second default; smoke mode may select a finite value in
`(0,3600]` seconds. `NaN`, infinities, non-positive, noncanonical, and larger
values fail before a benchmark process can start.

Before importing the local contract module, the top-level harness disables
bytecode and forces `PYTHONDONTWRITEBYTECODE=1`. Every nested Python
measurement runner is launched with the sidecar-authenticated interpreter plus
`-B`, and receives a copied environment with
`PYTHONDONTWRITEBYTECODE=1` regardless of caller input. The measured command
inherits that forced setting. Therefore a strict run cannot dirty the source
tree with Python bytecode before the end-of-run provenance recheck.

## Cross-platform evidence aggregation

Cross-platform performance is a separate, fail-closed claim over three already
retained authoritative evidence roots. Each root must contain exactly
`standard/` and `observed/` bundles. The aggregator re-reads and revalidates
raw rows, `equivalence.tsv`, and the recomputed `superiority.tsv`; it never
trusts an emitted summary or pools/averages samples:

```bash
/usr/bin/python3 tools/performance-evidence-aggregate.py aggregate \
  --darwin /absolute/path/to/darwin-evidence \
  --linux /absolute/path/to/linux-evidence \
  --windows /absolute/path/to/windows-evidence \
  --output /absolute/path/to/cross-platform-evidence.json
```

The three roots must have unique Darwin/Linux/Windows identities, the same
source, pinned-comparator, contract, fixture, workload, and statistics
identities, all 17 mandatory lanes (seven standard plus ten observed), and
both `measured` and `process-cold` classes. Every recomputed row must be
`verdict=pass`; any missing, duplicate, mismatched, forged, inconclusive,
failed, or non-authoritative input produces `claim_status=non-authoritative`
and `universal_claim=not-established` with a nonzero exit. A successful output
contains deterministic per-bundle root manifest hashes and an authenticated
aggregate `manifest_sha256`; it contains no cross-platform speed or memory
average. This aggregation step cannot make Windows authoritative while the
Windows runner and durable retained-results contract remain fail-closed.
