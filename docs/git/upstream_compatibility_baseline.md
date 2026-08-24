# Upstream Git Compatibility Baseline

## Authoritative current-Git contract

The machine-readable scope contract is
`tools/git-upstream-compat-contract.tsv`. This section is the canonical
compatibility scope; other compatibility documents and older evidence notes
must refer back to it instead of defining another denominator. Run
`tools/git-upstream-compat-audit.sh contract-check` before changing the pinned
source or scope files.
The CI entrypoint is
`tools/git-upstream-compat-contract-gate.sh prepare-and-check`; it verifies the
remote tag's peeled commit, downloads and hashes the exact archive when the
cache is absent, and then runs the local contract audit. It emits
`compatibility_claim=unverified`: passing the scope gate is not compatibility
evidence by itself. Suite outputs carry `run-metadata.tsv`; validate it with
`tools/git-upstream-compat-contract-gate.sh validate-run` so bounded, custom,
failed or exploratory runs cannot be treated as authoritative full evidence.
The gate also compares an archive-derived source-tree digest with every
archive-listed file in the cache, and its immutable CI sentinels reject drift
of the v2.55.0 identity, `1045/1046` counts or exact exclusion groups.

The frozen upstream identity is Git `v2.55.0`, commit
`e9019fcafe0040228b8631c30f97ae1adb61bcdc`, from
`https://github.com/git/git/archive/refs/tags/v2.55.0.tar.gz` with SHA-256
`72923418db7b26dfddc21e2268660c5118e560bdfaa09b4489b67b38e9b69c49`. The
cache path is `~/.cache/zmin/git-upstream/git-v2.55.0`. A source archive is
accepted for this contract only when its SHA-256 matches; a Git checkout must
also resolve to the recorded commit. Other tags are exploratory evidence, not
current-contract evidence.

### Explicit HTTP comparator profiles

`tools/git-upstream-http-provenance.sh` has two explicit profiles. The
`canonical` profile is the base `bundle.tsv` schema containing `git`,
`git-remote-http` and `git-http-backend`; it remains the default used by the
current-Git suite. The `with-http-fetch-pinned` profile is a separate
versioned schema (`manifest_version=3`) whose bundle name ends in
`-with-http-fetch-pinned` and whose manifest records the archive-bound source
manifest, declared tag/commit binding, exact build flags and toolchain hashes,
source-member hashes, member modes/sizes/hashes and link dependencies. It
builds `git-http-fetch` from the frozen v2.55.0 archive-derived source with
the Darwin reproducibility profile (`SOURCE_DATE_EPOCH=0`, path remapping,
`ZERO_AR_DATE=1` and `-Wl,-no_uuid,-headerpad_max_install_names`), then adds
one fixed UUID and an ad hoc signature so Darwin dispatch remains readable.
The trusted helper is mode `0555`, SHA-256
`fc2e8b9e47cafb39140ca90f56fbc6cb09912c6d715feb020157733868f08b0e`, and its
Mach-O dependencies are limited to the five recorded system paths.

Profile selection is explicit: pass `canonical` or
`with-http-fetch-pinned` as the validator's second argument (or set
`ZMIN_HTTP_BUNDLE_PROFILE`). The pinned profile records the fixed trusted
helper digest for reporting, but authorizes the published helper only by a
fresh deterministic source/toolchain byte comparison and requires both the
manifest helper field and fixed-tool hash to match that digest. It fails closed on
`ZMIN_HTTP_PERL` and Perl module-injection variables, verifies that `git
http-fetch -h` dispatches to the bundle-local helper, and rejects changed
Mach-O UUID/load commands, injected or non-system dylibs, and tampered helpers
even when the manifest and sidecar are rewritten. Run
`tools/test-git-upstream-http-fetch-profile.sh` for two clean-build byte
identity, dispatch, and tampered-helper regression checks; the base profile
remains covered by `tools/test-git-upstream-http-provenance.sh`.

The v3 `source_root` field is an absolute cache-local path and is intentionally
not a cross-machine portability claim; a relocated cache must rewrite that
field and its sidecar only after restoring the exact archive-derived source.
Pinned validation authorizes the helper by byte-for-byte comparison with a
fresh rebuild from that validated source and fixed `/usr/bin` toolchain, so a
manifest or hash-command rewrite cannot authorize a different helper.

The authoritative upstream manifest is `all-nondeprecated`: all `1046`
top-level `tNNNN-*.sh` files in the frozen tree except the one whole-file
deprecated group below. Its upstream-test denominator is therefore `1045`.
In this repository, `100% of current Git v2.55.0` means 100% of the current Git
v2.55.0 contract excluding deprecated/removed API, with every
supported/nondeprecated behavior backed by the pinned upstream shell suite,
stock-Git differential evidence, and applicable macOS, Linux and Windows
platform evidence. Command dispatch, parser acceptance, a local census,
written-row ratios or a green narrowed suite do not prove drop-in parity.

Zmin-only APIs have a separate contract and evidence denominator in
`docs/cli/zmin_extensions_inventory.md`; they are not silently added to the
Git denominator. The machine source for that separate contract is
`tools/zmin-extensions-contract.tsv`; the Markdown inventory is only its human
projection. It contains 40 stable primary rows and 7 relationship-neutral
rows. Together that is 47 tracked contract rows, not 47 Git APIs and not an
arbitrary Git LFS ecosystem-parity claim. The `command.lfs` row's configured
mTLS/client-identity exclusion fails before network access and belongs only to
the extension transport scope; it does not alter the Git denominator.

Any performance or RSS measurements later in this retained evidence document
are historical and non-authoritative. They do not establish a universal speed
or memory claim; use `docs/git/performance_evidence_contract.md` for the
cross-platform evidence contract.

## Release archive contract

Every supported release-matrix archive contains the `zmin` executable, its
sibling `zmin-git-remote-http` helper, and `ZMIN-MANIFEST.tsv` with the SHA-256
of both binaries. `tools/release-package.py check` validates this manifest;
the release workflow runs it for every archive and runs an extracted-package
smart-HTTP HTTPS clone with an explicit local test CA, empty `PATH`, and no
helper-path override. This is a packaging/install gate, not current-Git
compatibility or cross-platform evidence.

The exact exclusions and classifications are:

| Group | Files | Classification | Current contract | Upstream evidence |
| --- | ---: | --- | --- | --- |
| `t5323-pack-redundant` | `1` | upstream deprecated/removed | excluded | `Documentation/git-pack-redundant.adoc:14-26`, `t/t5323-pack-redundant.sh:39-53` |
| `git-svn` (`t91*`) | `69` | external-but-current | included | `Documentation/git-svn.adoc`, `command-list.txt:195`, `t/t91*` |
| `git-cvsserver` (`t94*`) | `3` | external-but-current | included | `Documentation/git-cvsserver.adoc`, `command-list.txt:91-92`, `t/t94*` |
| `gitweb` (`t95*`) | `3` | external-but-current | included | `Documentation/gitweb.adoc`, `command-list.txt:247`, `t/t95*` |
| `cvsimport` (`t96*`) | `5` | external-but-current | included | `Documentation/git-cvsimport.adoc`, `command-list.txt:91-92`, `t/t96*` |
| `git-p4` (`t9800`-`t9836`) | `37` | external-but-current | included | `Documentation/git-p4.adoc`, `command-list.txt:150`, `t/t9800`-`t9836` |

The `full-core` manifest excludes the five current external groups only for a
narrowed core developer-flow suite. It is not the current-Git denominator and
must not be described as `100% current Git`. `t9850-shell.sh` is not a
git-p4 test and remains in `full-core` and `all-nondeprecated`.

## Historical macOS preflight checkpoint (2026-08-10; non-authoritative)

This dated macOS checkpoint is retained as historical exploratory evidence.
The current authoritative upstream and performance runner supports Linux only;
Darwin and Windows fail closed before `make --version`. It cannot establish
current compatibility or platform readiness. The canonical current contract
and status are defined by the machine-readable scope plus
[`docs/cli/compatibility_acceptance.md`](../cli/compatibility_acceptance.md)
and [`performance_evidence_contract.md`](performance_evidence_contract.md).

The W2a preflight used a clean `git archive HEAD` snapshot at commit
`b7f48c2716273fff98a255ef36c4cbf62b390b01`, the exact v2.55.0 contract gate,
and the `compat` Cargo profile. The host manifest is recorded at
`/Users/dschewchenko/.cache/zmin/w2a-macos-v2.55.0-20260810.8n242w/preflight.txt`
(SHA-256 `472c6ddf4864359fe4e8f3048f8a54097e98ca018548e21a55d593aee7610ba2`):
macOS `26.5.2` arm64, Apple Git `2.50.1`, stable Rust `1.95.0`, Xcode
`26.6`, and 37.8 GB free disk at preflight. The gate passed the pinned
archive/source identity and `1045/1046` contract. The generated manifest has
`1045` rows with test-name digest
`b49ad3a4b94a93d77109da4060671ee5ba0fa92fd1b45885023bd5b358ed34a6` and
file SHA-256
`ba7f8c03b683eb984970c103f791e65a77c5664101ba8ca61d3c5d80ab4bafcb`.

No authoritative suite result was produced: the clean `compat` binary build
failed before the runner started (`cargo exit 101`, eight compile errors in
`crates/zmin-cli/src/runtime/object.rs`, `E0425=2` and `E0277=6`). The compact
failure record is
`/Users/dschewchenko/.cache/zmin/w2a-macos-v2.55.0-20260810.8n242w/build-failure-summary.tsv`
(SHA-256 `23e39575388490d13c0738767f139fb54df1e1eb0fcf6615841a92e7d5cd4d7b`);
the raw build log remains local at the same artifact directory. This is a
W3 product compile blocker, not a compatibility result. The status remains
`compatibility_claim=unverified`; no macOS, Linux, or Windows coverage is
implied, and no stock-Git control was run because no clean Zmin binary existed.

## Historical macOS full-manifest checkpoint (2026-08-10; non-authoritative)

The rerun used a clean `git archive HEAD` snapshot at commit
`5a5685aadf15a9671da282a4a5b6f45600252a32`, Rust/Cargo `1.95.0`, the `compat`
profile, macOS `26.5.2` arm64, Apple Git `2.50.1`, and Xcode `26.6`. The
preflight record is
`/Users/dschewchenko/.cache/zmin/w2a-macos-authoritative-20260810.XnYznV/preflight.txt`
(SHA-256 `c9a96ce27754d98f1dfcd7b6f09574db08d76df4e85c08557fe3e58b15559d35`).
The contract archive SHA is
`72923418db7b26dfddc21e2268660c5118e560bdfaa09b4489b67b38e9b69c49`, the
manifest has `1045/1046` rows with full-file SHA-256
`ba7f8c03b683eb984970c103f791e65a77c5664101ba8ca61d3c5d80ab4bafcb` and
test-name digest
`b49ad3a4b94a93d77109da4060671ee5ba0fa92fd1b45885023bd5b358ed34a6`.
The exact built binary hashes are zmin
`aa8aca4017c2c5f220eaf7c588eae1071e165aaa1b42ad8fc2d8e1062bc72b4b` and
`zmin-git-remote-http`
`2b950e8d900a6788d4bbf4daf5d068e902e9f74c1eee3391d251f0b49a11f93a`.

