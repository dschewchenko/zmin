# Matrix Row Growth Audit

This file explains behavior-row count growth on
`compat/status-pathspec-matrix` and defines the guardrail for future matrix
imports.

## Census-First Correction

As of 2026-06-22, do not continue importing rows directly from
`docs/cli/existing_oracle_test_inventory.tsv`. The next compatibility work
starts from `docs/cli/git_compatibility_census.md` and
`docs/cli/census/remaining_to_fix_or_verify.tsv`.

As of 2026-06-28 the latest completed batch is a helper-free local
`replay` formatter/output/revision-selector expansion across the existing
bounded linear advance lane. The selected change added `+26` matrix rows and
promoted twenty-six documented `replay` option pairs into the
reviewed-complete census set: `--abbrev-commit`, `--no-abbrev-commit`,
`--encoding`, `--expand-tabs`, `--no-expand-tabs`, `--pretty`, `--oneline`,
`--format`, `--date`, `--relative-date`, `--notes`, `--no-notes`,
`--show-notes`, `--show-notes-by-default`, `--standard-notes`,
`--no-standard-notes`, `--quiet`, `--show-linear-break`,
`--in-commit-order`, `--since-as-filter`, `--simplify-merges`,
`--simplify-by-decoration`, `--ancestry-path`, `--bisect`,
`--exclude-first-parent-only`, and `--exclude-hidden`. This was a bounded
runtime-and-evidence slice: replay now accepts the current formatting and
revision-selector family on the modeled local linear advance lane without
widening into graph/header output, walk-reflog-only failures, merge-only
pseudoref semantics, or multi-source range selection. Actual delta is `+26`
matrix rows, `+26` complete documented option pairs, `+26` represented
documented option pairs, `+26` verified rows, `+0` invalid-input rows, and
`+0` complete command matrices. Current census counts are `7358` matrix rows,
`6473` verified rows, `860` invalid-input rows, `0` exact-open rows,
`146/151` complete command matrices, `2950/3212` complete documented option
pairs, and `2950/3212` represented documented option pairs. Remaining
checklist rows fell from `288` to `262`. The guardrail held: row growth stayed
inside one coherent helper-free replay family verified by the dedicated
bounded stock-Git smoke script.

As of 2026-06-28 the latest completed batch is a helper-free local
`replay` formatter/filter/revision-selector expansion across the existing
bounded linear advance lane. The selected change added `+30` matrix rows and
promoted thirty documented `replay` option pairs into the reviewed-complete
census set: `--exclude`, `--author`, `--committer`, `--since`, `--after`,
`--until`, `--before`, `--grep`, `-i`, `--regexp-ignore-case`,
`--fixed-strings`, `-F`, `--basic-regexp`, `--extended-regexp`, `-E`,
`--perl-regexp`, `-P`, `--all-match`, `--invert-grep`, `--dense`,
`--sparse`, `--full-history`, `--children`, `--do-walk`, `--reflog`,
`--max-age`, `--max-parents`, `--no-max-parents`, `--min-parents`, and
`--no-min-parents`. This was a bounded runtime-and-evidence slice: replay now
accepts the current matching/wide no-op filter and traversal-control family on
the modeled local linear advance lane without widening into graph/header
output, walk-reflog-only failures, merge-only pseudoref semantics, or
multi-source globbing. Actual delta is `+30` matrix rows, `+30` complete
documented option pairs, `+30` represented documented option pairs, `+30`
verified rows, `+0` invalid-input rows, and `+0` complete command matrices.
Current census counts are `7332` matrix rows, `6447` verified rows, `860`
invalid-input rows, `0` exact-open rows, `146/151` complete command
matrices, `2924/3212` complete documented option pairs, and
`2924/3212` represented documented option pairs. Remaining checklist rows
fell from `318` to `288`. The guardrail held: row growth stayed inside one
coherent helper-free replay family verified by the dedicated bounded
stock-Git smoke script.

As of 2026-06-28 the latest completed batch is a helper-free local
`replay` represented-family expansion across the existing bounded linear
advance lane. The selected change added `+21` matrix rows and promoted
twenty-one documented `replay` option pairs into the reviewed-complete census
set: `--alternate-refs`, `--cherry`, `--cherry-mark`, `--cherry-pick`,
`--first-parent`, `--ignore-missing`, `--indexed-objects`, `--left-right`,
`--max-count`, `--no-merges`, `--no-walk`, `--objects`,
`--objects-edge`, `--objects-edge-aggressive`, `--parents`,
`--remove-empty`, `--right-only`, `--show-signature`,
`--single-worktree`, `--skip`, and `--unpacked`. This was a bounded
runtime-and-evidence slice: replay now accepts the current rev-list-derived
no-op selector family on the modeled local linear advance lane without
widening into formatting, grep, history-shape rewriting, merge replay, or
multi-branch update semantics. Actual delta is `+21` matrix rows, `+21`
complete documented option pairs, `+21` represented documented option pairs,
`+21` verified rows, `+0` invalid-input rows, and `+0` complete command
matrices. Current census counts are `7302` matrix rows, `6417` verified rows,
`860` invalid-input rows, `0` exact-open rows, `146/151` complete command
matrices, `2894/3212` complete documented option pairs, and
`2894/3212` represented documented option pairs. Remaining checklist rows
fell from `339` to `318`. The guardrail held: row growth stayed inside one
coherent helper-free replay option family verified by the dedicated bounded
stock-Git smoke script.

As of 2026-06-28 the latest completed batch is a helper-free local
`replay` rev-walk option-family expansion across the existing bounded linear
advance lane plus one stdin-fed range lane. The selected change added `+11`
matrix rows and promoted eleven documented `replay` option pairs into the
reviewed-complete census set: `--all`, `--author-date-order`, `--branches`,
`--count`, `--date-order`, `--not`, `--remotes`, `--reverse`, `--stdin`,
`--tags`, and `--topo-order`. This was a bounded runtime-and-evidence slice:
the replay parser and history command wiring now accept the current rev-walk
no-op/warning selector family on the modeled local linear advance lane,
including stock warning passthrough for `--date-order`, `--author-date-order`
and `--reverse`, stdin-fed revision range intake, and stock invalid-input
handling for multi-source `--branches` / `--all` advance and empty-selection
`--not`. Actual delta is `+11` matrix rows, `+11` complete documented option
pairs, `+11` represented documented option pairs, `+8` verified rows, `+3`
invalid-input rows, and `+0` complete command matrices. Current census counts
are `7281` matrix rows, `6396` verified rows, `860` invalid-input rows, `0`
exact-open rows, `146/151` complete command matrices, `2873/3212` complete
documented option pairs, and `2873/3212` represented documented option pairs.
Remaining checklist rows fell from `350` to `339`. The guardrail held: row
growth stayed inside one coherent helper-free replay family with direct
stock-Git oracle evidence from the new bounded smoke script, not from a broad
oracle import.

As of 2026-06-28 the latest completed batch is a helper-free local
`pack-objects` represented-family expansion across the existing explicit-
basename all-refs lane plus the current loose-object stdout lane. The selected
change added `+14` matrix rows and promoted fourteen documented
`pack-objects` option pairs into the reviewed-complete census set: `-q`,
`--all-progress-implied`, `--honor-pack-keep`, `--include-tag`,
`--incremental`, `--keep-true-parents`, `--local`, `--no-sparse`,
`--non-empty`, `--shallow`, `--sparse`, `--threads`, `--unpacked`, and
`--window-memory`. This was a bounded runtime-and-evidence slice: the parser
and pack command wiring now accept the current no-op/implied local option
family without widening into thin-pack generation, filter selection,
keep-pack, cruft/unreachable handling, or max-pack splitting. Actual delta is
`+14` matrix rows, `+14` complete documented option pairs, `+14`
represented documented option pairs, `+14` verified rows, `+0`
invalid-input rows, and `+0` complete command matrices. Current census counts
are `7270` matrix rows, `6388` verified rows, `857` invalid-input rows, `0`
exact-open rows, `146/151` complete command matrices, `2862/3212` complete
documented option pairs, and `2862/3212` represented documented option pairs.
Remaining checklist rows fell from `364` to `350`. The guardrail held: the
new rows came from one coherent helper-free documented option family with
existing stock-Git evidence, not from a broad oracle import.

As of 2026-06-28 the latest completed batch is a zero-row-growth
represented-family expansion across the already represented `daemon`
surface. The selected change added `+8` matrix rows and promoted eight
documented `daemon` option pairs into the reviewed-complete census set:
`--enable`, `--disable`, `--allow-override`, `--forbid-override`,
`--informative-errors`, `--no-informative-errors`, `--log-destination`, and
`--syslog`. This was a bounded runtime-and-evidence slice: the daemon parser
and transport command wiring now accept the service-toggle and logging family
without widening into process-management semantics such as detach, user/group
switching, interpolated paths, or access hooks. Actual delta is `+8` matrix
rows, `+8` complete documented option pairs, `+8` represented documented
option pairs, `+8` verified rows, `+0` invalid-input rows, and `+0`
complete command matrices. Current census counts are `7256` matrix rows,
`6374` verified rows, `857` invalid-input rows, `0` exact-open rows,
`146/151` complete command matrices, `2848/3212` complete documented option
pairs, and `2848/3212` represented documented option pairs. Remaining
checklist rows fell from `372` to `364`. The guardrail still held: row growth
stayed proportional to one coherent documented option family, not to a broad
oracle import.

As of 2026-06-28 the latest completed batch is a zero-row-growth
review-closure promotion across the already represented `fast-import`
surface. The selected change added `+0` matrix rows and promoted the
remaining `18` documented `fast-import` option pairs into the
reviewed-complete census set, closing the command at `19/19` reviewed-complete
documented option pairs. This was an evidence-only closure with no runtime
change: the exact stock-Git rows already covered the current helper-free local
done/progress framing lanes, bounded no-op control values, marks
import/export families, relative-marks path resolution, unsafe stream-feature
marks lanes, max-pack-size warning behavior, and rewrite-submodules invalid
input parity. Actual delta is `+0` matrix rows, `+18` complete documented
option pairs, `+0` represented documented option pairs, `+0` verified rows,
`+0` invalid-input rows, and `+0` complete command matrices. Current census
counts are `7248` matrix rows, `6366` verified rows, `857` invalid-input
rows, `0` exact-open rows, `146/151` complete command matrices,
`2836/3212` complete documented option pairs, and
`2840/3212` represented documented option pairs. Remaining checklist rows fell
from `394` to `376`. This is another zero-row-growth closure batch, so the
guardrail held exactly: only reviewed-complete coverage changed; the
evidence-layer denominator did not.

As of 2026-06-28 the latest completed batch is a zero-row-growth
review-closure promotion across the already represented `help`,
`fast-export`, and `rev-list` surfaces. The selected change added `+0`
matrix rows and promoted forty-four existing documented option pairs into the
reviewed-complete census set: all `17` documented `help` options, all `18`
documented `fast-export` options, and the remaining `9` documented
`rev-list` options. This was an evidence-only closure with no runtime change:
the exact stock-Git rows already covered the outside-repository help listing
families, the bounded helper-free local fast-export stream-shaping families,
and the remaining rev-list bisect/graph/header/object-edge tail. Actual delta
is `+0` matrix rows, `+44` complete documented option pairs, `+0`
represented documented option pairs, `+0` verified rows, `+0` invalid-input
rows, and `+0` complete command matrices. Current census counts are `7248`
matrix rows, `6366` verified rows, `857` invalid-input rows, `0`
exact-open rows, `146/151` complete command matrices, `2818/3212` complete
documented option pairs, and `2840/3212` represented documented option pairs.
Remaining checklist rows fell from `438` to `394`. This is another
zero-row-growth closure batch, so the guardrail held exactly: only
reviewed-complete coverage changed; the evidence-layer denominator did not.

As of 2026-06-28 the latest completed batch is a helper-free local
`fast-import` repeated marks-file and option-order expansion across the
existing done-terminated simple commit lane and the current import-marks
preload lane with prewritten helper blobs. The selected change added `+11`
exact stock-Git rows for repeated and reordered marks-file forms:
repeated `--export-marks` to the same path, repeated `--export-marks` with a
trailing different path, repeated missing `--import-marks-if-exists`, both
orders of `--no-relative-marks` with `--export-marks`, both orders of
`--import-marks-if-exists=missing.marks` with `--export-marks=out.marks`,
repeated `--import-marks` with duplicate mark ids in both orders, and
repeated `--import-marks-if-exists` with duplicate mark ids in both orders.
The runtime closure stayed intentionally narrow: this slice only removes
clap-level rejection for repeated marks-file command-line options, preserves
stock trailing export target selection, and preloads repeated
`--import-marks*` files in order so duplicate mark ids obey stock
last-file-wins behavior. Actual delta is `+11` matrix rows, `+0` complete
documented option pairs, `+0` represented documented option pairs, `+11`
verified rows, `+0` invalid-input rows, and `+0` complete command matrices.
Current census counts are `7226` matrix rows, `6351` verified rows, `850`
invalid-input rows, `0` exact-open rows, `146/151` complete command
matrices, `2774/3212` complete documented option pairs, and
`2840/3212` represented documented option pairs. `fast-import` now sits at
`19/19` represented documented option pairs with `71/71` classified rows, but
only `1/19` reviewed-complete documented option pairs. This remains an
expansion batch rather than a reviewed-complete closure because the newly
closed rows widen exact evidence on the current bounded local oracle without
exhausting the broader values, combinations, in-stream features, state, and
transport tails for the documented marks surface.

As of 2026-06-28 the latest completed batch is a helper-free local
`rev-list` represented-tail expansion across the existing two-commit local
history lane, the patch-equivalent symmetric-difference lane, and the current
helper-free raw-output surface. The selected change added `+9` exact
stock-Git rows for `--bisect`, `--bisect-all`, `--bisect-vars`, `--cherry`,
`--disk-usage`, `--graph`, `--header`, `--objects-edge`, and
`--objects-edge-aggressive`. The runtime closure stayed intentionally narrow:
`rev-list` now threads the remaining schema spellings into the history
pipeline, matches stock midpoint and distance reporting on the modeled bisect
lane, emits stock plain-cherry markers on the symmetric-difference lane,
reports commit-only disk bytes on the loose-object local lane, writes stock
NUL-delimited raw header records, renders the stock linear graph prefix lane,
and treats `--objects-edge*` as the current bounded stock object walk when no
excluded edges are present. Actual delta is `+9` matrix rows, `+0` complete
documented option pairs, `+9` represented documented option pairs, `+9`
verified rows, `+0` invalid-input rows, and `+0` complete command matrices.
Current census counts are `7088` matrix rows, `6222` verified rows, `841`
invalid-input rows, `0` exact-open rows, `146/151` complete command
matrices, `2774/3212` complete documented option pairs, and
`2783/3212` represented documented option pairs. `rev-list` now sits at
`117/117` represented documented option pairs with `169/169` classified rows,
but only `108/117` reviewed-complete documented option pairs. This remains an
expansion batch rather than a reviewed-complete closure because the newly
represented nine-option tail still sits in `doc_option_expansion_required`.

As of 2026-06-27 the latest completed batch is a helper-free `ls-remote`
option-family review closure across the already represented configured-local,
file URL, dumb HTTP, smart HTTP, git-daemon, and SSH lanes. The selected
change added `+0` matrix rows and promoted eleven existing `ls-remote`
doc-option pairs into the reviewed-complete census set:
`--branches`, `--exit-code`, `--get-url`, `--quiet`, `--server-option`,
`--sort`, `--symref`, `-b`, `-o`, `-q`, and `-t`. This was an evidence-only
closure with no runtime change: the exact stock-Git rows already covered
successful and invalid-input transport lanes, including `--exit-code` miss
rejections and SSH protocol-v2-required `server-option` failures. Actual delta
is `+0` matrix rows, `+11` complete documented option pairs, `+0`
represented documented option pairs, `+0` verified rows, `+0` invalid-input
rows, and `+0` complete command matrices. Current census counts are `6851`
matrix rows, `6006` verified rows, `820` invalid-input rows, `0`
exact-open rows, `146/151` complete command matrices, `2548/3212` complete
documented option pairs, and `2548/3212` represented documented option pairs.
Remaining checklist rows fell from `675` to `664`. This is another
zero-row-growth closure batch, so the guardrail held exactly: the
`ls-remote` review promotion changed only reviewed-complete coverage, not the
evidence-layer denominator.

As of 2026-06-27 the latest completed batch is a helper-free `diff*`
dirstat-family review closure across the already represented local porcelain
and plumbing lanes. The selected change added `+0` matrix rows and promoted
fourteen existing dirstat-family doc-option pairs into the
reviewed-complete census set: `diff --cumulative`, `diff -X`,
`diff-files --cumulative`, `diff-files --dirstat`,
`diff-files --dirstat-by-file`, `diff-files -X`,
`diff-index --cumulative`, `diff-index --dirstat`,
`diff-index --dirstat-by-file`, `diff-index -X`,
`diff-tree --cumulative`, `diff-tree --dirstat`,
`diff-tree --dirstat-by-file`, and `diff-tree -X`. This was an
evidence-only closure with no runtime change: the exact stock-Git rows already
covered the modeled treeish-pair, dirty-worktree, index-vs-tree, and
tree-to-tree dirstat lanes. Actual delta is `+0` matrix rows, `+14`
complete documented option pairs, `+0` represented documented option pairs,
`+0` verified rows, `+0` invalid-input rows, and `+0` complete command
matrices. Current census counts are `6851` matrix rows, `6006` verified rows,
`820` invalid-input rows, `0` exact-open rows, `146/151` complete command
matrices, `2537/3212` complete documented option pairs, and
`2548/3212` represented documented option pairs. Remaining checklist rows fell
from `689` to `675`. This is a zero-row-growth closure batch, so the guardrail
held exactly: the diff-family review promotion changed only reviewed-complete
coverage, not the evidence-layer denominator.

As of 2026-06-27 the latest completed batch is a helper-free `ls-remote`
option-family transport expansion across the existing file URL, dumb HTTP,
smart HTTP, git-daemon, and SSH lanes. The selected change added sixty-five
exact stock-Git rows for `--branches`, `-b`, `--quiet`, `-q`, `--get-url`,
`--symref`, `--exit-code` on both matching and missing-pattern lanes,
`--server-option=foo`, `-o foo`, `--sort=refname`, `--sort=-refname`, and
`-t` across those non-local transport surfaces. The runtime closure stayed
intentionally narrow: `ls-remote` now prints symbolic HEAD output for
git-daemon and SSH, abandons the SSH advertisement session on empty filtered
results so `--exit-code` no-match parity returns stock exit `2`, and rejects
SSH `--server-option` / `-o` without protocol v2 using the same stock fatal
hint plus remote-hung-up diagnostic sequence. Actual delta is `+65` matrix
rows, `+0` complete documented option pairs, `+0` represented documented
option pairs, `+58` verified rows, `+7` invalid-input rows, and `+0`
complete command matrices. Current census counts are `6851` matrix rows,
`6006` verified rows, `820` invalid-input rows, `0` exact-open rows,
`146/151` complete command matrices, `2523/3212` complete documented option
pairs, and `2548/3212` represented documented option pairs. `ls-remote` now
sits at `14/14` represented documented option pairs with `115/115`
classified rows, but only `3/14` reviewed-complete documented option pairs.
This remains an expansion batch rather than a reviewed-complete closure; the
transport-wide option family is now modeled, but more row-level evidence is
still required before the census can promote those `ls-remote` pairs into the
durable reviewed-complete list.

As of 2026-06-27 the latest completed batch is a helper-free local `ls-remote`
option-family expansion across the configured-local bare-remote lane. The
selected change added thirteen exact stock-Git rows for `--branches`, `-b`,
`--quiet`, `-q`, `--get-url`, `--symref`, `--exit-code` on both matching and
missing-pattern lanes, `--server-option=foo`, `-o foo`, `--sort=refname`,
`--sort=-refname`, and `-t`. The runtime closure stayed intentionally narrow:
the `ls-remote` schema now accepts the missing spellings, the shared transport
entry path threads `get-url`, `symref`, `refname` sorting, and pattern-gated
`exit-code`, and the helper-free local lane treats quiet plus server-option as
the same stock-compatible no-op observed in Git. Actual delta is `+13` matrix
rows, `+0` complete documented option pairs, `+11` represented documented
option pairs, `+12` verified rows, `+1` invalid-input row, and `+0` complete
command matrices. Current census counts are `6786` matrix rows, `5948`
verified rows, `813` invalid-input rows, `0` exact-open rows,
`146/151` complete command matrices, `2523/3212` complete documented option
pairs, and `2548/3212` represented documented option pairs. `ls-remote` now
sits at `14/14` represented documented option pairs with `50/50` classified
rows, but only `3/14` reviewed-complete documented option pairs. This remains
an expansion batch rather than a reviewed-complete closure; the next default
follow-up should widen the same option family across the already modeled HTTP,
SSH, git-daemon, and file/local non-helper lanes so those documented
`ls-remote` pairs can move from represented to reviewed-complete.

As of 2026-06-27 the latest completed batch is a helper-free local `diff*`
dirstat value-form expansion across the existing treeish-pair, tree-to-tree,
and dirty-worktree nested-directory lanes. The selected change added twenty-four
exact stock-Git rows for `-X10`, `--dirstat=files`, `--dirstat=files,10`,
`--dirstat=cumulative`, `--dirstat-by-file=10,cumulative`, and standalone
`--cumulative` across `diff`, `diff-files`, `diff-index`, and `diff-tree`.
The runtime closure stayed intentionally narrow: the CLI now pre-normalizes
short dirstat argv spellings before clap parsing, so `-X10` no longer misparses
by consuming the following revision operand as a value. Actual delta is `+24`
matrix rows, `+0` complete documented option pairs, `+0` represented documented
option pairs, `+24` verified rows, `+0` invalid-input rows, and `+0` complete
command matrices. Current census counts are `6773` matrix rows, `5936`
verified rows, `812` invalid-input rows, `0` exact-open rows,
`146/151` complete command matrices, `2523/3212` complete documented option
pairs, and `2537/3212` represented documented option pairs. This remained an
expansion-only batch inside the already represented `diff*` dirstat surface.

As of 2026-06-27 the latest completed batch is a helper-free local
`diff-tree` dirstat tail expansion across the existing tree-to-tree nested
directory lane. The selected change added four exact stock-Git rows for
`diff-tree --dirstat`, `diff-tree --dirstat-by-file`, `diff-tree -X`, and
`diff-tree --dirstat --cumulative`, and closes the prior unverified dirstat
surface on `diff-tree` by forcing that family onto the recursive-entry path
instead of the old non-recursive root-entry renderer. Actual delta is `+4`
matrix rows, `+0` complete documented option pairs, `+4` represented
documented option pairs, `+4` verified rows, `+0` invalid-input rows, and
`+0` complete command matrices. Current census counts are `6749` matrix rows,
`5912` verified rows, `812` invalid-input rows, `0` exact-open rows,
`146/151` complete command matrices, `2523/3212` complete documented option
pairs, and `2537/3212` represented documented option pairs. `diff-tree` now
sits at `101/132` represented documented option pairs with `134/134`
stock-matching rows and `0` invalid-input rows. This remains an expansion
batch rather than a reviewed-complete closure; the next default follow-up
should still promote the remaining represented `diff*` dirstat-family pairs
into reviewed-complete status before leaving the diff family.

As of 2026-06-27 the latest completed batch is a helper-free local `pull`
fetch-inherited tail closure across the named-local ff-only lane plus the
existing-`FETCH_HEAD` append rejection lane. The selected change added nine
exact stock-Git rows for `pull --append`, `--dry-run`, `--force`,
`--negotiation-tip=HEAD`,
`--refmap=+refs/heads/*:refs/remotes/origin/*`, `--set-upstream`, `-a`,
`-f`, and `-r`, and closes the final documented `pull` command-option tail.
Actual delta is `+9` matrix rows, `+9` complete documented option pairs, `+9`
represented documented option pairs, `+7` verified rows, `+2` invalid-input
rows, and `+0` complete command matrices. Current census counts are `6735`
matrix rows, `5898` verified rows, `812` invalid-input rows, `0` exact-open
rows, `146/151` complete command matrices, `2523/3212` complete documented
option pairs, and `2523/3212` represented documented option pairs. `pull` is
now fully closed at `99/99` reviewed-complete documented option pairs with
`101/127` stock-matching rows and `26` invalid-input rows. The next default
follow-up should move off `pull` and reselect from the larger backlog head,
which is still led by `replay`, `send-email`, `rebase`, `log`, `rev-list`,
and the `diff*` family.

As of 2026-06-27 the latest completed batch is a helper-free local
`diff`/`diff-files`/`diff-index` dirstat-family expansion across the
treeish-pair porcelain lane and the dirty-worktree plumbing lanes. The
selected change added ten exact stock-Git rows for `diff -X`,
`diff --dirstat --cumulative`, `diff-files --dirstat`,
`diff-files --dirstat-by-file`, `diff-files -X`,
`diff-files --dirstat --cumulative`, `diff-index --dirstat`,
`diff-index --dirstat-by-file`, `diff-index -X`, and
`diff-index --dirstat --cumulative`, plus the shared dirstat weighting and
ordering fixes needed to match stock Git on these lanes and a larger CLI
thread stack to keep the expanded clap schema parseable. Actual delta is `+10`
matrix rows, `+0` complete documented option pairs, `+10` represented
documented option pairs, `+10` verified rows, `+0` invalid-input rows, and
`+0` complete command matrices. Current census counts are `6745` matrix rows,
`5908` verified rows, `812` invalid-input rows, `0` exact-open rows,
`146/151` complete command matrices, `2523/3212` complete documented option
pairs, and `2533/3212` represented documented option pairs. The touched
commands now sit at `diff 94/117` represented documented option pairs with
`258/262` stock-matching rows, `diff-files 95/118` represented documented
option pairs with `115/115` stock-matching rows, and `diff-index 94/112`
represented documented option pairs with `122/122` stock-matching rows. This
was intentionally an expansion batch rather than a reviewed-complete closure;
the next default follow-up should either finish the remaining dirstat-family
expansion needed to promote these pairs into reviewed-complete status or close
the still-open `diff-tree` dirstat tail before leaving the diff family.

As of 2026-06-27 the latest completed batch is a helper-free local `pull`
invalid fetch-inherited closure across the named-local ff-only lane. The
selected change added twenty-one exact stock-Git invalid-input rows for
`pull --atomic`, `--auto-gc`, `--auto-maintenance`, `--multiple`,
`--negotiate-only`, `--no-auto-gc`, `--no-auto-maintenance`,
`--no-write-commit-graph`, `--no-write-fetch-head`, `--porcelain`,
`--prefetch`, `--prune-tags`, `--recurse-submodules-default=yes`,
`--refetch`, `--submodule-prefix=foo/`, `--update-head-ok`,
`--write-commit-graph`, `--write-fetch-head`, `-P`, `-e`, and `-u`. Actual
delta is `+21` matrix rows, `+21` complete documented option pairs, `+21`
represented documented option pairs, `+0` verified rows, `+21`
invalid-input rows, and `+0` complete command matrices. Current census counts
are `6726` matrix rows, `5891` verified rows, `810` invalid-input rows, `0`
exact-open rows, `146/151` complete command matrices, `2514/3212` complete
documented option pairs, and `2514/3212` represented documented option pairs.
`pull` now sits at `90/99` reviewed-complete documented option pairs with
`94/118` stock-matching rows and `24` invalid-input rows. The next default
follow-up should reselect from the refreshed larger backlog head rather than
stay on `pull`, because the remaining `pull` tail is now only nine documented
options and the denser unresolved queues are elsewhere.

As of 2026-06-27 the latest completed batch is a helper-free local `pull`
signature-verification closure across the explicit local no-rebase merge lane.
The selected change added three exact stock-Git rows for
`pull --verify-signatures` on both the signed-remote-tip success lane and the
unsigned-target rejection lane, plus `pull --no-verify-signatures` on the
signed bypass lane. Actual delta is `+3` matrix rows, `+2` complete
documented option pairs, `+2` represented documented option pairs, `+2`
verified rows, `+1` invalid-input row, and `+0` complete command matrices.
Current census counts are `6705` matrix rows, `5891` verified rows, `789`
invalid-input rows, `0` exact-open rows, `146/151` complete command matrices,
`2493/3212` complete documented option pairs, and `2493/3212` represented
documented option pairs. `pull` now sits at `69/99` reviewed-complete
documented option pairs with `94/97` stock-matching rows and `3`
invalid-input rows. The next default follow-up should reselect from the
refreshed backlog head rather than linger on pull micro-tails, because this
batch closed the cohesive signature-verification family without opening a new
shared runtime lane.

As of 2026-06-27 the latest completed batch is a helper-free local `merge`
signature-verification closure across the shared non-fast-forward clean merge
lane. The selected change added three exact stock-Git rows for
`merge --verify-signatures` on both the signed-feature success lane and the
unsigned-feature rejection lane, plus `merge --no-verify-signatures` on the
signed-feature bypass lane, and closed the final documented `merge`
command-option tail. Actual delta is `+3` matrix rows, `+2` complete
documented option pairs, `+2` represented documented option pairs, `+2`
verified rows, `+1` invalid-input row, and `+0` complete command matrices.
Current census counts are `6702` matrix rows, `5889` verified rows, `788`
invalid-input rows, `0` exact-open rows, `146/151` complete command matrices,
`2491/3212` complete documented option pairs, and `2491/3212` represented
documented option pairs. `merge` now sits at `51/51` reviewed-complete
documented option pairs with `60/69` stock-matching rows and `9`
invalid-input rows. The command is fully closed on the documented
Git-2.47.1 denominator; the next default follow-up should move back to the
refreshed backlog head outside `merge`.

As of 2026-06-27 the latest completed batch is a helper-free local `merge`
ff plus message-family and quit closure across the modeled fast-forward branch
lane, the shared non-fast-forward clean merge lane, and the conflicted
content-merge state cleanup lane. The selected change added seven exact
stock-Git rows for `merge --ff`, `merge -m`, `merge --file`, `merge -F`,
`merge --into-name`, and `merge --quit` on both the no-merge and conflicted
state lanes, plus the parser/runtime closure needed to accept these documented
spellings without widening unrelated merge behavior. Actual delta is `+7`
matrix rows, `+6` complete documented option pairs, `+6` represented
documented option pairs, `+7` verified rows, `+0` invalid-input rows, and `+0`
complete command matrices. Current census counts are `6699` matrix rows,
`5887` verified rows, `787` invalid-input rows, `0` exact-open rows,
`146/151` complete command matrices, `2489/3212` complete documented option
pairs, and `2489/3212` represented documented option pairs. `merge` now sits
at `49/51` reviewed-complete documented option pairs with `58/66`
stock-matching rows. The only remaining documented `merge` queue is the
signature-verification tail: `--verify-signatures` and
`--no-verify-signatures`.

As of 2026-06-27 the latest completed batch is a helper-free local
`pull`/`merge` acceptance-family closure across the explicit local no-rebase
merge lane and the shared merge default-commit lane. The selected change added
twelve exact stock-Git rows for `merge --verbose`, `merge -v`,
`merge --rerere-autoupdate`, `merge --no-rerere-autoupdate`,
`merge --autostash`, `merge --no-autostash`, `merge --cleanup=strip`,
`merge --overwrite-ignore`, `merge --no-overwrite-ignore`, `pull --autostash`,
`pull --no-autostash`, and `pull --cleanup=strip`, plus the schema and merge
dispatch closure needed to accept these documented helper-free spellings on the
already modeled clean merge lanes. Actual delta is `+12` matrix rows, `+12`
complete documented option pairs, `+12` represented documented option pairs,
`+12` verified rows, `+0` invalid-input rows, and `+0` complete command
matrices. Current census counts are `6692` matrix rows, `5880` verified rows,
`787` invalid-input rows, `0` exact-open rows, `146/151` complete command
matrices, `2483/3212` complete documented option pairs, and
`2483/3212` represented documented option pairs. The touched commands now sit
at `pull 67/99` reviewed-complete documented option pairs with `92/94`
stock-matching rows and `merge 43/51` reviewed-complete documented option
pairs with `51/59` stock-matching rows. The next default follow-up should stay
on the refreshed shared `pull`/`merge` backlog rather than drop into a one-row
tail, because this batch closes another dense acceptance family and leaves a
much smaller documented tail behind it.

As of 2026-06-27 the latest completed batch is a helper-free local
`pull`/`merge` gpg-sign closure across the explicit local no-rebase merge
lane and the shared merge default-commit lane. The selected change added six
exact stock-Git rows for `merge --gpg-sign`, `merge --no-gpg-sign`,
`merge -S`, `pull --gpg-sign`, `pull --no-gpg-sign`, and `pull -S`, plus the
shared parser/runtime closure needed to thread merge-commit signing through
porcelain merge dispatch and explicit local pull merges with stock-like
last-one-wins raw-argv resolution. Actual delta is `+6` matrix rows, `+6`
complete documented option pairs, `+6` represented documented option pairs,
`+6` verified rows, `+0` invalid-input rows, and `+0` complete command
matrices. Current census counts are `6680` matrix rows, `5868` verified rows,
`787` invalid-input rows, `0` exact-open rows, `146/151` complete command
matrices, `2471/3212` complete documented option pairs, and
`2471/3212` represented documented option pairs. The touched commands now sit
at `pull 64/99` reviewed-complete documented option pairs with `89/91`
stock-matching rows and `merge 34/51` reviewed-complete documented option
pairs with `42/50` stock-matching rows. The next default follow-up should
stay on the refreshed shared `pull`/`merge` backlog rather than drop into a
one-row tail, because this batch closes another dense shared merge-engine
family and leaves only broader documented-option expansion work behind it.

As of 2026-06-27 the latest completed batch is a helper-free local
`pull`/`merge` commit-flag closure across the explicit local no-rebase merge
lane and the shared merge default-commit lane. The selected change added
twenty-one exact stock-Git rows for merge and explicit-local pull
`edit`/`no-edit`, `signoff`/`no-signoff`, `verify`/`no-verify`,
`quiet`, `progress`/`no-progress`, plus merge `-e` / `-q` and pull `-q`.
It also closed the shared parser/runtime gap needed to route explicit pull
merge-behavior flags into the merge engine, append stock signoff trailers on
merge commits, and suppress success-path fetch or merge output on quiet lanes.
Actual delta is `+21` matrix rows, `+21` complete documented option pairs,
`+21` represented documented option pairs, `+21` verified rows, `+0`
invalid-input rows, and `+0` complete command matrices. Current census counts
are `6674` matrix rows, `5862` verified rows, `787` invalid-input rows, `0`
exact-open rows, `146/151` complete command matrices, `2465/3212` complete
documented option pairs, and `2465/3212` represented documented option pairs.
The touched commands now sit at `pull 61/99` reviewed-complete documented
option pairs with `86/88` stock-matching rows and `merge 31/51`
reviewed-complete documented option pairs with `39/47` stock-matching rows.
The next default follow-up should stay on the refreshed shared
`pull`/`merge` backlog rather than drop into a one-row tail, because the
merge engine lane is still active and the remaining `pull`/`merge` queue is
still meaningful documented-option expansion work.

As of 2026-06-27 the latest completed batch is a helper-free local
`pull`/`merge` stat-family closure across the explicit local no-rebase merge
lane and the shared merge default-commit lane. The selected change added ten
exact stock-Git rows for `pull --stat`, `pull --no-stat`, `pull --summary`,
`pull --no-summary`, `pull -n`, `merge --stat`, `merge --no-stat`,
`merge --summary`, `merge --no-summary`, and `merge -n`, plus the
parser/runtime closure needed to thread the diffstat toggle family through
`pull` merge dispatch and shared merge execution with stock-like last-one-wins
raw-argv resolution. The pull path also now routes explicit stat-family merge
flags through the merge engine instead of aborting in the fast-forward helper
path on non-fast-forward local pulls. Actual delta is `+10` matrix rows, `+10`
complete documented option pairs, `+10` represented documented option pairs,
`+10` verified rows, `+0` invalid-input rows, and `+0` complete command
matrices. Current census counts are `6639` matrix rows, `5827` verified rows,
`787` invalid-input rows, `0` exact-open rows, `146/151` complete command
matrices, `2434/3212` complete documented option pairs, and `2434/3212`
represented documented option pairs. The touched commands now sit at
`pull 46/99` reviewed-complete documented option pairs with `69/71`
stock-matching rows and `merge 15/51` reviewed-complete documented option
pairs with `21/29` stock-matching rows. The next default follow-up should
reselect from the refreshed census head rather than keep widening this
micro-family, because the shared stat parser tail is now closed and the
remaining `pull` and `merge` queues are broader documented-option expansion
work.

As of 2026-06-27 the latest completed batch is a helper-free local
`pull`/`merge` commit-mode closure across the explicit local no-rebase merge
lane and the shared merge default-commit lane. The selected change added six
exact stock-Git rows for `pull --commit`, `pull --no-commit`,
`pull --squash`, `pull --no-squash`, `merge --commit`, and
`merge --no-squash`, plus the parser/runtime closure needed to thread
`--commit` / `--no-commit` and `--squash` / `--no-squash` through `pull`
merge dispatch with stock-like last-one-wins raw-argv resolution. The pull
path also now routes explicit merge-mode flags through the merge engine
instead of aborting in the fast-forward-only helper path on non-fast-forward
local pulls. Actual delta is `+6` matrix rows, `+6` complete documented
option pairs, `+6` represented documented option pairs, `+6` verified rows,
`+0` invalid-input rows, and `+0` complete command matrices. Current census
counts are `6629` matrix rows, `5817` verified rows, `787` invalid-input
rows, `0` exact-open rows, `146/151` complete command matrices,
`2424/3212` complete documented option pairs, and `2424/3212` represented
documented option pairs. The touched commands now sit at `pull 41/99`
reviewed-complete documented option pairs with `64/66` stock-matching rows
and `merge 10/51` reviewed-complete documented option pairs with `16/24`
stock-matching rows. The next default follow-up should reselect from the
refreshed census head rather than keep widening this micro-family, because
the shared commit/squash parser tail is now closed and the remaining `pull`
and `merge` queues are broader documented-option expansion work.

As of 2026-06-27 the latest completed batch is a helper-free local `diff`
family color-word-diff and regex closure across the modeled local patch lanes.
The selected change added twelve exact stock-Git rows for `diff`,
`diff-files`, `diff-index`, and `diff-tree` covering bare `--color-words`,
explicit `--color-words=[a-z]+`, and
`--word-diff-regex=[a-z]+ --word-diff=plain`, plus the shared parser/runtime
closure needed to thread `color_words` and `word_diff_regex` through both
porcelain and plumbing render paths without widening unrelated diff behavior.
Actual delta is `+12` matrix rows, `+8` complete documented option pairs, `+8`
represented documented option pairs, `+12` verified rows, `+0` invalid-input
rows, and `+0` complete command matrices. Current census counts are `6623`
matrix rows, `5811` verified rows, `787` invalid-input rows, `0` exact-open
rows, `146/151` complete command matrices, `2418/3212` complete documented
option pairs, and `2418/3212` represented documented option pairs. The
touched commands now sit at `92/117` reviewed-complete option pairs for
`diff`, `91/118` for `diff-files`, `90/112` for `diff-index`, and `97/132`
for `diff-tree`, with zero exact-open rows across all four modeled surfaces.
The next default follow-up should move off this runtime tail and reselect from
the refreshed backlog head, because the remaining `diff*` work is now mostly
unrepresented documented-option expansion rather than another dense shared
behavior lane.

As of 2026-06-27 the latest completed batch is a helper-free shared `diff*`
parser-and-evidence closure for `--output` and plumbing `--line-prefix`. The
selected change added seven exact stock-Git rows for `diff --output`,
`diff-files --output`, `diff-index --output`, `diff-tree --output`,
`diff-files --line-prefix`, `diff-index --line-prefix`, and
`diff-tree --line-prefix`, plus the parser/runtime closure needed to route
plumbing `line-prefix` through shared render options and to implement
`diff* --output` as an early self-reexec stdout redirect without touching the
render layer. Actual delta is `+7` matrix rows, `+0` complete documented
option pairs, `+7` represented documented option pairs, `+7` verified rows,
`+0` invalid-input rows, and `+0` complete command matrices. Current census
counts are `6299` matrix rows, `5487` verified rows, `787` invalid-input
rows, `0` exact-open rows, `146/151` complete command matrices,
`2103/3212` complete documented option pairs, and `2164/3212` represented
documented option pairs. The touched commands now sit at `84/117` reviewed
complete option pairs for `diff`, `81/118` for `diff-files`, `80/112` for
`diff-index`, and `87/132` for `diff-tree`, with zero exact-open rows across
all four modeled surfaces. The next default follow-up should reselect from
the refreshed backlog head rather than keep widening `diff*`, because this
batch closed the main shared parser tail but left only expansion-only
documented-option heads on these commands.

As of 2026-06-26 the latest completed batch closes the final exact-open tail:
top-level `scalar` no-subcommand plus the modeled `git svn clone` and
`git svn dcommit` rows. The selected change added `+1` focused scalar
stock-oracle test, corrected the stale scalar probe to use standalone stock
`scalar`, added `+0` matrix rows, and reclassified the three remaining exact
rows from `open` to `closed`. Actual delta is `+0` matrix rows, `+0` complete
documented option pairs, `+0` represented documented option pairs, `+2`
verified rows, `+0` invalid-input rows, and `+0` complete command matrices.
Current census counts are `5984` matrix rows, `5221` verified rows, `738`
invalid-input rows, `0` exact-open rows, `146/151` complete command matrices,
`1927/3212` complete documented option pairs, and `1927/3212` represented
documented option pairs. `scalar` now sits at `19/19` stock-matching written
rows and `svn` at `3/3` stock-matching written rows on their modeled
surfaces. The next default follow-up should move off oracle unblock work and
return to the largest helper-free documented-option/schema family, with `git am`
currently at the head of `remaining_to_fix_or_verify.tsv`.

As of 2026-06-26 the latest completed batch is a helper-free `git update-index`
`--split-index` documented-option tail closure on the modeled single-entry
local lane. The selected change added one exact stock-Git row for
`update-index --split-index`, then promoted that represented documented option
into `docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Actual delta is
`+1` matrix row, `+1` complete documented option pair, `+1` represented
documented option pair, `+1` verified row, `+0` invalid-input rows, and `+0`
complete command matrices. Current census counts are `5984` matrix rows,
`5210` verified rows, `738` invalid-input rows, `12` exact-open rows,
`146/151` complete command matrices, `1919/3212` complete documented option
pairs, and `1927/3212` represented documented option pairs. `update-index`
now sits at `38/38` reviewed-complete documented option pairs with `69`
written rows, `69` classified rows, `61` stock-matching rows, `8`
invalid-input rows, and `0` exact-open rows. The next default follow-up
should move off `update-index` and return to the next dense represented
helper-free family outside it, with shared history-query promotion work
remaining a strong default queue.

As of 2026-06-26 the latest completed batch is a helper-free `git update-index`
helper-extension documented-option expansion on the current extensionless local
lane. The selected change added five exact stock-Git rows for
`update-index --force-untracked-cache`, `update-index --fsmonitor`,
`update-index --fsmonitor-valid a.txt`,
`update-index --no-fsmonitor-valid a.txt`, and
`update-index --untracked-cache`, then promoted those five represented
documented options into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Actual delta is `+5`
matrix rows, `+5` complete documented option pairs, `+5` represented
documented option pairs, `+5` verified rows, `+0` invalid-input rows, and `+0`
complete command matrices. Current census counts are `5983` matrix rows,
`5209` verified rows, `738` invalid-input rows, `12` exact-open rows,
`146/151` complete command matrices, `1918/3212` complete documented option
pairs, and `1926/3212` represented documented option pairs. `update-index`
now sits at `37/38` reviewed-complete documented option pairs with `68`
written rows, `68` classified rows, `60` stock-matching rows, `8`
invalid-input rows, and `0` exact-open rows. The next default follow-up
should isolate the final `update-index --split-index` tail, because the stock
lane writes the required lowercase `link` extension and `sharedindex.*`
artifacts that still need real support or an explicit defer decision.

As of 2026-06-26 the latest completed batch is a helper-free `git update-index`
disable-helper-toggle documented-option expansion on the current local lane.
The selected change added four exact stock-Git rows for
`update-index --no-split-index`, `update-index --no-untracked-cache`,
`update-index --test-untracked-cache`, and `update-index --no-fsmonitor`, then
promoted those four represented documented options into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Actual delta is `+4`
matrix rows, `+4` complete documented option pairs, `+4` represented
documented option pairs, `+4` verified rows, `+0` invalid-input rows, and `+0`
complete command matrices. Current census counts are `5978` matrix rows,
`5204` verified rows, `738` invalid-input rows, `12` exact-open rows,
`146/151` complete command matrices, `1913/3212` complete documented option
pairs, and `1921/3212` represented documented option pairs. `update-index`
now sits at `32/38` reviewed-complete documented option pairs with `63`
written rows, `63` classified rows, `55` stock-matching rows, `8`
invalid-input rows, and `0` exact-open rows. The next default follow-up
should stay on the last six `update-index` documented tails as one batched
helper-like closure attempt: `--force-untracked-cache`, `--fsmonitor`,
`--fsmonitor-valid`, `--no-fsmonitor-valid`, `--split-index`, and
`--untracked-cache`, closing the densest safe subset first and deferring only
the parts that truly require shared-index or index-extension modeling.

As of 2026-06-26 the latest completed batch is a helper-free `git update-index`
skip-worktree/remove plus submodule-refresh documented-option expansion on the
current local lane. The selected change added three exact stock-Git rows for
`update-index --refresh --ignore-submodules submod`,
`update-index --remove --ignore-skip-worktree-entries a.txt`, and
`update-index --remove --no-ignore-skip-worktree-entries a.txt`, then promoted
three newly represented documented options into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Actual delta is `+3`
matrix rows, `+3` complete documented option pairs, `+3` represented
documented option pairs, `+3` verified rows, `+0` invalid-input rows, and `+0`
complete command matrices. Current census counts are `5974` matrix rows,
`5200` verified rows, `738` invalid-input rows, `12` exact-open rows,
`146/151` complete command matrices, `1909/3212` complete documented option
pairs, and `1917/3212` represented documented option pairs. `update-index`
now sits at `28/38` reviewed-complete documented option pairs with `59`
written rows, `59` classified rows, `51` stock-matching rows, `8`
invalid-input rows, and `0` exact-open rows. The next default follow-up
should stay on the remaining safe helper-free `update-index` local tail,
which now mostly narrows to the more helper-like fsmonitor, split-index, and
untracked-cache families unless a new represented local lane is promoted.

As of 2026-06-26 the latest completed batch is a helper-free `git update-index`
refresh-family documented-option expansion on the current local lane. The
selected change added five exact stock-Git rows for dirty tracked
`update-index --refresh`, `update-index --refresh -q`,
`update-index --refresh --ignore-missing`,
`update-index --refresh --unmerged`, and short-alias `update-index -g`, then
promoted four newly represented documented options into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Actual delta is `+5`
matrix rows, `+4` complete documented option pairs, `+4` represented
documented option pairs, `+5` verified rows, `+0` invalid-input rows, and `+0`
complete command matrices. Current census counts are `5963` matrix rows,
`5190` verified rows, `737` invalid-input rows, `12` exact-open rows,
`146/151` complete command matrices, `1901/3212` complete documented option
pairs, and `1909/3212` represented documented option pairs. `update-index`
now sits at `20/38` reviewed-complete documented option pairs with `48`
written rows, `48` classified rows, `41` stock-matching rows, `7`
invalid-input rows, and `0` exact-open rows. The next default follow-up
should stay on the remaining safe helper-free `update-index` local tail before
switching back to helper-blocked foreign-SCM commands.

As of 2026-06-26 the latest completed batch is a helper-free `git worktree`
remote-inference plus unborn-branch documented-option tail closure on the
current local lane. The selected change added five exact stock-Git rows for
`worktree add --orphan`, `worktree add --track`, `worktree add --no-track`,
`worktree add --guess-remote`, and `worktree add --no-guess-remote`, then
promoted those five represented documented options into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Actual delta is
`+5` matrix rows, `+5` complete documented option pairs, `+5`
represented documented option pairs, `+5` verified rows, `+0`
invalid-input rows, and `+0` complete command matrices. Current census counts
are `5958` matrix rows, `5185` verified rows, `737` invalid-input rows, `12`
exact-open rows, `146/151` complete command matrices, `1897/3212` complete
documented option pairs, and `1905/3212` represented documented option
pairs. `worktree` now sits at `24/24` reviewed-complete documented option
pairs with `33` written rows, `33` classified rows, `32` stock-matching
rows, `1` invalid-input row, and `0` exact-open rows. The next default
follow-up should move to the next dense helper-free local family outside
worktree, with `update-index` currently the best evidence-backed target.

As of 2026-06-26 the latest completed batch is a helper-free `git worktree`
short-alias plus list-format documented-option family expansion on the
current local lane. The selected change added six exact stock-Git rows for
`worktree add -d`, `worktree add -q`, `worktree add --force`,
`worktree list -v`, `worktree list --porcelain -z`, and
`worktree prune -n`, then promoted those six represented documented options
into `docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Actual delta
is `+6` matrix rows, `+6` complete documented option pairs, `+6`
represented documented option pairs, `+6` verified rows, `+0`
invalid-input rows, and `+0` complete command matrices. Current census counts
are `5953` matrix rows, `5180` verified rows, `737` invalid-input rows, `12`
exact-open rows, `146/151` complete command matrices, `1892/3212` complete
documented option pairs, and `1900/3212` represented documented option
pairs. `worktree` now sits at `19/24` reviewed-complete documented option
pairs with `28` written rows, `28` classified rows, `27` stock-matching
rows, `1` invalid-input row, and `0` exact-open rows. The next default
follow-up should stay on the remaining helper-free `worktree` option tail
instead of dropping back to helper-blocked commands.

As of 2026-06-26 the latest completed batch is a helper-free `git worktree`
add/list documented-option family expansion on the current local lane. The
selected change added five exact stock-Git rows for `worktree add
--checkout`, `worktree add --no-checkout`, `worktree add --quiet`,
`worktree add --lock --reason why`, and `worktree list --porcelain`, then
promoted those five represented documented options into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Actual delta is
`+5` matrix rows, `+5` complete documented option pairs, `+5` represented
documented option pairs, `+5` verified rows, `+0` invalid-input rows, and
`+0` complete command matrices. Current census counts are `5947` matrix rows,
`5174` verified rows, `737` invalid-input rows, `12` exact-open rows,
`146/151` complete command matrices, `1886/3212` complete documented option
pairs, and `1894/3212` represented documented option pairs. `worktree` now
sits at `13/24` reviewed-complete documented option pairs with `22` written
rows, `22` classified rows, `21` stock-matching rows, `1` invalid-input row,
and `0` exact-open rows. The next default follow-up should stay on another
helper-free local documented-option family instead of dropping back to the
helper-blocked open helper commands.

As of 2026-06-26 the latest completed batch is a zero-code shared
history-query reviewed-complete promotion for `git log` and
`git rev-list`. The selected change promoted fourteen already-represented
documented option pairs into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv` after confirming the
existing exact stock-Git rows already covered the current helper-free local
lanes for `--no-standard-notes`, `--quiet`, `--reflog`, `--relative-date`,
`--show-notes`, `--show-notes-by-default`, and `--standard-notes` across both
commands. Actual delta is `+0` matrix rows, `+14` complete documented option
pairs, `+0` represented documented option pairs, `+0` verified rows, `+0`
invalid-input rows, and `+0` complete command matrices. Current census counts
are `5942` matrix rows, `5169` verified rows, `737` invalid-input rows, `12`
exact-open rows, `146/151` complete command matrices, `1881/3212` complete
documented option pairs, and `1889/3212` represented documented option pairs.
`log` now sits at `78/131` reviewed-complete documented option pairs with
`193` written rows, `193` classified rows, `183` stock-matching rows, `10`
invalid-input rows, and `0` exact-open rows, while `rev-list` now sits at
`75/117` reviewed-complete documented option pairs with `127` written rows,
`127` classified rows, `117` stock-matching rows, `10` invalid-input rows,
and `0` exact-open rows. The next default follow-up should stay census-first
on the remaining represented shared history-query surface instead of dropping
back to isolated row additions.

As of 2026-06-26 the latest completed batch is a zero-code shared history
matrix-shape cleanup for `git log`. The selected change repaired two malformed
`docs/cli/matrices/log_v2_47.tsv` rows where `repo_state` / `combination`
text had been split across an extra tab, so previously verified helper-free
rows became durably classifiable again in the census. Actual delta is `+0`
matrix rows, `+0` complete documented option pairs, `+0` represented
documented option pairs, `+2` verified rows, `+0` invalid-input rows, and
`+0` complete command matrices. Current census counts are `5942` matrix rows,
`5169` verified rows, `737` invalid-input rows, `12` exact-open rows,
`146/151` complete command matrices, `1867/3212` complete documented option
pairs, and `1889/3212` represented documented option pairs. `log` now sits at
`71/131` reviewed-complete documented option pairs with `193` written rows,
`193` classified rows, `183` stock-matching rows, `10` invalid-input rows,
and `0` exact-open rows, while `rev-list` remains at `68/117`
reviewed-complete documented option pairs with `127` written rows, `127`
classified rows, `117` stock-matching rows, `10` invalid-input rows, and `0`
exact-open rows. The next default follow-up should return to behavior-closing
shared history-query tails; this batch only made existing evidence durable.

As of 2026-06-26 the latest completed batch is a helper-free shared
`git log`/`git rev-list` date-order plus notes-precedence expansion on the
current local lane. The selected change added seventeen exact stock-Git rows
for order-sensitive `--date=iso` plus `--relative-date` lanes, `--quiet`
plus relative-date formatting, `--reflog` plus `--relative-date` pretty lanes,
and additional `show-notes` / `show-notes-by-default` / `standard-notes` /
`no-standard-notes` custom-format precedence combinations across `log` and
`rev-list`. Actual delta is `+17` matrix rows, `+0` complete documented
option pairs, `+0` represented documented option pairs, `+14` verified rows,
`+2` invalid-input rows, and `+0` complete command matrices. Current census
counts are `5942` matrix rows, `5167` verified rows, `737` invalid-input
rows, `12` exact-open rows, `146/151` complete command matrices,
`1867/3212` complete documented option pairs, and `1889/3212` represented
documented option pairs. `log` now sits at `71/131` reviewed-complete
documented option pairs with `193` written rows, `191` classified rows,
`181` stock-matching rows, `10` invalid-input rows, and `0` exact-open rows,
while `rev-list` now sits at `68/117` reviewed-complete documented option
pairs with `127` written rows, `127` classified rows, `117` stock-matching
rows, `10` invalid-input rows, and `0` exact-open rows on their current
modeled surfaces. The next default follow-up should keep harvesting shared
history-query represented tails instead of dropping back to isolated
one-row closures.

As of 2026-06-26 the latest completed batch is a helper-free shared
`git log`/`git rev-list` reflog-plus-notes precedence expansion on the current
local lane. The selected change added eight exact stock-Git rows for
`log --show-notes-by-default --no-standard-notes`,
`log --standard-notes --show-notes`, `log --quiet --reflog`,
`log --reflog --date=relative --pretty=format:%gd|%ad|%cd`,
`rev-list --quiet --reflog`,
`rev-list --reflog --date=relative --pretty=format:%gd|%ad|%cd`,
`rev-list --standard-notes --show-notes`, and
`rev-list --show-notes-by-default --no-standard-notes`. Actual delta is `+8`
matrix rows, `+0` complete documented option pairs, `+0` represented
documented option pairs, `+6` verified rows, `+2` invalid-input rows, and
`+0` complete command matrices. Current census counts are `5925` matrix rows,
`5153` verified rows, `735` invalid-input rows, `12` exact-open rows,
`146/151` complete command matrices, `1867/3212` complete documented option
pairs, and `1889/3212` represented documented option pairs. `log` now sits at
`71/131` reviewed-complete documented option pairs with `184` written rows,
`183` classified rows, `173` stock-matching rows, `10` invalid-input rows,
and `0` exact-open rows, while `rev-list` now sits at `68/117`
reviewed-complete documented option pairs with `119` written rows, `119`
classified rows, `111` stock-matching rows, `8` invalid-input rows, and `0`
exact-open rows on their current modeled surfaces. The next default follow-up
should keep harvesting shared history-query represented tails instead of
falling back to one-row isolated closures.

As of 2026-06-26 the latest completed batch is a helper-free shared
`git log`/`git rev-list` notes-plus-quiet precedence expansion on the current
local lane. The selected change added seven exact stock-Git rows for
`log --standard-notes`, `log --standard-notes --no-standard-notes`,
`log --show-notes --no-standard-notes`, `log --quiet --format=%H`,
`rev-list --quiet --format=%H`, `rev-list --standard-notes --no-standard-notes`,
and `rev-list --show-notes --no-standard-notes`, covering the current stock
literal `%N` custom-format lanes for `standard-notes` precedence, the stock
blank note-suppressed `log --show-notes --no-standard-notes` lane, the stock
empty-output `rev-list --quiet --format=%H` surface, and the stock
unsupported-notes fatal even when `rev-list --show-notes` is followed by
`--no-standard-notes`. Actual delta is `+7` matrix rows, `+0` complete
documented option pairs, `+1` represented documented option pair, `+6`
verified rows, `+1` invalid-input row, and `+0` complete command matrices.
Current census counts are `5917` matrix rows, `5147` verified rows, `733`
invalid-input rows, `12` exact-open rows, `146/151` complete command
matrices, `1867/3212` complete documented option pairs, and
`1889/3212` represented documented option pairs. `log` now sits at `71/131`
reviewed-complete documented option pairs with `180` written rows, `179`
classified rows, `169` stock-matching rows, `10` invalid-input rows, and `0`
exact-open rows, while `rev-list` now sits at `68/117`
reviewed-complete documented option pairs with `115` written rows, `115`
classified rows, `109` stock-matching rows, `6` invalid-input rows, and `0`
exact-open rows on their current modeled surfaces. The next default
follow-up should keep expanding the newly represented shared history-query
tails rather than switching back to isolated one-row tails.

As of 2026-06-26 the latest completed batch is a helper-free shared
`git rev-list` formatting/text documented-option family expansion on the
current local lane. The selected change added twelve exact stock-Git rows for
`rev-list --no-notes`, `--notes`, `--abbrev-commit`, `--no-abbrev-commit`,
`--oneline`, `--pretty`, `--expand-tabs`, `--no-expand-tabs`, `--encoding`,
`--date`, and `--format`, then promoted eleven represented documented options
into `docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta
was `+12` matrix rows, `+11` complete documented option pairs, `+11`
represented documented option pairs, `+11` verified rows, `+1`
invalid-input row, and `+0` complete command matrices. Actual delta matched.
Current census counts are `5871` matrix rows, `5110` verified rows, `724`
invalid-input rows, `12` exact-open rows, `146/151` complete command
matrices, `1853/3212` complete documented option pairs, and
`1861/3212` represented documented option pairs. `rev-list` now sits at
`61/117` reviewed-complete documented option pairs with `89/89` classified
rows, `88` stock-matching rows, `1` invalid-input row, and `0` exact-open
rows on its current modeled surface. The next default follow-up should keep
harvesting shared history-query represented families before switching back to
isolated tails.

As of 2026-06-26 the latest completed batch is a helper-free shared
`git rev-list` grep/regex documented-option family expansion on the current
local lane. The selected change added eighteen exact stock-Git rows for
`rev-list --grep`, `--all-match`, `--invert-grep`, `--regexp-ignore-case`,
`--basic-regexp`, `--extended-regexp`, `--fixed-strings`,
`--perl-regexp`, and the short aliases `-E`, `-F`, `-P`, and `-i`,
including selector-override ordering lanes, then promoted twelve represented
documented options into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta was
`+18` matrix rows, `+12` complete documented option pairs, `+12`
represented documented option pairs, `+18` verified rows, `+0`
invalid-input rows, and `+0` complete command matrices. Actual delta matched.
Current census counts are `5859` matrix rows, `5099` verified rows, `723`
invalid-input rows, `12` exact-open rows, `146/151` complete command
matrices, `1842/3212` complete documented option pairs, and
`1850/3212` represented documented option pairs. `rev-list` now sits at
`50/117` reviewed-complete documented option pairs with `77/77` classified
rows, `77` stock-matching rows, `0` invalid-input rows, and `0` exact-open
rows on its current modeled surface. The next default follow-up should keep
harvesting shared history-query represented families before switching back to
isolated tails.

As of 2026-06-26 the latest completed batch is a helper-free shared
`git rev-list` identity/time/parent filter documented-option family
expansion on the current local lane. The selected change added twelve exact
stock-Git rows for `rev-list --author`, `--committer`, `--since`, `--after`,
`--until`, `--before`, `--merges`, `--no-merges`, `--max-parents`,
`--no-max-parents`, `--min-parents`, and `--no-min-parents`, then promoted
twelve represented documented options into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta was
`+12` matrix rows, `+12` complete documented option pairs, `+12`
represented documented option pairs, `+12` verified rows, `+0`
invalid-input rows, and `+0` complete command matrices. Actual delta matched.
Current census counts are `5841` matrix rows, `5081` verified rows, `723`
invalid-input rows, `12` exact-open rows, `146/151` complete command
matrices, `1830/3212` complete documented option pairs, and
`1838/3212` represented documented option pairs. `rev-list` now sits at
`38/117` reviewed-complete documented option pairs with `59/59` classified
rows, `59` stock-matching rows, `0` invalid-input rows, and `0` exact-open
rows on its current modeled surface. The next default follow-up should stay
on shared history-query represented families rather than switching back to
single-row tails.

As of 2026-06-26 the latest completed batch is a helper-free shared
`git switch`/`git fetch`/`git pull` transport-and-invalid-input closure on
the current local lane. The selected change added six exact stock-Git rows for
`switch --track`, `switch --no-track`, `switch -t`, `fetch --porcelain`,
`fetch --progress`, and `pull --all`, then promoted six represented
documented options into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta was
`+6` matrix rows, `+6` complete documented option pairs, `+6`
represented documented option pairs, `+2` verified rows, `+3`
invalid-input rows, and `+0` complete command matrices. Actual delta matched.
Current census counts are `5829` matrix rows, `5069` verified rows, `723`
invalid-input rows, `12` exact-open rows, `146/151` complete command
matrices, `1818/3212` complete documented option pairs, and
`1826/3212` represented documented option pairs. `switch` is now fully
reviewed complete at `25/25` documented option pairs with `26/26` classified
rows and `0` exact-open rows, `fetch` is now fully reviewed complete at
`63/63` documented option pairs with `379/379` classified rows and
`0` exact-open rows, and `pull` now sits at `37/99`
reviewed-complete documented option pairs with `62/62` classified rows,
`60` stock-matching rows, `2` invalid-input rows, and `0` exact-open rows on
their current modeled surfaces. The next default follow-up should return to
the next largest census-first represented family instead of another
single-lane transport tail.

As of 2026-06-26 the latest completed batch is a helper-free shared `git log`
and `git show` text-rendering documented-option family expansion on the
current local lane. The selected change added six exact stock-Git rows for
`log --encoding=UTF-8`, `log --expand-tabs`, `log --no-expand-tabs`,
`show --encoding=UTF-8`, `show --expand-tabs`, and
`show --no-expand-tabs`, then promoted six represented documented options into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta was
`+6` matrix rows, `+6` complete documented option pairs, `+6`
represented documented option pairs, `+5` verified rows, `+0`
invalid-input rows, and `+0` complete command matrices. Actual delta matched.
Current census counts are `5791` matrix rows, `5035` verified rows, `720`
invalid-input rows, `12` exact-open rows, `146/151` complete command
matrices, `1784/3212` complete documented option pairs, and
`1792/3212` represented documented option pairs. `log` now sits at
`50/131` reviewed-complete documented option pairs with `144` written rows,
`143` classified rows, `137` stock-matching rows, `6` invalid-input rows,
and `0` exact-open rows, while `show` now sits at `14/15`
reviewed-complete documented option pairs with `49/49` classified rows,
`49` stock-matching rows, `0` invalid-input rows, and `0` exact-open rows on
their current modeled surfaces. The next default follow-up should return to a
denser shared history-query family such as `log` topology/object selection or
`rev-list` object/filter expansion, rather than spending the next iteration on
the one-row `show-signature` tail by itself.

As of 2026-06-26 the latest completed batch is a helper-free `git show`
notes-alias plus abbrev-commit documented-option family expansion on the
current local lane. The selected change added eight exact stock-Git `show`
rows for `--notes`, `--no-notes`, `--show-notes`, `--standard-notes`,
`--show-notes-by-default`, `--no-standard-notes`, `--abbrev-commit`, and
`--oneline --no-abbrev-commit`, then promoted eight represented documented
options into `docs/cli/census/reviewed_complete_doc_option_pairs.tsv`.
Expected delta was `+8` matrix rows, `+8` complete documented option pairs,
`+8` represented documented option pairs, `+8` verified rows, `+0`
invalid-input rows, and `+0` complete command matrices. Actual delta matched.
Current census counts are `5785` matrix rows, `5030` verified rows, `720`
invalid-input rows, `12` exact-open rows, `146/151` complete command
matrices, `1778/3212` complete documented option pairs, and
`1786/3212` represented documented option pairs. `show` now sits at
`11/15` reviewed-complete documented option pairs with `46/46` classified
rows, `46` stock-matching rows, `0` invalid-input rows, and `0`
exact-open rows on the current modeled surface. The next default follow-up
can either finish the small remaining `show` tail around encoding, tab
expansion and signature display, or return to a denser shared history-query
family such as `log` topology/object selection or `rev-list` object/filter
expansion.

As of 2026-06-26 the latest completed batch is a helper-free `git log`
notes plus abbrev-commit documented-option family expansion on the current
local lane. The selected change added four exact stock-Git `log` rows for
`--notes`, `--no-notes`, `--abbrev-commit`, and the order-sensitive
`--oneline --no-abbrev-commit` lane, then promoted four represented
documented options into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta was
`+4` matrix rows, `+4` complete documented option pairs, `+4`
represented documented option pairs, `+4` verified rows, `+0`
invalid-input rows, and `+0` complete command matrices. Actual delta matched.
Current census counts are `5777` matrix rows, `5022` verified rows, `720`
invalid-input rows, `12` exact-open rows, `146/151` complete command
matrices, `1770/3212` complete documented option pairs, and
`1778/3212` represented documented option pairs. `log` now sits at
`47/131` reviewed-complete documented option pairs with `141/141`
classified rows, `135` stock-matching rows, `6` invalid-input rows, and `0`
exact-open rows on the current modeled surface. The next default follow-up
should stay on shared history-query surfaces and prioritize another dense
reusable helper-free family such as `log` topology or object-selection
filters, or a `rev-list` object/filter expansion.

As of 2026-06-26 the latest completed batch is a helper-free `git log`
identity, time, and parent-count documented-option family expansion on the
current local lane. The selected change added twelve exact stock-Git `log`
rows for `--author`, `--committer`, `--after`, `--until`, `--before`,
`--count`, `--merges`, `--no-merges`, `--max-parents`, `--no-max-parents`,
`--min-parents`, and `--no-min-parents`, then promoted twelve represented
documented options into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta was
`+12` matrix rows, `+12` complete documented option pairs, `+12`
represented documented option pairs, `+12` verified rows, `+0`
invalid-input rows, and `+0` complete command matrices. Actual delta matched.
Current census counts are `5773` matrix rows, `5018` verified rows, `720`
invalid-input rows, `12` exact-open rows, `146/151` complete command
matrices, `1766/3212` complete documented option pairs, and
`1774/3212` represented documented option pairs. `log` now sits at
`43/131` reviewed-complete documented option pairs with `137/137`
classified rows, `131` stock-matching rows, `6` invalid-input rows, and `0`
exact-open rows on the current modeled surface. The next default follow-up
should stay on shared history-query surfaces and prioritize another dense
reusable helper-free family rather than switching back to already saturated
promotion lanes.

As of 2026-06-26 the latest completed batch is a helper-free `git log`
grep plus reflog-grep documented-option family expansion on the current local
lane. The selected change added fourteen exact stock-Git `log` rows for
`--grep`, `--all-match`, `--invert-grep`, `--regexp-ignore-case`,
`--basic-regexp`, `--extended-regexp`, `--fixed-strings`,
`--perl-regexp`, `--grep-reflog`, and the short aliases `-E`, `-F`, `-P`,
and `-i`, then promoted thirteen represented documented options into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta was
`+14` matrix rows, `+13` complete documented option pairs, `+13`
represented documented option pairs, `+13` verified rows, `+1`
invalid-input row, and `+0` complete command matrices. Actual delta matched.
Current census counts are `5761` matrix rows, `5006` verified rows, `720`
invalid-input rows, `12` exact-open rows, `146/151` complete command
matrices, `1754/3212` complete documented option pairs, and
`1762/3212` represented documented option pairs. `log` now sits at
`31/131` reviewed-complete documented option pairs with `125/125`
classified rows, `119` stock-matching rows, `6` invalid-input rows, and `0`
exact-open rows on the current modeled surface. The next default follow-up
should stay on shared history-query surfaces and prioritize another dense
helper-free reusable family rather than returning to already saturated local
promotion lanes.

As of 2026-06-26 the latest completed batch is a zero-code reviewed-complete
`git clone` plus `git init` documented-option promotion cluster on the current
modeled surface. The selected change promotes the currently represented
documented `clone` options `--bare`, `--branch`, `--config`, `--depth`,
`--dissociate`, `--jobs`, `--local`, `--mirror`, `--no-checkout`,
`--no-hardlinks`, `--no-reject-shallow`, `--no-single-branch`, `--no-tags`,
`--origin`, `--progress`, `--quiet`, `--recurse-submodules`, `--ref-format`,
`--reference`, `--reject-shallow`, `--remote-submodules`,
`--separate-git-dir`, `--shallow-submodules`, `--shared`,
`--single-branch`, `--template`, `--verbose`, `-b`, `-c`, `-j`, `-l`, `-n`,
`-o`, `-q`, `-s`, and `-v`, plus the currently represented `init` options
`--bare`, `--initial-branch`, `--object-format`, `--quiet`,
`--ref-format`, `--separate-git-dir`, `--template`, `-b`, and `-q`, into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv` because both
commands already had `100%` classified rows and `0` exact-open rows on their
current modeled surfaces after the doc-option extractor repair restored the
correct seed. Expected delta was `+45` complete documented option pairs,
`+0` represented documented option pairs, `+0` matrix rows, `+0` verified
rows, `+0` invalid-input rows, and `+0` complete command matrices. Actual
delta matched. Current census counts are `5747` matrix rows, `4993`
verified rows, `719` invalid-input rows, `12` exact-open rows,
`146/151` complete command matrices, `1741/3212` complete documented option
pairs, and `1749/3212` represented documented option pairs. `clone` now sits
at `36/47` reviewed-complete documented option pairs with `96/96` classified
rows and `0` exact-open rows, while `init` is now fully reviewed complete at
`9/9` documented option pairs with `12/12` classified rows and `0` exact-open
rows. The next default follow-up should keep using the corrected denominator
and prioritize another dense helper-free represented family or promotion
cluster, rather than returning to stale pre-fix queue ordering.

As of 2026-06-26 the latest completed batch is a helper-free Git-doc option
inventory extractor repair for markup-wrapped option definitions on the
current local lane. The selected change broadens
`tools/git-compat-option-inventory.sh` so AsciiDoc option lines wrapped in
backticks or `++` markup are seeded instead of being skipped, which restores
documented option visibility for `clone` and other commands using the same
formatting style without changing any behavior matrix rows. Expected delta was
`+56` documented option seed rows, `+45` represented documented option pairs,
`+0` complete documented option pairs, `+0` matrix rows, `+0` verified rows,
`+0` invalid-input rows, and `+0` complete command matrices. Actual delta
matched. Current census counts are `5747` matrix rows, `4993` verified rows,
`719` invalid-input rows, `12` exact-open rows, `146/151` complete command
matrices, `1696/3212` complete documented option pairs, and
`1749/3212` represented documented option pairs. `clone` now reports a
visible documented surface at `36/47` represented option pairs with `96`
written rows, `96` classified rows, `86` stock-matching rows, `10`
invalid-input rows, and `0` exact-open rows instead of the prior misleading
`0/0` doc-option denominator. The next default follow-up should use the
corrected seed to prioritize another dense helper-free represented family or a
clone-specific review-promotion sweep rather than continue from the stale
pre-fix denominator.

As of 2026-06-26 the latest completed batch is a helper-free `git tag`
documented creation and listing family expansion on the current local lane.
The selected change added seven exact stock-Git `tag` rows for `--message`,
`--file`, `-F`, `--create-reflog`, `--points-at`, `--omit-empty`, and
`--column`, then promoted those seven represented documented options into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta was
`+7` matrix rows, `+7` complete documented option pairs, `+7`
represented documented option pairs, `+7` verified rows, `+0`
invalid-input rows, and `+0` complete command matrices. Actual delta matched.
Current census counts are `5747` matrix rows, `4993` verified rows, `719`
invalid-input rows, `12` exact-open rows, `146/151` complete command
matrices, `1696/3156` complete documented option pairs, and
`1704/3156` represented documented option pairs. `tag` now sits at
`28/28` reviewed-complete represented option pairs with `47` written rows,
`47` classified rows, `37` stock-matching rows, `10` invalid-input rows, and
`0` exact-open rows on the current modeled surface. The next default
follow-up should move to another dense helper-free represented family unless a
bounded `tag` signing/editor slice closes a larger than micro-sized remainder.

As of 2026-06-26 the latest completed batch is a helper-free `git pull`
fetch-inherited documented-option family expansion on the current named-local
ff-only lane. The selected change added twenty-four exact stock-Git `pull`
rows for `--show-forced-updates`, `--no-show-forced-updates`, `--ipv4`,
`--ipv6`, `--keep`, `--no-all`, `--prune`, `--tags`, `--no-tags`,
`--verbose`, `--recurse-submodules`, `--no-recurse-submodules`,
`--server-option`, `--jobs`, `-4`, `-6`, `-k`, `-o`, `-p`, `-t`, `-v`,
and `-j`, plus the documented invalid-input rejection for
`pull --jobs=bad --recurse-submodules`, then promoted those twenty-two
represented documented options into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta was
`+24` matrix rows, `+22` complete documented option pairs, `+22`
represented documented option pairs, `+23` verified rows, `+1`
invalid-input row, and `+0` complete command matrices. Actual delta matched.
Current census counts are `5740` matrix rows, `4986` verified rows, `719`
invalid-input rows, `12` exact-open rows, `146/151` complete command
matrices, `1689/3156` complete documented option pairs, and
`1697/3156` represented documented option pairs. `pull` now sits at
`36/36` reviewed-complete represented option pairs with `61` written rows,
`61` classified rows, `59` stock-matching rows, `2` invalid-input rows, and
`0` exact-open rows on the current modeled surface. The next default
follow-up should either close another dense helper-free represented family
from the refreshed census queue or spend a bounded slice on a larger
remaining `pull` tail only if it closes more than a single micro-row.

As of 2026-06-26 the latest completed batch is a helper-free `git fetch`
documented-option family expansion on the current named-local-remote lane.
The selected change added twenty-one exact stock-Git `fetch` rows for
`--auto-gc`, `--auto-maintenance`, `--ipv4`, `--ipv6`, `--keep`,
`--no-all`, `--no-auto-gc`, `--no-auto-maintenance`,
`--recurse-submodules-default`, `--refetch`, `--show-forced-updates`,
`--no-show-forced-updates`, `--submodule-prefix`, `--write-commit-graph`,
`--no-write-commit-graph`, `-4`, `-6`, `-P`, `-k`, `-o`, and `-u`, then
promoted those twenty-one represented documented options into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta was
`+21` matrix rows, `+21` complete documented option pairs, `+21`
represented documented option pairs, `+0` verified rows, `+0`
invalid-input rows, and `+0` complete command matrices. Actual delta
matched. Current census counts are `5716` matrix rows, `4963` verified rows,
`718` invalid-input rows, `12` exact-open rows, `146/151` complete command
matrices, `1667/3156` complete documented option pairs, and
`1675/3156` represented documented option pairs. `fetch` now sits at
`61/61` reviewed-complete represented option pairs with `377` written rows,
`356` classified rows, `341` stock-matching rows, `15` invalid-input rows,
and `0` exact-open rows on the current modeled surface. The next default
follow-up should either close the two remaining documented `fetch` tails
`--progress` and `--porcelain` with explicit stock behavior, or move to the
next dense helper-free represented-family candidate from the refreshed
census queue.

As of 2026-06-26 the latest completed batch is a helper-free `git commit`
dry-run status-family expansion plus reviewed-complete promotion. The
selected change added thirteen exact stock-Git `commit` rows for `--dry-run`,
`--long`, `--short`, `--porcelain`, `--branch`, `-z`, `--null`, `-u`, and
`--untracked-files` across the stable local lane with one staged tracked
modification plus one untracked file, then promoted those nine represented
documented options into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta was
`+13` matrix rows, `+13` verified rows, `+9` complete documented option
pairs, `+9` represented documented option pairs, `+0` invalid-input rows,
and `+0` complete command matrices. Actual delta matched. Current census
counts are `5695` matrix rows, `4963` verified rows, `718` invalid-input
rows, `12` exact-open rows, `146/151` complete command matrices,
`1646/3156` complete documented option pairs, and
`1654/3156` represented documented option pairs. `commit` now sits at
`46/46` reviewed-complete represented option pairs with `93` written rows,
`93` classified rows, `88` stock-matching rows, `5` invalid-input rows,
and `0` exact-open rows on the current modeled surface. The next default
follow-up should move off `commit` and return to the next largest
helper-free represented-family candidate from the refreshed census queue,
such as `format-patch`, `replay`, `log`, or `rev-list`, rather than spending
another micro-slice on the now-closed `commit` dry-run family.

As of 2026-06-26 the latest completed batch is a helper-free `git switch`
documented-option family expansion plus reviewed-complete promotion. The
selected change added fifteen exact stock-Git `switch` rows for `--quiet`,
`-q`, `--merge`, `-m`, `--conflict=merge`, `--progress`,
`--no-progress`, `--guess`, `--no-guess`,
`--recurse-submodules`, `--no-recurse-submodules`,
`--ignore-other-worktrees`, `-d`, `--force-create`, and `-C` on stable local
branch-switch lanes, then promoted those represented documented options into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta was
`+15` matrix rows, `+15` complete documented option pairs, `+15`
represented documented option pairs, `+15` verified rows, `+0` invalid-input
rows, and `+0` complete command matrices. Actual delta matched. Current
census counts are `5682` matrix rows, `4950` verified rows, `718`
invalid-input rows, `12` exact-open rows, `146/151` complete command
matrices, `1637/3156` complete documented option pairs, and
`1645/3156` represented documented option pairs. `switch` now sits at
`22/22` reviewed-complete represented option pairs with `23` written rows,
`23` classified rows, `23` stock-matching rows, `0` invalid-input rows, and
`0` exact-open rows on the current modeled surface. The next default
follow-up should skip the remaining three-row `switch` tracking micro-tail
unless it unlocks a broader family and instead move to the next larger
helper-free represented-family candidate such as the `commit`
status/dry-run surface.

As of 2026-06-26 the latest completed batch is a helper-free `git submodule`
documented-option closure plus command promotion. The selected change added
thirteen exact stock-Git `submodule` rows for `-n`, `--merge`, `--rebase`,
`--dissociate`, `--recommend-shallow`, `--no-recommend-shallow`, and
`--no-single-branch`, then promoted `submodule` into
`docs/cli/census/reviewed_complete_command_matrices.tsv` after the command
reached `30/30` reviewed-complete represented option pairs with `0` exact-open
rows on the current modeled surface. Current census counts are `5667` matrix
rows, `4935` verified rows, `718` invalid-input rows, `12` exact-open rows,
`146/151` complete command matrices, `1621/3156` complete documented option
pairs, and `1630/3156` represented documented option pairs. The next default
follow-up should move off `submodule` and back to the refreshed exact-open
queue or the next dense represented-family candidate from the census.

As of 2026-06-26 the latest completed batch is a zero-code reviewed-complete
command-surface promotion cluster. The selected change promotes
`index-pack`, `daemon`, `show`, `switch`, `worktree`, `tag`,
`update-index`, `commit`, `ls-remote`, `for-each-ref`, `fetch`, `log`,
`pull`, `rev-list`, `format-patch`, `grep`, `pack-objects`, `merge`,
`apply`, `revert`, `cherry-pick`, `rebase`, `send-email`, `replay`,
`range-diff`, `fast-import`, `p4`, `am`, `cvsserver`, `fast-export`,
`help`, and `reset` into
`docs/cli/census/reviewed_complete_command_matrices.tsv` because each command
already had all represented documented option pairs reviewed complete,
`100%` classified written rows, and `0` exact-open rows on the currently
modeled surface. Expected delta was `+0` matrix rows, `+0` complete
documented option pairs, `+32` complete command matrices, `+0`
represented documented option pairs, `+0` verified rows, and `+0`
invalid-input rows. Actual delta matched. Current census counts are `5654`
matrix rows, `4922` verified rows, `718` invalid-input rows, `12`
exact-open rows, `145/151` complete command matrices, `1605/3156`
complete documented option pairs, and `1614/3156` represented documented
option pairs. The promoted cluster now sits at `index-pack 15/15`,
`daemon 13/13`, `show 3/3`, `switch 7/7`, `worktree 8/8`, `tag 21/21`,
`update-index 16/16`, `commit 37/37`, `ls-remote 3/3`,
`for-each-ref 2/2`, `fetch 40/40`, `log 18/18`, `pull 14/14`,
`rev-list 12/12`, `format-patch 12/12`, `grep 7/7`,
`pack-objects 10/10`, `merge 8/8`, `apply 5/5`, `revert 4/4`,
`cherry-pick 4/4`, `rebase 3/3`, `send-email 2/2`, `replay 2/2`,
`range-diff 1/1`, `fast-import 1/1`, `p4 1/1`, `am 0/0`,
`cvsserver 0/0`, `fast-export 0/0`, `help 0/0`, and `reset 0/0`
reviewed-complete represented option pairs on their current modeled
surfaces, with `0` exact-open rows across all thirty-two commands. The
next best default follow-up should move to a new represented-family
expansion, with `submodule` now the densest current candidate at `14/14`
represented documented option pairs, `0` open rows, and `16` remaining
documented tails to expand, rather than spending the next slice on the
lone `rev-parse --exclude-hidden` micro-tail.

As of 2026-06-26 the latest completed batch is a zero-code reviewed-complete
diff-family promotion cluster. The selected change promotes `diff`,
`diff-files`, `diff-index`, and `diff-tree` into
`docs/cli/census/reviewed_complete_command_matrices.tsv` because each command
already had all represented documented option pairs reviewed complete,
`100%` classified written rows, and `0` exact-open rows on the currently
modeled helper-free local surface. Expected delta was `+0` matrix rows,
`+0` complete documented option pairs, `+4` complete command matrices, `+0`
represented documented option pairs, `+0` verified rows, and `+0`
invalid-input rows. Actual delta matched. The diff family now sits at
`diff 83/83`, `diff-files 79/79`, `diff-index 78/78`, and
`diff-tree 85/85` reviewed-complete represented option pairs on their
current modeled surfaces, with `0` exact-open rows across all four commands.
The next best default follow-up should either close the lone
`rev-parse --exclude-hidden` tail if a helper-free lane is available or move
to the next dense represented family from the refreshed census queue.

As of 2026-06-26 the latest completed batch is a zero-code reviewed-complete
`git rev-parse` promotion cluster. The selected change promotes the remaining
twenty-five represented `rev-parse` documented options `--all`,
`--branches`, `--tags`, `--remotes`, `--glob`, `--exclude`,
`--local-env-vars`, `--resolve-git-dir`, `--flags`, `--no-flags`,
`--revs-only`, `--no-revs`, `--default`, `--not`, `--symbolic`,
`--prefix`, `--sq`, `--sq-quote`, `--output-object-format`,
`--disambiguate`, `--shared-index-path`, `--parseopt`,
`--keep-dashdash`, `--stop-at-non-option`, and `--stuck-long` into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`, then promotes
`rev-parse` into `docs/cli/census/reviewed_complete_command_matrices.tsv`
because the command already had `50/51` represented documented options,
`111/111` classified rows, and `0` exact-open rows on the currently modeled
helper-free local surface. Expected delta was `+0` matrix rows, `+25`
complete documented option pairs, `+1` complete command matrix, `+0`
represented documented option pairs, `+0` verified rows, and `+0`
invalid-input rows. Actual delta matched. `rev-parse` now sits at `50/50`
reviewed-complete represented option pairs on the current modeled surface,
with `111/111` classified rows, `102` stock-matching rows, `9`
invalid-input rows, and `0` exact-open rows, while the lone documented
`--exclude-hidden` tail remains explicitly outside the modeled reviewed-
complete surface. The next best default follow-up should either close that
single tail if a helper-free lane is available or move to the next dense
represented family from the refreshed census queue.

As of 2026-06-26 the latest completed batch is a zero-code reviewed-complete
`git checkout` promotion cluster plus one matrix-row shape repair. The
selected change promotes the remaining twenty-two represented `checkout`
documented options `--conflict`, `--guess`, `--ignore-other-worktrees`,
`--ignore-skip-worktree-bits`, `--merge`, `--no-guess`, `--no-overlay`,
`--no-overwrite-ignore`, `--no-track`, `--ours`, `--overlay`,
`--overwrite-ignore`, `--patch`, `--pathspec-file-nul`,
`--pathspec-from-file`, `--progress`, `--theirs`, `--track`, `-d`, `-m`,
`-p`, and `-t` into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`, then promotes
`checkout` into `docs/cli/census/reviewed_complete_command_matrices.tsv`
because the command now has `34/34` represented documented options,
`79/79` classified rows, and `0` exact-open rows on the currently modeled
helper-free local surface. The batch also fixes the malformed
`checkout -B feature-reset <default branch>` matrix row so its existing
stock-Git evidence lands back in the verified census. Expected delta was
`+0` matrix rows, `+22` complete documented option pairs, `+1` complete
command matrix, `+0` represented documented option pairs, `+1` verified
row, and `+0` invalid-input rows. Actual delta matched. `checkout` now
sits at `34/34` reviewed-complete documented option pairs, `34/34`
represented documented option pairs, `79/79` classified rows, `67`
stock-matching rows, `12` invalid-input rows, and `0` exact-open rows. The
next best default follow-up should move to the next dense represented family
from the refreshed census queue now that both `checkout` and `restore` are
reviewed complete.

As of 2026-06-26 the latest completed batch is a zero-code reviewed-complete
`git restore` promotion cluster. The selected change promotes the remaining
eighteen represented `restore` documented options `--conflict`,
`--ignore-skip-worktree-bits`, `--ignore-unmerged`, `--merge`,
`--no-progress`, `--no-recurse-submodules`, `--ours`, `--overlay`,
`--patch`, `--pathspec-file-nul`, `--pathspec-from-file`, `--progress`,
`--quiet`, `--recurse-submodules`, `--theirs`, `-m`, `-p`, and `-q` into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`, then promotes
`restore` into `docs/cli/census/reviewed_complete_command_matrices.tsv`
because the command already had `25/25` represented documented options,
`100/100` classified rows, and `0` exact-open rows on the currently modeled
helper-free local surface. Expected delta was `+0` matrix rows, `+18`
complete documented option pairs, `+1` complete command matrix, `+0`
represented documented option pairs, `+0` verified rows, and `+0`
invalid-input rows. Actual delta matched. `restore` now sits at `25/25`
reviewed-complete documented option pairs, `25/25` represented documented
option pairs, `100/100` classified rows, `80` stock-matching rows, `20`
invalid-input rows, and `0` exact-open rows. The next best default
follow-up should either deepen the still-incomplete `checkout` documented
option expansions or move to the next dense represented family from the
refreshed census queue.

As of 2026-06-26 the latest completed batch is a helper-free `git restore`
documented-option family closure batch. The selected change adds nineteen
exact stock-Git rows for `restore --progress`, `--no-progress`, `--quiet`,
`-q`, `--pathspec-from-file`, `--pathspec-file-nul`, `--ignore-skip-worktree-bits`,
`--overlay`, `--merge`, `-m`, `--ours`, `--theirs`, `--conflict=merge`,
`--conflict merge`, `--recurse-submodules`, `--no-recurse-submodules`,
`--ignore-unmerged`, `--patch`, and `-p` on the current helper-free local
tracked-file surface. Expected delta was `+19` matrix rows, `+18`
represented documented option pairs, `+19` verified rows, `+0`
invalid-input rows, `+0` complete documented option pairs, and `+0`
complete command matrices. Actual delta matched. `restore` now sits at
`25/25` represented documented option pairs, `100/100` written rows,
`100/100` classified rows, `80` stock-matching rows, `20` invalid-input
rows, and `0` exact-open rows. The next best default follow-up should either
deepen the still-incomplete `restore` and `checkout` documented option
expansions now that both helper-free schema tails are closed, or move to the
next dense represented family from the refreshed census queue.

As of 2026-06-26 the latest completed batch is a helper-free `git checkout`
patch-family closure batch. The selected change adds six exact stock-Git rows
for `checkout --patch .`, `checkout -p .`, `checkout --patch -- a.txt`,
`checkout -p -- a.txt`, the dot-pathspec accept-first-then-quit lane, and the
single-path accept lane on the current local tracked-file surface. Expected
delta was `+6` matrix rows, `+2` represented documented option pairs, `+6`
verified rows, `+0` invalid-input rows, `+0` complete documented option
pairs, and `+0` complete command matrices. Actual delta matched. `checkout`
now sits at `34/34` represented documented option pairs, `79/79` written
rows, `78/79` classified rows, `66` stock-matching rows, `12`
invalid-input rows, and `0` exact-open rows. The next best default follow-up
should either deepen the still-incomplete `checkout` documented option
expansions or move to another dense represented family now that `checkout`
has no remaining schema-gap options.

As of 2026-06-26 the latest completed batch is a helper-free `git checkout`
overwrite-ignore/toggle plus short-detach batch. The selected change adds
four exact stock-Git rows for `checkout --overwrite-ignore .`,
`checkout --no-overwrite-ignore .`, `checkout --ignore-other-worktrees .`,
and the short `checkout -d .` detach/path invalid-input diagnostic on the
current local path-style lane. Expected delta was `+4` matrix rows,
`+4` represented documented option pairs, `+3` verified rows, `+1`
invalid-input row, `+0` complete documented option pairs, and `+0` complete
command matrices. Actual delta matched. `checkout` now sits at `32/34`
represented documented option pairs, `73/73` written rows, `72/73`
classified rows, `60` stock-matching rows, `12` invalid-input rows, and `0`
exact-open rows. The next best default follow-up should keep the same
helper-free `checkout` momentum and decide whether the remaining interactive
`patch` family is worth a bounded dedicated slice.

As of 2026-06-26 the latest completed batch is a helper-free `git annotate`
documented-option family closure plus reviewed-complete promotion. The
selected change adds twenty-five exact stock-Git rows for the full documented
annotate surface: `--color-by-age`, `--color-lines`, `--contents`, `--date`,
`--encoding`, `--first-parent`, `--ignore-rev`, `--ignore-revs-file`,
`--incremental`, `--line-porcelain`, `--no-progress`, `--porcelain`,
`--progress`, `--reverse`, `--root`, `--show-stats`, `-C`, `-L`, `-M`, `-S`,
`-b`, `-h`, `-l`, `-p`, and `-t`, then promotes those twenty-five documented
option pairs into `docs/cli/census/reviewed_complete_doc_option_pairs.tsv`
and promotes `annotate` into
`docs/cli/census/reviewed_complete_command_matrices.tsv` because the current
modeled helper-free annotate surface is now fully represented with `0`
exact-open rows. Expected delta was `+25` matrix rows, `+25` complete
documented option pairs, `+1` complete command matrix, `+25` represented
documented option pairs, `+23` verified rows, and `+2` invalid-input rows.
Actual delta matched. `annotate` now sits at `25/25`
reviewed-complete documented option pairs, `25/25` represented documented
option pairs, `26/26` classified rows, `24` stock-matching rows, `2`
invalid-input rows, and `0` exact-open rows. The next best default follow-up
should move to another dense helper-free family from the refreshed census
queue rather than the lone config-driven `rev-parse --exclude-hidden`
micro-tail.

As of 2026-06-26 the latest completed batch is a helper-free `git rev-parse`
parseopt/output/disambiguation expansion. The selected change adds nine exact
stock-Git rows for `--output-object-format=storage`, `--output-object-format=sha1`,
the stock unsupported `--output-object-format=sha256` rejection,
`--disambiguate=<prefix>`, `--shared-index-path`, and the documented
`--parseopt` / `--keep-dashdash` / `--stop-at-non-option` / `--stuck-long`
combined mode on the current helper-free lane. Expected delta was `+9` matrix
rows, `+7` represented documented option pairs, `+8` verified rows, `+1`
invalid-input row, `+0` complete documented option pairs, and `+0` complete
command matrices. Actual delta matched. `rev-parse` now sits at `25/51`
reviewed-complete documented option pairs, `50/51` represented documented
option pairs, `111/111` classified rows, `102` stock-matching rows, `9`
invalid-input rows, and `0` exact-open rows. The next best default follow-up
should move to a new dense helper-free family from the refreshed census queue
unless the last config-driven `rev-parse --exclude-hidden` micro-tail is
explicitly chosen first.

As of 2026-06-26 the latest completed batch is a helper-free `git config`
`--url` urlmatch closure plus reviewed-complete promotion. The selected change
adds five exact stock-Git rows for `config get --url=...` best-match lookup,
section listing, `--name-only`, `http` section matching, no-match exit `1`,
and the stock `--all` conflict rejection, then promotes the remaining twelve
represented `config` documented option pairs into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv` and promotes
`config` into `docs/cli/census/reviewed_complete_command_matrices.tsv`
because the current modeled helper-free `config` surface is now fully
represented with `0` exact-open rows. Expected delta was `+5` matrix rows,
`+12` complete documented option pairs, `+1` complete command matrix,
`+1` represented documented option pair, `+4` verified rows, and `+1`
invalid-input row. Actual delta matched. `config` now sits at `30/30`
reviewed-complete documented option pairs, `30/30` represented documented
option pairs, `160/160` classified rows, `145` stock-matching rows, `15`
invalid-input rows, and `0` exact-open rows. The next best default follow-up
should come from a new dense helper-free schema family in the refreshed
census queue rather than the lone represented `shortlog --stdin` micro-slice.

As of 2026-06-26 the latest completed batch is a helper-free `git config`
write/filter family expansion. The selected change adds ten exact stock-Git
rows for modern `config set --all`, `config unset --all`, `config set --value`,
modern and legacy `--fixed-value` filtering, legacy `--replace-all`,
legacy `--comment`, and `--get-colorbool`, while also correcting the shared
legacy/modern multi-value set behavior so plain `config key value` and
`config set key value` now keep stock multi-value rejection semantics unless
an explicit replacement mode is used. Expected delta was `+10` matrix rows,
`+4` represented documented option pairs, `+9` verified rows, `+1`
invalid-input row, `+0` complete documented option pairs, and `+0`
complete command matrices. Actual delta matched. `config` now sits at
`18/30` reviewed-complete documented option pairs, `29/30` represented
documented option pairs, `155/155` classified rows, `141` stock-matching
rows, `14` invalid-input rows, and only the documented `--url` helper-free
tail remains unrepresented. The next best helper-free follow-up can finish
that last `config` lane or move to the next dense represented family from the
refreshed census queue.

As of 2026-06-26 the latest completed batch is a helper-free `git config`
modern read-family schema/runtime expansion. The selected change adds eight
exact stock-Git rows for `config get --all`, `config get --regexp`,
`config get --all --regexp`, `config --name-only --list`,
`config --type=bool --no-type`, `config --no-includes --get`,
`config --system`, and `config --blob`, while extending the explicit schema
surface for the documented `--all`, `--blob`, `--name-only`,
`--no-includes`, `--no-type`, `--regexp`, and `--system` options without
pulling in the heavier `--replace-all`, `--fixed-value`, `--comment`,
`--url`, or `--get-colorbool` tails. Expected delta was `+8` matrix rows,
`+7` represented documented option pairs, `+8` verified rows, `+0`
invalid-input rows, `+0` complete documented option pairs, and `+0`
complete command matrices. Actual delta matched. `config` now sits at
`18/30` reviewed-complete documented option pairs, `25/30` represented
documented option pairs, `145/145` classified rows, `132` stock-matching
rows, `13` invalid-input rows, and `0` exact-open rows. The next best
helper-free follow-up can stay on the remaining compact `config` documented
tails if another coherent family appears; otherwise it should move to the
next dense represented command from the refreshed census queue.

As of 2026-06-26 the latest completed batch is a helper-free `git add`
interactive and patch quit-lane closure plus reviewed-complete promotion. The
selected change adds four exact stock-Git rows for `--interactive`,
`--patch`, `-i`, and `-p` on the tracked-modification quit lane, then
promotes those four documented option pairs into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv` and promotes `add`
into `docs/cli/census/reviewed_complete_command_matrices.tsv` because the
current modeled helper-free `add` surface is now fully represented with `0`
exact-open rows. Expected delta was `+4` matrix rows, `+4` complete
documented option pairs, `+1` complete command matrix, `+4` represented
documented option pairs, `+4` verified rows, and `+0` invalid-input rows.
Actual delta matched. `add` now sits at `30/30` reviewed-complete documented
option pairs, `30/30` represented documented option pairs, `130/130`
classified rows, `114` stock-matching rows, `16` invalid-input rows, and `0`
exact-open rows. The next best helper-free follow-up should move to another
dense represented family such as `config`.

As of 2026-06-26 the latest completed batch is a zero-code reviewed-complete
`git shortlog` promotion cluster. The selected change promotes the remaining
represented shortlog documented options `--all-match`, `--grep`,
`--grep-reflog`, `--group`, `--invert-grep`, `--reflog`,
`--regexp-ignore-case`, `--walk-reflogs`, `-g`, and `-i` into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`, then promotes
`shortlog` into `docs/cli/census/reviewed_complete_command_matrices.tsv`
because the command already had `29/125` represented documented options,
`59/59` classified rows, and `0` exact-open rows on the currently modeled
helper-free local surface. Expected delta was `+0` matrix rows, `+10`
complete documented option pairs, `+1` complete command matrix, `+0`
represented documented option pairs, `+0` verified rows, and `+0`
invalid-input rows. Actual delta matched. `shortlog` now sits at `28/29`
reviewed-complete documented option pairs, `29/125` represented documented
option pairs, `59/59` classified rows, `54` stock-matching rows, `5`
invalid-input rows, and `0` exact-open rows. The next best helper-free
follow-up should move to another dense represented family or another zero-code
review-promotion cluster if it offers a larger safe closure batch.

As of 2026-06-26 the latest completed batch is a helper-free `git shortlog`
grouping and reflog evidence expansion. The selected change adds exact
stock-Git evidence for repeated and composed `--group` lanes
(`author+committer`, reversed order, duplicate group keys, and
trailer-plus-format summaries) plus repeated and reordered reflog-family
compositions (`--grep-reflog` twice under walk-reflogs and `--reflog` with
`-g` in both orders) on the modeled helper-free local shortlog lanes.
Expected delta was `+7` matrix rows, `+0` complete documented option pairs,
`+0` represented documented option pairs, `+7` verified rows, `+0`
invalid-input rows, and `+0` complete command matrices. Actual delta matched.
`shortlog` now sits at `18/29` reviewed-complete documented option pairs,
`29/125` represented documented option pairs, `59/59` classified rows, `54`
stock-matching rows, `5` invalid-input rows, and `0` exact-open rows. The
next best helper-free follow-up can stay on the remaining shortlog review tail
or move to another dense represented family if it offers a larger safe
closure batch.

As of 2026-06-26 the latest completed batch is a helper-free `git shortlog`
grep-mode ordering closure plus reviewed-complete promotion. The selected
change makes repeated `-E`/`-F`/`-P` and long
`--extended-regexp`/`--fixed-strings`/`--perl-regexp` combinations follow
stock last-one-wins ordering, adds exact stock-Git evidence for both short
and long ordering permutations on the modeled helper-free local grep lane,
and then promotes those six documented option pairs into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta was
`+8` matrix rows, `+6` complete documented option pairs, `+0` represented
documented option pairs, `+8` verified rows, `+0` invalid-input rows, and
`+0` complete command matrices. Actual delta matched. `shortlog` now sits at
`18/29` reviewed-complete documented option pairs, `29/125` represented
documented option pairs, `52/52` classified rows, `47` stock-matching rows,
`5` invalid-input rows, and `0` exact-open rows. The next best helper-free
follow-up can stay on the remaining shortlog review tail around `--group`,
grep-family compositions, and reflog-family review closure, or move to
another dense represented family if it offers a larger safe closure batch.

As of 2026-06-26 the latest completed batch is a helper-free `git shortlog`
repeated scalar-option closure plus reviewed-complete promotion. The selected
change makes repeated `--format`, `--date`, and `-w` accept stock
last-one-wins ordering, adds exact stock-Git evidence for both repeated
orderings on the modeled helper-free local shortlog lanes, and then promotes
those three documented option pairs into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta was
`+6` matrix rows, `+3` complete documented option pairs, `+0` represented
documented option pairs, `+6` verified rows, `+0` invalid-input rows, and
`+0` complete command matrices. Actual delta matched. `shortlog` now sits at
`12/29` reviewed-complete documented option pairs, `29/125` represented
documented option pairs, `44/44` classified rows, `39` stock-matching rows,
`5` invalid-input rows, and `0` exact-open rows. The next best helper-free
follow-up can stay on the remaining shortlog expansion tail or move to
another dense represented family if it offers a larger safe closure batch.

As of 2026-06-26 the latest completed batch is a zero-code reviewed-complete
`git blame` promotion cluster. The selected change promotes the remaining
represented documented options `--contents`, `--encoding`, `--first-parent`,
`--ignore-rev`, `--ignore-revs-file`, `--reverse`, `-S`, and `-h` into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`, then promotes
`blame` into `docs/cli/census/reviewed_complete_command_matrices.tsv`
because the command already had `36/36` represented documented options,
`180/180` classified rows, and `0` exact-open rows. Expected delta was `+0`
matrix rows, `+8` complete documented option pairs, `+0` represented
documented option pairs, `+1` complete command matrix, `+0` verified rows,
and `+0` invalid-input rows. Actual delta matched. `blame` is now
command-complete at `36/36` reviewed-complete documented option pairs with
`36/36` represented documented option pairs, `180/180` classified rows, `98`
stock-matching rows, `82` invalid-input rows, and `0` exact-open rows. The
next best helper-free follow-up should return to the next dense represented
family such as the remaining `shortlog` expansion tail.

As of 2026-06-26 the latest completed batch is a helper-free `git shortlog`
reflog-family schema/runtime expansion. The selected change adds the
shortlog-local parser/runtime surface for `--reflog`, `--walk-reflogs`,
`-g`, and `--grep-reflog`, reuses the existing shortlog summarization lane
for `--reflog`, matches the current stock empty-output lane for
`--walk-reflogs` and `-g`, and closes the stock `--grep-reflog` guard both
with and without walk-reflogs on the modeled helper-free local history lane.
Expected delta was `+5` matrix rows, `+4` represented documented option
pairs, `+4` verified rows, `+1` invalid-input row, `+0` complete documented
option pairs, and `+0` complete command matrices. Actual delta matched.
`shortlog` now sits at `29/125` represented documented option pairs,
`9/29` reviewed-complete documented option pairs, `38/38` classified rows,
`33` stock-matching rows, `5` invalid-input rows, and `0` exact-open rows.
The next best helper-free follow-up should continue the remaining shortlog
expansion tail before deciding on a broader rev-list-backed schema sweep.

As of 2026-06-26 the latest completed batch is a helper-free `git shortlog`
long-form grep-alias evidence expansion. The selected change reuses the
already implemented grep-family runtime and adds exact stock-Git evidence for
`--regexp-ignore-case`, `--extended-regexp`, `--fixed-strings`, and
`--perl-regexp` on the modeled helper-free local shortlog lane. Expected
delta was `+4` matrix rows, `+4` represented documented option pairs, `+4`
verified rows, `+0` invalid-input rows, `+0` complete documented option
pairs, and `+0` complete command matrices. Actual delta matched. `shortlog`
now sits at `25/125` represented documented option pairs, `9/25`
reviewed-complete documented option pairs, `33/33` classified rows, `29`
stock-matching rows, `4` invalid-input rows, and `0` exact-open rows. The
next best helper-free follow-up should finish the remaining shortlog-specific
`-g` tail before deciding on a broader rev-list-backed schema sweep.

As of 2026-06-26 the latest completed batch is a helper-free `git shortlog`
grep-family schema expansion. The selected change adds the shortlog grep
surface for `--grep`, `--invert-grep`, `--all-match`, `-i`, `-E`, `-F`, and
`-P`, reuses the existing helper-free local shortlog summarization lane, and
adds new exact stock-Git evidence for default grep, case-insensitive grep,
invert-grep, all-match composition, extended-regexp, fixed-string, and
perl-regexp filtering. Expected delta was `+7` matrix rows, `+7`
represented documented option pairs, `+7` verified rows, `+0` invalid-input
rows, `+0` complete documented option pairs, and `+0` complete command
matrices. Actual delta matched. `shortlog` now sits at `21/125` represented
documented option pairs, `9/21` reviewed-complete documented option pairs,
`29/29` classified rows, `25` stock-matching rows, `4` invalid-input rows,
and `0` exact-open rows. The next best helper-free follow-up should finish
the remaining shortlog-specific `-g` tail before deciding on a broader
rev-list-backed schema sweep.

As of 2026-06-26 the latest completed batch is a helper-free `git shortlog`
documented explicit-schema family expansion. The selected change adds the
shortlog-specific schema surface for `--group`, `--format`, `--date`, `-w`,
and stock-compatible rejecting `--stdin`, reuses existing author/committer
summary lanes, adds new exact stock-Git evidence for trailer and format
grouping, custom subject rendering, date-aware format grouping, attached wrap
and zero-width wrap behavior, and closes invalid-input rows for bad group,
bad date, bad attached wrap, and the stock unknown `--stdin` spelling.
Expected delta was `+11` matrix rows, `+5` represented documented option
pairs, `+7` verified rows, `+4` invalid-input rows, `+0` complete documented
option pairs, and `+0` complete command matrices. Actual delta matched.
`shortlog` now sits at `14/125` represented documented option pairs,
`9/14` reviewed-complete documented option pairs, `22/22` classified rows,
`18` stock-matching rows, `4` invalid-input rows, and `0` exact-open rows.
The next best helper-free follow-up should finish the remaining shortlog
specific tail around `-g` / `-i` before deciding on a broader rev-list-backed
schema sweep.

As of 2026-06-26 the latest completed batch is a helper-free `git blame`
documented explicit-schema family expansion. The selected change adds the
remaining documented `blame` schema surface for `--contents`, `--encoding`,
`--first-parent`, `--ignore-rev`, `--ignore-revs-file`, `--reverse`, `-S`,
and `-h`, reuses the existing durable helper-free `--contents` oracle lane,
adds exact stock-Git evidence for the represented simple local
`--encoding` / `--first-parent` / `--ignore-rev` / `--ignore-revs-file` /
`-S` lanes, and closes `--reverse` plus short-help with exact stock-shaped
invalid-input rows. Expected delta was `+8` matrix rows, `+8` represented
documented option pairs, `+6` verified rows, `+2` invalid-input rows, `+0`
complete documented option pairs, and `+0` complete command matrices. Actual
delta matched. `blame` now sits at `36/36` represented documented option
pairs, `28/36` reviewed-complete documented option pairs, `180/180`
classified rows, `98` stock-matching rows, `82` invalid-input rows, and `0`
exact-open rows. The next best helper-free batch should move to another large
schema-gap family such as `shortlog`.

As of 2026-06-26 the latest completed batch is a helper-free `git index-pack`
documented-option family review closure. The selected change extends the
previous schema/runtime expansion to cover repeated `--threads` and
`--object-format` parsing, removes the repository hard requirement from the
standalone `--strict` and `--fsck-objects` pack-file validation lane, adds
exact stock-Git evidence for repeated thread counts, connectivity-check plus
strict/fsck compositions, zero and malformed unlimited size limits, repeated
last-one-wins size limits, unknown object-format rejection, repeated sha1
object-format acceptance, and both promisor message side effects and
pack-name rejection, then promotes those five documented option pairs into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta was
`+11` matrix rows, `+5` complete documented option pairs, `+0` represented
documented option pairs, `+8` verified rows, `+3` invalid-input rows, and `+0`
complete command matrices. Actual delta matched. `index-pack` now sits at
`15/16` reviewed-complete documented option pairs, `15/16` represented
documented option pairs, `33/33` classified rows, `23` stock-matching rows,
`10` invalid-input rows, and the remaining tail is only the documented
`--progress-title` option, which current local stock Git `2.50.1` and `2.53.0`
both reject with generic usage output.

As of 2026-06-26 the latest completed batch is a helper-free `git index-pack`
documented schema/runtime family expansion. The selected change adds the
remaining safe schema surface for `--threads`,
`--check-self-contained-and-connected`, `--max-input-size`,
`--object-format`, and `--promisor`, preserves current help parity by hiding
those internal-use flags from `-h`, adds exact stock-Git evidence for the
represented standalone pack-file, stdin rejection, and promisor sidecar lanes,
and restores the baseline doc cache after local artifact cleanup removed
`target`. Expected delta was `+5` matrix rows, `+5` represented documented
option pairs, `+4` verified rows, `+1` invalid-input row, `+0` complete
documented option pairs, and `+0` complete command matrices. Actual delta
matched. `index-pack` now sits at `15/16` represented documented option pairs
with `22/22` classified rows, `15` stock-matching rows, `7` invalid-input
rows, `10/16` reviewed-complete documented option pairs, and the remaining
tail is narrowed to `--progress-title` plus review/expansion work for the five
newly represented option families.

As of 2026-06-26 the latest completed batch is a helper-free `git show-branch`
documented explicit-schema closure plus command promotion. The selected change
extends the schema and runtime to cover `--list`, `--more`, `--independent`,
`--merge-base`, `--topics`, `--topo-order`, `--date-order`, `--sparse`,
`--color`, `--no-color`, `--reflog`, and the documented `-g` short alias,
then promotes those twelve documented option pairs plus `show-branch` itself
into the reviewed-complete census lists. Expected delta was `+13` matrix rows,
`+12` complete documented option pairs, `+12` represented documented option
pairs, `+1` complete command matrix, `+13` verified rows, and `+0`
invalid-input rows. Actual delta matched, and `show-branch` is now
command-complete at `19/19` reviewed-complete documented option pairs with
`19/19` represented documented option pairs, `23` written rows, `23`
verified rows, `0` invalid-input rows, and `0` exact-open written rows.

As of 2026-06-26 the latest completed batch is a helper-free `git difftool`
documented explicit-schema closure plus command promotion. The selected change
extends the schema and runtime to cover `--tool-help`, gui and no-gui
selection, rotate and skip ordering, trust-exit-code toggles, dir-diff
materialization, symlink and copy dir-diff modes, and the documented `-d` and
`-g` short aliases, then promotes those twelve documented option pairs plus
`difftool` itself into the reviewed-complete census lists. Expected delta was
`+12` matrix rows, `+12` complete documented option pairs, `+12`
represented documented option pairs, `+1` complete command matrix, `+12`
verified rows, and `+0` invalid-input rows. Actual delta matched, and
`difftool` is now command-complete at `19/19` reviewed-complete documented
option pairs with `19/19` represented documented option pairs, `21` written
rows, `21` verified rows, `0` invalid-input rows, and `0` exact-open written
rows.

As of 2026-06-26 the latest completed batch is a helper-free `git reflog`
expire-family closure plus command promotion. The selected change exposes the
documented expire-family options in the schema for census accounting, adds
exact stock-Git evidence for `--expire=all`, `--expire-unreachable=all` with
`--expire=never`, `--all`, `--single-worktree`, `--stale-fix`, and `-n`,
fixes the runtime special-policy handling for `all` and `never`, preserves
stock `reflog expire -h` exit shape under the typed expire subcommand, and
then promotes those six documented option pairs plus `reflog` itself into the
reviewed-complete census lists. Expected delta was `+6` matrix rows, `+6`
complete documented option pairs, `+6` represented documented option pairs,
`+1` complete command matrix, `+6` verified rows, and `+0` invalid-input
rows. Actual delta matched, and `reflog` is now command-complete at `10/10`
reviewed-complete documented option pairs with `10/10` represented documented
option pairs, `27` written rows, `24` verified rows, `2` invalid-input rows,
and `0` exact-open written rows.

As of 2026-06-26 the latest completed batch is a helper-free `git cat-file`
documented mailmap family plus `--allow-unknown-type` closure and command
promotion. The selected change adds exact stock-Git evidence for
`--use-mailmap` on commit-size, annotated-tag pretty, batch-check, and
batch-command lanes, closes the alias and negated last-one-wins forms
`--mailmap`, `--no-mailmap`, and `--no-use-mailmap`, records the current
stock invalid-object-type rejection for `--allow-unknown-type` on the modeled
corrupt loose-object lane, promotes those five documented option pairs into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`, and then promotes
`cat-file` into `docs/cli/census/reviewed_complete_command_matrices.tsv`.
Expected delta was `+9` matrix rows, `+5` complete documented option pairs,
`+5` represented documented option pairs, `+1` complete command matrix, `+7`
verified rows, and `+2` invalid-input rows. Actual delta matched, and
`cat-file` is now command-complete at `21/21` reviewed-complete documented
option pairs with `51/51` classified rows and `0` exact-open written rows.

As of 2026-06-26 the latest completed batch is a helper-free `git multi-pack-index`
positive `--bitmap` writer-lane closure plus command promotion. The selected
change expands the local two-pack writer path to emit stock-shaped MIDX
`RIDX` and `BTMP` chunks, create a checksum-valid bitmap sidecar named from
the MIDX checksum, promotes `--bitmap` into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`, and then promotes
`multi-pack-index` into
`docs/cli/census/reviewed_complete_command_matrices.tsv`. Expected delta was
`+1` matrix row, `+1` complete documented option pair, `+0` represented
documented option pairs, `+1` complete command matrix, `+1` verified row, and
`+0` invalid-input rows. Actual delta matched, and `multi-pack-index` is now
command-complete at `9/9` reviewed-complete documented option pairs with
`29/29` classified rows and `0` exact-open written rows.

As of 2026-06-26 the latest completed batch is a helper-free `git http-push`
short-delete family closure plus command promotion. The selected change adds
schema coverage and stock-Git evidence for the documented `-d` and `-D`
aliases on the current writable HTTP lane, where upstream Git currently fails
with the URL-access error instead of deleting the remote ref, promotes those
two short aliases into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`, and then promotes
`http-push` into `docs/cli/census/reviewed_complete_command_matrices.tsv`.
Expected delta was `+2` matrix rows, `+2` complete documented option pairs,
`+2` represented documented option pairs, `+1` complete command matrix,
`+2` verified rows, and `+0` invalid-input rows. Actual delta matched, and
`http-push` is now command-complete at `6/6` reviewed-complete documented
option pairs with `10/10` classified rows and `0` exact-open written rows.
The next bounded high-throughput follow-up should return to the heavier local
`multi-pack-index --bitmap` writer lane.

As of 2026-06-26 the latest completed batch is a helper-free `git commit-tree`
explicit gpg-sign family closure plus command promotion. The selected change
adds stock-Git evidence and runtime parity for `-S`, `--gpg-sign`,
`-Smissing`, and `--gpg-sign=missing`, promotes the two explicit signing
families into `docs/cli/census/reviewed_complete_doc_option_pairs.tsv`, and
then promotes `commit-tree` into
`docs/cli/census/reviewed_complete_command_matrices.tsv`. Expected delta was
`+4` matrix rows, `+2` complete documented option pairs, `+2` represented
documented option pairs, `+1` complete command matrix, `+2` verified rows,
and `+2` invalid-input rows. Actual delta matched, and `commit-tree` is now
command-complete at `6/6` reviewed-complete documented option pairs with
`62/62` classified rows and `0` exact-open written rows. The next bounded
high-throughput follow-up should prefer the compact `http-push` review tail
before returning to the heavier local `multi-pack-index --bitmap` writer lane.

As of 2026-06-26 the next batch is a helper-free `git multi-pack-index`
incremental and refs-snapshot family closure. The selected change expands the
write subcommand schema and runtime to cover stock-Git incremental write side
effects on fresh and existing non-incremental MIDX lanes, plus the currently
accepted-and-ignored refs-snapshot lane and the stock missing-file rejection
when refs-snapshot is combined with bitmap. Expected delta is `+5` matrix
rows, `+2` complete documented option pairs, `+3` represented documented
option pairs, `+0` complete command matrices, `+3` verified rows, `+2`
invalid-input rows, and `-2` remaining checklist rows. Actual delta matched
at the reviewed and represented doc-option levels, and `multi-pack-index` now
sits at `8/9` reviewed-complete documented option pairs with only the
positive `--bitmap` writer lane still open.

As of 2026-06-25 the next batch is a helper-free `git mergetool`
`--tool-help` plus `-O` closure and command promotion. The selected change
expands the schema and runtime to cover stock-Git host-local tool-help
listing, orderfile-driven multi-path ordering, stock backup-file naming, and
the exact blank-line output shape between consecutive conflicts, then promotes
`mergetool` into `docs/cli/census/reviewed_complete_command_matrices.tsv`.
Expected delta is `+2` matrix rows, `+2` complete documented option pairs,
`+2` represented documented option pairs, `+1` complete command matrix, `+2`
verified rows, `+0` invalid-input rows, and `-2` remaining checklist rows.
Actual delta matched, and `mergetool` is now command-complete at `10/10`
reviewed-complete documented option pairs with `16/16` classified rows and
`0` exact-open written rows.

As of 2026-06-25 the next batch is a helper-free `git fmt-merge-msg`
summary-alias closure plus command promotion. The selected change expands the
schema to cover `--summary` and `--no-summary`, resolves them against
`--log/--no-log` with last-one-wins ordering, upgrades the runtime summary
body to read fetched commit metadata from real `FETCH_HEAD` entries, adds a
focused fetched-branch stock-Git oracle lane, and then promotes
`fmt-merge-msg` into
`docs/cli/census/reviewed_complete_command_matrices.tsv`. Expected delta is
`+4` matrix rows, `+2` complete documented option pairs, `+2` represented
documented option pairs, `+1` complete command matrix, `+4` verified rows,
`+0` invalid-input rows, and `-2` remaining checklist rows. Actual delta
matched at row and doc-option level, and `fmt-merge-msg` is now
command-complete at `9/9` reviewed-complete documented option pairs with
`12/12` classified rows and `0` exact-open written rows.

As of 2026-06-25 the next batch is a helper-free `git instaweb` module-path
tail closure plus command promotion. The selected change adds the documented
`--module-path` and `-m` spellings to schema coverage, records the stock-Git
lighttpd lane where both forms are silently accepted and ignored, adds focused
exact rows for those spellings, and then promotes `instaweb` into
`docs/cli/census/reviewed_complete_command_matrices.tsv`. Expected delta is
`+2` matrix rows, `+2` complete documented option pairs, `+2` represented
documented option pairs, `+1` complete command matrix, `+2` verified rows,
`+0` invalid-input rows, and `-2` remaining checklist rows. Actual delta
matched, and `instaweb` is now command-complete at `13/13`
reviewed-complete documented option pairs with `13/13` classified rows and
`0` exact-open written rows.

As of 2026-06-25 the next batch is a helper-free `git http-fetch`
documented-plural rejection closure plus command promotion. The selected
change adds the documented `--index-pack-args` spelling to schema coverage,
records exact stock-Git rejection rows for the packfile-helper and
non-packfile lanes where upstream Git currently ignores that plural spelling,
and then promotes `http-fetch` into
`docs/cli/census/reviewed_complete_command_matrices.tsv`. Expected delta is
`+2` matrix rows, `+1` complete documented option pair, `+1` represented
documented option pair, `+1` complete command matrix, `+0` verified rows,
`+2` invalid-input rows, and `-2` remaining checklist rows. Actual delta
matched, and `http-fetch` is now command-complete at `9/9`
reviewed-complete documented option pairs with `14/14` classified rows and
`0` exact-open written rows.

As of 2026-06-25 the next batch is a zero-row census-accounting correction for
reviewed-complete commands. The selected change teaches
`tools/git-compat-census.py` to suppress residual schema-only
`implemented but unverified` rows once a command has already been promoted into
`docs/cli/census/reviewed_complete_command_matrices.tsv`, which removes the
stale `archive <positional:args>` and `ls-tree --long` leftovers without
changing Rust runtime behavior or row-level stock-Git evidence. Expected delta
is `+0` matrix rows, `+0` complete documented option pairs, `+0` represented
documented option pairs, `+0` complete command matrices, `+0` verified rows,
`+0` invalid-input rows, and `-2` implemented-but-unverified schema rows.
Actual delta matched, and the census now reports
`implemented_but_unverified_rows=0`.

As of 2026-06-25 the next batch is the final helper-backed plus helper-free
`git fetch-pack` tail closure and command promotion. The selected change keeps
the existing local fetch-pack oracle lane, adds durable stock-Git evidence for
the documented `--check-self-contained-and-connected` and `--refetch`
surfaces, wires plain local `--refetch` into the default upload-pack helper
path so it skips local have negotiation like stock Git after a prior fetch,
and then promotes `fetch-pack` into
`docs/cli/census/reviewed_complete_command_matrices.tsv`. Expected delta is
`+2` matrix rows, `+2` complete documented option pairs, `+2` represented
documented option pairs, `+1` complete command matrix, `+2` verified rows,
`+0` invalid-input rows, and `-2` remaining checklist rows. Actual delta
matched, and `fetch-pack` is now command-complete at `18/18` reviewed-complete
documented option pairs with `25/25` classified rows and `0` exact-open
written rows.

As of 2026-06-25 the next batch is a helper-free `git multi-pack-index`
write-option family closure for `--no-bitmap`, `--preferred-pack`, and
`--stdin-packs`. The selected change expands the write subcommand schema and
runtime to match stock Git for suppressing bitmap sidecar creation, selecting a
preferred duplicate-object pack, warning on an unknown preferred pack, and
reading pack index basenames from stdin including the no-match failure case on
the existing local two-pack oracle lane. Expected delta is `+5` matrix rows,
`+3` complete documented option pairs, `+3` represented documented option
pairs, `+0` complete command matrices, `+4` verified rows, `+1` invalid-input
row, and `-3` remaining checklist rows. Actual delta matched, and
`multi-pack-index` now sits at `6/9` reviewed-complete documented option pairs
with only `--bitmap`, `--incremental`, and `--refs-snapshot` still open.

As of 2026-06-25 the next batch is a helper-free `git send-pack` schema-tail
closure plus command promotion. The selected change expands `send-pack` to
cover `--exec`, `--no-signed`, `--signed[=<mode>]`, and `--push-option`,
matches stock Git for signed-push fatal and warning paths on a local bare
remote, and fixes no-op local status reporting to emit `Everything up-to-date`
instead of synthetic ref lines. Expected delta is `+6` matrix rows, `+4`
complete documented option pairs, `+4` represented documented option pairs,
`+1` complete command matrix, `+4` verified rows, `+2` invalid-input rows,
and `-4` remaining checklist rows. Actual delta matched, and `send-pack` is
now command-complete at `12/12`.

As of 2026-06-25 the next batch is a helper-free `git ls-tree` schema-tail
closure plus command promotion. The selected change expands `ls-tree` to cover
the documented `-d`, `-l`, `-z`, `--name-status`, `--object-only`,
`--abbrev[=<n>]`, `--full-name`, `--full-tree`, and `--format` surfaces while
matching stock Git for repository-subdirectory scope and full-tree traversal.
Expected delta is `+9` matrix rows, `+10` complete documented option pairs,
`+10` represented documented option pairs, `+1` complete command matrix, `+9`
verified rows, `+0` invalid-input rows, and `-10` remaining checklist rows.
Actual delta matched, and `ls-tree` is now command-complete at `13/13`.

As of 2026-06-25 the next batch is a helper-free `git filter-branch`
`--prune-empty` closure plus docs-option inventory correction. The selected
change adds exact stock-Git evidence for pruning a single-parent empty commit
and rejecting the documented `--commit-filter` conflict, while tightening
`tools/git-compat-option-inventory.sh` to only seed AsciiDoc option-definition
lines so prose-only tokens such as the `filter-branch` example
`--ignore-unmatch` stop inflating the denominator. Expected delta is `+2`
matrix rows, `+0` net complete documented option pairs because the new
`--prune-empty` closure is offset by removing false docs seeds from the global
denominator, `+1` complete command matrix, `+2` verified rows, `+0`
invalid-input rows, and a lower documented-option seed plus remaining
checklist count. Actual delta matched, and `filter-branch` is now
command-complete at `15/15`.

As of 2026-06-25 the next batch is a zero-code command promotion for
helper-free `git describe`. The selected change promotes `describe` into
`docs/cli/census/reviewed_complete_command_matrices.tsv` once its already-
reviewed `14/14` documented option pairs, `14/14` represented documented
option pairs, and `19/19` classified rows are made durable at command level.
Expected delta is `+0` matrix rows, `+0` complete documented option pairs,
`+1` complete command matrix, `+0` represented documented option pairs, `+0`
verified rows, `+0` invalid-input rows, and `+0` remaining checklist rows.
Rust behavior changes are not expected. Actual delta matched.

As of 2026-06-25 the next batch is a helper-free `git describe` documented
option family closure for `--broken`, `--candidates`, `--contains`, `--debug`,
and `--first-parent`. The selected change expands the describe schema and
runtime to match stock Git on descendant-tag selection, first-parent-only
traversal, exact-match-only `--candidates=0` failure output, `--debug` search
trace stderr, and corrupt-index `--broken` fallback behavior. Expected delta
is `+5` matrix rows, `+5` complete documented option pairs, `+0` complete
command matrices, `+5` verified rows, `+0` invalid-input rows, and `-5`
remaining checklist rows. Actual delta matched.

As of 2026-06-25 the next batch is a helper-free upstream-oracle `instaweb`
lighttpd family closure plus internal-surface reclassification. The selected
change closes the stock Git `instaweb` start, stop, restart, browser-ignore,
httpd, local, port and short-alias lighttpd rows using an upstream Git
`v2.47.1` helper harness, while moving the Zmin-only internal
`--daemon-internal`, `--git-dir` and `--work-tree` surfaces out of the Git
compatibility denominator into the extension inventory. Expected delta is `-3`
matrix rows, `+11` complete documented option pairs, `+0` complete command
matrices, `+11` verified rows, `+0` invalid-input rows, `-14` open rows,
`+3` extension/deferred rows, and `-14` remaining checklist rows. Actual
delta matched.

As of 2026-06-25 the next batch is a helper-free existing-oracle
`multi-pack-index --progress/--no-progress` family closure. The selected
change promotes the currently modeled `multi-pack-index` progress toggles from
schema-only backlog into exact stock-Git evidence across write, verify,
expire, and batch-size-one repack lanes, and adds the missing runtime parity
for `verify --progress` stderr plus last-one-wins progress toggle resolution.
Expected delta is `+8` matrix rows, `+2` complete documented option pairs,
`+0` complete command matrices, `+8` verified rows, `+0` invalid-input rows,
and `-2` remaining checklist rows. Actual delta matched.

As of 2026-06-25 the next batch is a census-only classification correction for
top-level hyphen-prefix commands. The selected change does not add new
behavior evidence or change Rust runtime behavior; it fixes
`tools/git-compat-census.py` so nested schema refs only come from additional
nested commands outside the baseline Git command list. Expected delta is `+0`
matrix rows, `+0` complete documented option pairs, `+0` complete command
matrices, `+0` verified rows, `+0` invalid-input rows, and `+0` remaining
checklist rows, while removing false `implemented but unverified` backlog
items for top-level families such as `merge`, `diff`, `fetch`, and
`checkout`. Actual delta matched; the real remaining implemented-without-
matrix-evidence family is now `multi-pack-index --progress/--no-progress`.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented helper-free `rebase` surfaces
`--interactive` and `-i`. The selected rows do not add new behavior evidence;
they promote two exact stock-Git invalid-input `rebase --interactive` families
with existing helper-free interactive-todo parity coverage into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta is
`+0` matrix rows, `+2` complete documented option pairs, `+0` complete
command matrices, `+0` verified rows, `+0` invalid-input rows, and `-2`
remaining checklist rows. Rust behavior changes are not expected. Actual delta
matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented helper-free `checkout` surfaces
`--detach`, `-B`, and `-b`. The selected rows do not add new behavior
evidence; they promote three exact stock-Git `checkout` documented option
families with existing helper-free local detach, branch-create, and branch
reset parity coverage into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta is
`+0` matrix rows, `+3` complete documented option pairs, `+0` complete
command matrices, `+0` verified rows, `+0` invalid-input rows, and `-3`
remaining checklist rows. Rust behavior changes are not expected. Actual delta
matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented helper-free `submodule` surfaces
`--cached`, `--quiet`, and `--remote`. The selected rows do not add new
behavior evidence; they promote three exact stock-Git `submodule` documented
option families with existing helper-free local status, summary, add, update,
set-url, and deinit parity coverage into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta is
`+0` matrix rows, `+3` complete documented option pairs, `+0` complete
command matrices, `+0` verified rows, `+0` invalid-input rows, and `-3`
remaining checklist rows. Rust behavior changes are not expected. Actual delta
matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented helper-free `checkout` surfaces
`--force`, `--no-progress`, `--no-recurse-submodules`, `--orphan`,
`--quiet`, `--recurse-submodules`, `-f`, `-l`, and `-q`. The selected rows do
not add new behavior evidence; they promote nine exact stock-Git `checkout`
documented option families with existing helper-free local path checkout,
forced `HEAD`, current-branch, orphan, and create-reflog parity coverage into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta is
`+0` matrix rows, `+9` complete documented option pairs, `+0` complete
command matrices, `+0` verified rows, `+0` invalid-input rows, and `-9`
remaining checklist rows. Rust behavior changes are not expected. Actual delta
matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented helper-free `submodule` surfaces
`--branch`, `--depth`, `--files`, `--init`, `--name`, `--no-fetch`,
`--progress`, `--reference`, `--single-branch`, `--summary-limit`, and `-b`.
The selected rows do not add new behavior evidence; they promote eleven exact
stock-Git `submodule` documented option families with existing helper-free
local add, update, summary, and branch-metadata parity coverage into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta is
`+0` matrix rows, `+11` complete documented option pairs, `+0` complete
command matrices, `+0` verified rows, `+0` invalid-input rows, and `-11`
remaining checklist rows. Rust behavior changes are not expected. Actual delta
matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented helper-free `reflog expire`
surfaces `--dry-run`, `--rewrite`, `--updateref`, and `--verbose`. The
selected rows do not add new behavior evidence; they promote four exact
stock-Git `reflog expire` documented option families with existing dry-run
non-mutation and current-entry expiry parity coverage into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta is
`+0` matrix rows, `+4` complete documented option pairs, `+0` complete command
matrices, `+0` verified rows, `+0` invalid-input rows, and `-4` remaining
checklist rows. Rust behavior changes are not expected. Actual delta matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented helper-free `p4 --branch`
surface. The selected rows do not add new behavior evidence; they promote one
exact stock-Git `git p4 clone --branch master //depot/project <target>`
documented option family with existing branch/ref/message parity, imported
worktree contents, and clone-side config side-effect coverage into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta is
`+0` matrix rows, `+1` complete documented option pair, `+0` complete command
matrices, `+0` verified rows, `+0` invalid-input rows, and `-1` remaining
checklist row. Rust behavior changes are not expected. Actual delta matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented helper-free
`multi-pack-index --object-dir` surface. The selected rows do not add new
behavior evidence; they promote one exact stock-Git `multi-pack-index` option
family with existing object-directory write matrix coverage into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta is
`+0` matrix rows, `+1` complete documented option pair, `+0` complete command
matrices, `+0` verified rows, `+0` invalid-input rows, and `-1` remaining
checklist row. Rust behavior changes are not expected. Actual delta matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented helper-free
`fast-import --date-format` surface. The selected rows do not add new
behavior evidence; they promote one exact stock-Git `fast-import` option
family with existing `rfc2822`, `now`, and invalid-value matrix coverage into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta is
`+0` matrix rows, `+1` complete documented option pair, `+0` complete command
matrices, `+0` verified rows, `+0` invalid-input rows, and `-1` remaining
checklist row. Rust behavior changes are not expected. Actual delta matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented helper-free
`range-diff --no-dual-color` surface. The selected rows do not add new
behavior evidence; they promote one exact stock-Git `range-diff` option family
with existing explicit, repeated, negated, and invalid attached-value matrix
coverage into `docs/cli/census/reviewed_complete_doc_option_pairs.tsv`.
Expected delta is `+0` matrix rows, `+1` complete documented option pair,
`+0` complete command matrices, `+0` verified rows, `+0` invalid-input rows,
and `-1` remaining checklist row. Rust behavior changes are not expected.
Actual delta matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented helper-free `rebase --onto`
surface. The selected rows do not add new behavior evidence; they promote one
exact stock-Git `rebase` option family with existing onto-base, positional-
argument, tree, branch, log, and clean-status matrix coverage into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta is
`+0` matrix rows, `+1` complete documented option pair, `+0` complete command
matrices, `+0` verified rows, `+0` invalid-input rows, and `-1` remaining
checklist row. Rust behavior changes are not expected.
Actual delta matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented helper-free `replay` surface.
The selected rows do not add new behavior evidence; they promote two exact
stock-Git `replay` option families with existing linear-range advance, onto,
contained-mode, usage-failure, and outside-repository matrix coverage into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta is
`+0` matrix rows, `+2` complete documented option pairs, `+0` complete
command matrices, `+0` verified rows, `+0` invalid-input rows, and `-2`
remaining checklist rows. Rust behavior changes are not expected.
Actual delta matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented helper-free `for-each-ref`
surface. The selected rows do not add new behavior evidence; they promote two
exact stock-Git `for-each-ref` option families with existing format-atom,
tracking, date, bare-repository, sort-order, and invalid-value matrix
coverage into `docs/cli/census/reviewed_complete_doc_option_pairs.tsv`.
Expected delta is `+0` matrix rows, `+2` complete documented option pairs,
`+0` complete command matrices, `+0` verified rows, `+0` invalid-input rows,
and `-2` remaining checklist rows. Rust behavior changes are not expected.
Actual delta matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented helper-free `ls-tree` surface.
The selected rows do not add new behavior evidence; they promote three exact
stock-Git `ls-tree` option families with existing recursive, tree-entry, and
name-only matrix coverage into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta is
`+0` matrix rows, `+3` complete documented option pairs, `+0` complete
command matrices, `+0` verified rows, `+0` invalid-input rows, and `-3`
remaining checklist rows. Rust behavior changes are not expected.
Actual delta matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented helper-free `ls-remote` surface.
The selected rows do not add new behavior evidence; they promote two exact
stock-Git `ls-remote` option families with existing multi-transport `--refs`
and `--tags` matrix coverage into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta is
`+0` matrix rows, `+2` complete documented option pairs, `+0` complete
command matrices, `+0` verified rows, `+0` invalid-input rows, and `-2`
remaining checklist rows. Rust behavior changes are not expected.
Actual delta matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented helper-free `cherry-pick`
surface. The selected rows do not add new behavior evidence; they promote four
exact stock-Git `cherry-pick` option families with existing mainline and
no-commit matrix coverage into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta is
`+0` matrix rows, `+4` complete documented option pairs, `+0` complete
command matrices, `+0` verified rows, `+0` invalid-input rows, and `-4`
remaining checklist rows. Rust behavior changes are not expected.
Actual delta matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented helper-free `commit-tree`
surface. The selected rows do not add new behavior evidence; they promote four
exact stock-Git `commit-tree` option families with existing message, message-
file, parent, and no-gpg-sign matrix coverage into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta is
`+0` matrix rows, `+4` complete documented option pairs, `+0` complete
command matrices, `+0` verified rows, `+0` invalid-input rows, and `-4`
remaining checklist rows. Rust behavior changes are not expected.
Actual delta matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented helper-free `http-push` surface.
The selected rows do not add new behavior evidence; they promote four exact
stock-Git `http-push` option families with existing outside-repository
rejection and HTTP remote-ref matrix coverage into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta is
`+0` matrix rows, `+4` complete documented option pairs, `+0` complete
command matrices, `+0` verified rows, `+0` invalid-input rows, and `-4`
remaining checklist rows. Rust behavior changes are not expected.
Actual delta matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented helper-free `apply` surface.
The selected rows do not add new behavior evidence; they promote five exact
stock-Git `apply` option families with existing check, cached, index, reverse,
and short-reverse matrix coverage into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta is
`+0` matrix rows, `+5` complete documented option pairs, `+0` complete
command matrices, `+0` verified rows, `+0` invalid-input rows, and `-5`
remaining checklist rows. Rust behavior changes are not expected.
Actual delta matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented helper-free `difftool` surface.
The selected rows do not add new behavior evidence; they promote seven exact
stock-Git `difftool` option families with existing extcmd, configured-tool,
prompt, short-alias, and path-limited matrix coverage into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta is
`+0` matrix rows, `+7` complete documented option pairs, `+0` complete
command matrices, `+0` verified rows, `+0` invalid-input rows, and `-7`
remaining checklist rows. Rust behavior changes are not expected.
Actual delta matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented helper-free `switch` surface.
The selected rows do not add new behavior evidence; they promote seven exact
stock-Git `switch` option families with existing create, orphan, detach,
discard-changes, and force matrix coverage into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta is
`+0` matrix rows, `+7` complete documented option pairs, `+0` complete
command matrices, `+0` verified rows, `+0` invalid-input rows, and `-7`
remaining checklist rows. Rust behavior changes are not expected.
Actual delta matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented helper-free `restore` surface.
The selected rows do not add new behavior evidence; they promote seven exact
stock-Git `restore` option families with existing source, staged, worktree,
no-overlay, short-form alias, repeated-form, and invalid-value matrix
coverage into `docs/cli/census/reviewed_complete_doc_option_pairs.tsv`.
Expected delta is `+0` matrix rows, `+7` complete documented option pairs,
`+0` complete command matrices, `+0` verified rows, `+0` invalid-input rows,
and `-7` remaining checklist rows. Rust behavior changes are not expected.
Actual delta matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented helper-free `grep` surface. The
selected rows do not add new behavior evidence; they promote seven exact
stock-Git `grep` option families with existing tracked-file, treeish, path-
limited, cached, fixed-string, line-number, and filename-only matrix coverage
into `docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta
is `+0` matrix rows, `+7` complete documented option pairs, `+0` complete
command matrices, `+0` verified rows, `+0` invalid-input rows, and `-7`
remaining checklist rows. Rust behavior changes are not expected.
Actual delta matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented helper-free `merge` surface. The
selected rows do not add new behavior evidence; they promote eight exact
stock-Git `merge` option families with existing local fast-forward, merge-
commit, squash, strategy, continue, and abort matrix coverage into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta is
`+0` matrix rows, `+8` complete documented option pairs, `+0` complete
command matrices, `+0` verified rows, `+0` invalid-input rows, and `-8`
remaining checklist rows. Rust behavior changes are not expected.
Actual delta matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented helper-free `pull` surface. The
selected rows do not add new behavior evidence; they promote fourteen exact
stock-Git `pull` option families with existing local/file and HTTP fast-
forward/rebase, merge-mode, strategy, upload-pack, and shallow-fetch matrix
coverage into `docs/cli/census/reviewed_complete_doc_option_pairs.tsv`.
Expected delta is `+0` matrix rows, `+14` complete documented option pairs,
`+0` complete command matrices, `+0` verified rows, `+0` invalid-input rows,
and `-14` remaining checklist rows. Rust behavior changes are not expected.
Actual delta matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented helper-free `send-email`
surface. The selected rows do not add new behavior evidence; they promote two
exact stock-Git `send-email` option families with existing local alias
dump/translation matrix coverage into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta is
`+0` matrix rows, `+2` complete documented option pairs, `+0` complete
command matrices, `+0` verified rows, `+0` invalid-input rows, and `-2`
remaining checklist rows. Rust behavior changes are not expected.
Actual delta matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented helper-free `revert` surface.
The selected rows do not add new behavior evidence; they promote four exact
stock-Git `revert` option families with existing local merge-mainline and
no-commit matrix coverage into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta is
`+0` matrix rows, `+4` complete documented option pairs, `+0` complete
command matrices, `+0` verified rows, `+0` invalid-input rows, and `-4`
remaining checklist rows. Rust behavior changes are not expected.
Actual delta matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented helper-free `send-pack`
surface. The selected rows do not add new behavior evidence; they promote
seven exact stock-Git `send-pack` option families with existing local default
push, mirror sync, receive-pack override, stdin ref, atomic rejection, and
dry-run/force/verbose/thin matrix coverage into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta is
`+0` matrix rows, `+7` complete documented option pairs, `+0` complete
command matrices, `+0` verified rows, `+0` invalid-input rows, and `-7`
remaining checklist rows. Rust behavior changes are not expected.
Actual delta matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented helper-free `show-branch`
surface. The selected rows do not add new behavior evidence; they promote
seven exact stock-Git `show-branch` option families with existing local
branch-graph, explicit-revision, current-branch, no-name, and remote-listing
matrix coverage into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta is
`+0` matrix rows, `+7` complete documented option pairs, `+0` complete
command matrices, `+0` verified rows, `+0` invalid-input rows, and `-7`
remaining checklist rows. Rust behavior changes are not expected.
Actual delta matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented helper-free `shortlog` surface.
The selected rows do not add new behavior evidence; they promote nine exact
stock-Git `shortlog` option families with existing local author-summary,
committer-grouping, summary, numbering, email, range, and non-merge matrix
coverage into `docs/cli/census/reviewed_complete_doc_option_pairs.tsv`.
Expected delta is `+0` matrix rows, `+9` complete documented option pairs,
`+0` complete command matrices, `+0` verified rows, `+0` invalid-input rows,
and `-9` remaining checklist rows. Rust behavior changes are not expected.
Actual delta matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented helper-free `pack-objects`
surface. The selected rows do not add new behavior evidence; they promote ten
exact stock-Git `pack-objects` option families with existing local basename
output, stdout streaming, revision-selection stdin, index-version value,
progress, delta-shaping, and readable-pack matrix coverage into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta is
`+0` matrix rows, `+10` complete documented option pairs, `+0` complete
command matrices, `+0` verified rows, `+0` invalid-input rows, and `-10`
remaining checklist rows. Rust behavior changes are not expected. Actual
delta matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented helper-free `index-pack`
surface. The selected rows do not add new behavior evidence; they promote ten
exact stock-Git `index-pack` option families with existing local stdin pack
indexing, reverse-index toggles, keep/index-version side effects, strict/fsck
validation, thin-pack repair, and standalone output-path/verbose matrix
coverage into `docs/cli/census/reviewed_complete_doc_option_pairs.tsv`.
Expected delta is `+0` matrix rows, `+10` complete documented option pairs,
`+0` complete command matrices, `+0` verified rows, `+0` invalid-input rows,
and `-10` remaining checklist rows. Rust behavior changes are not expected.
Actual delta matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented helper-free `format-patch`
surface. The selected rows do not add new behavior evidence; they promote
twelve exact stock-Git `format-patch` option families with existing local
mail-series stdout, cover-letter, numbering, output-directory, merge-commit,
binary-summary, and filename-shaping matrix coverage into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta is
`+0` matrix rows, `+12` complete documented option pairs, `+0` complete
command matrices, `+0` verified rows, `+0` invalid-input rows, and `-12`
remaining checklist rows. Rust behavior changes are not expected. Actual
delta matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented helper-free `rev-list` surface.
The selected rows do not add new behavior evidence; they promote twelve exact
stock-Git `rev-list` option families with existing local history traversal,
object traversal, symmetric-difference, merge-graph child listing, filter, and
separator matrix coverage into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta is
`+0` matrix rows, `+12` complete documented option pairs, `+0` complete
command matrices, `+0` verified rows, `+0` invalid-input rows, and `-12`
remaining checklist rows. Rust behavior changes are not expected. Actual
delta matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented helper-free `stash` surface. The
selected rows do not add new behavior evidence; they promote sixteen exact
stock-Git `stash` option families with existing local push, show, list, apply,
pop, drop, branch, create, store, legacy save, patch, pathspec, and diff-option
matrix coverage into `docs/cli/census/reviewed_complete_doc_option_pairs.tsv`.
Expected delta is `+0` matrix rows, `+16` complete documented option pairs,
`+0` complete command matrices, `+0` verified rows, `+0` invalid-input rows,
and `-16` remaining checklist rows. Rust behavior changes are not expected.
Actual delta matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented helper-free `update-index`
surface. The selected rows do not add new behavior evidence; they promote
sixteen exact stock-Git `update-index` option families with existing local
index-mutation, flag-toggle, stdin, cacheinfo, index-info, replace, chmod,
and stat-refresh matrix coverage into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta is
`+0` matrix rows, `+16` complete documented option pairs, `+0` complete
command matrices, `+0` verified rows, `+0` invalid-input rows, and `-16`
remaining checklist rows. Rust behavior changes are not expected. Actual
delta matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented helper-free `log` surface. The
selected rows do not add new behavior evidence; they promote eighteen exact
stock-Git `log` option families with existing local history-traversal,
formatting, decoration, reflog-walk, merge-diff, pickaxe, date-format, and
replacement-shim matrix coverage into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta is
`+0` matrix rows, `+18` complete documented option pairs, `+0` complete
command matrices, `+0` verified rows, `+0` invalid-input rows, and `-18`
remaining checklist rows. Rust behavior changes are not expected. Actual
delta matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented helper-free `diff` surface. The
selected rows do not add new behavior evidence; they promote ten exact
stock-Git `diff` option families with existing local treeish-pair,
patch-format, ordering, rename-toggle, reverse, pickaxe, and no-index matrix
coverage into `docs/cli/census/reviewed_complete_doc_option_pairs.tsv`.
Expected delta is `+0` matrix rows, `+10` complete documented option pairs,
`+0` complete command matrices, `+0` verified rows, `+0` invalid-input rows,
and `-10` remaining checklist rows. Rust behavior changes are not expected.
Actual delta matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented `http-fetch` surface. The
selected rows do not add new behavior evidence; they promote eight exact
stock-Git `http-fetch` option families with existing packfile-helper,
dumb-http object-fetch, usage-rejection, and invalid-input matrix coverage
into `docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta
is `+0` matrix rows, `+8` complete documented option pairs, `+0` complete
command matrices, `+0` verified rows, `+0` invalid-input rows, and `-8`
remaining checklist rows. Rust behavior changes are not expected. Actual
delta matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented `mergetool` surface. The
selected rows do not add new behavior evidence; they promote eight exact
stock-Git `mergetool` option families with existing local configured-tool,
prompt-toggle, gui-toggle, path-selection, and invalid-input matrix coverage
into `docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta
is `+0` matrix rows, `+8` complete documented option pairs, `+0` complete
command matrices, `+0` verified rows, `+0` invalid-input rows, and `-8`
remaining checklist rows. Rust behavior changes are not expected. Actual
delta matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented `fetch-pack` surface. The
selected rows do not add new behavior evidence; they promote eleven exact
stock-Git `fetch-pack` option families with existing local repository, depth,
tag, keep, quiet, stdin, thin-pack, and invalid-input matrix coverage into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta is
`+0` matrix rows, `+11` complete documented option pairs, `+0` complete
command matrices, `+0` verified rows, `+0` invalid-input rows, and `-11`
remaining checklist rows. Rust behavior changes are not expected. Actual
delta matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented `daemon` surface. The selected
rows do not add new behavior evidence; they promote thirteen exact stock-Git
`daemon` option families with existing local git-daemon listen, export,
timeout, inetd, and path-serving matrix coverage into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta is
`+0` matrix rows, `+13` complete documented option pairs, `+0` complete
command matrices, `+0` verified rows, `+0` invalid-input rows, and `-13`
remaining checklist rows. Rust behavior changes are not expected. Actual
delta matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented `filter-branch` surface. The
selected rows do not add new behavior evidence; they promote fourteen exact
stock-Git `filter-branch` option families with existing local message, tree,
index, env, parent, setup, tag rename, state-branch, temp-dir, backup-ref,
and invalid-input matrix coverage into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta is
`+0` matrix rows, `+14` complete documented option pairs, `+0` complete
command matrices, `+0` verified rows, `+0` invalid-input rows, and `-14`
remaining checklist rows. Rust behavior changes are not expected. Actual
delta matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented helper-free `show` surface. The
selected rows do not add new behavior evidence; they promote three exact
stock-Git `show` option families with existing local root-commit,
object-display, pretty-format, patch/stat, merge-display, and pathspec
matrix coverage into `docs/cli/census/reviewed_complete_doc_option_pairs.tsv`.
Expected delta is `+0` matrix rows, `+3` complete documented option pairs,
`+0` complete command matrices, `+0` verified rows, `+0` invalid-input rows,
and `-3` remaining checklist rows. Rust behavior changes are not expected.
Actual delta matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented helper-free `worktree`
surface. The selected rows do not add new behavior evidence; they promote
eight exact stock-Git `worktree` option families with existing local add,
move, lock, prune, repair, config-scope, branch-option, and invalid-input
matrix coverage into `docs/cli/census/reviewed_complete_doc_option_pairs.tsv`.
Expected delta is `+0` matrix rows, `+8` complete documented option pairs,
`+0` complete command matrices, `+0` verified rows, `+0` invalid-input rows,
and `-8` remaining checklist rows. Rust behavior changes are not expected.
Actual delta matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented helper-free `cat-file`
surface. The selected rows do not add new behavior evidence; they promote
sixteen exact stock-Git `cat-file` option families with existing local batch,
batch-check, batch-command, typed-object, path, filter, textconv, promisor,
and invalid-input matrix coverage into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta is
`+0` matrix rows, `+16` complete documented option pairs, `+0` complete
command matrices, `+0` verified rows, `+0` invalid-input rows, and `-16`
remaining checklist rows. Rust behavior changes are not expected. Actual
delta matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented helper-free `blame` surface.
The selected rows do not add new behavior evidence; they promote twenty-eight
exact stock-Git `blame` option families with existing local annotation,
date-mode, line-range, regex-range, progress, and invalid-input matrix
coverage into `docs/cli/census/reviewed_complete_doc_option_pairs.tsv`.
Expected delta is `+0` matrix rows, `+28` complete documented option pairs,
`+0` complete command matrices, `+0` verified rows, `+0` invalid-input rows,
and `-28` remaining checklist rows. Rust behavior changes are not expected.
Actual delta matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented helper-free `rev-parse`
surface. The selected rows do not add new behavior evidence; they promote
twenty-five exact stock-Git `rev-parse` option families with existing local
discovery, path-format, revision, date-filter, symbolic-ref, and
replacement-shim matrix coverage into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta is
`+0` matrix rows, `+25` complete documented option pairs, `+0` complete
command matrices, `+0` verified rows, `+0` invalid-input rows, and `-25`
remaining checklist rows. Rust behavior changes are not expected. Actual
delta matched.

As of 2026-06-26 the next batch is a helper-free `rev-parse` ref-selection
and discovery family expansion. The selected rows add exact stock-Git local
evidence for `--all`, `--branches[=<pattern>]`, `--tags[=<pattern>]`,
`--remotes[=<pattern>]`, `--glob=<pattern>`, next-selector `--exclude`,
`--local-env-vars`, and `--resolve-git-dir <path>`, including the outside
repository invalid `--resolve-git-dir .` failure. Expected delta is `+11`
matrix rows, `+0` complete documented option pairs, `+0` complete command
matrices, `+10` verified rows, `+1` invalid-input row, and `+8`
represented documented option pairs. Rust behavior changes are expected in
`rev-parse` raw-argument ordered handling and lazy repository loading.
Actual delta matched.

As of 2026-06-26 the next batch is a helper-free `rev-parse` parse/filter and
quoting family expansion. The selected rows add exact stock-Git local
evidence for `--flags`, `--no-flags`, `--revs-only`, `--no-revs`,
`--default`, `--prefix <arg>`, `--not`, `--symbolic`, `--sq`, and
outside-repository `--sq-quote`, while also correcting the old
`--show-ref-format` invalid `extensions.refStorage` stderr shape so the
focused `rev_parse_` test target becomes fully green again. Expected delta is
`+11` matrix rows, `+0` complete documented option pairs, `+0` complete
command matrices, `+11` verified rows, `+0` invalid-input rows, and `+10`
represented documented option pairs. Rust behavior changes are expected in
`rev-parse` raw-argument classification, shell quoting, default/prefix
handling, and direct local config reads for ref/object format reporting.
Actual delta matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented helper-free `diff-files`
surface. The selected rows do not add new behavior evidence; they promote
seventy-nine exact stock-Git `diff-files` option families with existing local
matrix coverage into `docs/cli/census/reviewed_complete_doc_option_pairs.tsv`.
Expected delta is `+0` matrix rows, `+79` complete documented option pairs,
`+0` complete command matrices, `+0` verified rows, `+0` invalid-input rows,
and `-79` remaining checklist rows. Rust behavior changes are not expected.
Actual delta matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented helper-free `diff-tree`
surface. The selected rows do not add new behavior evidence; they promote
eighty-five exact stock-Git `diff-tree` option families with existing local
matrix coverage into `docs/cli/census/reviewed_complete_doc_option_pairs.tsv`.
Expected delta is `+0` matrix rows, `+85` complete documented option pairs,
`+0` complete command matrices, `+0` verified rows, `+0` invalid-input rows,
and `-85` remaining checklist rows. Rust behavior changes are not expected.
Actual delta matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented helper-free `diff-index`
surface. The selected rows do not add new behavior evidence; they promote
seventy-eight exact stock-Git `diff-index` option families with existing local
matrix coverage into `docs/cli/census/reviewed_complete_doc_option_pairs.tsv`.
Expected delta is `+0` matrix rows, `+78` complete documented option pairs,
`+0` complete command matrices, `+0` verified rows, `+0` invalid-input rows,
and `-78` remaining checklist rows. Rust behavior changes are not expected.
Actual delta matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented helper-free `diff` surface. The
selected rows do not add new behavior evidence; they promote seventy-three
exact stock-Git `diff` option families with existing local matrix coverage
into `docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta
is `+0` matrix rows, `+73` complete documented option pairs, `+0` complete
command matrices, `+0` verified rows, `+0` invalid-input rows, and `-73`
remaining checklist rows. Rust behavior changes are not expected. Actual delta
matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented `fetch` surface. The selected
rows do not add new behavior evidence; they promote thirty-nine exact stock-Git
`fetch` option families with existing local, file URL, smart HTTP, SSH,
git-daemon, prune/tag, shallow-history, and replacement-shim matrix coverage
into `docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta
is `+0` matrix rows, `+39` complete documented option pairs, `+0` complete
command matrices, `+0` verified rows, `+0` invalid-input rows, and `-39`
remaining checklist rows. Rust behavior changes are not expected. Actual delta
matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented helper-free local `commit`
surface. The selected rows do not add new behavior evidence; they promote
thirty-seven exact stock-Git `commit` option families with existing local
message, editor, hook, cleanup, trailer, pathspec, amend, and alias matrix
coverage into `docs/cli/census/reviewed_complete_doc_option_pairs.tsv`.
Expected delta is `+0` matrix rows, `+37` complete documented option pairs,
`+0` complete command matrices, `+0` verified rows, `+0` invalid-input rows,
and `-37` remaining checklist rows. Rust behavior changes are not expected.
Actual delta matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented helper-free local `add` surface.
The selected rows do not add new behavior evidence; they promote twenty-seven
exact stock-Git `add` option families with existing local pathspec,
index-mutation, ignore, mode-bit, filter, and parser matrix coverage into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta is
`+0` matrix rows, `+27` complete documented option pairs, `+0` complete
command matrices, `+0` verified rows, `+0` invalid-input rows, and `-27`
remaining checklist rows. Rust behavior changes are not expected. Actual delta
matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented helper-free local `tag` surface.
The selected rows do not add new behavior evidence; they promote twenty-one
exact stock-Git `tag` option families with existing local create, list,
filter, sort, format, verify, and delete matrix coverage into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta is
`+0` matrix rows, `+21` complete documented option pairs, `+0` complete
command matrices, `+0` verified rows, `+0` invalid-input rows, and `-21`
remaining checklist rows. Rust behavior changes are not expected. Actual delta
matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented helper-free local `notes`
surface. The selected rows do not add new behavior evidence; they promote
twenty-one exact stock-Git `notes` option families with existing local add,
append, edit, copy, merge, prune, remove, and ref-selection matrix coverage
into `docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta
is `+0` matrix rows, `+21` complete documented option pairs, `+0` complete
command matrices, `+0` verified rows, `+0` invalid-input rows, and `-21`
remaining checklist rows. Rust behavior changes are not expected. Actual delta
matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the represented helper-free local `config`
surface. The selected rows do not add new behavior evidence; they promote
eighteen exact stock-Git `config` option families with existing local get,
unset, replace-all, type, show-origin, and fixed-value parser matrix coverage
into `docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta
is `+0` matrix rows, `+18` complete documented option pairs, `+0` complete
command matrices, `+0` verified rows, `+0` invalid-input rows, and `-18`
remaining checklist rows. Rust behavior changes are not expected. Actual delta
matched.

As of 2026-06-25 the next batch is a helper-free `ls-files`
`--no-empty-directory` closure plus reviewed-complete command promotion. The
selected row adds the final represented documented `ls-files` option family to
the matrix, closes the stock-compatible empty-directory negation and
last-option-wins ordering against `--empty-directory`, and then promotes both
`--no-empty-directory` and the full `ls-files` command into the durable
reviewed-complete census lists. Expected delta is `+1` matrix row, `+1`
verified row, `+1` complete documented option pair, `+1` complete command
matrix, `+0` invalid-input rows, and `-1` remaining checklist row. Rust
behavior changes are expected because `ls-files` must add the missing
`--no-empty-directory` schema tail and stop reporting empty untracked
directories when that negation is active. Actual delta matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option tail promotion for the remaining represented helper-free `ls-files`
surface. The selected rows do not add new behavior evidence; they promote
eleven additional exact stock-Git `ls-files` option families into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`, closing every
currently represented documented option except the still-unrepresented
`--no-empty-directory` seed. Expected delta is `+0` matrix rows, `+11`
complete documented option pairs, `+0` complete command matrices, `+0`
verified rows, `+0` invalid-input rows, and `-11` remaining checklist rows.
Rust behavior changes are not expected. Actual delta matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for the supported `ls-files` selector, exclude, and
output family. The selected rows do not add new behavior evidence; they
promote twenty-seven existing exact stock-Git `ls-files` option families into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`, covering the dense
helper-free selector and recurse-submodules surface without changing Rust
behavior. Expected delta is `+0` matrix rows, `+27` complete documented option
pairs, `+0` complete command matrices, `+0` verified rows, `+0` invalid-input
rows, and `-27` remaining checklist rows. Rust behavior changes are not
expected. Actual delta matched.

As of 2026-06-25 the next batch is a zero-code reviewed-complete documented
option promotion cluster for `bugreport`, `fmt-merge-msg`, and `describe`.
The selected rows do not add new behavior evidence; they promote the existing
exact stock-Git evidence for six `bugreport` option families, seven
`fmt-merge-msg` option families, and nine `describe` option families into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta is
`+0` matrix rows, `+22` complete documented option pairs, `+0` complete
command matrices, `+0` verified rows, `+0` invalid-input rows, and `-22`
remaining checklist rows. Rust behavior changes are not expected. Actual delta
matched.

As of 2026-06-25 the next batch is a zero-code `read-tree`
reviewed-complete command promotion. Its behavior rows, exact stock-Git
evidence, and all `17/17` reviewed-complete documented option pairs already
exist in the matrix, focused compat tests, and generated census artifacts; this
slice only promotes `read-tree` into
`docs/cli/census/reviewed_complete_command_matrices.tsv`. Expected delta is
`+0` matrix rows, `+1` complete command matrix, `+0` complete documented option
pairs, `+0` verified rows, `+0` invalid-input rows, and `+0` remaining
checklist rows. Rust behavior changes are not expected. Actual delta matched.

As of 2026-06-25 the next batch is a helper-free `repack --filter`
value-family closure plus reviewed-complete command promotion. The selected
rows add exact stock-Git evidence for `blob:limit` values, all documented
`object:type` forms, documented `tree:<depth>` values, valid `sparse:oid`,
percent-encoded `combine`, invalid tree-depth rejection, and the dropped
`sparse:path` diagnostic on the helper-free local lane, then promote
`--filter` and the full `repack` command into the durable reviewed-complete
census lists. Expected delta is `+12` matrix rows, `+12` verified rows, `+1`
complete documented option pair, `+1` complete command matrix, `+0`
invalid-input rows, and `-1` remaining checklist row. Rust behavior changes
are expected because `repack` must accept valid sparse oid filters and emit
the stock dropped sparse-path diagnostic. Actual delta matched.

As of 2026-06-25 the next batch is a zero-code `repack`
reviewed-complete documented-option closure. The selected rows do not add new
behavior evidence; they promote the existing exact stock-Git evidence for
`--cruft`, `--cruft-expiration`, `--expire-to`, `--filter-to`,
`--geometric`, `--max-cruft-size`, `--unpack-unreachable`, and `-g` into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta is
`+0` matrix rows, `+8` complete documented option pairs, `+0` complete command
matrices, `+0` verified rows, `+0` invalid-input rows, and `-8` remaining
checklist rows. Rust behavior changes are not expected. Actual delta matched.

As of 2026-06-25 the next batch is a helper-free `repack`
`--filter` / `--filter-to` schema-tail closure. The selected rows add exact
stock-Git evidence for `--filter=blob:none`, `--filter=combine:blob:none+tree:1`,
repeated last-one-wins filter ordering in both directions, `--filter-to`
root-side pack artifact creation with repeated rightmost prefix selection, the
stock invalid `--filter=bogus` diagnostic, and the stock fatal guard for
`--filter-to` without `--filter`. Expected delta is `+8` matrix rows, `+8`
verified rows, `+2` represented documented option pairs, `+0` complete
documented option pairs, `+0` complete command matrices, and `0` remaining
checklist rows. Rust behavior changes are expected because `repack` must add
the missing `--filter` / `--filter-to` schema tail, validate stock-like filter
specs for the covered helper-free lane, and write stock-shaped root-side
filtered pack artifacts. Actual delta matched.

As of 2026-06-25 the next batch is a helper-free `repack` cruft-family
repeat/order closure inside the existing schema surface. The selected rows add
exact stock-Git evidence for repeated `--cruft`, last-one-wins
`--cruft-expiration` (`now` versus `never`), repeated `--expire-to` prefix
selection, ignored `--expire-to` when `--cruft-expiration=never` keeps the
cruft pack in the object store, and repeated `--max-cruft-size` with the
stock 1 MiB minimum-size warning. Expected delta is `+6` matrix rows, `+6`
verified rows, `+0` invalid-input rows, `+0` complete documented option
pairs, `+0` complete command matrices, and `-6` remaining checklist rows.
Rust behavior changes are expected because repeated cruft-family options must
parse like stock Git. Actual delta matched.

As of 2026-06-25 the next batch is a helper-free `repack` cruft-expiration
value-family closure inside the existing schema surface. The selected rows add
exact stock-Git evidence for `all`, `tomorrow`, `yesterday`, `1970-01-01`,
`2 weeks ago`, and `bogus` on the helper-free `--cruft -d -q` lane, while the
product change teaches `repack` to classify future, past absolute, past
relative, and now-like unparseable expiration values like stock Git. Expected
delta is `+6` matrix rows, `+6` verified rows, `+0` invalid-input rows, `+0`
complete documented option pairs, `+0` complete command matrices, and `0`
remaining checklist rows. Rust behavior changes are expected because the
cruft-expiration parser must match stock Git's date-like cutoff behavior.
Actual delta matched.

As of 2026-06-25 the next batch is a helper-free `repack` geometric
schema-tail closure. The selected rows add exact stock-Git evidence for `-g`
and `--geometric` with joined and separate value forms, repeated short and
long forms with last-one-wins behavior, mixed short/long ordering, and the
stock invalid-value diagnostic for `--geometric=bogus`. Expected delta is `+8`
matrix rows, `+8` verified rows, `+2` represented documented option pairs,
`+0` complete documented option pairs, `+0` complete command matrices, and
`0` remaining checklist rows. Rust behavior changes are expected because
`repack` must add the missing `-g` / `--geometric` schema tail and stock-like
value parsing. Actual delta matched.

As of 2026-06-25 the next batch is a helper-free `repack`
`--unpack-unreachable` schema-tail closure. The selected rows add exact
stock-Git evidence for now-like, future, past absolute, past relative, bogus,
and repeated last-one-wins `--unpack-unreachable=<when>` values on the
covered `-a -d -q` and `-A -d -q` lanes, while the product change teaches
`repack` when to keep the stock prune path and when to switch to stock-like
loosened unreachable behavior. Expected delta is `+8` matrix rows, `+8`
verified rows, `+1` represented documented option pair, `+0` complete
documented option pairs, `+0` complete command matrices, and `0` remaining
checklist rows. Rust behavior changes are expected because `repack` must add
the missing `--unpack-unreachable` schema tail and cutoff-based behavior.
Actual delta matched.

As of 2026-06-25 the next batch is a zero-row reviewed-complete closure
cluster for `branch`, `status`, `rm`, `fsck`, `maintenance`, and
`merge-tree`. Their behavior rows, stock evidence, and documented option
coverage already exist in the matrices, focused compat tests, and schema/oracle
smokes; this slice only promotes them into
`docs/cli/census/reviewed_complete_command_matrices.tsv` and
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta is
`+0` matrix rows, `+6` complete command matrices, `+109` complete documented
option pairs, `0` verified rows, `0` invalid-input rows, and `-109`
remaining checklist rows. Rust behavior changes are not expected. Actual delta
matched.

As of 2026-06-25 the next batch is a zero-row reviewed-complete closure for
`bisect`. Its behavior rows, stock evidence, and both documented option
surfaces already exist in the matrix and focused sequencer and invalid-input
tests; this slice only promotes the command plus `--first-parent` and
`--no-checkout` into the durable reviewed-complete census source lists.
Expected delta is `+0` matrix rows, `+1` complete command matrix, `+2`
complete documented option pairs, `0` verified rows, `0` invalid-input rows,
and `-2` remaining checklist rows. Rust behavior changes are not expected.
Actual delta matched.

As of 2026-06-25 the next batch is a zero-row command-only reviewed-complete
closure cluster for `clone`, `stage`, `push`, `init`, `sparse-checkout`, and
`shell`. None of these commands currently contribute documented option seed
rows in the tightened extractor, and all six already have zero remaining
census rows plus `100%` classified written-row coverage. This slice only
promotes them into `docs/cli/census/reviewed_complete_command_matrices.tsv`.
Expected delta is `+0` matrix rows, `+6` complete command matrices, `+0`
complete documented option pairs, `0` verified rows, `0` invalid-input rows,
and `0` remaining checklist rows. Rust behavior changes are not expected.
Actual delta matched.

As of 2026-06-25 the next batch is a helper-free `archive`
documented-option family closure. The selected rows add exact stock-Git
evidence for filename-based format inference through `-o` and `--output`,
repeated `--prefix` rightmost-wins behavior with per-add-file prefix capture,
repeated `--add-file` and `--add-virtual-file` ordering, and
`--worktree-attributes` export-ignore parity, then promote the full
documented `archive` option family into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta is
`+7` matrix rows, `+7` verified rows, `+14` complete documented option pairs,
`0` complete command matrices, `0` invalid-input rows and `-14` remaining
checklist rows. Rust behavior changes are expected because the archive parser
must preserve documented option ordering. Actual delta matched.

As of 2026-06-25 the next batch is a helper-free `clean` documented-family
and reviewed-complete closure. The selected rows add exact stock-Git evidence
for the remaining documented alias-only surfaces `--force`, repeated
`--force`, and `-i`, then promote `clean` into
`docs/cli/census/reviewed_complete_command_matrices.tsv` and
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta is
`+3` matrix rows, `+3` verified rows, `+1` complete command matrix, `+13`
complete documented option pairs, `0` invalid-input rows and `-13`
remaining checklist rows. Rust behavior changes are not expected. Actual
delta matched.

As of 2026-06-25 the next batch is a zero-row reviewed-complete closure
cluster for helper-free commands `check-ignore` and `column`. Their behavior
rows, stock evidence, and documented option coverage already exist in the
matrices, focused tests, and schema/oracle smokes; this slice only promotes
them into `docs/cli/census/reviewed_complete_command_matrices.tsv` and
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta is
`+0` matrix rows, `+2` complete command matrices, `+16` complete documented
option pairs, `0` verified rows, `0` invalid-input rows, and `-16` remaining
checklist rows. Rust behavior changes are not expected. Actual delta matched.

As of 2026-06-25 the next batch is a helper-free `checkout-index`
documented-option family closure. The selected rows add exact stock-Git
evidence for repeated `-a`/`-f`/`-q`, `--all --quiet --force`,
`--stdin --prefix=out/`, and the stock-invalid `--stdin` plus explicit-path
and `--all --stdin` mixes, while the product fixes make repeated documented
boolean flags parse like stock Git and align the invalid mixing diagnostics.
Expected delta is `+7` matrix rows, `+7` verified rows, `+8` complete
documented option pairs, `0` invalid-input rows and `-8` remaining checklist
rows. Rust behavior changes are expected. Actual delta matched.

As of 2026-06-25 the next batch is a helper-free `read-tree`
documented-option family closure. The selected rows add exact stock-Git
evidence for `--prefix` separate and normalized value forms, repeated `-m`,
and `--empty` contradiction failures with treeish and prefix combinations,
while the product fix makes `--prefix` retain existing index entries and makes
repeated `-m` parse like stock Git. Expected delta is `+5` matrix rows, `+5`
verified rows, `+3` complete documented option pairs, `0` invalid-input rows
and `-3` remaining checklist rows. Rust behavior changes are expected.
Actual delta matched.

As of 2026-06-25 the next batch is a helper-free `read-tree` schema-tail
closure. The selected rows add exact stock-Git evidence for the remaining
parser surface `-u`, `-v`, `--trivial`, and `--aggressive`, including the
stock fatal guards for bare `-u` and mixed `-i -u`, plus worktree-update
parity for `-m -u`, `--reset -u`, and `--prefix=import/ -u`. Expected delta
is `+10` matrix rows, `+8` verified rows, `+2` invalid-input rows, `+4`
represented documented option pairs, `0` complete documented option pairs,
`0` complete command matrices, and `0` remaining checklist rows. Rust
behavior changes are expected because `read-tree` now needs stock-compatible
`-u` guards and direct worktree materialization for the single-tree helper-free
paths. Actual delta matched.

As of 2026-06-25 the next batch is a helper-free `unpack-objects`
documented-family plus reviewed-complete closure. The selected rows add the
remaining documented `--max-input-size` parser surface, align repeated
documented booleans `-n`, `-q`, `-r`, and `--strict` with stock Git, and add
exact stock-Git evidence for repeated short/long forms, large-enough and
too-small size limits, repeated `--max-input-size` last-one-wins behavior, and
leading-digit-only parsing for malformed suffix values like `1m`. Expected
delta is `+11` matrix rows, `+8` verified rows, `+3` invalid-input rows, `+1`
complete command matrix, `+5` complete documented option pairs, and `-5`
remaining checklist rows. Rust behavior changes are expected because
`unpack-objects` now needs repeatable documented flag parsing and
stock-compatible max-input-size enforcement on stdin pack streams. Actual delta
matched.

As of 2026-06-25 the next batch is a helper-free `gc` documented-option family
closure. The selected rows add exact stock-Git evidence for long `--quiet`,
bare `--prune` default-missing-value behavior, and `--auto --quiet`, then
promote the supported `gc` documented option pairs into the durable reviewed
complete option list. Expected delta is `+3` matrix rows, `+3` verified rows,
`+5` complete documented option pairs, `0` invalid-input rows and `-5`
remaining checklist rows. Rust behavior changes are not expected. Actual delta
matched.

As of 2026-06-25 the next batch is a helper-free `gc` schema-tail plus
reviewed-complete closure. The selected rows add the remaining documented
parser surface `--[no-]detach`, `--[no-]cruft`, `--max-cruft-size`,
`--force`, and `--keep-largest-pack`, while the product changes add
stock-compatible max-cruft-size parsing with the 1 MiB minimum-size warning,
warning suppression under `--no-cruft`, last-one-wins repeated value
selection, and stock invalid-value rejection. Expected delta is `+12` matrix
rows, `+12` verified rows, `+7` complete documented option pairs, `+1`
complete command matrix, `0` invalid-input rows and `-7` remaining checklist
rows. Rust behavior changes are expected. Actual delta matched.

As of 2026-06-25 the next batch is a helper-free `repack` accepted-schema-tail
closure. The selected rows add the documented parser surface
`--window-memory`, `--max-pack-size`, `--delta-islands` / `-i`,
`--pack-kept-objects`, and `--keep-unreachable` / `-k`, while the product
changes add stock-compatible size parsing for the accepted size-hint options,
the 1 MiB minimum-size warning and invalid-value rejection for max-pack-size,
and helper-free `keep-unreachable` packing for dangling loose objects on the
`-adq` lane. Expected delta is `+11` matrix rows, `+11` verified rows, `+7`
complete documented option pairs, `0` complete command matrices, `0`
invalid-input rows and `-7` remaining checklist rows. Rust behavior changes
are expected. Actual delta matched.

As of 2026-06-25 the next batch is a helper-free `read-tree` expansion-evidence
closure inside the existing schema surface. The selected rows add repeated
quiet and verbose forms, quiet compositions for dry-run/trivial/aggressive and
no-sparse-checkout, reset plus alternate index output, index-only plus reset
and prefix-plus-index-output, and both recurse-submodule override orders on
repositories without submodules. Expected delta is `+12` matrix rows, `+12`
verified rows, `+0` invalid-input rows, `+0` represented doc-option pairs,
`+0` complete documented option pairs, `+0` complete command matrices, and
`+0` remaining checklist rows. Rust behavior changes are not expected. Actual
delta matched.

As of 2026-06-25 the `git_foreign_scm_compat.rs` `p4 submit` follow-up is a
zero-row closure slice. The selected row already existed in
`docs/cli/matrices/p4_v2_47.tsv`; the work closes it by matching stock Git's
fake-Perforce submit sync/apply/import/rebase stdout flow, exit status `0`,
empty stderr, opened edit/add/delete operations and remote-ref advance.
Expected delta is `+0` matrix rows, `+1` verified row, `-1` open row, `0`
invalid-input rows and `-1` remaining checklist row. Actual delta matched.

As of 2026-06-25 the next batch is an `interpret-trailers`
parser-plus-evidence closure plus reviewed-complete promotion. The selected
rows add the documented `--no-where`, `--no-if-exists`, and
`--no-if-missing` parser surfaces, close their exact stock-Git reset-ordering
rows, and then promote `interpret-trailers` into the durable reviewed-complete
command and doc-option census artifacts. Expected delta is `+3` matrix rows,
`+3` verified rows, `+1` complete command matrix, `+14` complete documented
option pairs, `0` invalid-input rows and `-14` remaining checklist rows. Rust
behavior changes are expected. Actual delta matched.

As of 2026-06-25 the next batch is a `repack` documented-option family
closure. The selected rows add repeated quiet aliases, separate/equal
`--threads`, order-sensitive `--window` / `--depth`, long and repeated
`--write-midx` / `-m`, long and repeated `--write-bitmap-index` / `-b`, and
repeated `--keep-pack` values, then promote the now-closed `repack`
doc-option pairs into the durable reviewed-complete option list. Expected
delta is `+13` matrix rows, `+13` verified rows, `+10` complete documented
option pairs, `0` invalid-input rows and `-10` remaining checklist rows. Rust
behavior changes are expected because repeated documented boolean `repack`
flags must parse like stock Git. Actual delta matched.

As of 2026-06-25 the next batch is a second `repack` helper-free short-option
closure. The selected rows add repeated and reordered short-option forms for
`-a`, `-A`, `-d`, `-f`, `-F`, `-l`, and `-n`, then promote those seven
short-option pairs into the durable reviewed-complete option list. Expected
delta is `+8` matrix rows, `+8` verified rows, `+7` complete documented
option pairs, `0` invalid-input rows and `-7` remaining checklist rows. No
new command surfaces are expected; the work stays inside the already supported
`repack` synopsis tail. Actual delta matched.

As of 2026-06-25 the next batch is a zero-row reviewed-complete closure
cluster for helper-free commands `diagnose`, `imap-send`, `mailsplit`, `mv`
and `quiltimport`. Their behavior rows, stock evidence and documented option
coverage already exist in matrices/tests/smokes; this slice only promotes them
into `docs/cli/census/reviewed_complete_command_matrices.tsv` and
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta is
`+0` matrix rows, `+5` complete command matrices, `+30` complete documented
option pairs, `0` verified rows, `0` invalid-input rows and `-30` remaining
checklist rows. Rust behavior changes are not expected. Actual delta matched.

As of 2026-06-25 the next batch is another zero-row reviewed-complete closure
cluster for helper-free commands `replace` and `merge-file`. Their matrices,
stock evidence and documented option coverage already exist in focused tests
and schema smoke probes; this slice only promotes them into the reviewed
complete command and doc-option census artifacts. Expected delta is `+0`
matrix rows, `+2` complete command matrices, `+21` complete documented option
pairs, `0` verified rows, `0` invalid-input rows and `-21` remaining checklist
rows. Rust behavior changes are not expected. Actual delta matched.

As of 2026-06-25 the next `git_maintenance_compat.rs` follow-up is a small
local parser-plus-evidence `prune` batch. The selected rows add exact
documented `--dry-run`, `--verbose`, `-v`, `--progress` and `--no-progress`
parity for `--expire=now <object>` loose-object pruning behavior. Expected
delta is `+5` matrix rows, `+5` verified rows, `0` invalid-input rows and `0`
remaining checklist rows. Rust behavior changes are not expected. Actual delta
matched.

As of 2026-06-25 the next `git_ref_resolution_compat.rs` follow-up is a
one-row local parser-plus-evidence `name-rev` batch. The selected row adds
exact documented `--no-undefined` parity for an unreferenced dangling commit,
including stock Git's partial stdout prefix and fatal stderr. Expected delta
is `+1` matrix row, `+1` verified row, `0` invalid-input rows and `-1`
remaining checklist row. Actual delta matched except the current census total
for `remaining_to_fix_or_verify_rows` stayed flat because the regenerated
backlog still contains broader doc-option expansion rows for `name-rev`.

As of 2026-06-25 the next `git_object_plumbing_compat.rs` follow-up is a
small `hash-object` parser-plus-evidence batch. The selected rows add exact
documented `--path=<file> --stdin`, `--stdin-paths`, `--stdin-paths
--no-filters`, `--literally --stdin`, and the exact invalid combinations
`--no-filters --stdin --path=<file>`, `--stdin-paths --path=<file>`, and
`--stdin --stdin-paths`. Expected delta is `+7` matrix rows, `+4` verified
rows, `+3` invalid-input rows and `-4` remaining checklist rows. Actual delta
matched except the current census total for `remaining_to_fix_or_verify_rows`
stayed flat because the regenerated backlog still contains broader
documented-option expansion rows for `hash-object`.

As of 2026-06-25 the next `git_mail_tools_compat.rs` follow-up is a one-row
`mailinfo` parser-plus-evidence batch. The selected row adds exact documented
`--no-scissors` parity for the common patch-mail input shape already covered by
the focused stock oracle. Expected delta is `+1` matrix row, `+1` verified
row, `0` invalid-input rows and `-1` remaining checklist row. Actual delta
matched except the current census total for `remaining_to_fix_or_verify_rows`
stayed flat because the regenerated backlog still contains broader
documented-option expansion rows for `mailinfo`.

As of 2026-06-25 the `git_text_tools_compat.rs` follow-up is a local
parser-plus-evidence slice for `git column`. The selected rows add the
documented `--command`, `--indent` and `--nl` option surfaces, exact
`--raw-mode=0` parity, and the documented no-toggle plus empty-value forms
`--no-command`, `--no-mode`, bare `--mode`, `--mode=`, `--no-width`,
`--no-indent`, `--no-nl`, `--no-padding`, `--width=` and `--padding=`.
Expected delta is `+14` matrix rows, `+12` verified rows, `+2` invalid-input
rows and `0` oracle `missing_or_unclassified` functions. Actual delta matched.

As of 2026-06-25 the same `git_text_tools_compat.rs` slice was extended again
for focused `raw-mode` parity. The selected rows add exact `--raw-mode=1`,
`--raw-mode=17`, invalid `--raw-mode=bogus`, `--raw-mode` / `--mode`
last-one-wins order, and raw-mode-plain interactions that ignore explicit
`--padding`, `--indent` and `--nl`. Expected delta is `+10` matrix rows, `+9`
verified rows, `+1` invalid-input row and `0` oracle
`missing_or_unclassified` functions. Actual delta matched.

As of 2026-06-25 the same `git_text_tools_compat.rs` slice was extended once
more for focused `--command` parity. The selected rows add exact
`column.status=column,dense`, `column,nodense` and `row,dense` command-mode
config behavior plus invalid first-argument failures for `--command status`,
`--width=20 --command=status` and `--command=status --no-command`. Expected
delta is `+6` matrix rows, `+3` verified rows, `+3` invalid-input rows and
`0` oracle `missing_or_unclassified` functions. Actual delta matched.

As of 2026-06-25 the same `git_text_tools_compat.rs` slice was extended yet
again for the next exact `--command` evidence group. The selected rows add
repository `column.status=dense`, `nodense` and `row,nodense` behavior through
`git column --command=status`, plus explicit empty `--command=` combined with
`--mode=column --width=20`. Expected delta is `+4` matrix rows, `+4`
verified rows, `0` invalid-input rows and `0` oracle
`missing_or_unclassified` functions. Actual delta matched.

As of 2026-06-25 the same `git_text_tools_compat.rs` slice was extended one
more time to close the remaining local `--command` evidence subgroup. The
selected rows add post-command `--mode` and `--raw-mode` overrides, explicit
empty `--command=` with width/raw-mode/no-command forms, and explicit
`--no-command` with mode/raw-mode forms. Expected delta is `+16` matrix rows,
`+15` verified rows, `+1` invalid-input row and `0` oracle
`missing_or_unclassified` functions. Actual delta matched.

As of 2026-06-25 the `git_object_plumbing_compat.rs` batch is a census-first
evidence-import slice for already-implemented object-plumbing surfaces. The
selected rows make exact promisor-backed `cat-file` hydration, stage-object
revision and diff forms, and attribute-driven `add` / `checkout` behavior
explicit in the matrices. Expected delta is `+11` matrix rows, `+11` verified
rows, `0` invalid-input rows and `-4` oracle `missing_or_unclassified`
functions. Rust behavior changes are not expected. Actual delta matched.

As of 2026-06-25 the `git_clone_compat.rs` follow-up is a zero-row
extension-evidence slice. The selected row shape already exists in
`docs/cli/zmin_extensions_inventory.md` for Zmin-only `clone --instant`; the
work only adds the exact local-repository fetch/pull canonical-operations test
to that extension row. Expected delta is `0` matrix rows, `0` verified rows,
`0` invalid-input rows and `-1` oracle `missing_or_unclassified` function.
Rust behavior changes are not expected. Actual delta matched.

As of 2026-06-25 the `git_cli_failure_compat.rs` follow-up is a small
invalid-input evidence-import slice. The selected rows make exact `commit`
failure shapes explicit for conflicting `--squash` plus `--fixup`, and an
invalid `--cleanup` mode. Expected delta is `+2` matrix rows, `0` verified
rows, `+2` invalid-input rows and `-1` oracle `missing_or_unclassified`
function. Rust behavior changes are not expected. Actual delta matched.

As of 2026-06-25 the `compatibility_command.rs` follow-up is a zero-row
deferral-classification slice. The selected test validates the generated
compatibility report acceptance gate, not an exact Git behavior row, so it
belongs in `docs/cli/oracle_test_deferrals.md` rather than in any Git
compatibility matrix. Expected delta is `0` matrix rows, `0` verified rows,
`0` invalid-input rows and `-1` oracle `missing_or_unclassified` function.
Rust behavior changes are not expected. Actual delta matched.

As of 2026-06-25 the `git_fast_import_export_compat.rs` batch is a behavior-row
evidence-import slice for existing import/export interoperability surfaces. The
selected rows make exact fast-export stream reimport, stock fast-export stream
ingest, bulk commit helper shape, `--date-format=now` with missing author, and
adjacent commit-record parsing explicit in the matrices. Expected delta is `+5`
matrix rows, `+5` verified rows, `0` invalid-input rows and `-5` oracle
`missing_or_unclassified` functions. Rust behavior changes are not expected.
Actual delta matched.

As of 2026-06-25 the `git_global_cli_compat.rs` batch is a root/global-CLI
evidence-import slice without Rust behavior changes. The selected rows make
exact empty `git` invocation usage, repository-configured alias expansion,
`init` under `GIT_DIR` plus `GIT_WORK_TREE`, root-dotfile checkout dispatch
and global `-C` status routing explicit in the matrices. Expected delta is
`+5` matrix rows, `+5` verified rows, `0` invalid-input rows and `-5` oracle
`missing_or_unclassified` functions. Rust behavior changes are not expected.
Actual delta matched.

As of 2026-06-25 the `git_index_mutation_compat.rs` batch is an
evidence-import-plus-fix slice. The selected rows make symlink-parent
replacement through nested `add`, case-insensitive absolute-path add on
supported filesystems, symlink `--cacheinfo`, `--replace` directory/file swap,
child-path `--cacheinfo` rejection under an indexed file, unmerged
`--index-info` stage ingestion, and missing-object `--index-info` rows
explicit in the matrices. The batch also fixes `update-index --add --replace`
so parent file entries are removed before nested path replacement, matching
stock Git for directory/file swaps. Expected delta is `+7` matrix rows, `+6`
verified rows, `+1` invalid-input row and `-6` oracle
`missing_or_unclassified` functions. Actual delta matched.

As of 2026-06-25 the `git_merge_plumbing_compat.rs` batch is an evidence-import
slice without product-code changes. The selected rows make exact
`merge-one-file` clean-text and identical add/add shapes, default configured
`mergetool` dispatch, rerere unresolved-path `status` and `remaining`,
pathspec-scoped `forget`, and `--rerere-autoupdate` recorded-resolution reuse
explicit in the matrices. Expected delta is `+6` matrix rows, `+6` verified
rows, `0` invalid-input rows and `-6` oracle `missing_or_unclassified`
functions. Actual delta matched.

As of 2026-06-25 the `git_clone_ref_format_compat.rs` stale-test correction is
an example of a zero-row census slice. The selected row shape was already
present in `clone_v2_47.tsv`; the work only rewired stock-oracle evidence from
an old probe to passing focused tests. Expected delta was `0` matrix rows,
`0` verified rows, `0` invalid-input rows and `-1` oracle
`missing_or_unclassified` function. Actual delta matched.

As of 2026-06-25 the `git_scalar_compat.rs` batch is a row-growth slice driven
by already-selected scalar row shapes. The stock-scalar oracle was polluted by
the local PATH resolving plain `git` to Zmin, so the batch first fixed the
test harness to put the stock Git directory first for stock `scalar` runs.
Expected delta was `+9` matrix rows, `+9` verified rows, `0` invalid-input
rows and `-3` oracle `missing_or_unclassified` functions. Actual delta matched.

As of 2026-06-25 the `git_admin_tools_compat.rs` follow-up is another zero-row
evidence slice. The selected row shapes already existed in
`diagnose_v2_47.tsv`, `bugreport_v2_47.tsv` and `instaweb_v2_47.tsv`; the work
only attached the exact focused tests to those rows. Expected delta was `0`
matrix rows, `0` verified rows, `0` invalid-input rows and `-4` oracle
`missing_or_unclassified` functions. Actual delta matched.

As of 2026-06-25 the `git_cms_porcelain_compat.rs` follow-up is a zero-row
extension-classification slice. The CMS-friendly commands were already tracked
as Zmin-only surfaces in `docs/cli/zmin_extensions_inventory.md`; the work
only replaced the suite-level evidence placeholder with exact focused test
functions. Expected delta was `0` matrix rows, `0` verified rows, `0`
invalid-input rows and `-4` oracle `missing_or_unclassified` functions. Actual
delta matched.

`docs/cli/existing_oracle_test_inventory.tsv` is now an evidence layer only.
After a row or coherent expansion group is selected from the census, this audit
still records the source bucket, expected row/status delta and oracle evidence
movement before any TSV matrix edit.

## Why The Count Grew

The count grew because the branch imported already-existing stock-oracle test
coverage into `docs/cli/matrices/*_v2_47.tsv`.

That is legitimate compatibility accounting only when each imported row records
the full row shape:

`command + option + value + option combination + repo state + transport/local mode + platform + stdout/stderr/exit/.git side effects + stock Git evidence`

The process gap was that the known command and option seeds existed, but the
behavior-row backlog was not frozen up front. As rows were discovered in focused
compat tests, the written-row denominator moved. That made progress honest at
the row level, but operationally too surprising.

The correction is now part of this audit: the current focused-oracle backlog is
the generated and committed `docs/cli/existing_oracle_test_inventory.tsv`, and a
batch may only import rows from entries already present there unless a new
frozen inventory layer is added first. New denominator sources must be declared
as their own inventory, with counts, before any matrix rows are added from that
source.

Do not treat `151/151` commands or `4632/4632` documented option pairs as a
complete behavior denominator. They are command/option seed layers only. The
compatibility denominator grows only when a specific row shape has oracle
evidence and is written as a matrix row.

## Current Baseline

Pushed branch state audited from `9275ac4d` to `HEAD`:

| Metric | At `9275ac4d` | At `HEAD` | Delta |
| --- | ---: | ---: | ---: |
| Written behavior rows | `1094` | `2846` | `+1752` |
| Matching stock Git rows | `823` | `2457` | `+1634` |
| Open rows | `1` | `1` | `0` |
| Invalid-input rows | `270` | `388` | `+118` |
| Commands with rows | `50/151` | `108/151` | `+58` |
| Represented doc-option pairs | `253/4632` | `729/4632` | `+476` |

The text-level row delta audit must be regenerated with
`tools/git-matrix-row-delta-audit.sh 9275ac4d HEAD` after each slice. The strict
behavior row count is authoritative for row-level progress because some commits
rewrite or split existing rows rather than adding net-new row coverage.

The stock-oracle test inventory currently has `969` focused oracle functions:
`946` represented by matrix, extension or deferral evidence, and `23` still
missing or unclassified.

## Net Growth By Command

This table compares actual behavior rows per command at `9275ac4d` and at
`HEAD`.

| Command | Rows at `9275ac4d` | Rows at `HEAD` | Delta |
| --- | ---: | ---: | ---: |
| `stash` | `25` | `207` | `+182` |
| `diff` | `68` | `239` | `+171` |
| `ls-files` | `72` | `155` | `+83` |
| `diff-tree` | `0` | `74` | `+74` |
| `blame` | `101` | `171` | `+70` |
| `config` | `60` | `130` | `+70` |
| `clone` | `6` | `72` | `+66` |
| `commit` | `0` | `78` | `+78` |
| `status` | `76` | `135` | `+59` |
| `branch` | `0` | `49` | `+49` |
| `notes` | `0` | `42` | `+42` |
| `add` | `3` | `51` | `+48` |
| `fsck` | `0` | `35` | `+35` |
| `for-each-ref` | `0` | `34` | `+34` |
| `show` | `0` | `34` | `+34` |
| `remote` | `0` | `32` | `+32` |
| `cat-file` | `8` | `40` | `+32` |
| `ls-remote` | `2` | `31` | `+29` |
| `rev-list` | `0` | `28` | `+28` |
| `tag` | `0` | `27` | `+27` |
| `fetch` | `300` | `326` | `+26` |
| `bundle` | `0` | `21` | `+21` |
| `rev-parse` | `52` | `73` | `+21` |
| `apply` | `0` | `20` | `+20` |
| `check-ignore` | `0` | `20` | `+20` |
| `diff-files` | `0` | `20` | `+20` |
| `maintenance` | `0` | `20` | `+20` |
| `clean` | `12` | `30` | `+18` |
| `log` | `87` | `105` | `+18` |
| `pack-objects` | `0` | `18` | `+18` |
| `send-email` | `0` | `16` | `+16` |
| `interpret-trailers` | `0` | `15` | `+15` |
| `archive` | `1` | `17` | `+16` |
| `diff-index` | `0` | `14` | `+14` |
| `http-backend` | `0` | `14` | `+14` |
| `reflog` | `2` | `15` | `+13` |
| `describe` | `0` | `12` | `+12` |
| `filter-branch` | `2` | `14` | `+12` |
| `grep` | `0` | `12` | `+12` |
| `merge-base` | `0` | `13` | `+13` |
| `var` | `0` | `12` | `+12` |
| `check-ref-format` | `0` | `11` | `+11` |
| `pull` | `0` | `10` | `+10` |
| `rm` | `0` | `10` | `+10` |
| `check-attr` | `0` | `9` | `+9` |
| `check-mailmap` | `0` | `8` | `+8` |
| `cherry` | `0` | `8` | `+8` |
| `count-objects` | `0` | `8` | `+8` |
| `show-ref` | `10` | `18` | `+8` |
| `sparse-checkout` | `0` | `8` | `+8` |
| `symbolic-ref` | `0` | `10` | `+10` |
| `fast-import` | `0` | `7` | `+7` |
| `prune` | `0` | `7` | `+7` |
| `push` | `0` | `7` | `+7` |
| `submodule` | `0` | `7` | `+7` |
| `update-ref` | `0` | `9` | `+9` |
| `fetch-pack` | `0` | `6` | `+6` |
| `format-patch` | `0` | `6` | `+6` |
| `multi-pack-index` | `0` | `6` | `+6` |
| `patch-id` | `0` | `6` | `+6` |
| `replace` | `0` | `6` | `+6` |
| `shortlog` | `0` | `6` | `+6` |
| `verify-pack` | `0` | `6` | `+6` |
| `checkout` | `0` | `5` | `+5` |
| `hash-object` | `0` | `7` | `+7` |
| `stripspace` | `0` | `5` | `+5` |
| `column` | `0` | `4` | `+4` |
| `commit-tree` | `0` | `5` | `+5` |
| `credential` | `0` | `5` | `+5` |
| `credential-cache` | `0` | `6` | `+6` |
| `difftool` | `0` | `21` | `+21` |
| `fmt-merge-msg` | `0` | `4` | `+4` |
| `for-each-repo` | `0` | `4` | `+4` |
| `init` | `0` | `4` | `+4` |
| `ls-tree` | `0` | `6` | `+6` |
| `mailinfo` | `0` | `4` | `+4` |
| `mailsplit` | `0` | `4` | `+4` |
| `read-tree` | `0` | `5` | `+5` |
| `replay` | `0` | `4` | `+4` |
| `send-pack` | `0` | `4` | `+4` |
| `unpack-file` | `0` | `4` | `+4` |
| `version` | `0` | `4` | `+4` |
| `bugreport` | `0` | `9` | `+9` |
| `commit-graph` | `0` | `3` | `+3` |
| `credential-store` | `0` | `5` | `+5` |
| `http-fetch` | `1` | `4` | `+3` |
| `index-pack` | `0` | `3` | `+3` |
| `mktree` | `0` | `4` | `+4` |
| `range-diff` | `0` | `3` | `+3` |
| `request-pull` | `0` | `3` | `+3` |
| `cherry-pick` | `0` | `2` | `+2` |
| `get-tar-commit-id` | `0` | `2` | `+2` |
| `mktag` | `0` | `2` | `+2` |
| `quiltimport` | `0` | `2` | `+2` |
| `update-server-info` | `0` | `2` | `+2` |
| `write-tree` | `0` | `3` | `+3` |
| `am` | `0` | `1` | `+1` |
| `bisect` | `0` | `1` | `+1` |
| `checkout-index` | `0` | `7` | `+7` |
| `merge` | `0` | `1` | `+1` |
| `p4` | `0` | `1` | `+1` |
| `rebase` | `0` | `1` | `+1` |
| `rerere` | `0` | `1` | `+1` |
| `show-index` | `1` | `2` | `+1` |
| `worktree` | `0` | `1` | `+1` |

## Growth Control Rule

Before adding new behavior rows, record the selected bucket:

- source: Git docs expansion, focused stock-oracle test, upstream Git test,
  real tool trace, or guard classification
- file and function or command-option source
- expected row count delta
- expected status split: closed, open, invalid-input, or deferral
- whether Rust behavior changes are required

After the batch, rerun:

```bash
tools/git-matrix-row-delta-audit.sh 9275ac4d HEAD
tools/git-compat-command-summary.sh --tsv
tools/git-compat-audit-summary.sh --tsv
python3 tools/git-existing-oracle-inventory.py > docs/cli/existing_oracle_test_inventory.tsv
```

If actual growth differs from the declared bucket, stop and explain the
difference before committing.

## Current Known Queues

The known queues are:

- `docs/cli/existing_oracle_test_inventory.tsv`: focused stock-oracle test
  functions, currently `961` total with `256` missing or unclassified.
- `docs/cli/git_compatibility_inventory.md`: command and documented option
  seed accounting, currently `151` commands and `4632` documented
  command-option pairs.
- `docs/cli/matrices/*_v2_47.tsv`: current written behavior rows.

These are not a complete final Git behavior denominator. They are the frozen
known inventory layers that must be expanded deliberately.

## Full Backlog Verification

`docs/cli/existing_oracle_test_inventory.tsv` is the full current list of
already-existing focused stock-oracle test functions to walk before importing
more rows from that source. Do not reconstruct this list from chat history or
from a partial `rg` result.

Verify it before each oracle-import batch:

```bash
python3 tools/git-existing-oracle-inventory.py > /tmp/zmin-oracle-inventory.tsv
cmp -s /tmp/zmin-oracle-inventory.tsv docs/cli/existing_oracle_test_inventory.tsv
awk -F '\t' 'NR==1{for(i=1;i<=NF;i++) h[$i]=i; next} { total++; c[$h["inventory_status"]]++ } END { printf "total=%d represented=%d missing_or_unclassified=%d\n", total, c["represented"], c["missing_or_unclassified"] }' docs/cli/existing_oracle_test_inventory.tsv
```

Then select rows only from `missing_or_unclassified`, read the exact test
function, declare the expected row delta below and rerun the same inventory
check after the slice. The expected invariant for an import from this backlog
is:

- `represented` increases by the declared evidence-function count.
- `missing_or_unclassified` decreases by the same count.
- `behavior_rows_written` grows only by the declared row count.
- Any different movement is a process error to investigate before committing.

## Declared Oracle Import Batches

### Completed: Mail Series Binary Format-Patch Batch

- source: focused stock-oracle test backlog
- file: `crates/zmin-cli/tests/git_mail_series_compat.rs`
- functions:
  - `format_patch_binary_summary_matches_stock_git`
- expected row delta: `+1` behavior row in
  `docs/cli/matrices/format_patch_v2_47.tsv`
- expected status split: `+1` closed row, `+0` open rows, `+0` invalid-input
  rows
- expected oracle inventory delta: represented `+1`,
  missing_or_unclassified `-1`
- expected command/doc-option movement: `+0` commands with rows,
  `+1` represented doc-option pair for the modeled binary `--stdout -1 HEAD`
  format-patch surface
- Rust behavior changes required: no
- actual post-import movement matched the declaration: `+1` behavior row,
  `+1` closed row, `+0` open rows, `+0` invalid-input rows, `+1`
  represented oracle function, `-1` missing-or-unclassified oracle function,
  `+0` commands with rows and `+1` represented doc-option pair.

### Completed: Repository-State Zmin Seed Handoff Batch

- source: focused stock-oracle test backlog
- file: `crates/zmin-cli/tests/git_repository_state_compat.rs`
- functions:
  - `zmin_seed_handoff_keeps_repository_state_identical`
- expected row delta: `+1` behavior row in
  `docs/cli/matrices/init_v2_47.tsv`
- expected status split: `+1` closed row, `+0` open rows, `+0` invalid-input
  rows
- expected oracle inventory delta: represented `+1`,
  missing_or_unclassified `-1`
- expected command/doc-option movement: `+0` commands with rows,
  `+0` represented doc-option pairs; the row exercises a downstream
  repository-state handoff rooted in an already represented `init`
  `--initial-branch main` seed workflow
- Rust behavior changes required: no
- actual post-import movement matched the declaration: `+1` behavior row,
  `+1` closed row, `+0` open rows, `+0` invalid-input rows, `+1`
  represented oracle function, `-1` missing-or-unclassified oracle function,
  `+0` commands with rows and `+0` represented doc-option pairs.

### Completed: Stash Index Config Apply Batch

- source: focused stock-oracle test backlog
- file: `crates/zmin-cli/tests/git_stash_compat.rs`
- functions:
  - `stash_index_config_implies_apply_index_like_stock_git`
- expected row delta: `+1` behavior row in
  `docs/cli/matrices/stash_v2_47.tsv`
- expected status split: `+1` closed row, `+0` open rows, `+0` invalid-input
  rows
- expected oracle inventory delta: represented `+1`,
  missing_or_unclassified `-1`
- expected command/doc-option movement: `+0` commands with rows,
  `+1` represented doc-option pair for the modeled `stash.index=true`
  config-driven apply-index behavior
- Rust behavior changes required: no
- actual post-import movement matched the declaration: `+1` behavior row,
  `+1` closed row, `+0` open rows, `+0` invalid-input rows, `+1`
  represented oracle function, `-1` missing-or-unclassified oracle function,
  `+0` commands with rows and `+1` represented doc-option pair.

### Completed: Mail Tools SMTP and IMAP Transport Batch

- source: focused stock-oracle test backlog
- file: `crates/zmin-cli/tests/git_mail_tools_compat.rs`
- functions:
  - `send_email_sends_patch_to_configured_smtp_server`
  - `imap_send_appends_mbox_messages_to_plain_imap_server`
- expected row delta: `+2` behavior rows across
  `docs/cli/matrices/send_email_v2_47.tsv` and
  `docs/cli/matrices/imap_send_v2_47.tsv`
- expected status split: `+2` closed rows, `+0` open rows, `+0` invalid-input
  rows
- expected oracle inventory delta: represented `+2`,
  missing_or_unclassified `-2`
- expected command/doc-option movement: `+0` commands with rows,
  `+2` represented doc-option pairs for the current modeled send-email
  positional patch send flow and `imap-send --no-curl` append flow
- Rust behavior changes required: no
- actual post-import movement matched the declaration: `+2` behavior rows,
  `+2` closed rows, `+0` open rows, `+0` invalid-input rows, `+2`
  represented oracle functions, `-2` missing-or-unclassified oracle functions,
  `+0` commands with rows and `+2` represented doc-option pairs.

### Completed: Sparse-Checkout Pathspec and Sparse-Index Batch

- source: focused stock-oracle test backlog
- file: `crates/zmin-cli/tests/git_sparse_checkout_compat.rs`
- functions:
  - `sparse_checkout_pathspec_edge_cases_match_stock_git`
  - `sparse_checkout_sparse_index_options_match_stock_git`
- expected row delta: `+2` behavior rows in
  `docs/cli/matrices/sparse_checkout_v2_47.tsv`
- expected status split: `+2` closed rows, `+0` open rows, `+0` invalid-input
  rows
- expected oracle inventory delta: represented `+2`,
  missing_or_unclassified `-2`
- expected command/doc-option movement: `+0` commands with rows,
  `+2` represented doc-option pairs for the current modeled
  `sparse-checkout` pathspec edge-case surface and `--sparse-index`
- Rust behavior changes required: no
- actual post-import movement matched the declaration: `+2` behavior rows,
  `+2` closed rows, `+0` open rows, `+0` invalid-input rows, `+2`
  represented oracle functions, `-2` missing-or-unclassified oracle functions,
  `+0` commands with rows and `+2` represented doc-option pairs.

### Completed: Daemon Inetd Unknown-Service Evidence Remap

- source: focused stock-oracle test backlog
- file: `crates/zmin-cli/tests/git_transport_http_compat.rs`
- functions:
  - `daemon_unknown_service_matches_stock_git_inetd_failure`
- expected row delta: `+0` behavior rows; remap existing evidence in
  `docs/cli/matrices/daemon_v2_47.tsv`
- expected status split: `+0` closed rows, `+0` open rows, `+0` invalid-input
  rows
- expected oracle inventory delta: represented `+1`,
  missing_or_unclassified `-1`
- expected command/doc-option movement: `+0` commands with rows,
  `+0` represented doc-option pairs; the existing daemon inetd invalid-input
  row already covers this behavior shape
- Rust behavior changes required: no
- actual post-import movement matched the declaration: `+0` behavior rows,
  `+0` closed rows, `+0` open rows, `+0` invalid-input rows, `+1`
  represented oracle function, `-1` missing-or-unclassified oracle function,
  `+0` commands with rows and `+0` represented doc-option pairs.

### Completed: HTTP Backend Invalid Filter Batch

- source: focused stock-oracle test backlog
- file: `crates/zmin-cli/tests/git_transport_http_compat.rs`
- functions:
  - `http_backend_upload_pack_invalid_filters_match_stock_git_failures`
- expected row delta: `+1` behavior row in
  `docs/cli/matrices/http_backend_v2_47.tsv`
- expected status split: `+0` closed rows, `+0` open rows, `+1` invalid-input
  row
- expected oracle inventory delta: represented `+1`,
  missing_or_unclassified `-1`
- expected command/doc-option movement: `+0` commands with rows,
  `+0` represented doc-option pairs; `http-backend` has no documented CLI
  option seed in the current census and already has matrix coverage
- Rust behavior changes required: no
- actual post-import movement matched the declaration: `+1` behavior row,
  `+0` closed rows, `+0` open rows, `+1` invalid-input row, `+1`
  represented oracle function, `-1` missing-or-unclassified oracle function,
  `+0` commands with rows and `+0` represented doc-option pairs.

### Completed: HTTP Push Writable Remote Batch

- source: focused stock-oracle test backlog
- file: `crates/zmin-cli/tests/git_transport_http_compat.rs`
- functions:
  - `http_push_puts_loose_objects_and_updates_remote_ref`
  - `http_push_deletes_remote_refspec`
- expected row delta: `+2` behavior rows in
  `docs/cli/matrices/http_push_v2_47.tsv`
- expected status split: `+2` closed rows, `0` open rows, `0` invalid-input
  rows
- expected oracle inventory delta: represented `+2`,
  missing_or_unclassified `-2`
- expected command/doc-option movement: `+0` commands with rows,
  `+1` represented doc-option pair for `--dry-run`; `http-push` already has
  rows, while the delete refspec row exercises the existing positional heads
  surface in a real repository state
- Rust behavior changes required: no
- actual post-import movement matched the declaration: `+2` behavior rows,
  `+2` closed rows, `+0` open rows, `+0` invalid-input rows, `+2`
  represented oracle functions, `-2` missing-or-unclassified oracle functions,
  `+0` commands with rows and `+1` represented doc-option pair.

### Completed: HTTP Packfile Read Batch

- source: focused stock-oracle test backlog
- file: `crates/zmin-cli/tests/git_transport_http_compat.rs`
- functions:
  - `http_fetch_fetches_dumb_http_objects_like_stock_git`
  - `clone_reads_smart_http_pack_like_stock_git`
  - `pull_rebase_reads_smart_http_pack_like_stock_git`
- expected row delta: `+3` behavior rows across
  `docs/cli/matrices/http_fetch_v2_47.tsv`,
  `docs/cli/matrices/clone_v2_47.tsv` and
  `docs/cli/matrices/pull_v2_47.tsv`
- expected status split: `+3` closed rows, `0` open rows, `0` invalid-input
  rows
- expected oracle inventory delta: represented `+3`,
  missing_or_unclassified `-3`
- expected command/doc-option movement: `+0` commands with rows,
  `+0` represented doc-option pairs; all three commands and option spellings
  are already represented in the current census
- Rust behavior changes required: no
- actual post-import movement matched the declaration: `+3` behavior rows,
  `+3` closed rows, `+0` open rows, `+0` invalid-input rows, `+3`
  represented oracle functions, `-3` missing-or-unclassified oracle functions,
  `+0` commands with rows and `+0` represented doc-option pairs.

### Completed: Local Fetch Refspec Resolution Batch

- source: focused stock-oracle test backlog
- file: `crates/zmin-cli/tests/git_transport_local_compat.rs`
- functions:
  - `fetch_direct_location_resolves_short_remote_name_but_not_short_tag_like_stock_git`
  - `fetch_direct_location_lhs_refspec_disambiguation_like_stock_git`
  - `fetch_explicit_head_to_branch_backfills_tags_like_stock_git`
  - `fetch_direct_file_url_accepts_multiple_explicit_refspecs_like_stock_git`
  - `fetch_empty_refmap_with_branch_disables_configured_refspec_like_stock_git`
- expected row delta: `+5` behavior rows in
  `docs/cli/matrices/fetch_v2_47.tsv`
- expected status split: `+5` closed rows, `0` open rows, `0` invalid-input
  rows
- expected oracle inventory delta: represented `+5`,
  missing_or_unclassified `-5`
- Rust behavior changes required: no

### Completed: Smart HTTP Fetch Pack Batch

- source: focused stock-oracle test backlog
- file: `crates/zmin-cli/tests/git_transport_http_compat.rs`
- functions:
  - `fetch_reads_smart_http_pack_like_stock_git`
  - `fetch_smart_http_wildcard_refspec_updates_remote_refs_like_stock_git`
  - `fetch_smart_http_incremental_thin_pack_repairs_existing_bases_like_stock_git`
  - `fetch_smart_http_noop_skips_upload_pack_when_roots_exist_locally`
  - `fetch_smart_http_multiple_explicit_tags_with_protocol_v2_like_stock_git`
- expected row delta: `+5` behavior rows in
  `docs/cli/matrices/fetch_v2_47.tsv`
- expected status split: `+5` closed rows, `0` open rows, `0` invalid-input
  rows
- expected oracle inventory delta: represented `+5`,
  missing_or_unclassified `-5`
- Rust behavior changes required: no

### Completed: Local Fetch-Pack Batch

- source: focused stock-oracle test backlog
- file: `crates/zmin-cli/tests/git_transport_local_compat.rs`
- functions:
  - `fetch_pack_copies_local_ref_objects_like_stock_git`
  - `fetch_pack_accepts_thin_and_no_progress_like_stock_git`
  - `fetch_pack_include_tag_copies_annotated_tag_objects_like_stock_git`
  - `fetch_pack_include_tag_with_depth_limited_like_stock_git`
  - `fetch_pack_include_tag_depth_includes_nested_annotated_tags_like_stock_git`
  - `fetch_pack_depth_one_like_stock_git`
- expected row delta: `+6` behavior rows in
  `docs/cli/matrices/fetch_pack_v2_47.tsv`
- expected status split: `+6` closed rows, `0` open rows, `0` invalid-input
  rows
- expected oracle inventory delta: represented `+6`,
  missing_or_unclassified `-6`
- Rust behavior changes required: no
- actual post-import movement matched the declaration: `+6` behavior rows,
  `+6` closed rows, `+0` open rows, `+0` invalid-input rows, `+6`
  represented oracle functions, `-6` missing-or-unclassified oracle functions,
  `+1` command with rows and `+3` represented doc-option pairs.

### Completed: HTTP Backend Upload-Pack Filter Batch

- source: focused stock-oracle test backlog
- file: `crates/zmin-cli/tests/git_transport_http_compat.rs`
- functions:
  - `http_backend_upload_pack_filter_blob_none_omits_blob_objects`
  - `http_backend_upload_pack_filter_blob_limit_omits_large_blobs`
  - `http_backend_upload_pack_filter_object_type_blob_omits_trees`
  - `http_backend_upload_pack_filter_tree_depth_limits_tree_walk`
  - `http_backend_upload_pack_filter_combine_applies_all_filters`
  - `http_backend_upload_pack_filter_sparse_oid_omits_unmatched_blobs`
- expected row delta: `+6` behavior rows in
  `docs/cli/matrices/http_backend_v2_47.tsv`
- expected status split: `+6` closed rows, `0` open rows, `0` invalid-input
  rows
- expected oracle inventory delta: represented `+6`,
  missing_or_unclassified `-6`
- expected command/doc-option movement: `+1` command with rows,
  `+0` represented doc-option pairs; `http-backend` has no documented CLI
  option spelling seed in the current Git docs inventory
- Rust behavior changes required: no
- actual post-import movement matched the declaration: `+6` behavior rows,
  `+6` closed rows, `+0` open rows, `+0` invalid-input rows, `+6`
  represented oracle functions, `-6` missing-or-unclassified oracle functions,
  `+1` command with rows and `+0` represented doc-option pairs.

### Completed: Local Send-Pack Batch

- source: focused stock-oracle test backlog
- file: `crates/zmin-cli/tests/git_transport_local_compat.rs`
- functions:
  - `send_pack_updates_local_bare_ref_like_stock_git`
  - `send_pack_thin_updates_local_bare_ref_like_stock_git`
  - `send_pack_mirror_syncs_heads_tags_and_deletions_like_stock_git`
  - `send_pack_atomic_rejects_all_updates_when_one_ref_fails`
- expected row delta: `+4` behavior rows in
  `docs/cli/matrices/send_pack_v2_47.tsv`
- expected status split: `+3` closed rows, `0` open rows, `+1` invalid-input
  row
- expected oracle inventory delta: represented `+4`,
  missing_or_unclassified `-4`
- expected command/doc-option movement: `+1` command with rows,
  `+3` represented doc-option pairs for `--thin`, `--mirror` and `--atomic`
- Rust behavior changes required: no
- actual post-import movement matched the declaration: `+4` behavior rows,
  `+3` closed rows, `+0` open rows, `+1` invalid-input row, `+4`
  represented oracle functions, `-4` missing-or-unclassified oracle functions,
  `+1` command with rows and `+3` represented doc-option pairs.

### Completed: HTTP Backend Upload-Pack Deepen Batch

- source: focused stock-oracle test backlog
- file: `crates/zmin-cli/tests/git_transport_http_compat.rs`
- functions:
  - `http_backend_upload_pack_deepen_emits_shallow_boundary_and_depth_limited_pack`
  - `http_backend_upload_pack_deepen_since_emits_time_limited_pack`
  - `http_backend_upload_pack_deepen_not_excludes_named_ref_history`
  - `http_backend_upload_pack_deepen_relative_extends_existing_shallow_boundary`
- expected row delta: `+4` behavior rows in
  `docs/cli/matrices/http_backend_v2_47.tsv`
- expected status split: `+4` closed rows, `0` open rows, `0` invalid-input
  rows
- expected oracle inventory delta: represented `+4`,
  missing_or_unclassified `-4`
- expected command/doc-option movement: `+0` commands with rows,
  `+0` represented doc-option pairs; `http-backend` already has matrix rows
  and no documented CLI option spelling seed in the current Git docs inventory
- Rust behavior changes required: no
- actual post-import movement matched the declaration: `+4` behavior rows,
  `+4` closed rows, `+0` open rows, `+0` invalid-input rows, `+4`
  represented oracle functions, `-4` missing-or-unclassified oracle functions,
  `+0` commands with rows and `+0` represented doc-option pairs.

### Completed: HTTP Fetch Packfile Batch

- source: focused stock-oracle test backlog
- file: `crates/zmin-cli/tests/git_transport_http_compat.rs`
- functions:
  - `http_fetch_packfile_downloads_and_indexes_pack`
  - `http_fetch_packfile_requires_index_pack_args_like_stock_git`
- expected row delta: `+3` behavior rows in
  `docs/cli/matrices/http_fetch_v2_47.tsv`
- expected status split: `+2` closed rows, `0` open rows, `+1`
  invalid-input row
- expected oracle inventory delta: represented `+2`,
  missing_or_unclassified `-2`
- expected command/doc-option movement: `+0` commands with rows,
  `+1` represented doc-option pair for `--packfile`; `--index-pack-arg` is
  already represented by the existing invalid delegated-argument row
- Rust behavior changes required: no
- actual post-import movement matched the declaration: `+3` behavior rows,
  `+2` closed rows, `+0` open rows, `+1` invalid-input row, `+2`
  represented oracle functions, `-2` missing-or-unclassified oracle functions,
  `+0` commands with rows and `+1` represented doc-option pair.

## Frozen Oracle Backlog Snapshot

This snapshot explains the remaining known denominator growth from focused
stock-oracle tests. It is intentionally file-level, not row-level: each listed
test function still must be read before adding TSV rows, because one function
can prove one row, several command variants, or a non-Git extension/deferral.

As of this commit, `docs/cli/existing_oracle_test_inventory.tsv` contains `961`
focused oracle functions. `705` are already represented by matrix rows,
extension rows or explicit deferrals, and `256` are
`missing_or_unclassified`.

Largest missing/unclassified buckets:

| Test file | Missing/unclassified functions |
| --- | ---: |
| `git_transport_http_compat.rs` | `21` |
| `git_transport_local_compat.rs` | `22` |
| `git_worktree_state_compat.rs` | `22` |
| `git_maintenance_compat.rs` | `21` |
| `git_pack_integrity_compat.rs` | `18` |
| `git_submodule_compat.rs` | `16` |
| `git_worktree_compat.rs` | `15` |
| `git_notes_compat.rs` | `14` |
| `git_merge_compat.rs` | `13` |
| `git_sequencer_compat.rs` | `10` |
| `git_admin_tools_compat.rs` | `10` |
| `git_merge_plumbing_compat.rs` | `9` |
| `git_foreign_scm_compat.rs` | `8` |
| `git_index_mutation_compat.rs` | `7` |
| `git_refs_compat.rs` | `7` |
| `git_scalar_compat.rs` | `6` |
| `git_ref_resolution_compat.rs` | `6` |
| `git_global_cli_compat.rs` | `5` |
| `git_fast_import_export_compat.rs` | `5` |
| `git_object_plumbing_compat.rs` | `4` |
| `git_cms_porcelain_compat.rs` | `4` |
| `git_sparse_checkout_compat.rs` | `3` |
| `git_mail_tools_compat.rs` | `3` |
| `git_clone_compat.rs` | `2` |
| `compatibility_command.rs` | `1` |
| `git_stash_compat.rs` | `1` |
| `git_repository_state_compat.rs` | `1` |
| `git_mail_series_compat.rs` | `1` |
| `git_cli_failure_compat.rs` | `1` |

Largest command-hint buckets inside those `256` functions:

| Command hint | Missing/unclassified functions |
| --- | ---: |
| `<none>` | `59` |
| `worktree` | `43` |
| `merge` | `28` |
| `remote` | `24` |
| `maintenance` | `23` |
| `submodule` | `17` |
| `branch` | `16` |
| `refs` | `15` |
| `commit` | `14` |
| `notes` | `14` |
| `add` | `13` |
| `checkout` | `7` |

## Oracle Import Walk Order

Until the `missing_or_unclassified` queue is empty, use this order for
docs-only imports from existing focused tests:

1. Prefer the largest file bucket that can produce a coherent 3-10 row batch
   without Rust changes.
2. Within that file, prefer functions that share one command and one behavior
   shape, such as one config family, one transport mode or one state transition.
3. Before editing TSV rows, write the exact evidence functions and expected
   row/status delta in `Latest Declared Import`.
4. After editing, regenerate `docs/cli/existing_oracle_test_inventory.tsv` and
   require the missing count to decrease by the declared evidence-function
   count.

For the current snapshot, the default candidate order is:

| Order | Bucket | Why first |
| ---: | --- | --- |
| 1 | `git_transport_local_compat.rs` (`22`) | remaining local/file transport and remote-management rows |
| 2 | `git_worktree_state_compat.rs` (`22`) | remaining worktree state rows that may expose implementation gaps |
| 3 | `git_transport_http_compat.rs` (`21`) | remaining network transport coverage over HTTP, SSH and git-daemon |
| 4 | `git_maintenance_compat.rs` (`21`) | remaining dense maintenance/repack/multi-pack-index rows |
| 5 | `git_pack_integrity_compat.rs` (`18`) | remaining pack/fsck/bundle rows; continue here only when the selected function group is coherent |

If a new WebStorm or replacement-binary blocker appears, it overrides this
walk order. If a selected bucket produces Zmin-only extension behavior or an
intentional deferral instead of Git matrix rows, record that classification and
do not increase written behavior rows.

### Completed: Local Fetch Prune Batch

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`, first frozen backlog bucket
`git_transport_local_compat.rs`, local fetch prune behavior group.

Evidence functions:

- `git_transport_local_compat::fetch_prune_with_branch_name_keeps_other_remote_tracking_refs_like_stock_git`
- `git_transport_local_compat::fetch_prune_config_prunes_stale_remote_tracking_refs_like_stock_git`
- `git_transport_local_compat::fetch_prune_only_prints_remote_url_header_like_stock_git`
- `git_transport_local_compat::fetch_prune_resolves_remote_tracking_directory_file_conflict_like_stock_git`
- `git_transport_local_compat::fetch_prune_tags_config_with_prune_config_removes_stale_tags_like_stock_git`
- `git_transport_local_compat::fetch_direct_file_url_prune_tags_prunes_tags_but_keeps_remote_tracking_refs_like_stock_git`

Expected movement:

- behavior rows: `+6`
- closed rows: `+6`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+6`
- missing-or-unclassified oracle functions: `-6`
- commands with rows: `+0`
- represented doc-option pairs: expected `+0`; `fetch --prune`,
  `--prune-tags` and `<none>` already had rows
- Rust behavior changes: no

Expected rows:

- `git fetch --prune origin main`
- `git -c fetch.prune=true fetch origin`
- `git fetch --prune origin` with a stale remote-tracking branch
- `git fetch --prune` resolving a remote-tracking directory/file conflict
- `git -c fetch.prune=true -c fetch.pruneTags=true fetch origin`
- `git fetch file://repo --prune --prune-tags`

The evidence checks stock-compatible local/file transport prune behavior:
branch-limited prune keeps unrelated stale remote-tracking refs, prune config
removes stale remote-tracking refs, prune stderr begins with the `From <url>`
header, prune resolves a remote-tracking D/F conflict, pruneTags config removes
stale tags with prune enabled, and direct file URL pruneTags prunes tags while
keeping remote-tracking refs.

Actual post-import movement matched the declaration: `+6` behavior rows, `+6`
closed rows, `+0` open rows, `+0` invalid-input rows, `+6` represented oracle
functions, `-6` missing-or-unclassified oracle functions, `+0` commands with
rows and `+0` represented doc-option pairs.

## Latest Declared Import

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`, first frozen backlog bucket
`git_transport_http_compat.rs`, network clone shallow/shared behavior group.

Evidence functions:

- `git_transport_http_compat::clone_reads_shallow_ssh_remote_like_stock_git`
- `git_transport_http_compat::clone_shared_is_ignored_for_ssh_remote_like_stock_git`
- `git_transport_http_compat::clone_reads_shallow_smart_http_pack_like_stock_git`
- `git_transport_http_compat::clone_reads_shallow_smart_http_tags_like_stock_git`
- `git_transport_http_compat::clone_shared_is_ignored_for_smart_http_like_stock_git`
- `git_transport_http_compat::clone_reads_shallow_dumb_http_repository_like_stock_git`
- `git_transport_http_compat::clone_shared_is_ignored_for_dumb_http_like_stock_git`
- `git_transport_http_compat::clone_reads_shallow_git_daemon_remote_like_stock_git`
- `git_transport_http_compat::clone_shared_is_ignored_for_git_daemon_like_stock_git`

Expected movement:

- behavior rows: `+9`
- closed rows: `+8`
- open rows: `+0`
- invalid-input rows: `+1`
- represented oracle functions: `+9`
- missing-or-unclassified oracle functions: `-9`
- commands with rows: `+0`
- represented doc-option pairs: expected `+0`; `clone --depth` and
  `clone --shared` already had rows
- Rust behavior changes: no

Expected rows:

- `GIT_SSH_COMMAND=<fake-ssh> git clone --depth=1 ssh://host/path/remote.git dst`
- `GIT_SSH_COMMAND=<fake-ssh> git clone --shared ssh://host/path/remote.git dst`
- `git clone --depth=1 http://host/remote.git dst` over smart HTTP
- `git clone --depth=1 http://host/remote.git dst` over smart HTTP with tags
- `git clone --shared http://host/remote.git dst` over smart HTTP
- `git clone --depth=1 http://host/.git dst` over dumb HTTP
- `git clone --shared http://host/.git dst` over dumb HTTP
- `git clone --depth=1 git://127.0.0.1:<port>/remote.git dst`
- `git clone --shared git://127.0.0.1:<port>/remote.git dst`

The evidence compares stock Git and Zmin clone results for network shallow
state, tag/ref handling and the non-local `--shared` behavior where stock Git
does not write an alternates file. The dumb HTTP shallow row is
stock-compatible invalid input because stock Git rejects shallow capabilities
for dumb HTTP.

Actual post-import movement matched the declaration: `+9` behavior rows, `+8`
closed rows, `+0` open rows, `+1` invalid-input row, `+9` represented oracle
functions, `-9` missing-or-unclassified oracle functions, `+0` commands with
rows and `+0` represented doc-option pairs.

## Latest Declared Import

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`, first frozen backlog bucket
`git_transport_local_compat.rs`, remote-HEAD fetch behavior group.

Evidence functions:

- `git_transport_local_compat::fetch_follow_remote_head_never_does_not_recreate_remote_head`
- `git_transport_local_compat::fetch_default_follow_remote_head_preserves_existing_remote_head`
- `git_transport_local_compat::fetch_explicit_refspec_does_not_update_remote_head_like_stock_git`

Expected movement:

- behavior rows: `+3`
- closed rows: `+3`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+3`
- missing-or-unclassified oracle functions: `-3`
- commands with rows: `+0`
- represented doc-option pairs: expected `+0`; `fetch <none>` and
  `<refspec>` already had rows
- Rust behavior changes: no

Expected rows:

- `git -c remote.origin.followRemoteHEAD=never fetch`
- `git fetch` with an existing manually set `refs/remotes/origin/HEAD`
- `git fetch origin refs/heads/main:refs/remotes/origin/main` with
  `remote.origin.followRemoteHEAD=always`

The evidence checks stock-compatible remote HEAD handling for configured local
remotes: `followRemoteHEAD=never` does not recreate a deleted remote HEAD, a
default fetch preserves an existing remote HEAD, and an explicit refspec fetch
does not update remote HEAD even when followRemoteHEAD is `always`.

Actual post-import movement matched the declaration: `+3` behavior rows, `+3`
closed rows, `+0` open rows, `+0` invalid-input rows, `+3` represented oracle
functions, `-3` missing-or-unclassified oracle functions, `+0` commands with
rows and `+0` represented doc-option pairs.

## Latest Declared Import

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`, first frozen backlog bucket
`git_transport_http_compat.rs`.

Evidence functions:

- `git_transport_http_compat::clone_reads_git_daemon_remote_like_stock_git`
- `git_transport_http_compat::clone_reads_ssh_remote_like_stock_git`

Expected movement:

- behavior rows: `+2`
- closed rows: `+2`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+2`
- missing-or-unclassified oracle functions: `-2`
- commands with rows: `+0`
- represented doc-option pairs: expected `+0`; `clone <repository>` already
  had rows
- Rust behavior changes: no

Expected rows:

- `git clone git://127.0.0.1:<port>/remote.git dst`
- `GIT_SSH_COMMAND=<fake-ssh> git clone ssh://host/path/remote.git dst`

The evidence asserts successful execution and compares stock Git and Zmin
checked-out file contents, `HEAD` and refs for default clone over git-daemon
and SSH transports.

Actual post-import movement matched the declaration: `+2` behavior rows, `+2`
closed rows, `+0` open rows, `+0` invalid-input rows, `+2` represented oracle
functions, `-2` missing-or-unclassified oracle functions, `+0` commands with
rows and `+0` represented doc-option pairs.

## Latest Declared Import

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`, first frozen backlog bucket
`git_transport_http_compat.rs`.

Evidence functions:

- `git_transport_http_compat::fetch_reads_git_daemon_remote_like_stock_git`
- `git_transport_http_compat::fetch_reads_ssh_remote_like_stock_git`
- `git_transport_http_compat::fetch_ssh_wildcard_refspec_prune_no_tags_like_stock_git`

Expected movement:

- behavior rows: `+3`
- closed rows: `+3`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+3`
- missing-or-unclassified oracle functions: `-3`
- commands with rows: `+0`
- represented doc-option pairs: expected `+0`; `<none>`, `<refspec>`,
  `--prune` and `--no-tags` already had `fetch` rows
- Rust behavior changes: no

Expected rows:

- `git fetch origin` with a configured git-daemon remote
- `GIT_SSH_COMMAND=<fake-ssh> git fetch origin` with a configured SSH remote
- `GIT_SSH_COMMAND=<fake-ssh> git fetch origin +refs/heads/*:refs/remotes/origin/* --prune --no-tags`

The evidence asserts successful execution and compares stock Git and Zmin
remote-tracking refs plus fetched object contents for the default transport
fetches and the explicit wildcard/prune/no-tags SSH fetch.

Actual post-import movement matched the declaration: `+3` behavior rows, `+3`
closed rows, `+0` open rows, `+0` invalid-input rows, `+3` represented oracle
functions, `-3` missing-or-unclassified oracle functions, `+0` commands with
rows and `+0` represented doc-option pairs.

## Latest Declared Import

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`, first frozen backlog bucket
`git_transport_http_compat.rs`.

Evidence function:

- `git_transport_http_compat::ls_remote_reads_ssh_remote_like_stock_git`

Expected movement:

- behavior rows: `+6`
- closed rows: `+6`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+1`
- missing-or-unclassified oracle functions: `-1`
- commands with rows: `+0`
- represented doc-option pairs: expected `+0`; `<repository>`, `--heads`,
  `--tags`, `--refs` and `<pattern>` already had `ls-remote` rows
- Rust behavior changes: no

Expected rows:

- `GIT_SSH_COMMAND=<fake-ssh> git ls-remote ssh://host/path/remote.git`
- `GIT_SSH_COMMAND=<fake-ssh> git ls-remote --heads ssh://host/path/remote.git`
- `GIT_SSH_COMMAND=<fake-ssh> git ls-remote --tags ssh://host/path/remote.git`
- `GIT_SSH_COMMAND=<fake-ssh> git ls-remote --refs ssh://host/path/remote.git`
- `GIT_SSH_COMMAND=<fake-ssh> git ls-remote ssh://host/path/remote.git v*`
- `GIT_SSH_COMMAND=<fake-ssh> git ls-remote host:path/remote.git`

The evidence asserts successful execution and compares stock Git and Zmin
stdout for SSH transport across default repository listing, head filtering, tag
filtering, `--refs` filtering, a pattern argument and scp-like URL syntax.

Actual post-import movement matched the declaration: `+6` behavior rows, `+6`
closed rows, `+0` open rows, `+0` invalid-input rows, `+1` represented oracle
function, `-1` missing-or-unclassified oracle function, `+0` commands with rows
and `+0` represented doc-option pairs.

## Latest Declared Import

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`, first frozen backlog bucket
`git_transport_http_compat.rs`.

Evidence function:

- `git_transport_http_compat::ls_remote_reads_git_daemon_remote_like_stock_git`

Expected movement:

- behavior rows: `+5`
- closed rows: `+5`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+1`
- missing-or-unclassified oracle functions: `-1`
- commands with rows: `+0`
- represented doc-option pairs: expected `+0`; `<repository>`, `--heads`,
  `--tags`, `--refs` and `<pattern>` already had `ls-remote` rows
- Rust behavior changes: no

Expected rows:

- `git ls-remote git://127.0.0.1:<port>/remote.git`
- `git ls-remote --heads git://127.0.0.1:<port>/remote.git`
- `git ls-remote --tags git://127.0.0.1:<port>/remote.git`
- `git ls-remote --refs git://127.0.0.1:<port>/remote.git`
- `git ls-remote git://127.0.0.1:<port>/remote.git v*`

The evidence compares stock Git and Zmin stdout, stderr and exit code for
git-daemon transport across default repository listing, head filtering, tag
filtering, `--refs` filtering and a pattern argument.

Actual post-import movement matched the declaration: `+5` behavior rows, `+5`
closed rows, `+0` open rows, `+0` invalid-input rows, `+1` represented oracle
function, `-1` missing-or-unclassified oracle function, `+0` commands with rows
and `+0` represented doc-option pairs.

## Latest Declared Import

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`, first frozen backlog bucket
`git_transport_http_compat.rs`.

Evidence functions:

- `git_transport_http_compat::ls_remote_reads_dumb_http_info_refs_like_stock_git`
- `git_transport_http_compat::ls_remote_reads_smart_http_info_refs_like_stock_git`

Expected movement:

- behavior rows: `+10`
- closed rows: `+10`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+2`
- missing-or-unclassified oracle functions: `-2`
- commands with rows: `+0`
- represented doc-option pairs: expected `+3` for `ls-remote --heads`,
  `ls-remote --tags` and `ls-remote <pattern>`; `<repository>` and `--refs`
  already had rows
- Rust behavior changes: no

Expected rows:

- `git ls-remote http://127.0.0.1:<port>/.git`
- `git ls-remote --heads http://127.0.0.1:<port>/.git`
- `git ls-remote --tags http://127.0.0.1:<port>/.git`
- `git ls-remote --refs http://127.0.0.1:<port>/.git`
- `git ls-remote http://127.0.0.1:<port>/.git v*`
- `git ls-remote http://127.0.0.1:<port>/remote.git`
- `git ls-remote --heads http://127.0.0.1:<port>/remote.git`
- `git ls-remote --tags http://127.0.0.1:<port>/remote.git`
- `git ls-remote --refs http://127.0.0.1:<port>/remote.git`
- `git ls-remote http://127.0.0.1:<port>/remote.git v*`

The evidence compares stock Git and Zmin stdout, stderr and exit code for
dumb HTTP `info/refs` and smart HTTP discovery across default repository
listing, head filtering, tag filtering, `--refs` filtering and a pattern
argument.

Actual post-import movement matched the corrected declaration: `+10` behavior
rows, `+10` closed rows, `+0` open rows, `+0` invalid-input rows, `+2`
represented oracle functions, `-2` missing-or-unclassified oracle functions,
`+0` commands with rows and `+3` represented doc-option pairs. The pre-edit
estimate expected `+2` represented doc-option pairs, but generated summary
correctly counted `ls-remote <pattern>` as a represented documented seed too;
this correction was made before committing.

## Latest Declared Import

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions:

- `git_pack_integrity_compat::fsck_gitmodules_blob_severity_config_matches_stock_git`
- `git_pack_integrity_compat::fsck_gitmodules_missing_severity_config_matches_stock_git`
- `git_pack_integrity_compat::fsck_gitmodules_name_config_validation_matches_stock_git`
- `git_pack_integrity_compat::fsck_gitmodules_url_severity_config_matches_stock_git`
- `git_pack_integrity_compat::fsck_gitmodules_path_severity_config_matches_stock_git`
- `git_pack_integrity_compat::fsck_gitmodules_update_severity_config_matches_stock_git`

Expected movement:

- behavior rows: `+6`
- closed rows: `+0`
- open rows: `+0`
- invalid-input rows: `+6`
- represented oracle functions: `+6`
- missing-or-unclassified oracle functions: `-6`
- commands with rows: `+0`
- represented doc-option pairs: `+0`
- Rust behavior changes: no

Expected rows:

- `git -c fsck.gitmodulesBlob=bogus fsck`
- `git -c fsck.gitmodulesMissing=bogus fsck`
- `git -c fsck.gitmodulesName=bogus fsck`
- `git -c fsck.gitmodulesUrl=bogus fsck`
- `git -c fsck.gitmodulesPath=bogus fsck`
- `git -c fsck.gitmodulesUpdate=bogus fsck`

The evidence compares stock Git and Zmin output and exit status for invalid
`fsck.<message>` severity config values against invalid `.gitmodules` object
and content states.

Actual post-import movement matched the declaration: `+6` behavior rows, `+0`
closed rows, `+0` open rows, `+6` invalid-input rows, `+6` represented oracle
functions, `-6` missing-or-unclassified oracle functions, `+0` commands with
rows and `+0` represented doc-option pairs.

## Latest Declared Import

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions:

- `git_pack_integrity_compat::fsck_tree_not_sorted_severity_config_matches_stock_git`
- `git_pack_integrity_compat::fsck_special_tree_name_severity_config_matches_stock_git`
- `git_pack_integrity_compat::fsck_full_pathname_severity_config_matches_stock_git`
- `git_pack_integrity_compat::fsck_duplicate_entries_severity_config_matches_stock_git`
- `git_pack_integrity_compat::fsck_null_sha1_severity_config_matches_stock_git`
- `git_pack_integrity_compat::fsck_gitmodules_parse_severity_config_matches_stock_git`

Expected movement:

- behavior rows: `+8`
- closed rows: `+0`
- open rows: `+0`
- invalid-input rows: `+8`
- represented oracle functions: `+6`
- missing-or-unclassified oracle functions: `-6`
- commands with rows: `+0`
- represented doc-option pairs: `+0`
- Rust behavior changes: no

Expected rows:

- `git -c fsck.treeNotSorted=bogus fsck`
- `git -c fsck.hasDot=bogus fsck`
- `git -c fsck.hasDotdot=bogus fsck`
- `git -c fsck.hasDotgit=bogus fsck`
- `git -c fsck.fullPathname=bogus fsck`
- `git -c fsck.duplicateEntries=bogus fsck`
- `git -c fsck.nullSha1=bogus fsck`
- `git -c fsck.gitmodulesParse=bogus fsck`

The evidence compares stock Git and Zmin output and exit status for invalid
`fsck.<message>` severity config values against malformed tree objects and
malformed `.gitmodules` content.

Actual post-import movement matched the declaration: `+8` behavior rows, `+0`
closed rows, `+0` open rows, `+8` invalid-input rows, `+6` represented oracle
functions, `-6` missing-or-unclassified oracle functions, `+0` commands with
rows and `+0` represented doc-option pairs.

## Latest Declared Import

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions:

- `git_pack_integrity_compat::fsck_missing_space_before_email_severity_config_matches_stock_git`
- `git_pack_integrity_compat::fsck_missing_name_before_email_severity_config_matches_stock_git`
- `git_pack_integrity_compat::fsck_missing_space_before_date_severity_config_matches_stock_git`
- `git_pack_integrity_compat::fsck_zero_padded_date_severity_config_matches_stock_git`
- `git_pack_integrity_compat::fsck_zero_padded_filemode_severity_config_matches_stock_git`
- `git_pack_integrity_compat::fsck_bad_filemode_severity_config_matches_stock_git`

Expected movement:

- behavior rows: `+6`
- closed rows: `+0`
- open rows: `+0`
- invalid-input rows: `+6`
- represented oracle functions: `+6`
- missing-or-unclassified oracle functions: `-6`
- commands with rows: `+0`
- represented doc-option pairs: `+0`
- Rust behavior changes: no

Expected rows:

- `git -c fsck.missingSpaceBeforeEmail=bogus fsck`
- `git -c fsck.missingNameBeforeEmail=bogus fsck`
- `git -c fsck.missingSpaceBeforeDate=bogus fsck`
- `git -c fsck.zeroPaddedDate=bogus fsck`
- `git -c fsck.zeroPaddedFilemode=bogus fsck`
- `git -c fsck.badFilemode=bogus fsck`

The evidence compares stock Git and Zmin output and exit status for invalid
`fsck.<message>` severity config values against malformed commit and tree
objects.

Actual post-import movement matched the declaration: `+6` behavior rows, `+0`
closed rows, `+0` open rows, `+6` invalid-input rows, `+6` represented oracle
functions, `-6` missing-or-unclassified oracle functions, `+0` commands with
rows and `+0` represented doc-option pairs.

## Latest Declared Classification

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`, classified as Zmin-only
extension evidence rather than Git `2.47.1` matrix coverage.

Evidence functions:

- `git_transport_http_compat::clone_instant_git_daemon_materializes_head_then_fetch_hydrates_refs`
- `git_transport_http_compat::clone_instant_git_daemon_demand_hydrate_recovers_missing_head_objects`
- `git_transport_http_compat::clone_instant_git_daemon_background_fetch_hydrates_refs`
- `git_transport_http_compat::clone_instant_ssh_materializes_head_then_fetch_hydrates_refs`
- `git_transport_http_compat::clone_instant_ssh_demand_hydrate_recovers_missing_head_objects`
- `git_transport_http_compat::clone_instant_ssh_background_fetch_hydrates_refs`
- `git_transport_http_compat::clone_instant_smart_http_materializes_head_then_fetch_hydrates_refs`
- `git_transport_http_compat::clone_instant_smart_http_demand_hydrate_recovers_missing_head_objects`
- `git_transport_http_compat::clone_instant_smart_http_background_fetch_hydrates_refs`

Expected movement:

- behavior rows: `+0`
- closed rows: `+0`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+9`
- missing-or-unclassified oracle functions: `-9`
- commands with rows: `+0`
- represented doc-option pairs: `+0`
- Rust behavior changes: no

Classification:

- `zmin clone --instant` over git-daemon, SSH and smart HTTP is an additive
  Zmin clone mode and not a Git `2.47.1` option row.
- `zmin clone --instant --background-fetch` over git-daemon, SSH and smart HTTP
  is a Zmin-only extension mode.
- `zmin clone --instant --demand-hydrate` over git-daemon, SSH and smart HTTP
  is a Zmin-only extension mode.

Actual post-classification movement matched the declaration: `+0` behavior
rows, `+0` closed rows, `+0` open rows, `+0` invalid-input rows, `+9`
represented oracle functions, `-9` missing-or-unclassified oracle functions,
`+0` commands with rows and `+0` represented doc-option pairs.

Use this snapshot as the upper bound for already-known oracle-import growth.
Future slices should reduce the `missing_or_unclassified` count by their
declared evidence-function count. If a slice increases written TSV rows without
reducing this count or without naming a different source bucket, that is a
process error.

## Previous Declared Import

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence function:

- `git_commit_compat::commit_hook_failures_match_stock_git_flow`

Expected movement:

- behavior rows: `+3`
- closed rows: `+3`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+1`
- missing-or-unclassified oracle functions: `-1`
- commands with rows: `+0`
- represented doc-option pairs: `+0`
- Rust behavior changes: no

Expected rows:

- `git commit -m fail` with failing `pre-commit`
- `git commit -m fail` with failing `commit-msg`
- `git commit -m post` with failing `post-commit`

The evidence compares stock Git and Zmin output, exit status and repository
state for blocked and post-commit hook flows.

Actual post-import movement matched the declaration: `+3` behavior rows,
`+3` closed rows, `+0` open rows, `+0` invalid-input rows, `+1`
represented oracle function, `-1` missing-or-unclassified oracle function,
`+0` commands with rows and `+0` represented doc-option pairs.

Source bucket: census-selected implemented-but-unverified `cherry-pick`
schema rows, using focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv` as the evidence layer.

Evidence functions:

- `git_sequencer_compat::cherry_pick_and_revert_match_stock_git_for_clean_single_commit`
- `git_sequencer_compat::cherry_pick_and_revert_mainline_merge_match_stock_git`

Expected movement:

- behavior rows: `+2`
- closed rows: `+2`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+2`
- missing-or-unclassified oracle functions: `-2`
- commands with rows: `+1`
- represented doc-option pairs: expected `+1` for the shared `mainline`
  parser surface
- Rust behavior changes: no

Expected rows:

- `git cherry-pick <feature-commit>`
- `git cherry-pick -m 1 <merge-commit>`

The evidence compares stock Git and Zmin exit status, stdout/stderr, resulting
tree, commit subject and clean worktree state for clean single-commit input
and selected-mainline merge input.

Actual post-import movement matched the declaration: `+2` behavior rows,
`+2` closed rows, `+0` open rows, `+0` invalid-input rows, `+2`
represented oracle functions, `-2` missing-or-unclassified oracle functions,
`+1` command with rows and `+1` represented doc-option pair.

## Latest Declared Import

Source bucket: census-selected implemented-but-unverified `verify-pack`
`<positional:packs>` schema row, using already represented focused
stock-oracle evidence from `docs/cli/existing_oracle_test_inventory.tsv`.

Evidence function:

- `git_pack_integrity_compat::verify_pack_matches_stock_git_for_default_and_stats`

Expected movement:

- behavior rows: `+1`
- closed rows: `+1`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: expected `+0`; this closes the Zmin schema
  positional `packs` parser surface for the already represented default
  verify-pack case
- Rust behavior changes: no

Expected row:

- `git verify-pack .git/objects/pack/pack-*.idx`

The evidence compares stock Git and Zmin default verify-pack stdout for the
same local pack index.

Actual post-import movement matched the declaration: `+1` behavior row,
`+1` closed row, `+0` open rows, `+0` invalid-input rows, `+0`
represented oracle functions, `+0` missing-or-unclassified oracle functions,
`+0` commands with rows and `+0` represented doc-option pairs.

## Latest Declared Import

Source bucket: census-selected implemented-but-unverified positional schema
rows for object plumbing commands, using already represented focused
stock-oracle evidence from `docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions:

- `git_object_plumbing_compat::hash_object_and_cat_file_match_stock_git`
- `git_object_plumbing_compat::unpack_file_matches_stock_git_blob_behavior`

Expected movement:

- behavior rows: `+2`
- closed rows: `+2`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: expected `+0`; these close the Zmin schema
  positional `paths` and `object` parser surfaces for already represented
  stock-Git parity cases
- Rust behavior changes: no

Expected rows:

- `git hash-object a.txt`
- `git unpack-file <blob>`

The evidence compares stock Git and Zmin object ids for path hashing, and the
temporary worktree file content for unpacking a blob object.

Actual post-import movement matched the declaration: `+2` behavior rows,
`+2` closed rows, `+0` open rows, `+0` invalid-input rows, `+0`
represented oracle functions, `+0` missing-or-unclassified oracle functions,
`+0` commands with rows and `+0` represented doc-option pairs.

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence function:

- `git_pack_integrity_compat::verify_pack_matches_stock_git_for_default_and_stats`

Expected movement:

- behavior rows: `+3`
- closed rows: `+3`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+1`
- missing-or-unclassified oracle functions: `-1`
- commands with rows: `+0`; `verify-pack` already has matrix rows
- represented doc-option pairs: expected `+2` for `verify-pack -v` and
  `verify-pack -s` if those documented short options were not already counted
- Rust behavior changes: no

Expected rows:

- `git verify-pack <idx>`
- `git verify-pack -v <idx>`
- `git verify-pack -s <idx>`

The evidence compares stock Git and Zmin stdout for default verification,
verbose object listing and statistics output against the same local pack index.

Actual post-import movement matched the declaration: `+3` behavior rows, `+3`
closed rows, `+0` open rows, `+0` invalid-input rows, `+1` represented oracle
function, `-1` missing-or-unclassified oracle function, `+0` commands with
rows and `+2` represented doc-option pairs.

## Latest Declared Import

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions:

- `git_pack_integrity_compat::bundle_create_list_heads_and_unbundle_are_stock_readable`
- `git_pack_integrity_compat::bundle_create_accepts_version_option_for_upstream_fetch_suite`
- `git_pack_integrity_compat::bundle_create_accepts_since_option_for_upstream_fetch_suite`
- `git_pack_integrity_compat::bundle_unbundle_accepts_prerequisite_bundles`

Expected movement:

- behavior rows: `+10`
- closed rows: `+10`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+4`
- missing-or-unclassified oracle functions: `-4`
- commands with rows: `+0`; `bundle` already has matrix rows
- represented doc-option pairs: expected `+1`; `bundle --version` and bundle
  path rows already have represented rows, while `bundle --since` becomes newly
  represented if it was not already counted
- Rust behavior changes: no

Expected rows:

- `git bundle create repo.bundle HEAD <branch>`
- `git bundle list-heads repo.bundle`
- `git bundle list-heads repo.bundle refs/heads/main`
- `git bundle list-heads repo.bundle refs/heads/*`
- `git bundle unbundle repo.bundle`
- `git bundle unbundle repo.bundle refs/heads/*`
- `git bundle create --version=3 versioned.bundle main^..main`
- `git bundle create since.bundle main --since=<date>`
- `git bundle verify incremental.bundle` with prerequisites satisfied
- `git bundle unbundle incremental.bundle` with prerequisites satisfied

The evidence checks stock-readable Zmin-created bundle payloads, stock-matching
`list-heads` and `unbundle` stdout, fetchability of a versioned bundle,
`--since` bundle verification, and prerequisite bundle verify/unbundle behavior.

Actual post-import movement matched the declaration: `+10` behavior rows,
`+10` closed rows, `+0` open rows, `+0` invalid-input rows, `+4` represented
oracle functions, `-4` missing-or-unclassified oracle functions, `+0` commands
with rows and `+1` represented doc-option pair.

## Latest Declared Import

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions:

- `git_maintenance_compat::multi_pack_index_repack_batch_size_one_noops_like_stock_git`
- `git_maintenance_compat::multi_pack_index_repack_and_expire_consolidate_like_stock_git`
- `git_maintenance_compat::maintenance_incremental_repack_writes_stock_verifiable_multi_pack_index`

Expected movement:

- behavior rows: `+3`
- closed rows: `+3`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+3`
- missing-or-unclassified oracle functions: `-3`
- commands with rows: `+0`; `multi-pack-index` and `maintenance` already
  have matrix rows
- represented doc-option pairs: expected `+1`; `maintenance --task` already
  has represented rows, and `multi-pack-index --batch-size` becomes newly
  represented if it was not already counted
- Rust behavior changes: no

Expected rows:

- `git multi-pack-index repack --batch-size=1`
- `git multi-pack-index repack --batch-size=0` followed by
  `git multi-pack-index expire`
- `git maintenance run --task=incremental-repack`

The evidence compares stock Git and Zmin pack-file effects, no-op behavior for
small repack batches, consolidation plus expire behavior, stock-verifiable
multi-pack-index metadata and `fsck --strict`/`multi-pack-index verify`
side effects.

Actual post-import movement matched the declaration: `+3` behavior rows, `+3`
closed rows, `+0` open rows, `+0` invalid-input rows, `+3` represented oracle
functions, `-3` missing-or-unclassified oracle functions, `+0` commands with
rows and `+1` represented doc-option pair.

### Declared: Push Network Transport Batch

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`, current largest missing bucket
`git_transport_http_compat.rs`, push network transport behavior group.

Evidence functions:

- `git_transport_http_compat::push_writes_ssh_remote_like_stock_git`
- `git_transport_http_compat::push_writes_smart_http_remote_like_stock_git`

Expected movement:

- behavior rows: `+6`
- closed rows: `+6`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+2`
- missing-or-unclassified oracle functions: `-2`
- commands with rows: `+0`
- represented doc-option pairs: to be confirmed by generated summary because
  this extends existing `push` coverage and may add the first `push -u` row
- Rust behavior changes: no

Expected rows:

- `GIT_SSH_COMMAND=<fake-ssh> git push -u origin main`
- `GIT_SSH_COMMAND=<fake-ssh> git push origin feature`
- `GIT_SSH_COMMAND=<fake-ssh> git push origin :feature`
- `git push -u origin main` over smart HTTP
- `git push origin feature` over smart HTTP
- `git push origin :feature` over smart HTTP
The evidence compares stock Git and Zmin remote refs, pushed object contents,
feature branch creation/deletion and upstream branch config after pushes over
SSH and smart HTTP transports.

Actual post-import movement matched the declaration: `+6` behavior rows,
`+6` closed rows, `+0` open rows, `+0` invalid-input rows, `+2`
represented oracle functions, `-2` missing-or-unclassified oracle functions,
`+0` commands with rows and `+1` represented doc-option pair.

### Declared: Pack Objects Readable Pack Batch

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`, current largest missing bucket
`git_pack_integrity_compat.rs`, pack-objects readable-pack behavior group.

Evidence functions:

- `git_pack_integrity_compat::pack_objects_stdout_writes_pack_readable_by_stock_git`
- `git_pack_integrity_compat::pack_objects_progress_flags_write_stock_readable_pack`
- `git_pack_integrity_compat::pack_objects_undeltified_compat_flags_write_stock_readable_pack`
- `git_pack_integrity_compat::pack_objects_window_depth_writes_stock_readable_delta_pack`
- `git_pack_integrity_compat::pack_objects_base_name_writes_stock_readable_pack_and_index`

Expected movement:

- behavior rows: `+8`
- closed rows: `+8`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+5`
- missing-or-unclassified oracle functions: `-5`
- commands with rows: `+0`
- represented doc-option pairs: to be confirmed by generated summary because
  this extends existing `pack-objects` coverage and may add the first rows for
  `--stdout`, `--revs`, progress and delta-control options
- Rust behavior changes: no

Expected rows:

- `git pack-objects --stdout --revs`
- `git pack-objects --stdout --revs --progress`
- `git pack-objects --stdout --revs --no-progress`
- `git pack-objects --stdout --revs --no-reuse-delta`
- `git pack-objects --stdout --revs --no-reuse-object`
- `git pack-objects --stdout --revs --delta-base-offset`
- `git pack-objects --window=10 --depth=10 .git/objects/pack/pack-zmin`
- `git pack-objects .git/objects/pack/pack-zmin`

The evidence verifies Zmin-created packs with stock Git by unpacking objects,
checking object contents, verifying delta linkage with `verify-pack -v`, and
checking that basename mode writes stock-readable `.pack` and `.idx` files.

Actual post-import movement matched the declaration: `+8` behavior rows,
`+8` closed rows, `+0` open rows, `+0` invalid-input rows, `+5`
represented oracle functions, `-5` missing-or-unclassified oracle functions,
`+0` commands with rows and `+6` represented doc-option pairs.

### Declared: Maintenance Prefetch Local Batch

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`, current largest missing bucket
`git_maintenance_compat.rs`, maintenance prefetch local/no-remote behavior
group.

Evidence functions:

- `git_maintenance_compat::maintenance_prefetch_noops_without_remotes_like_stock_git`
- `git_maintenance_compat::maintenance_prefetch_local_remote_writes_prefetch_refs_like_stock_git`
- `git_maintenance_compat::maintenance_prefetch_unsupported_remote_helper_failure_matches_stock_git`

Expected movement:

- behavior rows: `+4`
- closed rows: `+3`
- open rows: `+0`
- invalid-input rows: `+1`
- represented oracle functions: `+3`
- missing-or-unclassified oracle functions: `-3`
- commands with rows: `+0`
- represented doc-option pairs: to be confirmed by generated summary because
  this extends existing `maintenance --task` coverage and may add the first
  `maintenance --quiet` row
- Rust behavior changes: no

Expected rows:

- `git maintenance run --task=prefetch` in a repository without remotes
- `git maintenance run --task=gc --task=prefetch --quiet` in a repository
  without remotes
- `git maintenance run --task=prefetch` with a local path remote
- `git maintenance run --task=prefetch` with an unsupported remote-helper URL

The evidence compares stock Git and Zmin command output for no-remote and
unsupported-helper cases, and compares `refs/prefetch` plus `fsck --strict`
state after local-remote prefetch.

Actual post-import movement matched the declaration: `+4` behavior rows,
`+3` closed rows, `+0` open rows, `+1` invalid-input row, `+3`
represented oracle functions, `-3` missing-or-unclassified oracle functions,
`+0` commands with rows and `+1` represented doc-option pair.

### Declared: Maintenance Run Local Tasks Batch

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`, current largest missing bucket
`git_maintenance_compat.rs`, maintenance run local task and schedule behavior
group.

Evidence functions:

- `git_maintenance_compat::maintenance_run_gc_reuses_repository_gc`
- `git_maintenance_compat::maintenance_run_local_tasks_create_stock_readable_metadata`
- `git_maintenance_compat::maintenance_run_schedule_matches_stock_git_strategy_selection`
- `git_maintenance_compat::maintenance_run_daily_prunes_packed_loose_duplicates_like_stock_git`
- `git_maintenance_compat::maintenance_run_weekly_packs_refs_like_stock_git`

Expected movement:

- behavior rows: `+6`
- closed rows: `+6`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+5`
- missing-or-unclassified oracle functions: `-5`
- commands with rows: `+0`
- represented doc-option pairs: expected `+0`; `maintenance --task`,
  `--schedule` and `--quiet` already have matrix rows
- Rust behavior changes: no

Expected rows:

- `git maintenance run --task=gc --quiet`
- `git maintenance run --task=commit-graph --task=pack-refs --task=loose-objects`
- `git maintenance run --schedule=hourly`
- `git maintenance run --schedule=daily --quiet` with incremental strategy
  metadata creation
- `git maintenance run --schedule=daily --quiet` pruning duplicate loose
  objects
- `git maintenance run --schedule=weekly --quiet`

The evidence checks stock-readable repository state after local maintenance
tasks: pack creation and reachable `HEAD`, commit-graph verification, packed
refs readability, multi-pack-index verification, duplicate loose-object pruning
and weekly packed-ref side effects.

Actual post-import movement matched the declaration: `+6` behavior rows,
`+6` closed rows, `+0` open rows, `+0` invalid-input rows, `+5`
represented oracle functions, `-5` missing-or-unclassified oracle functions,
`+0` commands with rows and `+0` represented doc-option pairs.

### Completed: Pull Local Rebase Batch

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence file:

- `git_transport_local_compat.rs`

Evidence functions:

- `git_transport_local_compat::pull_local_remote_fast_forwards_like_stock_git`
- `git_transport_local_compat::pull_rebase_local_remote_replays_local_commit_like_stock_git`
- `git_transport_local_compat::pull_rebase_interactive_applies_sequence_editor_drop_like_stock_git`
- `git_transport_local_compat::pull_rebase_interactive_reword_uses_commit_editor_like_stock_git`
- `git_transport_local_compat::pull_rebase_interactive_edit_stops_and_continue_matches_stock_git`
- `git_transport_local_compat::pull_rebase_interactive_edit_abort_restores_original_head_like_stock_git`
- `git_transport_local_compat::pull_rebase_merges_local_remote_preserves_merge_topology_like_stock_git`
- `git_transport_local_compat::pull_rebase_config_replays_local_commit_like_stock_git`
- `git_transport_local_compat::pull_branch_rebase_config_replays_local_commit_like_stock_git`
- `git_transport_local_compat::pull_rebase_false_overrides_config_like_stock_git`

Expected movement:

- behavior rows: `+10`
- closed rows: `+10`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+10`
- missing-or-unclassified oracle functions: `-10`
- commands with rows: `+1`
- represented doc-option pairs: to be confirmed by generated summary because
  this creates the first `pull` matrix and maps command/subcommand options
  against the Git docs seed
- Rust behavior changes: no

Expected rows:

- `git pull --ff-only`
- `git pull --rebase`
- `GIT_SEQUENCE_EDITOR=<drop> git pull --rebase=interactive`
- `GIT_SEQUENCE_EDITOR=<reword> GIT_EDITOR=<rewrite> git pull --rebase=interactive`
- `GIT_SEQUENCE_EDITOR=<edit> git pull --rebase=interactive` with
  `git rebase --continue` verification after the stop
- `GIT_SEQUENCE_EDITOR=<edit> git pull --rebase=interactive` followed by
  `git rebase --abort`
- `git pull --rebase=merges`
- `git -c pull.rebase=true pull`
- `git -c branch.main.rebase=true pull`
- `git -c pull.rebase=true pull --rebase=false`

The evidence compares stock Git and Zmin command output, resulting `HEAD`,
tree contents, commit subjects, merge-parent counts, rebase control
directories and `status --porcelain=v1 --branch` output across local/file
pull workflows.

Actual post-import movement matched the declaration: `+10` behavior rows,
`+10` closed rows, `+0` open rows, `+0` invalid-input rows, `+10`
represented oracle functions, `-10` missing-or-unclassified oracle functions,
`+1` command with rows and `+3` represented doc-option pairs.

### Completed: Maintenance Prefetch Transport Batch

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence file:

- `git_transport_http_compat.rs`

Evidence functions:

- `git_transport_http_compat::maintenance_prefetch_reads_dumb_http_remote_like_stock_git`
- `git_transport_http_compat::maintenance_prefetch_reads_smart_http_remote_like_stock_git`
- `git_transport_http_compat::maintenance_prefetch_reads_git_daemon_remote_like_stock_git`
- `git_transport_http_compat::maintenance_prefetch_reads_ssh_remote_like_stock_git`

Expected movement:

- behavior rows: `+4`
- closed rows: `+4`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+4`
- missing-or-unclassified oracle functions: `-4`
- commands with rows: `+0`
- represented doc-option pairs: expected `+0`; `maintenance --task` already
  has matrix rows
- Rust behavior changes: no

Expected rows:

- `git maintenance run --task=prefetch` with a dumb HTTP remote
- `git maintenance run --task=prefetch` with a smart HTTP remote
- `git maintenance run --task=prefetch` with a git-daemon remote
- `GIT_SSH_COMMAND=<fake-ssh> git maintenance run --task=prefetch` with an
  SSH remote

The evidence compares stock Git and Zmin `refs/prefetch` refnames/object IDs
and fetched object contents after maintenance prefetch over each transport.

Actual post-import movement matched the declaration: `+4` behavior rows,
`+4` closed rows, `+0` open rows, `+0` invalid-input rows, `+4`
represented oracle functions, `-4` missing-or-unclassified oracle functions,
`+0` commands with rows and `+0` represented doc-option pairs.

## Latest Declared Import

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions:

- `git_index_mutation_compat::add_resolves_unmerged_entries_preserving_disabled_mode_bits_like_stock_git`
- `git_index_mutation_compat::add_update_matches_stock_git_state`
- `git_index_mutation_compat::add_rejects_submodule_object_format_mismatch_like_upstream_git`

Expected movement:

- behavior rows: `+4`
- matching stock Git rows: `+2`
- open rows: `+0`
- invalid-input rows: `+2`
- represented oracle functions: `+3`
- missing-or-unclassified oracle functions: `-3`
- commands with rows: `+0`
- represented doc-option pairs: expected `+0`
- Rust behavior changes: no

Expected rows:

- `git add file symlink` with `core.filemode=false`,
  `core.symlinks=false` and unmerged file/symlink index stages
- `git add -u dir`
- `git add submodule` in a sha256 parent with a sha1 submodule
- `git add submodule` in a sha1 parent with a sha256 submodule

The evidence compares stock Git and Zmin index or status output for the
unmerged and update rows, and checks the upstream Git object-format mismatch
diagnostic shape for invalid submodule additions.

## Latest Declared Import

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions:

- `git_index_mutation_compat::rm_file_dir_and_cached_match_stock_git_state`
- `git_index_mutation_compat::rm_cached_recursive_root_pathspec_matches_stock_git`
- `git_index_mutation_compat::rm_common_options_match_stock_git`

Expected movement:

- behavior rows: `+9`
- matching stock Git rows: `+8`
- open rows: `+0`
- invalid-input rows: `+1`
- represented oracle functions: `+3`
- missing-or-unclassified oracle functions: `-3`
- commands with rows: `+0`
- represented doc-option pairs: expected `+6` for `rm -r`, `rm -n`,
  `rm -q`, `rm --ignore-unmatch`, `rm --pathspec-from-file` and
  `rm --pathspec-file-nul`
- Rust behavior changes: no

Expected rows:

- `git rm a.txt`
- `git rm -r dir`
- `git rm --cached cached.txt`
- `git rm --cached -r .`
- `git rm -n a.txt`
- `git rm -q a.txt`
- `git rm --ignore-unmatch missing.txt`
- `git rm --cached --pathspec-from-file paths.nul --pathspec-file-nul`
- `git rm --pathspec-file-nul`

The evidence compares stock Git and Zmin stdout/stderr, exit status, status
output, `ls-files` output and worktree side effects for file, directory,
cached, dry-run, quiet, ignore-unmatch and pathspec-file modes.

## Latest Declared Import

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions:

- `git_index_mutation_compat::add_autocrlf_warning_and_blob_normalization_match_stock_git`
- `git_index_mutation_compat::add_autocrlf_mixed_eol_over_binary_index_matches_stock_git`
- `git_index_mutation_compat::add_ignore_errors_stages_readable_siblings_like_stock_git`
- `git_index_mutation_compat::add_ignore_errors_config_stages_readable_siblings_like_stock_git`

Expected movement:

- behavior rows: `+5`
- matching stock Git rows: `+5`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+4`
- missing-or-unclassified oracle functions: `-4`
- commands with rows: `+0`
- represented doc-option pairs: expected `+1` for `add --ignore-errors`
- Rust behavior changes: no

Expected rows:

- `git -c core.autocrlf=true add LF`
- `git -c core.autocrlf=input add CRLF`
- `git -c core.autocrlf=true add file.txt` after a binary-index path is
  rewritten with mixed line endings
- `git add --ignore-errors .` with a readable and unreadable sibling
- `git add .` with `add.ignore-errors=true`

The evidence compares stock Git and Zmin autocrlf output, staged blob
normalization, status output and index contents for line-ending and
ignore-errors add behavior.

## Latest Declared Import

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions:

- `git_index_mutation_compat::add_escaped_bracket_pathspec_matches_literal_path_like_stock_git`
- `git_index_mutation_compat::add_resolves_conflict_on_ignored_path_like_stock_git`
- `git_index_mutation_compat::add_embedded_repository_warning_matches_stock_git`
- `git_index_mutation_compat::add_empty_embedded_repository_error_matches_stock_git`

Expected movement:

- behavior rows: `+4`
- matching stock Git rows: `+3`
- open rows: `+0`
- invalid-input rows: `+1`
- represented oracle functions: `+4`
- missing-or-unclassified oracle functions: `-4`
- commands with rows: `+0`
- represented doc-option pairs: expected `+0`
- Rust behavior changes: no

Expected rows:

- `git add 'fo\[ou\]bar'`
- `git add track-this` resolving an unmerged ignored path
- `git add .` with embedded repositories that have commits
- `git add empty` with an embedded repository that has no checked-out commit

The evidence compares stock Git and Zmin pathspec handling, index entries,
stdout/stderr, exit status and failure diagnostics for index mutation edge
cases.

## Latest Declared Import

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions:

- `git_index_mutation_compat::add_chmod_stages_mode_like_stock_git`
- `git_index_mutation_compat::add_chmod_dry_run_and_symlink_errors_match_stock_git`
- `git_index_mutation_compat::add_chmod_stages_regular_paths_when_non_regular_path_fails_like_stock_git`
- `git_index_mutation_compat::add_chmod_rejects_index_symlink_even_when_worktree_path_is_regular_like_stock_git`

Expected movement:

- behavior rows: `+6`
- matching stock Git rows: `+3`
- open rows: `+0`
- invalid-input rows: `+3`
- represented oracle functions: `+4`
- missing-or-unclassified oracle functions: `-4`
- commands with rows: `+0`
- represented doc-option pairs: expected `+1` for `add --chmod`
- Rust behavior changes: no

Expected rows:

- `git add --chmod=+x foo`
- `git add --chmod=-x foo`
- `git add --chmod=+x --dry-run foo`
- `git add --chmod=+x --dry-run link` for a symlink path
- `git add --chmod=+x link regular` with a symlink and a regular path
- `git add --chmod=+x link regular` when `core.symlinks=false` but the index
  entry remains a symlink

The evidence compares stock Git and Zmin index entries, dry-run output, exit
status and failure diagnostics for chmod staging and non-regular path
rejection behavior.

## Latest Declared Import

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions:

- `git_index_mutation_compat::add_dry_run_reports_without_mutating_index_like_stock_git`
- `git_index_mutation_compat::add_dry_run_allows_tracked_ignored_path_like_stock_git`
- `git_index_mutation_compat::add_dry_run_ignore_missing_reports_tracked_and_ignored_like_stock_git`

Expected movement:

- behavior rows: `+3`
- matching stock Git rows: `+3`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+3`
- missing-or-unclassified oracle functions: `-3`
- commands with rows: `+0`
- represented doc-option pairs: expected `+1` for `add --dry-run`
- Rust behavior changes: no

Expected rows:

- `git add --dry-run track-this` for a new file without mutating the index
- `git add --dry-run track-this` for a tracked path now ignored by `.gitignore`
- `git add --dry-run --ignore-missing track-this ignored-file`

The evidence compares stock Git and Zmin dry-run output, exit status and
index side effects where observable.

## Latest Declared Import

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions:

- `git_index_mutation_compat::add_all_stages_mode_change_with_unchanged_content_like_stock_git`
- `git_index_mutation_compat::add_preserves_index_symlink_mode_when_core_symlinks_false_like_stock_git`
- `git_index_mutation_compat::add_refresh_updates_stat_after_read_tree_like_stock_git`
- `git_index_mutation_compat::add_refresh_pathspec_leaves_other_stat_dirty_paths_like_stock_git`
- `git_index_mutation_compat::add_all_stages_same_size_rewrite_after_reset_like_stock_git`
- `git_index_mutation_compat::add_refresh_reports_unmatched_pathspec_like_stock_git`

Expected movement:

- behavior rows: `+6`
- matching stock Git rows: `+5`
- open rows: `+0`
- invalid-input rows: `+1`
- represented oracle functions: `+6`
- missing-or-unclassified oracle functions: `-6`
- commands with rows: `+0`
- represented doc-option pairs: expected `+1` for `add --refresh`
- Rust behavior changes: no

Expected rows:

- `git add -A` after an executable-mode-only worktree change
- `git add xfoo1` with `core.symlinks=false` preserving an index symlink mode
- `git add --refresh -- foo` after `read-tree HEAD`
- `git add --refresh bar` leaving another stat-dirty path dirty
- `git add -A` after reset and same-size tracked rewrite
- `git add --refresh nonexistent` as stock-compatible invalid input

The evidence compares stock Git and Zmin index entries, cached diffs,
diff-index/diff-files output or invalid-pathspec exit status and stderr.

## Latest Declared Import

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions:

- `git_index_mutation_compat::add_all_pathspec_limits_tracked_deletes_like_stock_git`
- `git_index_mutation_compat::add_force_stages_explicit_ignored_paths_like_stock_git`
- `git_index_mutation_compat::add_rejects_explicit_ignored_paths_without_force_like_stock_git`
- `git_index_mutation_compat::add_honors_nested_gitignore_negation_like_stock_git`
- `git_index_mutation_compat::add_respects_core_filemode_false_like_stock_git`

Expected movement:

- behavior rows: `+5`
- closed rows: `+5`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+5`
- missing-or-unclassified oracle functions: `-5`
- commands with rows: `+0`
- represented doc-option pairs: expected `+2` for `add -A` and `add -f`
- Rust behavior changes: no

Expected rows:

- `git add -A dir` after deleting tracked paths inside and outside `dir`
- `git add -f ignored.txt` for an explicitly ignored path
- `git add a.if a.ig` where one explicit path is ignored and rejected
- `git add sub/dir` with nested `.gitignore` negation
- `git -c core.filemode=false add script.sh` for an executable worktree file

The evidence compares stock Git and Zmin status, index entries, cached diffs or
command exit status for index mutation behavior.

## Latest Declared Import

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence function:

- `git_notes_compat::notes_edit_matches_stock_git_for_update_and_empty_remove`

Expected movement:

- behavior rows: `+2`
- closed rows: `+2`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+1`
- missing-or-unclassified oracle functions: `-1`
- commands with rows: `+0`
- represented doc-option pairs: expected `+0`; notes subcommand option rows
  are not represented in the current Git docs option seed
- Rust behavior changes: no

Expected rows:

- `GIT_EDITOR=edit-note.sh git notes edit HEAD` updates an existing note
- `GIT_EDITOR=edit-note.sh git notes edit HEAD` empties the note file and
  removes the note

The evidence compares stock Git and Zmin command output plus resulting note
contents/status for editor-backed update and empty-result removal flows.

Actual post-import movement matched the declaration: `+2` behavior rows,
`+2` closed rows, `+0` open rows, `+0` invalid-input rows, `+1`
represented oracle function, `-1` missing-or-unclassified oracle function,
`+0` commands with rows and `+0` represented doc-option pairs.

## Latest Declared Import

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions:

- `git_notes_compat::notes_reuse_message_matches_stock_git_for_add_and_append`
- `git_notes_compat::notes_reedit_message_matches_stock_git_for_add_and_append`

Expected movement:

- behavior rows: `+4`
- closed rows: `+4`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+2`
- missing-or-unclassified oracle functions: `-2`
- commands with rows: `+0`
- represented doc-option pairs: expected `+0`; notes subcommand option rows
  are not represented in the current Git docs option seed
- Rust behavior changes: no

Expected rows:

- `git notes add -m literal -C <blob> HEAD`
- `git notes append --reuse-message <blob> HEAD`
- `GIT_EDITOR=reedit-note.sh git notes add -c <blob> HEAD`
- `GIT_EDITOR=reedit-note.sh git notes append --reedit-message <blob> HEAD`

The evidence compares stock Git and Zmin command output plus resulting note
contents for reuse-message and reedit-message add/append flows.

Actual post-import movement matched the declaration: `+4` behavior rows,
`+4` closed rows, `+0` open rows, `+0` invalid-input rows, `+2`
represented oracle functions, `-2` missing-or-unclassified oracle functions,
`+0` commands with rows and `+0` represented doc-option pairs.

## Latest Declared Import

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions:

- `git_notes_compat::notes_copy_stdin_matches_stock_git_for_pair_stream`
- `git_notes_compat::notes_copy_stdin_no_stdin_toggles_match_stock_git_order`
- `git_notes_compat::notes_copy_for_rewrite_matches_stock_git_config_gate`

Expected movement:

- behavior rows: `+5`
- closed rows: `+5`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+3`
- missing-or-unclassified oracle functions: `-3`
- commands with rows: `+0`
- represented doc-option pairs: expected `+0`; notes subcommand option rows
  are not represented in the current Git docs option seed
- Rust behavior changes: no

Expected rows:

- `git notes copy --stdin` with an object-pair stream
- `git notes copy --stdin --no-stdin <from> <to>`
- `git notes copy --no-stdin --stdin` with an object-pair stream
- `git notes copy --for-rewrite=rebase` with rewrite ref config
- `git notes copy --for-rewrite rebase` with rewrite disabled by config

The evidence compares stock Git and Zmin command output plus destination note
contents or absence for stdin, no-stdin and for-rewrite copy flows.

Actual post-import movement matched the declaration: `+5` behavior rows,
`+5` closed rows, `+0` open rows, `+0` invalid-input rows, `+3`
represented oracle functions, `-3` missing-or-unclassified oracle functions,
`+0` commands with rows and `+0` represented doc-option pairs.

## Previous Declared Import

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence function:

- `git_commit_compat::commit_trailers_match_stock_git_object_and_editor_buffer`

Expected movement:

- behavior rows: `+5`
- closed rows: `+5`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+1`
- missing-or-unclassified oracle functions: `-1`
- commands with rows: `+0`
- represented doc-option pairs: `+2`
- Rust behavior changes: no

Expected rows:

- `git commit -m subject --trailer ...`
- `git commit -F message.txt --trailer ...`
- `git commit -m subject --trailer ... --trailer ...`
- `git commit --signoff -m subject --trailer ...`
- editor-backed `git commit --trailer ...`

The evidence compares stock Git and Zmin commit objects, command output and
editor input buffers for trailer insertion flows.

Actual post-import movement matched the declaration: `+5` behavior rows,
`+5` closed rows, `+0` open rows, `+0` invalid-input rows, `+1`
represented oracle function, `-1` missing-or-unclassified oracle function,
`+0` commands with rows and `+2` represented doc-option pairs.

## Previous Declared Import

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence function:

- `git_commit_compat::commit_cleanup_modes_match_stock_git_object`

Expected movement:

- behavior rows: `+4`
- closed rows: `+4`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+1`
- missing-or-unclassified oracle functions: `-1`
- commands with rows: `+0`
- Rust behavior changes: no

Expected rows:

- `git commit --cleanup strip -F message.txt`
- `git commit --cleanup whitespace -F message.txt`
- `git commit --cleanup default -F message.txt`
- `git commit --no-cleanup -F message.txt`

The evidence compares stock Git and Zmin commit objects for each cleanup mode.

Actual post-import movement matched the declaration: `+4` behavior rows,
`+4` closed rows, `+0` open rows, `+0` invalid-input rows, `+1`
represented oracle function, `-1` missing-or-unclassified oracle function,
`+0` commands with rows and `+1` represented doc-option pair.

## Previous Declared Import

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence function:

- `git_commit_compat::commit_summary_and_quiet_output_match_stock_git`

Expected movement:

- behavior rows: `+4`
- closed rows: `+4`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+1`
- missing-or-unclassified oracle functions: `-1`
- commands with rows: `+0`
- Rust behavior changes: no

Expected rows:

- `git commit -m initial` root commit summary output
- `git commit -m second` update summary output
- `git commit -m delete` deletion summary output
- `git commit --quiet -m quiet`

The evidence compares stock Git and Zmin stdout/stderr/exit behavior for root,
update, deletion and quiet commit flows.

Actual post-import movement matched the declaration: `+4` behavior rows,
`+4` closed rows, `+0` open rows, `+0` invalid-input rows, `+1`
represented oracle function, `-1` missing-or-unclassified oracle function,
`+0` commands with rows and `+1` represented doc-option pair.

## Previous Declared Import

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence function:

- `git_commit_compat::commit_edit_and_no_edit_message_sources_match_stock_git`

Expected movement:

- behavior rows: `+5`
- closed rows: `+5`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+1`
- missing-or-unclassified oracle functions: `-1`
- commands with rows: `+0`
- Rust behavior changes: no

Expected rows:

- `git commit -e -m "Msg subject" -m "Msg body"`
- `git commit -e -F msg.txt`
- `git commit -e -C HEAD`
- `git commit --no-edit -c HEAD`
- `git commit --no-edit -m "No edit subject"`

The evidence compares stock Git and Zmin commit objects, command output,
editor invocation and editor input buffers for edit/no-edit message-source
flows.

Actual post-import movement matched the declaration: `+5` behavior rows,
`+5` closed rows, `+0` open rows, `+0` invalid-input rows, `+1`
represented oracle function, `-1` missing-or-unclassified oracle function,
`+0` commands with rows and `+2` represented doc-option pairs.

## Previous Declared Import

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence function:

- `git_commit_compat::commit_reuse_message_matches_stock_git_object`

Expected movement:

- behavior rows: `+4`
- closed rows: `+4`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+1`
- missing-or-unclassified oracle functions: `-1`
- commands with rows: `+0`
- Rust behavior changes: no

Expected rows:

- `git commit -C HEAD`
- `git commit -C HEAD~1 --author ... --date ...`
- `git commit -c HEAD~2`
- `git commit -c HEAD~3`

The evidence compares stock Git and Zmin commit objects, command output and
editor input buffers for reuse and reedit message flows.

Actual post-import movement matched the declaration: `+4` behavior rows,
`+4` closed rows, `+0` open rows, `+0` invalid-input rows, `+1`
represented oracle function, `-1` missing-or-unclassified oracle function and
`+0` commands with rows.

## Previous Declared Import

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence function:

- `git_commit_compat::commit_messages_match_stock_git_object`

Expected movement:

- behavior rows: `+4`
- closed rows: `+4`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+1`
- missing-or-unclassified oracle functions: `-1`
- commands with rows: `+0`
- Rust behavior changes: no

Expected rows:

- `git commit -m subject -m body`
- `git commit -F message.txt`
- `git commit --allow-empty-message -m ""`
- `git commit --amend -m amended -m details`

The evidence compares stock Git and Zmin commit objects for multiple `-m`
paragraphs, file-backed messages, empty-message commits and amend commits with
multiple message paragraphs.

Actual post-import movement matched the declaration: `+4` behavior rows,
`+4` closed rows, `+0` open rows, `+0` invalid-input rows, `+1`
represented oracle function, `-1` missing-or-unclassified oracle function and
`+0` commands with rows.

## Earlier Declared Import

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence function:

- `git_commit_compat::commit_pathspec_only_and_fixup_match_stock_git_state`

Expected movement:

- behavior rows: `+7`
- closed rows: `+7`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+1`
- missing-or-unclassified oracle functions: `-1`
- commands with rows: `+0`
- Rust behavior changes: no

Expected rows:

- `git commit -m base`
- `git commit -m path -- a.txt`
- `git commit --only -m "only path" a.txt`
- `git commit --fixup HEAD`
- `git commit --fixup HEAD -m "fixup detail"`
- `git commit --fixup=amend:HEAD`
- `git commit --fixup=reword:HEAD`

The evidence compares stock Git and Zmin commit objects, command output,
editor input buffers, porcelain status and index state for pathspec-only and
autosquash fixup commit flows.

Actual post-import movement matched the declaration: `+7` behavior rows,
`+7` closed rows, `+0` open rows, `+0` invalid-input rows, `+1`
represented oracle function, `-1` missing-or-unclassified oracle function and
`+0` commands with rows.

## Previous Declared Import

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions:

- `git_commit_compat::commit_amend_matches_stock_git_state`
- `git_commit_compat::commit_dot_pathspec_matches_stock_git_state`

Expected movement:

- behavior rows: `+6`
- closed rows: `+6`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+2`
- missing-or-unclassified oracle functions: `-2`
- commands with rows: `+1`
- Rust behavior changes: no

Expected rows:

- `git commit -m initial`
- `git commit --amend -m amended`
- `git commit --amend -m message only`
- `git commit --amend --no-edit`
- `git commit --amend` with `GIT_EDITOR=:`
- `git commit -m "add attrs" .`

The evidence compares stock Git and Zmin commit objects, parent shape, commit
count, porcelain status and index state for amend and dot-pathspec commit
flows. Broader pathspec/fixup/editor rows from the same test file stay
unimported until a separate batch.

Actual post-import movement matched the declaration: `+6` behavior rows,
`+6` closed rows, `+0` open rows, `+0` invalid-input rows, `+2`
represented oracle functions, `-2` missing-or-unclassified oracle functions
and `+1` command with rows.

## Previous Inventory Classification Fix

Source bucket: focused stock-oracle inventory tooling for evidence cells that
already contain multiple test references.

The previous inventory script treated an entire TSV evidence cell as one key.
Rows such as
`git_status_compat::status_porcelain_matches_stock_git_for_clean_dirty_and_ignored_worktrees; git_status_compat::status_porcelain_v2_matches_stock_git_for_staged_states`
therefore failed to credit the second evidence function. The inventory now
extracts every `module::test` reference from matrix rows and classification
docs.

Expected movement:

- behavior rows: `+0`
- closed rows: `+0`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+3`
- missing-or-unclassified oracle functions: `-3`
- Rust behavior changes: no

Reclassified functions:

- `git_global_cli_compat::rev_parse_show_prefix_inside_git_dir_matches_stock_git`
- `git_global_cli_compat::rev_parse_core_bare_config_affects_discovery_flags`
- `git_status_compat::status_porcelain_v2_matches_stock_git_for_staged_states`

Actual post-fix movement matched the declaration: inventory represented
functions moved from `500` to `503`, and `missing_or_unclassified` moved from
`461` to `458`. Written behavior rows stayed `2354`.

## Previous Declared Import

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence function:
`git_stash_compat::stash_apply_pop_branch_reject_too_many_refs_like_stock_git`
in `crates/zmin-cli/tests/git_stash_compat.rs`.

Expected delta:

- behavior rows: `+4`
- closed rows: `+0`
- open rows: `+0`
- invalid-input rows: `+4`
- represented oracle functions: `+1`
- Rust behavior changes: no

Expected rows:

- `git stash apply stash@{0} stash@{1}`
- `git stash pop stash@{0} stash@{1}`
- `git stash show stash@{0} stash@{1}`
- `git stash branch stash-branch stash@{0} stash@{1}`

The evidence compares stock Git and Zmin failure output and verifies the dirty
worktree file is unchanged after each rejected invocation.

Actual post-import movement matched the declaration: `+4` behavior rows,
`+0` closed rows, `+0` open rows, `+4` invalid-input rows and `+1`
represented oracle function.

## Previous Declared Import

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence function:
`git_stash_compat::stash_push_apply_pop_matches_stock_git_state` in
`crates/zmin-cli/tests/git_stash_compat.rs`.

Expected delta:

- behavior rows: `+5`
- closed rows: `+5`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+1`
- Rust behavior changes: no

Expected rows:

- `git stash push -m work`
- `git stash list`
- `git stash clear`
- `git stash apply`
- `git stash pop`

The evidence compares stock Git and Zmin for the default tracked-change stash
flow, including clean worktree/index after push, stash list/rev side effects,
clearing the stack, applying a stash without dropping it and popping a stash
while emptying the stack.

Actual post-import movement matched the declaration: `+5` behavior rows,
`+5` closed rows, `+0` open rows, `+0` invalid-input rows and `+1`
represented oracle function.

## Previous Declared Import

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence function:
`git_stash_compat::stash_pop_drops_selected_stack_entry_like_stock_git` in
`crates/zmin-cli/tests/git_stash_compat.rs`.

Expected delta:

- behavior rows: `+5`
- closed rows: `+5`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+1`
- Rust behavior changes: no

Expected rows:

- `git stash pop stash@{1}`
- `git stash push --message=custom --no-message`
- `git stash push --no-message --message=after-no`
- `git stash push -m no-pathspec-file --pathspec-from-file=paths.txt --no-pathspec-from-file`
- `git stash push -m no-pathspec-nul --pathspec-file-nul --no-pathspec-file-nul --pathspec-from-file=paths.txt`

The evidence compares stock Git and Zmin for selected stash-pop stack updates,
last-option-wins message/no-message push behavior, and negated pathspec file
mode behavior while checking worktree status and stash show output against
stock Git.

Actual post-import movement matched the declaration: `+5` behavior rows,
`+5` closed rows, `+0` open rows, `+0` invalid-input rows and `+1`
represented oracle function.

## Previous Declared Import

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions in `crates/zmin-cli/tests/git_stash_compat.rs`:

- `git_stash_compat::stash_preserves_missing_skip_worktree_entry_like_stock_git`
- `git_stash_compat::stash_uses_git_stash_identity_when_user_identity_is_missing_like_stock_git`
- `git_stash_compat::stash_push_captures_same_size_rewrite_after_reset_like_stock_git`
- `git_stash_compat::stash_pop_rejects_overlapping_dirty_paths_like_stock_git`
- `git_stash_compat::stash_apply_and_drop_selected_stack_entry_match_stock_git`
- `git_stash_compat::stash_save_rm_then_recreate_matches_stock_git`
- `git_stash_compat::stash_save_file_to_directory_matches_stock_git`

Expected delta:

- behavior rows: `+7`
- closed rows: `+7`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+7`
- Rust behavior changes: no

Expected rows:

- `git stash` with a missing skip-worktree tracked entry
- `git stash` without configured user identity, using `git stash <git@stash>`
- `git stash; git stash drop stash@{1}; git stash apply` after same-size rewrites
- `git stash pop` rejected by an overlapping dirty path
- `git stash apply stash@{1}; git stash drop 1`
- `git stash save "rm then recreate"; git stash apply`
- `git stash save "file to directory"; git stash apply`

The evidence compares stock Git and Zmin for stash commit contents, fallback
stash identity, repeated rewrite capture, dirty-path rejection, selected stack
entry operations and legacy `save` path-shape restoration.

Actual post-import movement matched the declaration: `+7` behavior rows,
`+7` closed rows, `+0` open rows, `+0` invalid-input rows and `+7`
represented oracle functions.

## Previous Declared Import

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions in `crates/zmin-cli/tests/git_stash_compat.rs`:

- `git_stash_compat::stash_rejects_intent_to_add_entries_like_stock_git`
- `git_stash_compat::stash_patch_selects_hunks_and_leaves_rejected_hunks_like_stock_git`
- `git_stash_compat::stash_patch_all_done_quit_and_pathspec_match_stock_git`
- `git_stash_compat::stash_patch_split_pathspec_restores_selected_hunk_like_stock_git`

Expected delta:

- behavior rows: `+6`
- closed rows: `+5`
- open rows: `+0`
- invalid-input rows: `+1`
- represented oracle functions: `+4`
- Rust behavior changes: no

Expected rows:

- `git add --intent-to-add file4; git stash`
- `printf 'y\nn\n' | git stash push --patch -m patchy`
- `printf 'a\n' | git stash push --patch -m all-a -- a.txt`
- `printf 'd\n' | git stash push --patch -m done`
- `printf 'q\ny\n' | git stash push --patch -m quit`
- `printf 's\ny\nn\n' | git stash push -m "stash bar" --patch file`

The evidence compares stock Git and Zmin for intent-to-add rejection,
interactive patch hunk selection, pathspec-limited all selection, done/quit
commands, split hunk selection, stash list/show output, exit status and
worktree file contents.

Actual post-import movement matched the declaration: `+6` behavior rows,
`+5` closed rows, `+0` open rows, `+1` invalid-input row and `+4`
represented oracle functions.

## Previous Declared Import

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions in `crates/zmin-cli/tests/git_reflog_compat.rs`:

- `git_reflog_compat::reflog_show_list_and_exists_match_stock_git`
- `git_reflog_compat::reflog_show_date_modes_match_stock_git`
- `git_reflog_compat::reflog_show_passes_pathspec_after_double_dash`

Expected delta:

- behavior rows: `+7`
- closed rows: `+7`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+3`
- Rust behavior changes: no

Expected rows:

- `git reflog`
- `git reflog show refs/heads/main --format=%H`
- `git reflog list`
- `git reflog exists HEAD`
- `GIT_TEST_DATE_NOW=1700003600 git reflog --date=<common-mode>`
- `git reflog show -- --does-not-exist`
- `git reflog show -- --a-file`

The evidence compares stock Git and Zmin for default reflog show output,
formatted ref output, reflog listing, exists exit status, date rendering under
a fixed oracle clock and double-dash pathspec filtering.

Actual post-import movement matched the declaration: `+7` behavior rows,
`+7` closed rows, `+0` open rows, `+0` invalid-input rows and `+3`
represented oracle functions.

## Previous Declared Import

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions in `crates/zmin-cli/tests/git_reflog_compat.rs`:

- `git_reflog_compat::reset_hard_records_branch_and_head_reflog`
- `git_reflog_compat::commit_records_branch_and_head_reflog`
- `git_reflog_compat::branch_create_reflog_handles_nested_branch_names`
- `git_reflog_compat::log_reflog_orphan_checkout_uses_contiguous_commit_ordinals`

Expected delta:

- behavior rows: `+4`
- closed rows: `+4`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+4`
- Rust behavior changes: no

Expected rows:

- `git reset --hard HEAD~1; git reflog show main`
- `git commit --allow-empty -m one; git commit --allow-empty -m two; git reflog show main`
- `git branch one/two main; git log -g --format=%gd\ %gs one/two`
- `git log -g --format=%gd\ %gs HEAD` after orphan checkout commits

The evidence compares stock Git and Zmin for reflog entries produced by reset,
commit and branch creation plus `log -g` ordinal display after orphan-checkout
reflog entries. The hidden zero-oid reflog row from
`git_reflog_compat::log_reflog_keeps_ordinals_for_hidden_zero_oid_entries` was
already represented in `log_v2_47.tsv`, so it is intentionally not counted in
this import batch.

Actual post-import movement matched the corrected declaration: `+4` behavior
rows, `+4` closed rows, `+0` open rows, `+0` invalid-input rows and `+4`
represented oracle functions.

## Previous Declared Import

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions in `crates/zmin-cli/tests/git_reflog_compat.rs`:

- `git_reflog_compat::reflog_expire_dry_run_does_not_touch_reflog`
- `git_reflog_compat::reflog_expire_default_current_entries_match_stock_git`
- `git_reflog_compat::reflog_expire_pattern_config_matches_stock_git`

Expected delta:

- behavior rows: `+3`
- closed rows: `+3`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+3`
- Rust behavior changes: no

Expected rows:

- `git reflog expire --dry-run main`
- `git reflog expire`, `git reflog expire main`, `git reflog expire HEAD`,
  `git reflog expire --updateref main`, `git reflog expire --rewrite main`
  and `git reflog expire --verbose main`
- `git reflog expire root1/branch1 root1/branch2 root2/branch1 root2/branch2`
  with per-pattern `gc.<pattern>.reflogExpire` config

The evidence compares stock Git and Zmin for `reflog expire` stdout/stderr,
exit status, resulting HEAD/main reflogs, dry-run non-mutation and per-pattern
expiry config side effects. The dry-run focused test was tightened in this
slice to compare stock and Zmin command output directly before counting the
row as represented.

Actual post-import movement matched the declaration: `+3` behavior rows,
`+3` closed rows, `+0` open rows, `+0` invalid-input rows and `+3`
represented oracle functions.

## Previous Declared Import

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions in `crates/zmin-cli/tests/git_pack_integrity_compat.rs`:

- `git_pack_integrity_compat::fsck_missing_email_severity_config_matches_stock_git`
- `git_pack_integrity_compat::fsck_bad_email_severity_config_matches_stock_git`
- `git_pack_integrity_compat::fsck_missing_author_severity_config_matches_stock_git`
- `git_pack_integrity_compat::fsck_missing_committer_severity_config_matches_stock_git`
- `git_pack_integrity_compat::fsck_missing_tagger_entry_severity_config_matches_stock_git`

Expected delta:

- behavior rows: `+5`
- closed rows: `+0`
- open rows: `+0`
- invalid-input rows: `+5`
- represented oracle functions: `+5`
- commands with rows: `+1`
- Rust behavior changes: no

Expected rows:

- `git -c fsck.missingEmail=bogus fsck`
- `git -c fsck.badEmail=bogus fsck`
- `git -c fsck.missingAuthor=bogus fsck`
- `git -c fsck.missingCommitter=bogus fsck`
- `git -c fsck.missingTaggerEntry=bogus fsck`

The evidence compares stock Git and Zmin stdout/stderr and exit status for
invalid `fsck.<message>` severity config values against malformed commit and
tag objects. The accepted severity values from the same tests are not counted
in this batch; they need separate closed rows to avoid mixing accepted and
invalid-input statuses.

Actual post-import movement matched the declaration: `+5` behavior rows,
`+0` closed rows, `+0` open rows, `+5` invalid-input rows, `+5` represented
oracle functions and `+1` command with rows.

## Previous Declared Import

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions in `crates/zmin-cli/tests/git_pack_integrity_compat.rs`:

- `git_pack_integrity_compat::fsck_bad_tag_name_severity_config_matches_stock_git`
- `git_pack_integrity_compat::fsck_bad_date_tagger_severity_config_matches_stock_git`
- `git_pack_integrity_compat::fsck_missing_email_tagger_severity_config_matches_stock_git`
- `git_pack_integrity_compat::fsck_bad_email_tagger_severity_config_matches_stock_git`
- `git_pack_integrity_compat::fsck_missing_name_before_email_tagger_severity_config_matches_stock_git`

Expected delta:

- behavior rows: `+5`
- closed rows: `+0`
- open rows: `+0`
- invalid-input rows: `+5`
- represented oracle functions: `+5`
- commands with rows: `+0`
- Rust behavior changes: no

Expected rows:

- `git -c fsck.badTagName=bogus fsck`
- `git -c fsck.badDate=bogus fsck`
- `git -c fsck.missingEmail=bogus fsck` for a malformed tagger identity
- `git -c fsck.badEmail=bogus fsck` for a malformed tagger identity
- `git -c fsck.missingNameBeforeEmail=bogus fsck`

The evidence compares stock Git and Zmin stdout/stderr and exit status for
invalid `fsck.<message>` severity config values against malformed tag objects.
Accepted severity values from the same tests remain uncounted until they get
separate closed rows.

Actual post-import movement matched the declaration: `+5` behavior rows,
`+0` closed rows, `+0` open rows, `+5` invalid-input rows, `+5` represented
oracle functions and `+0` commands with rows.

## Previous Declared Import

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions in `crates/zmin-cli/tests/git_pack_integrity_compat.rs`:

- `git_pack_integrity_compat::fsck_missing_space_before_email_tagger_severity_config_matches_stock_git`
- `git_pack_integrity_compat::fsck_missing_space_before_date_tagger_severity_config_matches_stock_git`
- `git_pack_integrity_compat::fsck_zero_padded_date_tagger_severity_config_matches_stock_git`
- `git_pack_integrity_compat::fsck_bad_date_severity_config_matches_stock_git`
- `git_pack_integrity_compat::fsck_bad_timezone_severity_config_matches_stock_git`

Expected delta:

- behavior rows: `+5`
- closed rows: `+0`
- open rows: `+0`
- invalid-input rows: `+5`
- represented oracle functions: `+5`
- commands with rows: `+0`
- Rust behavior changes: no

Expected rows:

- `git -c fsck.missingSpaceBeforeEmail=bogus fsck`
- `git -c fsck.missingSpaceBeforeDate=bogus fsck`
- `git -c fsck.zeroPaddedDate=bogus fsck`
- `git -c fsck.badDate=bogus fsck` for a malformed author date
- `git -c fsck.badTimezone=bogus fsck`

The evidence compares stock Git and Zmin stdout/stderr and exit status for
invalid `fsck.<message>` severity config values against malformed tagger date,
tagger identity, author date and author timezone objects. Accepted severity
values from the same tests remain uncounted until they get separate closed
rows.

Actual post-import movement matched the declaration: `+5` behavior rows,
`+0` closed rows, `+0` open rows, `+5` invalid-input rows, `+5` represented
oracle functions and `+0` commands with rows.

## Previous Declared Import

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence function:

- `git_commit_compat::commit_status_option_and_config_match_stock_git_buffer`

Expected movement:

- behavior rows: `+4`
- closed rows: `+4`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+1`
- missing-or-unclassified oracle functions: `-1`
- commands with rows: `+0`
- represented doc-option pairs: expected `+3` if `--status`, `--no-status`
  and `-v` were not represented for `commit`
- Rust behavior changes: no

Expected rows:

- `GIT_EDITOR=.git/editor.sh git commit --no-status`
- `GIT_EDITOR=.git/editor.sh git commit` with `commit.status=false`
- `GIT_EDITOR=.git/editor.sh git commit --status` with
  `commit.status=false`
- `GIT_EDITOR=.git/editor.sh git commit -v --no-status`

The evidence compares stock Git and Zmin command output, captured editor
buffer and resulting commit object for status-template suppression, explicit
status override, config-driven status suppression and verbose/no-status
combination behavior.

Actual post-import movement matched the declaration: `+4` behavior rows,
`+4` closed rows, `+0` open rows, `+0` invalid-input rows, `+1`
represented oracle function, `-1` missing-or-unclassified oracle function,
`+0` commands with rows and `+3` represented doc-option pairs.

## Previous Declared Import

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions in `crates/zmin-cli/tests/git_commit_compat.rs`:

- `git_commit_compat::commit_template_editor_matches_stock_git_object`
- `git_commit_compat::commit_editor_without_message_matches_stock_git_buffer`
- `git_commit_compat::commit_editor_empty_and_unchanged_abort_like_stock_git`
- `git_commit_compat::commit_verbose_template_editor_matches_stock_git_buffer`
- `git_commit_compat::commit_verbose_verbose_template_editor_matches_stock_git_buffer`
- `git_commit_compat::commit_template_requires_editor_change_like_stock_git`

Expected movement:

- behavior rows: `+8`
- closed rows: `+5`
- open rows: `+0`
- invalid-input rows: `+3`
- represented oracle functions: `+6`
- missing-or-unclassified oracle functions: `-6`
- commands with rows: `+0`
- represented doc-option pairs: expected `+1` for `--template`; `-vv` is a
  value/spelling form of the existing verbose option seed rather than a
  separate documented option pair
- Rust behavior changes: no

Expected rows:

- `GIT_EDITOR=./editor.sh git commit --template template.txt`
- `GIT_EDITOR=./editor.sh git commit` with `commit.template` configured
- `GIT_EDITOR=.git/editor.sh git commit`
- `GIT_EDITOR=.git/editor.sh git commit` with unchanged editor buffer
- `GIT_EDITOR=.git/editor.sh git commit` with empty editor buffer
- `GIT_EDITOR=.git/editor.sh git commit --template .git/template.txt -v`
- `GIT_EDITOR=.git/editor.sh git commit --template .git/template.txt -vv`
- `GIT_EDITOR=true git commit --template template.txt`

The evidence compares stock Git and Zmin command output, captured editor
buffers, resulting commit objects and failure output for editor-backed commits,
template messages, verbose template buffers and unchanged or empty editor
abort behavior.

Actual post-import movement matched the declaration: `+8` behavior rows,
`+5` closed rows, `+0` open rows, `+3` invalid-input rows, `+6`
represented oracle functions, `-6` missing-or-unclassified oracle functions,
`+0` commands with rows and `+1` represented doc-option pair.

## Previous Declared Import

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions in `crates/zmin-cli/tests/git_commit_compat.rs`:

- `git_commit_compat::commit_long_message_option_matches_stock_git_object`
- `git_commit_compat::commit_author_matches_stock_git_object`
- `git_commit_compat::commit_date_matches_stock_git_object`
- `git_commit_compat::commit_amend_author_date_options_match_stock_git_object`
- `git_commit_compat::commit_reset_author_matches_stock_git_object`
- `git_commit_compat::commit_signoff_matches_stock_git_object`
- `git_commit_compat::commit_squash_matches_stock_git_object`

Expected movement:

- behavior rows: `+8`
- closed rows: `+8`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+7`
- missing-or-unclassified oracle functions: `-7`
- commands with rows: `+0`
- represented doc-option pairs: expected up to `+5` for `--message`,
  `--author`, `--date`, `--reset-author` and `--squash`
- Rust behavior changes: no

Expected rows:

- `git commit --allow-empty --message empty`
- `git commit --author "Alice Example <alice@example.test>" -m author`
- `git commit --date "1700001234 +0000" -m date`
- `git commit --amend --author "Alice Example <alice@example.test>" --date "1700001234 +0000" -m amended`
- `git commit --amend --reset-author -m reset`
- `git commit --allow-empty-message --signoff -m ""`
- `git commit --signoff -m subject -m details`
- `git commit --squash HEAD~1 -m work`

The evidence compares stock Git and Zmin commit objects and, for the author
row, log-rendered author identity for long message spelling, explicit
author/date metadata, amend metadata overrides, reset-author behavior, signoff
messages and squash messages.

Actual post-import movement matched the declaration: `+8` behavior rows,
`+8` closed rows, `+0` open rows, `+0` invalid-input rows, `+7`
represented oracle functions, `-7` missing-or-unclassified oracle functions,
`+0` commands with rows and `+5` represented doc-option pairs.

## Previous Declared Import

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions in `crates/zmin-cli/tests/git_commit_compat.rs`:

- `git_commit_compat::commit_hooks_match_stock_git_flow`
- `git_commit_compat::commit_prepare_and_post_rewrite_hooks_match_stock_git_flow`

Expected movement:

- behavior rows: `+4`
- closed rows: `+4`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+2`
- missing-or-unclassified oracle functions: `-2`
- commands with rows: `+0`
- represented doc-option pairs: expected `+1` if `--no-verify` was not
  represented for `commit`
- Rust behavior changes: no

Expected rows:

- `git commit -m subject` with successful `pre-commit`, `commit-msg` and
  `post-commit` hooks
- `git commit --no-verify -m skip` with verification hooks skipped
- `git commit --no-verify -m subject` with `prepare-commit-msg` still running
- `git commit --amend -m amended` with `post-rewrite` running

The evidence compares stock Git and Zmin command output, hook logs and commit
objects for successful hook execution, no-verify behavior,
prepare-commit-msg and post-rewrite amend behavior.

Actual post-import movement matched the declaration: `+4` behavior rows,
`+4` closed rows, `+0` open rows, `+0` invalid-input rows, `+2`
represented oracle functions, `-2` missing-or-unclassified oracle functions,
`+0` commands with rows and `+1` represented doc-option pair.

## Latest Declared Import

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions:

- `git_transport_local_compat::ls_remote_matches_stock_git_for_local_remotes`
- `git_transport_local_compat::ls_remote_reads_local_gitfile_repository_like_stock_git`

Expected movement:

- behavior rows: `+8`
- closed rows: `+8`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+2`
- missing-or-unclassified oracle functions: `-2`
- commands with rows: `+0`
- represented doc-option pairs: expected `+0`; `ls-remote` repository,
  `--heads`, `--tags` and `--refs` already have rows
- Rust behavior changes: no

Expected rows:

- `git ls-remote origin`
- `git ls-remote --heads origin`
- `git ls-remote --tags origin`
- `git ls-remote --refs origin`
- `git ls-remote origin main v*`
- `git ls-remote <local-bare-path>`
- `git ls-remote file://<local-bare-path>`
- `git ls-remote <local-gitfile-worktree>`

The evidence compares stock Git and Zmin stdout for named local remotes,
direct local paths, file URLs, multi-pattern filtering and repositories whose
`.git` entry is a gitfile pointing to the real Git directory.

Actual post-import movement matched the declaration: `+8` behavior rows, `+8`
closed rows, `+0` open rows, `+0` invalid-input rows, `+2` represented oracle
functions, `-2` missing-or-unclassified oracle functions, `+0` commands with
rows and `+0` represented doc-option pairs.

## Latest Declared Import

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions:

- `git_worktree_state_compat::checkout_dot_pathspec_restores_root_like_stock_git`
- `git_worktree_state_compat::checkout_dot_reports_updated_paths_like_stock_git`
- `git_worktree_state_compat::checkout_separator_pathspec_omits_updated_paths_like_stock_git`
- `git_worktree_state_compat::checkout_recurse_submodules_flag_keeps_dot_pathspec_like_stock_git`

Expected movement:

- behavior rows: `+4`
- closed rows: `+4`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+4`
- missing-or-unclassified oracle functions: `-4`
- commands with rows: `+0`
- represented doc-option pairs: expected `+1`; checkout pathspec rows now
  represent one more documented option pair
- Rust behavior changes: no

Expected rows:

- `git checkout --quiet --no-progress .`
- `git checkout .`
- `git checkout -- a.txt`
- `git checkout --recurse-submodules .`

The evidence compares stock Git and Zmin worktree restoration and updated-path
reporting for dot pathspec checkout, separator pathspec checkout and
`--recurse-submodules` dot pathspec handling.

Actual post-import movement matched the declaration: `+4` behavior rows, `+4`
closed rows, `+0` open rows, `+0` invalid-input rows, `+4` represented oracle
functions, `-4` missing-or-unclassified oracle functions, `+0` commands with
rows and `+1` represented doc-option pair.

## Latest Declared Import

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions:

- `git_transport_http_compat::http_backend_info_refs_matches_stock_git_smart_discovery_refs`
- `git_transport_http_compat::http_backend_resolves_scriptalias_path_translated_like_stock_git`
- `git_transport_http_compat::http_backend_serves_scriptalias_non_bare_repo_like_stock_git`
- `git_transport_http_compat::http_backend_upload_pack_post_returns_stock_readable_pack`

Expected movement:

- behavior rows: `+4`
- closed rows: `+4`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+4`
- missing-or-unclassified oracle functions: `-4`
- commands with rows: `+0`
- represented doc-option pairs: expected `+0`; `http-backend` already has
  represented doc-option rows
- Rust behavior changes: no

Expected rows:

- `GIT_PROJECT_ROOT=<root> PATH_INFO=/remote.git/info/refs?service=git-upload-pack git http-backend`
- `GIT_PROJECT_ROOT=<root> PATH_TRANSLATED=<root>/remote.git/info/refs git http-backend`
- `GIT_PROJECT_ROOT=<root> PATH_TRANSLATED=<root>/server/info/refs git http-backend`
- `GIT_PROJECT_ROOT=<root> PATH_INFO=/remote.git/git-upload-pack REQUEST_METHOD=POST git http-backend`

The evidence compares stock Git and Zmin smart HTTP advertised ref lines for
normal and ScriptAlias discovery paths, and verifies that the upload-pack POST
response includes stock-readable sideband pack data, acknowledges the common
`have`, includes delta objects and omits common base objects.

Actual post-import movement matched the declaration: `+4` behavior rows, `+4`
closed rows, `+0` open rows, `+0` invalid-input rows, `+4` represented oracle
functions, `-4` missing-or-unclassified oracle functions, `+0` commands with
rows and `+0` represented doc-option pairs.

## Latest Declared Import

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence function:

- `git_notes_compat::notes_add_list_show_remove_match_stock_git`

Expected movement:

- behavior rows: `+10`
- closed rows: `+9`
- open rows: `+0`
- invalid-input rows: `+1`
- represented oracle functions: `+1`
- missing-or-unclassified oracle functions: `-1`
- commands with rows: `+0`
- represented doc-option pairs: to be confirmed by generated summary because
  `notes` subcommands and options are seeded separately
- Rust behavior changes: no

Expected rows:

- `git notes add -m "note text" HEAD`
- `git notes get-ref`
- `git notes list`
- `git notes show HEAD`
- `git notes remove HEAD`
- `git notes show HEAD` after note removal
- `git notes add -F note.txt HEAD`
- `git notes append -m appended HEAD`
- `git notes append -F append-note.txt HEAD`
- `git notes copy HEAD~1 HEAD`

The evidence compares stock Git and Zmin notes ref output, list/show output,
remove behavior, missing-note failure status, file-backed add, message and
file-backed append, and copy behavior across two commits.

Actual post-import movement matched the declaration: `+10` behavior rows,
`+9` closed rows, `+0` open rows, `+1` invalid-input row, `+1`
represented oracle function, `-1` missing-or-unclassified oracle function,
`+0` commands with rows and `+0` represented doc-option pairs.

## Latest Declared Import

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence function:

- `git_notes_compat::notes_edit_message_source_options_match_stock_git`

Expected movement:

- behavior rows: `+12`
- closed rows: `+12`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+1`
- missing-or-unclassified oracle functions: `-1`
- commands with rows: `+0`
- represented doc-option pairs: expected `+0`; notes subcommand option rows
  are not represented in the current Git docs option seed
- Rust behavior changes: no

Expected rows:

- `git notes edit -m msg HEAD`
- `git notes edit --message=long HEAD`
- `git notes edit -mcompact HEAD`
- `git notes edit -F note.txt HEAD`
- `git notes edit --file=note.txt HEAD`
- `git notes edit -Fnote.txt HEAD`
- `git notes edit -C <blob> HEAD`
- `git notes edit --reuse-message=<blob> HEAD`
- `git notes edit -C<blob> HEAD`
- `GIT_EDITOR=edit-source-note.sh git notes edit -c <blob> HEAD`
- `GIT_EDITOR=edit-source-note.sh git notes edit --reedit-message=<blob> HEAD`
- `GIT_EDITOR=edit-source-note.sh git notes edit -c<blob> HEAD`

The evidence compares stock Git and Zmin command output and resulting note
contents for short, long and compact message, file, reuse-message and
reedit-message forms.

Actual post-import movement matched the declaration: `+12` behavior rows,
`+12` closed rows, `+0` open rows, `+0` invalid-input rows, `+1`
represented oracle function, `-1` missing-or-unclassified oracle function,
`+0` commands with rows and `+0` represented doc-option pairs.

## Latest Declared Import

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence function:

- `git_notes_compat::notes_allow_empty_matches_stock_git_for_add_and_append`

Expected movement:

- behavior rows: `+3`
- closed rows: `+3`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+1`
- missing-or-unclassified oracle functions: `-1`
- commands with rows: `+0`
- represented doc-option pairs: expected `+0`; notes subcommand option rows
  are not represented in the current Git docs option seed
- Rust behavior changes: no

Expected rows:

- `GIT_EDITOR=true git notes add --allow-empty HEAD`
- `GIT_EDITOR=true git notes append --allow-empty HEAD` without an existing
  note
- `GIT_EDITOR=true git notes append --allow-empty HEAD` with an existing note

The evidence compares stock Git and Zmin command output plus resulting note
contents for allow-empty add and append flows.

Actual post-import movement matched the declaration: `+3` behavior rows,
`+3` closed rows, `+0` open rows, `+0` invalid-input rows, `+1`
represented oracle function, `-1` missing-or-unclassified oracle function,
`+0` commands with rows and `+0` represented doc-option pairs.

## Latest Declared Import

Source bucket: focused stock-oracle test already represented in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence function:

- `git_mail_tools_compat::mailinfo_matches_stock_git_for_common_patch_mail`

Expected movement:

- behavior rows: `+2`
- closed rows: `+2`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: expected `+0`; `mailinfo` was already
  represented and these rows split exact positional output path surfaces
- Rust behavior changes: no

Expected rows:

- `git mailinfo git-msg git-patch < patch-mail.txt` as `<positional:msg>`
- `git mailinfo git-msg git-patch < patch-mail.txt` as `<positional:patch>`

The evidence compares stock Git and Zmin stdout plus the resulting message and
patch output files for the default patch-mail split flow.

Actual post-import movement matched the declaration: `+2` behavior rows,
`+2` closed rows, `+0` open rows, `+0` invalid-input rows, `+0`
represented oracle functions, `+0` missing-or-unclassified oracle functions,
`+0` commands with rows and `+0` represented doc-option pairs.

## Latest Declared Import

Source bucket: focused stock-oracle test already represented in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence function:

- `git_mail_tools_compat::request_pull_matches_stock_git_for_local_pushed_branch`

Expected movement:

- behavior rows: `+2`
- closed rows: `+2`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: expected `+0`; `request-pull` was already
  represented and these rows split exact positional input surfaces
- Rust behavior changes: no

Expected rows:

- `git request-pull <start> file://<remote> main` as `<positional:url>`
- `git request-pull <start> file://<remote> main` as `<positional:end>`

The evidence compares stock Git and Zmin output for a local bare remote with a
pushed branch ahead of the start commit.

Actual post-import movement matched the declaration: `+2` behavior rows,
`+2` closed rows, `+0` open rows, `+0` invalid-input rows, `+0`
represented oracle functions, `+0` missing-or-unclassified oracle functions,
`+0` commands with rows and `+0` represented doc-option pairs.

## Latest Declared Import

Source bucket: focused stock-oracle test already represented in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence function:

- `git_mail_tools_compat::mailsplit_matches_stock_git_for_mbox_and_maildir`

Expected movement:

- behavior rows: `+2`
- closed rows: `+2`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: expected `+1` for `mailsplit -f`; the
  positional mbox input row is not represented in the Git docs option seed
- Rust behavior changes: no

Expected rows:

- `git mailsplit -d4 -f3 -oout mbox` as `-f 3`
- `git mailsplit -d4 -f3 -oout mbox` as `<positional:paths> mbox`

The evidence compares stock Git and Zmin stdout plus output files for the mbox
split flow.

Actual post-import movement matched the declaration: `+2` behavior rows,
`+2` closed rows, `+0` open rows, `+0` invalid-input rows, `+0`
represented oracle functions, `+0` missing-or-unclassified oracle functions,
`+0` commands with rows and `+1` represented doc-option pairs.

## Latest Declared Import

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence function:

- `git_ref_resolution_compat::replace_matches_stock_git_for_list_create_delete`

Expected movement:

- behavior rows: `+6`
- closed rows: `+6`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+1`
- missing-or-unclassified oracle functions: `-1`
- commands with rows: `+1`
- represented doc-option pairs: expected `+3` for `replace -d`,
  `replace -l` and `replace --format`; default and positional creation rows
  are not represented in the Git docs option seed
- Rust behavior changes: no

Expected rows:

- `git replace <object> <replacement>`
- `git replace`
- `git replace -l '*'`
- `git replace --format=medium`
- `git replace --format=long`
- `git replace -d <object-prefix>`

The evidence compares stock Git and Zmin output plus replacement ref side
effects for create, list, formatted list and delete flows.

Actual post-import movement matched the declaration: `+6` behavior rows,
`+6` closed rows, `+0` open rows, `+0` invalid-input rows, `+1`
represented oracle function, `-1` missing-or-unclassified oracle function,
`+1` command with rows and `+3` represented doc-option pairs.

## Latest Declared Import

Source bucket: census implemented-but-unverified `checkout-index` schema
surfaces, then focused stock-oracle evidence already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence function:

- `git_worktree_state_compat::checkout_index_matches_stock_git_for_all_paths_stdin_and_prefix`

Expected movement:

- behavior rows: `+4`
- closed rows: `+4`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+1`
- missing-or-unclassified oracle functions: `-1`
- commands with rows: `+0`
- represented doc-option pairs: expected `+3` for `checkout-index -a`,
  `--prefix` and `--stdin`; the explicit path row is not represented in the
  Git docs option seed
- implemented-but-unverified schema rows: expected `-5` because `-a` closes
  the shared `arg_id=all` parser surface for the `--all` alias
- Rust behavior changes: no

Expected rows:

- `git checkout-index -a`
- `git checkout-index README.md`
- `git checkout-index --prefix=out/ README.md docs/guide.md`
- `printf 'docs/guide.md\n' | git checkout-index --stdin`

The evidence compares stock Git and Zmin stdout, stderr and restored worktree
file contents for all-path, explicit-path, prefixed-output and stdin-path
flows.

Actual post-import movement matched the declaration: `+4` behavior rows,
`+4` closed rows, `+0` open rows, `+0` invalid-input rows, `+1`
represented oracle function, `-1` missing-or-unclassified oracle function,
`+0` commands with rows, `+3` represented doc-option pairs and `-5`
implemented-but-unverified schema rows.

## Latest Declared Import

Source bucket: census implemented-but-unverified `checkout-index` quiet schema
surfaces, with new focused stock-oracle smoke evidence.

Evidence command:

- `tools/git-checkout-index-oracle-smoke.sh`

Expected movement:

- behavior rows: `+2`
- closed rows: `+2`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: expected `+2` for `checkout-index --quiet`
  and `checkout-index -q`
- implemented-but-unverified schema rows: expected `-2`
- Rust behavior changes: no

Expected rows:

- `git checkout-index --quiet README.md`
- `git checkout-index -q docs/guide.md`

The evidence compares stock Git and Zmin exit status, stdout, stderr,
worktree status and restored file contents for quiet long and short forms.

Actual post-import movement matched the declaration: `+2` behavior rows,
`+2` closed rows, `+0` open rows, `+0` invalid-input rows, `+0`
represented oracle functions, `+0` missing-or-unclassified oracle functions,
`+0` commands with rows, `+2` represented doc-option pairs and `-2`
implemented-but-unverified schema rows.

## Latest Declared Import

Source bucket: census implemented-but-unverified `symbolic-ref` positional
schema surfaces, with focused stock-oracle smoke evidence.

Evidence command:

- `tools/git-symbolic-ref-schema-oracle-smoke.sh`

Expected movement:

- behavior rows: `+2`
- closed rows: `+2`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: `+0`; these rows close positional schema
  surfaces rather than documented option spellings
- implemented-but-unverified schema rows: expected `-2`
- Rust behavior changes: no

Expected rows:

- `git symbolic-ref HEAD`
- `git symbolic-ref HEAD refs/heads/plumbing`

The evidence compares stock Git and Zmin exit status, stdout, stderr,
`.git/HEAD` content and worktree status in an attached repository with an
existing target branch.

Actual post-import movement matched the declaration: `+2` behavior rows,
`+2` closed rows, `+0` open rows, `+0` invalid-input rows, `+0` represented
oracle functions, `+0` missing-or-unclassified oracle functions, `+0`
commands with rows, `+0` represented doc-option pairs and `-2`
implemented-but-unverified schema rows.

## Latest Declared Import

Source bucket: census implemented-but-unverified `ls-tree` positional schema
surfaces, with focused stock-oracle smoke evidence.

Evidence command:

- `tools/git-ls-tree-schema-oracle-smoke.sh`

Expected movement:

- behavior rows: `+2`
- closed rows: `+2`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: `+0`; these rows close positional schema
  surfaces rather than documented option spellings
- implemented-but-unverified schema rows: expected `-2`
- Rust behavior changes: no

Expected rows:

- `git ls-tree HEAD`
- `git ls-tree HEAD src/main.rs`

The evidence compares stock Git and Zmin exit status, stdout, stderr and
worktree status in a repository with root and nested files.

Actual post-import movement matched the declaration: `+2` behavior rows,
`+2` closed rows, `+0` open rows, `+0` invalid-input rows, `+0` represented
oracle functions, `+0` missing-or-unclassified oracle functions, `+0`
commands with rows, `+0` represented doc-option pairs and `-2`
implemented-but-unverified schema rows.

## Latest Declared Import

Source bucket: census implemented-but-unverified `hash-object` schema
surfaces, with focused stock-oracle smoke evidence and one parser schema fix.

Evidence command:

- `tools/git-hash-object-schema-oracle-smoke.sh`

Expected movement:

- behavior rows: `+2`
- closed rows: `+1`
- open rows: `+0`
- invalid-input rows: `+1`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: expected `+1` for `hash-object -t`; `--type`
  is an invalid undocumented spelling and does not add a Git doc-option pair
- implemented-but-unverified schema rows: expected `-2`; `-t` gets exact
  stock-Git evidence and `--type` is removed from the schema after proving
  stock Git rejects it
- Rust behavior changes: yes, parser schema removes `hash-object --type` and
  the pre-clap guard returns stock-style exit `129`/stderr for that spelling

Expected rows:

- `git hash-object -t blob a.txt`
- `git hash-object --type blob a.txt`

The evidence compares stock Git and Zmin exit status, stdout, stderr and
worktree status in a local repository with a regular file.

Actual post-import movement matched the declaration: `+2` behavior rows,
`+1` closed row, `+0` open rows, `+1` invalid-input row, `+0` represented
oracle functions, `+0` missing-or-unclassified oracle functions, `+0`
commands with rows, `+1` represented doc-option pair and `-2`
implemented-but-unverified schema rows.

## Latest Declared Import

Source bucket: census implemented-but-unverified `mktree --missing` schema
surface, with focused stock-oracle smoke evidence.

Evidence command:

- `tools/git-mktree-schema-oracle-smoke.sh`

Expected movement:

- behavior rows: `+1`
- closed rows: `+1`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: expected `+1` for `mktree --missing`
- implemented-but-unverified schema rows: expected `-1`
- Rust behavior changes: no

Expected row:

- `printf '100644 blob <missing-oid>\tmissing.txt\n' | git mktree --missing`

The evidence compares stock Git and Zmin exit status, stdout, stderr and the
written tree object type using stock Git.

Actual post-import movement matched the declaration: `+1` behavior row,
`+1` closed row, `+0` open rows, `+0` invalid-input rows, `+0`
represented oracle functions, `+0` missing-or-unclassified oracle functions,
`+0` commands with rows, `+1` represented doc-option pair and `-1`
implemented-but-unverified schema row.

## Latest Declared Import

Source bucket: census implemented-but-unverified `commit-tree`
`<positional:tree>` schema surface, with focused stock-oracle smoke evidence.

Evidence command:

- `tools/git-commit-tree-schema-oracle-smoke.sh`

Expected movement:

- behavior rows: `+1`
- closed rows: `+1`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: `+0`; this is a positional schema surface and
  is not represented as a separate Git doc option seed row by the current census
- implemented-but-unverified schema rows: expected `-1`
- Rust behavior changes: no

Expected row:

- `git commit-tree <tree> -m root`

The evidence compares stock Git and Zmin exit status, stdout, stderr and the
stored commit object using fixed identity and timestamps.

Actual post-import movement matched the declaration: `+1` behavior row,
`+1` closed row, `+0` open rows, `+0` invalid-input rows, `+0`
represented oracle functions, `+0` missing-or-unclassified oracle functions,
`+0` commands with rows, `+0` represented doc-option pairs and `-1`
implemented-but-unverified schema row.

## Latest Declared Import

Source bucket: census implemented-but-unverified `read-tree`
`<positional:treeish>` schema surface, with focused stock-oracle smoke
evidence.

Evidence command:

- `tools/git-read-tree-schema-oracle-smoke.sh`

Expected movement:

- behavior rows: `+1`
- closed rows: `+1`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: `+0`; this is a positional schema surface and
  is not represented as a separate Git doc option seed row by the current census
- implemented-but-unverified schema rows: expected `-1`
- Rust behavior changes: no

Expected row:

- `git read-tree <tree>`

The evidence compares stock Git and Zmin exit status, stdout, stderr,
`ls-files --stage`, `write-tree` output and worktree status.

Actual post-import movement matched the declaration: `+1` behavior row,
`+1` closed row, `+0` open rows, `+0` invalid-input rows, `+0`
represented oracle functions, `+0` missing-or-unclassified oracle functions,
`+0` commands with rows, `+0` represented doc-option pairs and `-1`
implemented-but-unverified schema row.

## Latest Declared Import

Source bucket: census implemented-but-unverified `write-tree --missing-ok`
schema surface, with focused stock-oracle smoke evidence.

Evidence command:

- `tools/git-write-tree-schema-oracle-smoke.sh`

Expected movement:

- behavior rows: `+1`
- closed rows: `+1`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: expected `+1` for `write-tree --missing-ok`
- implemented-but-unverified schema rows: expected `-1`
- Rust behavior changes: no

Expected row:

- `git write-tree --missing-ok` with an index entry whose blob object is absent

The evidence compares stock Git and Zmin exit status, stdout, stderr and the
written tree object type using stock Git.

Actual post-import movement matched the declaration: `+1` behavior row,
`+1` closed row, `+0` open rows, `+0` invalid-input rows, `+0`
represented oracle functions, `+0` missing-or-unclassified oracle functions,
`+0` commands with rows, `+1` represented doc-option pair and `-1`
implemented-but-unverified schema row.

## Latest Declared Import

Source bucket: census implemented-but-unverified `credential-cache --timeout`
and positional action schema surfaces, with focused stock-oracle smoke
evidence.

Evidence command:

- `tools/git-credential-cache-schema-oracle-smoke.sh`

Expected movement:

- behavior rows: `+2`
- closed rows: `+2`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: expected `+1` for `credential-cache --timeout`
- implemented-but-unverified schema rows: expected `-2`
- Rust behavior changes: yes, remove the extra blank line from
  `credential-cache get` output

Expected rows:

- `git credential-cache --socket=<path> --timeout=60 store/get/erase`
- positional `store`, `get` and `erase` action surface with `--timeout=60`

The evidence compares stock Git and Zmin exit status, stdout and stderr for
store, get, erase and post-erase get over explicit Unix socket paths.

Actual post-import movement matched the declaration: `+2` behavior rows,
`+2` closed rows, `+0` open rows, `+0` invalid-input rows, `+0`
represented oracle functions, `+0` missing-or-unclassified oracle functions,
`+0` commands with rows, `+1` represented doc-option pair and `-2`
implemented-but-unverified schema rows.

## Latest Declared Import

Source bucket: census implemented-but-unverified `credential-store --file`
and positional action schema surfaces, with focused stock-oracle smoke
evidence.

Evidence command:

- `tools/git-credential-store-schema-oracle-smoke.sh`

Expected movement:

- behavior rows: `+2`
- closed rows: `+2`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: expected `+1` for `credential-store --file`
- implemented-but-unverified schema rows: expected `-2`
- Rust behavior changes: no

Expected rows:

- `git credential-store --file <path> store/get/erase`
- positional `store`, `get` and `erase` action surface with `--file <path>`

The evidence compares stock Git and Zmin exit status, stdout, stderr, explicit
credential file side effects and the absence of default HOME credential-file
side effects.

Actual post-import movement matched the declaration: `+2` behavior rows,
`+2` closed rows, `+0` open rows, `+0` invalid-input rows, `+0`
represented oracle functions, `+0` missing-or-unclassified oracle functions,
`+0` commands with rows, `+1` represented doc-option pair and `-2`
implemented-but-unverified schema rows.

## Latest Declared Import

Source bucket: census implemented-but-unverified `count-objects` long option
surfaces, with focused stock-oracle smoke evidence.

Evidence command:

- `tools/git-count-objects-schema-oracle-smoke.sh`

Expected movement:

- behavior rows: `+3`
- closed rows: `+3`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: expected `+2` for
  `count-objects --verbose` and `count-objects --human-readable`; the combined
  row extends both already represented long option spellings
- implemented-but-unverified schema rows: `+0`; the current census keeps these
  schema args unchanged while the rows add exact long-option evidence
- Rust behavior changes: no

Expected rows:

- `git count-objects --verbose`
- `git count-objects --human-readable`
- `git count-objects --verbose --human-readable`

The evidence compares stock Git and Zmin exit status, stdout, stderr and
worktree status in a repository with loose objects.

Actual post-import movement matched the declaration: `+3` behavior rows,
`+3` closed rows, `+0` open rows, `+0` invalid-input rows, `+0`
represented oracle functions, `+0` missing-or-unclassified oracle functions,
`+0` commands with rows, `+2` represented doc-option pairs and `+0`
implemented-but-unverified schema rows.

## Latest Declared Import

Source bucket: census implemented-but-unverified `archive -l` and `archive -v`
doc-option surfaces, with focused stock-oracle smoke evidence.

Evidence command:

- `tools/git-archive-schema-oracle-smoke.sh`

Expected movement:

- behavior rows: `+2`
- closed rows: `+2`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: expected `+2` for `archive -l` and
  `archive -v`
- implemented-but-unverified schema rows: `+0`; these rows close short
  documented alias evidence while the current census keeps schema args
  unchanged
- Rust behavior changes: no

Expected rows:

- `git archive -l`
- `git archive --format=tar -v --output=out.tar HEAD dir`

The evidence compares stock Git and Zmin exit status, stdout, stderr and
worktree status. The `-v` row also compares the tar entry listing for the
written output file. The same probe found `git archive --output=out.tar HEAD`
produces a binary tar difference, so `--output` remains unverified and is not
counted in this import.

Actual post-import movement matched the declaration: `+2` behavior rows,
`+2` closed rows, `+0` open rows, `+0` invalid-input rows, `+0`
represented oracle functions, `+0` missing-or-unclassified oracle functions,
`+0` commands with rows, `+2` represented doc-option pairs and `+0`
implemented-but-unverified schema rows.

## Latest Declared Import

Source bucket: census implemented-but-unverified `add --verbose` and `add -v`
surfaces, with focused stock-oracle smoke evidence.

Evidence command:

- `tools/git-add-oracle-smoke.sh`

Expected movement:

- behavior rows: `+2`
- closed rows: `+2`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: expected `+2` for `add --verbose` and
  `add -v`
- implemented-but-unverified schema rows: expected `-2`
- Rust behavior changes: yes, `add` now prints stock-compatible
  `add '<path>'` rows for successful normal verbose staging

Expected rows:

- `git add --verbose verbose.txt`
- `git add -v verbose.txt`

The evidence compares stock Git and Zmin exit status, stdout, stderr,
`status --short` and stable `ls-files --stage --debug` index fields.

Actual post-import movement matched the declaration: `+2` behavior rows,
`+2` closed rows, `+0` open rows, `+0` invalid-input rows, `+0`
represented oracle functions, `+0` missing-or-unclassified oracle functions,
`+0` commands with rows, `+2` represented doc-option pairs and `-2`
implemented-but-unverified schema rows.

## Latest Declared Import

Source bucket: census implemented-but-unverified `add` schema/doc-option
surfaces, with focused stock-oracle smoke evidence.

Evidence command:

- `tools/git-add-oracle-smoke.sh`

Expected movement:

- behavior rows: `+5`
- closed rows: `+5`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: expected `+5` for `add --all`,
  `add --force`, `add --pathspec-file-nul`, `add --update` and `add -n`
- implemented-but-unverified schema rows: expected `-1`; the census maps these
  added rows onto one remaining schema gap, while the other option spellings
  become represented doc-option evidence and still require broader expansion
- Rust behavior changes: no

Expected rows:

- `git add --all`
- `git add --force force.ignored`
- `git add --pathspec-from-file=paths.nul --pathspec-file-nul`
- `git add --update`
- `git add -n dry.txt`

The evidence compares stock Git and Zmin exit status, stdout, stderr,
`status --short` and stable `ls-files --stage --debug` index fields. The same
probe found that `git add --verbose verbose.txt` and `git add -v verbose.txt`
still differ because stock Git prints `add 'verbose.txt'` while Zmin is silent;
those verbose surfaces remain implemented-but-unverified and are not counted in
this import.

Actual post-import movement matched the declaration: `+5` behavior rows,
`+5` closed rows, `+0` open rows, `+0` invalid-input rows, `+0`
represented oracle functions, `+0` missing-or-unclassified oracle functions,
`+0` commands with rows, `+5` represented doc-option pairs and `-1`
implemented-but-unverified schema row.

## Latest Declared Import

Source bucket: census implemented-but-unverified `commit` short/options
surfaces, with focused stock-oracle smoke evidence.

Evidence command:

- `tools/git-commit-short-options-oracle-smoke.sh`

Expected movement:

- behavior rows: `+6`
- closed rows: `+6`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: expected `+6` for `commit -q`, `commit -s`,
  `commit -o`, `commit -n`, `commit -t` and `commit --verbose`
- implemented-but-unverified schema rows: `+0`; these are doc-option evidence
  gaps after related schema arg IDs were already covered or remain schema
  aliases rather than new command entries
- Rust behavior changes: no

Expected rows:

- `git -c commit.gpgsign=false commit -q -m quiet`
- `git -c commit.gpgsign=false commit -s -m subject`
- `git -c commit.gpgsign=false commit -o -m only -- a.txt`
- `git -c commit.gpgsign=false commit -n -m skip`
- `GIT_EDITOR=.git/editor.sh git -c commit.gpgsign=false commit -t template.txt`
- `GIT_EDITOR=.git/editor.sh git -c commit.gpgsign=false commit --verbose --no-status`

The evidence compares stock Git and Zmin exit status, stdout, stderr, commit
object, HEAD and porcelain status. The `-n` row also verifies that a failing
pre-commit hook is skipped by comparing hook-log presence. The smoke disables
ambient commit signing with `-c commit.gpgsign=false` so local global Git config
cannot change the oracle commit object.

Actual post-import movement matched the declaration: `+6` behavior rows,
`+6` closed rows, `+0` open rows, `+0` invalid-input rows, `+0`
represented oracle functions, `+0` missing-or-unclassified oracle functions,
`+0` commands with rows, `+6` represented doc-option pairs and `+0`
implemented-but-unverified schema rows.

## Latest Declared Import

Source bucket: census implemented-but-unverified `commit` doc-option alias
surfaces, with focused stock-oracle smoke evidence.

Evidence command:

- `tools/git-commit-alias-oracle-smoke.sh`

Expected movement:

- behavior rows: `+4`
- closed rows: `+4`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: expected `+4` for `commit --file`,
  `commit --edit`, `commit --reuse-message` and `commit --reedit-message`
- implemented-but-unverified schema rows: `+0`; these are doc-option evidence
  gaps after the matching schema arg IDs were already covered by existing
  short-option rows
- Rust behavior changes: no

Expected rows:

- `git -c commit.gpgsign=false commit --file msg.txt`
- `GIT_EDITOR=.git/editor.sh git -c commit.gpgsign=false commit --edit --file msg.txt`
- `git -c commit.gpgsign=false commit --reuse-message HEAD~1`
- `GIT_EDITOR=.git/editor.sh git -c commit.gpgsign=false commit --reedit-message HEAD~1`

The evidence compares stock Git and Zmin exit status, stdout, stderr, commit
object, HEAD and porcelain status. The smoke disables ambient commit signing
with `-c commit.gpgsign=false` so local global Git config cannot change the
oracle commit object.

Actual post-import movement matched the declaration: `+4` behavior rows,
`+4` closed rows, `+0` open rows, `+0` invalid-input rows, `+0`
represented oracle functions, `+0` missing-or-unclassified oracle functions,
`+0` commands with rows, `+4` represented doc-option pairs and `+0`
implemented-but-unverified schema rows.

## Latest Declared Import

Source bucket: census implemented-but-unverified `add` schema surfaces, with
new focused stock-oracle smoke evidence.

Evidence command:

- `tools/git-add-oracle-smoke.sh`

Expected movement:

- behavior rows: `+3`
- closed rows: `+3`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: expected `+2` for `add --intent-to-add` and
  `add -N`; the positional path row is not represented in the Git docs option
  seed
- implemented-but-unverified schema rows: expected `-3`
- Rust behavior changes: no

Expected rows:

- `git add --intent-to-add intent.txt`
- `git add -N intent.txt`
- `git add new.txt`

The evidence compares stock Git and Zmin exit status, stdout, stderr,
`status --short` and stable `ls-files --stage --debug` index fields for
intent-to-add and normal positional add flows. The same smoke initially found
that `git add --verbose new.txt` prints `add 'new.txt'` while Zmin is silent;
that surface is not counted as verified in this import.

Actual post-import movement matched the declaration: `+3` behavior rows,
`+3` closed rows, `+0` open rows, `+0` invalid-input rows, `+0`
represented oracle functions, `+0` missing-or-unclassified oracle functions,
`+0` commands with rows, `+2` represented doc-option pairs and `-3`
implemented-but-unverified schema rows.

## Latest Declared Import

Source bucket: census implemented-but-unverified `bugreport` schema surfaces,
with new focused stock-oracle smoke evidence.

Evidence command:

- `tools/git-bugreport-oracle-smoke.sh`

Expected movement:

- behavior rows: `+6`
- closed rows: `+6`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: expected `+4` for `bugreport --no-suffix`,
  `bugreport -o`, `bugreport --output-directory` and `bugreport -s`; the
  extra `--suffix` value-form rows extend an already represented option
- implemented-but-unverified schema rows: expected `-1` because
  `--no-suffix` closes a schema arg; `-o`, `--output-directory`, `-s` and
  `--suffix` are documented option rows, but the matching schema arg IDs were
  already covered by existing suffix evidence or remain doc-expansion work
- Rust behavior changes: no

Expected rows:

- `git bugreport -o <out> --no-suffix`
- `git bugreport --output-directory <out> --no-suffix`
- `git bugreport -o <out> -s custom`
- `git bugreport -o <out> --suffix=eqcustom`
- `git bugreport -o <out> --suffix sepcustom`
- an explicit `-o` row using the no-suffix evidence

The evidence compares stock Git and Zmin exit status, stdout, stderr and
created report filenames. It does not count `bugreport --diagnose=stats`
because that probe emits different diagnostic payload, so `--diagnose` remains
implemented-but-unverified.

Actual post-import movement matched the declaration: `+6` behavior rows,
`+6` closed rows, `+0` open rows, `+0` invalid-input rows, `+0`
represented oracle functions, `+0` missing-or-unclassified oracle functions,
`+0` commands with rows, `+4` represented doc-option pairs and `-1`
implemented-but-unverified schema rows.

## Latest Declared Import

Source bucket: census documented-option gap for `stash -S` and `stash -p`,
using existing helper-free local stash compat lanes.

Evidence commands:

- `cargo test -p zmin-cli --test git_stash_compat stash_push_short_staged_alias_matches_stock_git -- --exact`
- `cargo test -p zmin-cli --test git_stash_compat stash_push_short_patch_alias_matches_stock_git -- --exact`

Expected movement:

- behavior rows: `+2`
- closed rows: `+2`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+2`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: `+2`
- complete command matrices: `+1` after promotion

Actual post-import movement matched the declaration: `+2` behavior rows,
`+2` closed rows, `+0` open rows, `+0` invalid-input rows, `+0` commands with
rows, `+2` complete documented option pairs, and `+1` reviewed-complete
command matrix after promoting `stash`.

## Previous Declared Import

Source bucket: census documented-option gap for `bugreport --no-diagnose`,
using the existing focused stock-vs-Zmin oracle test lane.

Evidence command:

- `cargo test -p zmin-cli --test git_admin_tools_compat bugreport_no_diagnose_modes_match_stock_git -- --exact`

Expected movement:

- behavior rows: `+3`
- closed rows: `+3`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+1`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: `+1`
- complete command matrices: `+1` after promotion

Actual post-import movement matched the declaration: `+3` behavior rows,
`+3` closed rows, `+0` open rows, `+0` invalid-input rows, `+0` commands with
rows, `+1` complete documented option pair, and `+1` reviewed-complete
command matrix after promoting `bugreport`.

## Earlier Declared Import

Source bucket: census implemented-but-unverified `cat-file --no-filter`
schema surface, with focused stock-oracle smoke evidence.

Evidence command:

- `tools/git-cat-file-schema-oracle-smoke.sh`

Expected movement:

- behavior rows: `+1`
- closed rows: `+1`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: `+0`; this is a Zmin schema surface and is
  not represented as a separate Git doc option seed row by the current census
- implemented-but-unverified schema rows: expected `-1`
- Rust behavior changes: no

Expected row:

- `printf '<oid>\n' | git cat-file --batch-check --no-filter`

The evidence compares stock Git and Zmin exit status, stdout, stderr and clean
worktree status in a repository with a committed blob.

Actual post-import movement matched the declaration: `+1` behavior row,
`+1` closed row, `+0` open rows, `+0` invalid-input rows, `+0`
represented oracle functions, `+0` missing-or-unclassified oracle functions,
`+0` commands with rows, `+0` represented doc-option pairs and `-1`
implemented-but-unverified schema row.

## Latest Declared Import

Source bucket: census implemented-but-unverified `commit --all` schema
surfaces, with focused stock-oracle smoke evidence.

Evidence command:

- `tools/git-commit-all-oracle-smoke.sh`

Expected movement:

- behavior rows: `+2`
- closed rows: `+2`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: expected `+2` for `commit --all` and
  `commit -a`
- implemented-but-unverified schema rows: expected `-2`
- Rust behavior changes: no

Expected rows:

- `git -c commit.gpgsign=false commit --all -m all-long`
- `git -c commit.gpgsign=false commit -a -m all-short`

The evidence compares stock Git and Zmin exit status, stdout, stderr, commit
object, HEAD, tree and porcelain status for tracked modifications with an
unrelated untracked file. The smoke disables ambient commit signing with
`-c commit.gpgsign=false` so local global Git config cannot change the oracle
commit object.

Actual post-import movement matched the declaration: `+2` behavior rows,
`+2` closed rows, `+0` open rows, `+0` invalid-input rows, `+0`
represented oracle functions, `+0` missing-or-unclassified oracle functions,
`+0` commands with rows, `+2` represented doc-option pairs and `-2`
implemented-but-unverified schema rows.

## Latest Declared Import

Source bucket: census implemented-but-unverified `merge-base` positional
schema surface, with focused stock-oracle smoke evidence.

Evidence command:

- `tools/git-merge-base-schema-oracle-smoke.sh`

Expected movement:

- behavior rows: `+1`
- closed rows: `+1`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: `+0`; this row closes a positional schema
  surface rather than a documented option spelling
- implemented-but-unverified schema rows: expected `-1`
- Rust behavior changes: no

Expected row:

- `git merge-base main side`

The evidence compares stock Git and Zmin exit status, stdout, stderr and
worktree status using copied workdirs from one seed repository so object ids
match byte-for-byte.

Actual post-import movement matched the declaration: `+1` behavior row,
`+1` closed row, `+0` open rows, `+0` invalid-input rows, `+0` represented
oracle functions, `+0` missing-or-unclassified oracle functions, `+0`
commands with rows, `+0` represented doc-option pairs and `-1`
implemented-but-unverified schema row.

## Latest Declared Import

Source bucket: census implemented-but-unverified `show-ref` positional schema
surface, with focused stock-oracle smoke evidence.

Evidence command:

- `tools/git-show-ref-schema-oracle-smoke.sh`

Expected movement:

- behavior rows: `+1`
- closed rows: `+1`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: `+0`; this row closes a positional schema
  surface rather than a documented option spelling
- implemented-but-unverified schema rows: expected `-1`
- Rust behavior changes: no

Expected row:

- `git show-ref refs/heads/main`

The evidence compares stock Git and Zmin exit status, stdout, stderr, full
`show-ref` output and worktree status using copied workdirs from one seed
repository so ref object ids match byte-for-byte.

Actual post-import movement matched the declaration: `+1` behavior row,
`+1` closed row, `+0` open rows, `+0` invalid-input rows, `+0` represented
oracle functions, `+0` missing-or-unclassified oracle functions, `+0`
commands with rows, `+0` represented doc-option pairs and `-1`
implemented-but-unverified schema row.

## Latest Declared Import

Source bucket: census implemented-but-unverified `rev-parse` positional schema
surface, with focused stock-oracle smoke evidence.

Evidence command:

- `tools/git-rev-parse-schema-oracle-smoke.sh`

Expected movement:

- behavior rows: `+1`
- closed rows: `+1`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: `+0`; this row closes a positional schema
  surface rather than a documented option spelling
- implemented-but-unverified schema rows: expected `-1`
- Rust behavior changes: no

Expected row:

- `git rev-parse HEAD`

The evidence compares stock Git and Zmin exit status, stdout, stderr and
worktree status using copied workdirs from one seed repository so object ids
match byte-for-byte.

Actual post-import movement matched the declaration: `+1` behavior row,
`+1` closed row, `+0` open rows, `+0` invalid-input rows, `+0` represented
oracle functions, `+0` missing-or-unclassified oracle functions, `+0`
commands with rows, `+0` represented doc-option pairs and `-1`
implemented-but-unverified schema row.

## Latest Declared Import

Source bucket: census implemented-but-unverified `update-index` schema
surfaces, with focused stock-oracle smoke evidence and first command matrix
rows for `update-index`.

Evidence command:

- `tools/git-update-index-schema-oracle-smoke.sh`

Expected movement:

- behavior rows: `+2`
- closed rows: `+2`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+1`
- represented doc-option pairs: expected `+1` for `update-index --add`; the
  positional path row closes a schema surface rather than a documented option
  spelling
- implemented-but-unverified schema rows: expected `-2`
- remaining checklist rows: expected `-1` because the new `--add` row moves the
  documented option seed from no matrix evidence to expansion-required
- Rust behavior changes: no

Expected rows:

- `git update-index --add a.txt`
- `git update-index a.txt`

The evidence compares stock Git and Zmin exit status, stdout, stderr,
`ls-files --stage` output and worktree status for an untracked add case and a
tracked modified path case.

Actual post-import movement matched the declaration: `+2` behavior rows,
`+2` closed rows, `+0` open rows, `+0` invalid-input rows, `+0` represented
oracle functions, `+0` missing-or-unclassified oracle functions, `+1` command
with rows, `+1` represented doc-option pair, `-2` implemented-but-unverified
schema rows and `-1` remaining checklist row.

## Latest Declared Import

Source bucket: census implemented-but-unverified `update-index` schema
surfaces, with focused stock-oracle smoke evidence for simple index flag,
removal and stdin modes.

Evidence command:

- `tools/git-update-index-schema-oracle-smoke.sh`

Expected movement:

- behavior rows: `+10`
- closed rows: `+10`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: expected `+10`
- implemented-but-unverified schema rows: expected `-10`
- remaining checklist rows: expected `+0`; documented option seeds move from
  implemented-without-matrix to expansion-required, but remain in the
  remaining expansion checklist
- Rust behavior changes: no

Expected rows:

- `git update-index --refresh`
- `git update-index --really-refresh`
- `git update-index --assume-unchanged a.txt`
- `git update-index --no-assume-unchanged a.txt`
- `git update-index --skip-worktree a.txt`
- `git update-index --no-skip-worktree a.txt`
- `git update-index --remove a.txt`
- `git update-index --force-remove a.txt`
- `printf 'a.txt\n' | git update-index --stdin`
- `printf 'a.txt\0' | git update-index -z --stdin`

The evidence compares stock Git and Zmin exit status, stdout, stderr,
`ls-files --stage`, `ls-files -v` index flags and worktree status. Refresh
rows intentionally use a clean tracked file because stock Git exits non-zero
for a modified file while the current Zmin behavior still needs a separate fix
slice.

Actual post-import movement matched the declaration: `+10` behavior rows,
`+10` closed rows, `+0` open rows, `+0` invalid-input rows, `+0` represented
oracle functions, `+0` missing-or-unclassified oracle functions, `+0` commands
with rows, `+10` represented doc-option pairs, `-10`
implemented-but-unverified schema rows and `+0` remaining checklist rows.

## Latest Declared Import

Source bucket: census implemented-but-unverified `update-index` schema
surfaces, with focused stock-oracle smoke evidence for cacheinfo, replace and
index-info formats.

Evidence command:

- `tools/git-update-index-schema-oracle-smoke.sh`

Expected movement:

- behavior rows: `+5`
- closed rows: `+5`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: expected `+3` for `--cacheinfo`,
  `--replace` and `--index-info`
- implemented-but-unverified schema rows: expected `-3`
- remaining checklist rows: expected `+0`; documented option seeds move from
  implemented-without-matrix to expansion-required, but remain in the
  remaining expansion checklist
- Rust behavior changes: no

Expected rows:

- `git update-index --add --cacheinfo 100644,<blob>,b.txt`
- `git update-index --add --cacheinfo 100644 <blob> b.txt`
- `git update-index --replace --cacheinfo 100644,<blob>,a.txt`
- `printf '100644 blob <blob>\tb.txt\n' | git update-index --index-info`
- `printf '100644 <blob> 0\tb.txt\n' | git update-index --index-info`

The evidence compares stock Git and Zmin exit status, stdout, stderr,
`ls-files --stage`, `ls-files -v` index flags and worktree status. Standalone
`--cacheinfo` without `--add`, `--index-info` two-field input and `--chmod`
are intentionally left for later fix slices because probe output showed
current parity gaps.

Actual post-import movement matched the declaration: `+5` behavior rows, `+5`
closed rows, `+0` open rows, `+0` invalid-input rows, `+0` represented oracle
functions, `+0` missing-or-unclassified oracle functions, `+0` commands with
rows, `+3` represented doc-option pairs, `-3` implemented-but-unverified
schema rows and `+0` remaining checklist rows.

## Latest Declared Import

Source bucket: census implemented-but-unverified `branch` positional schema
surfaces, with focused stock-oracle smoke evidence.

Evidence command:

- `tools/git-schema-parser-oracle-smoke.sh`

Expected movement:

- behavior rows: `+2`
- closed rows: `+2`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: `+0`; these rows close positional schema
  surfaces rather than documented option spellings
- implemented-but-unverified schema rows: expected `-2`
- remaining checklist rows: expected `+0`
- Rust behavior changes: no

Expected rows:

- `git branch plain_branch`
- `git branch from_head HEAD`

The evidence compares stock Git and Zmin exit status, stdout, stderr, refs and
config using copied workdirs from one seed repository. `branch --track`,
`branch -t`, `branch --help` and `branch -h` remain fix-needed surfaces because
probe output showed stdout/stderr differences.

Actual post-import movement matched the declaration: `+2` behavior rows, `+2`
closed rows, `+0` open rows, `+0` invalid-input rows, `+0` represented oracle
functions, `+0` missing-or-unclassified oracle functions, `+0` commands with
rows, `+0` represented doc-option pairs, `-2` implemented-but-unverified
schema rows and `+0` remaining checklist rows.

## Latest Declared Import

Source bucket: census implemented-but-unverified `describe` schema surfaces,
with focused stock-oracle smoke evidence.

Evidence command:

- `tools/git-describe-schema-oracle-smoke.sh`

Expected movement:

- behavior rows: `+2`
- closed rows: `+2`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: expected `+1` for `--exact-match`; the
  positional commit-ish row closes a schema surface rather than a documented
  option spelling
- implemented-but-unverified schema rows: expected `-2`
- remaining checklist rows: expected `+0`
- Rust behavior changes: no

Expected rows:

- `git describe v1.0.0`
- `git describe --exact-match v1.0.0`

The evidence compares stock Git and Zmin exit status, stdout, stderr and clean
worktree status using copied workdirs from one seed repository. Negative
`git describe --exact-match HEAD` remains a fix-needed surface because probe
output showed current stderr differences.

Actual post-import movement matched the declaration: `+2` behavior rows, `+2`
closed rows, `+0` open rows, `+0` invalid-input rows, `+0` represented oracle
functions, `+0` missing-or-unclassified oracle functions, `+0` commands with
rows, `+1` represented doc-option pair, `-2` implemented-but-unverified schema
rows and `+0` remaining checklist rows.

## Latest Declared Import

Source bucket: census implemented-but-unverified `name-rev` schema surfaces,
with focused stock-oracle test evidence.

Evidence source:

- `git_ref_resolution_compat::name_rev_matches_stock_git_for_refs_tags_and_stdin_annotation`

Expected movement:

- behavior rows: `+7`
- closed rows: `+7`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+1`
- missing-or-unclassified oracle functions: expected `-1`
- commands with rows: `+1`
- represented doc-option pairs: expected `+5` for `--name-only`, `--tags`,
  `--refs`, `--exclude` and `--annotate-stdin`; positional commit rows close
  schema surfaces rather than documented option spellings
- implemented-but-unverified schema rows: expected `-6`; the two positional
  rows share one schema arg id
- remaining checklist rows: expected `-1` because `name-rev` now has a matrix
  file
- Rust behavior changes: no

Expected rows:

- `git name-rev <HEAD>`
- `git name-rev --name-only <HEAD>`
- `git name-rev <HEAD~1>`
- `git name-rev --tags <HEAD~1>`
- `git name-rev --refs=refs/heads/main <HEAD>`
- `git name-rev --exclude=refs/heads/feature <HEAD>`
- `printf 'head <HEAD> base <HEAD~1>\n' | git name-rev --annotate-stdin`

The evidence compares stock Git and Zmin outputs in the focused Rust oracle
test. `--all`, `--always` and `--undefined` remain implemented-but-unverified
until exact rows are added.

Actual post-import movement matched the declaration for matrix and census
counts: `+7` behavior rows, `+7` closed rows, `+0` open rows, `+0`
invalid-input rows, `+1` command with rows, `+5` represented doc-option pairs,
`-6` implemented-but-unverified schema rows and `-1` remaining checklist row.

## Latest Declared Import

Source bucket: census implemented-but-unverified `show-branch` schema
surfaces, with focused stock-oracle test evidence.

Evidence source:

- `git_ref_resolution_compat::show_branch_matches_stock_git_for_local_branch_graphs`

Expected movement:

- behavior rows: `+7`
- closed rows: `+7`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+1`
- missing-or-unclassified oracle functions: expected `-1`
- commands with rows: `+1`
- represented doc-option pairs: expected `+4` for `--sha1-name`, `--all`,
  `--remotes` and `--current`; `-a`/`-r` aliases share the represented
  `arg_id`, while the default and positional rev rows close schema surfaces
- implemented-but-unverified schema rows: expected `-7`
- remaining checklist rows: expected `-1` because `show-branch` now has a
  matrix file
- Rust behavior changes: no

Expected rows:

- `git show-branch`
- `git show-branch main feature`
- `git show-branch --sha1-name main feature`
- `git show-branch --all`
- `git show-branch --remotes`
- `git show-branch --current`
- `git show-branch --current main`

The evidence compares stock Git and Zmin outputs in the focused Rust oracle
test. `show-branch --no-name` remains implemented-but-unverified until an exact
row is added.

Actual post-import movement matched the declaration for matrix and census
counts: `+7` behavior rows, `+7` closed rows, `+0` open rows, `+0`
invalid-input rows, `+1` command with rows, `+4` represented doc-option pairs,
`-7` implemented-but-unverified schema rows and `-1` remaining checklist row.

## Latest Declared Import

Source bucket: census implemented-but-unverified `name-rev` and `show-branch`
schema surfaces, with focused stock-oracle shell evidence.

Evidence source:

- `tools/git-name-rev-show-branch-schema-oracle-smoke.sh`

Expected movement:

- behavior rows: `+2`
- closed rows: `+2`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: expected `+1` for `show-branch --no-name`;
  `name-rev --undefined` closes a schema surface but is not a documented
  option seed pair
- implemented-but-unverified schema rows: expected `-2`
- remaining checklist rows: expected `+0`
- Rust behavior changes: no

Expected rows:

- `git name-rev --undefined <dangling-commit>`
- `git show-branch --no-name main feature`

The evidence compares stock Git and Zmin exit status, stdout, stderr and clean
worktree status using copied workdirs from one seed repository. Probe attempts
for `git name-rev --all` and `git name-rev --always <dangling-commit>` exposed
current output-order/fallback mismatches, so those schema surfaces were not
counted as closed rows in this import.

Actual post-import movement matched the narrowed declaration: `+2` behavior
rows, `+2` closed rows, `+0` open rows, `+0` invalid-input rows, `+0`
represented oracle functions, `+0` missing-or-unclassified oracle functions,
`+0` commands with rows, `+1` represented doc-option pair, `-2`
implemented-but-unverified schema rows and `+0` remaining checklist rows.

## Latest Declared Import

Source bucket: census implemented-but-unverified `name-rev --always` schema
surface plus the fallback behavior gap found by the previous smoke probe.

Evidence source:

- `tools/git-name-rev-show-branch-schema-oracle-smoke.sh`

Expected movement:

- behavior rows: `+1`
- closed rows: `+1`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: expected `+1` for `name-rev --always`
- implemented-but-unverified schema rows: expected `-1`
- remaining checklist rows: expected `+0`
- Rust behavior changes: yes, positional `name-rev` fallback naming

Expected row:

- `git name-rev --always <dangling-commit>`

The evidence compares stock Git and Zmin exit status, stdout, stderr and clean
worktree status using copied workdirs from one seed repository. `git name-rev
--all` remains open because stock output ordering does not match Zmin's current
object-id ordering.

Actual post-import movement matched the declaration: `+1` behavior row, `+1`
closed row, `+0` open rows, `+0` invalid-input rows, `+0` represented oracle
functions, `+0` missing-or-unclassified oracle functions, `+0` commands with
rows, `+1` represented doc-option pair, `-1` implemented-but-unverified schema
row and `+0` remaining checklist rows.

## Latest Declared Import

Source bucket: remaining hard-fail scan unclassified reftable parse guards.

Evidence source:

- `docs/cli/oracle_test_deferrals.md` source-guard deferrals for
  `unsupported reftable version`
- `docs/cli/oracle_test_deferrals.md` source-guard deferrals for
  `unsupported reftable ref value type`

Expected movement:

- behavior rows: `+0`
- closed rows: `+0`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: expected `+0`
- implemented-but-unverified schema rows: expected `+0`
- remaining checklist rows: expected `-2`
- Rust behavior changes: no

Expected rows:

- none; this is classification-only

The goal is to document the last two hard-fail scan hits so they stop showing
up as unclassified backlog without pretending they are closed Git behavior
rows.

Actual post-import movement matched the declaration: `+0` behavior rows, `+0`
closed rows, `+0` open rows, `+0` invalid-input rows, `+0` represented oracle
functions, `+0` missing-or-unclassified oracle functions, `+0` commands with
rows, `+0` represented doc-option pairs, `+0` implemented-but-unverified
schema rows and `-2` remaining checklist rows.

## Latest Declared Import

Source bucket: regenerated oracle inventory `git_foreign_scm_compat.rs`
evidence gaps, with exact helper-trace tests and explicit local stock-helper
availability classification.

Evidence source:

- `git_foreign_scm_compat::cvsexportcommit_exports_text_commit_to_cvs_checkout`
- `git_foreign_scm_compat::cvsimport_imports_cvsps_patchsets_into_git_commits`
- `git_foreign_scm_compat::cvsimport_runs_cvsps_when_patchset_file_is_not_provided`
- `git_foreign_scm_compat::p4_clone_imports_head_revision_into_git_refs_and_worktree`
- `git_foreign_scm_compat::p4_submit_opens_changed_files_and_submits_head`
- `git_foreign_scm_compat::p4_unknown_subcommand_matches_stock_git_usage`
- `git_foreign_scm_compat::svn_clone_imports_head_tree_into_git_svn_ref_and_worktree`
- `git_foreign_scm_compat::svn_dcommit_adds_deletes_commits_and_updates_git_svn_ref`
- `git_foreign_scm_compat::archimport_imports_tree_snapshot_into_git_repo`

Expected movement:

- behavior rows: `+7`
- closed rows: `+0`
- open rows: `+7`
- invalid-input rows: `+0`
- represented oracle functions: `+9`
- missing-or-unclassified oracle functions: `-9`
- commands with rows: `+0`
- represented doc-option pairs: expected `+0`
- implemented-but-unverified schema rows: expected `+0`
- remaining checklist rows: expected `+7`
- Rust behavior changes: no

Expected rows:

- `git cvsexportcommit -w <cvs checkout> HEAD`
- `git cvsimport -a -R -z 0 -P <patchset file> -C <target> -d <cvsroot> module`
- `git cvsimport -C <target> -d <cvsroot> module`
- `git p4 clone --branch master //depot/project <target>`
- `git p4 submit`
- `git svn clone <url> <target>`
- `git svn dcommit`

The `git p4 unknown` rows already existed and only needed exact test evidence
added. `git archimport` already had an exact-open row; this slice rewires it
to the exact focused test and removes the plan-doc placeholder evidence.

Actual post-import movement matched the declaration: `+7` behavior rows, `+0`
closed rows, `+7` open rows, `+0` invalid-input rows, `+9` represented oracle
functions, `-9` missing-or-unclassified oracle functions, `+0` commands with
rows, `+0` represented doc-option pairs, `+0` implemented-but-unverified
schema rows and `+7` remaining checklist rows.

## Latest Declared Import

Source bucket: regenerated oracle inventory `git_sequencer_compat.rs`
evidence gaps, with exact stock-oracle Rust test evidence.

Evidence source:

- `git_sequencer_compat::bisect_terms_next_skip_and_replay_match_stock_git_state`
- `git_sequencer_compat::bisect_custom_terms_and_skip_range_match_stock_git_state`
- `git_sequencer_compat::bisect_skip_reports_skipped_only_candidates_like_stock_git`
- `git_sequencer_compat::bisect_help_view_no_checkout_and_pathspec_cover_stable_modes`
- `git_sequencer_compat::bisect_first_parent_limits_candidates_to_first_parent_chain`
- `git_sequencer_compat::rebase_replays_linear_topic_like_stock_git`
- `git_sequencer_compat::rebase_uses_configured_upstream_like_stock_git`
- `git_sequencer_compat::rebase_with_branch_argument_checks_out_and_replays_like_stock_git`

Expected movement:

- behavior rows: `+23`
- closed rows: `+23`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+8`
- missing-or-unclassified oracle functions: `-8`
- commands with rows: `+0`
- represented doc-option pairs: expected `+0`
- implemented-but-unverified schema rows: expected `+0`
- remaining checklist rows: expected `+0`
- Rust behavior changes: no

Expected rows:

- `git bisect terms`
- `git bisect skip`
- `git bisect replay bisect.log`
- `git bisect start --term-old=fixed --term-new=broken HEAD HEAD~4`
- `git bisect start --no-checkout HEAD HEAD~4`
- `git bisect start --first-parent HEAD HEAD~2`
- `git rebase origin/main`
- `git rebase`
- `git rebase origin/main topic`

This is an evidence-import slice for already-implemented sequencer behavior;
the matrix expansion makes the stable bisect and non-interactive rebase
surface explicit without growing the product backlog.

Actual post-import movement matched the declaration: `+23` behavior rows, `+23`
closed rows, `+0` open rows, `+0` invalid-input rows, `+8` represented oracle
functions, `-8` missing-or-unclassified oracle functions, `+0` commands with
rows, `+0` represented doc-option pairs, `+0` implemented-but-unverified
schema rows and `+0` remaining checklist rows.

## Latest Declared Import

Source bucket: regenerated oracle inventory `git_refs_compat.rs` focused
`update-ref` evidence gaps, with exact stock-oracle Rust test evidence.

Evidence source:

- `git_refs_compat::update_ref_pseudoref_matches_stock_git_and_resolves_revision`
- `git_refs_compat::update_ref_stdin_batch_transactions_match_stock_git`
- `git_refs_compat::update_ref_stdin_batch_updates_match_stock_git`
- `git_refs_compat::update_ref_reflog_updates_match_stock_git`
- `git_refs_compat::update_ref_no_deref_modes_match_stock_git_head_storage`
- `git_refs_compat::update_ref_invalid_refname_failures_match_stock_git`

Expected movement:

- behavior rows: `+2`
- closed rows: `+2`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+6`
- missing-or-unclassified oracle functions: `-6`
- commands with rows: `+0`
- represented doc-option pairs: expected `+0`
- implemented-but-unverified schema rows: expected `+0`
- remaining checklist rows: expected `+0`
- Rust behavior changes: no

Expected rows:

- `git update-ref REVERSE HEAD`
- `git update-ref refs/heads/bad..name <oid>`

The remaining focused evidence maps onto existing `update-ref` matrix rows for
stdin transactions, `--batch-updates`, reflog writes and `--no-deref` HEAD
behavior. This is an evidence-wiring slice, not a new backlog-growth pass.

Actual post-import movement matched the declaration: `+2` behavior rows, `+2`
closed rows, `+0` open rows, `+0` invalid-input rows, `+6` represented oracle
functions, `-6` missing-or-unclassified oracle functions, `+0` commands with
rows, `+0` represented doc-option pairs, `+0` implemented-but-unverified
schema rows and `+0` remaining checklist rows.

## Latest Declared Import

Source bucket: census implemented-but-unverified `update-ref` schema surfaces,
with focused stock-oracle shell evidence.

Evidence source:

- `tools/git-update-ref-oracle-smoke.sh`

Expected movement:

- behavior rows: `+2`
- closed rows: `+2`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: expected `+2` for `--create-reflog` and
  `--no-deref`
- implemented-but-unverified schema rows: expected `-2`
- remaining checklist rows: expected `+0`
- Rust behavior changes: no

Expected rows:

- `git update-ref --create-reflog refs/heads/new <oid>`
- `git update-ref --no-deref HEAD <oid>`

The evidence compares stock Git and Zmin exit status, stdout, stderr, refs,
reflogs and `.git/HEAD` contents using copied workdirs from one seed
repository. Probe attempts showed `git update-ref --deref HEAD <oid>` still has
reflog differences and the three-operand positional form is not parser-compatible
yet, so those surfaces are not counted as closed rows.

Actual post-import movement matched the declaration: `+2` behavior rows, `+2`
closed rows, `+0` open rows, `+0` invalid-input rows, `+0` represented oracle
functions, `+0` missing-or-unclassified oracle functions, `+0` commands with
rows, `+2` represented doc-option pairs, `-2` implemented-but-unverified schema
rows and `+0` remaining checklist rows.

## Latest Declared Import

Source bucket: census implemented-but-unverified `credential` positional
operation schema surface, with focused stock-oracle Rust test evidence.

Evidence source:

- `git_credential_compat::credential_matches_stock_git_for_basic_protocol_flows`

Expected movement:

- behavior rows: `+1`
- closed rows: `+1`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: expected `+0` because this closes a positional
  schema surface rather than a documented option spelling
- implemented-but-unverified schema rows: expected `-1`
- remaining checklist rows: expected `+0`
- Rust behavior changes: no

Expected row:

- `git credential fill`

The evidence compares stock Git and Zmin stdout, stderr, exit status and basic
credential protocol behavior for `fill`, `approve`, `reject` and a missing-field
failure. This row makes the required positional operation parser surface
explicit in the matrix.

Actual post-import movement matched the declaration: `+1` behavior row, `+1`
closed row, `+0` open rows, `+0` invalid-input rows, `+0` represented oracle
functions, `+0` missing-or-unclassified oracle functions, `+0` commands with
rows, `+0` represented doc-option pairs, `-1` implemented-but-unverified schema
row and `+0` remaining checklist rows.

## 2026-06-25 - check-ref-format doc-option parser gap

Expected movement:

- behavior rows: `+6`
- closed rows: `+4`
- open rows: `+0`
- invalid-input rows: `+2`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: `+2`
- implemented-but-unverified schema rows: `+0`
- remaining checklist rows: `+0`
- Rust behavior changes: yes

Expected rows:

- `git check-ref-format --no-allow-onelevel refs/heads/main`
- `git check-ref-format --allow-onelevel --normalize main`
- `git check-ref-format --refspec-pattern foo/bar*baz`
- `git check-ref-format --refspec-pattern foo/bar*/baz`
- `git check-ref-format --refspec-pattern refs/heads/*:refs/remotes/origin/*`
- `git check-ref-format --refspec-pattern foo/*/bar/*`

This batch closes the remaining local parser gap for the documented
`--no-allow-onelevel` and `--refspec-pattern` surfaces. The evidence compares
stock Git and Zmin exit status, stdout and stderr through the focused
`git_check_ref_format_compat` test for accepted default/no-toggle forms,
normalize pairing, valid single-wildcard refspec patterns and invalid
multi-wildcard or colon-containing forms.

Actual post-import movement matched the declaration: `+6` behavior rows, `+4`
closed rows, `+0` open rows, `+2` invalid-input rows, `+0` represented oracle
functions, `+0` missing-or-unclassified oracle functions, `+0` commands with
rows, `+2` represented doc-option pairs, `+0` implemented-but-unverified
schema rows and `+0` remaining checklist rows.

## 2026-06-25 - pack-refs documented option surface expansion

Expected movement:

- behavior rows: `+4`
- closed rows: `+4`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: `+3`
- implemented-but-unverified schema rows: `+0`
- remaining checklist rows: `+0`
- Rust behavior changes: yes

Expected rows:

- `git pack-refs --auto`
- `git pack-refs --include refs/heads/feature`
- `git pack-refs --exclude refs/heads/feature`
- `git pack-refs --all --exclude refs/heads/feature`

This batch closes the next local `git pack-refs` parser gap by adding the
documented `--auto`, `--include` and `--exclude` surfaces and wiring them to
stock-shaped files-backend behavior. The evidence compares stock Git and Zmin
via the focused `git_maintenance_compat` test on packed-refs content and
remaining loose refs for auto, include-only, exclude-only and all-plus-exclude
cases.

Actual post-import movement matched the declaration: `+4` behavior rows, `+4`
closed rows, `+0` open rows, `+0` invalid-input rows, `+0` represented oracle
functions, `+0` missing-or-unclassified oracle functions, `+0` commands with
rows, `+3` represented doc-option pairs, `+0` implemented-but-unverified
schema rows and `+0` remaining checklist rows.

## 2026-06-25 - bundle quiet parser surface

Expected movement:

- behavior rows: `+3`
- closed rows: `+3`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: `+2`
- implemented-but-unverified schema rows: `+0`
- remaining checklist rows: `+0`
- Rust behavior changes: yes

Expected rows:

- `git bundle create --quiet repo.bundle HEAD`
- `git bundle create -q repo.bundle HEAD`
- `git bundle verify --quiet repo.bundle`

This batch closes the next local `git bundle` parser gap by adding the
documented `--quiet` / `-q` surface and wiring it to stock-compatible
`bundle create` and `bundle verify` behavior. The evidence compares stock Git
and Zmin exit status, stdout, stderr and bundle side effects for quiet create,
short quiet create and quiet verify.

Actual post-import movement matched the declaration: `+3` behavior rows, `+3`
closed rows, `+0` open rows, `+0` invalid-input rows, `+0` represented oracle
functions, `+0` missing-or-unclassified oracle functions, `+0` commands with
rows, `+2` represented doc-option pairs, `+0` implemented-but-unverified
schema rows and `+0` remaining checklist rows.

## 2026-06-26 - checkout pathspec-file family

Expected movement:

- behavior rows: `+4`
- closed rows: `+3`
- open rows: `+0`
- invalid-input rows: `+1`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: `+2`
- implemented-but-unverified schema rows: `+0`
- remaining checklist rows: `+0`
- Rust behavior changes: yes

Expected rows:

- `git checkout --pathspec-from-file=paths.txt`
- `git checkout --pathspec-from-file paths.txt`
- `git checkout --pathspec-from-file paths.nul --pathspec-file-nul`
- `git checkout --pathspec-file-nul`

This batch closes the helper-free local `git checkout` pathspec-file family by
adding `--pathspec-from-file` and `--pathspec-file-nul` to the schema/runtime
and proving stock parity for LF-delimited and NUL-delimited tracked-path
restore plus the missing-`--pathspec-from-file` rejection. The focused
evidence uses `git_worktree_state_compat` against stock Git 2.53.0 through the
shared `stock_git_bin` test helper.

Actual post-import movement matched the durable shape with one backlog nuance:
`+4` behavior rows, `+3` closed rows, `+0` open rows, `+1` invalid-input row,
`+0` represented oracle functions, `+0` missing-or-unclassified oracle
functions, `+0` commands with rows, `+2` represented doc-option pairs, `+0`
implemented-but-unverified schema rows, and `+0` remaining checklist rows.
The `checkout` pathspec options moved from `doc_option_not_in_zmin_schema` to
`doc_option_expansion_required`, so they remain in the checklist until the
full newer-option family covers negations, repeated forms and other stateful
lanes.

## 2026-06-26 - checkout path modifiers and negated pathspec flags

Expected movement:

- behavior rows: `+14`
- closed rows: `+14`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: `+8`
- implemented-but-unverified schema rows: `+0`
- remaining checklist rows: `+0`
- Rust behavior changes: yes

Expected rows:

- `git checkout --guess .`
- `git checkout --no-guess .`
- `git checkout --progress .`
- `git checkout --overlay .`
- `git checkout --no-overlay .`
- `git checkout --ignore-skip-worktree-bits .`
- `git checkout --ours .`
- `git checkout --theirs .`
- `git checkout -2 .`
- `git checkout -3 .`
- `git checkout --no-pathspec-from-file a.txt`
- `git checkout --no-pathspec-from-file --no-pathspec-from-file a.txt`
- `git checkout --no-pathspec-file-nul a.txt`
- `git checkout --no-pathspec-file-nul --no-pathspec-file-nul a.txt`

This batch closes a helper-free local `git checkout` path-restore modifier
family by exposing stock-compatible parser/runtime handling for the documented
guess, progress, overlay, skip-worktree and ours/theirs spellings on the
current dot-pathspec lane, plus repeated negated pathspec-file flags on the
single-path restore lane. The focused evidence uses
`git_worktree_state_compat` against stock Git 2.53.0 through the shared
`stock_git_bin` test helper.

Actual post-import movement matched the declared closure size after adding the
short `-2` / `-3` alias rows to eliminate transient schema-only debt:
`+14` behavior rows, `+14` closed rows, `+0` open rows, `+0` invalid-input
rows, `+0` represented oracle functions, `+0` missing-or-unclassified oracle
functions, `+0` commands with rows, `+8` represented doc-option pairs, `+0`
implemented-but-unverified schema rows, and `+0` remaining checklist rows.

## 2026-06-26 - checkout merge conflict path family

Expected movement:

- behavior rows: `+6`
- closed rows: `+6`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: `+3`
- implemented-but-unverified schema rows: `+0`
- remaining checklist rows: `+0`
- Rust behavior changes: yes

Expected rows:

- `git checkout --merge .`
- `git checkout -m .`
- `git checkout --conflict=merge .`
- `git checkout --conflict=diff3 .`
- `git checkout --conflict=zdiff3 .`
- `git checkout --conflict merge .`

This batch closes the helper-free local `git checkout` merge/conflict family
on the non-conflicted dot-pathspec restore lane by exposing stock-compatible
parser/runtime handling for `--merge`, `-m`, and the documented conflict style
spellings without introducing branch-switching or real conflict-resolution
logic into the slice. The focused evidence uses
`git_worktree_state_compat` against stock Git 2.53.0 through the shared
`stock_git_bin` test helper.

Actual post-import movement matched the declaration: `+6` behavior rows, `+6`
closed rows, `+0` open rows, `+0` invalid-input rows, `+0` represented oracle
functions, `+0` missing-or-unclassified oracle functions, `+0` commands with
rows, `+3` represented doc-option pairs, `+0` implemented-but-unverified
schema rows, and `+0` remaining checklist rows.

## 2026-06-26 - checkout track invalid-input family

Expected movement:

- behavior rows: `+9`
- closed rows: `+0`
- open rows: `+0`
- invalid-input rows: `+9`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: `+3`
- implemented-but-unverified schema rows: `+0`
- remaining checklist rows: `+0`
- Rust behavior changes: yes

Expected rows:

- `git checkout --track .`
- `git checkout -t .`
- `git checkout --no-track .`
- `git checkout --track -- a.txt`
- `git checkout -t -- a.txt`
- `git checkout --no-track -- a.txt`
- `git checkout --track a.txt`
- `git checkout -t a.txt`
- `git checkout --no-track a.txt`

This batch closes the helper-free local `git checkout` track-family parser
surface as stock-compatible invalid input. The focused rows capture the two
stock fatal shapes that matter locally: `missing branch name; try -b` for the
path-like forms and `--track needs a branch name` for explicit separator
forms. The evidence uses `git_worktree_state_compat` against stock Git 2.53.0
through the shared `stock_git_bin` test helper.

Actual post-import movement matched the declaration: `+9` behavior rows, `+0`
closed rows, `+0` open rows, `+9` invalid-input rows, `+0` represented oracle
functions, `+0` missing-or-unclassified oracle functions, `+0` commands with
rows, `+3` represented doc-option pairs, `+0` implemented-but-unverified
schema rows, and `+0` remaining checklist rows.

## 2026-06-25 - repack cruft-family schema-tail closure

Expected movement:

- behavior rows: `+8`
- closed rows: `+8`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: `+4`
- implemented-but-unverified schema rows: `+0`
- remaining checklist rows: `+0`
- Rust behavior changes: yes

Expected rows:

- `git repack --cruft -d -q`
- `git repack --cruft --cruft-expiration=now -d -q`
- `git repack --cruft --expire-to=out --cruft-expiration=now -d -q`
- `git repack --cruft --max-cruft-size=1 -d -q`
- `git repack --cruft --max-cruft-size=2m -d -q`
- `git repack --cruft --max-cruft-size=bogus -d -q`
- `git repack --cruft -k -d -q`
- `git repack --cruft --cruft-expiration=now --max-cruft-size=1 -d -q` or another focused cruft row if stock evidence diverges

This batch closes the next large helper-free `git repack` schema tail without
opening filter or geometric complexity. The parser adds `--cruft`,
`--cruft-expiration`, `--expire-to`, and `--max-cruft-size`; the runtime then
matches stock Git for the covered local `-d -q` cruft lane, including the
side-pack artifact shape for `--expire-to=out --cruft-expiration=now`, the
1 MiB warning floor, invalid size rejection, and the `--cruft -k` fatal
conflict.

Actual post-import movement matched the declaration except the eighth row was
used for the stock `--max-cruft-size=2m` acceptance lane instead of a mixed
cruft-expiration-plus-size row: `+8` behavior rows, `+8` closed rows, `+0`
open rows, `+0` invalid-input rows, `+0` represented oracle functions, `+0`
missing-or-unclassified oracle functions, `+0` commands with rows, `+4`
represented doc-option pairs, `+0` implemented-but-unverified schema rows and
`+0` remaining checklist rows.

## 2026-06-25 - read-tree repeated parser and evidence closure

Expected movement:

- behavior rows: `+25`
- closed rows: `+25`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: `+0`
- implemented-but-unverified schema rows: `+0`
- remaining checklist rows: `-13`
- Rust behavior changes: yes

Expected rows:

- repeated and reordered `--dry-run`, `-n`, `--quiet`, `-q`, `-v`
- repeated and reordered `--trivial` and `--aggressive`
- repeated `--reset` and reordered `--index-output` plus `--reset`
- repeated `--no-sparse-checkout`
- repeated and triple-order `--recurse-submodules` / `--no-recurse-submodules`
- repeated `-i`, separate `--index-output` value form, and repeated
  `--index-output` last-one-wins

This batch closes the remaining helper-free parser/evidence tail for the
current `git read-tree` schema surface instead of adding another small
evidence-only slice. The parser now accepts stock-compatible repeats for the
documented long booleans and for `-i`, plus repeated `--index-output` with
last-one-wins semantics. The focused oracle test then proves the repeated and
reordered read-tree rows against stock Git and promotes the covered documented
option pairs into reviewed-complete census state.

Actual post-import movement matched the declaration: `+25` behavior rows, `+25`
closed rows, `+0` open rows, `+0` invalid-input rows, `+0` represented oracle
functions, `+0` missing-or-unclassified oracle functions, `+0` commands with
rows, `+0` represented doc-option pairs, `+0` implemented-but-unverified
schema rows and `-13` remaining checklist rows.

## 2026-06-25 - read-tree update-worktree completion batch

Expected movement:

- behavior rows: `+15`
- closed rows: `+15`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: `+0`
- implemented-but-unverified schema rows: `+0`
- remaining checklist rows: `-1`
- Rust behavior changes: no

Expected rows:

- repeated `-u` with `-m`, `--reset`, and `--prefix=import/`
- quiet ordering with `-u --reset` and `-u --prefix=import/`
- `-u` with `--trivial --reset` and `--aggressive --reset`
- `-u` plus `--index-output` ordering with `--reset` and `--prefix=import/`

This batch closes the last expansion-required documented `read-tree -u`
surface inside the current helper-free schema lane. No runtime change is
required because the parser already accepts repeated `-u`; the work is purely
focused stock-Git evidence plus durable matrix/census promotion for the now
complete documented option family.

Actual post-import movement matched the declaration: `+15` behavior rows, `+15`
closed rows, `+0` open rows, `+0` invalid-input rows, `+0` represented oracle
functions, `+0` missing-or-unclassified oracle functions, `+0` commands with
rows, `+0` represented doc-option pairs, `+0` implemented-but-unverified
schema rows and `-1` remaining checklist row.

## 2026-06-25 - checkout-index remaining documented surface closure

Expected movement:

- behavior rows: `+12`
- closed rows: `+12`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: `+8`
- implemented-but-unverified schema rows: `+0`
- remaining checklist rows: `-8`
- Rust behavior changes: yes

Expected rows:

- `git checkout-index -n -a`
- `git checkout-index --no-create -a`
- `git checkout-index -n --no-create -a`
- `git checkout-index -u README.md`
- `git checkout-index --index README.md`
- `git checkout-index -u --index README.md`
- `git checkout-index -a --ignore-skip-worktree-bits`
- `git checkout-index --temp README.md`
- `git checkout-index --stage=all f`
- `git checkout-index --stage=2 --temp f`
- `printf 'README.md\0' | git checkout-index -z --stdin`
- `printf 'README.md\0docs/guide.md\0' | git checkout-index -z --stdin --prefix=out/`

This batch closes the remaining local `git checkout-index` documented parser
surface by wiring `--index` / `-u`, `--no-create` / `-n`, `--temp`,
`--stage`, `--ignore-skip-worktree-bits`, and `-z`, then proving them against
stock Git through focused worktree-state parity tests. The new evidence covers
metadata refresh, skip-worktree selection during `--all`, direct and
stage-composed temp-file export, and NUL-delimited stdin restore and prefix
export flows.

Actual post-import movement matched the declaration: `+12` behavior rows, `+12`
closed rows, `+0` open rows, `+0` invalid-input rows, `+0` represented oracle
functions, `+0` missing-or-unclassified oracle functions, `+0` commands with
rows, `+8` represented doc-option pairs, `+0` implemented-but-unverified
schema rows and `-8` remaining checklist rows. The reviewed-complete census now
marks `checkout-index` `16/16` documented option pairs and the full command
matrix as finished.

## 2026-06-25 - read-tree helper-free parser surface batch

Expected movement:

- behavior rows: `+12`
- closed rows: `+10`
- open rows: `+0`
- invalid-input rows: `+2`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: `+10`
- implemented-but-unverified schema rows: `+0`
- remaining checklist rows: `+0`
- Rust behavior changes: yes

Expected rows:

- `git read-tree --dry-run <tree>`
- `git read-tree -n <tree>`
- `git read-tree --quiet <tree>`
- `git read-tree -q <tree>`
- `git read-tree --reset <tree>`
- `git read-tree --index-output=alt.index <tree>`
- `git read-tree -i --prefix=import/ <tree>`
- `git read-tree -i <tree>`
- `git read-tree -i --index-output=alt.index <tree>`
- `git read-tree --no-sparse-checkout <tree>`
- `git read-tree --recurse-submodules <tree>`
- `git read-tree --no-recurse-submodules <tree>`

This batch closes the helper-free `git read-tree` parser surface that does not
require new merge or worktree-update machinery. Zmin now accepts and matches
stock Git for dry-run, quiet, reset, alternate-index output, index-only prefix
import, and the no-op sparse/submodule toggles on simple repositories, while
also matching the stock fatal `-i` guard when it is used without `-m`,
`--reset`, or `--prefix`.

Actual post-import movement matched the declaration: `+12` behavior rows, `+10`
closed rows, `+0` open rows, `+2` invalid-input rows, `+0` represented oracle
functions, `+0` missing-or-unclassified oracle functions, `+0` commands with
rows, `+10` represented doc-option pairs, `+0` implemented-but-unverified
schema rows and `+0` remaining checklist rows. `read-tree` is now `13/17`
represented documented option pairs with only `--aggressive`, `--trivial`,
`-u`, and `-v` still outside the schema.

## 2026-06-25 - archive reviewed-complete command promotion

Expected movement:

- behavior rows: `+0`
- closed rows: `+0`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: `+0`
- implemented-but-unverified schema rows: `+0`
- remaining checklist rows: `+0`
- Rust behavior changes: no

Expected promotion:

- `archive` into `docs/cli/census/reviewed_complete_command_matrices.tsv`

This is a zero-code census closure. `archive` now has `14/14` documented option
pairs reviewed complete, `24/24` classified rows, no remaining checklist rows,
and exact stock-Git evidence for local and remote archive creation, format
listing, output inference, mtime parsing, verbose output, worktree attributes,
and invalid-format rejection. The earlier backend-tail concern is no longer
reflected in the current census, so the durable command-level promotion is now
safe.

Actual post-import movement matched the declaration: `+0` behavior rows, `+0`
closed rows, `+0` open rows, `+0` invalid-input rows, `+0` represented oracle
functions, `+0` missing-or-unclassified oracle functions, `+0` commands with
rows, `+0` represented doc-option pairs, `+0` implemented-but-unverified
schema rows and `+0` remaining checklist rows. The reviewed-complete census now
marks `archive` as a finished command matrix.

## 2026-06-25 - check-ref-format precedence and branch-history surface

Expected movement:

- behavior rows: `+10`
- closed rows: `+7`
- open rows: `+0`
- invalid-input rows: `+3`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: `+0`
- implemented-but-unverified schema rows: `+0`
- remaining checklist rows: `+0`
- Rust behavior changes: yes

Expected rows:

- `git check-ref-format --allow-onelevel --no-allow-onelevel main`
- `git check-ref-format --no-allow-onelevel --allow-onelevel main`
- `git check-ref-format --allow-onelevel --refspec-pattern foo*`
- `git check-ref-format --normalize --refspec-pattern /refs//heads/*`
- `git check-ref-format --branch @{-1}`
- `git check-ref-format --branch @{-1}` in detached-checkout history
- `git check-ref-format --branch @{-2}`
- `git check-ref-format --branch @{-3}`
- `git check-ref-format --branch -bad`
- `git check-ref-format --branch @{-99}`

This batch closes the next cheap `git check-ref-format` parser-plus-evidence
surface by teaching Zmin to honor last-one-wins
`--allow-onelevel` / `--no-allow-onelevel`, accept leading-dash branch values
as branch operands so they can be rejected with the stock fatal error, and
expand branch-mode previous-checkout syntax through HEAD reflog history like
stock Git. The focused evidence compares exact stdout, stderr and exit codes
for the new precedence, refspec and branch-history rows through
`git_check_ref_format_compat`.

Actual post-import movement matched the declaration: `+10` behavior rows, `+7`
closed rows, `+0` open rows, `+3` invalid-input rows, `+0` represented oracle
functions, `+0` missing-or-unclassified oracle functions, `+0` commands with
rows, `+0` represented doc-option pairs, `+0`
implemented-but-unverified schema rows and `+0` remaining checklist rows.

## 2026-06-25 - refs verify no-toggle surface

Expected movement:

- behavior rows: `+8`
- closed rows: `+8`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: `+0`
- implemented-but-unverified schema rows: `+0`
- remaining checklist rows: `+0`
- Rust behavior changes: yes

Expected rows:

- `git refs verify --no-verbose`
- `git refs verify --no-strict`
- `git refs verify --strict --verbose`
- `git refs verify --verbose --strict`
- `git refs verify --strict --no-strict`
- `git refs verify --no-strict --strict`
- `git refs verify --verbose --no-verbose`
- `git refs verify --no-verbose --verbose`

This batch closes the next cheap local `git refs verify` toggle subgroup by
teaching Zmin to accept the documented `--[no-]verbose` and `--[no-]strict`
forms, and to honor last-one-wins ordering for both switches like stock Git.
The focused evidence compares exact stdout, stderr and exit codes for the
healthy-repository rows through `git_refs_compat`.

Actual post-import movement matched the declaration: `+8` behavior rows, `+8`
closed rows, `+0` open rows, `+0` invalid-input rows, `+0` represented oracle
functions, `+0` missing-or-unclassified oracle functions, `+0` commands with
rows, `+0` represented doc-option pairs, `+0`
implemented-but-unverified schema rows and `+0` remaining checklist rows.

## 2026-06-25 - bundle invalid-version quiet composition surface

Expected movement:

- behavior rows: `+8`
- closed rows: `+0`
- open rows: `+0`
- invalid-input rows: `+8`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: `+0`
- implemented-but-unverified schema rows: `+0`
- remaining checklist rows: `+0`
- Rust behavior changes: no

Expected rows:

- `git bundle create --version=1 --quiet repo.bundle HEAD`
- `git bundle create --quiet --version=1 repo.bundle HEAD`
- `git bundle create --version=1 --no-quiet repo.bundle HEAD`
- `git bundle create --no-quiet --version=1 repo.bundle HEAD`
- `git bundle create --version=foo --quiet repo.bundle HEAD`
- `git bundle create --quiet --version=foo repo.bundle HEAD`
- `git bundle create --version= --quiet repo.bundle HEAD`
- `git bundle create --quiet --version= repo.bundle HEAD`

This batch closes the next cheap local `git bundle` invalid-input subgroup by
making the existing stock-Git rejections for unsupported or unparsable bundle
versions explicit when composed with `--quiet` and `--no-quiet`. The focused
evidence compares exact stdout, stderr and exit codes through
`git_pack_integrity_compat`.

Actual post-import movement matched the declaration: `+8` behavior rows, `+0`
closed rows, `+0` open rows, `+8` invalid-input rows, `+0` represented oracle
functions, `+0` missing-or-unclassified oracle functions, `+0` commands with
rows, `+0` represented doc-option pairs, `+0`
implemented-but-unverified schema rows and `+0` remaining checklist rows.

## 2026-06-25 - bundle short quiet version surface

Expected movement:

- behavior rows: `+12`
- closed rows: `+6`
- open rows: `+0`
- invalid-input rows: `+6`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: `+0`
- implemented-but-unverified schema rows: `+0`
- remaining checklist rows: `+0`
- Rust behavior changes: no

Expected rows:

- `git bundle create -q --version=2 repo.bundle HEAD`
- `git bundle create --version=2 -q repo.bundle HEAD`
- `git bundle create -q --version=3 repo.bundle HEAD`
- `git bundle create --version=3 -q repo.bundle HEAD`
- `git bundle create -q --version=-1 repo.bundle HEAD`
- `git bundle create --version=-1 -q repo.bundle HEAD`
- `git bundle create -q --version=1 repo.bundle HEAD`
- `git bundle create --version=1 -q repo.bundle HEAD`
- `git bundle create -q --version=foo repo.bundle HEAD`
- `git bundle create --version=foo -q repo.bundle HEAD`
- `git bundle create -q --version= repo.bundle HEAD`
- `git bundle create --version= -q repo.bundle HEAD`

This batch closes the next cheap local `git bundle` subgroup by making the
short quiet alias `-q` explicit when composed with versioned `create`,
including both accepted and rejected version values. The focused evidence
compares exact stdout, stderr and exit codes, plus stock verification for
created bundles, through `git_pack_integrity_compat`.

Actual post-import movement matched the declaration: `+12` behavior rows, `+6`
closed rows, `+0` open rows, `+6` invalid-input rows, `+0` represented oracle
functions, `+0` missing-or-unclassified oracle functions, `+0` commands with
rows, `+0` represented doc-option pairs, `+0`
implemented-but-unverified schema rows and `+0` remaining checklist rows.

## 2026-06-25 - command progress classified closure metric

Expected movement:

- behavior rows: `+0`
- closed rows: `+0`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: `+0`
- implemented-but-unverified schema rows: `+0`
- remaining checklist rows: `+0`
- Rust behavior changes: no

Expected outputs:

- `tools/git-compat-command-summary.sh` emits `behavior_rows_classified`
- `docs/cli/census/command_progress.tsv` includes classified row counts and pct
- `tools/git-cli-readiness-status.sh` prints classified rows alongside verified

This docs-only tooling batch fixes the per-command closure metric so
`command_progress.tsv` distinguishes exact verified-success rows from total
classified rows (`verified + invalid-input parity`). That removes false
"remaining" impressions for commands whose matrices are fully classified but
contain many stock-compatible rejection rows.

Actual post-import movement matched the declaration: `+0` behavior rows, `+0`
closed rows, `+0` open rows, `+0` invalid-input rows, `+0` represented oracle
functions, `+0` missing-or-unclassified oracle functions, `+0` commands with
rows, `+0` represented doc-option pairs, `+0`
implemented-but-unverified schema rows and `+0` remaining checklist rows.

## 2026-06-25 - reviewed-complete closure batch for seven helper-free commands

Expected movement:

- behavior rows: `+0`
- closed rows: `+0`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: `+0`
- implemented-but-unverified schema rows: `+0`
- remaining checklist rows: `-28`
- complete command matrices: `+7`
- complete doc-option pairs: `+28`
- Rust behavior changes: no

Expected commands:

- `bundle`
- `check-attr`
- `check-ref-format`
- `commit-graph`
- `merge-base`
- `pack-refs`
- `refs`

This docs-only census batch promotes seven already fully classified
helper-free commands into the durable reviewed-complete command and
doc-option-pair lists. Each command already had `100%` represented documented
options, `0` exact-open rows, and `100%` classified written-row coverage, so
the remaining work was only to mark those surfaces as reviewed complete and
regenerate the census.

Actual post-import movement matched the declaration: `+0` behavior rows, `+0`
closed rows, `+0` open rows, `+0` invalid-input rows, `+0` represented oracle
functions, `+0` missing-or-unclassified oracle functions, `+0` commands with
rows, `+0` represented doc-option pairs, `+0`
implemented-but-unverified schema rows, `-28` remaining checklist rows,
`+7` complete command matrices, and `+28` complete doc-option pairs.

## 2026-06-25 - interpret-trailers no-* reset closure and reviewed-complete promotion

Expected movement:

- behavior rows: `+3`
- closed rows: `+3`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: `+3`
- implemented-but-unverified schema rows: `+0`
- remaining checklist rows: `-14`
- complete command matrices: `+1`
- complete doc-option pairs: `+14`
- Rust behavior changes: yes

Expected rows:

- `git interpret-trailers --where before --no-where --trailer 'Acked-by: C' < message.txt`
- `git interpret-trailers --if-exists add --no-if-exists --trailer 'Acked-by: B' < message.txt`
- `git interpret-trailers --if-missing doNothing --no-if-missing --trailer 'Reviewed-by: C' < message.txt`

This batch closes the remaining local `git interpret-trailers` documented
parser tail by adding the three documented `--no-*` reset forms and matching
their stock-Git last-one-wins behavior against earlier `--where`,
`--if-exists`, and `--if-missing` overrides. With all `14/14` documented
options represented, `22/22` written rows classified, and `0` open rows, the
same slice then promotes `interpret-trailers` into the durable
reviewed-complete command and doc-option-pair lists.

Actual post-import movement matched the declaration: `+3` behavior rows, `+3`
closed rows, `+0` open rows, `+0` invalid-input rows, `+0` represented oracle
functions, `+0` missing-or-unclassified oracle functions, `+0` commands with
rows, `+3` represented doc-option pairs, `+0`
implemented-but-unverified schema rows, `-14` remaining checklist rows,
`+1` complete command matrix, and `+14` complete doc-option pairs.

## 2026-06-25 - repack documented-option family batch

Expected movement:

- behavior rows: `+13`
- closed rows: `+13`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: `+0`
- implemented-but-unverified schema rows: `+0`
- remaining checklist rows: `-10`
- complete command matrices: `+0`
- complete doc-option pairs: `+10`
- Rust behavior changes: yes

Expected rows:

- `git repack --quiet -q`
- `git repack -q -q`
- `git repack --threads 1`
- `git repack --depth=10 --window=10 -q`
- `git repack --window 10 --depth 10 -q`
- `git repack --write-midx -q`
- `git repack --write-midx -m -q`
- `git repack -m -m -q`
- `git repack --write-bitmap-index -adq`
- `git repack --write-bitmap-index -b -adq`
- `git repack -b -b -adq`
- `git repack -adq --keep-pack <pack1> --keep-pack <pack2>`

This batch closes a large helper-free `git repack` subgroup without opening
the broader unsupported surfaces such as cruft packs or filtering. Zmin now
accepts repeated documented boolean flags for `repack`, matching stock Git for
mixed `--quiet` / `-q` and repeated `-m` / `-b` forms. The new evidence also
covers separate `--threads 1`, value-form and ordering parity for
`--window` / `--depth`, and repeated `--keep-pack` values. With those rows in
place, the reviewed-complete census now marks `10` `repack` documented option
pairs complete.

Actual post-import movement matched the declaration: `+13` behavior rows,
`+13` closed rows, `+0` open rows, `+0` invalid-input rows, `+0`
represented oracle functions, `+0` missing-or-unclassified oracle functions,
`+0` commands with rows, `+0` represented doc-option pairs, `+0`
implemented-but-unverified schema rows, `-10` remaining checklist rows, and
`+10` complete doc-option pairs.

## 2026-06-25 - repack short-option repetition closure

Expected movement:

- behavior rows: `+8`
- closed rows: `+8`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: `+0`
- implemented-but-unverified schema rows: `+0`
- remaining checklist rows: `-7`
- complete command matrices: `+0`
- complete doc-option pairs: `+7`
- Rust behavior changes: no

Expected rows:

- `git repack -a -a -d -q`
- `git repack -d -d -a -q`
- `git repack -A -A -d -q`
- `git repack -A -d -A -q`
- `git repack -f -f -F -a -d -q`
- `git repack -F -F -f -a -d -q`
- `git repack -l -l -a -d -q`
- `git repack -m -n -n -q`

This batch finishes the helper-free supported `repack` synopsis tail. The new
rows prove that repeated and reordered short options keep matching stock Git
for all remaining supported synopsis switches outside the broader unsupported
surfaces such as cruft and filter modes. With these rows in place, all
`17/17` represented documented `repack` option pairs are now reviewed
complete.

Actual post-import movement matched the declaration: `+8` behavior rows,
`+8` closed rows, `+0` open rows, `+0` invalid-input rows, `+0`
represented oracle functions, `+0` missing-or-unclassified oracle functions,
`+0` commands with rows, `+0` represented doc-option pairs, `+0`
implemented-but-unverified schema rows, `-7` remaining checklist rows, and
`+7` complete doc-option pairs.

## 2026-06-25 - commit-graph progress ordering surface

Expected movement:

- behavior rows: `+4`
- closed rows: `+4`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: `+0`
- implemented-but-unverified schema rows: `+0`
- remaining checklist rows: `+0`
- Rust behavior changes: yes

Expected rows:

- `git commit-graph write --reachable --progress --no-progress`
- `git commit-graph write --reachable --no-progress --progress`
- `git commit-graph verify --progress --no-progress`
- `git commit-graph verify --no-progress --progress`

This batch closes the next cheap local `git commit-graph` subgroup by
teaching Zmin to honor last-one-wins ordering for `--progress` and
`--no-progress` on both `write` and `verify` like stock Git. The focused
evidence compares exact stdout, stderr, exit codes and repository state for
the new ordering rows through `git_maintenance_compat`.

Actual post-import movement matched the declaration: `+4` behavior rows, `+4`
closed rows, `+0` open rows, `+0` invalid-input rows, `+0` represented oracle
functions, `+0` missing-or-unclassified oracle functions, `+0` commands with
rows, `+0` represented doc-option pairs, `+0`
implemented-but-unverified schema rows and `+0` remaining checklist rows.

## 2026-06-25 - bundle version 3 and -1 quiet composition surface

Expected movement:

- behavior rows: `+8`
- closed rows: `+8`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: `+0`
- implemented-but-unverified schema rows: `+0`
- remaining checklist rows: `+0`
- Rust behavior changes: no

Expected rows:

- `git bundle create --version=3 --quiet repo.bundle HEAD`
- `git bundle create --quiet --version=3 repo.bundle HEAD`
- `git bundle create --version=3 --no-quiet repo.bundle HEAD`
- `git bundle create --no-quiet --version=3 repo.bundle HEAD`
- `git bundle create --version=-1 --quiet repo.bundle HEAD`
- `git bundle create --quiet --version=-1 repo.bundle HEAD`
- `git bundle create --version=-1 --no-quiet repo.bundle HEAD`
- `git bundle create --no-quiet --version=-1 repo.bundle HEAD`

This batch closes the next cheap local `git bundle` subgroup by making the
existing version `3` and default-sentinel `-1` create paths explicit when
composed with `--quiet` and `--no-quiet`, including both orderings of the
version option. The focused evidence compares exact stdout, stderr, exit codes
and stock verification of the created bundle payload through
`git_pack_integrity_compat`.

Actual post-import movement matched the declaration: `+8` behavior rows, `+8`
closed rows, `+0` open rows, `+0` invalid-input rows, `+0` represented oracle
functions, `+0` missing-or-unclassified oracle functions, `+0` commands with
rows, `+0` represented doc-option pairs, `+0`
implemented-but-unverified schema rows and `+0` remaining checklist rows.

## 2026-06-25 - bundle no-quiet toggle surface

Expected movement:

- behavior rows: `+6`
- closed rows: `+6`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: `+0`
- implemented-but-unverified schema rows: `+0`
- remaining checklist rows: `+0`
- Rust behavior changes: yes

Expected rows:

- `git bundle create --no-quiet repo.bundle HEAD`
- `git bundle create --quiet --no-quiet repo.bundle HEAD`
- `git bundle create --no-quiet --quiet repo.bundle HEAD`
- `git bundle verify --no-quiet repo.bundle`
- `git bundle verify --quiet --no-quiet repo.bundle`
- `git bundle verify --no-quiet --quiet repo.bundle`

This batch closes the next cheap local `git bundle` subgroup by teaching Zmin
to accept the documented `--no-quiet` form and honor last-one-wins ordering
against `--quiet` for both `create` and `verify` like stock Git. The focused
evidence compares exact stdout, stderr, exit codes and bundle side effects for
the new quiet-toggle rows through `git_pack_integrity_compat`.

Actual post-import movement matched the declaration: `+6` behavior rows, `+6`
closed rows, `+0` open rows, `+0` invalid-input rows, `+0` represented oracle
functions, `+0` missing-or-unclassified oracle functions, `+0` commands with
rows, `+0` represented doc-option pairs, `+0`
implemented-but-unverified schema rows and `+0` remaining checklist rows.

## 2026-06-25 - bundle version quiet composition surface

Expected movement:

- behavior rows: `+4`
- closed rows: `+4`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: `+0`
- implemented-but-unverified schema rows: `+0`
- remaining checklist rows: `+0`
- Rust behavior changes: no

Expected rows:

- `git bundle create --version=2 --quiet repo.bundle HEAD`
- `git bundle create --quiet --version=2 repo.bundle HEAD`
- `git bundle create --version=2 --no-quiet repo.bundle HEAD`
- `git bundle create --no-quiet --version=2 repo.bundle HEAD`

This batch closes the next cheap local `git bundle` subgroup by making the
existing `--version=2` create path explicit when composed with `--quiet` and
`--no-quiet`, including both orderings of the version option. The focused
evidence compares exact stdout, stderr, exit codes and stock verification of
the created bundle payload through `git_pack_integrity_compat`.

Actual post-import movement matched the declaration: `+4` behavior rows, `+4`
closed rows, `+0` open rows, `+0` invalid-input rows, `+0` represented oracle
functions, `+0` missing-or-unclassified oracle functions, `+0` commands with
rows, `+0` represented doc-option pairs, `+0`
implemented-but-unverified schema rows and `+0` remaining checklist rows.

## 2026-06-25 - bundle progress ordering surface

Expected movement:

- behavior rows: `+4`
- closed rows: `+4`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: `+0`
- implemented-but-unverified schema rows: `+0`
- remaining checklist rows: `+0`
- Rust behavior changes: yes

Expected rows:

- `git bundle create --progress --no-progress repo.bundle HEAD`
- `git bundle create --no-progress --progress repo.bundle HEAD`
- `git bundle unbundle --progress --no-progress repo.bundle`
- `git bundle unbundle --no-progress --progress repo.bundle`

This batch closes the next cheap local `git bundle` subgroup by teaching Zmin
to honor last-one-wins ordering for `--progress` and `--no-progress` on both
`create` and `unbundle` like stock Git. The focused evidence compares exact
stdout, stderr, exit codes and bundle side effects for the new ordering rows
through `git_pack_integrity_compat`.

Actual post-import movement matched the declaration: `+4` behavior rows, `+4`
closed rows, `+0` open rows, `+0` invalid-input rows, `+0` represented oracle
functions, `+0` missing-or-unclassified oracle functions, `+0` commands with
rows, `+0` represented doc-option pairs, `+0`
implemented-but-unverified schema rows and `+0` remaining checklist rows.

## 2026-06-25 - commit-graph progress and object-dir surface

Expected movement:

- behavior rows: `+6`
- closed rows: `+6`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: `+3`
- implemented-but-unverified schema rows: `+0`
- remaining checklist rows: `+0`
- Rust behavior changes: yes

Expected rows:

- `git commit-graph write --reachable --progress`
- `git commit-graph write --reachable --no-progress`
- `git commit-graph write --object-dir=.git/objects --reachable`
- `git commit-graph verify --progress`
- `git commit-graph verify --no-progress`
- `git commit-graph verify --object-dir=.git/objects`

This batch closes the smallest local `git commit-graph` documented parser
subgroup by adding `--progress`, `--no-progress` and `--object-dir` for
`write` and `verify`, plus stock-like verify progress stderr. It also fixes
the census/command-summary nested-subcommand option accounting so parent
`commit-graph` command progress reflects child subcommand rows.

Actual post-import movement matched the declaration: `+6` behavior rows, `+6`
closed rows, `+0` open rows, `+0` invalid-input rows, `+0` represented oracle
functions, `+0` missing-or-unclassified oracle functions, `+0` commands with
rows, `+3` represented doc-option pairs, `+0` implemented-but-unverified
schema rows and `+0` remaining checklist rows.

## 2026-06-25 - maintenance no-auto and no-quiet surface

Expected movement:

- behavior rows: `+6`
- closed rows: `+6`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: `+0`
- implemented-but-unverified schema rows: `+0`
- remaining checklist rows: `+0`
- Rust behavior changes: yes

Expected rows:

- `git maintenance run --task=gc --no-quiet`
- `git maintenance run --task=gc --quiet --no-quiet`
- `git maintenance run --task=gc --no-quiet --quiet`
- `git maintenance run --task=gc --no-auto`
- `git maintenance run --task=gc --auto --no-auto`
- `git maintenance run --task=gc --no-auto --auto`

This batch closes the next helper-free `git maintenance` option subgroup by
adding `--no-auto` and `--no-quiet` plus stock last-one-wins interaction with
`--auto` and `--quiet`.

Actual post-import movement matched the declaration: `+6` behavior rows, `+6`
closed rows, `+0` open rows, `+0` invalid-input rows, `+0` represented oracle
functions, `+0` missing-or-unclassified oracle functions, `+0` commands with
rows, `+0` represented doc-option pairs, `+0` implemented-but-unverified
schema rows and `+0` remaining checklist rows.

## 2026-06-25 - refs invalid-option parity surface

Expected movement:

- behavior rows: `+4`
- closed rows: `+0`
- open rows: `+0`
- invalid-input rows: `+4`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: `+2`
- implemented-but-unverified schema rows: `+0`
- remaining checklist rows: `+0`
- Rust behavior changes: yes

Expected rows:

- `git refs verify --dry-run`
- `git refs verify --ref-format=files`
- `git refs verify --ref-format=reftable`
- `git refs verify --ref-format=`

This batch closes the remaining `git refs` documented option spellings for
Git `2.47.1` by teaching Zmin to reject unsupported `verify` flags with the
same exit code, stderr and usage block as stock Git.

Actual post-import movement matched the declaration: `+4` behavior rows, `+0`
closed rows, `+0` open rows, `+4` invalid-input rows, `+0` represented oracle
functions, `+0` missing-or-unclassified oracle functions, `+0` commands with
rows, `+2` represented doc-option pairs, `+0` implemented-but-unverified
schema rows and `+0` remaining checklist rows.

## 2026-06-25 - maintenance no-schedule invalid-input surface

Expected movement:

- behavior rows: `+5`
- closed rows: `+0`
- open rows: `+0`
- invalid-input rows: `+5`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: `+0`
- implemented-but-unverified schema rows: `+0`
- remaining checklist rows: `+0`
- Rust behavior changes: yes

Expected rows:

- `git maintenance run --no-schedule`
- `git maintenance run --schedule=daily --no-schedule`
- `git maintenance run --task=gc --no-schedule`
- `git maintenance run --schedule=hourly --no-schedule --task=gc`
- `git maintenance run --no-schedule --schedule=hourly --task=gc`

This batch closes the next cheap invalid-input subgroup for `git maintenance`
by teaching Zmin to reject `--no-schedule` with the same fatal exit `128` and
stderr as stock Git, regardless of option order with `--schedule` and
`--task`.

Actual post-import movement matched the declaration: `+5` behavior rows, `+0`
closed rows, `+0` open rows, `+5` invalid-input rows, `+0` represented oracle
functions, `+0` missing-or-unclassified oracle functions, `+0` commands with
rows, `+0` represented doc-option pairs, `+0` implemented-but-unverified
schema rows and `+0` remaining checklist rows.

## 2026-06-25 - bundle progress parser surface

Expected movement:

- behavior rows: `+5`
- closed rows: `+5`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: `+1`
- implemented-but-unverified schema rows: `+0`
- remaining checklist rows: `+0`
- Rust behavior changes: yes

Expected rows:

- `git bundle create --progress repo.bundle HEAD`
- `git bundle create --quiet --progress repo.bundle HEAD`
- `git bundle unbundle --progress repo.bundle`
- `git bundle unbundle --progress repo.bundle refs/heads/*`
- `git bundle unbundle --no-progress repo.bundle`

This batch closes the remaining local `git bundle` documented progress surface
by adding parser support for `--progress` and wiring stock-compatible progress
stderr for `create` and `unbundle`, plus `--no-progress` suppression for the
unbundle path. The evidence compares stock Git and Zmin exit status, stdout,
stderr and bundle side effects through a focused `git_pack_integrity_compat`
test.

Actual post-import movement matched the declaration: `+5` behavior rows, `+5`
closed rows, `+0` open rows, `+0` invalid-input rows, `+0` represented oracle
functions, `+0` missing-or-unclassified oracle functions, `+0` commands with
rows, `+1` represented doc-option pairs, `+0` implemented-but-unverified
schema rows and `+0` remaining checklist rows.

## 2026-06-25 - check-attr cached source and z surfaces

Expected movement:

- behavior rows: `+8`
- closed rows: `+8`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: `+3`
- implemented-but-unverified schema rows: `+0`
- remaining checklist rows: `+0`
- Rust behavior changes: yes

Expected rows:

- `git check-attr --cached text diff -- main.rs`
- `git check-attr --source=HEAD text diff -- main.rs`
- `git check-attr --source HEAD text diff -- main.rs`
- `git check-attr -z text diff -- main.rs`
- `git check-attr --all -z -- main.rs`
- `git check-attr --stdin -z text diff`
- `git check-attr --source=HEAD --all -z -- main.rs`
- `git check-attr --cached -z text diff -- main.rs`

This batch closes the remaining local `git check-attr` documented parser
surface by adding `--cached`, `--source` and `-z`, including tree-ish-backed
attribute loading, index-backed attribute loading, NUL-delimited output, and
NUL-delimited stdin path parsing. The evidence compares stock Git and Zmin
stdout bytes for the exact query rows through a focused `git_text_tools_compat`
test.

Actual post-import movement matched the declaration: `+8` behavior rows, `+8`
closed rows, `+0` open rows, `+0` invalid-input rows, `+0` represented oracle
functions, `+0` missing-or-unclassified oracle functions, `+0` commands with
rows, `+3` represented doc-option pairs, `+0` implemented-but-unverified
schema rows and `+0` remaining checklist rows.

## 2026-06-25 - merge-base all parser surface

Expected movement:

- behavior rows: `+3`
- closed rows: `+3`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: `+2`
- implemented-but-unverified schema rows: `+0`
- remaining checklist rows: `+0`
- Rust behavior changes: yes

Expected rows:

- `git merge-base --all a b`
- `git merge-base -a a b`
- `git merge-base --all HEAD HEAD`

This batch closes the remaining local `git merge-base` documented parser
surface by adding `--all` / `-a` and wiring it to the existing
`merge_bases_all_cached` implementation. The evidence compares stock Git and
Zmin stdout for the exact all-mode rows through a focused `git_merge_compat`
test.

Actual post-import movement matched the declaration: `+3` behavior rows, `+3`
closed rows, `+0` open rows, `+0` invalid-input rows, `+0` represented oracle
functions, `+0` missing-or-unclassified oracle functions, `+0` commands with
rows, `+2` represented doc-option pairs, `+0` implemented-but-unverified
schema rows and `+0` remaining checklist rows.
As of 2026-06-25 the latest closure is a helper-backed `git fetch-pack`
documented-option family batch for `--exec`, `--shallow-since`,
`--shallow-exclude`, and `--deepen-relative`. Zmin now matches stock Git on
the local helper lane for explicit upload-pack helper override, date-limited
history fetch, named-history exclusion, and relative deepening from an already
shallow destination, with matching stdout, normalized progress stderr and
resulting shallow/object side effects in focused `git_transport_local_compat`
rows. The remaining `fetch-pack` doc tail is now only the lighter
`--check-self-contained-and-connected` / `--refetch` pair, so the next bounded
follow-up can either finish `fetch-pack` outright or return to the heavier
`multi-pack-index` bitmap/incremental family.
As of 2026-06-26 the latest completed batch is a helper-free shared `git log`
and `git rev-list` `simplify-merges` plus `simplify-by-decoration`
documented-option family expansion on the current local lane. The selected
change added six exact stock-Git rows for `log --simplify-merges`,
`log --simplify-by-decoration`, the composed `log --simplify-merges
--simplify-by-decoration` lane, `rev-list --simplify-merges`,
`rev-list --simplify-by-decoration`, and the composed
`rev-list --simplify-merges --simplify-by-decoration` lane, then promoted
four represented documented options into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. Expected delta was
`+6` matrix rows, `+4` complete documented option pairs, `+4` represented
documented option pairs, `+6` verified rows, `+0` invalid-input rows, and
`+0` complete command matrices. Actual delta matched. Current census counts
are `5823` matrix rows, `5067` verified rows, `720` invalid-input rows, `12`
exact-open rows, `146/151` complete command matrices, `1812/3212` complete
documented option pairs, and `1820/3212` represented documented option pairs.
`log` now sits at `64/131` reviewed-complete documented option pairs with
`160` written rows, `159` classified rows, `153` stock-matching rows, `6`
invalid-input rows, and `0` exact-open rows, while `rev-list` now sits at
`26/117` reviewed-complete documented option pairs with `47/47` classified
rows, `47` stock-matching rows, `0` invalid-input rows, and `0` exact-open
rows on their current modeled surfaces. The next default follow-up should
move off this shared history-query topology tail and return to the next
largest census-first represented family.

As of 2026-06-26 the latest completed batch is a helper-free shared `git log`
and `git rev-list` history-simplification acceptance plus ancestry-path
documented-option family expansion on the current local lane. The selected
change added twelve exact stock-Git rows for `log --full-history`,
`log --dense`, `log --sparse`, `log --show-pulls`, two `log --ancestry-path`
range lanes, `rev-list --full-history`, `rev-list --dense`,
`rev-list --sparse`, `rev-list --show-pulls`, and two
`rev-list --ancestry-path` range lanes, then promoted ten represented
documented options into `docs/cli/census/reviewed_complete_doc_option_pairs.tsv`.
Expected delta was `+12` matrix rows, `+10` complete documented option pairs,
`+10` represented documented option pairs, `+12` verified rows, `+0`
invalid-input rows, and `+0` complete command matrices. Actual delta matched.
Current census counts are `5817` matrix rows, `5061` verified rows, `720`
invalid-input rows, `12` exact-open rows, `146/151` complete command
matrices, `1808/3212` complete documented option pairs, and
`1816/3212` represented documented option pairs. `log` now sits at
`62/131` reviewed-complete documented option pairs with `157` written rows,
`156` classified rows, `150` stock-matching rows, `6` invalid-input rows,
and `0` exact-open rows, while `rev-list` now sits at `24/117`
reviewed-complete documented option pairs with `44/44` classified rows,
`44` stock-matching rows, `0` invalid-input rows, and `0` exact-open rows on
their current modeled surfaces. The next default follow-up should move to the
remaining shared history-query topology family centered on
`simplify-merges`/`simplify-by-decoration`.

As of 2026-06-26 the latest completed batch is a helper-free shared `git log`
and `git rev-list` traversal-order documented-option family expansion on the
current local lane. The selected change added six exact stock-Git rows for
`log --topo-order`, `log --date-order`, `log --author-date-order`,
`rev-list --topo-order`, `rev-list --date-order`, and
`rev-list --author-date-order`, then promoted six represented documented
options into `docs/cli/census/reviewed_complete_doc_option_pairs.tsv`.
Expected delta was `+6` matrix rows, `+6` complete documented option pairs,
`+6` represented documented option pairs, `+6` verified rows, `+0`
invalid-input rows, and `+0` complete command matrices. Actual delta matched.
Current census counts are `5805` matrix rows, `5049` verified rows, `720`
invalid-input rows, `12` exact-open rows, `146/151` complete command
matrices, `1798/3212` complete documented option pairs, and
`1806/3212` represented documented option pairs. `log` now sits at
`57/131` reviewed-complete documented option pairs with `151` written rows,
`150` classified rows, `144` stock-matching rows, `6` invalid-input rows,
and `0` exact-open rows, while `rev-list` now sits at `19/117`
reviewed-complete documented option pairs with `38/38` classified rows,
`38` stock-matching rows, `0` invalid-input rows, and `0` exact-open rows on
their current modeled surfaces. The next default follow-up should move to the
remaining shared history-query topology family such as
`full-history`/`simplify-merges`/`ancestry-path`.

## 2026-06-27 - grep long-form reviewed-complete promotion batch

Expected movement:

- behavior rows: `+0`
- closed rows: `+0`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- complete documented option pairs: `+33`
- represented documented option pairs: `+0`
- complete command matrices: `+0`
- implemented-but-unverified schema rows: `+0`
- remaining checklist rows: `-33`
- Rust behavior changes: no

Selected promoted options:

- `grep --after-context`
- `grep --all-match`
- `grep --and`
- `grep --basic-regexp`
- `grep --before-context`
- `grep --color`
- `grep --column`
- `grep --context`
- `grep --exclude-standard`
- `grep --extended-regexp`
- `grep --function-context`
- `grep --max-depth`
- `grep --name-only`
- `grep --no-color`
- `grep --no-exclude-standard`
- `grep --no-index`
- `grep --no-recursive`
- `grep --no-textconv`
- `grep --not`
- `grep --null`
- `grep --only-matching`
- `grep --open-files-in-pager`
- `grep --or`
- `grep --perl-regexp`
- `grep --quiet`
- `grep --recurse-submodules`
- `grep --recursive`
- `grep --show-function`
- `grep --text`
- `grep --textconv`
- `grep --threads`
- `grep --untracked`
- `grep --word-regexp`

This batch is a census/docs promotion rather than a Rust behavior change. It
promotes the current represented helper-free long-form `grep` surface into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv` because exact stock-Git
evidence already exists in `docs/cli/matrices/grep_v2_47.tsv`,
`crates/zmin-cli/tests/git_grep_compat.rs`, and the existing schema-oracle
smoke coverage for the relevant long-form parser lanes. The represented surface
already covers context families, boolean-expression composition, output-shape
toggles, traversal and depth/thread ordering, text and regexp mode selection,
and the bounded binary/pager tails on the current modeled local lanes.

Actual post-import movement matched the declaration: `+0` behavior rows, `+0`
closed rows, `+0` open rows, `+0` invalid-input rows, `+0` represented oracle
functions, `+0` missing-or-unclassified oracle functions, `+0` commands with
rows, `+33` complete documented option pairs, `+0` represented documented
option pairs, `+0` complete command matrices, `+0`
implemented-but-unverified schema rows, and `-33` remaining checklist rows.

## 2026-06-27 - grep short-alias reviewed-complete promotion batch

Expected movement:

- behavior rows: `+0`
- closed rows: `+0`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- complete documented option pairs: `+19`
- represented documented option pairs: `+0`
- complete command matrices: `+0`
- implemented-but-unverified schema rows: `+0`
- remaining checklist rows: `-19`
- Rust behavior changes: no

Selected promoted options:

- `grep -A`
- `grep -B`
- `grep -C`
- `grep -E`
- `grep -G`
- `grep -I`
- `grep -O`
- `grep -P`
- `grep -W`
- `grep -a`
- `grep -e`
- `grep -f`
- `grep -h`
- `grep -o`
- `grep -p`
- `grep -q`
- `grep -r`
- `grep -w`
- `grep -z`

This batch is the short-alias follow-through to the preceding long-form `grep`
promotion. It promotes the remaining represented helper-free short spellings
into `docs/cli/census/reviewed_complete_doc_option_pairs.tsv` because exact
stock-Git evidence already exists in `docs/cli/matrices/grep_v2_47.tsv`,
`crates/zmin-cli/tests/git_grep_compat.rs`, and the existing schema-oracle
smoke coverage for the parser-only long-form lanes. The promoted short surface
covers context aliases, regexp-mode aliases, boolean-expression pattern
spellings, local traversal toggles, output-shape aliases, function-context
aliases, NUL-delimited filename output, and the bounded binary/pager aliases on
the current modeled helper-free local lanes.

Actual post-import movement matched the declaration: `+0` behavior rows, `+0`
closed rows, `+0` open rows, `+0` invalid-input rows, `+0` represented oracle
functions, `+0` missing-or-unclassified oracle functions, `+0` commands with
rows, `+19` complete documented option pairs, `+0` represented documented
option pairs, `+0` complete command matrices, `+0`
implemented-but-unverified schema rows, and `-19` remaining checklist rows.

## 2026-06-28 - fast-import stream-feature marks expansion batch

Expected movement:

- behavior rows: `+8`
- closed rows: `+5`
- open rows: `+0`
- invalid-input rows: `+3`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- complete documented option pairs: `+0`
- represented documented option pairs: `+0`
- complete command matrices: `+0`
- implemented-but-unverified schema rows: `+0`
- remaining checklist rows: `+0`
- Rust behavior changes: yes

Selected variants:

- `fast-import feature export-marks=marks.txt` with `--allow-unsafe-features`
- `fast-import feature import-marks=one.marks` with `--allow-unsafe-features`
- `fast-import feature import-marks-if-exists=one.marks` with
  `--allow-unsafe-features`
- `fast-import feature no-relative-marks` plus
  `feature export-marks=marks.txt`
- `fast-import --import-marks=one.marks` plus stream
  `feature import-marks=two.marks`
- `fast-import feature export-marks=marks.txt` without
  `--allow-unsafe-features`
- `fast-import feature import-marks=one.marks` without
  `--allow-unsafe-features`
- `fast-import feature import-marks-if-exists=one.marks` without
  `--allow-unsafe-features`

This batch extends the current helper-free local `fast-import` oracle through
the bounded in-stream marks feature tail instead of widening into new runtime
surfaces. Rust changes taught the parser to recognize `feature ...` commands
for the currently modeled stream families, enforce stock-style
`--allow-unsafe-features` gating, honor CLI `--import-marks*` precedence over
stream features, and emit the stock-shaped rejection surface for unsupported
unsafe forms. Test infrastructure also now honors `ZMIN_BIN` in the shared
integration harness so focused compatibility tests can force the current
`target/debug/zmin` binary instead of accidentally sampling a stale build.

Focused verification was
`cargo build -p zmin-cli --bin zmin`,
`ZMIN_BIN=/Users/dschewchenko/work/private/skron-git/target/debug/zmin CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo test -p zmin-cli --test git_fast_import_date_compat fast_import_stream_feature_marks_fail_like_stock_git -- --exact --nocapture`,
`ZMIN_BIN=/Users/dschewchenko/work/private/skron-git/target/debug/zmin CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo test -p zmin-cli --test git_fast_import_date_compat -- --nocapture`,
`ZMIN_BIN=/Users/dschewchenko/work/private/skron-git/target/debug/zmin CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo test -p zmin-cli --test git_fast_import_export_compat -- --nocapture`,
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo check -p zmin-cli -p zmin-cli-schema`,
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(fast-import|summary)\t|^complete_command_matrices\t|^complete_doc_option_pairs\t|^doc_option_pairs_represented_by_rows\t|^behavior_rows_written\t|^written_rows_matching_stock_git\t|^behavior_rows_classified\t|^invalid_input_rows\t'`,
and `git diff --check`.

Actual post-import movement matched the declaration: `+8` behavior rows, `+5`
closed rows, `+0` open rows, `+3` invalid-input rows, `+0` represented oracle
functions, `+0` missing-or-unclassified oracle functions, `+0` commands with
rows, `+0` complete documented option pairs, `+0` represented documented
option pairs, `+0` complete command matrices, `+0`
implemented-but-unverified schema rows, and `+0` remaining checklist rows.

## 2026-06-28 - fast-import relative-marks path-resolution and repeated stream marks batch

Expected movement:

- behavior rows: `+14`
- closed rows: `+10`
- open rows: `+0`
- invalid-input rows: `+4`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- complete documented option pairs: `+0`
- represented documented option pairs: `+0`
- complete command matrices: `+0`
- implemented-but-unverified schema rows: `+0`
- remaining checklist rows: `+0`
- Rust behavior changes: yes

Selected variants:

- `fast-import --relative-marks --export-marks=marks.txt`
- `fast-import --import-marks=plain.marks --relative-marks`
- `fast-import --relative-marks --import-marks=rel/child.marks`
- `fast-import --relative-marks --import-marks-if-exists=missing.marks`
- `fast-import --relative-marks --export-marks=out.marks --no-relative-marks --export-marks=tail.marks`
- `feature relative-marks` plus `feature export-marks=marks.txt`
- `feature relative-marks` plus `feature import-marks=rel/child.marks`
- `feature import-marks=plain.marks` plus `feature relative-marks`
- `feature relative-marks` plus `feature import-marks-if-exists=missing.marks`
- `feature relative-marks`, `feature export-marks=one.marks`,
  `feature no-relative-marks`, `feature export-marks=two.marks`
- repeated `feature import-marks=one.marks` then `feature import-marks=two.marks`
- `feature import-marks=one.marks` then `feature import-marks-if-exists=two.marks`
- repeated `feature import-marks-if-exists=one.marks` then
  `feature import-marks-if-exists=two.marks`
- CLI `--import-marks=one.marks` plus repeated stream `feature import-marks=*`

This batch closes the previously deferred helper-free relative-marks tail on
the current `fast-import` oracle. Rust changes switch the schema surface from a
required-value `--relative-marks` parser to stock-compatible flag behavior
while still preserving the stock invalid attached-value crash lane for
`--relative-marks=rel`. Command-line marks path resolution now walks raw args
in order so `--relative-marks` / `--no-relative-marks` toggle only subsequent
marks-file options, while stream `feature relative-marks` and
`feature no-relative-marks` update parser state for subsequent marks features.
The export side now creates parent directories like stock Git and repeated
stream `feature export-marks=` follows stock last-one-wins behavior. The
existing repeated stream import-marks guard was tightened to match stock's
capitalized fatal text exactly.

Focused verification was
`cargo build -p zmin-cli --bin zmin`,
`ZMIN_BIN=/Users/dschewchenko/work/private/skron-git/target/debug/zmin CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo test -p zmin-cli --test git_fast_import_date_compat fast_import_relative_marks_path_resolution_matches_stock_git -- --exact --nocapture`,
`ZMIN_BIN=/Users/dschewchenko/work/private/skron-git/target/debug/zmin CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo test -p zmin-cli --test git_fast_import_date_compat fast_import_relative_marks_stream_features_match_stock_git -- --exact --nocapture`,
`ZMIN_BIN=/Users/dschewchenko/work/private/skron-git/target/debug/zmin CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo test -p zmin-cli --test git_fast_import_date_compat fast_import_stream_feature_marks_fail_like_stock_git -- --exact --nocapture`,
`ZMIN_BIN=/Users/dschewchenko/work/private/skron-git/target/debug/zmin CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo test -p zmin-cli --test git_fast_import_date_compat -- --nocapture`,
`ZMIN_BIN=/Users/dschewchenko/work/private/skron-git/target/debug/zmin CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo test -p zmin-cli --test git_fast_import_export_compat -- --nocapture`,
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo check -p zmin-cli -p zmin-cli-schema`,
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(fast-import|summary)\t|^complete_command_matrices\t|^complete_doc_option_pairs\t|^doc_option_pairs_represented_by_rows\t|^behavior_rows_written\t|^written_rows_matching_stock_git\t|^behavior_rows_classified\t|^invalid_input_rows\t'`,
and `git diff --check`.

Actual post-import movement matched the declaration: `+14` behavior rows, `+10`
closed rows, `+0` open rows, `+4` invalid-input rows, `+0` represented oracle
functions, `+0` missing-or-unclassified oracle functions, `+0` commands with
rows, `+0` complete documented option pairs, `+0` represented documented
option pairs, `+0` complete command matrices, `+0`
implemented-but-unverified schema rows, and `+0` remaining checklist rows.