The runner emitted all `1045` manifest rows, with `358 pass` and `687 fail`
(`suite exit 1`). Its metadata is
`/Users/dschewchenko/.cache/zmin/w2a-macos-authoritative-20260810.XnYznV/suite-out/run-metadata.tsv`
(SHA-256 `6fe32fd29e7cc5e5f6b788da11a11f79da5d75d3a7db13f7cc446e65434d90c2`),
and the summary is
`/Users/dschewchenko/.cache/zmin/w2a-macos-authoritative-20260810.XnYznV/suite-out/summary.tsv`
(SHA-256 `da9481178b749cb4a57f025a2aae01665555e892eab7abf4bc8550f3e9bf9fb0`).
The contract check passed, but `validate-run` classified the result as
`exploratory-or-incomplete`; `validate-run ... --require-authoritative`
failed closed. Metadata therefore records
`evidence_scope=authoritative-suite-incomplete` and
`compatibility_claim=unverified`.

The bounded local interruption record lists `t5532-fetch-proxy.sh`,
`t5570-git-daemon.sh`, `t5700-protocol-v1.sh`,
`t5702-protocol-v2.sh`, `t5731-protocol-v2-bundle-uri-git.sh`,
`t5811-proto-disable-git.sh`, `t9300-fast-import.sh`, and
`t9700-perl-git.sh`. Compact first-20-line classification found `677`
ordinary product-failure rows, `8` interrupted product hangs, and `2`
environment/upstream-marker rows (`t4109-apply-multifrag.sh` and
`t7800-difftool.sh`), with `0` harness-failure rows. The largest failure
families were `t40*`
(`67`), `t55*` (`63`), `t41*` (`40`), `t34*` (`38`), `t64*` (`35`), and
`t53*` (`33`). Stock-Git differential control and other platforms were not
run; this checkpoint is not a 100% compatibility claim.

## W3.1 local daemon readiness checkpoint (2026-08-10)

W3.1 applied the smallest shared lifecycle fix: `zmin daemon --verbose` now
emits the upstream-compatible `[numeric-pid] Ready to rumble` line after the
listener and optional pidfile are ready. This unblocks the upstream daemon
helper; it does not implement protocol negotiation, bundle-uri, protocol
policy, or daemon-option semantics.

The exact exploratory rerun used the frozen v2.55.0 source identity and a
temporary five-row manifest. The requested
`t5731-protocol-v2-hidden-refs-http.sh` name is absent from v2.55.0; the
matching frozen-source test is `t5731-protocol-v2-bundle-uri-git.sh`. The
manifest is `/Users/dschewchenko/.cache/zmin/w31-five.u9Wp2S/manifest.tsv`
(raw-file SHA-256
`b2ecb4168893cbc482d09d5ca4c3c62246b59580c9f63e6a7f43e04467d2808f`; the
metadata test-name digest is
`5c8eaaf52ef1738087c3a296a7ba895100888dde5527e9b3adcbe31d64e14454`), and
the summary is
`/Users/dschewchenko/.cache/zmin/w31-five.u9Wp2S/out/summary.tsv` (SHA-256
`849e93022f9855fa9dd0b179f3acd4203da24505418efb6f5f905a3aa182825f`). The
run metadata is
`/Users/dschewchenko/.cache/zmin/w31-five.u9Wp2S/out/run-metadata.tsv` (SHA-256
`8632fcd365270fdfee8e08374d7fa03a1227f20eeb6684c71698f845afa35174`). All
five rows completed as product failures (`0 pass`, `5 fail`), with no target
processes left behind. The run metadata remains exploratory and records
`compatibility_claim=unverified`; the authoritative-required gate fails
closed. Before this fix, all five target runs were interrupted on the
readiness path; `t5570` also had independent option-validation failures. After
it, all five reach their independent product failures.

Remaining W3 slices are separate: remaining `t5570` daemon transport/access
behavior, `t5700`/`t5702` daemon protocol-version dispatch,
`t5731-protocol-v2-bundle-uri-git.sh` bundle-uri support, and `t5811`
`GIT_ALLOW_PROTOCOL` policy.

## W3.2 daemon option-validation checkpoint (2026-08-10)

W3.2 preserved daemon numeric option values as raw strings and validates them
once before daemon side effects. Against the pinned Git v2.55.0 `git-daemon`
(SHA-256 `a0b82a7bdb62aa38b36ebccaa0d9e47341a37ea397d0d3c0843d1bef0f206cec`),
`--timeout` and `--init-timeout` accept decimal `0..4294967295`; invalid or
negative values exit `128` with Git's exact non-negative-integer fatal text.
`--max-connections` accepts signed `-2147483648..2147483647`, including `-1`;
invalid values exit `128` with Git's exact integer fatal text. Invalid runs
create no pidfile and leave no daemon process.

The compat zmin binary SHA-256 is
`4c168b0288838da8f45da37e2b02625c5b9931a439ea28362786e1576a998699`, and the
remote HTTP helper SHA-256 is
`4bca16fd2da209e4ba21259bcb9339f5bb85e050835e401d3cefba56ab21f9fe`.
Focused Rust validation and the W1 contract/audit guards passed. The
exploratory one-test `t5570-git-daemon.sh` artifact is
`/tmp/zmin-w32-t5570.R4Ydgn`; its manifest has one row, raw-file SHA-256
`1bada99f2762eee2e535f8e13a75b68f29a353d412a525e8ebaf8a5a270e751e`, and
metadata test-name digest
`feb1233e5ae8118e3a6cb09bfeeec85751d7e1531ebc9efd9c2576a95638a507`.
Metadata SHA-256 is
`e66f6611a9a8e8b2c51502574a184afbc0f6a306f832d402b6fa50e89b52cec7` and
summary SHA-256 is
`39b6f2dd34e7d574afca1eb16473d66946c2311127217d7005464caad14ece59`.
The official test reported `14/25` subtests passed and `11/25` failed:
`6,7,8,10,14,19,20,21,22,23,25`. The run is exploratory, not authoritative;
the authoritative-required gate failed closed, and no full current-Git or
cross-platform claim is implied.

## W3.3 t5570 informative daemon errors checkpoint (2026-08-10)

W3.3 selected one shared root cause from the residual t5570 failures: the
informative daemon remote-error protocol. Residual classification was:

| IDs | Surface | Classification / next owner |
| --- | --- | --- |
| `6,7` | clone/fetch upload-pack data path | client transport; later slice |
| `8` | verbose no-op fetch output | client transport; later slice |
| `10` | HEAD/ref advertisement | daemon advertisement; later slice |
| `14` | newline URL validation | daemon URL parser; later slice |
| `19-22` | missing, disabled and unexported access errors | W3.3 selected root |
| `23,25` | interpolated host/path access | daemon interpolation; later slice |

The fix matches Git v2.55.0 `daemon_error`: informative mode emits exact
`ERR no such repository`, `ERR service not enabled` and `ERR repository not
exported` payloads, flushes before the stock inetd rc `255`, and maps daemon
upload-pack/receive-pack errors to the stock client text `fatal: remote error:`.
Unknown services retain the stock empty-packet behavior. Generic local, SSH and
HTTP error parsers are unchanged.

The final compat binary SHA-256 is
`8a066cf5bde90641a4f9acf1bd4c80b05dbe7a8858dc529e7afc4a04ba76aa98`; the
remote HTTP helper SHA-256 is
`a079b1e6b9bfe5e757bd4be623dcd968c8a537cbd025fdf12a729ce64d8c9fda`.
The full exploratory t5570 artifact is
`/tmp/zmin-w33-t5570-reviewed.jYScdB`. Its one-row manifest raw SHA-256 is
`1bada99f2762eee2e535f8e13a75b68f29a353d412a525e8ebaf8a5a270e751e`, its
metadata test-name digest is
`feb1233e5ae8118e3a6cb09bfeeec85751d7e1531ebc9efd9c2576a95638a507`, the
summary SHA-256 is
`5ed36dc4a82ed18e267f84be290da769a915c71b5f3ce6da5b70edfc74128d1a`, and
the run-metadata SHA-256 is
`cffd3fa6c67d3441738bd116c4447b21440f012c125543fe285508ef4496f43f`.
The exploratory run completed `17/25` subtests; target IDs `20,21,22` moved
from fail to pass. Residual full-run failures are `6,7,8,10,14,19,23,25`.
The remaining t19 failure is a separate clone-destination side effect: the
earlier default error case leaves `nowhere/`, so the later clone stops before
daemon access. A clean-prerequisite target replay at
`/tmp/zmin-w33-t5570-targets.Mr3u4x` passed `4,5,19,20,21,22` (`6/6`);
its summary SHA-256 is
`2d0fc40db843b777b8bc1637763ca1e699d438e306bd920318b72558e5dbc079`, and
its run-metadata SHA-256 is
`7df011d70ae9a9357e65cd3db528b4738d87617d42e1ca2bd449c36bad1a7dc0`.
Both runs are exploratory and `compatibility_claim=unverified`; no
authoritative current-Git or cross-platform claim is made.

W3.4 on 2026-08-10 fixed the t5570 clone-destination cascade. The git-daemon
clone path now distinguishes a protocol flush from an early transport EOF and
uses a scoped rollback guard through pre-checkout clone stages. An interrupted
or failed new clone destination is removed; an existing non-empty destination
and its user files remain untouched; a valid empty repository still retains
its initialized destination. No daemon error mapping or cross-transport clone
behavior changed.

The focused stock/Zmin evidence passed the new-destination rollback,
pre-existing-file preservation, pre-existing bare-destination preservation,
separate-git-dir rollback, and valid-empty-repository cases, with no child
daemon left running after teardown. The final full exploratory artifact is
`/tmp/zmin-w34-final3.PCett9`; its one-row manifest uses the existing raw SHA
`1bada99f2762eee2e535f8e13a75b68f29a353d412a525e8ebaf8a5a270e751e`, metadata
test-name digest
`feb1233e5ae8118e3a6cb09bfeeec85751d7e1531ebc9efd9c2576a95638a507`, summary
SHA-256 `af0750bd2a00bbeb070ad9db2d31935f2bea0220c3fc4fe6e2b096fcad98a875`,
run-metadata SHA-256
`8ab22b4f819e5fefb1a65d280d24eb21e4ad0c80eee7fb7d4a1425ec70b6f4e7`, and log
SHA-256 `fb5927f1bf8c5a4d9d0b2e34b7e5485b6f19ee6a33dbd9a6f7ddad2647927b64`.
The run remains exploratory (`total=1`, `passed=0`, `failed=1` at the
top-level file; `18/25` subtests pass) with residual IDs `6,7,8,10,14,23,25`.
The compat zmin SHA-256 is
`3168bf7f195018e56d73830fce6bb1d606b86139fcd086cfdc96b5623a891119` and the
remote helper remains
`a079b1e6b9bfe5e757bd4be623dcd968c8a537cbd025fdf12a729ce64d8c9fda`.
`compatibility_claim=unverified`; this does not establish a current-Git or
cross-platform claim.

## W3.5 git-daemon client verbosity checkpoint (2026-08-10)

W3.5 fixed one shared v0 git-daemon client gap for t5570 IDs 6, 7, and 8.
The client now carries `-v` through clone, pull, and fetch discovery and emits
the stock connection diagnostics: `Looking up ... done.` and `Connecting to
... done.`. Stock and Zmin prerequisite-complete probes had matching clone
objects, HEAD, refs, file state, and `ls-remote --symref` output; the prior
difference was the missing stderr diagnostics. No daemon option, readiness,
error, cleanup, protocol-v1/v2, bundle, proxy, or API behavior was changed.

Pinned stock Git v2.55.0 control passed all 25 t5570 subtests. The exploratory
stock artifact is `/tmp/zmin-w35-stock-final.mnBgc8`; its summary SHA-256 is
`8f25da5ea18088969b0697dc52d9c39416db83938c0aa841508a08c1ebe1f703`,
run-metadata SHA-256 is
`3e9d5cbb1216cfbffbc6d49da5c5a0dec778638cdb6bc70341c2fd3fee96ba34`, and
log SHA-256 is
`f933c7a00696fd581e6ae0bbf9fc4b2e7d5a81636d4b98b137fec7c8da466f91`.

The final Zmin exploratory artifact is `/tmp/zmin-w35-zmin-final3.nodLvd`.
Its one-row manifest is the reviewed t5570 manifest with raw SHA-256
`1bada99f2762eee2e535f8e13a75b68f29a353d412a525e8ebaf8a5a270e751e`, and
metadata test-name digest `feb1233e5ae8118e3a6cb09bfeeec85751d7e1531ebc9efd9c2576a95638a507`.
The summary SHA-256 is
`0d24e60b8454b95d4a4f6d05f945606a48c807ee068e989fde53874f3660a85b`,
run-metadata SHA-256 is
`eb12a8b32a3257166e5462e265a6371079e5c6859d5bc135179b12a8649581be`, and
log SHA-256 is
`8cc775c40a507a34ef11447c99a471753ce54555ecf3bbe15a01680220f3a0a6`.
The run is exploratory and unverified (`total=1`, `passed=0`, `failed=1` at
the top-level file); 21/25 t5570 subtests pass. IDs 6, 7, and 8 moved from
fail to pass. Residual IDs are 10, 14, 23, and 25. ID 10 remains the separate
network `remote set-head -a` ownership gap; IDs 14, 23, and 25 are unchanged.

The final compat zmin SHA-256 is `25b07f9eddcf140f16fc6d17e42bf9a7b74734bf3c70fbddde365ebd5fc782e3`;
the remote helper SHA-256 is
`a079b1e6b9bfe5e757bd4be623dcd968c8a537cbd025fdf12a729ce64d8c9fda`.
The W3.5 verbose-propagation correction is committed in `61f0d9c5` and was
built as compat zmin SHA-256
`ad90e13b16dab1d72fe9658187dc780fae1d01ca4cd3b7e05884701850f59bad`; it
passed focused multi-refspec `fetch -v` and `pull --all -v` stock/Zmin
differentials. The exploratory artifact above remains bound to the earlier
binary and is not expanded into a new full-suite claim here.
The pinned stock client and daemon used for differential control have SHA-256
`0c58c409a1689b9668c39f6d5f6db7a566f9772c8dbfa8c4e4ec297f7541fc0e` and
`36c57ddf35d3602a4901355b6075876d1a21b0f1769594000437822a5b973328`.
This checkpoint does not establish a current-Git, 100% compatibility, or
cross-platform claim.

## W3.6 git-daemon remote set-head checkpoint (2026-08-10)

W3.6 fixed t5570 ID 10 for the canonical upstream option order
`remote set-head -d origin` followed by `remote set-head -a origin`. The
schema now parses the typed options before the remote name, and the command
uses the existing git-daemon advertisement parser to read `symref=HEAD`.
After deleting the local remote HEAD, stock and Zmin both returned rc 0 with
empty stderr, printed `'origin/HEAD' is now created and points to 'main'`,
and wrote `refs/remotes/origin/HEAD -> refs/remotes/origin/main`.

The final bounded exploratory t5570 run used the reviewed one-row manifest
with raw SHA-256
`1bada99f2762eee2e535f8e13a75b68f29a353d412a525e8ebaf8a5a270e751e` and
test-name digest
`feb1233e5ae8118e3a6cb09bfeeec85751d7e1531ebc9efd9c2576a95638a507`.
Its local artifact is `/tmp/zmin-w36-final.kCB6Hp`; summary SHA-256 is
`37441c8340db7853efbb9c552ec64e3c22d2e1d07356a333af967afc5c54aa66`,
run-metadata SHA-256 is
`19bbce7df4b89d1603411df1a1d67e470ad4b1534a0836926d4ba2e37bde42f7`, and
log SHA-256 is
`dbdd5ffe1a21958775f55dfb5bcffa82311cca46afe0fb633d9d5218b4009246`.
The compat zmin binary was SHA-256
`16f673e6a3042a2839bb9101664f6862be7dd8c3547b22147f2548f910ad2c85` on
Darwin 25.5.0 arm64 with Rust 1.95.0. The top-level metadata is
`total=1`, `passed=0`, `failed=1` because the run is bound to one manifest
row; the t5570 subtest log is 22/25, with only IDs 14, 23, and 25 failing.
The gate classifies the evidence as exploratory and `compatibility_claim` as
`unverified`; the authoritative-required gate fails closed. This checkpoint
does not establish a current-Git, 100% compatibility, performance, or
cross-platform claim. IDs 14, 23, and 25 remain the next bounded slices.

## W3.7 git-daemon newline URL validation checkpoint (2026-08-10)

W3.7 fixed t5570 ID 14. The pinned Git v2.55.0 source rejects a newline in the
parsed git:// host or repository path before opening the client connection, with
rc 128, empty stdout, and `fatal: newline is forbidden in git:// hosts and repo
paths`. The smallest stock-vs-Zmin probe used
`git://127.0.0.1:<port>/repo\n.git`: stock and Zmin now match rc 128, empty
stdout, the exact fatal line, no destination directory, and no TCP connection.
The typed `ParsedDaemonUrl` boundary performs this validation before request
serialization or transport side effects. The focused parser and integration
tests also cover newline-in-host and newline-in-path inputs.

The final bounded exploratory t5570 run used the reviewed one-row manifest with
raw SHA-256
`1bada99f2762eee2e535f8e13a75b68f29a353d412a525e8ebaf8a5a270e751e` and
test-name digest
`feb1233e5ae8118e3a6cb09bfeeec85751d7e1531ebc9efd9c2576a95638a507`.
Its local artifact is `/tmp/zmin-w37-final.wDopcP`; summary SHA-256 is
`89d97a7200b7297e8ccb0fb8bed298b3488a22d1196e37dc4ed0862d4880b658`,
run-metadata SHA-256 is
`743f671f45c9a17ae85fadb3d4f549ba85651f581acb0a8b13c35a0aa2c997da` and
log SHA-256 is
`9ac2a0573f8c95f78ad381dadddab43676f044aeab42d33447801083cd8828f8`.
The compat zmin binary was SHA-256
`fc1505554e4bd852b7e44405c4c76e586a6d21dd4e116e8e50873eec0fe43db9` on
Darwin 25.5.0 arm64 with Rust 1.95.0. The top-level metadata is `total=1`,
`passed=0`, `failed=1` because the run is bound to one manifest row; the t5570
subtest log passed 23/25, with only IDs 23 and 25 failing. The pinned contract
identity is Git v2.55.0 commit
`e9019fcafe0040228b8631c30f97ae1adb61bcdc` and archive SHA-256
`72923418db7b26dfddc21e2268660c5118e560bdfaa09b4489b67b38e9b69c49`.
The gate classifies this evidence as exploratory and `compatibility_claim` as
`unverified`; the authoritative-required gate fails closed. This checkpoint
does not establish a current-Git, 100% compatibility, performance, or
cross-platform claim. IDs 23 and 25 remain explicit residuals.

## W3.8 git-daemon interpolated-path checkpoint (2026-08-10)

W3.8 diagnosed t5570 IDs 23 and 25 as two layers of the same upstream feature,
but not one identical code defect. Both hit `--interpolated-path=<root>/%H%D`.
The Zmin daemon previously rejected every request when this option was present.
The raw ID 25 probe therefore returned `ERR access denied or repository not
exported: /interp.git`, while pinned stock Git expanded `host=localhost` and
`/interp.git` to `<root>/localhost/interp.git` and advertised `refs/heads/main`.
The ID 23 client probe additionally showed that Zmin does not yet honor the
client-side `GIT_OVERRIDE_VIRTUAL_HOST` environment variable: Zmin sends the
connection host instead of the overridden virtual host. That client-only gap
is left explicit for the next bounded slice.

The W3.8 fix is server-side only. Daemon request parsing now applies Git's
host split, separator sanitization, and lower-case canonicalization for `%H`,
including bracketed IPv6 and ports. It expands the upstream `%H` and `%D`
placeholders, requires a safe absolute expanded path, checks the resolved
expanded path against export directories, and rejects dot, empty, and
parent-directory components. Missing interpolated targets return the stock
access-denied packet without an extra LF and the stock protocol-failure status.
Focused stock/Zmin inetd tests cover canonicalized hosts, path traversal,
allowlist ordering, missing-target packet bytes, and prior daemon behavior.
Existing daemon error, newline, remote-head, verbose, and cleanup tests remain
green.

The follow-up security correction makes strict export-directory matching exact,
while non-strict matching permits descendants as in Git. Resolver rejection and
missing-repository failures now use one typed error path: informative errors say
`no such repository`, unexported repositories say `repository not exported`,
and non-informative errors use the generic access-denied text. All of these
paths flush the packet without an LF and return the daemon protocol-failure
status. Raw requests without extended arguments are covered with active
interpolated-path rejection and base-path strict-allowlist success
differentials; allowlist checks now always use the resolved base/interpolated
target rather than the raw request string.

The final bounded exploratory t5570 run used the reviewed one-row manifest with
raw SHA-256
`1bada99f2762eee2e535f8e13a75b68f29a353d412a525e8ebaf8a5a270e751e` and
test-name digest
`feb1233e5ae8118e3a6cb09bfeeec85751d7e1531ebc9efd9c2576a95638a507`.
Its latest local artifact is `/tmp/zmin-w38-final7.anVj8G`; summary SHA-256 is
`d131f6aa6a494ebfc7a8a85cadeaafbfda27ea0e3f5b2e1d8f6bef52cb58612a`,
run-metadata SHA-256 is
`f0cad444ce07ec9b2c0a526be2453e5f5e867b1e734fcff10323093cf2558cfe`, and
log SHA-256 is
`6526207120ab9f036fd17047d98594226f3ef399d3558f4f1e7d88028e05ad5a`.
The compat zmin binary was SHA-256
`bdb1576636581da3defc78b03019a447ca5b051fce556d801f9f6ae0c4712d3f` on
Darwin 25.5.0 arm64 with Rust 1.95.0. The top-level metadata is `total=1`,
`passed=0`, `failed=1` because the run is bound to one manifest row; the t5570
subtest log passed 24/25, with only ID 23 failing. The run metadata binds the
evidence to Git v2.55.0 commit
`e9019fcafe0040228b8631c30f97ae1adb61bcdc`, archive SHA-256
`72923418db7b26dfddc21e2268660c5118e560bdfaa09b4489b67b38e9b69c49`, and the
same one-row manifest digest above.
The gate classifies this evidence as exploratory and `compatibility_claim` as
`unverified`; the authoritative-required gate fails closed. This checkpoint
does not establish a current-Git, 100% compatibility, performance, or
cross-platform claim. ID 23 remains the explicit client-override residual.

## W3.9 git-daemon virtual-host override checkpoint (2026-08-11)

W3.9 fixed t5570 ID 23's client-side `GIT_OVERRIDE_VIRTUAL_HOST` propagation.
Pinned Git v2.55.0 reads this variable for git:// upload-pack connections and
uses its exact value as the extended `host=` field. The override replaces the
URL host and URL port in that field, while the URL host and port remain the TCP
connection target. The shared daemon request writer covers clone, fetch,
ls-remote, and receive-pack paths; unset variables retain the existing URL
host/port behavior, and newline validation applies to the override as well.

Focused stock/Zmin coverage passed for exact request bytes, URL-port
precedence, the interpolated-host `ls-remote` differential, Unix raw
non-UTF-8 override bytes, raw-LF rejection, and existing clone/fetch/daemon
preservation tests. On Unix, the override is read with `var_os` and carried as
raw bytes; empty and unset values retain their distinct Git semantics. The
final bounded exploratory run used the reviewed one-row manifest with raw
SHA-256
`1bada99f2762eee2e535f8e13a75b68f29a353d412a525e8ebaf8a5a270e751e` and
test-name digest
`feb1233e5ae8118e3a6cb09bfeeec85751d7e1531ebc9efd9c2576a95638a507`.
Its local artifact is `/tmp/zmin-w39-final9.xQdp1W`; summary SHA-256 is
`21dae5f3ff3f297d6ec0f3356a048f7c46c48c6437f26caddaf209d0981e92d5`,
run-metadata SHA-256 is
`c2667bef82421e7a4baaa3d8d2f743e4bad1b24b261773fcfa5dad1929124037`, and
log SHA-256 is
`f933c7a00696fd581e6ae0bbf9fc4b2e7d5a81636d4b98b137fec7c8da466f91`.
The compat zmin binary was SHA-256
`e22da54592db9cee3d3b059b75fbbe36bfb6092b536fc487347d48823f886feb` on
Darwin 25.5.0 arm64 with Rust 1.95.0. The one-row top-level run passed 1/1,
and the t5570 log passed 25/25. This is full-file t5570 evidence only: the W1
gate still classifies it as exploratory because the manifest contains one row,
so it does not establish global 1045-test parity, performance, or
cross-platform compatibility.

## Historical v2.47.1 and targeted evidence

The evidence below records earlier v2.47.1 frontier work and targeted
v2.55.0 probes. It is not the current-Git scope contract above. When a date,
tag or count differs, the contract and generated audit are authoritative.

A fresh integrated `all-nondeprecated` run with offset `0` and limit `50`
passed `50/50` files against pinned Git `v2.47.1`. The previously failing
offset-`50` slice is also effectively `50/50`: all fourteen original failures
now have green complete-file or focused-file reruns, including the final
`t0613-reftable-write-options.sh` (`11/11`),
`t1013-read-tree-submodule.sh`, and `t1022-read-tree-partial-clone.sh` closures.
Together these results cover the first 100 selected top-level files, although
the second 50-file slice has not yet been replayed as one uninterrupted final
run. This is a verified frontier, not a claim that the remaining upstream
manifest is green.

The final offset-`0` run used the current debug binary after the worktree,
ignore, CRLF, cache-tree, split-index, and interactive-patch fixes. Its summary
reported `total=50`, `passed=50`, and `failed=0`. A fresh offset-`100`, limit-`50`
replay reported `total=50`, `passed=40`, and `failed=10`; the current release
binary has SHA-256
`f789a0204b00d3e96fff0ad885e34305c99cc8bf572d11a81e94e0c6b069c632`.
The focused `t1502-rev-parse-parseopt.sh` replay is green at `37/37` after
closing the complete usage/specification and shell-eval compatibility cluster.
The focused `t1416-ref-transaction-hooks.sh` replay is green at `9/9` after
matching Git's transaction phases, queued symref input, and non-atomic push
hook interleaving. The focused rev-parse replays pass
  `t1508-at-combinations.sh` at `35/35`, while `t1506-rev-parse-diagnosis.sh`
  is `29/30` and `t1507-rev-parse-upstream.sh` is now green at `29/29`.
Additional current `v2.55.0` targeted runs are green for the early
conversion/CRLF/encoding files (`t0021`, `t0022`, `t0024`, `t0025`, `t0027`
at `2600/2600`, and `t0028`), text/safety/filesystem edge cases (`t0030`,
`t0031`, `t0035`, `t0050`, `t0055`), credential helpers (`t0301`, `t0302`),
partial clone (`t0410`, `t0411`), and reffiles backend (`t0600`, `t0601`).
The remaining `t0602` difference is an explicit version decision: Git
`v2.55.0` accepts a loose `refs/heads/@` entry, while Zmin targets the
supported Git `v2.47`-family behavior and keeps the local differential gate's
`badRefName` result.
The upstream suite remains the authoritative compatibility denominator; the
closed command catalog and local differential tests do not imply universal Git
parity.

The next pinned v2.47.1 slice (selected files 150–199) initially passed `9/50`.
Focused reruns now pass the six high-leverage closures found there:
`t1511-rev-parse-caret.sh`, `t1513-rev-parse-prefix.sh`,
`t1514-rev-parse-push.sh`, `t1515-rev-parse-outside-repo.sh`,
`t1601-index-bogus.sh`, and `t2100-update-cache-badpath.sh`. These cover
negative message search, revision/path prefixing, push-destination resolution,
separate-git-dir resolution, null-SHA refusal/override in index plumbing, and
directory/file conflict refusal in `update-index`; the remaining slice failures
are broader checkout, root-work-tree, split-index, and index-format gaps.

The current release also passes focused `t1600-index.sh` (`7/7`) and
`t1517-outside-repo.sh` (`10/10`). These close index version/skip-hash policy,
patch/diff behavior outside a repository, empty IMAP input, and the
`remote-http` error contract without a runtime Git fallback.

The split-index slice `t1700-split-index.sh` now passes all `29/29` assertions.
Its validated core covers overlay/replacement/deletion bitmaps, split-index
collapse and re-enable controls, expiry and permission policy, null-SHA
cache-tree safety, alternate shared-index lookup, and `GIT_TEST_SPLIT_INDEX`.

Date: 2026-06-18

This document tracks compatibility against upstream Git test-suite files. It
is intentionally stricter than the local command inventory and smoke tests:
command presence is not counted as behavior parity.

## Upstream strategy

Do not vendor-copy the upstream `t/` suite into this repository.

Use the pinned upstream source in the local cache as the single source of truth:

- pinned source: `~/.cache/zmin/git-upstream/git-v2.55.0`
- machine-readable contract: `tools/git-upstream-compat-contract.tsv`
- default core allowlist:
  `tools/git-upstream-compat-tests-core.txt`
- generated full-core manifest:
  `tools/git-upstream-compat-manifest.sh full-core`
- generated all-upstream manifest:
  `tools/git-upstream-compat-manifest.sh all-top-level`
- generated all-minus-whole-file-deprecated manifest:
  `tools/git-upstream-compat-manifest.sh all-nondeprecated`
- scratch subtree export from the cached upstream suite:
  `tools/git-upstream-compat-materialize.sh full-core /tmp/zmin-upstream-full-core`
  `tools/git-upstream-compat-materialize.sh all-nondeprecated /tmp/zmin-upstream-all-nondeprecated`
- local gitignored snapshot sync:
  `tools/git-upstream-sync.sh nondeprecated`
  `tools/git-upstream-sync.sh refresh-all`
- upstream scope audit:
  `tools/git-upstream-compat-audit.sh legacy-audit`
- frozen contract check:
  `tools/git-upstream-compat-audit.sh contract-check`
- deprecated-surface audit:
  `tools/git-upstream-deprecated-audit.sh audit`
- explicit legacy/external family excludes:
  `tools/git-upstream-compat-tests-legacy-excludes.tsv`
- optional per-file excludes:
  `tools/git-upstream-compat-tests-file-excludes.tsv`

The frozen cached upstream `t/` tree contains `1046` top-level `tNNNN-*.sh`
shell test files. `all-nondeprecated` keeps `1045` of them in the current-Git
denominator. `full-core` keeps `928` files after excluding `118` files from
the narrowed core-only list: the one upstream deprecated group and the five
external-but-current groups. Those external groups remain in
`all-nondeprecated`; they are not deprecated and are not removed from the
current-Git claim.

The deprecated-surface audit currently shows `14` top-level upstream shell
files with explicit deprecated/removal markers and `13` files with the
distinct `WITH_BREAKING_CHANGES` marker; their union is `20`. The latter is a
breaking-change prerequisite, not deprecated/removed evidence. Only `1` of the
deprecated-marker files is currently fully excluded from full-core
(`t5323-pack-redundant.sh` via the explicit legacy-exclude manifest). The
other `13` deprecated-marker files remain in scope because they are
still-supported upstream shell suites with mixed deprecated assertions inside
live commands and repository flows, for example:

- `t1403-show-ref.sh`: deprecated `--heads` coverage inside live `show-ref`
- `t5512-ls-remote.sh`: deprecated `--heads` / `-h` coverage inside live
  `ls-remote`
- `t6120-describe.sh`: deprecated `name-rev --stdin` lane inside live
  `describe` / `name-rev`
- `t4013-diff-various.sh` and `t4202-log.sh`: `whatchanged` lanes mixed into
  the shared diff/log surface

This distinction is non-negotiable: do not treat every upstream
"deprecated"/"scheduled for removal" mention as justification for removing the
whole shell test from the current denominator, and do not treat
`WITH_BREAKING_CHANGES` as deprecated evidence. Mixed deprecated assertions or
breaking-change prerequisite markers inside still-present commands remain in
`all-nondeprecated`.

If a local copied subset is needed for triage or suite consolidation, export
only the selected subtree from the cache instead of vendoring upstream `t/`
into this repository:

```bash
tools/git-upstream-compat-materialize.sh all-top-level /tmp/zmin-upstream-all
tools/git-upstream-compat-materialize.sh all-nondeprecated /tmp/zmin-upstream-all-nondeprecated
tools/git-upstream-compat-materialize.sh full-core /tmp/zmin-upstream-full-core
tools/git-upstream-compat-materialize.sh fully-excluded-deprecated /tmp/zmin-upstream-deprecated-only
tools/git-upstream-sync.sh refresh-all
```

The default local sync copies the current `1045` nondeprecated top-level
upstream shell tests into `.upstream-snapshots/git-v2.55.0/nondeprecated`,
while the explicit whole-file deprecated surface remains isolated under
`.upstream-snapshots/git-v2.55.0/deprecated-only`.

Use `tools/git-compat-test-topology.sh audit` to keep the local Rust compat
layer honest relative to this upstream scope. The policy is:

- upstream shell tests are the authoritative broad contract for stock Git
  behavior;
- local Rust compat suites stay only for faster focused stock-Git oracles,
  invalid-input parity, exact observed IDE/client traces, replace-git dogfood,
  bounded LFS workflow probes, and Zmin-only surfaces;
- do not vendor-copy the upstream `t/` tree into this repository and do not
  answer coverage pressure by adding another parallel pile of issue-specific
  local test files.

Before adding another top-level family exclusion, run:

```bash
tools/git-upstream-compat-audit.sh legacy-audit
tools/git-upstream-deprecated-audit.sh audit
tools/git-compat-test-topology.sh audit
```

The audit must continue to show exact count matches for every explicit exclude
row, and it also prints the neighboring `t90xx`, `t92xx`, `t93xx`, `t97xx`,
and `t99xx` families that currently remain in scope. This prevents the
full-core suite from silently dropping still-relevant Git surfaces such as
`send-email`, `scalar`, `fast-import` / `fast-export`, `Git.pm`, bash
completion, or `git web--browse` just because they live near the external
bridge families in the upstream numbering scheme.

## Historical macOS core-suite checkpoint (2026-07-02; non-authoritative)

The dated macOS direct/debug/oracle results below are historical exploratory
evidence. They cannot establish current compatibility or platform readiness;
the current authoritative runner is Linux-only and Darwin/Windows fail closed
before `make --version`. Refer to the canonical current contract and status
above and to [`performance_evidence_contract.md`](performance_evidence_contract.md).

As of 2026-07-02 on macOS, the new default core allowlist had direct exploratory
evidence for:

- `quick`: green at `3/3`
- `standard`: green at `9/9`

The first broad `all-nondeprecated` macOS batch also had dated evidence through
the file-level manifest batching path on that `debug` binary:

- batch command:
  `ZMIN_BIN=/Users/dschewchenko/.cache/skron-git/cargo-target/debug/zmin ZMIN_UPSTREAM_ALLOW_FAILURES=1 ZMIN_UPSTREAM_MANIFEST_OFFSET=0 ZMIN_UPSTREAM_MANIFEST_LIMIT=50 tools/git-upstream-compat-suite.sh all-nondeprecated`
- summary:
  `/tmp/zmin-upstream-all-nondeprecated-batch1-current.zPqNil/summary.tsv`
- result:
  `50/50` pass, `0/50` fail

Do not treat the older `23/50` and `38/50` manifests for this first
`all-nondeprecated` slice as the active frontier. They remain useful only as
historical progression evidence. The entire earliest `0..49` top-level file
slice was reported closed by that historical local macOS oracle,
including the previously noisy help/alias/path/config and CRLF/conversion
families in that batch.

This latest `50/50` rerun still includes one important
compatibility-accounting rule in the upstream suite wrapper itself.
`t0050-filesystem.sh` did not count as a product failure when the log showed
that every real assertion passed and the shell exits non-zero only because
upstream `TODO known breakage` markers vanished. The focused evidence for that
accounting rule is:

- Zmin focused rerun:
  `/var/folders/l3/y2d_2zz51z731b86_sstzz0h0000gn/T/zmin-t0050-rerun.bGAJmb/summary.tsv`
- stock Git control rerun:
  `/var/folders/l3/y2d_2zz51z731b86_sstzz0h0000gn/T/zmin-stock-t0050.EfvX9m/summary.tsv`

In that environment, stock Git still reproduces the upstream known breakages,
while Zmin now clears the underlying assertions and only trips the stale
`TODO` bookkeeping. The wrapper therefore records the run as `pass` with the
reason suffix `upstream TODO breakage vanished only` instead of treating it as
an active replace-git compatibility gap.

Focused replay evidence now also closes the previous-checkout syntax suite:

- `t0100-previous.sh`: green at `1/1` in
  `/tmp/zmin-upstream-t0100-focused.ys4fv1/summary.tsv`

- `t0101-at-syntax.sh`: green at `1/1` in
  `/tmp/zmin-upstream-t0101-focused.015rWC/summary.tsv`

That dated replay covered stock-compatible `@{-n}` handling across branch deletion,
merge target resolution, ancestor shorthand such as `@{-1}~1`, and reflog
rendering (`log -g @{-1}`) on the current `compat` binary.

The adjacent `@{...}` replay now also covers date-based reflog selectors in
revision arguments, including `@{now}`, absolute historical dates such as
`@{2001-09-17}`, and stock-compatible noisy-token forms like
`@{3.hot.dogs.on.2001-09-17}`.

The dated focused `t0000-basic.sh` rerun on that debug binary was
green at `1/1`:

- summary:
  `/tmp/zmin-upstream-t0000.0zvFCQ/summary.tsv`

That closure came from fixing `diff-files`/`diff-index` handling for
`read-tree` stale-stat entries: Zmin now keeps tracked worktree entries in the
diff set even when the blob content still matches the index and only the index
stat cache is stale.

Additional focused upstream replays since that batch have also closed the
remaining early safety/filesystem blockers except for one still-expected
known-breakage lane:

- `t0003-attributes.sh`: green at `1/1` in
  `/tmp/zmin-upstream-t0003-focused.0qLrjt/summary.tsv`, after fixing
  `check-attr` path normalization so mixed-case existing directory names no
  longer get canonicalized to filesystem case before attribute matching.
- `t0020-crlf.sh`: green at `1/1` in
  `/tmp/zmin-upstream-t0020-focused-rerun.Lpy4iG/summary.tsv`, after fixing
  `crlf` attribute handling so `crlf` forces CRLF checkout output like stock
  Git even without a separate `core.eol=crlf` override.
- `t0030-stripspace.sh`: green at `1/1` in
  `/tmp/zmin-upstream-t0030-focused.LPkDcd/summary.tsv`.
- `t0031-lockfile-pid.sh`: green at `1/1` in
  `/tmp/zmin-upstream-t0031-focused.HcI9O5/summary.tsv`.
- `t0035-safe-bare-repository.sh`: green at `1/1` in
  `/tmp/zmin-upstream-t0035.UVLgMg/summary.tsv`
- `t0026-eol-config.sh`: green at `1/1` in
  `/tmp/zmin-upstream-t0026-focused.dVbAC8/summary.tsv`, after fixing
  `text` attribute precedence so `core.autocrlf=true` overrides `core.eol=lf`
  like stock Git.

The remaining gettext lane was also reported closed on that dated `compat` binary:

- `t0203-gettext-setlocale-sanity.sh`: green at `1/1` in
  `/var/folders/l3/y2d_2zz51z731b86_sstzz0h0000gn/T/zmin-t0203-check2.IooOPZ/summary.tsv`

That closure came from fixing commit identity environment handling so
non-UTF-8 `GIT_AUTHOR_NAME` / `GIT_COMMITTER_NAME` values sourced by the
upstream ISO-8859-1 fixture are no longer dropped by UTF-8-only env decoding
before commit creation.

## Historical macOS second-batch checkpoint (2026-07-02; non-authoritative)

This dated macOS frontier is historical exploratory evidence only and cannot
establish current compatibility or platform readiness. The current
authoritative runner is Linux-only; Darwin and Windows fail closed before
`make --version`.

After the gettext and shell-helper fixes, a dated partial rerun of the next
`all-nondeprecated` batch (`offset=50 limit=50`) produced the following
incremental summary before the run was interrupted during the second half of
the manifest:

- partial summary:
  `/var/folders/l3/y2d_2zz51z731b86_sstzz0h0000gn/T/zmin-all-batch2-refresh.K54oS6/summary.tsv`

The observed pass/fail split in that historical partial summary was:

- pass: `t0092`, `t0095`, `t0100`, `t0101`, `t0200`, `t0201`, `t0202`,
  `t0203`, `t0204`, `t0300`, `t0303`, `t0500`
- fail: `t0091`, `t0210`, `t0211`, `t0212`, `t0213`, `t0301`, `t0302`,
  `t0410`, `t0411`, `t0450`, `t0600`, `t0601`, `t0602`

This established the next historical broad upstream frontier after the gettext
cluster:

- `t0091-bugreport.sh`: report template, system info section, duplicate-file
  refusal, usage/error shape, and enabled-hooks reporting are still incomplete
  in the current built-in implementation
- historical `t0210` to `t0213`: trace2 normal/perf/event/ancestry surfaces
  appeared red in that partial batch snapshot, but those failures were
  contaminated by a stale upstream `t/helper/test-tool` wrapper that still
  pointed at an older `compat`-profile `zmin` path. The runner now rewrites
  that wrapper against the current `ZMIN_BIN` on every non-Windows prepare,
  and the focused reruns below have since closed the quartet
- `t0301` and `t0302`: credential cache/store helper behavior is now green in
  current pinned-v2.55.0 focused replays
- `t0410` and `t0411`: partial-clone suites are now green in current focused
  replays, including clone-from-partial hydration
- `t0450`: generated help/docs parity is still incomplete for upstream
  text-doc versus help validation
- `t0600` and `t0601`: reffiles backend and packed-ref suites are now green;
  `t0602` retains the documented v2.55-only loose-`@` semantic delta

Focused replay evidence now also closes the bugreport shell suite:

- `t0091-bugreport.sh`: green at `1/1` in
  `/var/folders/l3/y2d_2zz51z731b86_sstzz0h0000gn/T/zmin-t0091-check5.qLx6XE/summary.tsv`

Dated pinned-v2.55.0 `t0450-txt-doc-vs-help.sh` replay (2026-07-17)
reports `20` failures among `815` non-expected assertions. Those failures are
not one shared help renderer defect: the v2.55 documentation adds or changes
usage forms for commands that Zmin deliberately exposes at the supported
Git-v2.47.1 surface (for example `add`, `backfill`, `cat-file`, and
`update-ref`). The run remains useful as a drift detector, but it is not a
release gate until the upstream documentation tag is pinned to the same
compatibility version; changing the help text to satisfy v2.55 would regress
the local v2.47.1 differential fixtures.

A pinned Git-v2.47.1 replay of `t1090-sparse-checkout-scope.sh` is green at
`7/7`. The fixes cover sparse-pattern reapplication when switching between
branches at the same commit, the selected-index-bit behavior of
`checkout-index --ignore-skip-worktree-bits`, and local promisor backfill for
the partial-clone lazy-fetch case.

## Historical Trace2 oracle status (non-authoritative)

The macOS oracle/debug results in this section are dated exploratory evidence.
They cannot establish current compatibility or platform readiness. The current
authoritative runner is Linux-only; Darwin and Windows fail closed before
`make --version`. Refer to the canonical current contract and status above and
to [`performance_evidence_contract.md`](performance_evidence_contract.md).

Focused trace2 reruns showed that the earlier red signal for this family was
contaminated by harness state rather than only by product behavior. Two
separate harness issues existed:

- the upstream `t/helper/test-tool` helper was built from the pinned
  older `v2.54.0` tree, while `ZMIN_UPSTREAM_STOCK_GIT_CONTROL=1` previously used the
  ambient `git` from `PATH`, which was a different version on this machine
- the non-Windows `t/helper/test-tool` trace2 wrapper could remain pinned to a
  stale `zmin` artifact path from an older run, so later upstream replays
  silently executed a missing binary instead of the current `ZMIN_BIN`

The harness now builds and uses the pinned upstream `git` binary for
`ZMIN_UPSTREAM_STOCK_GIT_CONTROL=1`, aligning `git` and `t/helper/test-tool`
to the same source tree. It also rewrites the non-Windows trace2
`t/helper/test-tool` shim on every prepare step so focused and broad reruns
always target the current `ZMIN_BIN`.

Historical direct evidence after those fixes:

- `t0210-trace2-normal.sh`: stock-control green at `1/1` in
  `/var/folders/l3/y2d_2zz51z731b86_sstzz0h0000gn/T/zmin-trace2-stock-fixed.h7qELI/out/summary.tsv`
- `t0211-trace2-perf.sh`: stock-control still red at `0/1` in
  `/var/folders/l3/y2d_2zz51z731b86_sstzz0h0000gn/T/zmin-trace2-stock-perf.mPesY6/out/summary.tsv`,
  but only in the two `_run_dashed_` / `remote-http` / `http-fetch` assertions
  that depend on dashed helper propagation through a reduced upstream build.

This means:

- the previous all-red `t0210` stock-control result was not valid evidence of a
  `zmin` product gap;
- the later broad `offset=50 limit=50` failures for `t0210` to `t0213` were
  also not valid product evidence once the stale helper wrapper path was
  identified;
- the historical `trace2` frontier must be split into:
  a valid oracle/harness lane and a product-implementation lane;
- `zmin` still does not have enough built-in trace2 behavior to claim parity,
  but the oracle for the base normal stream is now trustworthy again.

Dated focused product-side progress after the oracle fix:

- `cargo test -q -p zmin-cli --test git_trace2_compat -- --nocapture`
  was green at `5/5` for the then-implemented built-in surface:
  normal/perf lifecycle emission for `zmin version`, config-driven target
  resolution, config/env `def_param` emission, and default credential redaction
  with `GIT_TRACE2_REDACT=0` opt-out, plus unredacted `clone` start/`def_param`
  URL parity for `url.*.insteadOf` remotes.
- A focused upstream rerun against `zmin` is still red:
  `/var/folders/l3/y2d_2zz51z731b86_sstzz0h0000gn/T/zmin-t0210-after-trace2b.W9avTA/out/summary.tsv`
  because most of `t0210` is still driven by upstream `test-tool trace2`
  helper behaviors (`001return`, `002exit`, `003error`, `007bug` to `010bug`,
  and global-config helper lanes) rather than only by ordinary Git command
  execution through the `zmin` CLI.

Dated focused replay after routing upstream `test-tool trace2` helper lanes
through Zmin's built-in `git test-tool trace2` implementation:

- `t0210-trace2-normal.sh`: green at `1/1` in
  `/var/folders/l3/y2d_2zz51z731b86_sstzz0h0000gn/T/tmp.HzXgHhf6xG/out/summary.tsv`
- `t0211-trace2-perf.sh`: green at `1/1` in
  `/var/folders/l3/y2d_2zz51z731b86_sstzz0h0000gn/T/zmin-upstream-compat.jFOMVF/summary.tsv`
- `t0212-trace2-event.sh`: green at `1/1` in
  `/var/folders/l3/y2d_2zz51z731b86_sstzz0h0000gn/T/zmin-upstream-compat.1r08OZ/summary.tsv`
- `t0213-trace2-ancestry.sh`: green at `1/1` in
  `/var/folders/l3/y2d_2zz51z731b86_sstzz0h0000gn/T/zmin-upstream-compat.Natbuu/summary.tsv`

The last `t0211` closure came from fixing the `http-fetch` dashed startup path
so perf targets are prepared before repository discovery truncation and the
trace emits only one root lifecycle (`version` / `start` / `cmd_name`) while
still preserving the `_run_dashed_` child-start and `http-fetch`
depth-1 `def_param` events expected by upstream.

Focused local product coverage expanded alongside that replay:

- `cargo test -q -p zmin-cli --test git_trace2_compat -- --nocapture`: green
  at `13/13`, now covering built-in helper return/error/bug lanes, nested
  child-process perf emission, timer/counter perf summaries, config-driven
  normal/perf/event targets, redaction behavior, clone URL parity, and the
  synthetic `_query_` trace2 cmd-name lane for `git --man-path`, plus the
  single-lifecycle dashed `http-fetch` perf path.

So the `t0210` normal, `t0211` perf, `t0212` event, and `t0213` ancestry
streams are now genuinely closed in the upstream shell suite. The remaining
trace2 family work, if any, now sits outside this previously failing `t0210`
to `t0213` cluster rather than in the earlier helper-routing, event-target,
or `http-fetch` dashed-lifecycle gaps.

That closure came from replacing the previous stub bugreport writer with a
stock-shaped template/system-info/hooks report, duplicate-target refusal, and
pre-clap invalid-argument handling that now matches upstream `bugreport`
stderr/usage shapes for unknown options and stray positional arguments.
- `t0027-auto-crlf.sh`: green at `1/1` in
  `/tmp/zmin-upstream-t0027-focused.37hV9y/summary.tsv`.
- `t0055-beyond-symlinks.sh`: green at `1/1` in
  `/tmp/zmin-upstream-t0055.e3nu7i/summary.tsv`
- `t0050-filesystem.sh`: focused replay now passes all remaining assertions in
  `/tmp/zmin-upstream-t0050-current/t0050-filesystem.log`

Do not keep treating the older broad-batch mentions of `t0003`, `t0020`,
`t0030`, `t0031`, or `t0035` as authoritative active frontier evidence without
rerunning the same broad batch on the current binary. Those older batch
artifacts predate the focused fixes above and are stale for these files.

The current `t0050-filesystem.sh` evidence is important to read precisely:

- suite summary:
  `/tmp/zmin-upstream-t0050-current/summary.tsv`
- focused log:
  `/tmp/zmin-upstream-t0050-current/t0050-filesystem.log`
- current upstream harness result:
  `fail`

That `fail` no longer means the old broad filesystem frontier is still open.
The current log shows:

The latest post-fix focused rerun still has that exact shape:

- summary:
  `/tmp/zmin-upstream-t0050-focused.pPG1Tu/summary.tsv`
- focused log:
  `/tmp/zmin-upstream-t0050-focused.pPG1Tu/t0050-filesystem.log`
- current upstream harness result:
  `fail`

But the reason remains identical: all runtime assertions pass, while upstream
still exits non-zero only because its local `TODO known breakage vanished`
markers now fire. Do not treat `t0050` as an active product-parity failure on
the current macOS `compat` binary.

- `core.ignorecase` init detection is no longer failing
- case-change rename is no longer failing
- directory add with mixed-case path segments is no longer failing
- silent unicode rename/merge lanes are no longer failing
- the `Gitweb`/`gitweb` orphan-reset checkout flow is no longer failing

The remaining upstream `t0050` status is now purely harness/accounting:

- `add (with different case)` now vanishes as a known breakage
- `rename (silent unicode normalization)` now vanishes as a known breakage
- `merge (silent unicode normalization)` now vanishes as a known breakage

So the current earliest upstream frontier must not continue to treat
`t0050-filesystem.sh` as an unresolved filesystem behavior cluster. All
remaining assertions are currently green; the shell file exits non-zero only
because the upstream `expect_failure` markers are now stale for the current
Zmin behavior and need local tracking cleanup.

The next rerun of the same broad early batch after the focused `t0000` closure
is:

- summary:
  `/tmp/zmin-upstream-all-nondeprecated-batch1-rerun4.d3ZsyW/summary.tsv`
- result:
  `40/50` pass, `10/50` fail

Current failing top-level suites in that newest refreshed first large batch
are:

- `t0021-conversion.sh`
- `t0022-crlf-rename.sh`
- `t0024-crlf-archive.sh`
- `t0025-crlf-renormalize.sh`
- `t0028-working-tree-encoding.sh`
- `t0030-stripspace.sh`
- `t0031-lockfile-pid.sh`
- `t0035-safe-bare-repository.sh`
- `t0050-filesystem.sh`
- `t0055-beyond-symlinks.sh`

That broad rerun proves `t0000-basic.sh` is no longer an active member of the
earliest all-nondeprecated frontier. The remaining early red cluster is now:

- conversion and archive/encoding behavior (`t0021`, `t0022`, `t0024`,
  `t0025`, `t0028`)
- text/comment and safety diagnostics (`t0030`, `t0031`, `t0035`)
- filesystem and symlink edge cases (`t0050`, `t0055`)

The latest focused `t0021-conversion.sh` rerun on the current debug binary is
now green at `1/1`:

- summary:
  `/tmp/zmin-upstream-t0021.QoY3rV/summary.tsv`

That closure came from tightening long-running and one-shot filter parity:
Zmin now avoids `SIGPIPE` self-termination when a clean filter stops reading,
only sends `can-delay=1` when a process filter actually advertises `delay`,
preserves the rebased branch ref in filter metadata, and refreshes checkout
metadata without spuriously rerunning clean filters.

The next rerun of the same broad early batch after the focused `t0021`
closure is:

- summary:
  `/tmp/zmin-upstream-all-nondeprecated-batch1-rerun5.K63mUS/summary.tsv`
- result:
  `41/50` pass, `9/50` fail

Current failing top-level suites in that newest refreshed first large batch
are:

- `t0022-crlf-rename.sh`
- `t0024-crlf-archive.sh`
- `t0025-crlf-renormalize.sh`
- `t0028-working-tree-encoding.sh`
- `t0030-stripspace.sh`
- `t0031-lockfile-pid.sh`
- `t0035-safe-bare-repository.sh`
- `t0050-filesystem.sh`
- `t0055-beyond-symlinks.sh`

That broad rerun proves `t0021-conversion.sh` is no longer an active member of
the earliest all-nondeprecated frontier. The remaining early red cluster is
now:

- archive and encoding behavior (`t0022`, `t0024`, `t0025`, `t0028`)
- text/comment and safety diagnostics (`t0030`, `t0031`, `t0035`)
- filesystem and symlink edge cases (`t0050`, `t0055`)

The latest focused `t0022-crlf-rename.sh` rerun on the current debug binary is
now green at `1/1`:

- summary:
  `/tmp/zmin-upstream-t0022-fixed.4DcYYK/summary.tsv`

That closure came from correcting rename similarity scoring so CR characters in
CRLF sequences do not count against the similarity score. Zmin now normalizes
CRLF-to-LF for similarity-only comparisons before running the LCS byte count,
which brings `diff-tree -M` back to stock Git rename detection for CRLF-only
line-ending rewrites.

The next rerun of the same broad early batch after the focused `t0022`
closure is:

- summary:
  `/tmp/zmin-upstream-all-nondeprecated-batch1-rerun6.ICLgEa/summary.tsv`
- result:
  `42/50` pass, `8/50` fail

Current failing top-level suites in that newest refreshed first large batch
are:

- `t0024-crlf-archive.sh`
- `t0025-crlf-renormalize.sh`
- `t0028-working-tree-encoding.sh`
- `t0030-stripspace.sh`
- `t0031-lockfile-pid.sh`
- `t0035-safe-bare-repository.sh`
- `t0050-filesystem.sh`
- `t0055-beyond-symlinks.sh`

That broad rerun proves `t0022-crlf-rename.sh` is no longer an active member
of the earliest all-nondeprecated frontier. The remaining early red cluster is
now:

- archive and encoding behavior (`t0024`, `t0025`, `t0028`)
- text/comment and safety diagnostics (`t0030`, `t0031`, `t0035`)
- filesystem and symlink edge cases (`t0050`, `t0055`)

The latest focused `t0024-crlf-archive.sh` rerun on the current debug binary is
now green at `1/1`:

- summary:
  `/tmp/zmin-upstream-t0024-fixed.cUyMOJ/summary.tsv`

That closure came from routing archive blob export through the shared checkout
smudge pipeline instead of invoking only filter-smudge logic. Zmin archive
entries now honor `core.autocrlf` EOL conversion before archive packaging while
still applying attribute-driven filters through the same runtime path.

The next rerun of the same broad early batch after the focused `t0024`
closure is:

- summary:
  `/tmp/zmin-upstream-all-nondeprecated-batch1-rerun7.xtEv6q/summary.tsv`
- result:
  `43/50` pass, `7/50` fail

Current failing top-level suites in that newest refreshed first large batch
are:

- `t0025-crlf-renormalize.sh`
- `t0028-working-tree-encoding.sh`
- `t0030-stripspace.sh`
- `t0031-lockfile-pid.sh`
- `t0035-safe-bare-repository.sh`
- `t0050-filesystem.sh`
- `t0055-beyond-symlinks.sh`

That broad rerun proves `t0024-crlf-archive.sh` is no longer an active member
of the earliest all-nondeprecated frontier. The remaining early red cluster is
now:

- renormalize and encoding behavior (`t0025`, `t0028`)
- text/comment and safety diagnostics (`t0030`, `t0031`, `t0035`)
- filesystem and symlink edge cases (`t0050`, `t0055`)

The latest focused `t0025-crlf-renormalize.sh` rerun on the current debug
binary is now green at `1/1`:

- summary:
  `/tmp/zmin-upstream-t0025-fixed.uEkQTm/summary.tsv`

That closure came from splitting `git add --renormalize` out of the plain
`-u` lane and forcing tracked matches through a dedicated renormalize clean
path that ignores the old index CRLF bias. The same change also fixed quoted
glob pathspec handling in plain `git add` for nonexistent literal arguments by
expanding matching tracked and worktree paths before staging.

The next rerun of the same broad early batch after the focused `t0025`
closure is:

- summary:
  `/tmp/zmin-upstream-all-nondeprecated-batch1-rerun8.ZNCwuh/summary.tsv`
- result:
  `44/50` pass, `6/50` fail

Current failing top-level suites in that newest refreshed first large batch
are:

- `t0028-working-tree-encoding.sh`
- `t0030-stripspace.sh`
- `t0031-lockfile-pid.sh`
- `t0035-safe-bare-repository.sh`
- `t0050-filesystem.sh`
- `t0055-beyond-symlinks.sh`

That broad rerun proves `t0025-crlf-renormalize.sh` is no longer an active
member of the earliest all-nondeprecated frontier. The remaining early red
cluster is now:

- working-tree encoding behavior (`t0028`)
- text/comment and safety diagnostics (`t0030`, `t0031`, `t0035`)
- filesystem and symlink edge cases (`t0050`, `t0055`)

The latest focused `t0028-working-tree-encoding.sh` replay on the current
`compat` binary is still red, but the shared clean/smudge encoding gap has
been reduced from a broad `18/22` failure shape down to `3/22`:

- summary:
  `/var/folders/l3/y2d_2zz51z731b86_sstzz0h0000gn/T/zmin-upstream-compat.LZFlTn/summary.tsv`
- remaining subcases:
  - UTF-32 checkout EOL conversion lane
  - diff/reporting when invalid encoded garbage is already stored in Git
  - `core.checkRoundtripEncoding` tracing/config parity

That reduction came from adding shared `working-tree-encoding` conversion to
the stage/checkout content pipeline, including UTF-8/UTF-16/UTF-32 and
SHIFT-JIS decoding/encoding support plus BOM validation for the UTF-16 and
UTF-32 families.

Latest core quick summary:
`/var/folders/l3/y2d_2zz51z731b86_sstzz0h0000gn/T/zmin-upstream-core-quick.krhv89/summary.tsv`

Latest core standard summary:
`/tmp/zmin-t3200-after-fix/summary.tsv`

`tools/git-upstream-compat-suite.sh exhaustive` now defaults to the generated
full-core manifest instead of a second hand-curated allowlist. The suite
runner also now has first-class large-batch modes for the broader upstream
surface:

- `all-nondeprecated`: every top-level upstream shell test except the explicit
  whole-file deprecated excludes; current count `1045`
- `all-top-level`: the complete pinned top-level upstream shell suite; current
  count `1046`

That means future upstream work can run the supported core surface, the
all-minus-whole-file-deprecated surface, or the literal full top-level shell
surface directly from the pinned cache without copying Git's `t/` tree into
this repository.

The focused path/config cluster has moved forward since that first broad slice.
After adding command-scope config env parsing for `GIT_CONFIG_PARAMETERS` and
`GIT_CONFIG_COUNT`, stock-style malformed `for-each-repo --config=...`
diagnostics, `log.decorate` config support for `%d`/`%D` history formatting,
and linked-worktree decoration loading from the common refs directory,
`t0068-for-each-repo.sh` is now green again on the current `compat` binary:

- targeted replay summary:
  `/var/folders/l3/y2d_2zz51z731b86_sstzz0h0000gn/T//zmin-upstream-t0068-replay3.Lg0E64/summary.tsv`
- result:
  `1/1` pass

That narrows the previously known path/config frontier to the remaining
`t0056-git-C.sh` and `t0060-path-utils.sh` lanes.

`t0056-git-C.sh` is now also green on the current `compat` binary. The final
closure came from reusing repo-aware absolute path resolution for `git add`
when global `--git-dir` and `--work-tree` are active from a cwd outside the
worktree, so the upstream lane `git --git-dir c/a.git --work-tree=c/a add a.txt`
now matches stock Git instead of resolving `a.txt` against the caller cwd.

- targeted replay summary:
  `/var/folders/l3/y2d_2zz51z731b86_sstzz0h0000gn/T//zmin-upstream-t0056-replay4.EQL2P5/summary.tsv`
- result:
  `1/1` pass

`t0060-path-utils.sh` is now also green on the current `compat` binary. The
final closure came from making `rev-parse --git-path` respect the same
stock-Git routing rules for `GIT_GRAFT_FILE`, `GIT_INDEX_FILE`, and
`GIT_OBJECT_DIRECTORY`, plus the worktree-private exceptions that must ignore
`GIT_COMMON_DIR` (`index`, `index.lock`, `HEAD`, `logs/HEAD*`,
`logs/refs/bisect/*`, `refs/bisect/*`, and `info/sparse-checkout`). The same
pass also restored trailing-slash output for directory-form paths such as
`logs/refs/`.

- targeted replay summary:
  `/var/folders/l3/y2d_2zz51z731b86_sstzz0h0000gn/T//zmin-upstream-t0060-replay3.W2Pz3h/summary.tsv`
- result:
  `1/1` pass

That closes the previously bounded `t0056` / `t0060` / `t0068` path/config
cluster on the current `compat` binary.

`t3200-branch.sh` is now green on the current `compat` binary. The final
closure came from two rebase fixes:

- interactive rebase todo parsing no longer treats decimal-looking abbreviated
  commit ids as positional indexes, so an upstream-style `edit` stop on a hash
  such as `9275698` no longer resolves to the wrong later commit;
- non-root rebase replay now writes the same progress prefix as stock Git on
  the stop lane (`Rebasing (1/2)\rStopped at ...`), which closes the remaining
  observed stderr drift on the `--list during rebase` family.

Current direct evidence:

- targeted current-binary summary:
  `/tmp/zmin-t3200-after-fix/summary.tsv`
- focused local evidence:
  `git_rebase_interactive_compat::branch_list_during_real_interactive_rebase_matches_stock_git`
  `git_rebase_interactive_compat::branch_list_during_real_interactive_rebase_from_detached_head_matches_stock_git`
  `git_rebase_interactive_compat::rebase_interactive_two_commit_todo_order_matches_stock_git`
  `git_transport_local_compat::pull_rebase_interactive_edit_stops_and_continue_matches_stock_git`
  `git_transport_local_compat::pull_rebase_interactive_edit_abort_restores_original_head_like_stock_git`

The earlier `48` `deleting checked-out branch from repo that is a submodule`
setup gap remains closed by bounded upstream evidence
`/tmp/zmin-t3200-bounded48/summary.tsv` and the focused local
`git_submodule_compat::submodule_add_existing_repo_path_matches_stock_git`.

The previously failing `t3700-add.sh` and `t3903-stash.sh` now pass on the
current `compat` binary:

- `t3700-add.sh` targeted rerun summary:
  `/tmp/zmin-upstream-current.LH8Q7m/t3700-add/summary.tsv`
- `t3903-stash.sh` targeted rerun summary:
  `/tmp/zmin-upstream-t3903-final.reclQS/summary.tsv`

The `stash` closure came from two aligned fixes:

- an in-tree upstream-contract test now covers the invalid export/import reject
  sequence:
  `git_stash_compat::stash_import_invalid_exported_commit_contract_matches_upstream_t3903`
- `checkout --orphan` no longer rejects the follow-up orphan transition in this
  state, and now matches stock Git's success/stdout shape on the focused path:
  `git_worktree_state_compat::checkout_orphan_again_after_attaching_current_orphan_branch_matches_stock_git`

## Local runners

macOS:

```bash
ZMIN_UPSTREAM_ALLOW_FAILURES=1 \
tools/git-upstream-compat-suite.sh standard
```

The local runner now resolves Cargo's real target directory before selecting
the built `zmin` binary, so repos with an external `target-dir` no longer need
manual `ZMIN_BIN=...` just to use the upstream harness.

To run an explicit upstream slice outside the default core allowlist, point
`ZMIN_UPSTREAM_TEST_LIST` at a narrower manifest such as
`tools/git-upstream-compat-tests-rev-parse.txt`.

To print the full generated shell-suite scope directly:

```bash
tools/git-upstream-compat-manifest.sh full-core
tools/git-upstream-compat-manifest.sh all-nondeprecated
tools/git-upstream-compat-manifest.sh all-top-level
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

The same runner entrypoint also accepts `all-nondeprecated` and
`all-top-level` when a larger guest batch is needed.

For large-batch triage, the suite runner can now shard the selected top-level
shell manifest directly without creating ad-hoc TSV copies:

```bash
ZMIN_UPSTREAM_MANIFEST_OFFSET=0 \
ZMIN_UPSTREAM_MANIFEST_LIMIT=50 \
tools/git-upstream-compat-suite.sh all-nondeprecated
```

`ZMIN_UPSTREAM_MANIFEST_OFFSET` skips already-covered top-level shell files
after mode resolution, and `ZMIN_UPSTREAM_MANIFEST_LIMIT` caps how many more
top-level shell files run from that point. This is file-level batching across
the generated manifest, not the upstream shell `--run` selector inside an
individual test file.

`upstream-fast` is zmin-only tooling. It reuses
`C:\Users\skron\zmin-target\release\zmin.exe`, skips the Windows native
preflight, and detaches the upstream task for polling. Use the stricter
`upstream` command when a fresh binary/preflight proof is required.

For behavior-only upstream iteration, the local harness can build the faster
Cargo compatibility profile instead of the production release profile:

```bash
ZMIN_UPSTREAM_CARGO_PROFILE=compat tools/git-upstream-compat-suite.sh exhaustive
ZMIN_UPSTREAM_CARGO_PROFILE=compat tools/git-upstream-compat-suite.sh all-nondeprecated
```

The Parallels runner also has a zmin-only compat path:

```bash
tools/parallels-windows-runner.sh upstream-compat exhaustive
tools/parallels-windows-runner.sh upstream-compat all-nondeprecated
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

The first runtime crate split is now in place. `crates/zmin-cli-runtime` owns
stable runtime contracts (`CliError`, `Result`, `GitRepo`, `CloneOptions`) and
the hard-error clone/upload-pack/receive-pack service registry. `zmin-cli`
keeps narrow internal re-exports so command/runtime behavior did not need a
broad rewrite, and no fallback was added for missing service registration.
Validation passed for `cargo check -p zmin-cli --bin zmin --profile compat`,
focused primitive transport adapter tests, primitive runtime mode-stability
tests, submodule clone/update tests, patch-id parity, commit cleanup parity,
scoped rustfmt, and scoped `git diff --check`. Clean no-op
`cargo build -p zmin-cli --bins --profile compat --timings` passed in `0.24s`;
the first timed build after the split had Cargo lock contention and is not
accepted timing evidence.

The follow-up runtime helper extraction moved phase tracing, content-addressed
temporary-file writes, unique temp sibling generation, and remove-if-exists
filesystem helpers into `zmin-cli-runtime`. `zmin-cli` now re-exports these
helpers from the runtime crate instead of compiling separate local modules.
This keeps Git behavior unchanged while moving another commonly touched
runtime surface behind the stable crate boundary. Validation passed for
`cargo check -p zmin-cli --bin zmin --profile compat`, focused
`git_clone_compat clone_` (`10/10`), full `git_pack_integrity_compat`
(`61/61`), touched-file rustfmt with `skip_children=true` where needed, and
`git diff --check`. A clean no-op compat `--bins` build completed in `0.43s`
Cargo time (`0.55s` wall); after touching only
`crates/zmin-cli-runtime/src/phase_trace.rs`, compat `--bins` rebuilt in
`4.06s` Cargo time (`4.14s` wall). That keeps the current runtime-helper edit
loop within the target `<10-20s` macOS range.

The Parallels cleanup path is now bounded by
`ZMIN_PARALLELS_GUEST_EXEC_TIMEOUT_SECONDS` and prints guest cleanup counts
instead of suppressing output from one monolithic `prlctl exec`. Validation:
`bash -n tools/parallels-windows-runner.sh`, bounded cleanup with a 45s limit
returned `guest cleanup complete tasks=0 procs=0 roots=0 temp_roots=0`, and
`tools/parallels-windows-runner.sh tools` returned the Windows version. A
detached Windows upstream smoke using `tools/git-upstream-compat-tests-basic.txt`
with `--run=1 -q` passed `1/1` at
`C:\Users\skron\zmin-upstream-20260619T081022Z-40917-out`; a post-smoke
bounded cleanup again returned `tasks=0`, `procs=0`, `roots=0`, and
`temp_roots=0`. The earlier contended `quick` smoke selected zero tests because
that test list is marked for `exhaustive` mode only.

## Historical measured baseline (dated snapshots; non-authoritative)

The macOS and Windows/Git-for-Windows selected and exhaustive runs below are
dated historical snapshots, not current authoritative readiness evidence. The
current authoritative runner is Linux-only; Darwin and Windows fail closed
before `make --version`. These records cannot establish current platform
readiness; see [`performance_evidence_contract.md`](performance_evidence_contract.md).

Dated full selected macOS `standard` run:
`/var/folders/l3/y2d_2zz51z731b86_sstzz0h0000gn/T/zmin-upstream-compat.wIhuNm/summary.tsv`

Dated full selected Windows/Git-for-Windows `standard` run:
`C:\Users\zmin\zmin-upstream-20260615T043109Z-18392-out\summary.tsv`

Dated expanded macOS `exhaustive` supported-surface run with unsupported
reftable assertions skipped:
`/var/folders/l3/y2d_2zz51z731b86_sstzz0h0000gn/T//zmin-upstream-compat.SUcvtW/summary.tsv`

Dated expanded Windows/Git-for-Windows `exhaustive` supported-surface run with
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
`/tmp/zmin-upstream-t0000.0zvFCQ/summary.tsv`

Latest targeted `t0021-conversion.sh` macOS run:
`/tmp/zmin-upstream-t0021.QoY3rV/summary.tsv`

Latest targeted `t0022-crlf-rename.sh` macOS run:
`/tmp/zmin-upstream-t0022-fixed.4DcYYK/summary.tsv`

Latest targeted `t0024-crlf-archive.sh` macOS run:
`/tmp/zmin-upstream-t0024-fixed.cUyMOJ/summary.tsv`

Latest targeted `t0025-crlf-renormalize.sh` macOS run:
`/tmp/zmin-upstream-t0025-fixed.uEkQTm/summary.tsv`

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

## Historical macOS/Windows evidence retained for context

The macOS and Windows/Git-for-Windows results in this section are historical
exploratory evidence produced before the current descriptor-bound authoritative
runner was sealed. They do not establish current-Git readiness or current
authoritative platform coverage. The current authoritative upstream and
performance runner supports Linux only; Darwin and Windows fail closed before
`make --version`, as specified by the
[`performance_evidence_contract.md`](performance_evidence_contract.md).

- Historical macOS and Windows/Git-for-Windows runs could execute the earlier
  upstream compatibility harness locally.
- Historical quick and selected standard suites were green on both platforms.
- Historical expanded supported-surface evidence reported `15/15` selected
  files on macOS and `15/15` selected files on Windows/Git-for-Windows, with
  reftable assertions skipped as unsupported. This is retained as exploratory
  evidence only; it is not a current Windows readiness claim.

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
- Historical exploratory behavior evidence was green only for the selected
  supported upstream surface: expanded runs reported `16/16` selected files on
  macOS and Windows/Git-for-Windows with unsupported reftable assertions
  skipped. This does not override the current Linux-only authoritative runner.
- Additive Zmin CLI surface is not counted as upstream parity, but its
  canonical `.git` behavior is currently covered by focused macOS and Windows
  tests for managed hooks, CMS porcelain, local/remote `clone --instant`,
  `--background-fetch`, and `--demand-hydrate`.
- Practical replace-git dogfood gates are currently green on the main macOS
  workspace for the audited lanes they model: `cargo test -q -p zmin-cli --test
  git_runtime_dependency_audit -- --nocapture` passed `1/1`, `cargo test -q -p
  zmin-cli --test git_replacement_dogfood_compat -- --nocapture` passed `5/5`,
  and `cargo test -q -p zmin-cli --test git_observed_client_compat --
  --nocapture` passed `16/16` on 2026-07-02. Those gates strengthen confidence
  in git-in-PATH replacement, IDE/client command shapes, and test-only
  confinement of audited stock-Git runtime patterns, but they do not upgrade
  the project to full Git parity on their own.
- Historical exploratory performance measurements were collected on macOS and
  Windows with Gitoxide where comparable. They are not current authoritative
  performance evidence: authoritative mode disables Gitoxide and the current
  descriptor-bound Make runner is Linux-only. See
  [`performance_evidence_contract.md`](performance_evidence_contract.md).

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
  `extensions.refStorage=reftable`) has partial reader, writer, and stack
  support in the current source, but its current-Git acceptance remains open
  pending complete clone/ref-resolution/fsck and platform evidence. Supported-
  surface upstream runs may set `ZMIN_UPSTREAM_SKIP_UNSUPPORTED_REFTABLE=1`;
  full upstream runs without that flag must still report unverified reftable
  assertions rather than treating partial implementation as parity.
- Fast-import pack and edge-pack publication is currently unsupported on
  Windows: the product path fails closed before Windows handle-relative
  publication ([`import_impl.rs`](../../crates/zmin-cli/src/cli/commands/import_impl.rs#L1781)).
  This is an implementation blocker, not a denominator exclusion;
  `t9300-fast-import.sh` remains in `all-nondeprecated`, and historical
  Windows selected-surface evidence must not be reported as Windows t9300
  readiness. The platform evidence policy is centralized in
  [`performance_evidence_contract.md`](performance_evidence_contract.md).
- Authenticated HTTPS/SSH transport, credential-helper flows, corporate proxy
  handling, custom enterprise TLS/proxy environments, and long-running
  non-loopback network scenarios are outside the current supported parity claim
  until each has a dedicated local and Windows scenario gate. Existing
  unauthenticated public-provider and loopback transport smokes do not imply
  parity for these environments. The currently verified helper-backed auth
  surface covers repo-configured `git credential fill|approve|reject` flows for
  `credential.helper=store` and `credential.helper=cache`, plus loopback smart
  HTTP discovery using `credential.helper=store`, including quoted `--file`
  paths with spaces (`git_credential_compat::credential_fill_uses_configured_store_helpers_like_stock_git`,
  `git_credential_compat::credential_approve_and_reject_use_configured_store_helper_like_stock_git`,
  `git_credential_compat::credential_fill_approve_and_reject_use_configured_cache_helper_like_stock_git`,
  `git_transport_http_compat::ls_remote_sends_basic_auth_from_credential_store_helper_with_quoted_file_path`
  and
  `git_replacement_dogfood_compat::replacement_dogfood_smoke_script_passes_with_current_zmin_binary`)
  integrate the current git-in-PATH replacement smoke into `cargo test`, so
  IDE-shaped shim flows are now covered by a durable automated gate instead of
  only a manual shell step. That replacement smoke now also covers shim-level
  `git lfs version`, `git lfs env`, and empty-repo `git lfs ls-files` probes as
  client/plugin readiness evidence for the built-in local LFS foundation.
  `transport_impl::tests::parsed_http_url_reads_credential_store_helper_basic_auth_with_quoted_file_path`).
- Arbitrary Git LFS ecosystem parity, partial clone filter negotiation beyond the
  explicit demand-hydration surface, sparse-checkout expansion, signed
  commit/tag verification workflows, and platform-specific file watcher /
  daemon behavior remain outside the current parity claim unless a later test
  slice explicitly adds them. The currently verified LFS-compatible surface
  covers pointer workflows through configured `filter.lfs.process` on `add`,
  `checkout`, and `cat-file --filters`
  (`git_object_plumbing_compat::lfs_process_filter_pointer_workflow_matches_stock_git`)
  plus built-in local `git lfs` foundation commands:
  `version`, `env`, `install --local --skip-repo`,
  `install --local --skip-smudge`, `track`, `untrack`, `ls-files` (including
  single historical `<ref>` parity plus invalid-ref stderr parity), and local
  `pre-push` stdin-shape validation
  (`git_lfs_local_compat::{lfs_track_and_untrack_match_stock_git_for_basic_patterns,lfs_install_local_skip_repo_matches_stock_git_filter_config,lfs_install_local_skip_smudge_writes_builtin_pre_push_hook,lfs_ls_files_default_name_only_and_long_match_stock_git_for_pointer_entries,lfs_ls_files_ref_argument_matches_stock_git_for_historical_pointer_tree,lfs_ls_files_invalid_ref_matches_stock_git_error_shape,lfs_version_and_env_report_builtin_local_foundation_state,lfs_pre_push_validates_update_stream_shape}`).
  The replacement-git smoke also now covers hook-callback dogfood for
  `git lfs post-commit`, `git lfs post-checkout`, and `git lfs post-merge`
  through the shim path, so repositories with standard installed Git LFS hook
  wrappers no longer immediately fall off the local replace-git path.
  The stable extension slice also covers HTTP Batch download/upload,
  action-specific and credential-helper authentication, scoped proxy/TLS/header
  policy, and rolling dial/TLS/activity timeouts
  (`git_lfs_network_commands` plus the focused `lfs_batch`, `lfs_auth`,
  `lfs_network_session`, `lfs_transfer`, and `lfs_http_policy` suites).
  Configured mTLS/client identity remains an explicit fail-before-network
  transport exclusion. Custom transfer adapters, untracked Git LFS commands,
  and arbitrary client/plugin ecosystem compatibility are not claimed; none of
  these extension boundaries changes the 1045-test Git denominator.
- Larger real-repository scale scenarios are not complete. Existing real-repo
  smokes are useful preservation evidence, but they are not a substitute for a
  documented scale matrix with repository size, object count, ref count,
  transport, auth/proxy mode, and macOS/Windows results.

## Current Git Linux replay evidence

The manual current-Git replay is the authoritative 1,045-file upstream
denominator: Git v2.55.0 has 1,046 top-level shell tests and only
`t5323-pack-redundant.sh` is excluded. The retained git-svn, git-cvsserver,
gitweb, cvsimport, and git-p4 families remain in scope; optional external
client assertions are reported from raw logs rather than assumed to run.

The replay binds the v2.55.0 archive SHA-256
`72923418db7b26dfddc21e2268660c5118e560bdfaa09b4489b67b38e9b69c49`, annotated
tag object `5ce91c059e41090e7d2cffad39c04af8acf98dc1`, and peeled commit
`e9019fcafe0040228b8631c30f97ae1adb61bcdc` through
`tools/git-current-compat-contract.json`. Full authority requires
`per_test_timeout=0`; a nonzero timeout is diagnostic.

The cached current state is 358/1045 pass, 687 fail, with two timeout
markers, and remains unverified. It must not be described as a current Linux
compatibility result. That claim is withheld until a complete
`per_test_timeout=0` replay succeeds in both stock and Zmin lanes with zero
fully skipped retained top-level tests; assertion-level platform skips are
reported separately.

## Completion rule

This goal is not complete until:

- selected upstream quick, standard, and agreed exhaustive files are green on
  macOS and Windows/Git-for-Windows, or every remaining failure has an explicit
  unsupported/out-of-scope decision;
- local scenario tests still pass;
- Windows validation runs locally through Parallels without GitHub Actions;
- the compatibility evidence matrix links to the latest passing local outputs.
