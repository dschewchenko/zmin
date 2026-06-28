# Variant Compatibility Plan

Command-name coverage is not full Git compatibility. Option spelling coverage
is not full Git compatibility. A supported item must be counted as a behavior
variant:

`command + option + value + option combination + repository state + transport/workflow + platform`.

Examples:

- `blame --date=iso` and `blame --date=relative` are two variants.
- `fetch --depth=1 <remote> <refspec>` and `fetch --depth=1 <remote> <refspec> <refspec>` are separate variants.
- `status -z` and `status --porcelain=v2 -z --branch` are separate variants.
- `log --date=iso`, `log --date=unix` and `log --date=format:%Y-%m-%d` are separate variants.
- Parser acceptance does not count. The behavior must match stock Git output,
  exit code and repository state.

Start each resume from `docs/cli/git_compatibility_execution_plan.md`. This
file remains the detailed live handoff for counting rules, slice queues, guard
mappings and latest completed slices.

## Current Slice Pointer

As of 2026-06-29 the latest completed batch is the bounded helper-free local
`rebase --root` completion family across the existing linear and `--onto`
replay fixtures. This batch added three exact stock-Git rows and promoted the
final documented `rebase` option pair into the reviewed-complete census set:
`--root`. The runtime closure stayed intentionally bounded: Zmin now matches
stock on the current branch root-replay lane, the explicit branch-argument root
lane, and the explicit `--onto origin/main` root lane, including the stock
progress stderr, final tree, recent log subjects, current branch, and clean
status. The `--onto` lane now also matches the stock commit-selection rule that
skips root-side commits already reachable from the new base, so the bounded
fixture replays only the two non-ancestor commits instead of replaying the
shared base commit. This does not change the separate `replay` surface
directly, but the matching `replaying down from root commit is not supported
yet!` guards in `history_impl.rs` are now documented in the hard-fail
inventory instead of remaining unclassified. Focused gates were
`cargo test -q -p zmin-cli --test git_sequencer_compat rebase_root_option_family_matches_stock_git -- --exact`,
`cargo check -q -p zmin-cli -p zmin-cli-schema`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`, and `git diff --check`. Durable counts are
now `7668` matrix rows, `6748` verified rows, `895` invalid-input rows,
`2` remaining-to-fix-or-verify rows, and `131` extension/deferred rows. The
active backlog is now down to the single-pair tails `apply` and `log`. The
next default follow-up should close one of those two remaining documented
single-pair tails.

As of 2026-06-29 the latest completed batch is a stock-oracle deferral closure
for the remaining documented Perl-backed `git send-email` helper tail:
`--annotate`, `--compose`, `--quiet`, `--sendmail-cmd`, and `--smtp-ssl`.
This batch did not add new stock-matching behavior rows. Instead, it promoted
the five open doc-option seed tails into the durable deferral inventory because
the current local stock Git was built with `NO_PERL=1`: `git send-email -h`
fails before helper dispatch, so no subcommand-level Git `2.47.1` oracle
exists for these remaining `send-email` options in this environment. The
census now keeps those option pairs in
`docs/cli/census/deferred_doc_option_pairs.tsv` and surfaces them through the
extension/deferred layer rather than the active fix-or-verify queue. This
keeps the active backlog honest without falsely increasing reviewed-complete
coverage counts. Focused gates were `git send-email -h`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`, and `git diff --check`. Durable counts are
now `7665` matrix rows, `6745` verified rows, `895` invalid-input rows,
`5` remaining-to-fix-or-verify rows, and `131` extension/deferred rows. The
active backlog is now down to the single-pair tails `apply`, `log`, `rebase`,
plus the two unclassified source guards. The next default follow-up should move
to `rebase --root`, because the remaining documented option pair and both
unclassified guards all converge on the same root-replay code path.

As of 2026-06-29 the latest completed batch is a stock-oracle deferral closure
for the remaining documented Perl-backed legacy helper surface:
`git cvsimport`, `git cvsexportcommit`, and `git archimport`. This batch did
not add new stock-matching behavior rows. Instead, it promoted thirty-two open
doc-option seed tails into the durable deferral inventory because the current
local stock Git was built with `NO_PERL=1`: `git cvsimport -h`,
`git cvsexportcommit -h`, and `git archimport -h` all fail before helper
dispatch, so no subcommand-level Git `2.47.1` oracle exists for these legacy
bridges in this environment. The census now keeps those option pairs in
`docs/cli/census/deferred_doc_option_pairs.tsv` and surfaces them through the
extension/deferred layer rather than the active fix-or-verify queue. This keeps
the active backlog honest without falsely increasing reviewed-complete coverage
counts. Focused gates were `git cvsimport -h`, `git cvsexportcommit -h`,
`git archimport -h`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`, and `git diff --check`. Durable counts are
now `7665` matrix rows, `6745` verified rows, `895` invalid-input rows,
`10` remaining-to-fix-or-verify rows, and `126` extension/deferred rows. The
active backlog head is now `send-email` (`5`), followed by the single-pair
tails `apply`, `log`, `rebase`, and the two remaining unclassified source
guards. The next default follow-up should move to `send-email`, because it is
now the largest remaining active doc-option family with a live modeled surface.

As of 2026-06-29 the latest completed batch is a stock-oracle deferral closure
for the remaining documented `git svn` option surface. This batch did not add
new stock-matching behavior rows. Instead, it promoted all twenty-five
previously open `svn` doc-option seed tails into the durable deferral inventory
because the current local stock helper is unusable: `git svn --version` fails
before helper startup when Perl tries to load an x86_64 `SVN::_Core` bundle on
this arm64 host, so no command-level Git `2.47.1` oracle exists for the
legacy SVN bridge options in this environment. The census now keeps those
option pairs in `docs/cli/census/deferred_doc_option_pairs.tsv` and surfaces
them through the extension/deferred layer rather than the active fix-or-verify
queue. This keeps the active backlog honest without falsely increasing
reviewed-complete coverage counts. Focused gates were
`git svn --version`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`, and `git diff --check`. Durable counts are
now `7665` matrix rows, `6745` verified rows, `895` invalid-input rows,
`42` remaining-to-fix-or-verify rows, and `94` extension/deferred rows.
`svn` now has `0` active remaining rows, while the next active backlog head is
`cvsimport` (`14`), followed by `cvsexportcommit` (`11`), `archimport` (`7`),
`send-email` (`5`), and the single-pair tails `rebase`, `log`, and `apply`.
The next default follow-up should stay on the largest remaining legacy-bridge
batch, which is now `cvsimport`.

As of 2026-06-29 the latest completed batch is the final bounded local `p4 clone`
completion family across the existing fake Perforce depot import fixture. This
batch added four exact stock-Git rows and promoted the final four documented
`p4` option pairs into the reviewed-complete census set: `--bare`,
`--changesfile`, `--destination`, and `--keep-path`. The runtime closure stayed
intentionally bounded: Zmin now matches stock on the current fake clone lanes
where `--keep-path` fails early with the exact missing-destination stderr and no
repository creation; `--destination destdir` preserves the stock usage text and
the exact `Depot paths must start with "//": <target>` stderr when an extra
positional target collides with the destination mode; `--changesfile` produces
the stock no-checkout transcript with the carriage-return progress line, leaves
refs absent in an initialized non-bare repository, and keeps clean status; and
`--bare` now matches the stock bare repository shape, `HEAD` target, ref
topology, commit message, revision-suffixed tree payload, and worktree-less
status failure. This does not widen `p4` beyond the existing fake Perforce
oracle or into broader helper-backed workflows; it closes the remaining
documented clone-side family only. The implementation stayed focused on the
missing closure: clone parsing now preserves the destination conflict positional,
the clone runtime branches into stock-shaped bounded bare and changesfile lanes,
and the final failure tails emit the stock usage and stderr without creating
repositories. Two exact compat tests cover all four rows while checking exact
stdout, stderr, exit status, and repository shape/state where relevant. Focused
gates were `cargo test -q -p zmin-cli --test git_foreign_scm_compat p4_`,
`cargo check -q -p zmin-cli -p zmin-cli-schema`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`, and `git diff --check`. Durable counts
should now move to `7665` matrix rows, `6745` verified rows, `895` invalid-input
rows, `146/151` complete command matrices, and `3147/3212` complete documented
option pairs once the reviewed-complete census artifacts are regenerated. `p4`
should close at `29/29` reviewed-complete documented option pairs with `32/32`
classified rows, `32` stock-matching rows, and `0` invalid-input rows. The
overall backlog head should then become `svn` (`25`), followed by `cvsimport`
(`14`), `cvsexportcommit` (`11`), `archimport` (`7`), `send-email` (`5`), and
the single-pair tails `rebase`, `log`, and `apply`; the next default follow-up
should move off `p4` and onto the largest remaining foreign-SCM/doc-option
group, which is now `svn`.

As of 2026-06-29 the latest completed batch is a bounded local `p4 submit`
prepare/shelve/export-labels family across the existing fake Perforce submit
fixture. This batch added four exact stock-Git rows and promoted four
documented `p4` option pairs into the reviewed-complete census set:
`--export-labels`, `--prepare-p4-only`, `--shelve`, and `--update-shelve`.
The runtime closure stayed intentionally bounded: Zmin now matches stock on
the current fake submit lanes where `--prepare-p4-only` applies the local
commit into the client workspace, leaves the remote submit ref unchanged, and
prints the stock submit-template guidance; `--shelve` and
`--update-shelve 1234` stop after the apply phase with the stock `Reverting
shelved files.` stdout tail and the stock shelve command failure stderr; and
`--export-labels` preserves the normal submit/import/rebase transcript before
failing with the stock Python traceback that terminates in `KeyError:
'label'`. This does not widen `p4` into `--bare`, `--changesfile`,
`--destination`, or `--keep-path`. The implementation stays focused on the
missing closure only: submit parsing now carries the four bounded spellings,
the submit flow can branch into stock-shaped prepare-only and shelve/error
transcripts without advancing the remote submit ref, and export-labels now
surfaces the stock traceback after the existing submit success tail. Three
exact compat tests cover all four rows while checking exact stdout, stderr,
exit status, and state where relevant. Focused gates were
`cargo test -q -p zmin-cli --test git_foreign_scm_compat p4_`,
`cargo check -q -p zmin-cli -p zmin-cli-schema`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`, and `git diff --check`. Durable counts
are now `7661` matrix rows, `6741` verified rows, `895` invalid-input rows,
`146/151` complete command matrices, and `3143/3212` complete documented
option pairs. `p4` now sits at `25/29` reviewed-complete documented option
pairs with `28/28` classified rows, `28` stock-matching rows, and `0`
invalid-input rows. The overall backlog head remains `svn` (`25`), followed
by `cvsimport` (`14`), `cvsexportcommit` (`11`), `archimport` (`7`),
`send-email` (`5`), `p4` (`4`), and the single-pair tails `rebase`, `log`,
and `apply`; the next default follow-up should still prefer `p4`, because the
remaining four clone-side rows share the same working fake-Perforce oracle and
can close the command with one final bounded family.

As of 2026-06-28 the latest completed batch is a helper-free local `p4 clone`
transcript-control family across the existing fake Perforce depot import lane.
This batch added three exact stock-Git rows and promoted three documented
`p4` option pairs into the reviewed-complete census set: `--silent`,
`--verbose`, and `-v`. The runtime closure stayed intentionally bounded:
Zmin now matches stock on the current fake Perforce clone fixture where
`--verbose` and `-v` emit the stock-like import transcript and verbose stderr
trace, while `--silent` keeps only the reduced stock stdout prelude without
the verbose trace lines. This does not widen `p4` into submit-side verbose
trace parity, branch detection, label import/export, client-spec flows,
import-local ref rewrites, or other helper-backed lanes. The implementation
stays focused on the missing closure only: clone parsing now carries the
bounded `--silent` toggle, and the existing helper-free import path emits the
stock-shaped transcript only for the newly covered verbose and silent lanes.
One exact compat test covers all three rows while checking exact stdout,
stderr, exit status, branch, imported commit message, imported worktree
contents, and the bounded helper-free local flow. Focused gates were
`cargo test -q -p zmin-cli --test git_foreign_scm_compat p4_`,
`cargo check -q -p zmin-cli -p zmin-cli-schema`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`, and `git diff --check`. Durable counts
are now `7644` matrix rows, `6724` verified rows, `895` invalid-input rows,
`146/151` complete command matrices, and `3126/3212` complete documented
option pairs. `p4` now sits at `8/29` reviewed-complete documented option
pairs with `11/11` classified rows, `11` stock-matching rows, and `0`
invalid-input rows. The overall backlog head remains `svn` (`25`), followed
by `p4` (`21`), `cvsimport` (`14`), `cvsexportcommit` (`11`), `archimport`
(`7`), `send-email` (`5`), and the single-pair tails `rebase`, `log`, and
`apply`; the next default follow-up should stay on the larger foreign-SCM
queues, with `p4` still the most attractive helper-free batch after `svn`.

As of 2026-06-28 the latest completed batch is a helper-free local `rebase`
remaining non-root control family across the existing upstream,
branch-argument, and `--onto` replay lanes. This batch added fifteen exact
stock-Git rows and promoted five documented `rebase` option pairs into the
reviewed-complete census set: `--keep-base`, `--reset-author-date`,
`--exec`, `-x`, and `-C`. The runtime closure stayed intentionally bounded:
Zmin now matches stock on the current helper-free lanes where `--keep-base`
leaves the topic branch unchanged when it still sits on the merge-base,
`--keep-base` combined with `--onto` returns the stock fatal incompatibility,
`--reset-author-date` follows the stock author-date rewrite behavior already
shared with the bounded ignore-date replay path, `--exec true` and `-x true`
append the stock helper-free exec step to each replayed commit, and `-C 1`
stays on the apply-backend lane with stock-equivalent stdout/stderr and final
state. This does not widen `rebase` into root replay yet; `--root` remains
the only uncovered documented `rebase` option pair. The implementation stays
focused on the missing closure only: the rebase schema now exposes the five
spellings, helper-free replay now supports bounded keep-base no-op detection,
bounded exec-step execution, and apply-backend context-line parsing, and one
exact compat test covers all fifteen rows while checking exact stdout, stderr,
exit status, tree, recent log subjects, raw dates where relevant, current
branch, and clean status. Focused gates were
`cargo test -q -p zmin-cli --test git_sequencer_compat rebase_remaining_helper_free_option_family_matches_stock_git -- --exact`,
`cargo test -q -p zmin-cli --test git_sequencer_compat rebase_extended_clean_metadata_option_family_matches_stock_git -- --exact`,
`cargo check -q -p zmin-cli -p zmin-cli-schema`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`, and `git diff --check`. Durable counts
are now `7641` matrix rows, `6721` verified rows, `895` invalid-input rows,
`146/151` complete command matrices, and `3123/3212` complete documented
option pairs. `rebase` now sits at `56/57` reviewed-complete documented
option pairs with `169/169` classified rows, `164` stock-matching rows, and
`5` invalid-input rows. The overall backlog head is now `svn` (`25`),
followed by `p4` (`24`), `cvsimport` (`14`), `cvsexportcommit` (`11`),
`archimport` (`7`), `send-email` (`5`), and the single-pair tails
`rebase`, `log`, and `apply`; the next default follow-up should either finish
the final `rebase --root` tail or switch back to one of the larger
foreign-SCM queues.

As of 2026-06-28 the latest completed batch is a helper-free local `rebase`
extended clean metadata family across the existing upstream, branch-argument,
and `--onto` replay lanes. This batch added twenty-seven exact stock-Git rows
and promoted nine documented `rebase` option pairs into the
reviewed-complete census set: `--autosquash`, `--empty`, `--gpg-sign`,
`--ignore-date`, `--no-autosquash`, `--no-gpg-sign`,
`--no-reschedule-failed-exec`, `--reschedule-failed-exec`, and `-S`. The
runtime closure stayed intentionally bounded: Zmin now matches stock on the
current clean replay lanes where autosquash toggles stay inert because there
are no fixup/squash commits, `--empty=drop` stays on lanes where no replayed
commit becomes empty, `--gpg-sign` and `-S` preserve configured signing,
`--no-gpg-sign` disables it, `--ignore-date` rewrites the replayed author date
to the stock current-time behavior while preserving the stock committer flow,
and the reschedule-failed-exec toggles stay inert because there are no exec
steps to reschedule. This does not widen `rebase` into exec steps, keep-base,
reset-author-date, root replay, explicit context lines, or the short `-x`
exec alias. The implementation stays focused on the missing closure only: the
rebase schema now exposes the nine spellings, helper-free replay now carries
through bounded GPG-sign and ignore-date semantics, internal rebase callsites
were updated for the expanded replay signature, and one exact compat test
covers all twenty-seven rows while checking exact stdout, stderr, exit status,
tree, recent log subjects, raw dates, signature status, branch, and clean
status. Focused gates were
`cargo test -q -p zmin-cli --test git_sequencer_compat rebase_extended_clean_metadata_option_family_matches_stock_git -- --exact`,
`cargo check -q -p zmin-cli -p zmin-cli-schema`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`, and `git diff --check`. Durable counts
are now `7626` matrix rows, `6707` verified rows, `894` invalid-input rows,
`146/151` complete command matrices, and `3118/3212` complete documented
option pairs. `rebase` now sits at `51/57` reviewed-complete documented
option pairs with `154/154` classified rows, `150` stock-matching rows, and
`4` invalid-input rows. The overall backlog head is now `svn` (`25`),
followed by `p4` (`24`), `cvsimport` (`14`), `cvsexportcommit` (`11`),
`archimport` (`7`), `rebase` (`6`), `send-email` (`5`), then the
single-pair tails `log` and `apply`; the next default follow-up should either
finish the small remaining `rebase` tail (`--exec`, `--keep-base`,
`--reset-author-date`, `--root`, `-C`, `-x`) or switch back to one of the
larger foreign-SCM queues.

As of 2026-06-28 the latest completed batch is a helper-free local
`send-email` invalid-SMTP and dry-run closure family across the existing
explicit patch-file lanes. This batch added twenty-one exact stock-Git rows
and promoted twenty-one documented `send-email` option pairs into the
reviewed-complete census set: `--8bit-encoding`, `--batch-size`, `--cc-cmd`,
`--compose-encoding`, `--confirm`, `--dry-run`, `--envelope-sender`,
`--header-cmd`, `--in-reply-to`, `--no-cc-cover`, `--no-header-cmd`,
`--no-to`, `--no-xmailer`, `--relogin-delay`, `--signed-off-by-cc`,
`--smtp-encryption`, `--smtp-server-option`, `--smtp-ssl-cert-path`,
`--smtp-user`, `--to-cmd`, and `--transfer-encoding`. The runtime closure
stayed intentionally bounded: Zmin now matches stock on the current invalid
SMTP initialization lane where those spellings are accepted but never reach
their deeper semantics before the shared SMTP startup failure, and it also now
matches the stock explicit patch-file `--dry-run` preview transcript without
contacting SMTP. This does not widen `send-email` into compose or annotate
editor flows, quiet-mode suppression, sendmail transport, or SSL-enabled SMTP
handshakes. The implementation stays focused on the missing closure only: the
send-email schema now exposes the twenty-one spellings, the SMTP path now
emits the stock-shaped initialization failure on the bounded invalid-server
lane, `--dry-run` now renders the stock preview transcript on the explicit
patch-file lane, and focused exact compat tests cover both bundles. Focused
gates were
`cargo test -q -p zmin-cli --test git_mail_tools_compat send_email_invalid_smtp_noop_option_family_matches_stock_git -- --exact`,
`cargo test -q -p zmin-cli --test git_mail_tools_compat send_email_dry_run_matches_stock_git -- --exact`,
`cargo check -q -p zmin-cli -p zmin-cli-schema`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`, and `git diff --check`. Durable counts
are now `7599` matrix rows, `6680` verified rows, `894` invalid-input rows,
`146/151` complete command matrices, and `3109/3212` complete documented
option pairs. `send-email` now sits at `59/64` reviewed-complete documented
option pairs with `75/75` classified rows, `74` stock-matching rows, and `1`
invalid-input row. The overall backlog head is now `svn` (`25`), followed by
`p4` (`24`), `rebase` (`15`), `cvsimport` (`14`), `cvsexportcommit` (`11`),
`archimport` (`7`), `send-email` (`5`), then the single-pair tails `log` and
`apply`; the next default follow-up should choose between finishing the small
remaining `send-email` tail (`--annotate`, `--compose`, `--quiet`,
`--sendmail-cmd`, `--smtp-ssl`) or switching back to the larger `svn` / `p4`
queues.

As of 2026-06-28 the latest completed batch is a helper-free local
`p4` bounded-clone and dry-run preview family across the existing fake
Perforce clone and submit lanes. This batch added four exact stock-Git rows
and promoted four documented `p4` option pairs into the reviewed-complete
census set: `--changes-block-size`, `--max-changes`, `--dry-run`, and `-n`.
The runtime closure stayed intentionally bounded: Zmin now matches the current
stock fake-depot clone lane where `--changes-block-size=1` and
`--max-changes=1` do not alter the imported branch/message outcome, and it now
matches the stock submit preview lane where `git p4 submit --dry-run` and
`git p4 submit -n` report the stock preview text without mutating p4 or Git
state. This does not widen `p4` into verbose tracing, import-local namespace
changes, branch/label discovery, shelving flows, or client-spec-backed depot
mapping. The implementation stays focused on the missing closure only: the
existing clone parser continues to accept the bounded clone knobs on the
current fake depot lane, `p4 submit` now renders the stock dry-run preview
contract instead of mutating the fake depot, and two focused exact compat
tests exercise the four spellings on the represented lanes. Focused gates were
`cargo test -q -p zmin-cli --test git_foreign_scm_compat p4_clone_noop_option_family_matches_stock_git -- --exact`,
`cargo test -q -p zmin-cli --test git_foreign_scm_compat p4_submit_dry_run_option_family_matches_stock_git -- --exact`,
`cargo check -q -p zmin-cli -p zmin-cli-schema`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`, and `git diff --check`. Durable counts
are now `7578` matrix rows, `6659` verified rows, `894` invalid-input rows,
`146/151` complete command matrices, and `3088/3212` complete documented
option pairs. `p4` now sits at `5/29` reviewed-complete documented option
pairs with `8/8` classified rows and `8` stock-matching rows. The overall
backlog head is now `send-email` (`26`), followed by `svn` (`25`), `p4`
(`24`), `rebase` (`15`), `cvsimport` (`14`), `cvsexportcommit` (`11`),
`archimport` (`7`), then the single-pair tails `log` and `apply`; the next
default follow-up should re-evaluate whether another dense helper-free
`send-email` family exists before spending more cycles on the now more
helper-heavy `p4` tail.

As of 2026-06-28 the latest completed batch is a helper-free local
`send-email` metadata/control no-op family across the existing fake-SMTP
patch-sending lane. This batch added sixteen exact stock-Git rows and promoted
sixteen documented `send-email` option pairs into the reviewed-complete census
set: `--no-bcc`, `--no-cc`, `--no-identity`, `--no-mailmap`,
`--no-signed-off-by-cc`, `--no-suppress-from`, `--no-thread`,
`--no-to-cover`, `--cc-cover`, `--to-cover`, `--thread`,
`--chain-reply-to`, `--no-chain-reply-to`, `--mailmap`, `--identity`, and
`--suppress-from`. The runtime closure stayed intentionally bounded: Zmin now
matches the current stock single-patch local SMTP lane where those spellings
are accepted without altering the result because no cover letter is present,
threading headers are not introduced on the modeled one-patch flow, the
configured identity resolves to the same endpoint and recipients, mailmap
rewrites are absent, and there are no preconfigured Bcc/Cc/from-derived
additions for the clearing toggles to remove. This does not widen send-email
into quiet mode, annotate/compose flows, address-generation commands, confirm
policy, In-Reply-To control, transfer encoding, SMTP auth prompts, or TLS
option families. The implementation stays focused on the missing closure only:
the send-email schema now exposes the sixteen spellings, the patch-sending
path accepts them on the bounded fake-SMTP lane without changing the existing
stock-shaped output path, and one exact compat test exercises the full bundle
while normalizing only dynamic Date, Message-ID, and X-Mailer lines. Focused
gates were
`cargo test -q -p zmin-cli --test git_mail_tools_compat send_email_metadata_noop_option_family_matches_stock_git -- --exact`,
`cargo check -p zmin-cli -p zmin-cli-schema`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`, and `git diff --check`. Durable counts
are now `7574` matrix rows, `6655` verified rows, `894` invalid-input rows,
`146/151` complete command matrices, and `3084/3212` complete documented
option pairs. `send-email` now sits at `38/64` reviewed-complete documented
option pairs with `54/54` classified rows, `53` stock-matching rows, and `1`
invalid-input row. The overall backlog head remains `send-email` (`26`),
followed by `p4` (`28`), `svn` (`25`), `rebase` (`15`), `cvsimport` (`14`),
`cvsexportcommit` (`11`), `archimport` (`7`), then the single-pair tails
`log` and `apply`; the next default follow-up should re-evaluate whether the
remaining `send-email` tail still contains another dense helper-free family or
whether `p4`/`svn` now offer the better batch.

As of 2026-06-28 the latest completed batch is a helper-free local
`send-email` SMTP/header no-op family across the existing fake-SMTP
patch-sending lane. This batch added eleven exact stock-Git rows and promoted
eleven documented `send-email` option pairs into the reviewed-complete census
set: `--no-smtp-auth`, `--smtp-auth`, `--smtp-pass`, `--smtp-debug`,
`--smtp-domain`, `--validate`, `--no-validate`, `--force`,
`--format-patch`, `--no-format-patch`, and `--xmailer`. The runtime closure
stayed intentionally bounded: Zmin now matches the current stock single-patch
local SMTP lane where those spellings are accepted without altering the
no-auth fake-SMTP result, with `--smtp-auth=none` and `--no-smtp-auth`
preserving the no-auth flow, `--smtp-pass=secret`, `--smtp-debug=0`, and
`--smtp-domain=example.test` leaving the transcript unchanged, `--validate`
and `--no-validate` staying no-ops on the clean patch lane, `--force`
remaining inert on the already-sendable patch, `--format-patch` and
`--no-format-patch` staying inert on explicit patch-file input, and
`--xmailer` preserving the stock default X-Mailer header. This does not widen
send-email into SMTP auth prompts, SMTP users, TLS/encryption, compose mode,
confirm policy, alias/header command execution, threading controls, or quiet
mode. The implementation stays focused on the missing closure only: the
send-email schema now exposes the eleven spellings, the patch-sending path
accepts them on the bounded fake-SMTP lane without changing the existing
stock-shaped output path, and one exact compat test exercises the full no-op
bundle while normalizing only dynamic Date, Message-ID, and X-Mailer lines.
Focused gates were
`cargo test -q -p zmin-cli --test git_mail_tools_compat send_email_smtp_noop_option_family_matches_stock_git -- --exact`,
`cargo check -p zmin-cli -p zmin-cli-schema`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`, and `git diff --check`. Durable counts
are now `7558` matrix rows, `6639` verified rows, `894` invalid-input rows,
`146/151` complete command matrices, and `3068/3212` complete documented
option pairs. `send-email` now sits at `22/64` reviewed-complete documented
option pairs with `38/38` classified rows, `37` stock-matching rows, and `1`
invalid-input row. The overall backlog head remains `send-email` (`42`),
followed by `p4` (`28`), `svn` (`25`), `rebase` (`15`), `cvsimport` (`14`),
`cvsexportcommit` (`11`), `archimport` (`7`), then the single-pair tails
`log` and `apply`; the next default follow-up should keep the large-batch bias
and either continue another bounded helper-free `send-email` family or switch
to the foreign-SCM schema clusters once one of them proves denser.

As of 2026-06-28 the latest completed batch is a helper-free local
`send-email` explicit SMTP/recipient override family across the existing
fake-SMTP patch-sending lane. This batch added nine exact stock-Git rows and
promoted nine documented `send-email` option pairs into the reviewed-complete
census set: `--bcc`, `--cc`, `--from`, `--reply-to`, `--smtp-server`,
`--smtp-server-port`, `--subject`, `--suppress-cc`, and `--to`. The runtime
closure stayed intentionally bounded: Zmin now matches the current stock
single-patch local SMTP lane where command-line sender, primary recipients,
Cc/Bcc envelope expansion, Reply-To, and SMTP endpoint overrides replace the
config-backed defaults while `--suppress-cc=author` disables patch-author
auto-cc expansion and `--subject` is accepted without changing the stock patch
subject. This does not widen send-email into compose mode, confirm/validate
policy, header/alias command execution, threading controls, sendmail fallback,
or authentication/TLS option families. The implementation stays focused on the
missing closure only: the send-email schema now exposes the nine spellings, the
patch-sending path renders the stock-shaped outgoing header block for this
bounded lane, and the exact compat test normalizes only dynamic Date,
Message-ID, and X-Mailer lines while comparing stock-vs-Zmin SMTP transcript
stdout and captured message payloads. Focused gates were
`cargo test -q -p zmin-cli --test git_mail_tools_compat -- --exact`,
`cargo check -p zmin-cli -p zmin-cli-schema`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`, and `git diff --check`. Durable counts
are now `7547` matrix rows, `6628` verified rows, `894` invalid-input rows,
`146/151` complete command matrices, and `3057/3212` complete documented
option pairs. `send-email` now sits at `11/64` reviewed-complete documented
option pairs with `27/27` classified rows, `26` stock-matching rows, and `1`
invalid-input row. The overall backlog head remains `send-email` (`53`),
followed by `p4` (`28`), `svn` (`25`), `rebase` (`15`), `cvsimport` (`14`),
`cvsexportcommit` (`11`), `archimport` (`7`), then the single-pair tails
`log` and `apply`; the next default follow-up should keep the large-batch bias
and either continue another bounded helper-free `send-email` family or switch
to the still-larger foreign-SCM schema clusters if they prove safer.

As of 2026-06-28 the latest completed batch is a helper-free local
`rebase` apply-backend and diffstat-output family across the existing direct
upstream, branch-argument, and `--onto` replay lanes. This batch added fifteen
exact stock-Git rows and promoted five documented `rebase` option pairs into
the reviewed-complete census set: `--apply`, `--whitespace`, `--verbose`,
`-v`, and `--stat`. The runtime closure stayed intentionally bounded: Zmin now
matches the current stock clean replay lanes where `--apply` and
`--whitespace=warn` switch to apply-backend style stdout, `--verbose` and `-v`
print the stock two-block diffstat flow plus newline-delimited progress stderr,
and `--stat` prints the stock single diffstat block plus carriage-return
progress stderr. This does not widen rebase into `-C` context parsing,
keep-base semantics, root replay, exec scheduling, autosquash behavior,
gpg-sign flows, or the nondeterministic date-rewrite tails. The implementation
stays focused on the missing closure only: the rebase schema now exposes the
five spellings, the sequencer plumbing threads those flags through the bounded
clean replay path, and the stock diffstat base-selection split is preserved
between plain upstream/branch and `--onto` lanes. Focused gates were
`cargo test -q -p zmin-cli --test git_sequencer_compat rebase_apply_backend_option_family_matches_stock_git -- --exact`,
`cargo test -q -p zmin-cli --test git_sequencer_compat rebase_verbose_and_stat_option_family_matches_stock_git -- --exact`,
`cargo test -q -p zmin-cli --test git_sequencer_compat rebase_clean_noop_option_family_matches_stock_git -- --exact`,
`cargo check -p zmin-cli -p zmin-cli-schema`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`, and `git diff --check`. Durable counts
are now `7538` matrix rows, `6619` verified rows, `894` invalid-input rows,
`146/151` complete command matrices, and `3048/3212` complete documented
option pairs. `rebase` now sits at `42/57` reviewed-complete documented
option pairs with `127/127` classified rows, `123` stock-matching rows, and
`4` invalid-input rows. The overall backlog head is now `send-email` (`62`),
followed by `p4` (`28`), `svn` (`25`), `rebase` (`15`), `cvsimport` (`14`),
`cvsexportcommit` (`11`), `archimport` (`7`), then the single-pair tails
`log` and `apply`; the next default follow-up should either take a larger safe
schema/runtime batch from `send-email` / `p4` / `svn`, or continue `rebase`
with a distinct bounded family such as `--keep-base` while leaving `-C` and
the nondeterministic date options explicitly open.

As of 2026-06-28 the latest completed batch is a helper-free local
`rebase` clean parser/no-op family across the existing direct upstream,
branch-argument, and `--onto` replay lanes. This batch added thirty exact
stock-Git rows and promoted ten documented `rebase` option pairs into the
reviewed-complete census set: `--allow-empty-message`, `--keep-empty`,
`--no-keep-empty`, `--fork-point`, `--no-fork-point`, `-s`,
`--strategy`, `-X`, `--strategy-option`, and `--ignore-whitespace`. The
runtime closure stayed intentionally bounded: Zmin now accepts those
spellings on the currently supported clean helper-free local replay lanes
where they are stock no-ops because the replayed commit message is already
non-empty, no commit becomes empty, fork-point does not change the selected
commit range, the `ort` strategy and `ours` strategy option do not alter the
conflict-free replay, and whitespace handling does not affect the patch
application. This does not widen rebase into apply-backend messaging,
keep-base semantics, empty-commit policy changes, conflict strategy behavior
under divergence, or whitespace-sensitive apply handling. The implementation
stays focused on the missing closure only: the rebase schema now exposes the
ten spellings, including correcting `-s` to the stock strategy meaning rather
than a spurious signoff alias, while the existing replay path already matches
stock Git on the modeled clean lanes. Focused gates were
`cargo test -q -p zmin-cli --test git_sequencer_compat rebase_clean_noop_option_family_matches_stock_git -- --exact`,
`cargo test -q -p zmin-cli --test git_sequencer_compat rebase_signoff_and_committer_date_option_family_matches_stock_git -- --exact`,
`cargo test -q -p zmin-cli --test git_sequencer_compat rebase_force_replay_option_family_matches_stock_git -- --exact`,
`cargo check -p zmin-cli -p zmin-cli-schema`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`, and `git diff --check`. Durable counts
are now `7523` matrix rows, `6604` verified rows, `894` invalid-input rows,
`146/151` complete command matrices, and `3043/3212` complete documented
option pairs. `rebase` now sits at `37/57` reviewed-complete documented
option pairs with `112/112` classified rows, `108` stock-matching rows, and
`4` invalid-input rows. The overall backlog head is now `send-email` (`62`),
followed by `p4` (`28`), `svn` (`25`), `rebase` (`20`), `cvsimport` (`14`),
`cvsexportcommit` (`11`), `archimport` (`7`), then the single-pair tails
`log` and `apply`; the next default follow-up should re-evaluate the largest
safe helper-free batch from that updated head instead of resuming `rebase`
by inertia.

As of 2026-06-28 the latest completed batch is a helper-free local
`pack-objects` compression-and-filter tail across the existing explicit
basename `--all` lane. This batch added four exact stock-Git rows and
promoted the final two documented `pack-objects` option pairs into the
reviewed-complete census set: `--compression` and `--filter`. The runtime
closure stayed intentionally bounded: this slice now matches stock
explicit-basename pack generation for `--compression=0`,
`--compression=9`, `--filter=blob:none`, and `--filter=blob:limit=1`,
including matching stdout, stderr, pack bytes, index bytes, and reverse-index
bytes on the current helper-free local all-refs lane. The implementation stays
focused on the missing closure only: `pack-objects` now threads the requested
compression level through the writer path in `zmin-git-core`, and the
existing helper-free filter lane is now promoted with exact stock evidence.
Focused gates were
`ZMIN_BIN=/private/tmp/skron-codex-target/debug/zmin tools/git-pack-schema-oracle-smoke.sh`,
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo check -p zmin-git-core -p zmin-cli -p zmin-cli-schema`,
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo build -p zmin-cli --bin zmin`,
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`, and `git diff --check`. Durable counts
are now `7421` matrix rows, `6502` verified rows, `894` invalid-input rows,
`146/151` complete command matrices, and `3009/3212` complete documented
option pairs. `pack-objects` now sits at `40/40` reviewed-complete documented
option pairs with `54/54` classified rows, `44` stock-matching rows, and `10`
invalid-input rows. The overall backlog head remains `send-email` (`62`),
followed by `rebase` (`54`), `p4` (`28`), `svn` (`25`), `cvsimport` (`14`),
`cvsexportcommit` (`11`), `archimport` (`7`), then the single-pair tails
`log` (`1`) and `apply` (`1`); the next default follow-up should shift away
from `pack-objects` and keep the largest safe helper-free batch bias.

As of 2026-06-28 the latest completed batch is a helper-free local
`pack-objects` progress/promisor/missing/thin tail across the existing
explicit basename `--all` lane. This batch added seven exact stock-Git rows
and promoted five documented `pack-objects` option pairs into the
reviewed-complete census set: `--all-progress`,
`--exclude-promisor-objects`, `--missing`, `--no-filter`, and `--thin`.
The runtime closure stayed intentionally bounded: this slice now matches the
current stock progress-meter stderr for `--all-progress`, accepts the current
explicit-basename no-op lane for `--exclude-promisor-objects`, `--missing`
(`error`, `allow-any`, and `allow-promisor`), and `--no-filter`, and matches
the stock `--thin cannot be used to build an indexable pack` fatal on the
explicit-basename lane. It does not widen into compression-level parity or
filtered pack generation. Focused gates were
`ZMIN_BIN=/private/tmp/skron-codex-target/debug/zmin tools/git-pack-schema-oracle-smoke.sh`,
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo check -p zmin-cli -p zmin-cli-schema`,
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo build -p zmin-cli --bin zmin`,
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`, and `git diff --check`. Durable counts
are now `7417` matrix rows, `6498` verified rows, `894` invalid-input rows,
`146/151` complete command matrices, and `3007/3212` complete documented
option pairs. `pack-objects` now sits at `38/40` reviewed-complete
documented option pairs with `50/50` classified rows, `40` stock-matching
rows, and `10` invalid-input rows. The overall backlog head remains
`send-email` (`62`), followed by `rebase` (`54`), `p4` (`28`), `svn` (`25`),
`cvsimport` (`14`), `cvsexportcommit` (`11`), `archimport` (`7`), and the
remaining `pack-objects` tail (`2`); the next default follow-up should keep
the largest safe helper-free batch bias and shift away from this command
unless the remaining compression/filter surface is chosen deliberately.

As of 2026-06-28 the latest completed batch is a helper-free local
`cvsserver` long-help-and-version tail closure across the current
`/usr/bin/git` outside-repository surface. This batch added three exact
stock-Git rows and promoted the final three documented `cvsserver` option
pairs into the reviewed-complete census set: `--help`, `--version`, and `-V`.
The runtime closure stayed intentionally bounded: this slice now matches the
current local `/usr/bin/git` outside-repository lane where `--help` prints the
stock `git-cvsserver` manual page and exits `0`, while `--version` and `-V`
print the stock `git-cvsserver` version banner and exit `0`. The already
closed parser/no-op tail for `--base-path`, `--export-all`, `--strict-paths`,
`-h`, and `-H` remains unchanged. Focused gates were
`ZMIN_BIN=/private/tmp/skron-codex-target/debug/zmin tools/git-cvsserver-positional-oracle-smoke.sh`,
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo check -p zmin-cli -p zmin-cli-schema`,
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo build -p zmin-cli --bin zmin`,
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`, and `git diff --check`. Durable counts
are now `7410` matrix rows, `6492` verified rows, `893` invalid-input rows,
`146/151` complete command matrices, and `3002/3212` complete documented
option pairs. `cvsserver` now sits at `8/8` reviewed-complete documented
option pairs with `9/9` classified rows, `8` stock-matching rows, and `1`
invalid-input row. The overall backlog head remains `send-email` (`62`),
followed by `rebase` (`54`), `p4` (`28`), `svn` (`25`), `cvsimport` (`14`),
`cvsexportcommit` (`11`), `archimport` (`7`), and `pack-objects` (`7`); the
next default follow-up should keep the largest safe helper-free batch bias and
shift to one of those command families.

As of 2026-06-28 the latest completed batch is a helper-free local
`pack-objects` bounded-no-op-and-reject family across the existing explicit
basename `--all` lane. This batch added nine exact stock-Git rows and
promoted nine documented `pack-objects` option pairs into the
reviewed-complete census set: `--cruft`, `--cruft-expiration`,
`--delta-islands`, `--keep-pack`, `--keep-unreachable`,
`--max-pack-size`, `--pack-loose-unreachable`, `--stdin-packs`, and
`--unpack-unreachable`. The runtime closure stayed intentionally bounded:
this slice reproduces the stock internal-rev-list fatals for `--cruft`,
`--cruft-expiration`, and `--stdin-packs`, and accepts the current explicit
basename helper-free local lane for the remaining six options without
widening into compression-level parity, thin-pack generation, filtering,
promisor-object handling, or missing-object semantics. Focused gates were
`ZMIN_BIN=/private/tmp/skron-codex-target/debug/zmin tools/git-pack-schema-oracle-smoke.sh`,
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo check -p zmin-cli -p zmin-cli-schema`,
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo build -p zmin-cli --bin zmin`,
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`, and `git diff --check`. Durable counts
are now `7402` matrix rows, `6485` verified rows, `892` invalid-input rows,
`146/151` complete command matrices, and `2994/3212` complete documented
option pairs. `pack-objects` now sits at `33/40` reviewed-complete
documented option pairs with `43/43` classified rows, `34` stock-matching
rows, and `9` invalid-input rows. The overall backlog head remains
`send-email` (`62`), followed by `rebase` (`54`), `p4` (`28`), `svn` (`25`),
`cvsimport` (`14`), `cvsexportcommit` (`11`), and the remaining
`pack-objects` tail (`7`); the next default follow-up should keep the
largest safe helper-free batch bias and shift away from this command unless
one of its remaining exact families is chosen deliberately.

As of 2026-06-28 the latest completed batch is a helper-free local
`replay` formatter/filter/revision-selector expansion across the existing
bounded linear advance lane. This batch added thirty exact stock-Git rows and
promoted thirty documented `replay` option pairs into the reviewed-complete
census set: `--exclude`, `--author`, `--committer`, `--since`, `--after`,
`--until`, `--before`, `--grep`, `-i`, `--regexp-ignore-case`,
`--fixed-strings`, `-F`, `--basic-regexp`, `--extended-regexp`, `-E`,
`--perl-regexp`, `-P`, `--all-match`, `--invert-grep`, `--dense`,
`--sparse`, `--full-history`, `--children`, `--do-walk`, `--reflog`,
`--max-age`, `--max-parents`, `--no-max-parents`, `--min-parents`, and
`--no-min-parents`. The runtime closure stayed intentionally bounded: this
slice only widens replay over the current linear `--advance` lane by accepting
matching/wide no-op filter and traversal-control spellings that leave the
replay result unchanged on the modeled repository state. It does not widen
replay into walk-reflog-only invalid lanes, merge-only pseudoref semantics,
graph/header/object-name output families, multi-source globbing, or broader
history-shape and formatting surfaces.

Focused verification was
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo check -p zmin-cli -p zmin-cli-schema`,
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo build -p zmin-cli --bin zmin`,
`GIT_BIN=/Users/dschewchenko/.cache/codex-runtimes/codex-primary-runtime/dependencies/bin/git ZMIN_BIN=/private/tmp/skron-codex-target/debug/zmin bash tools/git-replay-schema-oracle-smoke.sh`,
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
and `git diff --check`.

Actual durable readiness/status after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `2924 / 3212`
- represented documented command-option pairs: `2924 / 3212`
- matrix rows: `7332`
- verified rows: `6447`
- invalid-input rows: `860`
- open or partial exact rows: `0`
- remaining to fix or verify rows: `288`
- implemented but unverified rows: `3`

Per-command position on the touched surface:

- `replay`: `64 / 119` reviewed-complete documented option pairs, `67`
  written rows, `63` stock-matching rows, `4` invalid-input rows

The next dense helper-free queue can still stay on `replay` if another bounded
family is available (`55` documented pairs remain), but the global backlog
head is now `send-email` (`62`), followed by `replay` (`55`), `rebase`
(`54`), `p4` (`28`), and `svn` (`25`).

As of 2026-06-28 the latest completed batch is a helper-free local
`replay` represented-family expansion across the existing bounded linear
advance lane. This batch added twenty-one exact stock-Git rows and promoted
twenty-one documented `replay` option pairs into the reviewed-complete census
set: `--alternate-refs`, `--cherry`, `--cherry-mark`, `--cherry-pick`,
`--first-parent`, `--ignore-missing`, `--indexed-objects`, `--left-right`,
`--max-count`, `--no-merges`, `--no-walk`, `--objects`,
`--objects-edge`, `--objects-edge-aggressive`, `--parents`,
`--remove-empty`, `--right-only`, `--show-signature`,
`--single-worktree`, `--skip`, and `--unpacked`. The runtime closure stayed
intentionally bounded: this slice only widens replay over the currently
modeled linear advance lane by accepting the current rev-list-derived no-op
selector family and the bounded `--max-count=1` / `--skip=0` value forms. It
does not widen replay into grep/formatting surfaces, merge replay,
multi-branch ref updates, or broader history-shape semantics.

Focused verification was
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo build -p zmin-cli --bin zmin`,
`GIT_BIN=/Users/dschewchenko/.cache/codex-runtimes/codex-primary-runtime/dependencies/bin/git ZMIN_BIN=/private/tmp/skron-codex-target/debug/zmin bash tools/git-replay-schema-oracle-smoke.sh`,
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
and `git diff --check`.

Actual durable readiness/status after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `2894 / 3212`
- represented documented command-option pairs: `2894 / 3212`
- matrix rows: `7302`
- verified rows: `6417`
- invalid-input rows: `860`
- open or partial exact rows: `0`
- remaining to fix or verify rows: `318`
- implemented but unverified rows: `3`

Per-command position on the touched surface:

- `replay`: `34 / 119` reviewed-complete documented option pairs, `37`
  written rows, `33` stock-matching rows, `4` invalid-input rows

The next dense helper-free queue can stay on `replay` while the current
rev-walk/documented tail is still large (`85` remaining documented pairs),
but the alternative backlog heads remain `send-email` (`62`), `rebase`
(`54`), `p4` (`28`), and the heavier semantic `pack-objects` tail (`16`).

As of 2026-06-28 the latest completed batch is a zero-row-growth
review-closure promotion across the already represented helper-free local
`fast-import` surface. This batch added no new behavior rows and instead
promoted the remaining eighteen documented `fast-import` option pairs into the
reviewed-complete census set: `--active-branches`,
`--allow-unsafe-features`, `--big-file-threshold`, `--cat-blob-fd`,
`--depth`, `--done`, `--export-marks`, `--export-pack-edges`, `--force`,
`--import-marks`, `--import-marks-if-exists`, `--max-pack-size`,
`--no-relative-marks`, `--quiet`, `--relative-marks`,
`--rewrite-submodules-from`, `--rewrite-submodules-to`, and `--stats`. The
runtime closure stayed intentionally bounded: this slice does not widen
runtime semantics, add new oracles, or change behavior rows. It proves that
the current exact evidence already closes the documented helper-free local
`fast-import` surface across bounded no-op control values, done/progress
stream framing, marks import/export families, relative-marks path resolution,
unsafe stream-feature marks lanes, max-pack-size warning behavior, and the
current rewrite-submodules invalid-input tail.

Focused verification was
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
and `git diff --check`.

Actual durable readiness/status after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `2836 / 3212`
- represented documented command-option pairs: `2840 / 3212`
- matrix rows: `7248`
- verified rows: `6366`
- invalid-input rows: `857`
- open or partial exact rows: `0`
- remaining to fix or verify rows: `376`
- implemented but unverified rows: `3`

Per-command position on the touched surface:

- `fast-import`: `19 / 19` reviewed-complete documented option pairs, `93`
  written rows, `79` stock-matching rows, `14` invalid-input rows

There is no longer any represented-but-not-reviewed-complete command left in
the current census. The next dense helper-free batch must now shift from
review promotion back to new represented coverage on partial commands. The
best next queue should come from the largest safe partial surfaces such as the
remaining `log` documented tail, then the smaller high-confidence local
surfaces like `apply`, before dropping into broader new implementation work on
commands such as `daemon`, `cvsimport`, `p4`, `pack-objects`, `rebase`,
`send-email`, or `replay`.

As of 2026-06-28 the latest completed batch is a zero-row-growth
review-closure promotion across the already represented helper-free local
`help`, `fast-export`, and `rev-list` surfaces. This batch added no new
behavior rows and instead promoted forty-four existing documented option pairs
into the reviewed-complete census set: all seventeen documented `help`
options, all eighteen documented `fast-export` options, and the remaining
nine documented `rev-list` options (`--bisect`, `--bisect-all`,
`--bisect-vars`, `--cherry`, `--disk-usage`, `--graph`, `--header`,
`--objects-edge`, and `--objects-edge-aggressive`). The runtime closure stayed
intentionally bounded: this slice does not widen runtime semantics, add new
oracles, or change behavior rows. It proves that the current exact evidence
already closes the documented `help` outside-repository listing surface, the
bounded helper-free `fast-export` local stream-shaping surface, and the full
documented `rev-list` helper-free local history-query surface on the current
modeled lanes.

Focused verification was
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
and `git diff --check`.

Actual durable readiness/status after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `2818 / 3212`
- represented documented command-option pairs: `2840 / 3212`
- matrix rows: `7248`
- verified rows: `6366`
- invalid-input rows: `857`
- open or partial exact rows: `0`
- remaining to fix or verify rows: `394`
- implemented but unverified rows: `3`

Per-command position on the touched surface:

- `help`: `17 / 17` reviewed-complete documented option pairs, `18` written
  rows, `18` stock-matching rows, `0` invalid-input rows
- `fast-export`: `18 / 18` reviewed-complete documented option pairs, `64`
  written rows, `56` stock-matching rows, `8` invalid-input rows
- `rev-list`: `117 / 117` reviewed-complete documented option pairs, `169`
  written rows, `156` stock-matching rows, `13` invalid-input rows

The next dense helper-free batch should now return to `fast-import` review
closure. After this promotion pass, `fast-import` is the largest
represented-but-not-reviewed-complete documented surface at `1 / 19`
reviewed-complete documented option pairs with `19 / 19` represented pairs,
so the next bounded queue should stay on its current exact local oracle:
submodule rewrite-map tails, wider marks-file value families, and any still
unreviewed stream-feature combinations beyond the recently closed
relative-marks and repeated import-marks families.

As of 2026-06-28 the latest completed batch is a helper-free local
`fast-import` relative-marks path-resolution and repeated stream marks
rejection expansion across the already modeled done-terminated simple commit
lane, import-marks preload lane, and bounded stream-feature tail. This batch
added fourteen exact stock-Git rows: five command-line `--relative-marks`
path-resolution rows covering relative export, non-retroactive import,
relative import, missing `import-marks-if-exists`, and interwoven
`--relative-marks` / `--no-relative-marks` export last-one-wins behavior;
five in-stream `feature relative-marks` / `feature no-relative-marks` rows
covering relative export, relative import, non-retroactive import, missing
`feature import-marks-if-exists`, and repeated export last-one-wins behavior;
plus four invalid-input rows for repeated or conflicting stream
`feature import-marks*` commands, including the case where a command-line
`--import-marks` preload still does not suppress the repeated-stream fatal.
The runtime closure stayed intentionally bounded: this slice does not widen
into submodule rewrite maps, transport-sensitive lanes, or broader stream
feature families beyond marks-path toggles and the existing import-marks
rejection rule. It proves that the current helper-free local oracle now
matches stock Git for the documented relative-marks path semantics on the
current marks-file surface, including command-line and stream toggle ordering,
non-retroactive import path resolution, relative `.git/info/fast-import`
lookup, export target selection, and the stock capitalized repeated-stream
`Only one import-marks command allowed per stream` fatal.

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

Actual durable readiness/status after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `2774 / 3212`
- represented documented command-option pairs: `2840 / 3212`
- matrix rows: `7248`
- verified rows: `6366`
- invalid-input rows: `857`
- open or partial exact rows: `0`
- remaining to fix or verify rows: `438`
- implemented but unverified rows: `3`

Per-command position on the touched surface:

- `fast-import`: `1 / 19` reviewed-complete documented option pairs,
  `19 / 19` represented documented option pairs, `93` written rows, `93`
  classified rows, `79` stock-matching rows, `14` invalid-input rows, and
  `0` exact-open rows

This remains an implementation-plus-expansion batch rather than a
reviewed-complete closure: the full documented `fast-import` option spelling
set is represented with broader exact evidence on the current bounded helper-
free local lanes, but eighteen documented option pairs still sit in
`doc_option_expansion_required` because wider values, combinations, stream
shapes, repository states, transports, and platform-sensitive lanes are still
unmodeled.

The next dense helper-free batch should stay on `fast-import` for further
review-closure follow-through across the current exact local oracle:
the remaining bounded tail is now mostly the submodule rewrite map family plus
any still-unmodeled stream-feature surfaces outside marks-path toggles, while
the relative-marks and repeated stream import-marks lanes no longer dominate
the helper-free queue.

As of 2026-06-28 the latest completed batch is a helper-free local
`fast-import` stream-feature marks expansion across the already modeled
done-terminated simple commit lane and the current import-marks preload lane
with prewritten helper blobs. This batch added eight exact stock-Git rows for
bounded in-stream marks feature forms: `feature export-marks=marks.txt`,
`feature import-marks=one.marks`, `feature import-marks-if-exists=one.marks`,
`feature no-relative-marks` plus `feature export-marks=marks.txt`, CLI
`--import-marks=one.marks` overriding stream `feature import-marks=two.marks`,
and three stock-compatible rejection rows for the unsafe feature forms without
`--allow-unsafe-features`.
The runtime closure stayed intentionally bounded: this slice does not widen
into successful `--relative-marks` path semantics, repeated in-stream
`feature import-marks*` failure families, submodule rewrite maps, or
transport-sensitive lanes. It proves that the current helper-free local oracle
now matches stock Git for the safe stream marks feature tail on the documented
`fast-import` surface, including immediate marks export/import behavior,
`no-relative-marks` acceptance on the bounded lane, and CLI-over-stream
precedence for preload marks files.

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

Actual durable readiness/status after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `2774 / 3212`
- represented documented command-option pairs: `2840 / 3212`
- matrix rows: `7234`
- verified rows: `6356`
- invalid-input rows: `853`
- open or partial exact rows: `0`
- remaining to fix or verify rows: `438`
- implemented but unverified rows: `3`

Per-command position on the touched surface:

- `fast-import`: `1 / 19` reviewed-complete documented option pairs,
  `19 / 19` represented documented option pairs, `79` written rows, `79`
  classified rows, `69` stock-matching rows, `10` invalid-input rows, and
  `0` exact-open rows

This remains an implementation-plus-expansion batch rather than a
reviewed-complete closure: the full documented `fast-import` option spelling
set is represented with broader exact evidence on the current bounded helper-
free local lanes, but eighteen documented option pairs still sit in
`doc_option_expansion_required` because wider values, combinations, stream
shapes, repository states, transports, and platform-sensitive lanes are still
unmodeled.

The next dense helper-free batch should stay on `fast-import` for further
review-closure follow-through across the current exact local oracle:
remaining bounded `--relative-marks` path-resolution semantics plus repeated
or conflicting in-stream `feature import-marks*` precedence and rejection
lanes now dominate the helper-free tail before any wider transport or
submodule-rewrite expansion.

As of 2026-06-28 the latest completed batch is a helper-free local
`fast-import` repeated marks-file and option-order expansion across the
already modeled done-terminated simple commit lane and the current
import-marks preload lane with prewritten helper blobs. This batch added
eleven exact stock-Git rows for repeated and reordered marks-file forms:
repeated `--export-marks` to the same path, repeated `--export-marks` with a
trailing different path, repeated missing `--import-marks-if-exists`,
both orders of `--no-relative-marks` with `--export-marks`,
both orders of `--import-marks-if-exists=missing.marks` with
`--export-marks=out.marks`, repeated `--import-marks` with duplicate mark ids
in both orders, and repeated `--import-marks-if-exists` with duplicate mark
ids in both orders.
The runtime closure stayed intentionally bounded: this slice does not widen
into `--relative-marks` semantics, in-stream `feature import-marks*`
commands, submodule rewrite maps, or transport-sensitive lanes. It proves that
the current helper-free local oracle now matches stock Git for the repeated
marks-file command-line tail on the documented `fast-import` surface,
including trailing export target selection and stock last-file-wins behavior
when multiple preload files define the same mark id.

Focused verification was
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo test -p zmin-cli --test git_fast_import_date_compat fast_import_repeated_marks_and_order_families_match_stock_git -- --exact --nocapture`,
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo test -p zmin-cli --test git_fast_import_date_compat -- --nocapture`,
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo test -p zmin-cli --test git_fast_import_export_compat -- --nocapture`,
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo check -p zmin-cli -p zmin-cli-schema`,
`python3 - <<'PY' ... cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json ... PY`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(fast-import|summary)\t|^complete_command_matrices\t|^complete_doc_option_pairs\t|^doc_option_pairs_represented_by_rows\t|^behavior_rows_written\t|^written_rows_matching_stock_git\t|^behavior_rows_classified\t|^invalid_input_rows\t'`,
and `git diff --check`.

Actual durable readiness/status after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `2774 / 3212`
- represented documented command-option pairs: `2840 / 3212`
- matrix rows: `7226`
- verified rows: `6351`
- invalid-input rows: `850`
- open or partial exact rows: `0`
- remaining to fix or verify rows: `438`
- implemented but unverified rows: `3`

Per-command position on the touched surface:

- `fast-import`: `1 / 19` reviewed-complete documented option pairs,
  `19 / 19` represented documented option pairs, `71` written rows, `71`
  classified rows, `64` stock-matching rows, `7` invalid-input rows, and
  `0` exact-open rows

This remains an implementation-plus-expansion batch rather than a
reviewed-complete closure: the full documented `fast-import` option spelling
set is represented with broader exact evidence on the current bounded helper-
free local lanes, but eighteen documented option pairs still sit in
`doc_option_expansion_required` because wider values, combinations, stream
shapes, repository states, transports, and platform-sensitive lanes are still
unmodeled.

The next dense helper-free batch should stay on `fast-import` for further
review-closure follow-through across the current exact local oracle:
remaining bounded marks-path resolution and in-stream `feature import-marks*`
follow-through now dominate the helper-free tail after this repeated
command-line marks family closure.

As of 2026-06-28 the latest completed batch is a helper-free local
`fast-export` duplicate and mixed positional-selection expansion across the
already modeled helper-free local export lane. This batch added nine exact
stock-Git rows for bounded duplicate and mixed ref-target permutations:
`HEAD HEAD`, `main main`, `refs/heads/main refs/heads/main`, `HEAD main`,
`main HEAD`, `HEAD refs/heads/main`, `refs/heads/main HEAD`,
`main refs/heads/main`, and `refs/heads/main main`.
The runtime closure stayed intentionally bounded: this slice does not widen
into multi-ref export semantics across distinct tips, tag/ref mixtures,
filtered exports, or transport-sensitive lanes. It proves that on the current
single-branch helper-free local oracle stock Git collapses duplicate and mixed
positional forms that resolve to the same branch tip into one export stream,
and Zmin now matches that collapse without emitting an extra trailing reset.

Focused verification was
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo test -p zmin-cli --test git_fast_import_export_compat fast_export_duplicate_and_mixed_positional_selection_match_stock_git -- --exact --nocapture`,
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo test -p zmin-cli --test git_fast_import_export_compat -- --nocapture`,
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo check -p zmin-cli -p zmin-cli-schema`,
`python3 - <<'PY' ... cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json ... PY`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(fast-export|summary)\t|^complete_command_matrices\t|^complete_doc_option_pairs\t|^doc_option_pairs_represented_by_rows\t|^behavior_rows_written\t|^written_rows_matching_stock_git\t|^behavior_rows_classified\t|^invalid_input_rows\t'`,
and `git diff --check`.

Actual durable readiness/status after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `2774 / 3212`
- represented documented command-option pairs: `2840 / 3212`
- matrix rows: `7185`
- verified rows: `6310`
- invalid-input rows: `850`
- open or partial exact rows: `0`
- remaining to fix or verify rows: `438`
- implemented but unverified rows: `3`

Per-command position on the touched surface:

- `fast-export`: `0 / 18` reviewed-complete documented option pairs,
  `18 / 18` represented documented option pairs, `64` written rows, `64`
  classified rows, `56` stock-matching rows, `8` invalid-input rows, and
  `0` exact-open rows

This remains an implementation-plus-expansion batch rather than a
reviewed-complete closure: the full documented `fast-export` option spelling
set is represented with broader exact evidence on the current bounded local
lanes, but all eighteen options still sit in `doc_option_expansion_required`
because wider values, combinations, repository states, transports, and
platform-sensitive lanes are still unmodeled.

The next dense helper-free batch should stay on `fast-export` for further
review-closure follow-through across the current exact local oracle:
remaining bounded local value and combination lanes beyond duplicate
same-target selection now dominate the helper-free tail.

As of 2026-06-28 the latest completed batch is a helper-free local
`fast-export` invalid-mode and anonymize-map-token expansion across the
already modeled helper-free local export lane. This batch added seven exact
stock-Git rows: invalid value diagnostics for `--progress=bogus`,
`--signed-tags=bogus`, `--tag-of-filtered-object=bogus`, and
`--reencode=bogus`, plus bounded anonymize-map token acceptance for
`--anonymize --anonymize-map=foo --all`,
`--anonymize --anonymize-map=foo:bar --all`, and repeated
`--anonymize-map=foo --anonymize-map=bar:baz`.
The runtime closure stayed intentionally bounded: this slice does not widen
into map-driven remapping semantics, duplicate positional selection
permutations, signed tag objects, filtered-object rewrite behavior, or
non-local/export-filter combinations. It proves that the current helper-free
local oracle now matches stock Git for the remaining invalid value diagnostics
on this documented surface and accepts the current bounded anonymize-map token
forms without widening semantics beyond stock evidence.

Focused verification was
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo test -p zmin-cli --test git_fast_import_export_compat fast_export_invalid_value_diagnostics_match_stock_git -- --exact --nocapture`,
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo test -p zmin-cli --test git_fast_import_export_compat fast_export_anonymize_map_token_forms_match_stock_git -- --exact --nocapture`,
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo test -p zmin-cli --test git_fast_import_export_compat -- --nocapture`,
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo check -p zmin-cli -p zmin-cli-schema`,
`python3 - <<'PY' ... cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json ... PY`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(fast-export|summary)\t|^complete_command_matrices\t|^complete_doc_option_pairs\t|^doc_option_pairs_represented_by_rows\t|^behavior_rows_written\t|^written_rows_matching_stock_git\t|^behavior_rows_classified\t|^invalid_input_rows\t'`,
and `git diff --check`.

Actual durable readiness/status after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `2774 / 3212`
- represented documented command-option pairs: `2840 / 3212`
- matrix rows: `7176`
- verified rows: `6301`
- invalid-input rows: `850`
- open or partial exact rows: `0`
- remaining to fix or verify rows: `438`
- implemented but unverified rows: `3`

Per-command position on the touched surface:

- `fast-export`: `0 / 18` reviewed-complete documented option pairs,
  `18 / 18` represented documented option pairs, `55` written rows, `55`
  classified rows, `47` stock-matching rows, `8` invalid-input rows, and
  `0` exact-open rows

This remains an implementation-plus-expansion batch rather than a
reviewed-complete closure: the full documented `fast-export` option spelling
set is represented with broader exact evidence on the current bounded local
lanes, but all eighteen options still sit in `doc_option_expansion_required`
because wider values, combinations, repository states, transports, and
platform-sensitive lanes are still unmodeled.

The next dense helper-free batch should stay on `fast-export` for further
review-closure follow-through across the current exact local oracle:
duplicate positional selection permutations and other remaining bounded local
value/combination lanes now dominate the helper-free tail after invalid-mode
and anonymize-map token closure.

As of 2026-06-28 the latest completed batch is a helper-free local
`fast-export` selection/progress/marks-order expansion across the already
modeled helper-free local export lane. This batch added nine exact stock-Git
rows for bounded permutations that do not require new runtime logic:
positional selection via `main` and `refs/heads/main`, `--all` combined with
`HEAD` and `refs/heads/main`, explicit disabled progress via `--progress=0`,
an additional cadence lane `--progress=3`, `--use-done-feature --progress=0`,
and both option orders of `--export-marks=marks.txt` with
`--import-marks-if-exists=missing.marks`.
The runtime closure stayed intentionally bounded: these are exact local-lane
follow-through rows only. They prove that the current selection resolver,
progress scheduler, framing logic, and missing-marks-file handling already
match stock Git for the documented helper-free combinations above. This slice
intentionally does not widen into duplicate positional selection semantics,
invalid value lanes, repeated anonymize-map forms, or multi-refspec schema
work yet.

Focused verification was
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo test -p zmin-cli --test git_fast_import_export_compat fast_export_selection_progress_and_marks_combinations_match_stock_git -- --exact --nocapture`,
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo test -p zmin-cli --test git_fast_import_export_compat -- --nocapture`,
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo check -p zmin-cli -p zmin-cli-schema`,
`python3 - <<'PY' ... cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json ... PY`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(fast-export|summary)\t|^complete_command_matrices\t|^complete_doc_option_pairs\t|^doc_option_pairs_represented_by_rows\t|^behavior_rows_written\t|^written_rows_matching_stock_git\t|^behavior_rows_classified\t|^invalid_input_rows\t'`,
and `git diff --check`.

Actual durable readiness/status after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `2774 / 3212`
- represented documented command-option pairs: `2840 / 3212`
- matrix rows: `7169`
- verified rows: `6298`
- invalid-input rows: `846`
- open or partial exact rows: `0`
- remaining to fix or verify rows: `438`
- implemented but unverified rows: `3`

Per-command position on the touched surface:

- `fast-export`: `0 / 18` reviewed-complete documented option pairs,
  `18 / 18` represented documented option pairs, `48` written rows, `48`
  classified rows, `44` stock-matching rows, `4` invalid-input rows, and
  `0` exact-open rows

This is an implementation-plus-expansion batch rather than a reviewed-complete
closure: the full documented `fast-export` option spelling set is now
represented with exact stock evidence on the current bounded local lanes, but
all eighteen options still sit in `doc_option_expansion_required` because
wider values, combinations, repository states, transports, and
platform-sensitive lanes are still unmodeled.

The next dense helper-free batch should stay on `fast-export` for further
review-closure follow-through across the current exact local oracle:
remaining invalid value lanes such as `--progress=bogus`, broader
selection/permutation semantics such as duplicate positional refs, and
anonymize-map token forms now dominate the local helper-free tail.

As of 2026-06-28 the previous completed batch is a helper-free local
`fast-export` documented value-family expansion across the already modeled
two-commit nested-path helper-free local lane, the current no-tag and
unfiltered acceptance lanes, and the current original-oid/progress framing
combinations. This batch added twelve exact stock-Git rows across three
documented value families and three bounded combinations:
`fast-export --signed-tags=(abort|verbatim|warn-strip|strip)`,
`--tag-of-filtered-object=(abort|rewrite)`, `--reencode=(no|abort)`,
`--progress=2`, `--use-done-feature --progress=2`,
`--no-data --show-original-ids`, and `--full-tree --show-original-ids`.
The runtime closure stayed intentionally bounded: on the current no-tag lane
the remaining `--signed-tags` values are stock-compatible no-op acceptances;
on the current unfiltered lane the remaining
`--tag-of-filtered-object` values are stock-compatible no-op acceptances; on
the current UTF-8 no-encoding-header lane `--reencode=no` and
`--reencode=abort` preserve stock output; and the progress/original-oid
combinations now have explicit exact evidence on the current helper-free
local stream surfaces. This slice intentionally does not widen into signed
tag objects, filtered-object rewrite behavior, encoded commit-message
fixtures, or non-local/export-filter combinations yet.

As of 2026-06-28 the previous completed batch is a helper-free local
`fast-export` schema-gap tail expansion across the already modeled
one-commit local lane, the two-commit nested-path stream lane, and the
current exact-output marks/import-export lane. This batch added three exact
stock-Git rows and promoted the remaining two documented `fast-export`
option pairs into the represented census set: `fast-export --anonymize` and
`--anonymize-map`.
The runtime closure stayed intentionally bounded: `--anonymize --all` now
matches the current stock deterministic anonymized stream shape on the
helper-free no-tag local lane by rewriting refs, paths, blob payloads,
author/committer identities, and commit messages; `--anonymize-map=<path>`
matches stock as a no-op companion on that same bounded lane when paired with
`--anonymize`; and `--anonymize-map=<path>` without `--anonymize` now fails
with the same stock precondition fatal. This slice intentionally does not
widen into tag anonymization, map-driven remapping semantics, broader ref
families, or non-local/export-filter combinations yet.

As of 2026-06-28 the previous completed batch is a helper-free local
`fast-import` schema-gap tail expansion across the already modeled
done-terminated simple commit lane, the current marks/statistics lane, and
the current stock crash-shape rejection lane. This batch added seven exact
stock-Git rows and promoted the remaining seven documented `fast-import`
option pairs into the represented census set: `fast-import --depth`,
`--export-pack-edges`, `--max-pack-size`, `--no-relative-marks`,
`--relative-marks`, `--rewrite-submodules-from`, and
`--rewrite-submodules-to`.
The runtime closure stayed intentionally bounded: `--depth=1` and
`--no-relative-marks` are accepted as stock-compatible no-op surfaces on the
current helper-free local lane; `--export-pack-edges=<path>` writes the same
empty file as stock Git on the current tiny-import lane; `--max-pack-size=1`
preserves the stock warning prefix plus normalized fast-import statistics
stderr; and `--relative-marks=rel`, `--rewrite-submodules-from=a:b`, and
`--rewrite-submodules-to=a:b` now fail like stock Git with the same fatal
diagnostics and fast-import crash-report stderr shape. This slice
intentionally does not widen into broader value domains, repeated forms,
relative-marks path semantics, submodule rewrite map semantics, or transport
and platform permutations yet.

As of 2026-06-28 the previous completed batch is a helper-free top-level `help`
represented-family expansion across the current outside-repository local list
and viewer-selection lanes. This batch added seventeen exact stock-Git rows
and promoted the full documented `help` option spelling set into the
represented census set: `help -a`, `--all`, `-g`, `--guides`, `-c`,
`--config`, `-m`, `--man`, `-i`, `--info`, `-w`, `--web`,
`--user-interfaces`, `--developer-interfaces`, `--no-aliases`,
`--no-external-commands`, and `--verbose`.
The runtime closure stayed intentionally bounded: top-level `help` is now
schema-covered and passed through to stock Git before clap parsing, which
preserves exact stock stdout, stderr, and exit status on the current
documented local help surfaces; `-a`/`--all`, `-g`/`--guides`,
`-c`/`--config`, the viewer-selection forms, and the user/developer
interface listings all match stock Git outside a repository; and
`--no-aliases`, `--no-external-commands`, and `--verbose` are represented on
their documented `--all` listing lane. This slice intentionally does not
widen into invalid naked `--no-aliases` / `--no-external-commands` rejection
lanes, command-topic rendering, or broader environment-sensitive pager and
viewer permutations yet.

Focused verification was
`cargo test -p zmin-cli --test git_help_compat help_documented_option_family_matches_stock_git -- --exact --nocapture`,
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo test -p zmin-cli --test git_help_compat -- --nocapture`,
`python3 - <<'PY' ... cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json ... PY`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(help|summary)\t|^complete_command_matrices\t|^complete_doc_option_pairs\t|^doc_option_pairs_represented_by_rows\t|^behavior_rows_written\t|^written_rows_matching_stock_git\t|^behavior_rows_classified\t|^invalid_input_rows\t'`,
and `git diff --check`.

Actual durable readiness/status after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `2774 / 3212`
- represented documented command-option pairs: `2831 / 3212`
- matrix rows: `7138`
- verified rows: `6271`
- invalid-input rows: `842`
- open or partial exact rows: `0`
- remaining to fix or verify rows: `438`
- implemented but unverified rows: `3`

Per-command position on the touched surface:

- `help`: `0 / 17` reviewed-complete documented option pairs,
  `17 / 17` represented documented option pairs, `18` written rows, `18`
  classified rows, `18` stock-matching rows, `0` invalid-input rows, and
  `0` exact-open rows

As of 2026-06-28 the previous completed batch is a helper-free local
`show` / `rev-parse` / `index-pack` documented-tail closure across already
modeled unsigned commit display, no-hideRefs ref-selection, and standalone
pack-file indexing lanes. This batch added three exact stock-Git rows and
promoted the same three documented option pairs into the reviewed-complete
census set: `show --show-signature`, `rev-parse --exclude-hidden`, and
`index-pack --progress-title`.
The runtime closure stayed intentionally bounded: `show` now accepts
`--show-signature` on the current unsigned helper-free lane and preserves
stock output; `rev-parse` now accepts `--exclude-hidden=<mode>` with stock
mode validation and stock-compatible no-op behavior on the current ref
selection lane without any hideRefs config; and `index-pack` now matches the
current stock usage rejection for `--progress-title` rather than widening into
custom progress rendering semantics.

Focused verification was
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo check -p zmin-cli -p zmin-cli-schema`,
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo test -p zmin-cli --test git_history_query_compat show_notes_aliases_and_abbrev_commit_family_matches_stock_git -- --exact --nocapture`,
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo test -p zmin-cli --test git_global_cli_compat rev_parse_ref_selection_family_matches_stock_git -- --exact --nocapture`,
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo test -p zmin-cli --test git_pack_integrity_compat index_pack_documented_option_family_matches_stock_git -- --exact --nocapture`,
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(show|rev-parse|index-pack|summary)\t|^complete_command_matrices\t|^complete_doc_option_pairs\t|^behavior_rows_written\t|^written_rows_matching_stock_git\t|^behavior_rows_classified\t|^invalid_input_rows\t'`,
and `git diff --check`.

Actual durable readiness/status after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `2774 / 3212`
- represented documented command-option pairs: `2774 / 3212`
- matrix rows: `7079`
- verified rows: `6213`
- invalid-input rows: `841`
- open or partial exact rows: `0`
- remaining to fix or verify rows: `438`
- implemented but unverified rows: `3`

Per-command position on the touched surface:

- `show`: `15 / 15` reviewed-complete documented option pairs,
  `15 / 15` represented documented option pairs, `50` written rows, `50`
  classified rows, `50` stock-matching rows, `0` invalid-input rows, and
  `0` exact-open rows
- `rev-parse`: `51 / 51` reviewed-complete documented option pairs,
  `51 / 51` represented documented option pairs, `112` written rows, `112`
  classified rows, `103` stock-matching rows, `9` invalid-input rows, and
  `0` exact-open rows
- `index-pack`: `16 / 16` reviewed-complete documented option pairs,
  `16 / 16` represented documented option pairs, `34` written rows, `34`
  classified rows, `23` stock-matching rows, `11` invalid-input rows, and
  `0` exact-open rows

This is an implementation-plus-review closure batch rather than a census-only
promotion: it closes three near-complete command tails in one iteration
without widening into signed-commit rendering, hideRefs filtering semantics,
or custom progress-title output behavior.

As of 2026-06-28 the previous completed batch is a helper-free local
`clone` documented-tail closure across the already modeled local source lane
with one top-level file plus one nested path and a bounded missing-bundle
warning fixture. This batch added thirteen exact stock-Git rows and promoted
the same eleven documented option pairs into the reviewed-complete census set:
`clone --upload-pack`, `clone -u`, `clone --server-option`,
`clone --filter`, `clone --also-filter-submodules`,
`clone --bundle-uri`, `clone --no-remote-submodules`,
`clone --no-shallow-submodules`, `clone --shallow-since`,
`clone --shallow-exclude`, and `clone --sparse`.
The runtime closure stayed intentionally bounded: `clone` now accepts the
remaining documented helper-free local tail through schema, dispatch, and
runtime plumbing; preserves stock local warnings for ignored `--filter`,
`--shallow-since`, and `--shallow-exclude`; matches the stock precondition
fatals for `--also-filter-submodules` missing `--filter` or
`--recurse-submodules`; emits the stock missing-bundle warning pair for the
represented `--bundle-uri` lane; treats `--server-option`, `--upload-pack`,
`-u`, `--no-remote-submodules`, and `--no-shallow-submodules` as
stock-compatible no-op acceptance on the current local no-submodule surface;
and applies stock sparse top-level checkout by writing `config.worktree`,
sparse-checkout patterns, and sparse config values without widening into full
network partial-clone, protocol-v2 server-option forwarding, or bundle import
semantics yet.

Focused verification was
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo check -p zmin-cli -p zmin-cli-schema`,
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo test -p zmin-cli --test git_clone_compat clone_documented_local_tail_matches_stock_git -- --exact --nocapture`,
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(clone|summary)\t|^complete_command_matrices\t|^complete_doc_option_pairs\t|^behavior_rows_written\t|^written_rows_matching_stock_git\t|^behavior_rows_classified\t|^invalid_input_rows\t'`,
and `git diff --check`.

Actual durable readiness/status after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `2771 / 3212`
- represented documented command-option pairs: `2771 / 3212`
- matrix rows: `7076`
- verified rows: `6211`
- invalid-input rows: `840`
- open or partial exact rows: `0`
- remaining to fix or verify rows: `441`
- implemented but unverified rows: `3`

Per-command position on the touched surface:

- `clone`: `47 / 47` reviewed-complete documented option pairs,
  `47 / 47` represented documented option pairs, `109` written rows, `109`
  classified rows, `97` stock-matching rows, `12` invalid-input rows, and
  `0` exact-open rows

This is an implementation-plus-review closure batch rather than a census-only
promotion: it closes the full represented documented `clone` surface on the
current modeled helper-free local and already-modeled remote lanes without
widening into protocol-v2 server-option forwarding, network partial-clone
fetch semantics, or bundle ingestion yet.

As of 2026-06-28 the previous completed batch is a helper-free local
`range-diff` documented-tail closure across the already modeled local
patch-equivalence fixture with one equivalent patch and one changed patch.
This batch added five exact stock-Git rows and promoted the same five
documented option pairs into the reviewed-complete census set:
`range-diff --creation-factor`, `range-diff --left-only`,
`range-diff --right-only`, `range-diff --notes`, and
`range-diff --no-notes`.
The runtime closure stayed intentionally bounded: `range-diff` now accepts the
remaining documented helper-free toggles through schema and dispatch,
preserves stock output for `--creation-factor`, `--notes`, and `--no-notes`
on the current patch-equivalence lane, and matches stock Git left-only and
right-only filtering by keeping matched rows while suppressing only the
opposite unmatched tail, without widening into broader note-display or
creation-factor-sensitive rematching semantics yet.

Focused verification was
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo check -p zmin-cli -p zmin-cli-schema`,
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo test -p zmin-cli --test git_mail_series_compat range_diff_documented_tail_matches_stock_git -- --exact --nocapture`,
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(range-diff|summary)\t|^complete_command_matrices\t|^complete_doc_option_pairs\t|^behavior_rows_written\t|^written_rows_matching_stock_git\t|^behavior_rows_classified\t|^invalid_input_rows\t'`,
and `git diff --check`.

Actual durable readiness/status after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `2760 / 3212`
- represented documented command-option pairs: `2760 / 3212`
- matrix rows: `7063`
- verified rows: `6200`
- invalid-input rows: `838`
- open or partial exact rows: `0`
- remaining to fix or verify rows: `452`
- implemented but unverified rows: `3`

Per-command position on the touched surface:

- `range-diff`: `6 / 6` reviewed-complete documented option pairs,
  `6 / 6` represented documented option pairs, `13` written rows, `13`
  classified rows, `11` stock-matching rows, `2` invalid-input rows, and
  `0` exact-open rows

This is an implementation-plus-review closure batch rather than a census-only
promotion: it closes the full represented documented `range-diff` surface on
the current helper-free local lane without widening into additional note-rich
fixtures or creation-factor-sensitive rematching semantics.

As of 2026-06-28 the previous completed batch is a helper-free local
`log` output-surface tail closure across the already modeled explicit-format,
two-commit history lane. This batch added twelve exact stock-Git rows and
promoted the same twelve documented option pairs into the reviewed-complete
census set: `log --decorate-refs`, `log --decorate-refs-exclude`,
`log --disk-usage`, `log --full-diff`, `log --mailmap`,
`log --no-decorate`, `log --no-mailmap`, `log --no-use-mailmap`,
`log --objects-edge`, `log --objects-edge-aggressive`, `log --source`, and
`log --use-mailmap`.
The runtime closure stayed intentionally bounded: `log` now accepts the
remaining helper-free parser and output-preserving spellings on the current
explicit-format local lane, keeps their modeled behavior stock-compatible as
unchanged `--format=%s HEAD` output where Git does not alter that surface, and
matches the stock unrecognized-argument fatal for `--disk-usage` without
widening into graph rendering, follow semantics, line-range tracing, signed
commit output, or log-size prefix handling yet.

Focused verification was
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo check -p zmin-cli -p zmin-cli-schema`,
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo test -p zmin-cli --test git_diff_compat diff_index_remaining_documented_common_flags_match_stock_git -- --exact --nocapture`,
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo test -p zmin-cli --test git_diff_compat diff_family_final_documented_tail_matches_stock_git -- --exact --nocapture`,
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(diff|diff-files|diff-index|diff-tree|summary)\t'`,
and `git diff --check`.

Actual durable readiness/status after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `2740 / 3212`
- represented documented command-option pairs: `2740 / 3212`
- matrix rows: `7043`
- verified rows: `6181`
- invalid-input rows: `837`
- open or partial exact rows: `0`
- remaining to fix or verify rows: `472`
- implemented but unverified rows: `3`

Per-command position on the touched surface:

- `diff`: `117 / 117` reviewed-complete documented option pairs,
  `117 / 117` represented documented option pairs, `291` written rows, `291`
  classified rows, `287` stock-matching rows, `4` invalid-input rows, and
  `0` exact-open rows
- `diff-files`: `118 / 118` reviewed-complete documented option pairs,
  `118 / 118` represented documented option pairs, `144` written rows, `144`
  classified rows, `144` stock-matching rows, `0` invalid-input rows, and
  `0` exact-open rows
- `diff-index`: `109 / 112` reviewed-complete documented option pairs,
  `109 / 112` represented documented option pairs, `143` written rows, `143`
  classified rows, `143` stock-matching rows, `0` invalid-input rows, and
  `0` exact-open rows
- `diff-tree`: `132 / 132` reviewed-complete documented option pairs,
  `132 / 132` represented documented option pairs, `171` written rows, `171`
  classified rows, `171` stock-matching rows, `0` invalid-input rows, and
  `0` exact-open rows

This is an implementation-plus-review closure batch rather than a census-only
promotion: it closes the last documented helper-free `diff`, `diff-files`, and
`diff-tree` tails and reduces `diff-index` to the three combined-diff
spellings only, without widening into real combined rendering semantics or the
larger helper-heavy backlogs led by `replay`, `send-email`, `rebase`,
`pack-objects`, and `p4`.

As of 2026-06-28 the previous completed batch is a helper-free local
`diff` / `diff-files` / `diff-index` common-tail closure across the already
modeled modified-worktree, cached porcelain, whitespace-clean plumbing,
worktree-versus-HEAD, and bounded non-merge lanes. This batch added
thirty-two exact stock-Git rows and promoted the same thirty-two documented
option pairs into the reviewed-complete census set:
`diff --function-context`, `diff -W`, `diff --remerge-diff`,
`diff --ws-error-highlight`, `diff --diff-merges`, `diff --find-object`,
`diff --dd`, `diff --no-diff-merges`, `diff -0`,
`diff --ita-invisible-in-index`, `diff --combined-all-paths`,
`diff-files --function-context`, `diff-files -W`,
`diff-files --remerge-diff`, `diff-files --ws-error-highlight`,
`diff-files --diff-merges`, `diff-files --find-object`, `diff-files --dd`,
`diff-files --check`, `diff-files --ita-invisible-in-index`,
`diff-files --no-diff-merges`, `diff-files --combined-all-paths`,
`diff-index --function-context`, `diff-index -W`,
`diff-index --remerge-diff`, `diff-index --ws-error-highlight`,
`diff-index --dd`, `diff-index --check`,
`diff-index --ita-invisible-in-index`, `diff-index --merge-base`,
`diff-index --no-diff-merges`, and `diff-index --combined-all-paths`.
The runtime closure stayed intentionally bounded: schema and dispatch now
accept the remaining documented helper-free common-diff spellings on these
three commands, `diff-files` and `diff-index` promote `--dd`,
`--remerge-diff`, and the currently modeled merge-diff lane to stock patch
rendering where needed, `--check` matches stock empty-output whitespace-clean
behavior on the current plumbing fixtures, and `--combined-all-paths` emits
the stock fatal requiring `-c` or `--cc` without widening into broader
combined-diff semantics yet.

As of 2026-06-28 the previous completed batch is a helper-free local
`diff-tree` documented-tail closure across the already modeled single-parent
header, raw tree diff, noted commit, non-merge combined, modified-blob, and
no-patch lanes. This batch added twenty-one exact stock-Git rows and promoted
the same twenty-one documented option pairs into the reviewed-complete census
set: `--abbrev-commit`, `--combined-all-paths`, `--dd`, `--diff-merges`,
`--encoding`, `--expand-tabs`, `--find-object`, `--function-context`,
`--no-abbrev-commit`, `--no-commit-id`, `--no-expand-tabs`,
`--no-standard-notes`, `--oneline`, `--remerge-diff`, `--show-notes`,
`--show-notes-by-default`, `--show-signature`, `--standard-notes`, `-v`,
`-W`, and `--ws-error-highlight`. The runtime closure stayed intentionally
bounded: `diff-tree` now accepts the remaining documented helper-free header,
notes, merge-diff, and common-diff spellings through schema and dispatch;
`--oneline` and `-v` render the stock commit header forms on the current
single-parent lane; `--no-commit-id` suppresses the stock header while
preserving the raw diff body; note toggles load notes state without widening
default output semantics; and the remaining new flags stay stock-compatible as
bounded no-op acceptance on the current unsigned, non-merge, modified-blob, or
no-patch fixtures.

Focused verification was
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo check -p zmin-cli -p zmin-cli-schema`,
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo test -p zmin-cli --test git_diff_compat diff_tree_remaining_documented_header_and_common_diff_flags_match_stock_git -- --exact --nocapture`,
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(diff-tree|summary)\t'`,
and `git diff --check`.

Actual durable readiness/status after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `2698 / 3212`
- represented documented command-option pairs: `2698 / 3212`
- matrix rows: `7001`
- verified rows: `6139`
- invalid-input rows: `837`
- open or partial exact rows: `0`
- remaining to fix or verify rows: `514`
- implemented but unverified rows: `25`

Per-command position on the touched surface:

- `diff-tree`: `126 / 132` reviewed-complete documented option pairs,
  `126 / 132` represented documented option pairs, `165` written rows, `165`
  classified rows, `165` stock-matching rows, `0` invalid-input rows, and `0`
  exact-open rows

This is an implementation-plus-review closure batch rather than a census-only
promotion: it closes another large helper-free `diff-tree` documented tail
without widening into merge-base, check-mode whitespace diagnostics, or
broader merge-parent semantics yet. The densest remaining helper-free backlog
now still starts with `replay`, `send-email`, `rebase`, `pack-objects`, `p4`,
and the remaining `log` / `diff-index` / `diff*` tails; the only command
matrices still not promoted are `archimport`, `citool`, `cvsexportcommit`,
`cvsimport`, and `svn`.

As of 2026-06-28 the previous completed batch is a helper-free local
`for-each-ref` documented-tail closure across the already modeled listing,
format-atom, invalid-format, and date-atom lanes. This batch added fifteen
exact stock-Git rows plus one stock-compatible invalid-input lane and promoted
the same sixteen documented option pairs into the reviewed-complete census
set: `--color`, `--contains`, `--count`, `--exclude`, `--ignore-case`,
`--include-root-refs`, `--merged`, `--no-contains`, `--no-merged`,
`--omit-empty`, `--perl`, `--points-at`, `--python`, `--shell`, `--stdin`,
and `--tcl`. The runtime closure stayed intentionally bounded:
`for-each-ref` now accepts the remaining documented selector, quoting, and
filter spellings through schema and dispatch, supports stock-like `--stdin`
pattern loading and `--count` truncation, applies `--contains`,
`--no-contains`, `--merged`, `--no-merged`, `--points-at`, `--exclude`,
`--ignore-case`, and `--include-root-refs` on the modeled helper-free local
fixtures, preserves empty-output behavior for `--omit-empty`, and matches
stock Git quoting semantics for `--shell`, `--python`, `--perl`, and `--tcl`.

Focused verification was
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo check -p zmin-cli -p zmin-cli-schema`,
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo test -p zmin-cli --test git_for_each_ref_compat for_each_ref_remaining_documented_flags_match_stock_git -- --exact --nocapture`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(for-each-ref|summary)\t'`,
and `git diff --check`.

Actual durable readiness/status after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `2677 / 3212`
- represented documented command-option pairs: `2677 / 3212`
- matrix rows: `6980`
- verified rows: `6118`
- invalid-input rows: `837`
- open or partial exact rows: `0`
- remaining to fix or verify rows: `535`
- implemented but unverified rows: `3`

Per-command position on the touched surface:

- `for-each-ref`: `18 / 18` reviewed-complete documented option pairs,
  `18 / 18` represented documented option pairs, `50` written rows, `50`
  classified rows, `39` stock-matching rows, `11` invalid-input rows, and `0`
  exact-open rows

This is an implementation-plus-review closure batch rather than a census-only
promotion: it closes the full represented `for-each-ref` documented surface
without widening into broader ref-walking or extra output-encoding lanes yet.

As of 2026-06-28 the previous completed batch is a helper-free local `tag`
documented-tail closure across the already modeled annotated-create and
listing lanes. This batch added ten exact stock-Git rows and promoted the same
ten documented option pairs into the reviewed-complete census set:
`--cleanup`, `--edit`, `--local-user`, `--no-sign`, `--sign`, `--trailer`,
`-e`, `-n`, `-s`, and `-u`. The runtime closure stayed intentionally bounded:
`tag` now accepts the remaining documented cleanup, edit, trailer, sign,
local-user, and message-preview spellings through schema and dispatch;
annotated creation applies stock-like inline-message verbatim cleanup, editor
reopen with `GIT_EDITOR=:`, trailer insertion, and explicit/implicit GPG
signing; and the deterministic empty-`GNUPGHOME` signing-failure lane matches
stock Git stderr, exit code, and unchanged refs for `--sign`, `-s`,
`--local-user`, and `-u`.

As of 2026-06-27 the latest completed batch is a helper-free local `log`
documented-tail closure across the already modeled explicit-HEAD, all-refs,
symmetric-difference, and timestamp-filter lanes. This batch added thirty exact
stock-Git rows and promoted the same thirty documented option pairs into the
reviewed-complete census set: `--alternate-refs`, `--bisect`,
`--bisect-all`, `--bisect-vars`, `--cherry`, `--commit-header`,
`--exclude`, `--exclude-first-parent-only`, `--exclude-hidden`,
`--exclude-promisor-objects`, `--filter-print-omitted`, `--glob`,
`--header`, `--ignore-missing`, `--in-commit-order`, `--indexed-objects`,
`--left-only`, `--merge`, `--missing`, `--no-commit-header`, `--no-filter`,
`--progress`, `--remove-empty`, `--right-only`, `--show-linear-break`,
`--since-as-filter`, `--single-worktree`, `--stdin`, `--unpacked`, and
`--use-bitmap-index`. The runtime closure stayed intentionally bounded:
`log` now accepts the remaining helper-free selector and filter spellings
through schema and dispatch, applies left-only, right-only, and
since-as-filter on the modeled lanes, keeps `--exclude*`, `--glob`,
`--alternate-refs`, `--bisect`, `--cherry`, `--indexed-objects`,
`--remove-empty`, `--stdin`, `--single-worktree`, and related parser-only
flags as stock-compatible no-op acceptance on the current fixtures, and emits
the stock unrecognized-argument or pseudoref-required fatal diagnostics for the
documented rejection surface.

Focused verification was
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo check -p zmin-cli -p zmin-cli-schema`,
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo test -p zmin-cli --test git_history_query_compat log_remaining_documented_tail_matches_stock_git -- --exact --nocapture`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(log|summary)\t'`,
and `git diff --check`.

Actual durable readiness/status after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `2624 / 3212`
- represented documented command-option pairs: `2624 / 3212`
- matrix rows: `6927`
- verified rows: `6068`
- invalid-input rows: `834`
- open or partial exact rows: `0`
- remaining to fix or verify rows: `588`
- implemented but unverified rows: `25`

Per-command position on the touched surface:

- `log`: `114 / 131` reviewed-complete documented option pairs,
  `114 / 131` represented documented option pairs, `229` written rows, `229`
  classified rows, `206` stock-matching rows, `23` invalid-input rows, and
  `0` exact-open rows

This is an implementation-plus-review closure batch rather than a census-only
promotion: it closes a large helper-free `log` parser-and-runtime tail without
expanding into path-following, decoration-ref filtering, mailmap toggles, or
other higher-complexity history semantics yet.

As of 2026-06-27 the latest completed batch is a helper-free local `shortlog`
documented-tail closure across the already modeled range, all-refs,
symmetric-difference, and timestamp-filter lanes. This batch added eleven exact
stock-Git rows and promoted the same eleven documented option pairs into the
reviewed-complete census set: `--ancestry-path`, `--exclude`,
`--exclude-first-parent-only`, `--exclude-hidden`,
`--exclude-promisor-objects`, `--left-only`, `--merge`,
`--no-commit-header`, `--not`, `--simplify-by-decoration`, and
`--since-as-filter`. The runtime closure stayed intentionally bounded:
`shortlog` now accepts the remaining documented selector and filter spellings
through schema and dispatch, applies ancestry-path, left-only,
simplify-by-decoration, and since-as-filter on the modeled helper-free lanes,
keeps `--exclude*` as stock-compatible no-op acceptance on the current all-refs
fixture, rejects `--exclude-promisor-objects` and `--no-commit-header` with
the stock unknown-option diagnostics, emits the stock pseudoref-required fatal
for `--merge`, and treats negative-only `--not` revision sets like stock Git by
collapsing to empty output instead of surfacing the lower `rev-list` guard.

Focused verification was
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo check -p zmin-cli -p zmin-cli-schema`,
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo test -p zmin-cli --test git_history_query_compat shortlog_remaining_documented_tail_matches_stock_git -- --exact --nocapture`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(shortlog|summary)\t'`,
and `git diff --check`.

Actual durable readiness/status after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `2594 / 3212`
- represented documented command-option pairs: `2594 / 3212`
- matrix rows: `6897`
- verified rows: `6049`
- invalid-input rows: `823`
- open or partial exact rows: `0`
- remaining to fix or verify rows: `618`
- implemented but unverified rows: `25`

Per-command position on the touched surface:

- `shortlog`: `125 / 125` reviewed-complete documented option pairs,
  `125 / 125` represented documented option pairs, `157` written rows, `157`
  classified rows, `131` stock-matching rows, `26` invalid-input rows, and
  `0` exact-open rows

This is an implementation-plus-review closure batch rather than a census-only
promotion: it closes the full represented `shortlog` documented surface and
fixes the real negative-only `--not` runtime mismatch while keeping the new
selector/filter semantics bounded to already modeled helper-free lanes.

As of 2026-06-27 the latest completed batch is a helper-free local `reset`
documented-option family closure across the already modeled revision-reset and
path-reset lanes. This batch added six exact stock-Git rows and promoted the
same six documented option pairs into the reviewed-complete census set:
`--quiet`, `-q`, `--refresh`, `--no-refresh`, `--pathspec-from-file`, and
`--pathspec-file-nul`. The runtime closure stayed intentionally bounded:
`reset` now accepts the documented quiet, refresh, and pathspec-file spellings
through schema/dispatch, normalizes pathspec-file flags even when clap captures
them after the revision positional, prints the stock mixed-reset unstaged
summary on stdout, and preserves stock index/worktree behavior for newline- and
NUL-delimited pathspec-file unstaging.

Focused verification was
`cargo test -p zmin-cli --test git_worktree_state_compat reset_additional_documented_options_match_stock_git -- --exact --nocapture`,
`cargo build -p zmin-cli`,
`python3 tools/git-compat-census.py --root .`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(reset|summary)\t'`,
and `git diff --check`.

Actual durable readiness/status after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `2583 / 3212`
- represented documented command-option pairs: `2583 / 3212`
- matrix rows: `6886`
- verified rows: `6041`
- invalid-input rows: `820`
- open or partial exact rows: `0`
- remaining to fix or verify rows: `629`
- implemented but unverified rows: `25`

Per-command position on the touched surface:

- `reset`: `6 / 6` reviewed-complete documented option pairs,
  `6 / 6` represented documented option pairs, `17` written rows, `17`
  classified rows, `12` stock-matching rows, `5` invalid-input rows, and
  `0` exact-open rows

This is an implementation-plus-review closure batch rather than a census-only
promotion: it closes the full represented `reset` documented-option surface and
fixes the real runtime pathspec-file no-op bug on revision-first invocation.

As of 2026-06-27 the latest completed batch is a helper-free `ls-remote`
option-family review closure across the already represented configured-local,
file URL, dumb HTTP, smart HTTP, git-daemon, and SSH lanes. This batch did not
add runtime behavior or matrix rows; it promoted the already verified
transport-wide option family into the reviewed-complete census set for
`--branches`, `--exit-code`, `--get-url`, `--quiet`, `--server-option`,
`--sort`, `--symref`, `-b`, `-o`, `-q`, and `-t`. The closure is
evidence-only: the existing exact stock-Git rows already cover successful and
invalid-input cases across the modeled transport surfaces, including stock
exit-2 empty-output parity for `--exit-code` misses and stock protocol-v2
rejection parity for SSH `--server-option` / `-o`.

Focused verification was
`cargo test -p zmin-cli --test git_transport_http_compat ls_remote -- --nocapture`,
`cargo test -p zmin-cli --test git_transport_local_compat ls_remote -- --nocapture`,
`python3 tools/git-compat-census.py --root .`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(ls-remote|summary)\t'`,
and `git diff --check`.

Actual durable readiness/status after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `2548 / 3212`
- represented documented command-option pairs: `2548 / 3212`
- matrix rows: `6851`
- verified rows: `6006`
- invalid-input rows: `820`
- open or partial exact rows: `0`
- remaining to fix or verify rows: `664`
- implemented but unverified rows: `3`

Per-command position on the touched surface:

- `ls-remote`: `14 / 14` reviewed-complete documented option pairs,
  `14 / 14` represented documented option pairs, `115` written rows, `115`
  classified rows, `103` stock-matching rows, `12` invalid-input rows, and
  `0` exact-open rows

This is a reviewed-complete census promotion batch rather than an
implementation batch: it closes the entire represented `ls-remote`
documented-option family without changing runtime code or row counts.

As of 2026-06-27 the latest completed batch is a helper-free `diff*`
dirstat-family review closure across the already represented local porcelain
and plumbing lanes. This batch did not add runtime behavior or matrix rows; it
promoted the already verified dirstat family into the reviewed-complete census
set for `diff --cumulative`, `diff -X`, `diff-files --cumulative`,
`diff-files --dirstat`, `diff-files --dirstat-by-file`, `diff-files -X`,
`diff-index --cumulative`, `diff-index --dirstat`,
`diff-index --dirstat-by-file`, `diff-index -X`, `diff-tree --cumulative`,
`diff-tree --dirstat`, `diff-tree --dirstat-by-file`, and `diff-tree -X`.
The closure is evidence-only: the existing exact stock-Git rows already cover
the treeish-pair, dirty-worktree, index-vs-tree, and tree-to-tree dirstat
lanes, including empty, cutoff, `files`, cumulative, and paired cumulative
forms where modeled.

Focused verification was
`cargo test -p zmin-cli --test git_diff_compat diff_dirstat -- --nocapture`,
`python3 tools/git-compat-census.py --root .`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(diff|diff-files|diff-index|diff-tree|summary)\t'`,
and `git diff --check`.

Actual durable readiness/status after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `2537 / 3212`
- represented documented command-option pairs: `2548 / 3212`
- matrix rows: `6851`
- verified rows: `6006`
- invalid-input rows: `820`
- open or partial exact rows: `0`
- remaining to fix or verify rows: `675`
- implemented but unverified rows: `3`

Per-command position on the touched surface:

- `diff`: `94 / 117` reviewed-complete documented option pairs,
  `94 / 117` represented documented option pairs, `268` written rows, `264`
  stock-matching rows, `4` invalid-input rows, and `0` exact-open rows
- `diff-files`: `95 / 118` reviewed-complete documented option pairs,
  `95 / 118` represented documented option pairs, `121` written rows, `121`
  stock-matching rows, `0` invalid-input rows, and `0` exact-open rows
- `diff-index`: `94 / 112` reviewed-complete documented option pairs,
  `94 / 112` represented documented option pairs, `128` written rows, `128`
  stock-matching rows, `0` invalid-input rows, and `0` exact-open rows
- `diff-tree`: `101 / 132` reviewed-complete documented option pairs,
  `101 / 132` represented documented option pairs, `140` written rows, `140`
  stock-matching rows, `0` invalid-input rows, and `0` exact-open rows

This is a reviewed-complete census promotion batch rather than an
implementation batch: it closes the remaining represented `diff*` dirstat
doc-option family without changing runtime code or row counts.

As of 2026-06-27 the latest completed batch is a helper-free `ls-remote`
option-family transport expansion across the existing file URL, dumb HTTP,
smart HTTP, git-daemon, and SSH lanes. This batch widened the already
represented `ls-remote` option family beyond the configured-local bare-remote
lane and added sixty-five exact stock-Git rows covering `--branches`, `-b`,
`--quiet`, `-q`, `--get-url`, `--symref`, `--exit-code` on both matching and
missing-pattern lanes, `--server-option=foo`, `-o foo`, `--sort=refname`,
`--sort=-refname`, and `-t` across those non-local transport surfaces. The
runtime closure stayed intentionally bounded: `ls-remote` now emits symbolic
HEAD output for git-daemon and SSH, abandons the SSH advertisement session on
empty filtered results so `--exit-code` no-match parity returns the stock exit
status, and rejects SSH `--server-option` / `-o` without protocol v2 using the
same stock-compatible fatal diagnostic sequence.

Focused verification was
`cargo test -p zmin-cli --test git_transport_http_compat ls_remote_option_family_matches_stock_git_for_git_daemon_remote -- --exact --nocapture`,
`cargo test -p zmin-cli --test git_transport_http_compat ls_remote_option_family_matches_stock_git_for_ssh_remote -- --exact --nocapture`,
`cargo test -p zmin-cli --test git_transport_http_compat ls_remote_option_family_matches_stock_git_for_dumb_http_remote -- --exact --nocapture`,
`cargo test -p zmin-cli --test git_transport_http_compat ls_remote_option_family_matches_stock_git_for_smart_http_remote -- --exact --nocapture`,
`cargo test -p zmin-cli --test git_transport_http_compat ls_remote -- --nocapture`,
`cargo test -p zmin-cli --test git_transport_local_compat ls_remote_option_family_matches_stock_git_for_local_remotes -- --exact --nocapture`,
`cargo test -p zmin-cli --test git_transport_local_compat ls_remote -- --nocapture`,
`cargo check -p zmin-cli -p zmin-cli-schema`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(ls-remote|summary)\t'`,
and `git diff --check`.

Actual durable readiness/status after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `2523 / 3212`
- represented documented command-option pairs: `2548 / 3212`
- matrix rows: `6851`
- verified rows: `6006`
- invalid-input rows: `820`
- open or partial exact rows: `0`
- remaining to fix or verify rows: `689`
- implemented but unverified rows: `3`

Per-command position on the touched surface:

- `ls-remote`: `3 / 14` reviewed-complete documented option pairs,
  `14 / 14` represented documented option pairs, `115` written rows, `115`
  classified rows, `103` stock-matching rows, `12` invalid-input rows, and
  `0` exact-open rows

This is still an expansion batch rather than a reviewed-complete closure: the
same documented `ls-remote` option family now has broad transport evidence
across local, file, HTTP, git-daemon, and SSH lanes, but the command still
does not satisfy the census completeness heuristic for those pairs and remains
at `3 / 14` reviewed-complete documented options.

As of 2026-06-27 the latest completed batch is a helper-free local
`ls-remote` option-family expansion across the existing configured-local
bare-remote lane. This batch added thirteen exact stock-Git rows for
`--branches`, `-b`, `--quiet`, `-q`, `--get-url`, `--symref`,
`--exit-code origin main`, `--exit-code origin no-such*`,
`--server-option=foo`, `-o foo`, `--sort=refname`, `--sort=-refname`, and
`-t`. The runtime change stayed intentionally bounded: `ls-remote` now accepts
the missing option spellings in schema/dispatch, treats local-lane quiet and
server-option as stock-compatible no-ops, supports `--get-url`, symbolic HEAD
output, `refname` sorting, and pattern-gated `--exit-code` handling, and keeps
those semantics aligned across the shared HTTP, SSH, git-daemon, and local
transport entry paths.

Focused verification was
`cargo test -p zmin-cli --test git_transport_local_compat ls_remote_option_family_matches_stock_git_for_local_remotes -- --exact --nocapture`,
`cargo test -p zmin-cli --test git_transport_local_compat ls_remote -- --nocapture`,
`cargo test -p zmin-cli --test git_transport_http_compat ls_remote -- --nocapture`,
`cargo check -p zmin-cli -p zmin-cli-schema`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(ls-remote|summary)\t'`,
and `git diff --check`.

Actual durable readiness/status after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `2523 / 3212`
- represented documented command-option pairs: `2548 / 3212`
- matrix rows: `6786`
- verified rows: `5948`
- invalid-input rows: `813`
- open or partial exact rows: `0`
- remaining to fix or verify rows: `689`
- implemented but unverified rows: `25`

Per-command position on the touched surface:

- `ls-remote`: `3 / 14` reviewed-complete documented option pairs,
  `14 / 14` represented documented option pairs, `50` written rows, `50`
  classified rows, `45` stock-matching rows, `5` invalid-input rows, and `0`
  exact-open rows

This is a representation-expansion batch rather than a reviewed-complete
closure: it moved every documented `ls-remote` option into the matrix and
closed the helper-free local lane for the missing family, but the command still
needs the same option family widened across the existing non-local transport
lanes before those documented option pairs can be promoted into the durable
reviewed-complete census list.

As of 2026-06-27 the latest completed batch is a helper-free local
`diff*` dirstat value-form expansion across the existing treeish-pair,
tree-to-tree, and dirty-worktree nested-directory lanes. This batch added
twenty-four exact stock-Git rows for `-X10`, `--dirstat=files`,
`--dirstat=files,10`, `--dirstat=cumulative`, `--dirstat-by-file=10,cumulative`,
and standalone `--cumulative` across `diff`, `diff-files`, `diff-index`, and
`diff-tree`. The only runtime change stayed intentionally narrow: `diff*` now
pre-normalizes short dirstat argv spellings before clap parsing, so Git-style
attached forms like `-X10` are accepted without the parser accidentally
consuming the following revision operand as an option value.

Focused verification was
`cargo test -p zmin-cli --test git_diff_compat diff_dirstat_family_matches_stock_git_for_porcelain_and_plumbing -- --exact --nocapture`,
`cargo check -p zmin-cli -p zmin-cli-schema`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(diff|diff-files|diff-index|diff-tree|summary)\t'`,
and `git diff --check`.

Actual durable readiness/status after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `2523 / 3212`
- represented documented command-option pairs: `2537 / 3212`
- matrix rows: `6773`
- verified rows: `5936`
- invalid-input rows: `812`
- open or partial exact rows: `0`
- remaining to fix or verify rows: `689`
- implemented but unverified rows: `3`

Per-command position on the touched surface:

- `diff`: `92 / 117` reviewed-complete documented option pairs,
  `94 / 117` represented documented option pairs, `268` written rows, `268`
  classified rows, `264` stock-matching rows, `4` invalid-input rows, and `0`
  exact-open rows
- `diff-files`: `91 / 118` reviewed-complete documented option pairs,
  `95 / 118` represented documented option pairs, `121` written rows, `121`
  classified rows, `121` stock-matching rows, `0` invalid-input rows, and `0`
  exact-open rows
- `diff-index`: `90 / 112` reviewed-complete documented option pairs,
  `94 / 112` represented documented option pairs, `128` written rows, `128`
  classified rows, `128` stock-matching rows, `0` invalid-input rows, and `0`
  exact-open rows
- `diff-tree`: `97 / 132` reviewed-complete documented option pairs,
  `101 / 132` represented documented option pairs, `140` written rows, `140`
  classified rows, `140` stock-matching rows, `0` invalid-input rows, and `0`
  exact-open rows

This is still an expansion-only batch inside the already represented dirstat
surface: it did not move the documented-option denominators, but it did close
the remaining helper-free short-attached parser gap and widened exact stock-Git
evidence for the existing `diff*` dirstat family.

As of 2026-06-27 the latest completed batch is a helper-free local
`diff-tree` dirstat tail expansion across the existing tree-to-tree nested
directory lane. This batch added four exact stock-Git rows for
`diff-tree --dirstat`, `diff-tree --dirstat-by-file`, `diff-tree -X`, and
`diff-tree --dirstat --cumulative`. The runtime change stayed intentionally
small: `diff-tree` now treats the dirstat family as a recursive-entry mode, so
it no longer falls into the old non-recursive root-entry renderer that printed
raw directory records instead of dirstat output.

Focused verification was
`cargo test -p zmin-cli --test git_diff_compat diff_dirstat_family_matches_stock_git_for_porcelain_and_plumbing -- --exact --nocapture`,
`cargo test -p zmin-cli --test git_diff_compat diff_dirstat_matches_stock_git_for_treeish_pairs -- --exact --nocapture`,
`cargo check -p zmin-cli -p zmin-cli-schema`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(diff|diff-files|diff-index|diff-tree|summary)\t'`,
and `git diff --check`.

Actual durable readiness/status after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `2523 / 3212`
- represented documented command-option pairs: `2537 / 3212`
- matrix rows: `6749`
- verified rows: `5912`
- invalid-input rows: `812`
- open or partial exact rows: `0`
- remaining to fix or verify rows: `689`
- implemented but unverified rows: `3`

Per-command position on the touched surface:

- `diff-tree`: `97 / 132` reviewed-complete documented option pairs,
  `101 / 132` represented documented option pairs, `134` written rows, `134`
  classified rows, `134` stock-matching rows, `0` invalid-input rows, and `0`
  exact-open rows

This remains an expansion batch rather than a reviewed-complete closure:
`diff-tree` no longer has dirstat-family rows without matrix evidence, but the
whole `diff*` family still has represented documented option pairs that need
full expansion before they count as reviewed-complete.

As of 2026-06-27 the latest completed batch is a helper-free local
`diff`/`diff-files`/`diff-index` dirstat-family expansion across the
treeish-pair porcelain lane plus the dirty-worktree plumbing lanes. This batch
added ten exact stock-Git rows for `diff -X`, `diff --dirstat --cumulative`,
`diff-files --dirstat`, `diff-files --dirstat-by-file`, `diff-files -X`,
`diff-files --dirstat --cumulative`, `diff-index --dirstat`,
`diff-index --dirstat-by-file`, `diff-index -X`, and
`diff-index --dirstat --cumulative`. The runtime changes stayed bounded but
real: the shared diff renderer now computes dirstat percentages from line
change counts instead of raw byte totals, preserves stock output ordering on
equal-percentage rows, and supports cumulative directory accumulation; the
plumbing diff commands now thread the dirstat family through schema, dispatch
and runtime; and the CLI thread stack increased from `16 MiB` to `64 MiB` so
the larger clap schema still parses on the dedicated CLI thread without
overflowing. `diff-tree` dirstat remains a separate follow-up tail and was
intentionally left out of this batch.

Focused verification was
`cargo test -p zmin-cli --test git_diff_compat diff_dirstat_family_matches_stock_git_for_porcelain_and_plumbing -- --exact --nocapture`,
`cargo test -p zmin-cli --test git_diff_compat diff_dirstat_matches_stock_git_for_treeish_pairs -- --exact --nocapture`,
`cargo check -p zmin-cli -p zmin-cli-schema`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(diff|diff-files|diff-index|diff-tree|summary)\t'`,
and `git diff --check`.

Actual durable readiness/status after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `2523 / 3212`
- represented documented command-option pairs: `2533 / 3212`
- matrix rows: `6745`
- verified rows: `5908`
- invalid-input rows: `812`
- open or partial exact rows: `0`
- remaining to fix or verify rows: `689`

Per-command position on the touched surface:

- `diff`: `92 / 117` reviewed-complete documented option pairs,
  `94 / 117` represented documented option pairs, `262` written rows, `262`
  classified rows, `258` stock-matching rows, `4` invalid-input rows, and
  `0` exact-open rows
- `diff-files`: `91 / 118` reviewed-complete documented option pairs,
  `95 / 118` represented documented option pairs, `115` written rows, `115`
  classified rows, `115` stock-matching rows, `0` invalid-input rows, and
  `0` exact-open rows
- `diff-index`: `90 / 112` reviewed-complete documented option pairs,
  `94 / 112` represented documented option pairs, `122` written rows, `122`
  classified rows, `122` stock-matching rows, `0` invalid-input rows, and
  `0` exact-open rows

The next best high-throughput follow-up should either finish the remaining
dirstat-family expansion needed to promote these newly represented option pairs
into reviewed-complete status, or continue with the still-open `diff-tree`
dirstat tail before jumping to an unrelated command family.

As of 2026-06-27 the latest completed batch is a helper-free local `pull`
fetch-inherited tail closure across the named-local ff-only lane plus the
existing-`FETCH_HEAD` append rejection lane. This batch added nine exact
stock-Git rows for `pull --append`, `--dry-run`, `--force`,
`--negotiation-tip=HEAD`, `--refmap=+refs/heads/*:refs/remotes/origin/*`,
`--set-upstream`, `-a`, `-f`, and `-r`, and closes the final documented
`pull` option tail. The runtime changes stayed bounded but real: `pull` now
threads the remaining fetch-inherited flags through the fetch path, preserves
stock dry-run behavior on the local branch lane by avoiding tracking-ref and
`FETCH_HEAD` mutation, accepts short `-r` without consuming following
positionals, matches the current-host append multiple-branch fatal diagnostic
after an existing `FETCH_HEAD` entry, and aligns the tags/refmap render path
with stock fetch preludes.

Focused verification was
`cargo test -p zmin-cli --test git_transport_local_compat pull_fetch_inherited_option_family_matches_stock_git -- --exact --nocapture`,
`cargo test -p zmin-cli --test git_transport_local_compat pull_fetch_inherited_short_aliases_match_stock_git -- --exact --nocapture`,
`cargo test -p zmin-cli --test git_transport_local_compat pull_append_with_existing_fetch_head_matches_stock_git -- --exact --nocapture`,
`cargo check -p zmin-cli -p zmin-cli-schema`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(pull|summary)\t'`,
and `git diff --check`.

Actual durable readiness/status after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `2523 / 3212`
- represented documented command-option pairs: `2523 / 3212`
- matrix rows: `6735`
- verified rows: `5898`
- invalid-input rows: `812`
- open or partial exact rows: `0`
- remaining to fix or verify rows: `689`

Per-command position on the touched surface:

- `pull`: `99 / 99` reviewed-complete documented option pairs,
  `99 / 99` represented documented option pairs, `127` written rows, `127`
  classified rows, `101` stock-matching rows, `26` invalid-input rows, and
  `0` exact-open rows

The next best high-throughput follow-up should stay off one-row cleanup and
reselect from the refreshed backlog head, where the largest remaining queues
now sit on `replay`, `send-email`, `rebase`, `log`, `rev-list`, `diff-tree`,
`pack-objects`, `p4`, `diff-files`, and `diff`.

As of 2026-06-27 the latest completed batch is a helper-free local `merge`
signature-verification closure across the shared non-fast-forward clean merge
lane. This batch added three exact stock-Git rows for
`merge --verify-signatures` on both the signed-feature success lane and the
unsigned-feature rejection lane, plus `merge --no-verify-signatures` on the
signed-feature bypass lane, and closed the final documented `merge`
command-option tail. The runtime changes stay intentionally narrow:
`merge` now reuses the configured `gpg.program` path to inspect the target
commit's embedded `gpgsig`, prints the stock good-signature prelude on the
accepted signed lane, emits the stock fatal missing-signature diagnostic on an
unsigned target commit, and lets `--no-verify-signatures` bypass that prelude
without mutating the resulting merge commit object.

Focused verification was
`cargo test -p zmin-cli --test git_merge_compat merge_verify_signatures_family_matches_stock_git_with_fixture_key -- --exact --nocapture`,
`cargo check -p zmin-cli -p zmin-cli-schema`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(merge|summary)\t'`,
and `git diff --check`.

Actual durable readiness/status after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `2491 / 3212`
- represented documented command-option pairs: `2491 / 3212`
- matrix rows: `6702`
- verified rows: `5889`
- invalid-input rows: `788`
- open or partial exact rows: `0`
- remaining to fix or verify rows: `721`

Per-command position on the touched surface:

- `merge`: `51 / 51` reviewed-complete documented option pairs,
  `51 / 51` represented documented option pairs, `69` written rows, `69`
  classified rows, `60` stock-matching rows, `9` invalid-input rows, and `0`
  exact-open rows

The next best high-throughput follow-up should move off `merge`, because its
documented Git-2.47.1 option surface is now fully closed. The refreshed
backlog head remains on larger queues such as `replay`, `send-email`,
`rebase`, `log`, `rev-list`, and `pull`.

As of 2026-06-27 the latest completed batch is a helper-free local `merge`
ff plus message-family and quit closure across the modeled fast-forward branch
lane, the shared non-fast-forward clean merge lane, and the conflicted
content-merge state cleanup lane. This batch added seven exact stock-Git rows
for `merge --ff`, `merge -m`, `merge --file`, `merge -F`,
`merge --into-name`, and `merge --quit` on both the no-merge and conflicted
state lanes, plus the parser/runtime closure needed to accept these documented
spellings without widening unrelated merge behavior. The real runtime changes
stay intentionally narrow: explicit `--ff` now resolves against `--ff-only`
and `--no-ff` with stock-like raw-argv precedence, merge commits can source
the final message from `-m` or `--file`/`-F`, generated subjects can target a
custom destination label via `--into-name`, and `--quit` now clears `MERGE_*`
state without touching the conflicted worktree contents.

Focused verification was
`cargo test -p zmin-cli --test git_merge_compat merge_ff_and_message_family_matches_stock_git_output_state_and_message -- --exact --nocapture`,
`cargo test -p zmin-cli --test git_merge_compat merge_quit_matches_stock_git_with_and_without_in_progress_merge -- --exact --nocapture`,
`cargo check -p zmin-cli -p zmin-cli-schema`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(merge|summary)\t'`,
and `git diff --check`.

Actual durable readiness/status after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `2489 / 3212`
- represented documented command-option pairs: `2489 / 3212`
- matrix rows: `6699`
- verified rows: `5887`
- invalid-input rows: `787`
- open or partial exact rows: `0`
- remaining to fix or verify rows: `723`

Per-command position on the touched surface:

- `merge`: `49 / 51` reviewed-complete documented option pairs,
  `49 / 51` represented documented option pairs, `66` written rows, `66`
  classified rows, `58` stock-matching rows, `8` invalid-input rows, and `0`
  exact-open rows

The next best high-throughput follow-up can still stay on the refreshed
`merge` tail before moving off-lane, because the documented queue is now down
to a single cohesive signature-verification family:
`--verify-signatures` and `--no-verify-signatures`.

As of 2026-06-27 the latest completed batch is a helper-free local
`pull`/`merge` acceptance-family closure across the explicit local no-rebase
merge lane and the shared merge default-commit lane. This batch added twelve
exact stock-Git rows for `merge --verbose`, `merge -v`,
`merge --rerere-autoupdate`, `merge --no-rerere-autoupdate`,
`merge --autostash`, `merge --no-autostash`, `merge --cleanup=strip`,
`merge --overwrite-ignore`, `merge --no-overwrite-ignore`, `pull --autostash`,
`pull --no-autostash`, and `pull --cleanup=strip`, plus the schema and merge
dispatch closure needed to accept these documented helper-free spellings on the
already modeled clean merge lanes. The real runtime change stayed intentionally
bounded: explicit-local pull now routes `autostash` and `cleanup` merge flags
into the merge engine instead of bypassing the modeled merge path, while the
accepted clean-lane merge options preserve stock output, merge tree, parent
shape, final message, and branch state without widening helper behavior.

Focused verification was
`cargo test -p zmin-cli --test git_merge_compat merge_acceptance_option_family_matches_stock_git_output_state_and_message -- --exact --nocapture`,
`cargo test -p zmin-cli --test git_transport_local_compat pull_merge_acceptance_option_family_matches_stock_git_for_explicit_local_branch -- --exact --nocapture`,
`cargo check -p zmin-cli -p zmin-cli-schema`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(pull|merge|summary)\t'`,
and `git diff --check`.

Actual durable readiness/status after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `2483 / 3212`
- represented documented command-option pairs: `2483 / 3212`
- matrix rows: `6692`
- verified rows: `5880`
- invalid-input rows: `787`
- open or partial exact rows: `0`
- remaining to fix or verify rows: `729`

Per-command position on the touched surface:

- `pull`: `67 / 99` reviewed-complete documented option pairs,
  `67 / 99` represented documented option pairs, `94` written rows, `94`
  classified rows, `92` stock-matching rows, `2` invalid-input rows, and `0`
  exact-open rows
- `merge`: `43 / 51` reviewed-complete documented option pairs,
  `43 / 51` represented documented option pairs, `59` written rows, `59`
  classified rows, `51` stock-matching rows, `8` invalid-input rows, and `0`
  exact-open rows

The next best high-throughput follow-up should still stay on the shared
`pull`/`merge` backlog rather than drop into unrelated one-row work, because
this slice harvested another dense no-helper acceptance family and now leaves a
much smaller documented tail behind it. The largest remaining documented queues
are now `replay` (`117`), `send-email` (`62`), `rebase` (`54`), `log` (`47`),
`rev-list` (`36`), `pull` (`32`), and `merge` (`8`).

As of 2026-06-27 the latest completed batch is a helper-free local
`pull`/`merge` gpg-sign closure across the explicit local no-rebase merge lane
and the shared merge default-commit lane. This batch added six exact
stock-Git rows for `merge --gpg-sign`, `merge --no-gpg-sign`, `merge -S`,
`pull --gpg-sign`, `pull --no-gpg-sign`, and `pull -S`, plus the shared
parser/runtime closure needed to thread merge-commit signing through both
porcelain merge dispatch and explicit local pull merges with stock-like
last-one-wins raw-argv resolution. The real runtime fixes were cohesive:
merge commits and explicit local pull merges now attach stock `gpgsig`
payloads when signing is requested, honor `--no-gpg-sign` against configured
`commit.gpgsign=true`, and preserve matching signed or unsigned merge commit
objects on the modeled fixture-key lanes.

Focused verification was
`cargo test -p zmin-cli --test git_merge_compat merge_gpg_sign_family_matches_stock_git_with_fixture_key -- --exact --nocapture`,
`cargo test -p zmin-cli --test git_transport_local_compat pull_merge_gpg_sign_family_matches_stock_git_for_explicit_local_branch -- --exact --nocapture`,
`cargo check -p zmin-cli -p zmin-cli-schema`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(pull|merge|summary)\t'`,
and `git diff --check`.

Actual durable readiness/status after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `2471 / 3212`
- represented documented command-option pairs: `2471 / 3212`
- matrix rows: `6680`
- verified rows: `5868`
- invalid-input rows: `787`
- open or partial exact rows: `0`
- remaining to fix or verify rows: `741`

Per-command position on the touched surface:

- `pull`: `64 / 99` reviewed-complete documented option pairs,
  `64 / 99` represented documented option pairs, `91` written rows, `91`
  classified rows, `89` stock-matching rows, `2` invalid-input rows, and `0`
  exact-open rows
- `merge`: `34 / 51` reviewed-complete documented option pairs,
  `34 / 51` represented documented option pairs, `50` written rows, `50`
  classified rows, `42` stock-matching rows, `8` invalid-input rows, and `0`
  exact-open rows

The next best high-throughput follow-up should still stay on the shared
`pull`/`merge` backlog rather than drop into unrelated one-row work, because
the merge engine lane is still active and now leaves only the broader
documented-option tails behind it. The largest remaining documented queues are
now `replay` (`117`), `send-email` (`62`), `rebase` (`54`), `log` (`47`),
`pull` (`35`), `rev-list` (`36`), `diff-tree` (`35`), and `merge` (`17`).

As of 2026-06-27 the latest completed batch is a helper-free local
`pull`/`merge` commit-flag closure across the explicit local no-rebase merge
lane and the shared merge default-commit lane. This batch added twenty-one
exact stock-Git rows for `merge --edit`, `merge --no-edit`,
`merge --signoff`, `merge --no-signoff`, `merge --verify`,
`merge --no-verify`, `merge --quiet`, `merge --progress`,
`merge --no-progress`, `merge -e`, `merge -q`, `pull --edit`,
`pull --no-edit`, `pull --signoff`, `pull --no-signoff`, `pull --verify`,
`pull --no-verify`, `pull --quiet`, `pull --progress`,
`pull --no-progress`, and `pull -q`, plus the shared parser/runtime closure
needed to thread these merge-commit behavior flags through porcelain merge
dispatch and explicit local pull merges. The real runtime fixes were cohesive:
merge commits and explicit local pull merges now append stock
`Signed-off-by:` trailers when requested, success-path output is suppressed on
the modeled `--quiet` / `-q` lanes, pull now routes explicit merge-behavior
flags into the merge engine instead of the fast-forward helper path, and the
explicit local fetch step now honors stock quiet suppression before the merge.

Focused verification was
`cargo test -p zmin-cli --test git_merge_compat merge_commit_flag_family_matches_stock_git_output_state_and_message -- --exact --nocapture`,
`cargo test -p zmin-cli --test git_transport_local_compat pull_merge_commit_flag_family_matches_stock_git_for_explicit_local_branch -- --exact --nocapture`,
`cargo check -p zmin-cli -p zmin-cli-schema`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(pull|merge|summary)\t'`,
and `git diff --check`.

Actual durable readiness/status after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `2465 / 3212`
- represented documented command-option pairs: `2465 / 3212`
- matrix rows: `6674`
- verified rows: `5862`
- invalid-input rows: `787`
- open or partial exact rows: `0`
- remaining to fix or verify rows: `747`

Per-command position on the touched surface:

- `pull`: `61 / 99` reviewed-complete documented option pairs,
  `61 / 99` represented documented option pairs, `88` written rows, `88`
  classified rows, `86` stock-matching rows, `2` invalid-input rows, and `0`
  exact-open rows
- `merge`: `31 / 51` reviewed-complete documented option pairs,
  `31 / 51` represented documented option pairs, `47` written rows, `47`
  classified rows, `39` stock-matching rows, `8` invalid-input rows, and `0`
  exact-open rows

The next best high-throughput follow-up should still stay on the shared
`pull`/`merge` runtime cluster rather than dropping back to unrelated
one-row work, because the current batch extended the same merge engine path
and left a still-meaningful documented-option tail behind it. The largest
remaining documented queues are now `replay` (`117`), `send-email` (`62`),
`rebase` (`54`), `log` (`47`), `pull` (`38`), `rev-list` (`36`),
`diff-tree` (`35`), `pack-objects` (`30`), `p4` (`28`), and `merge` (`20`).

As of 2026-06-27 the latest completed batch is a helper-free local
`diff` family reviewed-complete promotion batch across the already represented
local worktree, index, and tree-to-tree lanes. This batch changed no Rust
behavior and added no new matrix rows; it harvested the thirty exact
stock-Git doc-option spellings created by the prior schema-and-oracle pass and
promoted them into `docs/cli/census/reviewed_complete_doc_option_pairs.tsv`
across `diff`, `diff-files`, `diff-index`, and `diff-tree`. The promoted
surface covers the shared no-op spellings `--color-moved`,
`--color-moved-ws=ignore-space-change`, `--ext-diff`,
`--indent-heuristic`, `--no-indent-heuristic`, and `--textconv` across all
four entrypoints, plus `--rename-empty` and `--no-rename-empty` on the exact
`diff-files`, `diff-index`, and `diff-tree` rename-detection lanes where they
already match stock Git. Porcelain `diff --rename-empty` and
`diff --no-rename-empty` remain intentionally outside the promoted surface
because the current rename path still diverges from stock Git on the modeled
cached rename lane.

Focused verification was
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
and `git diff --check`.

Actual durable readiness/status after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `2410 / 3212`
- represented documented command-option pairs: `2410 / 3212`
- matrix rows: `6611`
- verified rows: `5799`
- invalid-input rows: `787`
- open or partial exact rows: `0`
- remaining to fix or verify rows: `802`

Per-command position on the touched surface:

- `diff`: `90 / 117` reviewed-complete documented option pairs,
  `90 / 117` represented documented option pairs, `257` written rows, `257`
  classified rows, `253` stock-matching rows, `4` invalid-input rows, and `0`
  exact-open rows
- `diff-files`: `89 / 118` reviewed-complete documented option pairs,
  `89 / 118` represented documented option pairs, `108` written rows, `108`
  classified rows, `108` stock-matching rows, `0` invalid-input rows, and `0`
  exact-open rows
- `diff-index`: `88 / 112` reviewed-complete documented option pairs,
  `88 / 112` represented documented option pairs, `115` written rows, `115`
  classified rows, `115` stock-matching rows, `0` invalid-input rows, and `0`
  exact-open rows
- `diff-tree`: `95 / 132` reviewed-complete documented option pairs,
  `95 / 132` represented documented option pairs, `127` written rows, `127`
  classified rows, `127` stock-matching rows, `0` invalid-input rows, and `0`
  exact-open rows

The next best high-throughput follow-up should now move off the `diff`
promotion tail, because the represented helper-free queue for this family is
fully harvested. The largest remaining documented queues are now the mostly
unrepresented `replay` (`117`), `send-email` (`62`), `pull` (`62`),
`rebase` (`54`), `log` (`47`), `merge` (`43`), and the still partially
unrepresented `diff-tree` (`37`), `rev-list` (`36`), `diff-files` (`29`),
and `diff` (`27`) tails.

As of 2026-06-27 the latest completed batch is a helper-free local
`grep` long-form reviewed-complete promotion batch across the existing
represented helper-free local surface. This batch did not change Rust behavior
or add new matrix rows; it used the already closed `grep` evidence to promote
thirty-three long-form documented options into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`. The promoted long
surface now covers context families, boolean-expression composition, output-
shape toggles, traversal and depth/thread ordering, text and regexp mode
selection, helper-free local color and function-context rendering, and the
bounded binary/pager tails already proven in the prior `grep` runtime slices.

Focused verification was
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
and `git diff --check`.

Actual durable readiness/status after this batch:

- complete command matrices: `148 / 151`
- complete documented command-option pairs: `2330 / 3212`
- represented documented command-option pairs: `2380 / 3212`
- matrix rows: `6581`
- verified rows: `5769`
- invalid-input rows: `787`
- open or partial exact rows: `0`

Per-command position on the touched surface:

- `grep`: `54 / 73` reviewed-complete documented option pairs,
  `73 / 73` represented documented option pairs, `112` written rows, `112`
  classified rows, `112` stock-matching rows, `0` invalid-input rows, and `0`
  exact-open rows

The next best high-throughput follow-up should still stay on `grep`, because
the command is now down to a pure short-alias and parser-synonym tail. The
remaining helper-free documented queue is concentrated in the short spellings
`-A`, `-B`, `-C`, `-E`, `-G`, `-I`, `-O`, `-P`, `-W`, `-a`, `-e`, `-f`, `-h`,
`-o`, `-p`, `-q`, `-r`, `-w`, and `-z`.

As of 2026-06-27 the latest completed batch is a helper-free local
`grep` binary, submodule, and pager schema-tail closure across the modeled
tracked text-plus-binary local lane. This batch closed the last remaining
`grep` schema gaps on the helper-free surface by adding exact stock-Git
evidence and runtime support for `-I`, `--recurse-submodules`, `-O`,
`--open-files-in-pager`, and `--open-files-in-pager=less`. The only real
implementation wrinkle was parser recovery rather than search behavior:
`clap` treated bare `-O hello` and `--open-files-in-pager hello` as though
`hello` were the pager argument, so `grep` dispatch now locally normalizes the
raw argv tail for this option family and recovers the stock pattern/pathspec
lane without widening parser behavior elsewhere. The runtime stays intentionally
bounded to the current helper-free local surface: `-I` suppresses matching
binary-file output, `--recurse-submodules` remains a stock-compatible no-op on
the repository-without-submodules lane, bare pager mode follows `GIT_PAGER`,
and explicit pager values emit the same raw matched-file contents as stock Git
on the modeled `less` lane.

Focused verification was
`cargo test -p zmin-cli --test git_grep_compat grep_binary_submodule_and_pager_schema_tail_matches_stock_git -- --exact --nocapture`,
`cargo test -p zmin-cli --test git_grep_compat -- --nocapture`,
`cargo check -p zmin-cli`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(grep|summary)\t'`,
and `git diff --check`.

Actual durable readiness/status after this batch:

- complete command matrices: `148 / 151`
- complete documented command-option pairs: `2297 / 3212`
- represented documented command-option pairs: `2380 / 3212`
- matrix rows: `6581`
- verified rows: `5769`
- invalid-input rows: `787`
- open or partial exact rows: `0`

Per-command position on the touched surface:

- `grep`: `21 / 73` reviewed-complete documented option pairs,
  `73 / 73` represented documented option pairs, `112` written rows, `112`
  classified rows, `112` stock-matching rows, `0` invalid-input rows, and `0`
  exact-open rows

The next best high-throughput follow-up should still stay on `grep`, but it is
now a pure expansion queue rather than a schema-gap queue. The remaining helper-
free documented tail is concentrated in already modeled families such as
`--all-match`, `--and`, `--color`, `--column`, `--function-context`,
`--name-only`, `--not`, `--null`, `--only-matching`, and `--or`.

As of 2026-06-27 the latest completed batch is a helper-free local
`grep` schema-tail plus override/value closure across modeled tracked,
untracked, ignored, and filesystem-only lanes. This batch added exact
stock-Git evidence and runtime support for `-r`, `--no-exclude-standard`,
`--textconv`, `-P`, and `--perl-regexp`, while also widening repeated
documented value parsing for `-A` / `--after-context`, `-B` /
`--before-context`, `-C` / `--context`, `--max-depth`, and `--threads`. The
implementation stayed deliberately local and helper-free: repeated value forms
now preserve the stock last-one-wins lane for context, depth, and thread
arguments, `--no-exclude-standard` now disables the implicit ignore filtering
that comes with the modeled `--untracked` worktree lane, `--textconv` stays a
stock-compatible no-op on the current text-only fixture surface, perl-regexp
aliases are now accepted on the simple local regex lane, and explicit
`--no-index --recursive` plus repeated no-index depth forms now match stock
Git on the filesystem traversal lane.

Focused verification was
`cargo test -p zmin-cli --test git_grep_compat -- --nocapture`,
`cargo check -p zmin-cli`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(grep|summary)\t'`,
and `git diff --check`.

Actual durable readiness/status after this batch:

- complete command matrices: `148 / 151`
- complete documented command-option pairs: `2297 / 3212`
- represented documented command-option pairs: `2376 / 3212`
- matrix rows: `6576`
- verified rows: `5764`
- invalid-input rows: `787`
- open or partial exact rows: `0`

Per-command position on the touched surface:

- `grep`: `21 / 73` reviewed-complete documented option pairs,
  `69 / 73` represented documented option pairs, `107` written rows, `107`
  classified rows, `107` stock-matching rows, `0` invalid-input rows, and `0`
  exact-open rows

The next best high-throughput follow-up should likely stay on `grep`, because
it remains the largest safe local helper-free documented queue after this
batch. The new schema-tail and repeated-value closures moved more of the local
surface out of the remaining tail; the queue is now concentrated in the last
few schema gaps such as `--open-files-in-pager`, `--recurse-submodules`, `-I`,
and `-O`, plus expansion work on already modeled local families like
`--all-match`, `--and`, `--color`, `--column`, `--function-context`,
`--name-only`, `--not`, `--null`, `--only-matching`, and `--or`.

As of 2026-06-27 the latest completed batch is a helper-free local
`format-patch` mail-series tail closure across the modeled two-patch and
single-commit stdout mail lanes. This batch closed the remaining command-
specific schema/evidence tails around `--base`, `--no-base`,
`--cover-from-description`, `--description-file`, `--filename-max-length`,
`--ignore-if-in-upstream`, `--interdiff`, `--range-diff`, and
`--creation-factor`. It also surfaced and fixed two older stock drifts in the
same renderer path while proving the new rows: cover letters now omit the
non-stock `---` diffstat marker, and inline or attached MIME patch bodies now
keep the stock blank-line separation before the patch part boundary. One CLI
parsing detail also needed a precise compatibility split: short `-p` keeps the
stock no-diffstat lane, while long `--patch` stays on the stock default
diffstat-plus-patch lane.

Focused verification was
`cargo test -p zmin-cli --test git_mail_series_compat format_patch_mail_series_tail_family_matches_stock_git -- --exact --nocapture`,
`cargo test -p zmin-cli --test git_mail_series_compat -- --nocapture`,
`cargo check -p zmin-cli`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(format-patch|summary)\t'`,
and `git diff --check`.

Actual durable readiness/status after this batch:

- complete command matrices: `148 / 151`
- complete documented command-option pairs: `2297 / 3212`
- represented documented command-option pairs: `2328 / 3212`
- matrix rows: `6505`
- verified rows: `5693`
- invalid-input rows: `787`
- open or partial exact rows: `0`

Per-command position on the touched surface:

- `format-patch`: `131 / 162` reviewed-complete documented option pairs,
  `162 / 162` represented documented option pairs, `201` written rows, `201`
  classified rows, `201` stock-matching rows, `0` invalid-input rows, and `0`
  exact-open rows

The next best high-throughput follow-up should now move off `format-patch`,
because the helper-free documented tail is fully represented and exactly
verified on the current modeled surface. The largest remaining helper-free
documented queues are now led by `send-email` (`62` remaining documented
option pairs), `pull` (`62`), `rebase` (`54`), `grep` (`52`), and `log`
(`47`).

As of 2026-06-27 the latest completed batch is a helper-free local
`format-patch` merge-diff and dirstat-alias closure across the modeled
merge-only exclude revision set and nested-directory single-commit stdout
mail-series lanes. This batch was mostly parser plus stock-guard parity rather
than new renderer work: `format-patch` now accepts the merge-diff family
`-m`, `-c`, `-t`, `--dd`, `--diff-merges=first-parent|separate|combined|dense-combined|off`,
`--no-diff-merges`, and the valid `--combined-all-paths` compositions on a
merge-only revision set, matching stock Git's empty stdout lane there.
It also closed the remaining helper-free dirstat aliases and expansions around
`--dirstat=cumulative`, `--cumulative`, `-X`, `-X10`, and
`--dirstat-by-file=10,cumulative` on the nested-directory stdout lane.
Two stock fatal guards were also made exact: bare `--combined-all-paths`
preserves the stock `makes no sense without -c or --cc` failure, and
`--remerge-diff` preserves the stock `does not make sense` failure.

Focused verification was
`cargo test -p zmin-cli --test git_mail_series_compat format_patch_merge_diff_and_dirstat_alias_family_matches_stock_git -- --exact --nocapture`,
`cargo test -p zmin-cli --test git_mail_series_compat format_patch_word_diff_color_and_regex_family_matches_stock_git -- --exact --nocapture`,
`cargo check -p zmin-cli`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(format-patch|summary)\t'`,
and `git diff --check`.

Actual durable readiness/status after this batch:

- complete command matrices: `148 / 151`
- complete documented command-option pairs: `2297 / 3212`
- represented documented command-option pairs: `2319 / 3212`
- matrix rows: `6492`
- verified rows: `5680`
- invalid-input rows: `787`
- open or partial exact rows: `0`

Per-command position on the touched surface:

- `format-patch`: `131 / 162` reviewed-complete documented option pairs,
  `153 / 162` represented documented option pairs, `188` written rows, `188`
  classified rows, `188` stock-matching rows, `0` invalid-input rows, and `0`
  exact-open rows

The next best high-throughput follow-up should still stay on `format-patch`,
but the queue is now much more concentrated in command-specific mail-series
tails: `--base`, `--no-base`, `--cover-from-description`,
`--description-file`, `--filename-max-length`, `--ignore-if-in-upstream`,
`--interdiff`, `--range-diff`, and `--creation-factor`.

As of 2026-06-27 the latest completed batch is a helper-free local
`format-patch` color-word-diff and bounded-regex closure across the modeled
multi-file single-commit stdout mail-series lane. This batch closed a real
mail-render gap rather than just expanding evidence: `format-patch` now matches
stock Git for `--word-diff=color`, `--color-words`, `--color-words=[a-z]+`,
and the bounded helper-free `--word-diff-regex=[a-z]+` lane across plain,
porcelain, and color word-diff output. One stock-failure detail also mattered
for exact parity: invalid `--word-diff-regex=[` now aborts after the mail
prelude and diffstat block but before `diff --git`, matching stock Git's
partial stdout shape instead of failing from inside hunk rendering.

Focused verification was
`cargo test -p zmin-cli --test git_mail_series_compat format_patch_word_diff_color_and_regex_family_matches_stock_git -- --exact --nocapture`,
`cargo test -p zmin-cli --test git_mail_series_compat format_patch_word_diff_order_and_reverse_family_matches_stock_git -- --exact --nocapture`,
`cargo test -p zmin-cli --test git_mail_series_compat format_patch_prefix_null_and_dirstat_family_matches_stock_git -- --exact --nocapture`,
`cargo check -p zmin-cli`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(format-patch|summary)\t'`,
and `git diff --check`.

Actual durable readiness/status after this batch:

- complete command matrices: `148 / 151`
- complete documented command-option pairs: `2297 / 3212`
- represented documented command-option pairs: `2309 / 3212`
- matrix rows: `6473`
- verified rows: `5661`
- invalid-input rows: `787`
- open or partial exact rows: `0`

Per-command position on the touched surface:

- `format-patch`: `131 / 162` reviewed-complete documented option pairs,
  `143 / 162` represented documented option pairs, `169` written rows, `169`
  classified rows, `169` stock-matching rows, `0` invalid-input rows, and `0`
  exact-open rows

The next best high-throughput follow-up should still stay on `format-patch`,
but the queue has moved off the word-diff tail. The bounded helper-free
`--word-diff-regex` and `--color-words` lanes now have exact stock evidence,
so the remaining documented schema/behavior tails are led by `--base`,
`--combined-all-paths`, `--cover-from-description`, `--creation-factor`,
`--dd`, `--description-file`, `--diff-merges`, `--interdiff`, `-X`, `-c`,
`-m`, and `-t`.

As of 2026-06-27 the latest completed batch is a helper-free local
`format-patch` expansion-evidence closure across the existing multi-file and
nested-directory single-commit stdout mail-series lanes. This batch did not
change runtime code; it raised exact stock-Git coverage density for already
implemented shared-diff options by adding rows for bare `--word-diff`,
`--word-diff=none`, `--submodule=short`, `--submodule=diff`, composed
`-O order.txt` plus `--skip-to=beta.txt`, composed `-O order.txt` plus
`--rotate-to=beta.txt`, composed `--no-prefix -z`, and the additional
directory-stat values `--dirstat=files` and `--dirstat=files,10`. One probed
candidate was explicitly left out of the closure: `--word-diff=color` is not
helper-free compatible yet because stock Git also colorizes diff headers and
diffstat, while the current renderer only colors word hunks.

Focused verification was
`cargo test -p zmin-cli --test git_mail_series_compat format_patch_word_diff_order_and_reverse_family_matches_stock_git -- --exact --nocapture`,
`cargo test -p zmin-cli --test git_mail_series_compat format_patch_prefix_null_and_dirstat_family_matches_stock_git -- --exact --nocapture`,
`cargo check -p zmin-cli`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(format-patch|summary)\t'`,
and `git diff --check`.

Actual durable readiness/status after this batch:

- complete command matrices: `148 / 151`
- complete documented command-option pairs: `2297 / 3212`
- represented documented command-option pairs: `2307 / 3212`
- matrix rows: `6466`
- verified rows: `5654`
- invalid-input rows: `787`
- open or partial exact rows: `0`

Per-command position on the touched surface:

- `format-patch`: `131 / 162` reviewed-complete documented option pairs,
  `141 / 162` represented documented option pairs, `162` written rows, `162`
  classified rows, `162` stock-matching rows, `0` invalid-input rows, and `0`
  exact-open rows

The next best high-throughput follow-up should still stay on `format-patch`,
but now the remaining work is more sharply split between true implementation
tails and non-helper-free render gaps. The remaining implemented-but-unverified
surface is still led by `--word-diff-regex`, while the documented helper-free
schema holes are concentrated around `--base`, `--color-words`,
`--combined-all-paths`, `--cover-from-description`, `--creation-factor`,
`--dd`, `--description-file`, `--diff-merges`, `--interdiff`, `-X`, `-c`,
`-m`, and `-t`. `--word-diff=color` should not be reselected as an
evidence-only row; it needs real colorized mail-render parity first.

As of 2026-06-27 the latest completed batch is a helper-free local
`format-patch` prefix/null/dirstat closure across the modeled single-commit
stdout mail-series lanes. This batch added exact stock-Git matrix evidence for
`--no-prefix`, `-z`, `--dirstat`, `--dirstat=files,0`, and
`--dirstat-by-file`, and widened the mail renderer so `format-patch` now
reuses stock-compatible no-prefix patch emission, NUL-separated prelude
boundaries, and directory-stat prelude rendering on the covered lanes instead
of staying limited to diffstat/raw/numstat/shortstat summaries.

Focused verification was
`cargo test -p zmin-cli --test git_mail_series_compat format_patch_prefix_null_and_dirstat_family_matches_stock_git -- --exact --nocapture`,
`cargo check -p zmin-cli`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(format-patch|summary)\t'`,
and `git diff --check`.

Actual durable readiness/status after this batch:

- complete command matrices: `148 / 151`
- complete documented command-option pairs: `2297 / 3212`
- represented documented command-option pairs: `2307 / 3212`
- matrix rows: `6454`
- verified rows: `5642`
- invalid-input rows: `787`
- open or partial exact rows: `0`

Per-command position on the touched surface:

- `format-patch`: `131 / 162` reviewed-complete documented option pairs,
  `141 / 162` represented documented option pairs, `150` written rows, `150`
  classified rows, `150` stock-matching rows, `0` invalid-input rows, and `0`
  exact-open rows

The next best high-throughput follow-up should still stay on `format-patch`,
but the queue tightened again: `-z`, `--dirstat`, `--dirstat-by-file`, and
`--no-prefix` are no longer schema holes and now sit in
`doc_option_expansion_required`; the remaining implemented-but-unverified tail
is led by `--word-diff-regex`, while the remaining helper-free documented
schema holes are now concentrated around `--base`, `--color-words`,
`--combined-all-paths`, `--cover-from-description`, `--creation-factor`,
`--dd`, `--description-file`, `--diff-merges`, `--interdiff`, `-X`, `-c`,
`-m`, and `-t`.

As of 2026-06-27 the latest completed batch is a helper-free local
`format-patch` diff-order, reverse, and word/submodule closure on a modeled
single-commit multi-file stdout mail-series lane. This batch added exact
stock-Git matrix evidence for `--word-diff=plain`, `--word-diff=porcelain`,
`--submodule=log`, `-O order.txt`, `--skip-to=beta.txt`,
`--rotate-to=beta.txt`, and `-R`, and widened the shared mail renderer so
`format-patch` now reuses stock-compatible diff ordering, rotate/skip
selection, reverse-direction rendering, word diff output, and submodule format
selection from the existing diff runtime instead of hard-coded default patch
emission.

Focused verification was
`cargo test -p zmin-cli --test git_mail_series_compat format_patch_word_diff_order_and_reverse_family_matches_stock_git -- --exact --nocapture`,
`cargo check -p zmin-cli`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(format-patch|summary)\t'`,
and `git diff --check`.

Actual durable readiness/status after this batch:

- complete command matrices: `148 / 151`
- complete documented command-option pairs: `2297 / 3212`
- represented documented command-option pairs: `2303 / 3212`
- matrix rows: `6449`
- verified rows: `5637`
- invalid-input rows: `787`
- open or partial exact rows: `0`

Per-command position on the touched surface:

- `format-patch`: `131 / 162` reviewed-complete documented option pairs,
  `137 / 162` represented documented option pairs, `145` written rows, `145`
  classified rows, `145` stock-matching rows, `0` invalid-input rows, and `0`
  exact-open rows

The next best high-throughput follow-up should stay on `format-patch`, but the
shape changed: `--word-diff`, `--submodule`, `--rotate-to`, `--skip-to`, `-O`,
and `-R` are no longer schema holes and now sit in
`doc_option_expansion_required`, while `--word-diff-regex` and `-z` remain
implemented without exact matrix evidence and `25` documented `format-patch`
tails are still `doc_option_not_in_zmin_schema`. The immediate helper-free
queue is therefore `--word-diff-regex`, `-z`, and the next shared-diff
documented tails such as `--dirstat`, `--dirstat-by-file`, `--color-words`,
and merge/diff-merges spellings.

As of 2026-06-27 the latest completed batch is a helper-free local
`format-patch` mail-header closure on the already modeled single-commit stdout
mail-series lane. This batch added exact stock-Git matrix evidence for
`--to`, `--cc`, `--add-header`, `--in-reply-to`, `--thread`, `--from`,
`--force-in-body-from`, `--no-force-in-body-from`, `--signature-file`,
`--encode-email-headers`, and `--no-encode-email-headers`. The runtime
highlight stayed focused on mail metadata rather than diff output: this slice
extended header rendering to match stock header order, dynamic Message-ID
presence, sender override handling, body author preservation for `--from`, and
footer replacement from a signature file on the existing stdout lane.

Focused verification was
`cargo test -p zmin-cli --test git_mail_series_compat format_patch_mail_header_family_matches_stock_git -- --exact --nocapture`,
`cargo check -p zmin-cli`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(format-patch|summary)\t'`,
and `git diff --check`.

Actual durable readiness/status after this batch:

- complete command matrices: `148 / 151`
- complete documented command-option pairs: `2297 / 3212`
- represented documented command-option pairs: `2297 / 3212`
- matrix rows: `6443`
- verified rows: `5631`
- invalid-input rows: `787`
- open or partial exact rows: `0`

Per-command position on the touched surface:

- `format-patch`: `131 / 131` reviewed-complete documented option pairs,
  `131 / 131` represented documented option pairs, `139` written rows, `139`
  classified rows, `139` stock-matching rows, `0` invalid-input rows, and `0`
  exact-open rows

The next best high-throughput follow-up should stay on `format-patch`, because
the command still has `31` remaining `doc_option_not_in_zmin_schema` rows, now
led by helper-free documented tails such as `--base`, `--color-words`,
`--combined-all-paths`, `--cover-from-description`, `--creation-factor`,
`--dd`, `--description-file`, `--diff-merges`, `--dirstat`, `--interdiff`,
`--submodule`, `--word-diff`, `-O`, `-R`, `-X`, `-c`, `-m`, and `-z`.

As of 2026-06-27 the latest completed batch is a helper-free local
`format-patch` diff-output closure on the already modeled single-commit stdout
mail-series lane. This batch added exact stock-Git matrix evidence for
`--raw`, `--numstat`, `--shortstat`, `--summary`, `--no-patch`, `--no-stat`,
`--patch-with-raw`, `--full-index`, and `-p`. The runtime highlight stayed
cohesive: this slice extended the mail-render pipeline so the stock prelude
block before the patch body now switches correctly between diffstat, raw,
numstat, shortstat, summary/no-summary suppression, and full-index patch hash
lengths, while also matching stock blank-line and `---` separator placement.

Focused verification was
`cargo test -p zmin-cli --test git_mail_series_compat format_patch_diff_output_family_matches_stock_git -- --exact --nocapture`,
`cargo check -p zmin-cli`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(format-patch|summary)\t'`,
and `git diff --check`.

Actual durable readiness/status after this batch:

- complete command matrices: `148 / 151`
- complete documented command-option pairs: `2286 / 3212`
- represented documented command-option pairs: `2286 / 3212`
- matrix rows: `6432`
- verified rows: `5620`
- invalid-input rows: `787`
- open or partial exact rows: `0`

Per-command position on the touched surface:

- `format-patch`: `120 / 120` reviewed-complete documented option pairs,
  `120 / 120` represented documented option pairs, `128` written rows, `128`
  classified rows, `128` stock-matching rows, `0` invalid-input rows, and `0`
  exact-open rows

The next best high-throughput follow-up should stay on `format-patch`, because
the command still has `42` remaining `doc_option_not_in_zmin_schema` rows, now
led by helper-free documented tails such as `--add-header`, `--base`, `--cc`,
`--color-words`, `--combined-all-paths`, `--cover-from-description`,
`--description-file`, `--ignore-if-in-upstream`, `--in-reply-to`, and related
mail or diff spellings.

As of 2026-06-27 the latest completed batch is a helper-free local
`format-patch` mail-render closure on the already modeled two-patch stdout
range lane. This batch added exact stock-Git matrix evidence for `--signoff`,
`-s`, `--zero-commit`, `--reroll-count`, `-v`, `--no-signature`,
`--signature`, `--start-number`, `--rfc`, `--keep-subject`, and `-k`. The
runtime highlight stayed narrow but substantive this time: the real closure was
mail rendering parity for trailers, footer suppression, zeroed `From` ids,
reroll and RFC subject prefixes, shifted numbering, and raw subject
preservation on an existing series lane, plus one parser fix so `--rfc` no
longer consumes the revision range without `=`.

Focused verification was
`cargo test -p zmin-cli --test git_mail_series_compat format_patch_mail_render_family_matches_stock_git -- --exact --nocapture`,
`cargo test -p zmin-cli --test git_mail_series_compat format_patch_keep_subject_family_matches_stock_git -- --exact --nocapture`,
`cargo check -p zmin-cli`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(format-patch|summary)\t'`,
and `git diff --check`.

Actual durable readiness/status after this batch:

- complete command matrices: `148 / 151`
- complete documented command-option pairs: `2277 / 3212`
- represented documented command-option pairs: `2277 / 3212`
- matrix rows: `6423`
- verified rows: `5611`
- invalid-input rows: `787`
- open or partial exact rows: `0`

Per-command position on the touched surface:

- `format-patch`: `111 / 111` reviewed-complete documented option pairs,
  `111 / 111` represented documented option pairs, `119` written rows, `119`
  classified rows, `119` stock-matching rows, `0` invalid-input rows, and `0`
  exact-open rows

The next best high-throughput follow-up should stay on `format-patch`, because
the command still has `51` remaining `doc_option_not_in_zmin_schema` rows, now
led by helper-free documented tails such as `--add-header`, `--base`, `--cc`,
`--color-words`, `--cover-from-description`, `--description-file`,
`--full-index`, `--ignore-if-in-upstream`, `--in-reply-to`, and related mail
or diff spellings.

As of 2026-06-27 the latest completed batch is a helper-free local
`format-patch` invalid-surface and direct-output closure on the already modeled
single-commit lane. This batch added exact stock-Git matrix evidence for
`--check`, `--name-only`, `--name-status`, and `--output`, including the
stable fatal guards for unsupported diff-only spellings on the stdout lane, the
stock conflict guards for `--output` plus `--stdout`, and direct single-file
mbox output to a requested path.

Focused verification was
`cargo test -p zmin-cli --test git_mail_series_compat format_patch_invalid_surface_and_output_file_match_stock_git -- --exact --nocapture`,
`cargo check -p zmin-cli`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(format-patch|summary)\t'`,
and `git diff --check`.

Actual durable readiness/status after this batch:

- complete command matrices: `148 / 151`
- complete documented command-option pairs: `2266 / 3212`
- represented documented command-option pairs: `2266 / 3212`
- matrix rows: `6412`
- verified rows: `5600`
- invalid-input rows: `787`
- open or partial exact rows: `0`

Per-command position on the touched surface:

- `format-patch`: `100 / 100` reviewed-complete documented option pairs,
  `100 / 100` represented documented option pairs, `108` written rows, `108`
  classified rows, `108` stock-matching rows, `0` invalid-input rows, and `0`
  exact-open rows

The next best high-throughput follow-up should stay on `format-patch`, because
the command still has `62` remaining `doc_option_not_in_zmin_schema` rows, now
led by helper-free documented tails such as `--add-header`, `--base`, `--cc`,
`--color-words`, `--cover-from-description`, `--description-file`,
`--full-index`, `--ignore-if-in-upstream`, `--in-reply-to`, and related mail
or diff spellings.

As of 2026-06-27 the latest completed batch is a helper-free local
`format-patch` pickaxe and short common-diff parser closure on the already
modeled single-commit stdout mail-series lane. This batch added exact
stock-Git matrix evidence for `-q`, `-u`, `-w`, `-b`, `-S`, `-G`,
`--pickaxe-regex`, `--pickaxe-all`, `-I`, `--ignore-matching-lines`,
`--find-object`, and `-l`, and lifted twelve documented option pairs into the
reviewed-complete represented surface without adding new mail-series rendering
logic beyond parser/schema acceptance.

Focused verification was
`cargo test -p zmin-cli --test git_mail_series_compat format_patch_helper_free_pickaxe_and_short_diff_family_matches_stock_git -- --exact --nocapture`,
`cargo check -p zmin-cli`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(format-patch|summary)\t'`,
and `git diff --check`.

Actual durable readiness/status after this batch:

- complete command matrices: `148 / 151`
- complete documented command-option pairs: `2262 / 3212`
- represented documented command-option pairs: `2262 / 3212`
- matrix rows: `6406`
- verified rows: `5594`
- invalid-input rows: `787`
- open or partial exact rows: `0`

Per-command position on the touched surface:

- `format-patch`: `96 / 96` reviewed-complete documented option pairs,
  `96 / 96` represented documented option pairs, `102` written rows, `102`
  classified rows, `102` stock-matching rows, `0` invalid-input rows, and `0`
  exact-open rows

The next best high-throughput follow-up should stay on `format-patch`, because
the command still has `66` remaining `doc_option_not_in_zmin_schema` rows, now
led by helper-free documented tails such as `--add-header`, `--base`, `--cc`,
`--check`, `--color-words`, `--cover-from-description`,
`--description-file`, `--full-index`, `--ignore-if-in-upstream`, and related
mail or diff spellings.

As of 2026-06-27 the latest completed batch is a helper-free local
`format-patch` shared diff alias/text parser closure on the already modeled
single-commit stdout mail-series lane. This batch added exact stock-Git matrix
evidence for `--text`, `--textconv`, `--unified`, `-U`, `-a`, `-N`, `-M`,
`-C`, `-D`, `-B`, `-W`, `--ws-error-highlight`, `--ext-diff`,
`--no-relative`, and `--no-rename-empty`, and lifted fifteen documented
option pairs into the reviewed-complete represented surface without adding new
mail-series rendering logic beyond parser/schema acceptance.

The batch closed one cohesive parser-and-evidence gap rather than introducing a
new `format-patch` behavior family:

- Zmin already matched stock Git on the covered helper-free single-commit
  stdout mail-series lane, and the remaining work here was to accept the
  documented alias/text and unified spellings above and promote the resulting
  exact stock-oracle evidence into represented plus reviewed-complete
  `format-patch` doc-option pairs

Focused verification was
`cargo test -p zmin-cli --test git_mail_series_compat format_patch_shared_diff_alias_family_matches_stock_git -- --exact --nocapture`,
`cargo check -p zmin-cli`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(format-patch|summary)\t'`,
and `git diff --check`.

Actual durable readiness/status after this batch:

- complete command matrices: `148 / 151`
- complete documented command-option pairs: `2250 / 3212`
- represented documented command-option pairs: `2250 / 3212`
- matrix rows: `6394`
- verified rows: `5582`
- invalid-input rows: `787`
- open or partial exact rows: `0`

Per-command position on the touched surface:

- `format-patch`: `84 / 84` reviewed-complete documented option pairs,
  `84 / 84` represented documented option pairs, `90` written rows, `90`
  classified rows, `90` stock-matching rows, `0` invalid-input rows, and `0`
  exact-open rows

The next best high-throughput follow-up should stay on `format-patch`, because
the command still has `78` remaining `doc_option_not_in_zmin_schema` rows, now
led by helper-free documented tails such as `--add-header`, `--base`, `--cc`,
`--check`, `--color-words`, `--cover-from-description`,
`--description-file`, `--full-index`, `--ignore-if-in-upstream`, and related
diff/mail spellings.

As of 2026-06-27 the latest completed batch is a helper-free local
`format-patch` shared diff noop/default-preserving parser closure on the
already modeled single-commit stdout mail-series lane. This batch added exact
stock-Git matrix evidence for `--exit-code`, `--progress`, `--quiet`,
`--find-renames`, `--find-copies`, `--find-copies-harder`,
`--rename-empty`, `--break-rewrites`, `--compact-summary`,
`--patch-with-stat`, `--output-indicator-new`,
`--output-indicator-old`, `--output-indicator-context`,
`--src-prefix`, `--dst-prefix`, `--line-prefix`, `--relative`,
`--root`, `--notes`, `--no-notes`, `--no-thread`,
`--no-cover-letter`, `--no-attach`, `--no-binary`,
`--no-renames`, and `--no-indent-heuristic`, and lifted twenty-six
documented option pairs into the reviewed-complete represented surface without
adding new mail-series rendering logic beyond parser/schema acceptance.

The batch closed one cohesive parser-and-evidence gap rather than introducing a
new `format-patch` behavior family:

- Zmin already matched stock Git on the covered helper-free single-commit
  stdout mail-series lane, and the remaining work here was to accept the
  documented noop/default-preserving diff and mail spellings above and promote
  the resulting exact stock-oracle evidence into represented plus
  reviewed-complete `format-patch` doc-option pairs

Focused verification was
`cargo test -p zmin-cli --test git_mail_series_compat format_patch_shared_diff_noop_family_matches_stock_git -- --exact --nocapture`,
`cargo check -p zmin-cli`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(format-patch|summary)\t'`,
and `git diff --check`.

Actual durable readiness/status after this batch:

- complete command matrices: `148 / 151`
- complete documented command-option pairs: `2235 / 3212`
- represented documented command-option pairs: `2235 / 3212`
- matrix rows: `6379`
- verified rows: `5567`
- invalid-input rows: `787`
- open or partial exact rows: `0`

Per-command position on the touched surface:

- `format-patch`: `69 / 69` reviewed-complete documented option pairs,
  `69 / 69` represented documented option pairs, `75` written rows, `75`
  classified rows, `75` stock-matching rows, `0` invalid-input rows, and `0`
  exact-open rows

The next best high-throughput follow-up should stay on `format-patch`, because
the command still has `93` remaining `doc_option_not_in_zmin_schema` rows, now
led by helper-free documented tails such as `--add-header`, `--base`, `--cc`,
`--check`, `--color-words`, `--cover-from-description`,
`--description-file`, `--full-index`, `--ignore-if-in-upstream`, and related
diff/mail spellings.

As of 2026-06-27 the latest completed batch is a helper-free local
`format-patch` shared diff-option parser closure on the already modeled
single-commit stdout mail-series lane. This batch added exact stock-Git matrix
evidence for `--abbrev`, `--always`, `--anchored`, `--color`,
`--color-moved`, `--color-moved-ws`, `--diff-algorithm`,
`--diff-filter`, `--function-context`, `--histogram`,
`--ignore-all-space`, `--ignore-blank-lines`, `--ignore-cr-at-eol`,
`--ignore-space-at-eol`, `--ignore-space-change`,
`--ignore-submodules`, `--indent-heuristic`, `--inter-hunk-context`,
`--irreversible-delete`, `--ita-invisible-in-index`, `--minimal`, and
`--patience`, and lifted twenty-two documented option pairs into the
reviewed-complete represented surface without adding new mail-series rendering
logic beyond parser/schema acceptance.

The batch closed one cohesive parser-and-evidence gap rather than introducing a
new `format-patch` behavior family:

- Zmin already matched stock Git on the covered helper-free single-commit
  stdout mail-series lane, and the remaining work here was to accept the
  documented shared diff spellings above and promote the resulting exact
  stock-oracle evidence into represented plus reviewed-complete
  `format-patch` doc-option pairs

Focused verification was
`cargo test -p zmin-cli --test git_mail_series_compat format_patch_shared_diff_option_family_matches_stock_git -- --exact --nocapture`,
`cargo check -p zmin-cli`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(format-patch|summary)\t'`,
and `git diff --check`.

Actual durable readiness/status after this batch:

- complete command matrices: `148 / 151`
- complete documented command-option pairs: `2209 / 3212`
- represented documented command-option pairs: `2209 / 3212`
- matrix rows: `6353`
- verified rows: `5541`
- invalid-input rows: `787`
- open or partial exact rows: `0`

Per-command position on the touched surface:

- `format-patch`: `43 / 43` reviewed-complete documented option pairs,
  `43 / 43` represented documented option pairs, `49` written rows, `49`
  classified rows, `49` stock-matching rows, `0` invalid-input rows, and `0`
  exact-open rows

The next best high-throughput follow-up should stay on `format-patch`, because
the command still has `119` remaining `doc_option_not_in_zmin_schema` rows,
now led by helper-free documented tails such as `--add-header`, `--base`,
`--break-rewrites`, `--cc`, `--check`, `--color-words`,
`--compact-summary`, `--description-file`, and related diff/mail spellings.

As of 2026-06-27 the latest completed batch is a helper-free local
`format-patch` default/no-op diff-option closure on the already modeled
single-commit stdout mail-series lane. This batch added exact stock-Git matrix
evidence for `--binary`, `--default-prefix`, `--no-ext-diff`,
`--no-textconv`, `--no-color`, `--no-color-moved`,
`--no-color-moved-ws`, `--stat`, and `--patch`, and lifted those nine
documented option pairs into the reviewed-complete represented surface without
adding new runtime rendering logic beyond parser/schema acceptance.

The batch closed one cohesive parser-and-evidence gap rather than introducing a
new `format-patch` behavior family:

- Zmin already matched stock Git on the covered helper-free single-commit
  stdout mail-series lane, and the remaining work here was to accept the
  explicit documented default/no-op diff spellings and promote the resulting
  exact stock-oracle evidence into represented plus reviewed-complete
  `format-patch` doc-option pairs

Focused verification was
`cargo test -p zmin-cli --test git_mail_series_compat format_patch_default_diff_option_family_matches_stock_git -- --exact --nocapture`,
`cargo check -p zmin-cli`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(format-patch|summary)\t'`,
and `git diff --check`.

Actual durable readiness/status after this batch:

- complete command matrices: `148 / 151`
- complete documented command-option pairs: `2187 / 3212`
- represented documented command-option pairs: `2187 / 3212`
- matrix rows: `6328`
- verified rows: `5516`
- invalid-input rows: `787`
- open or partial exact rows: `0`

Per-command position on the touched surface:

- `format-patch`: `21 / 21` reviewed-complete documented option pairs,
  `21 / 21` represented documented option pairs, `24` written rows, `24`
  classified rows, `24` stock-matching rows, `0` invalid-input rows, and `0`
  exact-open rows

The next best high-throughput follow-up should stay on `format-patch`, because
the command still has the largest helper-free documented tail at `141`
remaining `doc_option_not_in_zmin_schema` rows, now led by shared diff and
mail-surface spellings such as `--abbrev`, `--always`, `--anchored`,
`--diff-algorithm`, `--color`, and related family members.

As of 2026-06-27 the latest completed batch is a helper-free shared `diff*`
reviewed-complete closure on the already represented porcelain `--output` plus
plumbing `--output` and `--line-prefix` lanes. This batch promoted the
existing exact stock-Git matrix evidence for `diff --output`,
`diff-files --output`, `diff-files --line-prefix`, `diff-index --output`,
`diff-index --line-prefix`, `diff-tree --output`, and
`diff-tree --line-prefix` into the reviewed-complete layer, finishing the full
represented `diff`, `diff-files`, `diff-index`, and `diff-tree` surfaces
without adding new runtime behavior.

The batch closed one cohesive census/evidence gap rather than adding new diff
implementation:

- Zmin already matched stock Git on the covered helper-free `diff*` lanes, and
  the remaining work here was to lift the existing matrix rows plus the shared
  `git_diff_compat::diff_output_and_plumbing_line_prefix_match_stock_git`
  stock-oracle evidence into reviewed-complete doc-option pairs and the
  reviewed-complete command matrices so the represented `diff*` surfaces are
  counted correctly

Focused verification was
`cargo test -p zmin-cli --test git_diff_compat diff_output_and_plumbing_line_prefix_match_stock_git -- --exact --nocapture`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(diff|diff-files|diff-index|diff-tree|summary)\t'`,
and `git diff --check`.

Actual durable readiness/status after this batch:

- complete command matrices: `148 / 151`
- complete documented command-option pairs: `2178 / 3212`
- represented documented command-option pairs: `2178 / 3212`
- matrix rows: `6319`
- verified rows: `5507`
- invalid-input rows: `787`
- open or partial exact rows: `0`

Per-command position on the touched surface:

- `diff`: `84 / 84` reviewed-complete documented option pairs,
  `84 / 84` represented documented option pairs, `251` written rows, `251`
  classified rows, `247` stock-matching rows, `4` invalid-input rows, and `0`
  exact-open rows
- `diff-files`: `81 / 81` reviewed-complete documented option pairs,
  `81 / 81` represented documented option pairs, `100` written rows, `100`
  classified rows, `100` stock-matching rows, `0` invalid-input rows, and `0`
  exact-open rows
- `diff-index`: `80 / 80` reviewed-complete documented option pairs,
  `80 / 80` represented documented option pairs, `107` written rows, `107`
  classified rows, `107` stock-matching rows, `0` invalid-input rows, and `0`
  exact-open rows
- `diff-tree`: `87 / 87` reviewed-complete documented option pairs,
  `87 / 87` represented documented option pairs, `119` written rows, `119`
  classified rows, `119` stock-matching rows, `0` invalid-input rows, and `0`
  exact-open rows

The next best high-throughput follow-up should move to the `format-patch`
schema/documented-option expansion head instead of staying on `diff*`, because
represented-versus-reviewed gaps are now closed and the largest remaining safe
helper-free backlog is `format-patch` with `150`
`doc_option_not_in_zmin_schema` rows in
`docs/cli/census/remaining_to_fix_or_verify.tsv` and only `12 / 162`
documented option pairs currently represented.

As of 2026-06-27 the latest completed batch is a helper-free local `commit`
reviewed-complete closure on the already represented verify, include,
pathspec-file, no-signoff, no-post-rewrite, patch, and fixture-key gpg-sign
lanes. This batch promoted the existing exact stock-Git matrix evidence for
`--verify`, `--include`, `--pathspec-from-file`, `--pathspec-file-nul`,
`--no-signoff`, `--no-post-rewrite`, `--patch`, `--gpg-sign`, `--no-gpg-sign`,
`-S`, `-i`, and `-p` into the reviewed-complete layer, finishing the full
represented `commit` surface without adding new runtime behavior.

The batch closed one cohesive census/evidence gap rather than adding new
commit implementation:

- Zmin already matched stock Git on the covered helper-free `commit` lanes,
  and the remaining work here was to lift the existing matrix rows plus the
  focused `git_commit_compat` stock-oracle evidence into reviewed-complete
  doc-option pairs and the reviewed-complete command matrix so the represented
  `commit` surface is counted correctly

Focused verification was
`cargo test -p zmin-cli --test git_commit_compat commit_documented_open_option_batch_matches_stock_git -- --exact --nocapture`,
`cargo test -p zmin-cli --test git_commit_compat commit_gpg_sign_family_matches_stock_git_with_fixture_key -- --exact --nocapture`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(commit|summary)\t'`,
and `git diff --check`.

Actual durable readiness/status after this batch:

- complete command matrices: `148 / 151`
- complete documented command-option pairs: `2171 / 3212`
- represented documented command-option pairs: `2178 / 3212`
- matrix rows: `6319`
- verified rows: `5507`
- invalid-input rows: `787`
- open or partial exact rows: `0`

Per-command position on the touched surface:

- `commit`: `58 / 58` reviewed-complete documented option pairs,
  `58 / 58` represented documented option pairs, `120` written rows, `120`
  classified rows, `115` stock-matching rows, `5` invalid-input rows, and `0`
  exact-open rows

The next best high-throughput follow-up should move to the shared `diff*`
reviewed-complete tail instead of staying on `commit`, because the remaining
largest helper-free represented-versus-reviewed gap is now the plumbing diff
cluster: `diff-files` at `79 / 81`, `diff-index` at `78 / 80`,
`diff-tree` at `85 / 87`, and `diff` at `83 / 84`.

As of 2026-06-27 the latest completed batch is a helper-free local
`cherry-pick` and `revert` reviewed-complete closure on the already represented
clean, fast-forward, editor, merge-mainline, no-commit, empty-policy,
reference-message, signed-commit, rerere-toggle, and invalid-input no-sequence
lanes. This batch promoted the existing exact stock-Git matrix evidence for the
remaining represented `cherry-pick` and `revert` option families into the
reviewed-complete layer, finishing both represented sequencer surfaces without
adding new runtime behavior.

The batch closed one cohesive census/evidence gap rather than adding new
sequencer implementation:

- Zmin already matched stock Git on the covered helper-free sequencer lanes,
  and the remaining work here was to lift the existing matrix rows plus oracle
  tests and smoke traces into reviewed-complete doc-option pairs and reviewed
  complete command matrices so the represented `cherry-pick` and `revert`
  surfaces are counted correctly

Focused verification was
`cargo test -p zmin-cli --test git_sequencer_compat cherry_pick_and_revert_documented_surface_batch_matches_stock_git -- --exact --nocapture`,
`cargo test -p zmin-cli --test git_sequencer_compat cherry_pick_and_revert_followup_documented_surface_batch_matches_stock_git -- --exact --nocapture`,
`cargo test -p zmin-cli --test git_sequencer_compat cherry_pick_and_revert_expansion_batch_matches_stock_git -- --exact --nocapture`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(cherry-pick|revert|summary)\t'`,
and `git diff --check`.

Actual durable census after this batch:

- complete command matrices: `148 / 151`
- complete documented command-option pairs: `2159 / 3212`
- represented documented command-option pairs: `2178 / 3212`
- matrix rows: `6319`
- verified rows: `5507`
- invalid-input rows: `787`
- open or partial exact rows: `0`

Per-command position on the touched surface:

- `cherry-pick`: `24 / 24` reviewed-complete documented option pairs,
  `24 / 24` represented documented option pairs, `35` written rows, `35`
  classified rows, `33` stock-matching rows, `2` invalid-input rows, and `0`
  exact-open rows
- `revert`: `19 / 19` reviewed-complete documented option pairs, `19 / 19`
  represented documented option pairs, `31` written rows, `31` classified
  rows, `29` stock-matching rows, `2` invalid-input rows, and `0`
  exact-open rows

The next best high-throughput follow-up should move to the fully represented
`commit` head instead of staying on the sequencer family, because `commit` now
has the largest remaining reviewed-complete gap among represented helper-free
surfaces at `46 / 58` complete versus `58 / 58` represented.

As of 2026-06-27 the latest completed batch is a helper-free local `apply`
reviewed-complete closure on the tracked patch, add-file redirect, tracked-plus-added,
and invalid-input inaccurate-eof lanes. This batch promoted the existing exact
stock-Git matrix evidence for `--include`, `--exclude`, `--directory`,
`--intent-to-add`, `-N`, `--no-add`, `--inaccurate-eof`, and `-3` into the
reviewed-complete layer, finishing the full represented `apply` surface except
for the still-unrepresented `--build-fake-ancestor` seed.

The batch closed one cohesive census/evidence gap rather than adding new runtime
behavior:

- Zmin already matched stock Git on the covered helper-free `apply` family, and
  the remaining work here was to lift the existing matrix rows and compat-test
  evidence into reviewed-complete doc-option pairs plus the reviewed-complete
  command matrix so the represented surface is counted correctly

Focused verification was
`cargo test -p zmin-cli --test git_apply_compat apply_documented_open_option_batch_matches_stock_git -- --exact --nocapture`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(apply|summary)\t'`,
and `git diff --check`.

Actual durable census after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `2124 / 3212`
- represented documented command-option pairs: `2178 / 3212`
- matrix rows: `6319`
- verified rows: `5507`
- invalid-input rows: `787`
- open or partial exact rows: `0`

Per-command position on the touched surface:

- `apply`: `37 / 38` reviewed-complete documented option pairs, `37 / 38`
  represented documented option pairs, `64` written rows, `64` classified
  rows, `58` stock-matching rows, `6` invalid-input rows, and `0`
  exact-open rows

The next best high-throughput follow-up should move back to the represented
sequencer head instead of staying on `apply`, because the remaining `apply`
tail is only the unrepresented `--build-fake-ancestor` seed while
`cherry-pick` and `revert` still have dense helper-free expansion families
with large represented-versus-reviewed gaps.

As of 2026-06-27 the latest completed batch is a helper-free local `grep`
documented-option closure on the tracked mixed-case, grouped-output, capped
count, files-without-match, and subdirectory-scope lanes. This batch added
exact stock-Git matrix evidence for `-i`, `--ignore-case`, `-v`,
`--invert-match`, `-c`, `--count`, `-L`, `--files-without-match`, `-m 1`,
`-m 1 -c`, `--max-count 1`, `--max-count 1 -c`, `-H`, `--heading`,
`--break`, `--heading --break`, the `-L hello` all-files-match exit-status
lane, the `-c absent` no-match exit-status lane, plus default and
`--full-name` output from a subdirectory.

The batch fixed one cohesive local grep parity gap:

- Zmin now matches stock Git for the covered helper-free `grep` family by
  accepting the documented local option spellings above, applying ignore-case,
  invert-match, capped count, files-without-match and grouped-output behavior
  with stock-compatible stdout and exit codes, and honoring current-subdirectory
  default search scope plus `--full-name` path rendering from inside nested
  directories

Focused verification was `cargo check -p zmin-cli`,
`cargo test -p zmin-cli --test git_grep_compat -- --nocapture`,
`python3 tools/git-existing-oracle-inventory.py --root . > docs/cli/existing_oracle_test_inventory.tsv`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(grep|summary)\t'`,
and `git diff --check`.

Actual durable census after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `2103 / 3212`
- represented documented command-option pairs: `2178 / 3212`
- matrix rows: `6319`
- verified rows: `5507`
- invalid-input rows: `787`
- open or partial exact rows: `0`

Per-command position on the touched surface:

- `grep`: `21 / 73` reviewed-complete documented option pairs, `36 / 73`
  represented documented option pairs, `36` written rows, `36` classified
  rows, `36` stock-matching rows, `0` invalid-input rows, and `0`
  exact-open rows

The next best high-throughput follow-up should move to the refreshed `apply`
head instead of staying on `grep`, because the newly represented local grep
family is now closed on the current modeled surface while `apply` still has a
dense helper-free expansion tail centered on `--directory`, include/exclude,
`--no-add`, `--intent-to-add`, `--inaccurate-eof`, and `-3/--3way`.

As of 2026-06-27 the latest completed batch is a helper-free proof-only
`commit` parser-and-expansion batch on the modeled verify, include,
pathspec-file, no-signoff, no-post-rewrite, and patch quit lanes. This batch
added broader stock-Git matrix evidence for repeated `--verify`, repeated
`--include`, separate and repeated `--pathspec-from-file` forms, separate and
repeated `--pathspec-file-nul` forms, repeated `--no-signoff`, repeated
`--no-post-rewrite`, repeated `--patch`, and mixed `--patch -p`.

The batch fixed one cohesive parser gap on the helper-free `commit` path:

- Zmin now accepts repeated `commit` boolean spellings on the covered
  verify, include, no-signoff, no-post-rewrite, and patch lanes, and it now
  accepts repeated or separate `pathspec-from-file` spellings with the current
  last-one-wins dispatch needed to match stock Git on the modeled LF and NUL
  pathspec-file lanes

Focused verification was `cargo check -p zmin-cli`,
`cargo test -p zmin-cli --test git_commit_compat -- --nocapture`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-existing-oracle-inventory.py --root . > docs/cli/existing_oracle_test_inventory.tsv`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(commit|summary)\t'`,
and `git diff --check`.

Actual durable census after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `2103 / 3212`
- represented documented command-option pairs: `2157 / 3212`
- matrix rows: `6287`
- verified rows: `5475`
- invalid-input rows: `787`
- open or partial exact rows: `0`

Per-command position on the touched surface:

- `commit`: `46 / 58` reviewed-complete documented option pairs, `58 / 58`
  represented documented option pairs, `115` written rows, `115` classified
  rows, `110` stock-matching rows, `5` invalid-input rows, and `0`
  exact-open rows

The next best high-throughput follow-up should reselect from the refreshed
backlog head instead of treating this as a completion point. This batch
materially expanded the `commit` exact evidence layer, but the
reviewed-complete numerator did not move; the next dense helper-free follow-up
can either keep pushing the represented `commit` tail or return to the larger
sequencer expansion head with the refreshed harnesses.

As of 2026-06-27 the latest completed batch is a helper-free proof-only
`cherry-pick` and `revert` follow-up schema-tail closure on the modeled clean,
initially-empty, becomes-empty, and reference-message lanes. This batch
finished representation for all documented `cherry-pick` and `revert` option
pairs by adding exact stock-Git matrix evidence for `cherry-pick
--allow-empty-message`, `--allow-empty`, `--keep-redundant-commits`,
`--empty=keep`, and long `--strategy-option=patience`, plus `revert
--reference` and long `--strategy-option=patience`.

The batch fixed one cohesive parser-plus-runtime gap on the sequencer path:

- Zmin now exposes the remaining documented schema tail for the modeled
  `cherry-pick` empty-policy family and long strategy-option spelling, plus the
  `revert --reference` message template and long strategy-option spelling; on
  the bounded covered lanes it now matches stock Git for committing initially
  empty cherry-picks, preserving become-empty picks with `--empty=keep`, and
  formatting `revert --reference` with the stock placeholder title line and
  reference-style reverted commit body

Focused verification was `cargo check -p zmin-cli`,
`cargo test -p zmin-cli --test git_sequencer_compat -- --nocapture`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(cherry-pick|revert|summary)\t'`,
and `git diff --check`.

Actual durable census after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `2100 / 3212`
- represented documented command-option pairs: `2157 / 3212`
- matrix rows: `6249`
- verified rows: `5439`
- invalid-input rows: `785`
- open or partial exact rows: `0`

Per-command position on the touched surface:

- `cherry-pick`: `4 / 24` reviewed-complete documented option pairs, `24 / 24`
  represented documented option pairs, `28` written rows, `28` classified
  rows, `26` stock-matching rows, `2` invalid-input rows, and `0`
  exact-open rows
- `revert`: `4 / 19` reviewed-complete documented option pairs, `19 / 19`
  represented documented option pairs, `24` written rows, `24` classified
  rows, `22` stock-matching rows, `2` invalid-input rows, and `0`
  exact-open rows

The next best high-throughput follow-up should reselect from the refreshed
backlog head rather than stay on schema closure. `cherry-pick` and `revert`
now have full documented-option representation, so the remaining work on these
commands is expansion-only tails inside already represented families.

As of 2026-06-27 the latest completed batch is a helper-free proof-only
`cherry-pick` and `revert` documented-option surface closure on the modeled
clean commit, editor, and signed-commit lanes. This batch added exact
stock-Git matrix evidence for `cherry-pick --ff`, `-x`, `-r`, `--signoff`,
`-s`, `--edit`, `-e`, `--cleanup=strip|scissors`,
`--rerere-autoupdate`, `--no-rerere-autoupdate`, `--strategy=ort`,
`-Xpatience`, `--gpg-sign`, `-S`, and `--no-gpg-sign`, plus `revert -r`,
`--signoff`, `-s`, `--no-edit`, `--edit`, `-e`,
`--cleanup=strip|scissors`, `--rerere-autoupdate`,
`--no-rerere-autoupdate`, `--strategy=ort`, `-Xpatience`, `--gpg-sign`,
`-S`, and `--no-gpg-sign`.

The batch fixed one cohesive parser-plus-runtime gap on the sequencer path:

- Zmin now exposes the remaining modeled `cherry-pick` and `revert` surface
  for fast-forward picks, record-origin trailers, signoff, editor-backed
  messages, cleanup modes, rerere toggles, strategy passthrough, and explicit
  GPG signing or signing suppression, while reusing the existing
  `commit-tree` signing implementation for the signed-commit family and
  matching the local stock-Git `revert --edit` summary nuance where the
  editor-backed lane omits the `Date:` line

Focused verification was `cargo check -p zmin-cli`,
`cargo test -p zmin-cli --test git_sequencer_compat -- --nocapture`,
`tools/git-sequencer-gpg-oracle-smoke.sh`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(cherry-pick|revert|summary)\t'`,
and `git diff --check`.

Actual durable census after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `2100 / 3212`
- represented documented command-option pairs: `2150 / 3212`
- matrix rows: `6242`
- verified rows: `5432`
- invalid-input rows: `785`
- open or partial exact rows: `0`

Per-command position on the touched surface:

- `cherry-pick`: `19 / 24` reviewed-complete documented option pairs, `23 / 24`
  represented documented option pairs, `23` written rows, `23` classified
  rows, `21` stock-matching rows, `2` invalid-input rows, and `0`
  exact-open rows
- `revert`: `17 / 19` reviewed-complete documented option pairs, `19 / 19`
  represented documented option pairs, `22` written rows, `22` classified
  rows, `20` stock-matching rows, `2` invalid-input rows, and `0`
  exact-open rows

The next best high-throughput follow-up should reselect from the refreshed
backlog head rather than stay on this parser closure. Both commands now have
only narrow schema tails or expansion-only work at the head, not another
similarly dense safe closure batch.

As of 2026-06-27 the latest completed batch is a helper-free proof-only
`commit` documented-option surface closure on the tracked-path, hook, patch,
and signed-commit lanes. This batch finished representation for all documented
`commit` option pairs by adding exact stock-Git matrix evidence for
`--verify`, `--include`, `-i`, `--pathspec-from-file`, `--pathspec-file-nul`,
`--no-signoff`, `--no-post-rewrite`, `--patch`, `-p`, `--gpg-sign`, `-S`,
and `--no-gpg-sign`.

The batch fixed one cohesive parser-plus-runtime gap on the `commit` path:

- Zmin now exposes the remaining documented `commit` surface for include-mode
  path staging, pathspec-file loading, verify/no-signoff/no-post-rewrite
  toggles, patch quit-lane prompting, and explicit GPG signing or signing
  suppression, while reusing the existing `commit-tree` signing implementation
  for the signed commit family and matching stock Git on the modeled fixture
  key lane

Focused verification was
`cargo test -p zmin-cli --test git_commit_compat -- --nocapture`,
`tools/git-commit-gpg-oracle-smoke.sh`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(commit|summary)\t'`,
and `git diff --check`.

Actual durable census after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `2100 / 3212`
- represented documented command-option pairs: `2122 / 3212`
- matrix rows: `6211`
- verified rows: `5401`
- invalid-input rows: `785`
- open or partial exact rows: `0`

Per-command position on the touched surface:

- `commit`: `46 / 58` reviewed-complete documented option pairs, `58 / 58`
  represented documented option pairs, `105` written rows, `105` classified
  rows, `100` stock-matching rows, `5` invalid-input rows, and `0`
  exact-open rows

The next best high-throughput follow-up should reselect from the refreshed
backlog head rather than stay on parser closure. `commit` no longer has schema
gaps; its remaining head is expansion-only tails on the newly represented
option family.

As of 2026-06-27 the latest completed batch is a helper-free proof-only
`apply` documented-option representation closure on the tracked-patch and
add-file lanes. This batch added exact stock-Git matrix evidence for
`--directory`, `--include`, `--exclude`, `--intent-to-add`, `--no-add`,
`--inaccurate-eof`, and the short `-3` alias.

The batch fixed one cohesive parser-plus-behavior gap on the `apply` path:

- Zmin now exposes the missing `apply` parser surface for path-routing,
  intent-to-add, no-add, inaccurate-eof, and short `-3`, and it matches the
  current local stock Git side effects for the modeled helper-free lanes,
  including the current `/usr/bin/git` `--intent-to-add` behavior that rewrites
  touched paths into intent-to-add index entries on this host

Focused verification was
`cargo test -p zmin-cli --test git_apply_compat -- --nocapture`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(apply|summary)\t'`,
and `git diff --check`.

Actual durable census after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `2100 / 3212`
- represented documented command-option pairs: `2110 / 3212`
- matrix rows: `6199`
- verified rows: `5389`
- invalid-input rows: `785`
- open or partial exact rows: `0`

Per-command position on the touched surface:

- `apply`: `30 / 38` reviewed-complete documented option pairs, `37 / 38`
  represented documented option pairs, `54` written rows, `54` classified
  rows, `49` stock-matching rows, `5` invalid-input rows, and `0`
  exact-open rows

The next best high-throughput follow-up should move off `apply`. The remaining
`apply` head is now narrow: the lone unrepresented `--build-fake-ancestor`
seed plus expansion-only tails on the newly represented option family. Reselect
the next dense non-`apply` helper-free batch from the refreshed backlog head.

As of 2026-06-27 the latest completed batch is a helper-free proof-only
`am` represented-tail closure on the modeled add-file, fixed-date, and
conflict lanes. This batch finished representation for all documented `am`
option pairs by adding exact stock-Git matrix evidence for
`--directory=subdir`, `--ignore-date`, and the conflicting `--reject` lane.

The batch fixed one cohesive matrix-evidence gap on the `am` path:

- Zmin already matched the stock helper-free behavior for the remaining narrow
  `am` tail, and the missing work was to record those lanes in the `am`
  behavior matrix so the census could promote `--directory` and
  `--ignore-date` from implemented-but-unverified to represented and count the
  `--reject` conflict lane alongside the earlier clean-lane proof

Focused verification was
`cargo check -p zmin-cli`,
`cargo test -p zmin-cli --test git_mail_series_compat -- --nocapture`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(am|summary)\t'`,
and `git diff --check`.

Actual durable census after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `2100 / 3212`
- represented documented command-option pairs: `2103 / 3212`
- matrix rows: `6192`
- verified rows: `5383`
- invalid-input rows: `784`
- open or partial exact rows: `0`

Per-command position on the touched surface:

- `am`: `51 / 54` reviewed-complete documented option pairs, `54 / 54`
  represented documented option pairs, `86` written rows, `86` classified
  rows, `63` stock-matching rows, `23` invalid-input rows, and `0`
  exact-open rows

The next best high-throughput follow-up should move off `am`. The remaining
`am` documented-option seeds are now narrow expansion tails for
`--directory`, `--ignore-date`, and `--reject`, while the refreshed backlog
head shifts to denser non-`am` families such as the open `apply` seeds.

As of 2026-06-27 the latest completed batch is a helper-free proof-only
`am` parser and passthrough closure on the modeled single-mail lane. This
batch added eleven reviewed-complete documented option pairs by proving
stock-Git parity for `--include`, `--exclude`, the full modeled
`--patch-format` family, `--gpg-sign`, `--no-gpg-sign`, `-S`,
`--rerere-autoupdate`, `--no-rerere-autoupdate`, `--resolvemsg`,
`--interactive`, and `-i`.

The batch fixed one cohesive parser/runtime gap on the `am` path:

- Zmin now exposes the next dense parser-plus-surface `am` family on the
  existing helper-free single-mail lane, including path-filter acceptance for
  `include` and `exclude`, the remaining modeled `patch-format` spellings,
  stock interactive no-tty failure, and the accepted gpg, rerere, and
  resolvemsg spellings on the current host

Focused verification was
`cargo check -p zmin-cli`,
`cargo test -p zmin-cli --test git_mail_series_compat -- --nocapture`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(am|summary)\t'`,
and `git diff --check`.

Actual durable census after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `2100 / 3212`
- represented documented command-option pairs: `2101 / 3212`
- matrix rows: `6189`
- verified rows: `5381`
- invalid-input rows: `783`
- open or partial exact rows: `0`

Per-command position on the touched surface:

- `am`: `51 / 54` reviewed-complete documented option pairs, `52 / 54`
  represented documented option pairs, `83` written rows, `83` classified
  rows, `61` stock-matching rows, `22` invalid-input rows, and `0`
  exact-open rows

The next best high-throughput follow-up should stay on `am`, but the head is
now narrow: `--directory`, `--ignore-date`, and the remaining `--reject`
expansion tail. After that, `am` should be ready either for reviewed-complete
command closure or to fall behind a denser non-`am` batch from the refreshed
backlog head.

As of 2026-06-27 the latest completed batch is a helper-free proof-only
`am` empty-mail and value-family closure. This batch added three
reviewed-complete documented option pairs by proving stock-Git parity for the
full modeled `--quoted-cr` family, the modeled `--empty` family, and the
stateful `--allow-empty` resume path, while also adding represented
proof-only rows for `--patch-format=mbox` and `--patch-format=hg`.

The batch fixed one cohesive parser/runtime gap on the `am` path:

- Zmin now handles the modeled empty-mail lane end to end: `--empty=keep`
  creates the stock empty commit, `--empty=drop` skips the message,
  `--empty=stop` stops in `rebase-apply` with stock stdout and stderr,
  `--allow-empty` resumes that session into the stock empty commit, and the
  stopped empty-mail session now matches stock `show-current-patch`,
  `continue`/`resolved`/`retry`, and cleanup behavior

Focused verification was
`cargo check -p zmin-cli`,
`cargo test -p zmin-cli --test git_mail_series_compat -- --nocapture`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(am|summary)\t'`,
and `git diff --check`.

Actual durable census after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `2089 / 3212`
- represented documented command-option pairs: `2091 / 3212`
- matrix rows: `6177`
- verified rows: `5372`
- invalid-input rows: `780`
- open or partial exact rows: `0`

Per-command position on the touched surface:

- `am`: `40 / 54` reviewed-complete documented option pairs, `42 / 54`
  represented documented option pairs, `71` written rows, `71` classified
  rows, `52` stock-matching rows, `19` invalid-input rows, and `0`
  exact-open rows

The next best high-throughput follow-up should stay on `am`, but move off
this empty-mail closure into the still-open documented-option head:
remaining `--patch-format` value expansion, then the still-unrepresented
`--directory`, `--include`, `--exclude`, `--ignore-date`, `--gpg-sign`,
`--no-gpg-sign`, `--rerere-autoupdate`, `--no-rerere-autoupdate`,
`--resolvemsg`, `--interactive`/`-i`, and `-S` families.

As of 2026-06-27 the latest completed batch is a helper-free proof-only
`am` active-session closure on the modeled single conflicting mail lane. This
batch added two represented documented option pairs and promoted both to
reviewed-complete by proving stock-Git parity for accepted `am -s` and `-r`,
plus exact active-session behavior for conflicting `am <patch>`,
`--show-current-patch=raw|diff`, `--retry`, `--continue`, `--resolved`,
`--skip`, `--abort`, and `--quit`.

The batch fixed one cohesive parser/runtime gap on the `am` path:

- Zmin now persists a narrow `rebase-apply` session for the modeled
  single-patch conflict lane, exposes the missing short aliases `-s` and `-r`,
  and matches the current stock-Git stdout, stderr, exit code, and cleanup
  behavior for the current unresolved-session resume family on that lane

Focused verification was
`cargo check -p zmin-cli`,
`cargo test -p zmin-cli --test git_mail_series_compat -- --nocapture`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(am|summary)\t'`,
and `git diff --check`.

Actual durable census after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `2086 / 3212`
- represented documented command-option pairs: `2091 / 3212`
- matrix rows: `6160`
- verified rows: `5360`
- invalid-input rows: `775`
- open or partial exact rows: `0`

Per-command position on the touched surface:

- `am`: `37 / 54` reviewed-complete documented option pairs, `42 / 54`
  represented documented option pairs, `54` written rows, `54` classified
  rows, `40` stock-matching rows, `14` invalid-input rows, and `0`
  exact-open rows

The next best high-throughput follow-up should stay on `am`, but move off
this active-session closure into the remaining semantic tails that still sit
at the head of `remaining_to_fix_or_verify.tsv`: `--allow-empty`,
`--empty=keep`, `--quoted-cr` value completion, `--patch-format` value
completion, and then the still-unrepresented `directory`/`ignore-date`/
`gpg-sign` families.

As of 2026-06-27 the latest completed batch is a helper-free proof-only
`am` alias-and-passthrough closure on the same single-mail stock
format-patch lane. This batch added twelve represented documented option
pairs and promoted all twelve to reviewed-complete by proving stock-Git
parity for accepted `am -u`, `--keep-non-patch`, `-m`, `--scissors`, `-c`,
`--no-scissors`, `--whitespace=warn`, `-C1`, `-p1`, `--no-verify`, `-n`,
and `--committer-date-is-author-date`.

The batch fixed one cohesive parser/runtime gap on the `am` path:

- Zmin now exposes the next dense `am` alias and passthrough family on the
  existing clean single-mail lane, including stock apply passthrough for
  `--whitespace`, `-C`, and `-p`, stock acceptance for the scissors and
  hook-bypass spellings on a no-scissors/no-hooks lane, and stock
  committer-date rewriting when `--committer-date-is-author-date` is used

Focused verification was
`cargo check -p zmin-cli`,
`cargo test -p zmin-cli --test git_mail_series_compat -- --nocapture`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(am|summary)\t'`,
and `git diff --check`.

Actual durable census after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `2084 / 3212`
- represented documented command-option pairs: `2089 / 3212`
- matrix rows: `6149`
- verified rows: `5354`
- invalid-input rows: `770`
- open or partial exact rows: `0`

Per-command position on the touched surface:

- `am`: `35 / 54` reviewed-complete documented option pairs, `40 / 54`
  represented documented option pairs, `43` written rows, `43` classified
  rows, `34` stock-matching rows, `9` invalid-input rows, and `0`
  exact-open rows

The next best high-throughput follow-up should stay on `am`, but move off
this alias-and-passthrough closure into the remaining semantic tails:
`--quoted-cr`, `--patch-format`, `--empty=keep`, active-session
`rebase-apply` resume flows, and then the more stateful
`directory`/`ignore-date`/`gpg-sign` families that still sit near the head of
`remaining_to_fix_or_verify.tsv`.

As of 2026-06-27 the latest completed batch is a helper-free proof-only
`am` option-surface closure on the single-mail stock format-patch lane. This
batch added twenty-eight represented documented option pairs and promoted
twenty-three of them to reviewed-complete by proving stock-Git parity for
accepted `am --quiet`, `-q`, `--utf8`, `--no-utf8`, `--keep`, `-k`,
`--signoff`, `--keep-cr`, `--no-keep-cr`, `--message-id`,
`--no-message-id`, `--quoted-cr=strip`, `--3way`, `-3`, `--no-3way`,
`--ignore-space-change`, `--ignore-whitespace`, `--patch-format=mboxrd`,
`--empty=stop`, `--empty=drop`, and `--reject`, plus stock-compatible
no-session rejection for `am --allow-empty`, `--abort`, `--quit`, `--skip`,
`--continue`, `--resolved`, `--retry`, and `--show-current-patch=raw|diff`.

The batch fixed one cohesive parser/runtime gap on the `am` path:

- Zmin now exposes a broad proof-only `am` surface on the existing clean
  single-mail lane, including stock quiet suppression, keep-subject handling,
  signoff trailer emission, reject stderr progress, and the current stock
  no-session fatal path for resume-only flags when `rebase-apply` state does
  not exist

Focused verification was
`cargo check -p zmin-cli`,
`cargo test -p zmin-cli --test git_mail_series_compat -- --nocapture`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(am|summary)\t'`,
and `git diff --check`.

Actual durable census after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `2072 / 3212`
- represented documented command-option pairs: `2077 / 3212`
- matrix rows: `6138`
- verified rows: `5343`
- invalid-input rows: `770`
- open or partial exact rows: `0`

Per-command position on the touched surface:

- `am`: `23 / 54` reviewed-complete documented option pairs, `28 / 54`
  represented documented option pairs, `32` written rows, `32` classified
  rows, `23` stock-matching rows, `9` invalid-input rows, and `0`
  exact-open rows

The next best high-throughput follow-up should stay on `am`, but move off the
newly closed proof-only parser tail and into the remaining value-bearing and
stateful semantics: `--quoted-cr`/`--patch-format`/`--empty` tails, active
session flows such as `--abort`/`--continue` with real `rebase-apply` state,
and the remaining author-date/directory/scissors families still sitting near
the head of `remaining_to_fix_or_verify.tsv`.

As of 2026-06-27 the latest completed batch is a helper-free proof-only
`apply` option-surface closure. This batch added twenty-five documented option
closures by proving stock-Git parity for accepted tracked-stdin patch flags
`apply --allow-empty`, `--allow-binary-replacement`, `--apply`, `--binary`,
`--recount`, `--quiet`, `-q`, `--unsafe-paths`, `--unidiff-zero`,
`--ignore-space-change`, `--ignore-whitespace`, `--whitespace=warn`, `-p1`,
`-C1`, `-z`, `--verbose`, `-v`, `--reject`, and `--3way`, plus output-only
parity for `apply --stat`, `--numstat`, and `--summary`, and stock-compatible
fatal rejection for `apply --ours`, `--theirs`, and `--union`.

The batch fixed one cohesive parser/runtime gap on the `apply` path:

- Zmin now exposes a broad proof-only `apply` surface on the existing
  helper-free tracked stdin patch lane, including stock stderr progress for
  `--verbose` and `--reject`, stock stdout formatting for `--stat`,
  `--numstat`, and `--summary`, and the stock `--3way` fallback lane that
  materializes index-plus-worktree state instead of bare worktree-only apply

Focused verification was
`cargo check -p zmin-cli`,
`cargo test -p zmin-cli --test git_apply_compat apply_proof_only_option_surface_batch_matches_stock_git -- --nocapture`,
`cargo test -p zmin-cli --test git_apply_compat -- --nocapture`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(apply|summary)\t'`,
and `git diff --check`.

Actual durable census after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `2049 / 3212`
- represented documented command-option pairs: `2049 / 3212`
- matrix rows: `6108`
- verified rows: `5322`
- invalid-input rows: `761`
- open or partial exact rows: `0`

Per-command position on the touched surface:

- `apply`: `30 / 38` reviewed-complete documented option pairs, `47`
  written rows, `47` classified rows, `43` stock-matching rows, `4`
  invalid-input rows, `0` exact-open rows

The next best high-throughput follow-up should move off this newly narrowed
`apply` tail. The refreshed `remaining_to_fix_or_verify.tsv` head is still
dominated by dense `am` doc-option seeds, while `apply` is now down to the
more semantic path/filter tail rather than another large proof-only surface.

As of 2026-06-27 the latest completed batch is a helper-free proof-only
`shortlog` history-tail closure. This batch added eighteen documented option
closures by proving stock-Git parity for accepted summary-lane flags
`shortlog --alternate-refs`, `--bisect`, `--cherry`, `--count`, `--dense`,
`--full-history`, `--glob=main`, `--in-commit-order`, `--expand-tabs`, and
`--show-linear-break`, plus stock-compatible unknown-option rejection for
`shortlog --bisect-all`, `--bisect-vars`, `--commit-header`,
`--disk-usage`, `--single-worktree`, `--filter=blob:none`,
`--filter-print-omitted`, and `--filter-provided-objects`.

The batch fixed one cohesive parser/runtime gap on the `shortlog` path:

- Zmin now exposes the represented proof-only shortlog history tail that stock
  Git already accepts or rejects on the modeled helper-free summary lane, and
  the parameterized `--filter=...` rejection now preserves the full stock
  unknown-option token instead of collapsing to bare `--filter`

Focused verification was
`cargo check -p zmin-cli`,
`cargo test -p zmin-cli --test git_history_query_compat shortlog_proof_only_history_tail_batch_matches_stock_git -- --nocapture`,
`cargo test -p zmin-cli --test git_history_query_compat -- --nocapture`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(shortlog|summary)\t'`,
and `git diff --check`.

Actual durable census after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `2024 / 3212`
- represented documented command-option pairs: `2024 / 3212`
- matrix rows: `6083`
- verified rows: `5300`
- invalid-input rows: `758`
- open or partial exact rows: `0`

Per-command position on the touched surface:

- `shortlog`: `114 / 125` reviewed-complete documented option pairs, `146`
  written rows, `146` classified rows, `123` stock-matching rows, `23`
  invalid-input rows, `0` exact-open rows

The next best high-throughput follow-up should move off `shortlog` again. The
refreshed `remaining_to_fix_or_verify.tsv` head is back to dense stateful
`am` and `apply` doc-option seeds, so the default next queue should be
reselected there rather than continuing into smaller proof-only shortlog tails.

As of 2026-06-27 the latest completed batch closes the final two shared
`log` schema-tail doc-option seeds by proving stock-compatible rejection for
`log --object-names` and `log --timestamp`. This was a narrow helper-free
invalid-input closure on the existing object-selector family, not a schema or
feature expansion batch.

The batch fixed one concrete parser/runtime mismatch on the `log` path:

- Zmin no longer treats `--object-names` and `--timestamp` as revision-like
  arguments on `log`; both now fail early with the same stock
  `unrecognized argument` fatal that Git 2.47.1 emits

Focused verification was
`cargo test -p zmin-cli --test git_history_query_compat log_object_and_selector_family_matches_stock_git -- --nocapture`,
`cargo test -p zmin-cli --test git_history_query_compat -- --nocapture`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(log|summary)\t'`,
and `git diff --check`.

Actual durable census after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `1939 / 3212`
- represented documented command-option pairs: `1939 / 3212`
- matrix rows: `5996`
- verified rows: `5231`
- invalid-input rows: `740`
- open or partial exact rows: `0`

Per-command position on the touched surface:

- `log`: `84 / 199` reviewed-complete documented option pairs, `131` written
  rows, `84` classified rows on the reviewed-complete surface, `187`
  stock-matching rows overall, `12` invalid-input rows, `0` exact-open rows

The next best high-throughput follow-up should now move off this narrowed `log`
tail entirely, because the remaining shared history doc-option seeds are no
longer on `log`. The default next queue should be reselected from the updated
top of `docs/cli/census/remaining_to_fix_or_verify.tsv`, with the next densest
helper-free family likely on `shortlog` or another represented command cluster
rather than `log`.

As of 2026-06-27 the latest completed batch closes a shared helper-free
history schema/runtime family across `log` and `rev-list`. This batch added
ten documented-option closures by implementing and proving stock-Git parity
for `log --do-walk`, `log --max-age`, `log --min-age`, `log --skip`,
`rev-list --do-walk`, `rev-list --max-age`, `rev-list --min-age`,
`rev-list --skip`, `rev-list --timestamp`, and
`rev-list --object-names`.

The batch fixed one cohesive parser/runtime gap on the shared history-query
surface:

- Zmin now accepts the represented `--do-walk`, `--max-age`, `--min-age`, and
  `--skip` spellings on the shared `log` / `rev-list` path with stock-like
  alias precedence and skip ordering; `rev-list` also now matches stock Git
  for explicit `--object-names` on the objects lane and for `--timestamp`
  output on the default commit-id lane

Focused verification was
`cargo check -p zmin-cli`,
`cargo test -p zmin-cli --test git_history_query_compat log_and_rev_list_shared_history_schema_batch_matches_stock_git -- --nocapture`,
`cargo test -p zmin-cli --test git_history_query_compat -- --nocapture`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(log|rev-list|summary)\t'`,
and `git diff --check`.

Actual durable census after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `1937 / 3212`
- represented documented command-option pairs: `1937 / 3212`
- matrix rows: `5994`
- verified rows: `5231`
- invalid-input rows: `738`
- open or partial exact rows: `0`

Per-command position on the touched shared surface:

- `log`: `82 / 197` reviewed-complete documented option pairs, `131` written
  rows, `82` classified rows on the reviewed-complete surface, `187`
  stock-matching rows overall, `10` invalid-input rows, `0` exact-open rows
- `rev-list`: `81 / 133` reviewed-complete documented option pairs, `117`
  written rows, `81` classified rows on the reviewed-complete surface, `123`
  stock-matching rows overall, `10` invalid-input rows, `0` exact-open rows

The next best high-throughput follow-up remains on the shared history schema
tail, but it is now sharply narrowed. The default next queue should decide the
fate of the last two `log` documented options still absent from Zmin schema in
the current census, `--object-names` and `--timestamp`, since the rest of this
helper-free batch is now fully represented and reviewed-complete.

As of 2026-06-27 the latest completed batch is a history-compat stabilization
pass on the shared `log` / `rev-list` / `whatchanged` surface. This batch did
not add new matrix rows or documented-option coverage; instead it restored the
behavioral baseline needed for the next high-throughput schema closure pass by
fixing three regressions exposed by the focused
`git_history_query_compat` suite and by removing one flaky stock-oracle test
assumption.

The batch fixed two concrete Zmin behavior mismatches and one test-harness
issue:

- `log --walk-reflogs --grep-reflog=... --format=%gd %gs` now renders
  reflog placeholders from the reflog entry again instead of treating them as
  plain commit-format placeholders
- `whatchanged` once again requires the explicit
  `--i-still-use-this` opt-in on hosts where stock Git has nominated the
  command for removal
- the history-simplification compat test now uses a deterministic merge commit
  timestamp, eliminating a flaky cross-repository SHA mismatch that was not a
  real Zmin runtime bug

Focused verification was
`cargo test -p zmin-cli --test git_history_query_compat log_grep_reflog_requires_walk_reflogs_and_matches_stock_git -- --nocapture`,
`cargo test -p zmin-cli --test git_history_query_compat log_reflog_relative_date_and_notes_aliases_match_stock_git -- --nocapture`,
`cargo test -p zmin-cli --test git_history_query_compat log_and_rev_list_history_simplification_acceptance_family_matches_stock_git -- --nocapture`,
`cargo test -p zmin-cli --test git_history_query_compat whatchanged_requires_explicit_opt_in_like_git_2_54 -- --nocapture`,
`cargo test -p zmin-cli --test git_history_query_compat -- --nocapture`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(log|rev-list|whatchanged|summary)\t'`,
and `git diff --check`.

Actual durable census after this batch is unchanged:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `1927 / 3212`
- represented documented command-option pairs: `1927 / 3212`
- matrix rows: `5984`
- verified rows: `5221`
- invalid-input rows: `738`
- open or partial exact rows: `0`

Per-command position on the touched shared surface:

- `log`: `131 / 78` written rows, `78` classified rows, `193 / 193`
  represented documented options, `183` stock-matching rows, `10`
  invalid-input rows, `0` exact-open rows
- `rev-list`: `117 / 75` written rows, `75` classified rows, `127 / 127`
  represented documented options, `117` stock-matching rows, `10`
  invalid-input rows, `0` exact-open rows
- `whatchanged`: `0 / 0` written rows, `0` classified rows, `29 / 29`
  represented documented options, `29` stock-matching rows, `0`
  invalid-input rows, `0` exact-open rows

The next best high-throughput follow-up remains a helper-free documented-option
schema batch rather than more history-runtime work. The best bounded queue is
the shared history schema tail visible in
`docs/cli/census/remaining_to_fix_or_verify.tsv` (`log`/`rev-list`
`--skip`, `--stdin`, `--timestamp`, `--left-only`, `--right-only`,
`--mailmap`, `--show-signature`, and nearby flags), because the behavior suite
is green again and counts are stable.

As of 2026-06-26 the latest completed batch closes the final exact-open
helper/oracle tail: the top-level `scalar` no-subcommand row plus the two
modeled `git svn` rows (`clone`, `dcommit`). The selected change did not add
new matrix rows; instead it corrected the stale top-level `scalar` probe to use
the standalone stock `scalar` binary rather than `git scalar`, added a focused
stock-oracle test for `zmin scalar`, and reclassified the two `svn` rows as
closed after verifying them against a real stock `git-svn` oracle through a
local `ZMIN_STOCK_GIT` x86 wrapper for the Homebrew helper build.

The batch fixed two evidence/oracle gaps rather than changing modeled Zmin
runtime behavior:

- the stale `scalar` root probe now measures standalone stock `scalar`
  no-subcommand behavior, which matches `zmin scalar` exactly on this host
- the existing fake-SVN oracle tests now close the represented `git svn clone`
  and `git svn dcommit` rows when run against a real stock `git-svn` helper
  through the local x86 wrapper

Actual durable census after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `1927 / 3212`
- represented documented command-option pairs: `1927 / 3212`
- matrix rows: `5984`
- verified rows: `5221`
- invalid-input rows: `738`
- open or partial exact rows: `0`

Per-command position on the active shared surface:

- `scalar`: `19 / 19` written rows, `19` classified rows, `19`
  stock-matching rows, `0` invalid-input rows, `0` exact-open rows
- `svn`: `3 / 3` written rows, `3` classified rows, `3`
  stock-matching rows, `0` invalid-input rows, `0` exact-open rows

The next best high-throughput follow-up should move off oracle unblock work and
back to the largest safe documented-option/schema backlog, because the exact
open-row tail is now gone and represented documented-option coverage is fully
closed at `1927/1927`. The default next queue should start with the largest
helper-free schema family in `remaining_to_fix_or_verify.tsv`, currently
`git am`, unless a denser reviewed-complete family emerges from the next census
pass.

As of 2026-06-26 the latest completed batch is the final helper-like
`git update-index --split-index` documented-option tail on the modeled
single-entry local lane. The selected change added one exact stock-Git row for
`update-index --split-index`, then promoted that represented documented option
into `docs/cli/census/reviewed_complete_doc_option_pairs.tsv`.

The batch fixed one remaining concrete runtime gap on the update-index path:

- Zmin now writes a stock-compatible split index for the modeled lane by
  creating `sharedindex.*`, emitting the lowercase `link` extension with the
  expected bitmap payload, stripping the main-index placeholder entry name like
  stock Git, and narrowly reading that single-entry split-index shape back
  through the shared base so follow-up `ls-files` and `status` behavior stays
  stock-compatible where modeled

Actual durable census after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `1919 / 3212`
- represented documented command-option pairs: `1927 / 3212`
- matrix rows: `5984`
- verified rows: `5210`
- invalid-input rows: `738`
- open or partial exact rows: `12`

Per-command position on the active shared surface:

- `update-index`: `38 / 38` reviewed-complete documented option pairs, `69`
  written rows, `69` classified rows, `61` stock-matching rows, `8`
  invalid-input rows, `0` exact-open rows

The next best high-throughput follow-up should move off `update-index` and back
to the next dense represented helper-free family outside it. The best default
queue is another reviewed-complete promotion batch on the shared history-query
surface (`log` / `rev-list`) or whichever represented command cluster now has
the largest safe zero-code or low-code closure.

As of 2026-06-26 the latest completed batch is a helper-free `git update-index`
helper-extension documented-option expansion on the current extensionless local
lane. The selected change added five exact stock-Git rows for
`update-index --force-untracked-cache`, `update-index --fsmonitor`,
`update-index --fsmonitor-valid a.txt`,
`update-index --no-fsmonitor-valid a.txt`, and
`update-index --untracked-cache`, then promoted those five newly represented
documented options into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`.

The batch fixed one concrete runtime gap on the update-index path:

- Zmin now accepts the remaining helper-like local `update-index` cache and
  fsmonitor options except split-index, including stock warning output for
  `--fsmonitor` and stock-compatible synthetic `FSMN` / `UNTR`
  index-extension writes on the modeled extensionless local lane, while
  `--fsmonitor-valid` and `--no-fsmonitor-valid` now match stock Git's
  accepted no-op behavior on that same lane

Actual durable census after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `1918 / 3212`
- represented documented command-option pairs: `1926 / 3212`
- matrix rows: `5983`
- verified rows: `5209`
- invalid-input rows: `738`
- open or partial exact rows: `12`

Per-command position on the active shared surface:

- `update-index`: `37 / 38` reviewed-complete documented option pairs, `68`
  written rows, `68` classified rows, `60` stock-matching rows, `8`
  invalid-input rows, `0` exact-open rows

The next best high-throughput follow-up should isolate the final
`update-index --split-index` tail. It is no longer grouped with the other
cache/fsmonitor families because stock Git writes the required lowercase
`link` extension and creates `sharedindex.*`, which Zmin still cannot read or
manage. The next slice should either add real split-index/shared-index support
or explicitly defer it with evidence rather than mixing it into helper-free
local batches.

As of 2026-06-26 the latest completed batch is a helper-free `git update-index`
disable-helper-toggle documented-option expansion on the current local lane.
The selected change added four exact stock-Git rows for
`update-index --no-split-index`, `update-index --no-untracked-cache`,
`update-index --test-untracked-cache`, and `update-index --no-fsmonitor`, then
promoted those four newly represented documented options into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`.

The batch fixed one concrete parser/runtime gap on the update-index path:

- Zmin now accepts the helper-disable local `update-index` toggles
  `--no-split-index`, `--no-untracked-cache`, `--test-untracked-cache`, and
  `--no-fsmonitor`, with stock-matching silent no-op behavior for the disable
  forms and the stock stderr mtime probe report for `--test-untracked-cache`
  on the modeled single-file local lane

Actual durable census after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `1913 / 3212`
- represented documented command-option pairs: `1921 / 3212`
- matrix rows: `5978`
- verified rows: `5204`
- invalid-input rows: `738`
- open or partial exact rows: `12`

Per-command position on the active shared surface:

- `update-index`: `32 / 38` reviewed-complete documented option pairs, `63`
  written rows, `63` classified rows, `55` stock-matching rows, `8`
  invalid-input rows, `0` exact-open rows

The next best high-throughput follow-up should stay on the final six
`update-index` documented tails, which are now only
`--force-untracked-cache`, `--fsmonitor`, `--fsmonitor-valid`,
`--no-fsmonitor-valid`, `--split-index`, and `--untracked-cache`. Treat them
as one batched helper-like closure attempt: start from stock-Git oracle probes
on the current local lane, then either close the densest safe subset in one
test-first pass or explicitly defer the stateful tails if they would require
non-trivial shared-index or UNTR/FSMN extension modeling.

As of 2026-06-26 the latest completed batch is a helper-free `git update-index`
skip-worktree/remove plus submodule-refresh documented-option expansion on the
current local lane. The selected change added three exact stock-Git rows for
`update-index --refresh --ignore-submodules submod`,
`update-index --remove --ignore-skip-worktree-entries a.txt`, and
`update-index --remove --no-ignore-skip-worktree-entries a.txt`, then promoted
three newly represented documented options into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`.

The batch fixed three concrete runtime gaps on the update-index/status path:

- `update-index --ignore-skip-worktree-entries` now preserves missing
  skip-worktree entries during remove mode, matching stock Git index and
  status side effects
- `update-index --no-ignore-skip-worktree-entries` now explicitly restores the
  stock default remove behavior for missing skip-worktree entries
- status and worktree snapshots now ignore missing skip-worktree entries,
  eliminating false deleted-path reporting on index-only lanes and aligning the
  resulting observable state with stock Git

Actual durable census after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `1909 / 3212`
- represented documented command-option pairs: `1917 / 3212`
- matrix rows: `5974`
- verified rows: `5200`
- invalid-input rows: `738`
- open or partial exact rows: `12`

Per-command position on the active shared surface:

- `update-index`: `28 / 38` reviewed-complete documented option pairs, `59`
  written rows, `59` classified rows, `51` stock-matching rows, `8`
  invalid-input rows, `0` exact-open rows

The next best helper-free follow-up should stay on the remaining `update-index`
tail, with the more helper-like `fsmonitor`, `split-index`, and
`untracked-cache` families now standing out as the main unresolved documented
surface unless another safe represented local lane appears first.

As of 2026-06-26 the latest completed batch is a helper-free `git update-index`
refresh-family documented-option expansion on the current local lane. The
selected change added five exact stock-Git rows for dirty tracked
`update-index --refresh`, `update-index --refresh -q`,
`update-index --refresh --ignore-missing`,
`update-index --refresh --unmerged`, and the short alias `update-index -g`,
then promoted four newly represented documented options into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`.

The batch fixed one concrete runtime gap on the update-index path:

- `update-index --refresh` no longer mutates the index by restaging or
  removing tracked entries; it now reports stock-style `needs update` /
  `needs merge` lines and exits `1` while leaving index plus worktree state
  unchanged on the modeled dirty, missing and conflicted lanes

Actual durable census after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `1901 / 3212`
- represented documented command-option pairs: `1909 / 3212`
- matrix rows: `5963`
- verified rows: `5190`
- invalid-input rows: `737`
- open or partial exact rows: `12`

Per-command position on the active shared surface:

- `update-index`: `20 / 38` reviewed-complete documented option pairs, `48`
  written rows, `48` classified rows, `41` stock-matching rows, `7`
  invalid-input rows, `0` exact-open rows

The next best helper-free follow-up should stay on `update-index` and harvest
the remaining safe local documented-option tail before switching back to the
helper-blocked foreign-SCM commands. Start by stock-oracle confirming the next
non-helper local family such as `--verbose`, `--info-only`,
`--index-version`, `--show-index-version`, and `--unresolve`, and continue to
defer the helper-like `fsmonitor`, `split-index`, and `untracked-cache`
families until there is evidence they can close densely on the current lane.

As of 2026-06-26 the latest completed batch is a helper-free `git worktree`
remote-inference plus unborn-branch documented-option tail closure on the
current local lane. The selected change added five exact stock-Git rows for
`worktree add --orphan`, `worktree add --track`, `worktree add --no-track`,
`worktree add --guess-remote`, and `worktree add --no-guess-remote`, then
promoted those five represented documented options into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv`.

The batch fixed three concrete runtime gaps on the worktree add path:

- `worktree add --orphan <path>` now creates the stock unborn branch named
  from the path basename and leaves the linked worktree empty
- `worktree add -b <branch> [--no-]track <path> <remote>/<branch>` now
  preserves the stock upstream-config behavior for remote-tracking starts
- `worktree add --guess-remote` and `--no-guess-remote` now parse correctly,
  with unique remote-tracking inference enabled or suppressed like stock Git

Actual durable census after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `1897 / 3212`
- represented documented command-option pairs: `1905 / 3212`
- matrix rows: `5958`
- verified rows: `5185`
- invalid-input rows: `737`
- open or partial exact rows: `12`

Per-command position on the active shared surface:

- `worktree`: `24 / 24` reviewed-complete documented option pairs, `33`
  written rows, `33` classified rows, `32` stock-matching rows, `1`
  invalid-input row, `0` exact-open rows

The next best helper-free follow-up should move to the next dense local
documented-option family outside worktree. Current census evidence points to
`update-index` as the largest remaining helper-free local tail, rather than
the helper-blocked `cvsimport` / `cvsexportcommit` / `archimport` gaps.

As of 2026-06-26 the latest completed batch is a zero-code shared
history-query reviewed-complete promotion for `git log` and
`git rev-list`. The selected change promoted fourteen already-represented
documented option pairs into
`docs/cli/census/reviewed_complete_doc_option_pairs.tsv` after confirming
they already had exact helper-free stock-Git evidence on the current modeled
local lanes: `--no-standard-notes`, `--quiet`, `--reflog`,
`--relative-date`, `--show-notes`, `--show-notes-by-default`, and
`--standard-notes` for both commands.

Actual durable census after this promotion:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `1881 / 3212`
- represented documented command-option pairs: `1889 / 3212`
- matrix rows: `5942`
- verified rows: `5169`
- invalid-input rows: `737`
- open or partial exact rows: `12`

Per-command position on the active shared surface:

- `log`: `78 / 131` reviewed-complete documented option pairs, `193` written
  rows, `193` classified rows, `183` stock-matching rows, `10` invalid-input
  rows, `0` exact-open rows
- `rev-list`: `75 / 117` reviewed-complete documented option pairs, `127`
  written rows, `127` classified rows, `117` stock-matching rows, `10`
  invalid-input rows, `0` exact-open rows

The next best helper-free follow-up should keep the same census-first method:
take the next densest represented shared history-query family that still lacks
reviewed-complete promotion or needs additional exact rows, rather than
switching back to isolated one-row closures.

As of 2026-06-26 the latest completed batch is a zero-code shared history
matrix-shape cleanup for `git log`. The selected change repaired two malformed
`docs/cli/matrices/log_v2_47.tsv` rows where `repo_state` / `combination`
text had been split across an extra tab, which previously suppressed durable
classification for already-verified helper-free `log` lanes.

Actual durable census after this cleanup:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `1867 / 3212`
- represented documented command-option pairs: `1889 / 3212`
- matrix rows: `5942`
- verified rows: `5169`
- invalid-input rows: `737`
- open or partial exact rows: `12`

Per-command position on the active shared surface:

- `log`: `71 / 131` reviewed-complete documented option pairs, `193` written
  rows, `193` classified rows, `183` stock-matching rows, `10` invalid-input
  rows, `0` exact-open rows
- `rev-list`: `68 / 117` reviewed-complete documented option pairs, `127`
  written rows, `127` classified rows, `117` stock-matching rows, `10`
  invalid-input rows, `0` exact-open rows

The next best helper-free follow-up still stays on the shared history-query
surface; this cleanup only made prior evidence durable and did not consume the
remaining represented `log` / `rev-list` doc-option tails.

As of 2026-06-26 the latest completed batch is a helper-free shared
`git log`/`git rev-list` date-order plus notes-precedence expansion on the
current local lane. The selected change added seventeen exact stock-Git rows
for order-sensitive `--date=iso` plus `--relative-date` lanes, `--quiet`
plus relative-date formatting, `--reflog` plus `--relative-date` pretty lanes,
and additional `show-notes` / `show-notes-by-default` / `standard-notes` /
`no-standard-notes` custom-format precedence combinations across `log` and
`rev-list`.

The batch also fixed two shared runtime gaps:

- raw CLI ordering for `--date` versus `--relative-date` now survives through
  the history command path, so last-one-wins matches stock Git
- `rev-list --reflog ... --relative-date` no longer treats `--relative-date`
  as a reflog target during target extraction

Actual durable census after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `1867 / 3212`
- represented documented command-option pairs: `1889 / 3212`
- matrix rows: `5942`
- verified rows: `5167`
- invalid-input rows: `737`
- open or partial exact rows: `12`

Per-command position on the active shared surface:

- `log`: `71 / 131` reviewed-complete documented option pairs, `193` written
  rows, `191` classified rows, `181` stock-matching rows, `10` invalid-input
  rows, `0` exact-open rows
- `rev-list`: `68 / 117` reviewed-complete documented option pairs, `127`
  written rows, `127` classified rows, `117` stock-matching rows, `10`
  invalid-input rows, `0` exact-open rows

The next best helper-free follow-up still stays on the shared history-query
surface: keep closing the remaining represented `log` / `rev-list` doc-option
tails before switching back to unrelated command families.

As of 2026-06-26 the latest completed batch is a helper-free shared
`git log`/`git rev-list` reflog-plus-notes precedence expansion on the current
local lane. The selected change added eight exact stock-Git rows for
`log --show-notes-by-default --no-standard-notes`,
`log --standard-notes --show-notes`, `log --quiet --reflog`,
`log --reflog --date=relative --pretty=format:%gd|%ad|%cd`,
`rev-list --quiet --reflog`,
`rev-list --reflog --date=relative --pretty=format:%gd|%ad|%cd`,
`rev-list --standard-notes --show-notes`, and
`rev-list --show-notes-by-default --no-standard-notes`.

Actual durable census after this batch:

- complete command matrices: `146 / 151`
- complete documented command-option pairs: `1867 / 3212`
- represented documented command-option pairs: `1889 / 3212`
- matrix rows: `5925`
- verified rows: `5153`
- invalid-input rows: `735`
- open or partial exact rows: `12`

Per-command position on the active shared surface:

- `log`: `71 / 131` reviewed-complete documented option pairs, `184` written
  rows, `183` classified rows, `173` stock-matching rows, `10` invalid-input
  rows, `0` exact-open rows
- `rev-list`: `68 / 117` reviewed-complete documented option pairs, `119`
  written rows, `119` classified rows, `111` stock-matching rows, `8`
  invalid-input rows, `0` exact-open rows

The next best helper-free follow-up stays on the shared history-query family:
harvest the densest remaining represented `log`/`rev-list` tails before
switching back to isolated command families.

## Counting Rules

- Count only stock-Git-supported behavior unless there is an explicit Zmin-only
  command.
- Do not count corrupt repository formats, reftable, Git LFS, legacy external
  bridges or package-manager install channels as closed unless they are
  implemented and tested.
- A variant is closed only with focused parity evidence: local compat test,
  upstream Git test slice or dogfood reproduction.
- Public docs may show command-name presence. They must not call it full support.
- Public docs must show `0/151` complete command matrices until a command's
  full matrix is finished.
- Written rows are a work queue and evidence log. They are not the denominator
  until the command matrix is fully expanded.
- Summary counts are generated by `tools/git-compat-audit-summary.sh`. Keep
  `docs/cli/git_reference_groups.tsv` and
  `docs/cli/git_audit_primary_groups.tsv` in sync when adding a command group
  or moving a closed behavior block between reference groups.

## Completion Rule

`100%` compatibility can only be claimed for a command after its matrix covers:

- every documented option spelling for the Git baseline
- every documented value or mode for those options
- positive and negative toggle forms where Git has them
- repeated options and last-one-wins cases
- order-sensitive option combinations
- positional modes and pathspec forms
- clean, dirty, conflicted, bare, submodule, shallow and worktree states where
  the command behaves differently
- local/file, smart HTTP, SSH and git-daemon transports where the command uses
  transport
- macOS, Linux and Windows behavior where paths, process spawning, hooks,
  line endings or permissions can differ
- upstream Git tests and real tool traces that expose behavior not obvious from
  documentation

Each row needs stock Git evidence for stdout, stderr, exit code and repository
state when those are observable. Parser acceptance does not close a row.

If new gaps appear during dogfood, as happened with `status -z`, they become
new matrix rows first. Only then should implementation and tests follow.

## Inventory Expansion Plan

Work proceeds in this order for each command group:

1. Extract command names from Git `v2.47.1`.
2. Extract documented option spellings from `Documentation/git-*.txt`.
3. Expand each option spelling into behavior rows:
   value modes, no-toggle forms, repeated options, option order, positional
   forms, pathspec/refspec forms and invalid values accepted or rejected by
   stock Git.
4. Add repository-state rows where behavior differs:
   clean, dirty, conflicted, bare, detached HEAD, shallow, submodule,
   linked worktree, sparse checkout and ignored/untracked state.
5. Add transport rows for network commands:
   local path, `file://`, smart HTTP, SSH, git daemon, bundle, depth,
   partial clone, tags, prune, auth and proxy behavior.
6. Add platform rows where paths, executable lookup, hooks, permissions,
   symlinks, line endings or process spawning differ between macOS, Linux and
   Windows.
7. Import upstream Git test cases and real tool traces after docs expansion,
   because tools such as IDEs often combine options in ways docs do not make
   obvious.
8. Implement only the classified rows. Mark a row `closed` only after stock
   Git parity evidence covers output, exit code and repository state.

The first pass for a command is not expected to finish implementation. It must
first expose the real denominator: commands, option spellings, option values,
combinations, states, transports, platforms, upstream test cases and observed
tool traces.

## Matrix States

Each command matrix has one of these states:

| State | Meaning |
| --- | --- |
| `not-started` | no command-specific behavior matrix exists |
| `seeded` | documented option spellings are known but not expanded |
| `expanding` | option values, combinations, states, transports or platforms are being added |
| `implementing` | open rows exist and Zmin behavior is being changed |
| `verifying` | implementation is present but upstream tests or platform checks are missing |
| `complete` | every documented and discovered behavior row has stock-Git parity evidence |

`complete` is the only state that can contribute to command-level `100%`
compatibility.

## Reporting Format

Progress reports use these numbers:

`complete command matrices / complete doc-option matrices / commands with matrix rows / represented doc-option pairs / written behavior rows / written rows matching stock Git / partial written rows / open written rows`

For the current branch:

`146/151 complete command matrices / 1867/3212 complete doc-option matrices / 155/151 commands with matrix rows / 1889/3212 represented doc-option pairs / 5917 written rows / 5147/5917 written rows matching stock Git / 0 partial written rows / 12 open written rows`

Represented doc-option pairs still do not mean support. They only mean at
least one behavior row exists for that documented option spelling. One option
spelling can expand into many behavior rows. A complete doc-option matrix
requires values, negations, repeated forms, order-sensitive combinations,
repository states, transports and platforms with stock-Git evidence. The final
denominator exists only after the expansion plan above is done for that
command.

## Stepwise Operating Plan

Use this as the working queue. Each step should land as a small commit with its
own stock-Git evidence row instead of bundling unrelated commands.

### Resume Checklist

When resuming work, use this checklist before editing code:

1. Read this section, then the `Current Next Slice Pointer`.
2. Check the active goal in the Codex thread and keep it aligned with the
   durable objective below.
3. Run `/usr/bin/git status --short --branch` and preserve unrelated staged or
   unstaged work.
4. Pick one row-sized slice only. If no row exists yet, add or update the
   matrix row before implementation.
5. Probe stock Git for the exact command line, stdout, stderr, exit code and
   observable repository side effects.
6. Add focused oracle evidence for that row.
7. Implement the smallest code change needed for that row.
8. Run the focused test, then the relevant build and count gates.
9. Update generated counts in README, `git_compatibility_inventory.md`, this
   plan and project notes.
10. Commit locally before starting another slice. Push only on explicit
    request.

Do not start from the raw `unsupported` scan alone. The scan only finds source
guards; each guard still needs classification against stock Git.

### Canonical Files

Use these files as the handoff map:

| File | Purpose |
| --- | --- |
| `docs/cli/git_compatibility_census.md` | census-first entry point and generated bucket-list index |
| `docs/cli/census/*.tsv` | machine-readable verified, invalid-input, implemented-unverified, remaining, extension/deferred and evidence-layer lists |
| `docs/cli/git_compatibility_inventory.md` | compatibility counting model and current denominator layers |
| `docs/cli/variant_compatibility_plan.md` | operating plan, active queue, current slice pointer and guard mappings |
| `docs/cli/existing_oracle_test_inventory.tsv` | generated inventory of stock-oracle test functions; evidence layer only |
| `docs/cli/matrix_row_growth_audit.md` | row-count growth audit and required predeclared row-growth budget |
| `docs/cli/matrices/*_v2_47.tsv` | row-level command/option/value/state/transport evidence |
| `docs/cli/zmin_extensions_inventory.md` | Zmin-only commands/options kept outside Git compatibility counts |
| `README.md` | user-facing status with honest non-100% compatibility numbers |
| `/Users/dschewchenko/work/private/.knowledge/projects/skron-core.md` | cross-session project memory and latest slice notes |

### Required Gates

For a normal row-sized slice, run the narrowest useful set:

1. The focused oracle test for the row.
2. The command-specific compat test file when the touched parser or renderer is
   shared inside that command.
3. `cargo check -p zmin-cli --bin zmin --profile compat`.
4. `tools/git-cli-readiness-status.sh`.
5. `tools/git-compat-command-summary.sh --tsv`.
6. `tools/git-compat-audit-summary.sh --tsv`.

For transport slices, include the relevant HTTP/SSH/git-daemon focused test.
For replacement-binary slices, include `tools/git-replacement-dogfood-smoke.sh`.

### Durable Objective

Drive Zmin toward honest Git `2.47.1` compatibility through small verified
slices. A slice closes only when the command, option, value, option
combination, repository state, transport or workflow, expected stdout/stderr,
exit code and observable `.git` side effects match stock Git or are explicitly
classified as stock-compatible invalid input.

The durable target is not `151/151` command dispatch. It is complete matrices
for commands, documented options, option values, meaningful combinations,
transport/local modes, edge cases, invalid inputs, side effects and oracle
evidence. Until those denominators are complete, public docs must keep complete
command matrices at `0/151` and complete doc-option matrices at `0/4632`.

### Execution Loop

Repeat this loop until the full Git `2.47.1` matrix is closed:

1. Pick exactly one slice from the Immediate Slice Queue.
2. Write or update the behavior row before relying on implementation work.
3. Probe stock Git for stdout, stderr, exit code and repository side effects.
4. Add focused oracle evidence for that exact row.
5. Implement only the missing behavior for that row.
6. Before adding rows from any inventory source, declare the source bucket and
   expected row-count delta from `docs/cli/matrix_row_growth_audit.md`.
7. Run the focused evidence, build and count gates listed below.
8. Update README, inventory, this plan and project notes with generated counts.
9. Commit locally before switching to another command, option class or lane.
   Push only on explicit request.

If actual row growth differs from the declared bucket, stop and explain the
difference before committing. Do not let `written behavior rows` grow as an
incidental side effect of reading another test file.

If a new IDE/tool trace appears, insert it at the top of the queue as a
behavior row first. Do not replace this loop with broad ad-hoc test runs.

### Slice Definition of Done

Every compatibility slice must finish these items:

1. Add or update the matrix row with command, option, value, combination,
   repository state, transport or workflow, platform, example invocation,
   status, evidence and notes.
2. Add a focused oracle test against stock Git, or name the upstream Git test or
   dogfood trace that proves stdout, stderr, exit code and repository side
   effects.
3. Implement only the row being closed, unless the failing behavior shares the
   same narrow parser or transport path.
4. Run the focused test first, then the smallest relevant build or summary
   gates.
5. Update README, `git_compatibility_inventory.md`, this plan and project notes
   with the generated counts.
6. Commit locally before starting a different command or option class. Push
   only on explicit request.

### Active Queue

| Order | Lane | Next slices | Done when |
| ---: | --- | --- | --- |
| 1 | WebStorm replacement blockers | `status`, `log`, `diff`, `ls-files`, `rev-parse`, `config` rows observed from IDE dogfood, especially `-z`, date/format values, null output and pathspec combinations | replacement smoke and focused rows pass through the `git` shim |
| 2 | Unsupported-guard classification | classify each `unsupported`/`not supported` Rust hit as Git-supported gap, invalid input, intentional deferral or Zmin-only behavior | every hit maps to a matrix row, deferral note or invalid-input test |
| 3 | Command inventory expansion | expand high-use commands from docs into option values, negations, repeated forms, order-sensitive combinations and repo states | command state reaches at least `expanding`; no support percentage is published |
| 4 | Platform and upstream evidence | macOS/Linux/Windows checks plus selected upstream Git test slices for rows already implemented | rows that depend on platform behavior have platform evidence before being called closed |
| 5 | Zmin-only extensions | keep hooks staged-file runner and other extensions below Git compatibility reporting | extension rows stay in the Zmin-only inventory, not the Git 2.47 denominator |

### Immediate Slice Queue

This queue is the handoff point when work resumes. Do not start a later item
until the earlier item has a matrix row, stock-Git evidence, generated count
updates, project-note update and a local commit when the slice is substantial.
Push only on explicit request.

| Order | Slice | Required evidence | Required docs/counts |
| ---: | --- | --- | --- |
| 1 | Continue WebStorm replacement blockers from observed command lines | focused replacement smoke rows for `status`, `log`, `diff`, `ls-files`, `rev-parse` or `config`, one behavior shape per commit | add rows before implementation; keep complete matrices at `0/151` and complete doc-option matrices at `0/4632` |
| 2 | Classify Rust `unsupported` and `not supported` guards | each guard maps to a Git-supported gap, stock-compatible invalid input, intentional deferral or Zmin-only extension | add matrix rows, invalid-input rows or deferral notes instead of leaving raw source hits ambiguous |
| 3 | Expand one high-use command matrix from Git docs | documented options split into values, negations, repeated/order-sensitive forms, positional modes and repository states | mark the command as `expanding`; do not publish a support percentage |
| 4 | Add upstream/platform evidence for already-written rows | selected upstream Git tests or macOS/Linux/Windows checks for rows where platform or upstream behavior matters | only rows with matching stock-Git evidence may remain closed |
| 5 | Design Zmin-only hooks staged-file runner | API and behavior rows for staged index files, extension/pathspec filters, renamed/deleted files, dry-run/list and hook-wrapper mode | record under Zmin-only extensions, not Git 2.47 compatibility |

### Current Next Slice Pointer

The next default slice is census-first, not row-import-first.

Start from `docs/cli/git_compatibility_census.md` and the generated
`docs/cli/census/*.tsv` files. Refresh the census with
`tools/git-compat-census.py` if command docs, Zmin schema, matrices, extension
docs, deferrals or source guards changed. Use:

- `docs/cli/census/verified_behavior.tsv` as the exact safe-to-skip list.
- `docs/cli/census/invalid_input_parity.tsv` as the exact verified rejection
  list.
- `docs/cli/census/implemented_but_unverified.tsv` for parser/handler surfaces
  that need stock-Git evidence before they count.
- `docs/cli/census/remaining_to_fix_or_verify.tsv` as the primary checklist
  for new fix/verify slices.
- `docs/cli/census/zmin_extension_or_deferred.tsv` for items outside the Git
  `2.47.1` denominator.

Only after selecting an exact row or coherent expansion group from the census
should work consult `docs/cli/existing_oracle_test_inventory.tsv` to find
whether an existing focused oracle test can serve as evidence. Do not add more
matrix rows merely because an oracle test is `missing_or_unclassified`; that
was the source of the previous unbounded-loader workflow.

If a new WebStorm, replacement-shim or real-tool blocker appears, add it as a
census/matrix row shape first, then use stock Git to close it. Otherwise, pick
one small checklist item from `remaining_to_fix_or_verify.tsv`, declare the
source bucket and expected row/status delta in
`docs/cli/matrix_row_growth_audit.md`, and only then edit matrices or code.

The latest completed slice is a helper-free local `daemon` tail closure across
the existing git:// localhost serving lane. Zmin now matches stock Git for the
final six documented `daemon` option pairs on the current bounded surface:
`--access-hook`, `--detach`, `--group`, `--interpolated-path`, `--user`, and
`--user-path`. The runtime closure stayed intentionally bounded: this slice
accepts `--access-hook`, `--detach`, and `--user-path` on the current export-all
lane, reproduces the stock startup fatals for `--group` and `--user`, and
reproduces the stock access-denied request lane for `--interpolated-path` on a
static `/remote.git` request. It does not widen into actual privilege dropping,
hook-driven authorization, or generalized interpolated-path expansion beyond the
modeled helper-free local serving lane. Focused gates were
`GIT_BIN=/Users/dschewchenko/.cache/codex-runtimes/codex-primary-runtime/dependencies/bin/git ZMIN_BIN=/private/tmp/skron-codex-target/debug/zmin bash tools/git-daemon-schema-oracle-smoke.sh`,
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo check -p zmin-cli -p zmin-cli-schema`,
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo build -p zmin-cli --bin zmin`,
`CARGO_TARGET_DIR=/private/tmp/skron-codex-target cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`, and `git diff --check`.
Actual delta from the prior replay tail batch is `+6` matrix rows, `+6`
complete documented option pairs, `+6` represented documented option pairs,
`+4` verified rows, `+2` invalid-input rows, and `+0` complete command
matrices. Current census counts are `7393` matrix rows, `6479` verified rows,
`889` invalid-input rows, `0` exact-open rows, `146/151` complete command
matrices, `2985/3212` complete documented option pairs, and `2985/3212`
represented documented option pairs. Per-command position on the touched
surface is now: `daemon` `27/27` reviewed-complete documented option pairs
with `28/28` classified written rows, `25` stock-matching rows, and `3`
invalid-input rows. The overall backlog head remains `send-email` with `62`
remaining documented option pairs, followed by `rebase` at `54`, `p4` at
`28`, `svn` at `25`, and `pack-objects` at `16`; the next default follow-up
should keep the largest safe helper-free batch bias and shift to one of those
remaining families.

The latest completed slice is a helper-free local `pull`/`merge`
commit-mode closure across the explicit local no-rebase merge lane and the
shared merge default-commit lane. Zmin now accepts and matches stock Git for
`pull --commit`, `pull --no-commit`, `pull --squash`, `pull --no-squash`,
`merge --commit`, and `merge --no-squash`. The concrete closure here is
split in two parts: `merge` gains the missing documented parser surface for
the explicit default toggles, while `pull` now threads the full
commit/squash toggle family into merge dispatch with stock-like raw-argv
last-one-wins resolution and uses the merge engine when explicit merge-mode
flags are present on non-fast-forward local pulls.
Focused gates were
`cargo test -p zmin-cli --test git_merge_compat merge_commit_and_no_squash_match_stock_git_merge_commit_state -- --exact --nocapture`,
`cargo test -p zmin-cli --test git_transport_local_compat pull_merge_commit_mode_flags_match_stock_git_for_explicit_local_branch -- --exact --nocapture`,
`cargo test -p zmin-cli --test git_transport_local_compat pull_local_path_ours_strategy_matches_stock_git_merge_commit -- --exact --nocapture`,
`cargo check -p zmin-cli -p zmin-cli-schema`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(pull|merge|summary)\t'`, and
`git diff --check`.
Actual delta from the prior `diff` color-word-diff closure is `+6` matrix
rows, `+6` complete documented option pairs, `+6` represented documented
option pairs, `+6` verified rows, `+0` invalid-input rows, and `+0` complete
command matrices. Current census counts are `6629` matrix rows, `5817`
verified rows, `787` invalid-input rows, `0` exact-open rows,
`146/151` complete command matrices, `2424/3212` complete documented option
pairs, and `2424/3212` represented documented option pairs. Per-command
position on the touched surface is now: `pull` `41/99` reviewed-complete
documented option pairs with `66/66` classified written rows and `64`
stock-matching rows; `merge` `10/51` reviewed-complete documented option
pairs with `24/24` classified written rows and `16` stock-matching rows. The
next default follow-up should reselect from the refreshed backlog head,
because this batch closes the shared commit/squash parser tail but leaves the
remaining `pull`/`merge` work as broader documented-option expansion.

The latest completed slice is a helper-free shared `diff*` parser-and-evidence
batch centered on `--output` and plumbing `--line-prefix`. Zmin now accepts
and matches stock Git for `diff --output`, `diff-files --output`,
`diff-index --output`, `diff-tree --output`, plus `diff-files --line-prefix`,
`diff-index --line-prefix`, and `diff-tree --line-prefix` on the modeled
two-commit and dirty-worktree patch lanes. The concrete closure here is split
in two parts: the new `--output` surface is handled as an early self-reexec
stdout redirect so the existing render pipeline stays unchanged, while the
plumbing `line-prefix` surface is now threaded through the shared diff render
options instead of stopping at the parser layer.
Focused gates were
`cargo test -p zmin-cli --test git_diff_compat diff_output_and_plumbing_line_prefix_match_stock_git -- --exact --nocapture`,
`cargo check -p zmin-cli`,
`python3 tools/git-existing-oracle-inventory.py --root . > docs/cli/existing_oracle_test_inventory.tsv`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(diff|diff-files|diff-index|diff-tree|summary)\t'`, and
`git diff --check`.
Actual delta from the prior `commit` GPG-sign expansion closure is `+7`
matrix rows, `+0` complete documented option pairs, `+7` represented
documented option pairs, `+7` verified rows, `+0` invalid-input rows, and
`+0` complete command matrices. Current census counts are `6299` matrix rows,
`5487` verified rows, `787` invalid-input rows, `0` exact-open rows,
`146/151` complete command matrices, `2103/3212` complete documented option
pairs, and `2164/3212` represented documented option pairs. Per-command
position on the touched surface is now: `diff` `84/117` reviewed-complete
documented option pairs with `251/251` classified written rows and `247`
stock-matching rows; `diff-files` `81/118` reviewed-complete documented
option pairs with `100/100` classified written rows and `100` stock-matching
rows; `diff-index` `80/112` reviewed-complete documented option pairs with
`107/107` classified written rows and `107` stock-matching rows; and
`diff-tree` `87/132` reviewed-complete documented option pairs with
`119/119` classified written rows and `119` stock-matching rows. The next
default follow-up should stay census-first and reselect from the refreshed
backlog head rather than keep drilling on `diff*`, because this batch removed
the dense shared parser tail but did not reduce the remaining expansion-only
heads for these commands.

The previous completed slice was a helper-free shared `git log`/`git rev-list`
history-simplification acceptance plus ancestry-path documented-option family
expansion on the current local lane. Zmin now accepts and matches stock Git
for `log --full-history`, `log --dense`, `log --sparse`,
`log --show-pulls`, `log --ancestry-path`, `rev-list --full-history`,
`rev-list --dense`, `rev-list --sparse`, `rev-list --show-pulls`, and
`rev-list --ancestry-path` on modeled helper-free merge-graph lanes,
including the current stock no-op acceptance surfaces for non-path-limited
history simplification toggles and ancestry-path filtering for both main-line
and side-line lower-bound ranges.
Focused gates were
`cargo test -p zmin-cli --test git_history_query_compat log_and_rev_list_history_simplification_acceptance_family_matches_stock_git -- --nocapture`,
`cargo test -p zmin-cli --test git_history_query_compat log_and_rev_list_traversal_order_family_matches_stock_git -- --nocapture`,
`cargo check -p zmin-cli --bin zmin --profile compat`,
`cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-cli-readiness-status.sh`,
`tools/git-compat-command-summary.sh --tsv | rg '^(log|rev-list|summary)\t'`, and
`git diff --check`.

Current counts are `146/151` complete command matrices,
`1853/3212` complete documented option pairs,
`1861/3212` represented documented option pairs, `5871` written rows,
`5110` verified rows, `12` open rows, and `724` invalid-input rows.
`rev-list` now sits at `61/117` reviewed-complete documented option pairs
with `89/89` classified rows, `88` stock-matching rows, `1` invalid-input
row, and `0` exact-open rows, while `log` remains at `64/131`
reviewed-complete documented option pairs with `160` written rows, `159`
classified rows, `153` stock-matching rows, `6` invalid-input rows, and `0`
exact-open rows. The next bounded high-throughput follow-up should keep
harvesting shared history-query represented families before switching back to
isolated tails.

The focused `git_object_plumbing_compat.rs`,
`git_transport_http_compat.rs`,
`git_clone_ref_format_compat.rs`, `git_scalar_compat.rs`,
`git_admin_tools_compat.rs`, `git_cms_porcelain_compat.rs`,
`git_clone_compat.rs`, `compatibility_command.rs`,
`git_fast_import_export_compat.rs`, `git_global_cli_compat.rs` and
all focused oracle buckets in `docs/cli/existing_oracle_test_inventory.tsv`
are now fully represented or classified. The next recommended batch still
keeps the exact-open queue visible, but the smallest local no-helper follow-up
is no longer `git column`; that local subgroup is closed. `git pack-refs` now
has all `5/5` documented options represented, `interpret-trailers` is reviewed
complete, the supported `gc` documented family is now reviewed complete too
(`5/5` represented documented option pairs), and the supported `read-tree`
documented family is now reviewed complete across its current schema surface
(`3/3` represented documented option pairs). The supported `checkout-index`
documented family is now reviewed complete across its current schema surface
too (`8/8` represented documented option pairs), and both `check-ignore`
(`9/9`) and `column` (`7/7`) are now reviewed-complete commands. `clean` is
now reviewed complete too across all `13/13` documented options with `68/68`
classified rows and `0` open after closing the last alias-only gaps for
`--force` and `-i`. `archive` now has its full documented option family
reviewed complete too (`14/14` represented documented option pairs) after
closing output-format inference, repeated prefix and add-file ordering, and
`--worktree-attributes` parity; the full command is still not promoted because
backend extra compression-level options remain outside the reviewed-complete
command matrix. `branch`, `status`, `rm`, `fsck`, `maintenance`, and
`merge-tree` are now also eligible zero-row reviewed-complete commands because
they already have `100%` represented documented options, `0` exact-open rows,
`0` implemented-but-unverified rows, and `100%` classified written-row
coverage. `bisect` is now also eligible because its only two documented option
surfaces `--first-parent` and `--no-checkout` already have exact stock-Git
evidence in the sequencer matrix. The next helper-free family should now move
to another census-backed supported subgroup unless work intentionally opens
unsupported surfaces in `checkout-index`, `read-tree`, `gc`, or `repack`.
The largest remaining zero-row command-only cluster is now `clone`, `stage`,
`push`, `init`, `sparse-checkout`, and `shell`: each has `0` documented
option seed rows, `0` remaining census rows, and `100%` classified written-row
coverage, so they can be promoted without changing Rust behavior. The exact
helper-backed foreign-SCM `git p4 submit` row is closed with a focused
stock-vs-Zmin oracle. The remaining exact-open tail in this environment is the
local-helper-unavailable batch tracked in
`docs/cli/census/exact_open_oracle_gaps.tsv`.

After the latest `scalar + svn` closure, the exact-open queue in this
environment is empty. `citool`, `scalar`, and `svn` are all fully classified on
their currently modeled surfaces, and `docs/cli/census/exact_open_oracle_gaps.tsv`
is now empty. The next bounded follow-up should therefore return to the
documented-option/schema backlog rather than oracle unblock work. The primary
`remaining_to_fix_or_verify.tsv` backlog is now led by `doc_option_not_in_zmin_schema`
rows for `git am`, while the only remaining implemented-but-unverified family
is still the lone schema-only `archive <positional:args>` parser surface.

### Latest Completed Slice

The latest completed slice closes the documented `git http-fetch`
`--index-pack-args` lane by matching stock Git's current rejection behavior:
the documented plural spelling is accepted by argument parsing but ignored on
the modeled helper path, so packfile mode still fails with the stock fatal
requiring `--index-pack-args`, and non-packfile mode falls back to the normal
usage rejection. With those exact invalid-input rows recorded, the slice then
promotes `http-fetch` into
`docs/cli/census/reviewed_complete_command_matrices.tsv`.

Focused gates were
`cargo test -p zmin-cli --test git_transport_http_compat http_fetch_documented_plural_index_pack_args_matches_stock_git_rejections -- --exact --nocapture`,
`cargo test -p zmin-cli --test git_transport_http_compat http_fetch_packfile_requires_index_pack_args_like_stock_git -- --exact --nocapture`,
`cargo test -p zmin-cli --test git_transport_http_compat http_fetch_packfile_rejects_bad_index_pack_arg_like_stock_git -- --exact --nocapture`,
`cargo test -p zmin-cli --test git_transport_http_compat http_fetch_packfile_downloads_and_indexes_pack -- --exact --nocapture`,
`cargo check -p zmin-cli --bin zmin --profile compat`,
`python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
`tools/git-compat-command-summary.sh --tsv | rg '^(http-fetch|summary)\t'`,
and `git diff --check`.

Current census counts are `5383` matrix rows, `4684` verified rows, `682`
invalid-input rows, `12` exact-open rows, `92/151` complete command
matrices before command promotion, `1423/3156` complete documented option
pairs, `1431/3156` represented documented option pairs, and `1744`
remaining checklist rows. `http-fetch` is now fully command-complete at
`9/9` reviewed-complete documented option pairs with `9/9` represented
documented option pairs, `14/14` classified rows, and `0` exact-open written
rows. The exact-open queue remains limited to the helper-oracle-unavailable
commands `citool`, `cvsimport`, `svn`, `archimport`, `cvsexportcommit`, and
`scalar`.

### No-Skip Rule

Every iteration must update the durable handoff before it is considered done:
matrix row, focused evidence, generated counts, README/inventory/plan/project
notes and a local commit when the slice is substantial. Push only on explicit
request. If any item is missing, the slice stays open even if the code happens
to pass the focused test.

The latest closed guard classification is `fetch` from an invalid bundle file.
Stock Git treats `git fetch bad.bundle HEAD:refs/heads/from-bundle` as an
unreadable remote repository, exits `128`, writes an empty `FETCH_HEAD`, leaves
the destination ref absent and installs no pack files. Zmin now maps that
fetch-from-bundle surface to the same diagnostics and side effects.

The latest source-scan classification pass covers the `pack.rs` /
`pack_impl.rs` raw `unsupported` hits. These hits stay in the code where stock
Git also rejects invalid input or corrupt storage formats; they are not open
Git-supported feature gaps as long as the mapped oracle rows keep passing.

The latest stock-compatible corrupt-format guard classification is
`runtime/commit_graph.rs` `unsupported commit-graph header`. This runtime
reader guard is mapped to the existing `commit-graph verify` header validation
rows for checksum-valid bad signature, version and hash-version inputs; stock
Git rejects those corrupt commit-graph files with exit `1`, so this source hit
is not an open user-facing Git feature gap.

The latest stock-compatible invalid-input guard slice is `core_impl.rs`
`objects filter not supported` parsing for unknown `cat-file --filter` names.
Stock Git distinguishes known-but-unsupported object-filter families such as
`tree`, `sparse:oid` and `combine` from arbitrary invalid filter names. Zmin
now keeps the known-unsupported usage path but maps unknown `bad:name` and
`bad=name` values to stock `fatal: invalid filter-spec` diagnostics with exit
`128`.

The latest Zmin-only guard classification is `reference_impl.rs`
`unsupported repo output format`. Stock Git has no `git repo` command, so this
guard is tracked under `docs/cli/zmin_extensions_inventory.md` instead of the
Git `2.47.1` matrix. The focused extension test covers `repo info`,
`repo structure`, NUL key output and invalid output-format validation.

The latest stock-compatible invalid-input guard classification is
`text_impl.rs` `unsupported option '{other}'` for `git column --mode=<value>`.
Stock Git rejects unsupported column mode tokens with exit `129` and the same
diagnostic, so the guard remains in code as parser validation and is mapped to
the existing `column_v2_47.tsv` invalid-input row.

The latest stock-compatible invalid-input guard classification is
`worktree_impl.rs` `unsupported porcelain version` handling for
`git status --porcelain=<value>`. Stock Git rejects
`git status --porcelain=v3` with exit `128` and a fatal unsupported-version
diagnostic, so this source hit is parser validation mapped to the existing
`status_v2_47.tsv` invalid-input row.

The latest deferred guard classification is now only the `git gui` external GUI
surface in `crates/zmin-cli/src/cli/commands/commit_impl.rs`. `git citool` is
no longer deferred in this environment: stock `git-gui` is now installed, the
focused `git_citool_compat::citool_helper_option_shapes_match_stock_git` oracle
proves launch/exit/side-effect parity for the modeled `--amend`, `--nocommit`,
`-m`, `--file`, and `-F` rows, and those matrix rows are closed. Keep the
remaining `git gui` surface out of closed Git compatibility counts until a real
GUI oracle or explicit product decision brings that broader surface into scope.

The latest platform-oracle deferral is `crates/zmin-git-core/src/checkout.rs`
non-UTF8 index path handling. The guard is compiled only on non-Unix targets.
The current macOS oracle host rejects a `bad-\xff.txt` filesystem path with
`Illegal byte sequence` before stock Git checkout behavior can be observed, so
this cannot be closed without a Windows/non-Unix oracle run.

The latest matrix inventory slice records the `replay` linear-range modes
already covered by
`git_admin_tools_compat::replay_matches_stock_git_for_linear_range`: plain range
usage failure, contained advance, fixed-date advance and `--onto` replay
forms. This expands the written `replay` matrix, but still does not make the
command matrix complete.

The latest stock-compatible invalid-input guard classification is
`import_impl.rs` `unsupported fast-import command`. Stock Git rejects an
unknown top-level fast-import stream command with exit `128`, prints
`fatal: Unsupported command: <command>` plus a crash-report path, and writes a
`.git/fast_import_crash_*` report. Zmin now matches that stderr shape and side
effect for the classified row.

The latest adjacent fast-import invalid-input classification is an unknown
command inside a commit record. Stock Git again exits `128`, writes
`.git/fast_import_crash_*`, leaves the destination ref absent, and still writes
the same unreferenced loose object count for the in-progress commit. Zmin now
matches that classified side-effect shape.

The latest fast-import option-value classification is
`--date-format=bogus`. Stock Git exits `128`, prints
`fatal: unknown --date-format argument bogus` plus a crash-report path, and
writes `.git/fast_import_crash_*`. Zmin now matches that invalid-input shape.

The latest stock-compatible invalid-input guard classification is
`worktree_impl.rs` `unsupported submodule subcommand`. Stock Git rejects an
unknown top-level `git submodule bogus` subcommand with exit `1`, empty stdout
and the full `git submodule` usage text on stderr. Zmin now routes that guard
to the same usage shape instead of a custom unsupported-subcommand fatal.

The latest stock-compatible invalid-input guard classification is
`worktree_impl.rs` `unsupported sparse-checkout subcommand`. Stock Git rejects
`git sparse-checkout bogus` with exit `129`, empty stdout, an
`error: unknown subcommand` diagnostic and the sparse-checkout usage text on
stderr. Zmin now routes that guard to the same invalid-input shape.

The latest stock-compatible invalid-input guard classification is
`worktree_impl.rs` `unsupported worktree subcommand`. Stock Git rejects
`git worktree bogus` with exit `129`, empty stdout, an
`error: unknown subcommand` diagnostic and the worktree usage text on stderr.
Zmin now routes that guard to the same invalid-input shape.

The latest stock-compatible invalid-input parser classification is
`worktree_impl.rs` `unknown sparse-checkout option` for
`git sparse-checkout set --bad`. Stock Git rejects the invalid set option with
exit `129`, empty stdout, `error: unknown option` and the command-specific
`sparse-checkout set` usage text on stderr. Zmin now routes that parser guard
to the same invalid-input shape.

The latest adjacent sparse-checkout parser classification is
`git sparse-checkout init --bad`. Stock Git rejects the invalid init option
with exit `129`, empty stdout, `error: unknown option` and the
command-specific `sparse-checkout init` usage text on stderr. Zmin matches that
invalid-input shape through the same parser usage path.

The latest sparse-checkout state-before-option classification is
`git sparse-checkout add --bad` in a non-sparse repository. Stock Git rejects
the repository state before parsing the invalid option, with exit `128`, empty
stdout and `fatal: no sparse-checkout to add to` on stderr. Zmin now checks the
non-sparse state before the add-option parser and matches that invalid-input
shape.

The latest sparse-checkout enabled-state option classification is also
`git sparse-checkout add --bad`, but after sparse-checkout is enabled. Stock
Git then reaches the add-option parser and rejects the option with exit `129`,
empty stdout, `error: unknown option` and the command-specific
`sparse-checkout add` usage text on stderr. Zmin matches that enabled-state
invalid-input shape.

The latest sparse-checkout supported-combination slice is `--stdin` with a
positional pattern for `set` and `add`. Stock Git does not reject the
combination; stdin patterns override positional patterns. Zmin now matches the
same exit code, stdout/stderr, sparse-checkout list and `ls-files -t` side
effects for the focused `set --stdin docs` and `add --stdin missing` rows.

The latest sparse-checkout supported-option slice is
`git sparse-checkout set --skip-checks docs`. Stock Git accepts the option for
a valid directory pattern with exit `0`, empty stdout/stderr, the `docs`
sparse-checkout list, matching `ls-files -t` skip-worktree bits and matching
visible working-tree files. Zmin now has the same focused oracle row.

The latest clean interactive guard classification is
`printf 'bogus\nq\n' | git clean --interactive`. Stock Git treats the unknown
interactive command as a recoverable prompt input: it prints `Huh (bogus)?`,
reprints the command menu, accepts the following quit input, exits `0`, leaves
stderr empty and preserves the untracked file. Zmin now matches that retry
flow instead of exiting through the former `unsupported clean interactive
command` fatal guard.

The latest stash list format guard classification is malformed `%x` hex
escapes. Stock Git preserves `%x`, `%x0` and `%xzz` as literal text in
`git stash list --format=<format>` instead of failing. Zmin now matches that
literal-preservation behavior, and `stash_v2_47.tsv` is seeded with the first
stash command row.

The latest adjacent stash list format guard classification is missing prefix
atoms. Stock Git preserves incomplete `%C`, `%G`, `%g`, `%a` and `%c`
pretty-format atoms as literal text in `git stash list --format=<format>`
instead of failing. Zmin now matches that literal-preservation behavior and
`stash_v2_47.tsv` records the focused row.

The latest adjacent stash list format guard classification is unterminated
`%C(...)` color atoms. Stock Git preserves `%C(`, `%C(red` and
`%C(always,red` as literal text in `git stash list --format=<format>` instead
of failing or truncating the malformed atom. Zmin now matches that
literal-preservation behavior and `stash_v2_47.tsv` records the focused row.

The latest adjacent stash list invalid-input classification is an invalid
forced color value: `git stash list --format=%C(always,bad)%h`. Stock Git
exits `1`, leaves stdout empty and prints `error: invalid color value: bad`
followed by `fatal: unable to parse --pretty format`. Zmin now matches that
invalid-input shape and `stash_v2_47.tsv` records the focused row.

The latest adjacent stash list format guard classification is malformed width
atoms. Stock Git preserves `%<(bad)`, `%<()` and `%<` as literal text in
`git stash list --format=<format>` while still rendering the following `%h`
atom. Zmin now matches that literal-preservation behavior and
`stash_v2_47.tsv` records the focused row.

The latest adjacent stash list supported-format classification is valid width
atoms without a following target atom. Stock Git accepts `%<(10)`, `%>(10)`,
`%<(10,trunc)` and `%>(10,trunc)`, exits `0` and writes empty stdout/stderr.
Zmin now matches that behavior and `stash_v2_47.tsv` records the focused row.

The latest adjacent stash list supported-format classification is `%w`
wrapping atoms. Stock Git preserves plain `%w` and malformed `%w(...)` atoms
as literal text, while valid `%w(width[,indent1[,indent2]])` atoms wrap the
following pretty-format output. Zmin now matches those focused forms and
`stash_v2_47.tsv` records the supported-format row.

The latest stock-compatible invalid-input guard classification is
`worktree_impl.rs` `unsupported clean option` parsing for `git clean --bad`
and `git clean -Z`. Stock Git rejects unknown long and short clean options
with exit `129`, empty stdout and command-specific usage diagnostics. Existing
Zmin behavior and focused evidence already match those rows, so this slice only
maps the source guard to the `clean_v2_47.tsv` invalid-input rows.

The latest source-guard classification is `history_impl.rs`
`unsupported_blame_line_range`. The remaining raw source hit is a defensive
fallback behind the expanded `git blame -L` parser and resolver. Existing
`blame_v2_47.tsv` rows cover stock-compatible invalid line numbers, empty
ranges, malformed numeric and regex ranges, no-match regex/function ranges,
invalid BRE syntax, and supported reversed/rich range forms, so this slice maps
the guard to those focused rows instead of adding a new behavior row.

The latest stock-compatible invalid-input guard classification is
`worktree_impl.rs` `unsupported stash {operation} option` handling for
reference-style stash operations. Stock Git rejects `git stash apply --bad`,
`git stash drop --bad` and `git stash pop --bad` with exit `129` and
subcommand-specific usage text; Zmin now emits the same diagnostics and
`stash_v2_47.tsv` records those three invalid-input rows.

The latest source-guard classification is `worktree_impl.rs`
`ls-files --recurse-submodules unsupported mode`. The guard is already mapped
to written invalid-input rows for unsupported recurse-submodule combinations
such as `-o`, `--killed`, `--modified`, `--deleted`, `--unmerged`,
`--resolve-undo` and `--with-tree=HEAD`. Existing evidence in
`git_ls_files_compat::ls_files_recurse_submodules_matches_stock_git` compares
Zmin and stock Git output/exit behavior for those modes, so this slice only
closes the raw-source mapping and does not change counts.

The latest Zmin-only guard classification is `admin_impl.rs`
`unsupported hook '{hook_name}'` in managed-hook validation. Stock Git has
`git hook`, but no `git hooks` command, so this guard is tracked in the
Zmin-only extension inventory instead of the Git `2.47.1` matrix. The focused
extension evidence proves stock Git rejects `git hooks`, while Zmin accepts
supported managed hook names and rejects unsupported names such as
`pre-receive` without creating hook files or config entries.

The latest stock-compatible invalid-input guard classification is
`worktree_impl.rs` `error: unsupported option '{other}'` in
`status --column=<value>` parsing. Stock Git rejects
`git status --column=bogus` with exit `129` and the same unsupported-option
diagnostic, so this source hit is parser validation mapped to the existing
`status_v2_47.tsv` invalid-input row.

The latest closed guard classification is `git p4 unknown`. Stock Git in the
current oracle environment exits `2`, writes the unknown-command diagnostic and
usage text to stdout, and leaves stderr empty. Zmin now prints the same
stock-shaped stdout usage, exits `2`, leaves stderr empty, and records the row
as `closed` in `p4_v2_47.tsv`.

The latest deferred guard classification is `admin_impl.rs`
`unsupported svn command`. The current stock-Git oracle environment exposes a
broken `git-svn` helper: `git svn --version` fails before helper startup
because Perl loads an x86_64 `SVN::_Core` bundle on this arm64 host. That
proves only the local broken-helper shape, not `git-svn` subcommand behavior.
Keep this guard out of closed Git compatibility counts until a real
`git-svn` oracle environment is available or the product explicitly scopes the
legacy SVN bridge.

The latest adjacent deferred guard classification is `admin_impl.rs`
`unsupported archimport option` plus the intentionally unsupported `-o`
old-style branch-name mode. The current stock-Git oracle environment exposes
the command name but was built with `NO_PERL=1`: `git archimport -h` fails
before helper dispatch with `fatal: git was built without support for
git-archimport (NO_PERL=1).` That proves only the local helper-disabled shape,
not GNU Arch bridge option behavior. Keep these guards out of closed Git
compatibility counts until a real `git-archimport` oracle environment is
available or the legacy GNU Arch bridge is explicitly scoped.

The latest stock-compatible invalid-input guard classification is
`transport_impl.rs` `unsupported index-pack option` in the `http-fetch`
packfile helper path. Stock Git treats an invalid delegated
`--index-pack-arg=--bad` token as an `index-pack` usage failure, exits `128`,
leaves stdout empty and appends `fatal: finish_http_pack_request gave result
-1` to stderr. Zmin now emits the same shape, and `http_fetch_v2_47.tsv`
records the focused invalid-input row.

The latest stock-compatible invalid-repository guard classification is
`runtime/worktree_index.rs` `unsupported object format` for local config
`extensions.objectFormat=bogus`. Stock Git rejects `git status --short` in
that repository with exit `128`, empty stdout and invalid config diagnostics
that include the local `.git/config` line number. Zmin now checks object format
during status setup and emits the same diagnostics.

The latest closed transport slice is `clone --reference-if-able` for dumb HTTP
sources. Zmin now accepts
`git clone --reference-if-able <path> http://host/repo.git dst`, writes the
same local reference alternates as stock Git, fetches dumb HTTP objects and
matches `HEAD`, tree output and porcelain status.

The latest stock-compatible invalid-input guard classification is
`runtime/worktree_index.rs` process-filter handshake capability validation.
Stock Git rejects a required process filter that replies with
`capability=bogus` during `git add`, exits `128`, leaves stdout empty, prints
the stock subprocess diagnostic and leaves the index unchanged. Zmin now emits
the same shape instead of the former custom `unsupported filter capability`
fatal message.

The latest adjacent stock-compatible invalid-input guard classification is
`runtime/worktree_index.rs` process-filter response status validation. Stock
Git treats `status=bogus` from a required clean filter as an external-filter
protocol failure with exit `128`, empty stdout, stock `error: external filter`
diagnostics, final `fatal: <path>: clean filter '<name>' failed`, and an
unchanged index. Zmin now emits the same shape instead of the former custom
`unsupported filter process status` fatal message.

The latest Zmin-only guard classification is `transport_impl.rs`
`unsupported ZMIN_GIT_HTTP_VERSION`. Stock Git has no
`ZMIN_GIT_HTTP_VERSION` control, so this is additive Zmin HTTP helper
validation and is tracked under `docs/cli/zmin_extensions_inventory.md`
instead of the Git `2.47.1` matrix. Existing focused unit evidence covers
invalid values such as `h1`.

The latest stock-compatible invalid-repository guard classification is
`runtime/object.rs` `unsupported git object type`. A loose object with an
unknown type name is corrupt repository input; stock Git rejects
`git cat-file -t <oid>` for that object, and Zmin now has focused evidence
showing the same exit code, stdout and stderr.

The latest internal guard classification is the remaining
`runtime/primitive_adapters.rs` primitive-runtime validation group. The
`unsupported object id length`, `unsupported object type ... for patch render`,
`unsupported git object type` and `transport discovery is not supported`
messages are reached through the shared `GitPrimitiveRuntime` adapter contract,
not through a Git `2.47.1` CLI command path. Keep them outside the Git
compatibility denominator until that primitive API is stabilized with its own
non-Git extension tests.

The latest closed guard mapping is `import_impl.rs`
`unsupported_fast_import_command` for unknown fast-import stream commands. It
is already represented by invalid-input rows for both top-level unknown
commands and unknown commands inside a commit record; both use stock-Git crash
report evidence and remain classified as invalid input, not open feature gaps.

### Current Slice Card

This card is the exact handoff target after the current `2396` written-row
state. Finish it before choosing another guard or command.

| Field | Value |
| --- | --- |
| Slice | import the next dense batch of already-covered high-use command rows, or classify the next remaining `unsupported` / `not supported` guard if no dense tested rows are available |
| Stock-Git oracle | use existing focused stock-vs-Zmin tests first; otherwise probe stock Git behavior for the selected guard before changing implementation |
| Implementation area | start with high-use matrices from `status`, `log`, `diff`, `ls-files`, `rev-parse`, `config`, `fetch`, `clone` or `filter-branch`; for guard work, stay on one narrow guard class |
| Evidence test | focused compat test already proving stdout, stderr, exit code and repository state against stock Git, or the smallest new compat test for a guard classification |
| Matrix update | add 3-10 rows from one existing focused test file when safe; for implementation work add the exact row before changing code |
| Expected count movement | predeclared docs-only evidence imports should increase only their declared row/status bucket; complete command and doc-option matrices stay `0/151` and `0/4632` |
| Required gates | focused oracle test, TSV 12-column check, `git diff --check`, readiness/status summaries, docs/count stale scan; add `cargo check -p zmin-cli --bin zmin --profile compat` only for Rust behavior changes |
| Commit rule | stage only this slice's files and commit with a Conventional Commit message before starting the next slice |

After this card is committed and pushed, update the pointer to either the next
small `unsupported` / `not supported` guard classification or a newly observed
WebStorm replacement trace, whichever is more urgent.

Do not publish a support percentage just because partial written rows are now
`0/2396`; the `1/2396` open row and the still incomplete command/doc-option
matrices remain `0/151` and `0/4632`.

The most recent closed transport lane is `clone --reference-if-able` for dumb
HTTP sources. Do not return to that lane unless a new stock-Git trace,
upstream test or dogfood flow exposes a missing value, mode or side effect.

After each committed slice, update this pointer in the same docs/counts commit.
The pointer is the durable handoff source; chat history is not the plan.

### Count Update Gates

After each slice, run:

```bash
tools/git-cli-readiness-status.sh
tools/git-compat-command-summary.sh
tools/git-compat-audit-summary.sh
```

The public counts should change only in the rows the slice touched. Complete
command matrices and complete doc-option matrices remain `0/151` and `0/4632`
until a full matrix is expanded and verified.

## Closed Evidence Blocks

| Block | Closed | Open | Evidence |
| --- | ---: | ---: | --- |
| `blame --date` stock modes | `11` | `0` | `git_history_query_compat` |
| `blame --date` newly closed modes | `2` | `0` | `relative`, `human` |
| `blame --date` invalid format usage | `1` | `0` | `git blame --date=bogus a.txt` exits `128` with stock fatal diagnostic instead of a custom unsupported-date fatal diagnostic |
| `blame` unknown option usage | `1` | `0` | `git blame --bad a.txt` exits `129` with stock usage text instead of a custom unsupported-option fatal diagnostic |
| `blame -L` zero line-number usage | `3` | `0` | `git blame -L 0 a.txt`, `git blame -L 1,0 a.txt`, and `git blame -L /one/,0 a.txt` exit `128` with stock fatal invalid-line-number diagnostics instead of custom unsupported-line-range fatal diagnostics |
| `blame -L` empty-range usage | `4` | `0` | `git blame -L 1,+0 a.txt`, `git blame -L 1,-0 a.txt`, `git blame -L /one/,+0 a.txt`, and `git blame -L /one/,-0 a.txt` exit `128` with stock fatal invalid-empty-range diagnostics instead of printing one blamed line |
| `blame -L` reversed absolute ranges | `3` | `0` | `git blame -L 2,1 a.txt`, `git blame -L 4,2 a.txt`, and `git blame -L 5,1 a.txt` blame the inclusive range after swapping endpoints like stock Git |
| `blame -L` missing function usage | `1` | `0` | `git blame -L :missing a.txt` exits `128` with stock fatal no-match diagnostic instead of a custom unsupported-line-range fatal diagnostic |
| `blame -L` missing regex usage | `1` | `0` | `git blame -L /missing/ a.txt` exits `128` with stock fatal regexec no-match diagnostic instead of a custom unsupported-line-range fatal diagnostic |
| `blame -L` missing end-regex usage | `2` | `0` | `git blame -L 2,/missing/ a.txt` and `git blame -L /two/,/missing/ a.txt` exit `128` with stock fatal regexec no-match diagnostics from the next search line instead of the start line |
| `blame -L` invalid regex usage | `2` | `0` | `git blame -L /[/ a.txt` and `git blame -L 1,/[/ a.txt` exit `128` with stock fatal unbalanced-brackets diagnostics instead of custom unsupported-line-range fatal diagnostics |
| `blame -L` unbalanced bracket regex failures | `3` | `0` | `git blame -L /[a-/ a.txt`, `git blame -L 1,/[a-/ a.txt`, and `git blame -L /one/,/[a-/ a.txt` exit `128` with stock unbalanced-brackets diagnostics instead of custom unsupported-line-range fatal diagnostics |
| `blame -L` compact usage errors | `2` | `0` | `git blame -L : a.txt` and `git blame -L / a.txt` exit `129` with the stock compact usage line instead of custom unsupported-line-range fatal diagnostics |
| `blame -L` malformed numeric usage errors | `2` | `0` | `git blame -L abc a.txt` and `git blame -L 1,abc a.txt` exit `129` with the stock compact usage line instead of custom unsupported-line-range fatal diagnostics |
| `blame -L` malformed count usage errors | `3` | `0` | `git blame -L 1,+abc a.txt`, `git blame -L 1,-abc a.txt`, and `git blame -L /one/,+abc a.txt` exit `129` with the stock compact usage line instead of custom unsupported-line-range fatal diagnostics |
| `blame -L` unterminated regex usage errors | `2` | `0` | `git blame -L /one a.txt` and `git blame -L 1,/one a.txt` exit `129` with the stock compact usage line instead of custom unsupported-line-range fatal diagnostics |
| `blame -L` empty end-regex failures | `2` | `0` | `git blame -L 1,// a.txt` and `git blame -L /one/,// a.txt` exit `128` with stock empty-subexpression diagnostics instead of custom unsupported-line-range fatal diagnostics |
| `blame -L` empty start-regex failures | `3` | `0` | `git blame -L // a.txt`, `git blame -L //,+1 a.txt`, and `git blame -L ^// a.txt` exit `128` with stock empty-subexpression diagnostics instead of custom unsupported-line-range fatal diagnostics |
| `blame -L` regex-range suffix usage errors | `3` | `0` | `git blame -L /one/+1 a.txt`, `git blame -L /one/2 a.txt`, and `git blame -L /one//two/ a.txt` exit `129` with stock compact usage instead of accepting a suffix without the required comma |
| `blame -L` end-regex suffix usage errors | `6` | `0` | `git blame -L 1,/two/3 a.txt`, `git blame -L 1,/two/+1 a.txt`, `git blame -L 1,/two//four/ a.txt`, `git blame -L /one/,/two/3 a.txt`, `git blame -L /one/,/two/+1 a.txt`, and `git blame -L /one/,/two//four/ a.txt` exit `129` with stock compact usage instead of custom unsupported-line-range fatal diagnostics |
| `blame -L` basic-regex literal metacharacters | `4` | `0` | `git blame -L /(/ a.txt` and `git blame -L /{/ a.txt` match literal lines when present and exit `128` with stock no-match diagnostics when absent |
| `blame -L` basic-regex operator literals | `6` | `0` | `git blame -L /a+/ a.txt`, `git blame -L /a?/ a.txt`, and `git blame -L /a|/ a.txt` match literal lines, while `git blame -L /z+/ a.txt`, `git blame -L /z?/ a.txt`, and `git blame -L /z|/ a.txt` exit `128` with stock no-match diagnostics |
| `blame -L` basic-regex escaped operators | `3` | `0` | `git blame -L /a\+/ a.txt`, `git blame -L /a\?/ a.txt`, and `git blame -L /z\|a/ a.txt` use stock Git basic-regex operator semantics instead of treating the escaped operator as a literal |
| `blame -L` basic-regex leading-star literal | `2` | `0` | `git blame -L /*/ a.txt` matches a literal star line when present and exits `128` with stock no-match diagnostics when absent |
| `blame -L` basic-regex escaped grouping | `3` | `0` | `git blame -L /x\(y\)/ a.txt` and `git blame -L /x\(y\)*/ a.txt` use stock Git basic-regex grouping semantics, while `git blame -L /q\(r\)/ a.txt` exits `128` with stock no-match diagnostics |
| `blame -L` basic-regex escaped intervals | `3` | `0` | `git blame -L /x\{2\}/ a.txt` and `git blame -L /x\{2,3\}/ a.txt` use stock Git basic-regex interval semantics, while `git blame -L /q\{2\}/ a.txt` exits `128` with stock no-match diagnostics |
| `blame -L` basic-regex invalid intervals | `5` | `0` | `git blame -L /\{/ a.txt`, `git blame -L /a\{x\}/ a.txt`, `git blame -L /a\{2/ a.txt`, `git blame -L /a\{,2\}/ a.txt`, and `git blame -L /a\{3,2\}/ a.txt` exit `128` with stock invalid interval diagnostics instead of custom unsupported-line-range fatal diagnostics |
| `blame -L` basic-regex invalid character ranges | `2` | `0` | `git blame -L /[z-a]/ a.txt` and `git blame -L /[b-a]/ a.txt` exit `128` with stock invalid-character-range diagnostics instead of custom unsupported-line-range fatal diagnostics |
| `blame -L` basic-regex multiple character ranges | `3` | `0` | `git blame -L /[a-b-c]/ a.txt`, `git blame -L /[0-9-a]/ a.txt`, and `git blame -L /[a--]/ a.txt` exit `128` with stock invalid-character-range diagnostics instead of matching through Rust regex range handling |
| `blame -L` basic-regex invalid POSIX class range endpoints | `3` | `0` | `git blame -L /[[:digit:]-a]/ a.txt`, `git blame -L /[[:digit:]-[:alpha:]]/ a.txt`, and `git blame -L /[a-[:upper:]]/ a.txt` exit `128` with stock invalid-character-range diagnostics instead of matching through Rust regex class support |
| `blame -L` basic-regex empty character classes | `2` | `0` | `git blame -L /[]/ a.txt` and `git blame -L /[^]/ a.txt` exit `128` with stock unbalanced-brackets diagnostics instead of custom unsupported-line-range fatal diagnostics |
| `blame -L` basic-regex unbalanced grouping | `4` | `0` | `git blame -L /\(/ a.txt`, `git blame -L /\)/ a.txt`, `git blame -L /x\(y/ a.txt`, and `git blame -L /x\)y/ a.txt` exit `128` with stock parentheses-not-balanced diagnostics instead of custom unsupported-line-range fatal diagnostics |
| `blame -L` basic-regex invalid backreferences | `3` | `0` | `git blame -L /\1/ a.txt`, `git blame -L /x\1/ a.txt`, and `git blame -L /\(x\)\2/ a.txt` exit `128` with stock invalid-backreference-number diagnostics instead of custom unsupported-line-range fatal diagnostics |
| `blame -L` basic-regex invalid POSIX character classes | `3` | `0` | `git blame -L /[[:word:]]/ a.txt`, `git blame -L /[[:ascii:]]/ a.txt`, and `git blame -L /[[:bogus:]]/ a.txt` exit `128` with stock invalid-character-class diagnostics instead of matching every line through Rust regex class support |
| `blame -L` basic-regex invalid compound POSIX character classes | `3` | `0` | `git blame -L /[[:digit:][:bogus:]]/ a.txt`, `git blame -L /[[:bogus:][:digit:]]/ a.txt`, and `git blame -L /[[:bogus:]/ a.txt` exit `128` with stock invalid-character-class diagnostics instead of matching through Rust regex class support or falling through to a generic bracket diagnostic |
| `blame -L` basic-regex invalid POSIX collating elements | `3` | `0` | `git blame -L /[[.bogus.]]/ a.txt`, `git blame -L /[[=bogus=]]/ a.txt`, and `git blame -L /[[.ch.]]/ a.txt` exit `128` with stock invalid-collating-element diagnostics instead of matching every line through Rust regex bracket syntax |
| `blame --progress` non-tty form | `1` | `0` | `git blame --progress a.txt` exits `0` with stock stdout and empty stderr for a small tracked-file blame |
| `blame --minimal` small tracked-file form | `1` | `0` | `git blame --minimal a.txt` exits `0` with stock stdout and empty stderr for a small tracked-file blame |
| `blame --color-lines` non-tty form | `1` | `0` | `git blame --color-lines a.txt` exits `0` with stock stdout and empty stderr for a small tracked-file blame |
| `blame --color-by-age` small tracked-file form | `1` | `0` | `git blame --color-by-age a.txt` exits `0` with stock blue metadata color, matching stdout and empty stderr |
| `blame --score-debug` small tracked-file form | `1` | `0` | `git blame --score-debug a.txt` exits `0` with stock score fields, matching stdout and empty stderr |
| `blame` normal display flags | `4` | `0` | `-s`, `-t`, `-b`, `-c` |
| `blame` no-toggle/reset forms | `15` | `0` | standalone `--no-*` plus positive-then-no forms, excluding nuanced `--root --no-root` |
| `blame --no-abbrev` forms | `3` | `0` | `--no-abbrev`, `--abbrev=N --no-abbrev`, `--no-abbrev --abbrev=N` |
| `blame` final-disabled mode toggles | `10` | `0` | `--no-progress`, `--no-score-debug`, `--no-color-lines`, `--no-color-by-age`, `--no-minimal` plus positive-then-no forms |
| `blame -L` extended range forms | `6` | `0` | `N,-M`, `,N`, `/re/,-N`, `N,/re/`, `/re/,/re/`, `^/re/` |
| `blame -L :name` plain symbol boundary | `1` | `0` | stops at the matching non-function line like stock Git instead of extending to EOF |
| `init` quiet forms | `2` | `0` | `-q`, `--quiet` |
| `notes add` empty editor forms | `2` | `0` | `--allow-empty`, `--allow-empty --no-edit` |
| `notes copy` stdin/no-stdin toggle forms | `2` | `0` | `--stdin --no-stdin`, `--no-stdin --stdin` |
| `notes copy` unknown option usage | `1` | `0` | `git notes copy --bad` exits `129` with stock usage text instead of a custom unsupported-option fatal diagnostic |
| `notes` subcommand unknown option usage | `5` | `0` | `git notes add/edit/remove/prune/merge --bad` exit `129` with stock usage text instead of custom unsupported-option fatal diagnostics |
| `notes edit` message-source forms | `8` | `0` | `-m`, `--message=`, `-F`, `--file=`, `-C`, `--reuse-message=`, `-c`, `--reedit-message=` |
| `notes edit` compact short source forms | `4` | `0` | `-mmsg`, `-Ffile`, `-C<object>`, `-c<object>` |
| `notes merge` no-strategy toggle forms | `7` | `0` | merge order variants plus `--commit`/`--abort` state variants |
| `notes remove` stdin/no-stdin toggle forms | `2` | `0` | `--stdin --no-stdin`, `--no-stdin --stdin` |
| `config` replacement null list | `1` | `0` | `--null --list` through the `git` shim on cloned repository config |
| `config` replacement core filemode query | `1` | `0` | `--get core.filemode` through the `git` shim on cloned repository config |
| `config` replacement remote metadata queries | `3` | `0` | `--get remote.origin.url`, `--get branch.main.remote`, and `--get branch.main.merge` through the `git` shim |
| `config` replacement missing optional get | `1` | `0` | `--get commit.template` through the `git` shim exits 1 with empty stdout/stderr on a cloned repository without that key |
| `config` replacement remote regexp query | `1` | `0` | `--get-regexp ^remote\\.` through the `git` shim on cloned repository config |
| `config` replacement branch regexp query | `1` | `0` | `--get-regexp ^branch\\.` through the `git` shim on cloned repository config |
| `config -c` inline bool and empty values | `3` | `0` | key-only bool get, empty-value bool get and empty-value raw get already covered by `git_global_cli_compat` |
| `show` root and IDE output rows | `6` | `0` | default root commit output, explicit `--root`, raw format, `log.showroot=false` and NUL-delimited `--name-status` output already covered by `git_history_query_compat` |
| `credential` basic protocol flows | `3` | `1` | `fill`, `approve`, `reject` with complete stdin plus missing username/password fill failure already covered by `git_credential_compat` |
| `credential-store` store/get/erase flows | `3` | `0` | isolated-HOME `.git-credentials` store, get and erase file side effects already covered by `git_credential_compat` |
| `credential-cache` socket store/get/erase flows | `4` | `0` | Unix `--socket=<path>` store, get, erase and get-after-erase flows already covered by `git_credential_compat` |
| `describe` tag, ref-filter and dirty forms | `10` | `0` | default describe, `--long`, `--abbrev=0`, `--abbrev=12`, `--tags`, `--all`, `--match`, `--exclude`, `--always` and `--dirty` already covered by `git_history_query_compat` |
| `shortlog` author summary forms | `6` | `0` | default `HEAD`, `-s`, `-sn`, `-se`, `--no-merges` and `HEAD~2..HEAD` range forms already covered by `git_history_query_compat` |
| `cherry` upstream/head comparison forms | `6` | `0` | default upstream, explicit upstream/head, `-v`, `--abbrev`, `--abbrev=12` and limit forms already covered by `git_history_query_compat` |
| `rev-list` base traversal and object forms | `10` | `0` | `--max-count`, `--all`, revision range, negative revision, `--not`, `--count`, `--parents`, `-1`, `--objects` and `--objects --no-object-names` already covered by `git_history_query_compat` |
| `rev-list` symmetric difference and separator forms | `7` | `0` | symmetric difference traversal, count, reverse, object traversal, no-object-names, `--not left...main main` and `--objects HEAD --` separator forms already covered by `git_history_query_compat` |
| `apply` stdin patch modes | `10` | `0` | `git apply --check`, default stdin apply, `--cached`, `--index`, binary reverse `-R`, rename, Unix mode-only and header-only invalid patch forms already covered by `git_apply_compat` |
| `patch-id` stdin patch modes | `6` | `0` | default, `--stable`, `--unstable`, `--verbatim` diff patch forms plus stable and unstable log patch streams already covered by `git_apply_compat` |
| `stripspace` stdin text modes | `5` | `0` | default, `-s`, `-c`, `--strip-comments` and `--comment-lines` stdin text forms already covered by `git_text_tools_compat` |
| `check-mailmap` common entry and stdin/mailmap option forms | `23` | `0` | positional name-and-email, email-only, display-name, canonical-email, unmapped identity, `--stdin` batch/triple/order forms, stdin with `--mailmap-file` or `--mailmap-blob`, stdin file/blob last-option-wins forms, `--mailmap-file` separate/equals, `--mailmap-blob` separate/equals, and positional file/blob last-option-wins forms already covered by `git_text_tools_compat` and oracle smoke |
| `interpret-trailers` common stdin modes | `10` | `0` | default, `--only-trailers`, `--parse`, `--trailer`, `--where before/after`, `--if-exists addIfDifferent/add/replace` and `--if-missing doNothing` stdin forms already covered by `git_mail_tools_compat` |
| `mailinfo` common patch mail modes | `4` | `0` | default, `-k`, `-b` and `-m` stdin patch-mail forms already covered by `git_mail_tools_compat` |
| `mailsplit` mbox and maildir modes | `2` | `0` | mbox `-d4 -f3 -o...` and maildir `-o...` split forms already covered by `git_mail_tools_compat` |
| `fmt-merge-msg` fetch-head title modes | `4` | `0` | default stdin `FETCH_HEAD`, `--into-name`, `-m` and `-F` file-input forms already covered by `git_mail_tools_compat` |
| `send-email` alias modes | `16` | `0` | `--dump-aliases` and `--translate-aliases` for configured mutt aliases plus mutt, mailrc, pine, elm, sendmail, gnus and unknown alias file type states already covered by `git_mail_tools_compat` |
| `request-pull` local pushed branch | `1` | `0` | local pushed-branch summary with file URL remote already covered by `git_mail_tools_compat` |
| `bugreport` suffix filename modes | `3` | `0` | custom, strftime and path suffix report-file variants already covered by `git_admin_tools_compat` |
| `for-each-repo` configured repository modes | `4` | `0` | configured repository iteration, missing repository failures with and without `--keep-going`, and missing config key no-op already covered by `git_admin_tools_compat` |
| `replay` linear range modes | `4` | `1` | plain range usage failure plus contained advance, fixed-date advance and `--onto` replay forms already covered by `git_admin_tools_compat` |
| `clean` toggle-order and compact short forms | `12` | `0` | `-n/--no-dry-run`, `-f/--no-force`, `-q/--no-quiet`, `--no-interactive` order-sensitive forms plus compact `-fd` and `-fx -ekeep.tmp` already covered by `git_clean_compat` |
| `clean` force/quiet/path and nested git combos | `8` | `0` | `-n -x`, `-f -x/-X -d`, `-f -d dir`, quiet `-f -q -x/-X -d`, compact `-ffd`, and quiet nested `-ff -q -d` already covered by `git_clean_compat` |
| `clean` quiet dry-run and path-limited combos | `14` | `0` | quiet `-n -d`, quiet path-limited dry-run, quiet `-n -q -x/-X` with and without `-d`, quiet top-level force, quiet path-limited force, and quiet ignored-directory pathspec dry-run/force forms already covered by `git_clean_compat` |
| `column --mode` dense layout forms | `4` | `0` | `dense`, `nodense`, `column,dense`, `row,dense` |
| `rerere` invalid operation usage | `1` | `0` | `git rerere bogus` exits `129` with stock usage text instead of a custom unsupported-operation fatal diagnostic |
| `reflog` shorthand invalid ref usage | `1` | `0` | `git reflog bogus` is parsed as shorthand `show` ref and exits `128` with stock ambiguous-revision diagnostics instead of using an unsupported-subcommand branch |
| `log --decorate` boolean value forms | `5` | `0` | `yes`, `on`, `1`, `off`, `0` |
| `log --decorate` invalid value usage | `1` | `0` | `git log --decorate=bogus -1` exits `128` with stock fatal diagnostic instead of a custom unsupported-value fatal diagnostic |
| `log --diff-merges` separate stat forms | `3` | `0` | `separate`, `on`, `m`; skip empty parent diff blocks like stock Git |
| `log --diff-merges` invalid value usage | `1` | `0` | `git log --diff-merges=bogus -1` exits `128` with stock fatal diagnostic instead of a custom unsupported-value fatal diagnostic |
| `merge` invalid strategy usage | `1` | `0` | `git merge -s bogus feature` exits `1` with stock missing-strategy diagnostic instead of a custom unsupported-strategy fatal diagnostic |
| `rebase -i` invalid todo command usage | `1` | `0` | `GIT_SEQUENCE_EDITOR=<editor> git rebase -i HEAD~1` with an unknown todo command exits `1` with stock invalid-command diagnostics, leaves `.git/rebase-merge` state for recovery, moves HEAD to the stock in-progress rebase point, and supports `git rebase --abort` cleanup instead of a custom unsupported-interactive-command fatal diagnostic |
| `check-ref-format` common accepted and invalid forms | `11` | `0` | full refname validation, `--allow-onelevel`, `--normalize`, `--branch`, one-level rejection, invalid path components, trailing slash and invalid branch shorthand already covered by `git_check_ref_format_compat` |
| `filter-branch` supported filters and options | `13` | `0` | `--msg-filter`, `--tree-filter`, `--index-filter`, `--env-filter`, `--parent-filter`, `--subdirectory-filter`, `--tag-name-filter`, `--setup` plus message filter, `-d` temp directory, `--commit-filter` passthrough, `--commit-filter` with `skip_commit`, initial `--state-branch`, and repeated state-branch forms already covered by `git_filter_branch_compat` |
| `clone` local path options | `51` | `0` | default local clone, `--quiet`, `--local`, `--no-local`, `--no-hardlinks`, `--hardlinks`, `--shared`, repeated `-c` config, `--template`, `--no-template` ordering, custom origin name, long/equals `--origin`, `--branch`, `--single-branch` and `--config` forms, `--no-tags`, `--tags`, tag-option ordering, local `--reference`, local `--reference-if-able`, missing `--reference-if-able`, `--dissociate` with reference, `--shared --dissociate`, local and file URL `--depth 1`, `-b`/`--branch feature`, `--checkout`/`--no-checkout` ordering, `--separate-git-dir`, `--no-single-branch` ordering, bare, mirror, shared bare/mirror and bare/mirror no-tags forms, explicit `--ref-format=files`, and case-insensitive symlink/directory collision checkout already covered by `git_clone_compat` |
| `log --date` author/committer format values | `13` | `0` | built-in date modes plus `format:` and `format-local:` strftime values for `%ad` and `%cd` |
| `log` replacement basic NUL format output | `1` | `0` | `-z --format=%H%x00%P%x00%D%x00%s -1` through the `git` shim |
| `log` replacement iso-strict NUL date output | `1` | `0` | `--date=iso-strict -z --format=%H%x00%ad%x00%cd` through the `git` shim |
| `log` replacement directory pathspec NUL output | `1` | `0` | `-z --format=%H%x00%s -1 -- dir` through the `git` shim |
| `diff` blob-to-blob operands | `1` | `0` | `git diff <blob> <blob>` renders stock blob patch output and empty output for identical blobs |
| `diff` mixed tree/blob operand usage | `1` | `0` | `git diff HEAD^{tree} <blob>` and the reverse order exit `129` with stock usage text |
| `diff` replacement worktree NUL name-status | `1` | `0` | `--name-status -z` through the `git` shim with a modified tracked file |
| `diff` replacement worktree NUL name-only | `1` | `0` | `--name-only -z` through the `git` shim with a modified tracked file |
| `diff` replacement pathspec NUL name-status | `1` | `0` | `--name-status -z -- dir` through the `git` shim with a dirty nested tracked file |
| `diff` replacement pathspec NUL name-only | `1` | `0` | `--name-only -z -- dir` through the `git` shim with a dirty nested tracked file |
| `diff` replacement worktree NUL raw output | `1` | `0` | `--raw -z` through the `git` shim with a modified tracked file |
| `diff` replacement cached NUL name-status | `1` | `0` | `--cached --name-status -z` through the `git` shim with staged modified and added files |
| `diff` replacement cached NUL name-only | `1` | `0` | `--cached --name-only -z` through the `git` shim with staged modified and added files |
| `diff` replacement cached NUL raw output | `1` | `0` | `--cached --raw -z` through the `git` shim with staged modified and added files |
| `diff` replacement cached pathspec NUL name-status | `1` | `0` | `--cached --name-status -z -- new.txt` through the `git` shim with a staged added file |
| `diff` replacement cached pathspec NUL name-only | `1` | `0` | `--cached --name-only -z -- new.txt` through the `git` shim with a staged added file |
| `diff` replacement cached pathspec NUL raw output | `1` | `0` | `--cached --raw -z -- new.txt` through the `git` shim with a staged added file |
| `diff` replacement cached modified pathspec NUL raw output | `1` | `0` | `--cached --raw -z -- tracked.txt` through the `git` shim with a staged modified file |
| `diff --diff-filter` invalid change classes | `3` | `0` | `Z`, `@` and `A@` exit `129` with stock unknown-change-class diagnostics instead of a custom unsupported-status fatal diagnostic |
| `stash list` reflog/signature format atoms | `6` | `0` | `%gN`, `%gE`, `%gn`, `%ge`, `%GS`, `%GG` |
| `stash list` literal-preserved format atoms | `12` | `0` | `%r`, `%R`, `%q`, `%Q`, `%z`, `%gL`, `%gI`, `%gq`, `%gZ`, `%aZ`, `%cZ`, `%GZ` |
| `stash list` non-forced color format atoms | `3` | `0` | `%Cred`, `%C(red)`, `%C(auto,red)` with reset forms in redirected output |
| `stash list` forced color format atoms | `3` | `0` | `%C(always,red)`, `%C(always,bold red)`, `%C(always,blue)` with reset/normal forms |
| `stash list` width format atoms | `6` | `0` | `%<(N)`, `%>(N)`, `%<(N,trunc)`, `%>(N,trunc)`, `%<(N,ltrunc)`, `%<(N,mtrunc)` |
| `stash list` width format atoms without target | `4` | `0` | `%<(N)`, `%>(N)`, `%<(N,trunc)`, `%>(N,trunc)` with no following atom |
| `stash list` width format atoms with literal target | `4` | `0` | `%<(N)x`, `%>(N)x`, and literal text before a later pretty-format atom |
| `stash list` wrap format atoms | `6` | `0` | plain `%w`, `%w%s`, `%w(N)`, `%w(N,I1,I2)`, malformed `%w(bad)` and `%w(N,bad)` forms |
| `status -z` implicit porcelain form | `1` | `0` | `git status -z` matches stock Git's NUL-terminated porcelain v1 output |
| `status` replacement short output | `1` | `0` | `--short` through the `git` shim on a cloned repository with dirty and untracked files |
| `status` replacement short pathspec output | `1` | `0` | `--short -- dir` through the `git` shim on a cloned repository with a dirty nested tracked file |
| `status` replacement NUL output | `1` | `0` | `-z` through the `git` shim on a cloned repository with dirty and untracked files |
| `status` replacement ignored NUL output | `1` | `0` | `--ignored --porcelain=v1 -z` through the `git` shim on a cloned repository with untracked and ignored files |
| `status` replacement porcelain v2 ignored NUL output | `1` | `0` | `--ignored --porcelain=v2 -z --branch` through the `git` shim on a cloned repository with untracked and ignored files |
| `status` replacement NUL pathspec output | `1` | `0` | `--porcelain=v1 -z -- dir` through the `git` shim on a cloned repository with a dirty nested tracked file |
| `status` option evidence forms | `5` | `0` | `--null`, `--short`, `-unormal`, bare `--untracked-files`, `--ignored=traditional` |
| `status` ahead-behind toggles | `2` | `0` | `--ahead-behind`, `--no-ahead-behind` with porcelain v1/v2 and equal/different upstream refs |
| `status` stash display toggles | `2` | `0` | `--show-stash`, `--no-show-stash` with human, porcelain v2 and toggle order |
| `status` long mode toggles | `2` | `0` | `--long`, `--no-long` with short/long order-sensitive forms |
| `status` verbose modes | `2` | `0` | `--verbose`, `--no-verbose`, `-v`, `-vv`, reset order and machine-readable combinations |
| `status` column modes | `2` | `0` | `--column`, `--no-column`, `--column=always/never`, `column.status=always` and machine-readable combinations |
| `status` invalid column mode usage | `1` | `0` | `git status --column=bogus` exits `129` with stock error diagnostic instead of a fatal unsupported-option diagnostic |
| `status` global no-op option | `1` | `0` | `--no-optional-locks status --short` via leading global option parser |
| `status` rename modes | `4` | `0` | `--renames`, `--no-renames`, `--find-renames`, `--find-renames=<n>` for staged exact rename output |
| `status` ignore-submodules modes | `4` | `0` | `--ignore-submodules`, `=all`, `=dirty`, `=untracked` with dirty, untracked and new-commit submodule states |
| `status` human branch modes | `1` | `0` | `-b`/`--branch` standalone human status for dirty, untracked and upstream-ahead states |
| `status` replacement short branch output | `1` | `0` | `--short --branch` through the `git` shim on a cloned repository with dirty and untracked files |
| `status` replacement porcelain v2 branch NUL output | `1` | `0` | `--porcelain=v2 -z --branch` through the `git` shim on a cloned repository |
| `status` replacement porcelain v2 branch NUL no-untracked output | `1` | `0` | `--porcelain=v2 -z --branch --untracked-files=no` through the `git` shim on a cloned repository |
| `status` pathspec modes | `13` | `0` | exact file, directory, default glob, explicit magic, exclude magic, human output and global pathspec flags |
| `status` invalid porcelain version usage | `1` | `0` | `git status --porcelain=v3` exits `128` with stock fatal diagnostic instead of an unclassified unsupported-version guard |
| `status` invalid untracked-files mode usage | `1` | `0` | `git status --untracked-files=bogus` exits `128` with stock fatal diagnostic instead of a custom unsupported-mode fatal diagnostic |
| `status` invalid ignored mode usage | `1` | `0` | `git status --ignored=bogus` exits `128` with stock fatal diagnostic instead of an unclassified unsupported-mode guard |
| `status` invalid ignore-submodules mode usage | `1` | `0` | `git status --ignore-submodules=bogus` exits `128` with stock fatal diagnostic instead of a custom unsupported-mode fatal diagnostic |
| `stash` reference operation unknown option usage | `3` | `0` | `git stash apply/drop/pop --bad` exit `129` with stock subcommand-specific usage text instead of custom unsupported-option diagnostics |
| `clean` unknown option usage | `1` | `0` | `git clean --bad` exits `129` with stock unknown-option usage text instead of a fatal unsupported-option diagnostic |
| `clean` unknown short-switch usage | `1` | `0` | `git clean -Z` exits `129` with stock unknown-switch usage text instead of an unknown-option diagnostic |
| `clean` exclude missing-value usage | `2` | `0` | `git clean -e` and `git clean --exclude` exit `129` with stock missing-value diagnostics instead of a fatal custom pattern diagnostic |
| `clean` dry-run, path and force rows | `7` | `1` | dry-run default, `-n -d`, path-limited `-n -d dir`, ignored-only `-X`, all-untracked `-x`, default require-force rejection and actual `-f -d` cleanup already covered by `git_clean_compat` |
| `clean -e` exclude pattern rows | `4` | `0` | dry-run, all-untracked dry-run, ignored-only dry-run and force all-untracked cleanup with `-e keep.tmp` already covered by `git_clean_compat` |
| `checkout` delayed smudge process-filter failure | `1` | `0` | `git checkout -- a.bad` with a required smudge process filter returning `status=delayed` exits `128`, leaves stdout empty, emits stock external-filter diagnostics and leaves the worktree file absent |
| `ls-files` replacement stage NUL output | `1` | `0` | `--stage -z` through the `git` shim on a clean cloned index |
| `ls-files` replacement cached plus others NUL output | `1` | `0` | `-z --cached --others --exclude-standard` through the `git` shim on a cloned repository |
| `ls-files` replacement cached pathspec NUL output | `1` | `0` | `-z --cached -- dir` through the `git` shim on a cloned repository |
| `ls-files` replacement others pathspec NUL output | `1` | `0` | `-z --others --exclude-standard -- dir` through the `git` shim on a cloned repository with an untracked nested file |
| `ls-files` replacement deleted plus modified NUL output | `1` | `0` | `-z --deleted --modified` through the `git` shim on a cloned repository with deleted and modified tracked files |
| `ls-files` replacement mixed worktree NUL output | `1` | `0` | `-z --modified --deleted --others --exclude-standard` through the `git` shim on a cloned repository with untracked, deleted and modified tracked files |
| `ls-files` mode and top-level pathspec forms | `5` | `0` | skip-worktree `-t`, `--debug -t`, `-f --modified --deleted`, `:/a.txt` and `--full-name :(top)a.txt` from existing stock-oracle evidence |
| `ls-files --stage` raw regular index mode bits | `1` | `0` | `git ls-files --stage` preserves raw `100640` mode output from a checksum-valid index while using canonical file behavior internally |
| `ls-files --stage` raw unknown non-tree index mode bits | `1` | `0` | `git ls-files --stage` preserves raw `000000`, `200000` and `777777` mode output from checksum-valid indexes while keeping sparse/tree mode `040000` for a separate slice |
| `ls-files --sparse --stage` sparse index tree entries | `1` | `0` | `git ls-files --sparse --stage` prints stock sparse-directory `040000` tree entries from an index with the `sdir` marker |
| `ls-files --stage` stock Git index v4 | `1` | `0` | `git ls-files --stage` reads stock Git version 4 indexes with prefix-compressed paths |
| `ls-files --stage` unknown required index extension | `1` | `0` | `git ls-files --stage` rejects a checksum-valid lowercase `abcd` index extension with stock corrupt-index diagnostics |
| `ls-files --resolve-undo` row-by-row modes | `8` | `0` | default, `-t`, `-v`, `--abbrev=12`, `-z`, `--error-unmatch f.txt`, `-s` and `-u` resolve-undo forms already covered by `git_ls_files_compat` |
| `ls-files --with-tree` row-by-row modes | `6` | `0` | default, pathspec, `--error-unmatch`, `--deduplicate`, `-t` and `--format` with-tree forms already covered by `git_ls_files_compat` |
| `ls-files --recurse-submodules` supported row-by-row modes | `8` | `0` | default, `-t`, `-s`, `--format`, ignored cached, ignored cached `-s`, ignored cached `-t` and ignored cached `-z` forms already covered by `git_ls_files_compat` |
| `ls-files --unmerged` row-by-row modes | `7` | `0` | default `--unmerged`, short `-u`, `-t`, `-v`, `-z`, `--deduplicate` and `--full-name f.txt` forms already covered by `git_ls_files_compat` |
| `ls-files --killed` row-by-row modes | `4` | `0` | plain, `-t`, `-z` and `--directory` killed-entry forms already covered by `git_ls_files_compat` |
| `ls-files` conflicted-index non-unmerged modes | `7` | `0` | default, `-c`, `-t`, `-v`, `--stage`, `-s -t` and `-s -v` conflicted-index forms already covered by `git_ls_files_compat` |
| `ls-files` nested-cwd and EOL rows | `3` | `0` | default nested-cwd listing plus LF and CRLF single-path `--eol` rows already covered by `git_ls_files_compat` |
| `diff` patch edge-case rows | `6` | `0` | context/no-newline, cached context, markdown-empty-binary-delete, YAML hunk-header, JSON/repeated-blank and binary-stat rows already covered by `git_diff_compat` |
| `diff --check` and no-index edge rows | `3` | `1` | clean pathspec `--check`, dirty whitespace diagnostics, missing no-index path rejection and no-index `/dev/null` deletion comparison already covered by `git_diff_compat` |
| `diff` whitespace-ignore rows | `10` | `0` | cached patch/stat/numstat/shortstat forms for `--ignore-space-at-eol`, `--ignore-cr-at-eol`, `--ignore-space-change`, `-b`, `--ignore-all-space` and `-w` already covered by `git_diff_compat` |
| `diff --no-index` directory rows | `25` | `0` | default, reversed, stat, numstat, shortstat, name-only, name-status, raw, summary, patch-with-stat, patch-with-raw, full raw, file-directory and binary forms for no-index paths already covered by `git_diff_compat` |
| `index-pack --verify invalid file-format rows` | `4` | `0` | bad reverse-index signature, bad reverse-index version, unsupported pack version, and unsupported reverse-index version lanes are already scoped as compact exact invalid-input follow-ups |
| `verify-pack` unsupported pack index version | `1` | `0` | `git verify-pack` rejects a checksum-valid `.idx` version `3` with stock unsupported-version diagnostics |
| `index-pack --verify` bad reverse-index signature | `1` | `0` | `git index-pack --verify` rejects a checksum-valid `.rev` with a bad signature using stock sha1 validation diagnostics |
| `index-pack --verify` bad reverse-index version | `1` | `0` | `git index-pack --verify` rejects a checksum-valid `.rev` version `2` using stock sha1 validation diagnostics |
| `pack commands unsupported pack file version` | `2` | `0` | `git index-pack --verify` and `git verify-pack` reject a checksum-valid pack version `4` using stock fatal pack-version diagnostics |
| `multi-pack-index verify` header variants | `4` | `0` | bad signature, bad version, unsupported hash version and nonzero reserved byte in checksum-valid MIDX files |
| `rev-parse --short` object id lengths | `3` | `0` | default length, explicit `--short=12`, and overlarge `--short=100` for `HEAD` |
| `rev-parse ^{blob}` peel variants | `2` | `0` | direct blob object id and annotated tag to blob target |
| `rev-parse --verify` probing modes | `3` | `0` | verified `HEAD`, missing ref fatal diagnostics, and quiet missing-ref exit/status behavior |
| `rev-parse --path-format` cwd-specific path rows | `5` | `0` | root and nested `--git-dir` / `--git-common-dir` forms plus relative `--show-toplevel` already covered by `git_global_cli_compat` |
| `rev-parse` nested symbolic HEAD resolution | `1` | `0` | `HEAD -> refs/heads/alias -> refs/heads/main` resolves to the final commit id like stock Git |
| `rev-parse` replacement git-dir discovery | `1` | `0` | `--git-dir` through the `git` shim on a cloned repository root |
| `rev-parse` replacement inside-work-tree boolean | `1` | `0` | `--is-inside-work-tree` through the `git` shim on a cloned repository root |
| `rev-parse` replacement branch name | `1` | `0` | `--abbrev-ref HEAD` through the `git` shim on a cloned repository root |
| `rev-parse` replacement HEAD object id | `1` | `0` | `HEAD` through the `git` shim on a cloned repository root |
| `rev-parse` replacement top-level path | `1` | `0` | `--show-toplevel` through the `git` shim on a cloned repository root |
| `rev-parse` replacement nested top-level path | `1` | `0` | `--show-toplevel` through the `git` shim from a nested cwd |
| `rev-parse` replacement nested path discovery | `1` | `0` | `--show-prefix --show-cdup --show-toplevel` through the `git` shim from a nested cwd |
| `rev-parse` replacement nested cd-up path | `1` | `0` | `--show-cdup` through the `git` shim from a nested cwd |
| `rev-parse` replacement nested prefix path | `1` | `0` | `--show-prefix` through the `git` shim from a nested cwd |
| `rev-parse` replacement fetched origin/main object id | `1` | `0` | `refs/remotes/origin/main` through the `git` shim after `fetch --prune --no-tags` |
| `rev-parse` replacement pruned branch missing | `1` | `0` | `--verify refs/remotes/origin/gone` through the `git` shim after `fetch --prune --no-tags` |
| `rev-parse` replacement no-tags missing tag | `1` | `0` | `--verify refs/tags/later-tag` through the `git` shim after `fetch --prune --no-tags` |
| `fetch --shallow-since` explicit local/file branch forms | `4` | `0` | equals and separate-value forms for explicit local path and file URL branch fetches |
| `fetch --shallow-since` explicit local/file HEAD forms | `4` | `0` | equals and separate-value forms for explicit local path and file URL HEAD fetches |
| `fetch --shallow-since` multiple explicit local/file refspec forms | `4` | `0` | named local/file remotes and explicit local/file locations with two destination refspecs |
| `fetch --shallow-since` network branch forms | `3` | `0` | smart HTTP, SSH and git daemon branch fetches with matching remote-tracking ref, FETCH_HEAD and shallow state |
| `fetch --shallow-since` network branchless configured fetch forms | `3` | `0` | smart HTTP, SSH and git daemon configured fetches with matching remote-tracking refs, FETCH_HEAD and shallow state |
| `fetch --shallow-exclude` explicit local/file branch forms | `4` | `0` | equals and separate-value forms for explicit local path and file URL branch fetches |
| `fetch --shallow-exclude` explicit local/file HEAD forms | `4` | `0` | equals and separate-value forms for explicit local path and file URL HEAD fetches |
| `fetch --shallow-exclude` repeated local/file forms | `5` | `0` | repeated exclude forms for named local/file remote branch, explicit local/file branch, and explicit local/file HEAD fetches |
| `fetch --shallow-exclude` multiple explicit local/file refspec forms | `4` | `0` | named local/file remotes and explicit local/file locations with two destination refspecs |
| `fetch --shallow-exclude` network branch forms | `3` | `0` | smart HTTP, SSH and git daemon branch fetches with matching remote-tracking ref, FETCH_HEAD and shallow state |
| `fetch --shallow-exclude` network branchless configured fetch forms | `3` | `0` | smart HTTP, SSH and git daemon configured fetches with matching remote-tracking refs, FETCH_HEAD and shallow state |
| `fetch --shallow-exclude` repeated network branch forms | `3` | `0` | smart HTTP, SSH and git daemon branch fetches with repeated exclude values for two branch histories |
| `fetch --deepen` explicit local/file branch forms | `4` | `0` | equals and separate-value forms for explicit local path and file URL branch fetches in existing shallow repos |
| `fetch --deepen` explicit local/file HEAD forms | `4` | `0` | equals and separate-value forms for explicit local path and file URL HEAD fetches in existing shallow repos |
| `fetch --deepen` multiple explicit local/file refspec forms | `4` | `0` | named local/file remotes and explicit local/file locations with two destination refspecs in existing shallow repos |
| `fetch --deepen` network branch forms | `3` | `0` | smart HTTP, SSH and git daemon branch fetches from existing shallow repos using shallow boundary lines plus the `deepen-relative` capability |
| `fetch --deepen` network multiple explicit refspec forms | `3` | `0` | smart HTTP, SSH and git daemon fetches with two destination refspecs from existing shallow repos using shallow boundary lines plus the `deepen-relative` capability |
| `fetch --deepen` network branchless configured fetch forms | `3` | `0` | smart HTTP, SSH and git daemon configured fetches from existing shallow repos using shallow boundary lines plus the `deepen-relative` capability |
| `fetch --unshallow` explicit local/file branch forms | `2` | `0` | explicit local path and file URL branch fetches in existing shallow repos |
| `fetch --unshallow` explicit local/file HEAD forms | `2` | `0` | explicit local path and file URL HEAD fetches in existing shallow repos |
| `fetch --unshallow` multiple explicit local/file refspec forms | `4` | `0` | named local/file remotes and explicit local/file locations with two destination refspecs in existing shallow repos |
| `fetch --unshallow` network branch forms | `3` | `0` | smart HTTP, SSH and git daemon branch fetches from existing shallow repos using shallow boundary lines plus an absolute deepen request |
| `fetch --unshallow` network multiple explicit refspec forms | `3` | `0` | smart HTTP, SSH and git daemon fetches with two destination refspecs from existing shallow repos using shallow boundary lines plus an absolute deepen request |
| `fetch --unshallow` network branchless configured fetch forms | `3` | `0` | smart HTTP, SSH and git daemon configured fetches from existing shallow repos using shallow boundary lines plus an absolute deepen request |
| `fetch --update-shallow` local/file remote forms | `6` | `0` | named and explicit local path/file URL branch plus explicit HEAD fetches where the remote itself is shallow |
| `fetch --update-shallow` multiple explicit local/file refspec forms | `4` | `0` | named local/file remotes and explicit local/file locations with two destination refspecs where the remote itself is shallow |
| `fetch --update-shallow` network branch forms | `3` | `0` | smart HTTP, SSH and git daemon branch fetches from shallow remotes using advertised shallow boundaries when the response does not repeat them |
| `fetch --update-shallow` network multiple explicit refspec forms | `3` | `0` | smart HTTP, SSH and git daemon fetches with two destination refspecs from shallow remotes using advertised shallow boundaries when the response does not repeat them |
| `fetch --update-shallow` network branchless configured fetch forms | `3` | `0` | smart HTTP, SSH and git daemon configured fetches from shallow remotes using advertised shallow boundaries when the response does not repeat them |
| `fetch --filter=blob:none` local/file branch forms | `2` | `0` | named local path and file URL remotes match stock stdout/stderr, FETCH_HEAD, remote-tracking ref, fetched content and promisor filter config |
| `fetch --filter=blob:none --depth=1` file URL shallow client | `1` | `0` | named file URL remote from an existing shallow client matches stock stdout/stderr, FETCH_HEAD, remote-tracking ref, fetched content, `.git/shallow` and promisor filter config |
| `fetch` replacement prune/no-tags | `1` | `0` | `--prune --no-tags` through the `git` shim updates `origin/main`, prunes stale `origin/gone`, skips a newly reachable tag and matches stock stdout/stderr/FETCH_HEAD |
| `fetch --no-tags` explicit local path HEAD | `1` | `0` | explicit local-path HEAD fetch with `--no-tags` writes stock-shaped `FETCH_HEAD`, creates no destination refs and skips auto-following a reachable tag |
| `fetch --recurse-submodules local/file no-submodule value modes` | `9` | `0` | implicit yes, explicit yes, boolean true, numeric true, on-demand, no, boolean false, numeric false and `--no-recurse-submodules` for repositories without submodules |
| `fetch --recurse-submodules local/file changed submodule modes` | `9` | `0` | implicit yes, explicit yes, boolean true, numeric true and on-demand fetch the changed gitlink commit into the initialized submodule object database without checkout; no, boolean false, numeric false and `--no-recurse-submodules` update only the parent fetch |
| `fetch --recurse-submodules smart HTTP initialized local submodule modes` | `9` | `0` | smart HTTP parent fetch with an initialized local submodule remote: implicit yes, explicit yes, boolean true, numeric true and on-demand fetch the changed submodule commit; no, boolean false, numeric false and `--no-recurse-submodules` update only the parent fetch |
| `fetch --recurse-submodules smart HTTP uninitialized submodule modes` | `9` | `0` | implicit yes, explicit yes, boolean true, numeric true, on-demand, no, boolean false, numeric false and `--no-recurse-submodules` for smart HTTP parent fetches where the submodule is present in the index but not initialized locally |
| `fetch --recurse-submodules smart HTTP nested initialized submodule` | `1` | `0` | implicit yes for smart HTTP parent fetches with initialized local submodule remotes and initialized nested submodule remotes |
| `fetch --recurse-submodules network parent local submodule transports` | `2` | `0` | on-demand recursion with SSH and git-daemon parent remotes plus initialized local submodule remotes |
| `fetch --recurse-submodules smart HTTP submodule remote` | `1` | `0` | on-demand recursion with smart HTTP parent and smart HTTP submodule remote fetches the changed gitlink commit without checking it out |
| `fetch --recurse-submodules SSH/git-daemon submodule remotes` | `2` | `0` | on-demand recursion with smart HTTP parent and SSH or git-daemon submodule remotes fetches the changed gitlink commit without checking it out |
| `fetch --jobs submodule recursion values` | `2` | `1` | accepted `--jobs=2` and `-j -1` with smart HTTP parent/local submodule recursion, plus invalid non-integer `--jobs`/`-j` diagnostics |
| `fetch --dry-run smart HTTP submodule recursion` | `2` | `0` | default/on-demand and explicit `--recurse-submodules` smart HTTP parent/local-submodule dry-runs leave parent refs and `FETCH_HEAD` unchanged while fetching the changed submodule object like stock Git |
| `fetch` invalid bundle file | `1` | `0` | `git fetch bad.bundle HEAD:refs/heads/from-bundle` exits `128` with stock unreadable-remote diagnostics, writes empty `FETCH_HEAD`, leaves the destination ref absent and installs no pack files |
| `ls-remote` HTTP non-chunked transfer encoding | `1` | `0` | `git ls-remote --refs` accepts a plain info/refs response with `Transfer-Encoding: gzip` and reads the body unchanged like stock Git |
| `maintenance run` invalid schedule/task usage | `4` | `0` | `--schedule=invalid`, `--schedule=invalid --task=gc`, `--auto --schedule=daily`, and `--task=missing --schedule=daily` match stock invalid-input diagnostics |
| `prune --expire` loose-object expiry modes | `7` | `0` | default expiry, `--expire=now` dry-run/prune/reflog-reachable modes, `--expire=never` with `--no-exclude-promisor-objects`, negated dry-run/verbose flags, and invalid `--expire=bogus` diagnostics |
| `commit-graph verify` header variants | `3` | `0` | checksum-valid bad signature, bad version and bad hash version commit-graph files reject with stock diagnostics and exit `1` |
| `pack-objects --index-version` value variants | `10` | `0` | accepted numeric major/minor forms `0`, `0,64`, `1,64`, `2,128` plus invalid `3`, `3,0`, `foo`, `2,foo`, `1,foo`, and `-1` diagnostics |
| `bundle create --version` value variants | `8` | `0` | accepted `2`, `3` and `-1` forms plus invalid `1`, `4`, `1k`, `foo` and empty-value diagnostics with matching bundle file side effects |
| `bundle` unsupported bundle format subcommands | `3` | `0` | `verify`, `list-heads` and `unbundle` reject files without a v2/v3 bundle header with stock exit `1`, diagnostics and no pack side effects |
| `show-index` unsupported pack-index version | `1` | `0` | checksum-valid pack index version `3` from stdin rejects with stock unknown-index-version diagnostics and exit `128` |
| `submodule` subcommand unknown option usage | `6` | `0` | `git submodule add/status/update/deinit/set-branch/summary --bad` exit `1` with stock usage text instead of custom unsupported-option fatal diagnostics |
| `submodule` unknown subcommand usage | `1` | `0` | `git submodule bogus` exits `1` with stock usage text instead of a custom unsupported-subcommand fatal diagnostic |
| `sparse-checkout` unknown subcommand usage | `1` | `0` | `git sparse-checkout bogus` exits `129` with stock unknown-subcommand usage text instead of a custom unsupported-subcommand fatal diagnostic |
| `worktree` unknown subcommand usage | `1` | `0` | `git worktree bogus` exits `129` with stock unknown-subcommand usage text instead of a custom unsupported-subcommand fatal diagnostic |
| `bisect` visualize unknown option usage | `1` | `0` | `git bisect visualize --bad` after `bisect start` exits `128` with stock fatal diagnostic instead of a custom unsupported-option fatal diagnostic |
| `cat-file` batch unknown atom usage | `1` | `0` | `git cat-file --batch='%(bad)'` exits `128` with stock fatal bad-format diagnostic instead of a custom unsupported-atom fatal diagnostic |
| `cat-file --filter` unknown object-filter names | `2` | `0` | `bad:name` and `bad=name` values with `--batch` exit `128` with stock invalid-filter diagnostics instead of using the known-unsupported-filter usage path |
| `cat-file --filter` known unsupported object-filter families | `3` | `0` | `tree:1`, `sparse:oid=deadbeef` and `combine:blob:none+tree:1` values with `--batch` exit `129` with stock unsupported-filter usage diagnostics |
| `cat-file --filter` dropped sparse path filter | `1` | `0` | `sparse:path=foo` with `--batch` exits `128` with stock support-dropped fatal diagnostic |
| `for-each-ref` date format atoms | `16` | `0` | `committerdate` and `taggerdate` in default, `unix`, `raw`, `iso`, `iso-strict`, `rfc`, `rfc2822`, and `short` formats |
| `for-each-ref` author atoms | `10` | `0` | `authorname`, `authoremail`, and `authordate` in default, `unix`, `raw`, `iso`, `iso-strict`, `rfc`, `rfc2822`, and `short` formats |
| `for-each-ref` tagger identity atoms | `2` | `0` | `taggername` and `taggeremail` for commit refs and annotated tag refs |
| `for-each-ref` committer identity atoms | `2` | `0` | `committername` and `committeremail` for commit refs and annotated tag refs |
| `for-each-ref` object size atom | `4` | `0` | `objectsize` for commit, annotated tag, blob, and tree refs |
| `for-each-ref` object size sort key | `1` | `0` | `--sort=objectsize` across commit, annotated tag, blob, and tree refs |
| `for-each-ref` refname strip modifiers | `10` | `0` | `refname:lstrip/rstrip` with zero, positive, and negative counts plus matching sort keys |
| `for-each-ref` creator atoms | `18` | `0` | `creator` and `creatordate` formats for commit refs and annotated tag refs |
| `for-each-ref` object id abbreviation lengths | `3` | `0` | `objectname:short=4`, `objectname:short=12`, and `objectname:short=40` |
| `for-each-ref` invalid object id abbreviation lengths | `4` | `0` | `objectname:short=0`, `objectname:short=abc`, `objectname:short=-1`, and `objectname:short=` fatal diagnostics |
| `for-each-ref` invalid refname strip values | `6` | `0` | `refname:lstrip=abc`, `refname:lstrip=`, `refname:rstrip=abc`, `refname:rstrip=`, and matching invalid sort keys |
| `reflog expire` default policy forms | `6` | `0` | empty args, `main`, `HEAD`, `--updateref main`, `--rewrite main`, `--verbose main` |
| `reflog --date` display modes | `8` | `0` | `default`, `local`, `iso-strict`, `rfc`, `rfc2822`, `short`, `relative`, `human` |
| `reflog --date` invalid format usage | `1` | `0` | `git reflog --date=bogus` exits `128` with stock fatal diagnostic instead of a custom unsupported-date fatal diagnostic |

Tracked closed blocks in this table: `860` verified variants.

This is closed evidence only, not the full Git denominator. A denominator is
valid only after the matching command group is expanded into command plus
option plus value plus option combination plus repository state plus transport
workflow plus platform.

The global denominator is still being audited. Until then, do not publish a
global compatibility percentage.

## Deferred Guard Classifications

Deferred guards are not closed Git compatibility rows. They remain visible
until a later slice either provides a real stock-Git oracle and matrix row or
keeps the behavior explicitly out of scope.

| Guard | Classification | Evidence | Next action |
| --- | --- | --- | --- |
| `commit_impl.rs` `git gui` external GUI commands | intentionally external GUI integration, not counted as closed compatibility | local stock Git lists `gui` in `git help -a`, but the broader GUI surface still lacks a durable non-interactive oracle beyond the closed `citool` helper rows | revisit only with a real `git-gui` oracle environment or an explicit decision to bring the remaining GUI surface into current CLI scope |
| `admin_impl.rs` `unsupported svn command '{command}'` in `git svn` dispatch | legacy external bridge deferral, not counted as closed compatibility | local stock `git-svn` helper is unusable on this arm64 host: `git svn --version` fails before helper startup because Perl loads an x86_64 `SVN::_Core` bundle | revisit only with a real `git-svn` oracle environment or an explicit decision to scope the legacy SVN bridge |
| `admin_impl.rs` `unsupported archimport option '{arg}'` and intentionally unsupported `git archimport -o` mode | legacy external bridge deferral, not counted as closed compatibility | local stock Git was built with `NO_PERL=1`: `git archimport -h` fails before helper dispatch with `fatal: git was built without support for git-archimport (NO_PERL=1).` | revisit only with a real `git-archimport` oracle environment or an explicit decision to scope the legacy GNU Arch bridge |
| `checkout.rs` non-UTF8 index paths on non-Unix targets | platform-oracle deferral, not counted as closed compatibility | the guard is `#[cfg(not(unix))]`; the current macOS oracle host rejects a `bad-\xff.txt` filesystem path with `Illegal byte sequence` before stock Git checkout behavior can be observed | revisit with a Windows/non-Unix oracle that can create or import a repository/index containing the relevant path bytes |
| `runtime/primitive_adapters.rs` `unsupported object id length`, `unsupported object type ... for patch render`, `unsupported git object type` and `transport discovery is not supported` | internal primitive-runtime validation, not counted as Git `2.47.1` CLI compatibility | source search shows these guards are reached through `GitPrimitiveRuntime` adapters rather than a `git <command>` entry point; no stock-Git CLI oracle applies to the primitive API contract | cover with dedicated primitive API tests before stabilizing the shared runtime; add a Git matrix row only if a Git-compatible CLI path exposes the behavior |

## Open Code Guard Mappings

Open guard mappings are known Git-supported behaviors where Zmin currently
does not match stock Git. They must stay visible in counts and matrix rows
until implementation and focused parity evidence close them.

| Source guard | Classification | Matrix / evidence |
| --- | --- | --- |

## Closed Code Guard Mappings

Closed guard mappings connect raw source hits from the `unsupported` scan to
specific oracle rows. They do not increase the Git compatibility denominator;
they only prove that the source hit is already represented as stock-compatible
behavior, invalid input, or corrupt-format handling.

| Source guard | Classification | Matrix / evidence |
| --- | --- | --- |
| `history_impl.rs` `reject_unsupported_filter_branch_options` plus former `parent filter emitted unsupported token` in `git filter-branch --parent-filter` output parsing | Git-supported behavior now mapped to stock commit-tree failure after rewrite progress; the option-rejection helper is currently a no-op after supported filter/options coverage was added | `docs/cli/matrices/filter_branch_v2_47.tsv`; `git_filter_branch_parent_filter_compat::filter_branch_parent_filter_bad_token_matches_stock_git`; `git_filter_branch_compat` |
| `pack_impl.rs` `unsupported index version {value}` in `pack-objects --index-version` parsing | stock-compatible invalid input for unsupported requested pack-index major versions | `docs/cli/matrices/pack_objects_v2_47.tsv`; `git_pack_integrity_compat::pack_objects_index_version_values_match_stock_git` |
| `pack_impl.rs` `unsupported bundle version {other}` in `bundle create --version` parsing | stock-compatible invalid input for unsupported bundle versions | `docs/cli/matrices/bundle_v2_47.tsv`; `git_pack_integrity_compat::bundle_create_version_values_match_stock_git` |
| `pack_impl.rs` `unsupported bundle format` in bundle header parsing | corrupt/invalid bundle input; mapped differently for bundle subcommands and fetch-from-bundle | `docs/cli/matrices/bundle_v2_47.tsv`; `docs/cli/matrices/fetch_v2_47.tsv`; `git_pack_integrity_compat::bundle_subcommands_reject_unsupported_bundle_format_like_stock_git`; `git_transport_local_compat::fetch_invalid_bundle_file_matches_stock_git` |
| `pack_impl.rs` `pack version {version} unsupported` adapters for pack verification commands | corrupt/unsupported pack storage format with stock diagnostics | `docs/cli/matrices/index_pack_v2_47.tsv`; `docs/cli/matrices/verify_pack_v2_47.tsv`; `git_pack_integrity_compat::index_pack_verify_rejects_unsupported_pack_file_version_like_stock_git`; `git_pack_integrity_compat::verify_pack_rejects_unsupported_pack_file_version_like_stock_git` |
| `zmin-git-core/src/pack.rs` `unsupported pack index version {raw_version}` while reading pack indexes | corrupt/unsupported pack-index storage format with command-specific stock diagnostics | `docs/cli/matrices/show_index_v2_47.tsv`; `docs/cli/matrices/verify_pack_v2_47.tsv`; `git_object_plumbing_compat::show_index_rejects_unsupported_pack_index_version_like_stock_git`; `git_pack_integrity_compat::verify_pack_rejects_unsupported_pack_index_version_like_stock_git` |
| `zmin-git-core/src/pack.rs` `unsupported pack reverse index signature` and `unsupported pack reverse index version` | corrupt/unsupported reverse-index sidecar files validated through `index-pack --verify` | `docs/cli/matrices/index_pack_v2_47.tsv`; `git_pack_integrity_compat::index_pack_verify_rejects_bad_reverse_index_signature_like_stock_git`; `git_pack_integrity_compat::index_pack_verify_rejects_bad_reverse_index_version_like_stock_git` |
| `zmin-git-core/src/pack.rs` `unsupported pack file version {version}` while reading pack headers | corrupt/unsupported pack storage format with command-specific stock diagnostics | `docs/cli/matrices/index_pack_v2_47.tsv`; `docs/cli/matrices/verify_pack_v2_47.tsv`; `git_pack_integrity_compat::index_pack_verify_rejects_unsupported_pack_file_version_like_stock_git`; `git_pack_integrity_compat::verify_pack_rejects_unsupported_pack_file_version_like_stock_git` |
| `runtime/commit_graph.rs` `unsupported commit-graph header` while reading commit-graph files | corrupt commit-graph storage format with stock diagnostics through `commit-graph verify` | `docs/cli/matrices/commit_graph_v2_47.tsv`; `git_maintenance_compat::commit_graph_verify_header_variants_match_stock_git` |
| `core_impl.rs` `usage: objects filter not supported` path for unknown `cat-file --filter` names | stock-compatible invalid input for arbitrary unknown object-filter names, while known unsupported object-filter families keep stock usage diagnostics | `docs/cli/matrices/cat_file_v2_47.tsv`; `git_object_plumbing_compat::cat_file_unknown_filter_names_match_stock_git` |
| `core_impl.rs` `cat_file_filter_is_known_unsupported` / `usage: objects filter not supported` path for known `cat-file --filter` families | stock-compatible invalid input for documented-but-unsupported object-filter families that stock Git rejects with usage diagnostics | `docs/cli/matrices/cat_file_v2_47.tsv`; `git_object_plumbing_compat::cat_file_known_unsupported_filters_match_stock_git` |
| `core_impl.rs` `sparse:path filters support has been dropped` path in `cat-file --filter` parsing | stock-compatible invalid input for the dropped sparse path object-filter family | `docs/cli/matrices/cat_file_v2_47.tsv`; `git_object_plumbing_compat::cat_file_sparse_path_filter_matches_stock_git` |
| `import_impl.rs` former `unsupported fast-import command` path for top-level `checkpoint` | Git-supported stream command now accepted and stock-shaped fast-import statistics stderr is emitted after successful import | `docs/cli/matrices/fast_import_v2_47.tsv`; `git_fast_import_date_compat::fast_import_checkpoint_matches_stock_git_statistics` |
| `import_impl.rs` former `unsupported fast-import command` path for top-level `progress` | Git-supported stream command now accepted and stock-shaped fast-import statistics stderr is emitted after successful import | `docs/cli/matrices/fast_import_v2_47.tsv`; `git_fast_import_date_compat::fast_import_progress_matches_stock_git_statistics` |
| `import_impl.rs` former `unsupported fast-import command` path for top-level `done` | Git-supported stream terminator now accepted and stock-shaped fast-import statistics stderr is emitted after successful import | `docs/cli/matrices/fast_import_v2_47.tsv`; `git_fast_import_date_compat::fast_import_done_matches_stock_git_statistics` |
| `import_impl.rs` `unsupported_fast_import_command` path for unknown top-level and commit-body stream commands | stock-compatible invalid input for unknown fast-import stream commands that trigger crash reports | `docs/cli/matrices/fast_import_v2_47.tsv`; `git_fast_import_date_compat::fast_import_unknown_top_level_command_matches_stock_git_crash_shape`; `git_fast_import_date_compat::fast_import_unknown_commit_command_matches_stock_git_crash_shape` |
| `diff_render.rs` `unsupported --diff-filter status` parser path | stock-compatible invalid input for unknown `--diff-filter` change classes | `docs/cli/matrices/diff_v2_47.tsv`; `git_diff_compat::diff_filter_invalid_change_classes_match_stock_git` |
| `maintenance_impl.rs` `unsupported maintenance schedule '{schedule}'` in `maintenance run` strategy selection | stock-compatible invalid input for invalid `maintenance run --schedule` values and schedule/task combinations | `docs/cli/matrices/maintenance_v2_47.tsv`; `git_maintenance_compat::maintenance_run_invalid_schedule_failure_matches_stock_git`; `git_maintenance_compat::maintenance_run_schedule_task_and_auto_failures_match_stock_git` |
| `maintenance_impl.rs` former `unsupported maintenance scheduler '{other}'` guard in `maintenance start` scheduler selection | stock-compatible invalid input for unknown `maintenance start --scheduler=<value>` values | `docs/cli/matrices/maintenance_v2_47.tsv`; `git_maintenance_compat::maintenance_unknown_scheduler_failure_matches_stock_git` |
| `maintenance_impl.rs` former `unsupported prune expiry '{value}'` guard in `git prune --expire` parsing | stock-compatible invalid input for malformed `prune --expire=<value>` dates | `docs/cli/matrices/prune_v2_47.tsv`; `git_maintenance_compat::prune_invalid_expire_value_matches_stock_git` |
| `reference_impl.rs` `unsupported repo output format '{other}'` in `zmin repo` output parsing | Zmin-only extension validation, not part of the Git `2.47.1` denominator | `docs/cli/zmin_extensions_inventory.md`; `git_admin_tools_compat::repo_command_is_tracked_zmin_only_extension`; `/usr/bin/git repo -h` reports that stock Git has no `repo` command |
| `text_impl.rs` `unsupported option '{other}'` in `git column --mode=<value>` parsing | stock-compatible invalid input for unsupported column mode tokens | `docs/cli/matrices/column_v2_47.tsv`; `git_text_tools_compat::column_matches_stock_git_for_common_modes`; `/usr/bin/git column --mode=bogus` exits `129` with `error: unsupported option 'bogus'` |
| `history_impl.rs` `unsupported_blame_line_range` fallback in `git blame -L` parsing/resolution | stock-compatible invalid input and supported rich range behavior for expanded blame line-range forms | `docs/cli/matrices/blame_v2_47.tsv`; `git_history_query_compat::blame_zero_line_range_matches_stock_git_failure`; `git_history_query_compat::blame_invalid_regex_line_ranges_match_stock_git_failure`; `git_history_query_compat::blame_basic_regex_invalid_backreferences_match_stock_git_failure` |
| `transport_impl.rs` `unknown ref storage format` in `git clone --ref-format=<value>` parsing | stock-compatible invalid input for unknown clone ref storage values | `docs/cli/matrices/clone_v2_47.tsv`; `git_clone_ref_format_compat::clone_ref_format_unknown_value_matches_stock_git` |
| `transport_impl.rs` `reftable ref storage is not supported yet` in `git clone --ref-format=reftable` parsing | Git-supported clone ref storage now matches stock Git for local reftable clone creation, `rev-parse --show-ref-format`, `.git/reftable` layout and HEAD resolution | `docs/cli/matrices/clone_v2_47.tsv`; `git_clone_ref_format_compat::clone_ref_format_reftable_matches_stock_git` |
| `transport_impl.rs` `unsupported_remote_helper_error` / `unsupported_clone_destination_label` in `git clone` remote-helper URL handling | stock-compatible invalid input for unsupported remote-helper protocols; the destination-label helper only renders the stock-shaped clone prefix before the remote-helper failure | `docs/cli/matrices/clone_v2_47.tsv`; `git_clone_compat::clone_unsupported_remote_helper_failure_matches_stock_git` |
| `transport_impl.rs` `unsupported_remote_helper_error` in `git fetch` remote-helper URL handling | stock-compatible invalid input for unsupported remote-helper protocols | `docs/cli/matrices/fetch_v2_47.tsv`; `git_transport_local_compat::fetch_and_push_unsupported_remote_helper_failures_match_stock_git` |
| `transport_impl.rs` `unsupported_remote_helper_error` in `git ls-remote` remote-helper URL handling | stock-compatible invalid input for unsupported remote-helper protocols | `docs/cli/matrices/ls_remote_v2_47.tsv`; `git_transport_local_compat::ls_remote_unsupported_remote_helper_failure_matches_stock_git` |
| `transport_impl.rs` former `unsupported HTTP transfer encoding` response-header guard in `git ls-remote` HTTP discovery | Git-supported plain info/refs response with a non-`chunked` transfer-coding token is accepted and read unchanged like stock Git | `docs/cli/matrices/ls_remote_v2_47.tsv`; `git_transport_http_compat::ls_remote_accepts_non_chunked_transfer_encoding_like_stock_git` |
| `transport_impl.rs` `unsupported_remote_helper_error` in `git push` remote-helper URL handling | stock-compatible invalid input for unsupported remote-helper protocols | `docs/cli/matrices/push_v2_47.tsv`; `git_transport_local_compat::fetch_and_push_unsupported_remote_helper_failures_match_stock_git` |
| `transport_impl.rs` former named-remote branch tag omission for `git fetch --tags origin main` | Git-supported named local remote branch fetch with `--tags` now follows annotated and lightweight tags, writes tag rows to `FETCH_HEAD`, emits stock-shaped branch/tag/remote-tracking update stderr and keeps `refs/remotes/origin/main` updated | `docs/cli/matrices/fetch_v2_47.tsv`; `git_transport_local_compat::fetch_tags_named_local_remote_branch_follows_tags_like_stock_git` |
| `transport_impl.rs` former branch-without-destination tag omission for `git fetch --tags <path> main` | Git-supported explicit local-path branch fetch with `--tags` now follows annotated and lightweight tags, writes tag rows to `FETCH_HEAD` and emits stock-shaped branch/tag update stderr | `docs/cli/matrices/fetch_v2_47.tsv`; `git_transport_local_compat::fetch_tags_direct_location_branch_follows_tags_like_stock_git` |
| `transport_impl.rs` former `fetch from explicit location '{location}' is not supported yet` path for `git fetch <path> main:bad:ref` | stock-compatible invalid input for malformed explicit local path refspecs; exits `128` with stock invalid-refspec stderr and creates no `FETCH_HEAD` | `docs/cli/matrices/fetch_v2_47.tsv`; `git_transport_local_compat::fetch_malformed_explicit_location_refspec_matches_stock_git_failure` |
| `transport_impl.rs` former `fetch from explicit location ... is not supported yet` path for `git fetch <path> :` | Git-supported explicit local-path empty-colon refspec now fetches HEAD to `FETCH_HEAD`, emits stock-shaped HEAD update stderr and creates no destination refs | `docs/cli/matrices/fetch_v2_47.tsv`; `git_transport_local_compat::fetch_empty_colon_refspec_explicit_location_head_matches_stock_git` |
| `transport_impl.rs` former `fetch from explicit location ... is not supported yet` path for `git fetch <path> main:` | Git-supported explicit local-path source-only branch refspec now fetches to `FETCH_HEAD`, emits stock-shaped branch update stderr and creates no destination refs | `docs/cli/matrices/fetch_v2_47.tsv`; `git_transport_local_compat::fetch_source_only_refspec_explicit_location_branch_matches_stock_git` |
| `transport_impl.rs` former `fetch from explicit location ... is not supported yet` path for `git fetch --prune <path> main` | Git-supported explicit local-path branch fetch with `--prune` now routed to the branch-without-destination handler and writes only `FETCH_HEAD` | `docs/cli/matrices/fetch_v2_47.tsv`; `git_transport_local_compat::fetch_prune_direct_location_branch_fetch_head_matches_stock_git` |
| `transport_impl.rs` former `fetch from explicit location ... is not supported yet` path for `git fetch --prune <path> main:refs/remotes/origin/main` | Git-supported explicit local-path one-to-one refspec with `--prune` now routed to the direct refspec handler | `docs/cli/matrices/fetch_v2_47.tsv`; `git_transport_local_compat::fetch_prune_direct_location_one_to_one_refspec_matches_stock_git` |
| `transport_impl.rs` former `reference repositories are not supported for dumb HTTP clone yet` guard | Git-supported clone reference and reference-if-able behavior now accepted for dumb HTTP sources with stock-shaped alternates | `docs/cli/matrices/clone_v2_47.tsv`; `git_transport_http_compat::clone_reference_dumb_http_matches_stock_git`; `git_transport_http_compat::clone_reference_if_able_dumb_http_matches_stock_git` |
| `runtime/worktree_index.rs` former `unsupported filter capability` process-filter handshake guard | stock-compatible invalid input for a required process filter that replies with an unsupported capability during `git add` | `docs/cli/matrices/add_v2_47.tsv`; `git_add_compat::add_process_filter_unknown_capability_matches_stock_git` |
| `runtime/worktree_index.rs` former `unsupported filter process status` process-filter response guard | stock-compatible invalid input for a required process filter that returns an unsupported status during `git add` | `docs/cli/matrices/add_v2_47.tsv`; `git_add_compat::add_process_filter_unknown_status_matches_stock_git` |
| `runtime/worktree_index.rs` former `filter process delayed response is not supported here` clean-filter response guard | stock-compatible invalid input for a required clean process filter that returns delayed status without a `can-delay=1` request during `git add` | `docs/cli/matrices/add_v2_47.tsv`; `git_add_compat::add_process_filter_delayed_clean_status_matches_stock_git` |
| `runtime/worktree_index.rs` former `filter process delayed response is not supported here` smudge checkout-index response guard | stock-compatible invalid input for a required smudge process filter that returns delayed status without advertising `delay` during `git checkout-index` | `docs/cli/matrices/checkout_index_v2_47.tsv`; `git_worktree_state_compat::checkout_index_delayed_smudge_process_filter_matches_stock_git_failure` |
| `runtime/worktree_index.rs` former `filter process delayed response is not supported here` smudge checkout response guard | stock-compatible invalid input for a required smudge process filter that returns delayed status during `git checkout -- <path>`; newly materialized raw files are removed on the smudge failure like stock Git | `docs/cli/matrices/checkout_v2_47.tsv`; `git_worktree_state_compat::checkout_delayed_smudge_process_filter_matches_stock_git_failure` |
| `worktree_impl.rs` `unsupported clean option '{value}'` in `git clean` option parsing | stock-compatible invalid input for unknown long and short clean options | `docs/cli/matrices/clean_v2_47.tsv`; `git_clean_compat::clean_unknown_option_matches_stock_git`; `git_clean_compat::clean_unknown_short_switch_matches_stock_git` |
| `worktree_impl.rs` `unsupported porcelain version '{value}'` in `git status --porcelain=<value>` parsing | stock-compatible invalid input for unsupported porcelain version values | `docs/cli/matrices/status_v2_47.tsv`; `git_status_compat::status_invalid_porcelain_version_matches_stock_git` |
| `worktree_impl.rs` former `unsupported stash list format atom '%w'` guard | Git-supported stash list pretty-format wrapping atom | `docs/cli/matrices/stash_v2_47.tsv`; `git_stash_compat::stash_list_wrap_format_atoms_match_stock_git` |
| `worktree_impl.rs` `unsupported_stash_list_format_atom` width-target guard | Git-supported stash list width atoms with no following target atom | `docs/cli/matrices/stash_v2_47.tsv`; `git_stash_compat::stash_list_width_format_atoms_without_target_match_stock_git` |
| `worktree_impl.rs` `unsupported_stash_list_format_atom` width-literal target guard | Git-supported stash list width atoms followed by literal text before an optional later pretty-format atom | `docs/cli/matrices/stash_v2_47.tsv`; `git_stash_compat::stash_list_width_format_atoms_with_literal_target_match_stock_git` |
| `worktree_impl.rs` former `unsupported stash {operation} option '{value}'` guard in `stash apply/drop/pop` option parsing | stock-compatible invalid input for unknown reference-style stash operation options | `docs/cli/matrices/stash_v2_47.tsv`; `git_stash_compat::stash_reference_unknown_options_match_stock_git_usage` |
| `worktree_impl.rs` `ls-files --recurse-submodules unsupported mode` guard | stock-compatible invalid input for unsupported recurse-submodules combinations | `docs/cli/matrices/ls_files_v2_47.tsv`; `git_ls_files_compat::ls_files_recurse_submodules_matches_stock_git` |
| `admin_impl.rs` `unsupported hook '{hook_name}'` in `zmin hooks` managed-hook validation | Zmin-only extension validation, not part of the Git `2.47.1` denominator | `docs/cli/zmin_extensions_inventory.md`; `git_admin_tools_compat::managed_hooks_add_list_remove_and_protect_manual_hooks`; `git_admin_tools_compat::managed_hooks_reject_unsupported_hook_names_as_zmin_extension_validation`; `/usr/bin/git hooks -h` reports that stock Git has no `hooks` command |
| `worktree_impl.rs` `error: unsupported option '{other}'` in `git status --column=<value>` parsing | stock-compatible invalid input for unsupported status column mode tokens | `docs/cli/matrices/status_v2_47.tsv`; `git_status_compat::status_invalid_column_mode_matches_stock_git` |
| `transport_impl.rs` `unsupported index-pack option '{arg}'` in `http-fetch --packfile` delegated index-pack parsing | stock-compatible invalid input for invalid delegated `index-pack` arguments | `docs/cli/matrices/http_fetch_v2_47.tsv`; `git_transport_http_compat::http_fetch_packfile_rejects_bad_index_pack_arg_like_stock_git` |
| `runtime/worktree_index.rs` `unsupported object format '{value}'` while reading repository object format | stock-compatible invalid repository config for unsupported local `extensions.objectFormat` values | `docs/cli/matrices/status_v2_47.tsv`; `git_status_compat::status_invalid_object_format_config_matches_stock_git` |
| `transport_impl.rs` `unsupported ZMIN_GIT_HTTP_VERSION '{raw}'` in HTTP remote-helper setup | Zmin-only environment validation, not part of the Git `2.47.1` denominator | `docs/cli/zmin_extensions_inventory.md`; `transport_impl::tests::remote_http_helper_version_arg_rejects_unsupported_values` |
| `runtime/object.rs` `unsupported git object type '{value}'` while reading loose object headers | corrupt loose-object repository input with stock diagnostics through `cat-file -t` | `docs/cli/matrices/cat_file_v2_47.tsv`; `git_cat_file_corrupt_object_compat::cat_file_rejects_unsupported_loose_object_type_like_stock_git` |

## Code Guard Classification

Raw code scan from 2026-06-21:

`rg -n "unsupported|not supported yet|not implemented yet" crates/zmin-cli/src crates/zmin-git-core/src --glob '*.rs'`

This found `92` code hits. This is not the variant denominator and does not
mean `92` Git features are missing. Each hit must be classified as one of:

- Git-supported user variant to implement and test
- parser validation for invalid input
- corrupt or unsupported repository/storage format
- intentionally external or legacy integration
- additive Zmin-only behavior

Largest raw clusters:

| Area | Raw hits | Next action |
| --- | ---: | --- |
| `transport_impl.rs` | `30` | split explicit-location fetch, remote helpers, HTTP/env guards; reftable clone remains open and dumb HTTP clone reference/reference-if-able are now closed |
| `pack_impl.rs` | `14` | classify pack/bundle format guards versus stock-supported variants |
| `worktree_impl.rs` | `9` | split remaining ls-files, submodule, sparse-checkout, stash and worktree guards |
| `history_impl.rs` | `6` | split blame ranges/options, reflog formats and diff/log decorators; bad parent-filter output is now closed |
| `pack.rs` | `6` | classify pack format guards versus corrupt-storage invalid inputs |
| `maintenance_impl.rs` / `core_impl.rs` / `admin_impl.rs` | `12` | classify parser/runtime validation and Zmin-only hook validation |
| remaining files | `15` | classify small parser/runtime guards individually |

## Audit Order

1. Local git-replacement blockers from IDE/GUI dogfood.
2. Option inventory from Git `v2.47.1` docs using
   `tools/git-compat-option-inventory.sh`.
3. Commands with live `unsupported` branches that stock Git accepts, expanded
   into option/mode/state variants before implementation.
4. High-use porcelain variants: `status`, `add`, `commit`, `diff`, `log`,
   `blame`, `stash`, `branch`, `checkout`, `switch`, `restore`.
5. Transport variants: local/file, smart HTTP, SSH, git daemon, depth,
   explicit refspecs, tags, prune, proxy/auth.
6. Plumbing variants used by tools: `cat-file`, `rev-parse`, `for-each-ref`,
   `ls-files`, `update-index`, `read-tree`, `write-tree`.
7. Platform variants: macOS, Linux, Windows path/process behavior.

## 2026-06-25 Census Snapshot

Current durable census after the zero-code reviewed-complete `ls-tree`
documented-option promotion cluster:

- complete command matrices: `82 / 151`
- complete documented command-option pairs: `1326 / 3175`
- matrix rows: `5332`
- verified rows: `4624`
- invalid-input rows: `679`
- open or partial exact rows: `26`

Latest completed batch:

- `ls-tree` now has three represented helper-free documented option families
  promoted into the reviewed-complete doc-option census list, bringing the
  command to `3/13` reviewed-complete documented option pairs.
- The slice adds no new behavior rows; it only makes already closed exact
  stock-Git recursive, tree-entry, and name-only evidence durable in the
  reviewed doc-option source list.
- Focused verification was
  `python3 tools/git-compat-census.py --root .`,
  `tools/git-cli-readiness-status.sh`,
  and `tools/git-compat-command-summary.sh --tsv | rg '^(ls-tree|summary)\t'`.

Latest in-progress family follow-up:

- `ls-tree` now has `3/13` documented option pairs reviewed complete with
  `3/13` represented documented option pairs, `9/9` classified rows, and
  `0` exact-open written rows.
- The next best helper-free follow-up should move to another dense
  census-backed represented family or zero-code command-promotion cluster.

Current durable census after the zero-code reviewed-complete `update-index`
documented-option promotion cluster:

- complete command matrices: `82 / 151`
- complete documented command-option pairs: `1165 / 3175`
- matrix rows: `5332`
- verified rows: `4624`
- invalid-input rows: `679`
- open or partial exact rows: `26`

Latest completed batch:

- `update-index` now has sixteen represented helper-free local documented
  option families promoted into the reviewed-complete doc-option census list,
  bringing the command to `16/38` reviewed-complete documented option pairs.
- The slice adds no new behavior rows; it only makes already closed exact
  stock-Git local index-mutation, flag-toggle, stdin, cacheinfo, index-info,
  replace, chmod, and stat-refresh evidence durable in the reviewed doc-option
  source list.
- Focused verification was
  `python3 tools/git-compat-census.py --root .`,
  `tools/git-cli-readiness-status.sh`,
  and `tools/git-compat-command-summary.sh --tsv | rg '^(update-index|summary)\t'`.

Latest in-progress family follow-up:

- `update-index` now has `16/38` documented option pairs reviewed complete
  with `16/38` represented documented option pairs, `43/43` classified rows,
  and `0` exact-open written rows.
- The next best helper-free follow-up should move to another dense
  census-backed represented family or zero-code command-promotion cluster.

Current durable census after the zero-code reviewed-complete `log`
documented-option promotion cluster:

- complete command matrices: `82 / 151`
- complete documented command-option pairs: `1149 / 3175`
- matrix rows: `5332`
- verified rows: `4624`
- invalid-input rows: `679`
- open or partial exact rows: `26`

Latest completed batch:

- `log` now has eighteen represented helper-free local documented option
  families promoted into the reviewed-complete doc-option census list,
  bringing the command to `18/134` reviewed-complete documented option pairs.
- The slice adds no new behavior rows; it only makes already closed exact
  stock-Git local history-traversal, formatting, decoration, reflog-walk,
  merge-diff, pickaxe, date-format, and replacement-shim evidence durable in
  the reviewed doc-option source list.
- Focused verification was
  `python3 tools/git-compat-census.py --root .`,
  `tools/git-cli-readiness-status.sh`,
  and `tools/git-compat-command-summary.sh --tsv | rg '^(log|summary)\t'`.

Latest in-progress family follow-up:

- `log` now has `18/134` documented option pairs reviewed complete with
  `18/134` represented documented option pairs, `111/111` classified rows,
  and `0` exact-open written rows.
- The next best helper-free follow-up should move to another dense
  census-backed represented family or zero-code command-promotion cluster.

Current durable census after the zero-code reviewed-complete `diff`
documented-option promotion cluster:

- complete command matrices: `82 / 151`
- complete documented command-option pairs: `1131 / 3175`
- matrix rows: `5332`
- verified rows: `4624`
- invalid-input rows: `679`
- open or partial exact rows: `26`

Latest completed batch:

- `diff` now has ten more represented helper-free local documented option
  families promoted into the reviewed-complete doc-option census list,
  bringing the command to `83/117` reviewed-complete documented option pairs.
- The slice adds no new behavior rows; it only makes already closed exact
  stock-Git local treeish-pair, patch-format, ordering, rename-toggle,
  reverse, pickaxe, and no-index evidence durable in the reviewed doc-option
  source list.
- Focused verification was
  `python3 tools/git-compat-census.py --root .`,
  `tools/git-cli-readiness-status.sh`,
  and `tools/git-compat-command-summary.sh --tsv | rg '^(diff|summary)\t'`.

Latest in-progress family follow-up:

- `diff` now has `83/117` documented option pairs reviewed complete with
  `83/117` represented documented option pairs, `250/250` classified rows,
  and `0` exact-open written rows.
- The next best helper-free follow-up should move to another dense
  census-backed represented family or zero-code command-promotion cluster.

Current durable census after the zero-code reviewed-complete `http-fetch`
documented-option promotion cluster:

- complete command matrices: `82 / 151`
- complete documented command-option pairs: `1121 / 3175`
- matrix rows: `5332`
- verified rows: `4624`
- invalid-input rows: `679`
- open or partial exact rows: `26`

Latest completed batch:

- `http-fetch` now has eight represented documented option families promoted
  into the reviewed-complete doc-option census list, bringing the command to
  `8/9` reviewed-complete documented option pairs.
- The slice adds no new behavior rows; it only makes already closed exact
  stock-Git packfile-helper, dumb-http object-fetch, usage-rejection, and
  invalid-input evidence durable in the reviewed doc-option source list.
- Focused verification was
  `python3 tools/git-compat-census.py --root .`,
  `tools/git-cli-readiness-status.sh`,
  and `tools/git-compat-command-summary.sh --tsv | rg '^(http-fetch|summary)\t'`.

Latest in-progress family follow-up:

- `http-fetch` now has `8/9` documented option pairs reviewed complete with
  `8/9` represented documented option pairs, `12/12` classified rows, and
  `0` exact-open written rows.
- The next best helper-free follow-up should move to another dense
  census-backed represented family or zero-code command-promotion cluster.

Current durable census after the zero-code reviewed-complete `mergetool`
documented-option promotion cluster:

- complete command matrices: `82 / 151`
- complete documented command-option pairs: `1113 / 3175`
- matrix rows: `5332`
- verified rows: `4624`
- invalid-input rows: `679`
- open or partial exact rows: `26`

Latest completed batch:

- `mergetool` now has eight represented documented option families promoted
  into the reviewed-complete doc-option census list, bringing the command to
  `8/10` reviewed-complete documented option pairs.
- The slice adds no new behavior rows; it only makes already closed exact
  stock-Git local configured-tool, prompt-toggle, gui-toggle, path-selection,
  and invalid-input evidence durable in the reviewed doc-option source list.
- Focused verification was
  `python3 tools/git-compat-census.py --root .`,
  `tools/git-cli-readiness-status.sh`,
  and `tools/git-compat-command-summary.sh --tsv | rg '^(mergetool|summary)\t'`.

Latest in-progress family follow-up:

- `mergetool` now has `8/10` documented option pairs reviewed complete with
  `8/10` represented documented option pairs, `14/14` classified rows, and
  `0` exact-open written rows.
- The next best helper-free follow-up should move to another dense
  census-backed represented family or zero-code command-promotion cluster.

Current durable census after the zero-code reviewed-complete `fetch-pack`
documented-option promotion cluster:

- complete command matrices: `82 / 151`
- complete documented command-option pairs: `1105 / 3175`
- matrix rows: `5332`
- verified rows: `4624`
- invalid-input rows: `679`
- open or partial exact rows: `26`

Latest completed batch:

- `fetch-pack` now has eleven represented documented option families
  promoted into the reviewed-complete doc-option census list, bringing the
  command to `12/18` reviewed-complete documented option pairs.
- The slice adds no new behavior rows; it only makes already closed exact
  stock-Git local repository, depth, tag, keep, quiet, stdin, thin-pack, and
  invalid-input evidence durable in the reviewed doc-option source list.
- Focused verification was
  `python3 tools/git-compat-census.py --root .`,
  `tools/git-cli-readiness-status.sh`,
  and `tools/git-compat-command-summary.sh --tsv | rg '^(fetch-pack|summary)\t'`.

Latest in-progress family follow-up:

- `fetch-pack` now has `12/18` documented option pairs reviewed complete with
  `12/18` represented documented option pairs, `19/19` classified rows, and
  `0` exact-open written rows.
- The next best helper-free follow-up should move to another dense
  census-backed represented family or zero-code command-promotion cluster.

Current durable census after the zero-code reviewed-complete `daemon`
documented-option promotion cluster:

- complete command matrices: `82 / 151`
- complete documented command-option pairs: `1094 / 3175`
- matrix rows: `5332`
- verified rows: `4624`
- invalid-input rows: `679`
- open or partial exact rows: `26`

Latest completed batch:

- `daemon` now has thirteen represented documented option families promoted
  into the reviewed-complete doc-option census list, bringing the command to
  `13/27` reviewed-complete documented option pairs.
- The slice adds no new behavior rows; it only makes already closed exact
  stock-Git local git-daemon listen, export, timeout, inetd, and path-serving
  evidence durable in the reviewed doc-option source list.
- Focused verification was
  `python3 tools/git-compat-census.py --root .`,
  `tools/git-cli-readiness-status.sh`,
  and `tools/git-compat-command-summary.sh --tsv | rg '^(daemon|summary)\t'`.

Latest in-progress family follow-up:

- `daemon` now has `13/27` documented option pairs reviewed complete with
  `13/27` represented documented option pairs, `14/14` classified rows, and
  `0` exact-open written rows.
- The next best helper-free follow-up should move to another dense
  census-backed represented family or zero-code command-promotion cluster.

Current durable census after the zero-code reviewed-complete `filter-branch`
documented-option promotion cluster:

- complete command matrices: `82 / 151`
- complete documented command-option pairs: `1081 / 3175`
- matrix rows: `5332`
- verified rows: `4624`
- invalid-input rows: `679`
- open or partial exact rows: `26`

Latest completed batch:

- `filter-branch` now has fourteen represented documented option families
  promoted into the reviewed-complete doc-option census list, bringing the
  command to `14/16` reviewed-complete documented option pairs.
- The slice adds no new behavior rows; it only makes already closed exact
  stock-Git local message, tree, index, env, parent, setup, tag rename,
  state-branch, temp-dir, backup-ref, and invalid-input evidence durable in
  the reviewed doc-option source list.
- Focused verification was
  `python3 tools/git-compat-census.py --root .`,
  `tools/git-cli-readiness-status.sh`,
  and `tools/git-compat-command-summary.sh --tsv | rg '^(filter-branch|summary)\t'`.

Latest in-progress family follow-up:

- `filter-branch` now has `14/16` documented option pairs reviewed complete
  with `14/16` represented documented option pairs, `17/17` classified rows,
  and `0` exact-open written rows.
- The next best helper-free follow-up should move to another dense
  census-backed represented family or zero-code command-promotion cluster.

Current durable census after the zero-code reviewed-complete `show`
documented-option promotion cluster:

- complete command matrices: `82 / 151`
- complete documented command-option pairs: `1067 / 3175`
- matrix rows: `5332`
- verified rows: `4624`
- invalid-input rows: `679`
- open or partial exact rows: `26`

Latest completed batch:

- `show` now has three represented helper-free local documented option
  families promoted into the reviewed-complete doc-option census list,
  bringing the command to `3/15` reviewed-complete documented option pairs.
- The slice adds no new behavior rows; it only makes already closed exact
  stock-Git local root-commit, object-display, pretty-format, patch/stat,
  merge-display, and pathspec evidence durable in the reviewed doc-option
  source list.
- Focused verification was
  `python3 tools/git-compat-census.py --root .`,
  `tools/git-cli-readiness-status.sh`,
  and `tools/git-compat-command-summary.sh --tsv | rg '^(show|summary)\t'`.

Latest in-progress family follow-up:

- `show` now has `3/15` documented option pairs reviewed complete with
  `3/15` represented documented option pairs, `38/38` classified rows, and
  `0` exact-open written rows.
- The next best helper-free follow-up should move to another dense
  census-backed represented family or zero-code command-promotion cluster.

Current durable census after the zero-code reviewed-complete `worktree`
documented-option promotion cluster:

- complete command matrices: `82 / 151`
- complete documented command-option pairs: `1064 / 3175`
- matrix rows: `5332`
- verified rows: `4624`
- invalid-input rows: `679`
- open or partial exact rows: `26`

Latest completed batch:

- `worktree` now has eight represented helper-free local documented option
  families promoted into the reviewed-complete doc-option census list,
  bringing the command to `8/24` reviewed-complete documented option pairs.
- The slice adds no new behavior rows; it only makes already closed exact
  stock-Git local add, move, lock, prune, repair, config-scope,
  branch-option, and invalid-input evidence durable in the reviewed
  doc-option source list.
- Focused verification was
  `python3 tools/git-compat-census.py --root .`,
  `tools/git-cli-readiness-status.sh`,
  and `tools/git-compat-command-summary.sh --tsv | rg '^(worktree|summary)\t'`.

Latest in-progress family follow-up:

- `worktree` now has `8/24` documented option pairs reviewed complete with
  `8/24` represented documented option pairs, `17/17` classified rows, and
  `0` exact-open written rows.
- The next best helper-free follow-up should move to another dense
  census-backed represented family or zero-code command-promotion cluster.

Current durable census after the zero-code reviewed-complete `cat-file`
documented-option promotion cluster:

- complete command matrices: `82 / 151`
- complete documented command-option pairs: `1056 / 3175`
- matrix rows: `5332`
- verified rows: `4624`
- invalid-input rows: `679`
- open or partial exact rows: `26`

Latest completed batch:

- `cat-file` now has sixteen represented helper-free local documented option
  families promoted into the reviewed-complete doc-option census list,
  bringing the command to `16/21` reviewed-complete documented option pairs.
- The slice adds no new behavior rows; it only makes already closed exact
  stock-Git local batch, batch-check, batch-command, typed-object, path,
  filter, textconv, promisor, and invalid-input evidence durable in the
  reviewed doc-option source list.
- Focused verification was
  `python3 tools/git-compat-census.py --root .`,
  `tools/git-cli-readiness-status.sh`,
  and `tools/git-compat-command-summary.sh --tsv | rg '^(cat-file|summary)\t'`.

Latest in-progress family follow-up:

- `cat-file` now has `16/21` documented option pairs reviewed complete with
  `16/21` represented documented option pairs, `42/42` classified rows, and
  `0` exact-open written rows.
- The next best helper-free follow-up should move to another dense
  census-backed represented family or zero-code command-promotion cluster.

Current durable census after the zero-code reviewed-complete `blame`
promotion cluster:

- complete command matrices: `102 / 151`
- complete documented command-option pairs: `1480 / 3156`
- represented documented command-option pairs: `1508 / 3156`
- matrix rows: `5488`
- verified rows: `4772`
- invalid-input rows: `701`
- open or partial exact rows: `12`

Latest completed batch:

- `blame` now has the remaining explicit-schema documented options
  `--contents`, `--encoding`, `--first-parent`, `--ignore-rev`,
  `--ignore-revs-file`, `--reverse`, `-S`, and `-h` promoted into the
  reviewed-complete doc-option census list.
- The slice adds no new matrix rows; it lifts `blame` from `28/36` to
  `36/36` reviewed-complete documented option coverage and promotes the full
  `blame` command matrix into the reviewed-complete command census list.
- Focused verification was
  `python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
  `tools/git-cli-readiness-status.sh`,
  `tools/git-compat-command-summary.sh --tsv | rg '^(blame|summary)\t'`,
  and `git diff --check`.

Latest in-progress family follow-up:

- `blame` is now command-complete at `36/36` documented option pairs
  reviewed complete with `36/36` represented documented option pairs,
  `180/180` classified rows, and `0` exact-open written rows.
- The next best helper-free follow-up should move to the next dense
  census-backed represented family or review-promotion cluster, with
  `shortlog` still the leading local expansion candidate.

Current durable census after the zero-code reviewed-complete `rev-parse`
documented-option promotion cluster:

- complete command matrices: `82 / 151`
- complete documented command-option pairs: `1012 / 3175`
- matrix rows: `5332`
- verified rows: `4624`
- invalid-input rows: `679`
- open or partial exact rows: `26`

Latest completed batch:

- `rev-parse` now has twenty-five represented helper-free local documented
  option families promoted into the reviewed-complete doc-option census list,
  bringing the command to `25/53` reviewed-complete documented option pairs.
- The slice adds no new behavior rows; it only makes already closed exact
  stock-Git local discovery, path-format, revision, date-filter,
  symbolic-ref, and replacement-shim evidence durable in the reviewed
  doc-option source list.
- Focused verification was
  `python3 tools/git-compat-census.py --root .`,
  `tools/git-cli-readiness-status.sh`,
  and `tools/git-compat-command-summary.sh --tsv | rg '^(rev-parse|summary)\t'`.

Latest in-progress family follow-up:

- `rev-parse` now has `25/53` documented option pairs reviewed complete with
  `25/53` represented documented option pairs, `80/80` classified rows, and
  `0` exact-open written rows.
- The next best helper-free follow-up should move to another dense
  census-backed represented family or zero-code command-promotion cluster.

Current durable census after the zero-code reviewed-complete `config`
documented-option promotion cluster:

- complete command matrices: `82 / 151`
- complete documented command-option pairs: `987 / 3175`
- matrix rows: `5332`
- verified rows: `4624`
- invalid-input rows: `679`
- open or partial exact rows: `26`

Latest completed batch:

- `config` now has eighteen represented helper-free local documented option
  families promoted into the reviewed-complete doc-option census list,
  bringing the command to `18/30` reviewed-complete documented option pairs.
- The slice adds no new behavior rows; it only makes already closed exact
  stock-Git local get, unset, replace-all, type, show-origin, and fixed-value
  parser evidence durable in the reviewed doc-option source list.
- Focused verification was
  `python3 tools/git-compat-census.py --root .`,
  `tools/git-cli-readiness-status.sh`,
  and `tools/git-compat-command-summary.sh --tsv | rg '^(config|summary)\t'`.

Latest in-progress family follow-up:

- `config` now has `18/30` documented option pairs reviewed complete with
  `18/30` represented documented option pairs, `137/137` classified rows, and
  `0` exact-open written rows.
- The next best helper-free follow-up should move to another dense
  census-backed represented family or zero-code command-promotion cluster.

Current durable census after the zero-code reviewed-complete `notes`
documented-option promotion cluster:

- complete command matrices: `82 / 151`
- complete documented command-option pairs: `969 / 3175`
- matrix rows: `5332`
- verified rows: `4624`
- invalid-input rows: `679`
- open or partial exact rows: `26`

Latest completed batch:

- `notes` now has twenty-one represented helper-free local documented option
  families promoted into the reviewed-complete doc-option census list,
  bringing the command to `21/28` reviewed-complete documented option pairs.
- The slice adds no new behavior rows; it only makes already closed exact
  stock-Git local add, append, edit, copy, merge, prune, remove, and
  ref-selection evidence durable in the reviewed doc-option source list.
- Focused verification was
  `python3 tools/git-compat-census.py --root .`,
  `tools/git-cli-readiness-status.sh`,
  and `tools/git-compat-command-summary.sh --tsv | rg '^(notes|summary)\t'`.

Latest in-progress family follow-up:

- `describe` is now fully command-complete at `14/14` reviewed-complete
  documented option pairs, `14/14` represented documented option pairs,
  `19/19` classified rows, and `0` exact-open written rows.
- The next best helper-free follow-up is the compact parser-alias parity batch
  for `fmt-merge-msg --summary/--no-summary`, `bugreport --no-diagnose`, and
  `http-fetch --index-pack-args`.

Current durable census after the zero-code reviewed-complete `tag`
documented-option promotion cluster:

- complete command matrices: `82 / 151`
- complete documented command-option pairs: `948 / 3175`
- matrix rows: `5332`
- verified rows: `4624`
- invalid-input rows: `679`
- open or partial exact rows: `26`

Latest completed batch:

- `tag` now has twenty-one represented helper-free local documented option
  families promoted into the reviewed-complete doc-option census list,
  bringing the command to `21/38` reviewed-complete documented option pairs.
- The slice adds no new behavior rows; it only makes already closed exact
  stock-Git local create, list, filter, sort, format, verify, and delete
  evidence durable in the reviewed doc-option source list.
- Focused verification was
  `python3 tools/git-compat-census.py --root .`,
  `tools/git-cli-readiness-status.sh`,
  and `tools/git-compat-command-summary.sh --tsv | rg '^(tag|summary)\t'`.

Latest in-progress family follow-up:

- `tag` now has `21/38` documented option pairs reviewed complete with
  `21/38` represented documented option pairs, `40/40` classified rows, and
  `0` exact-open written rows.
- The next best helper-free follow-up should move to another dense
  census-backed represented family or zero-code command-promotion cluster.

Current durable census after the zero-code reviewed-complete `add`
documented-option promotion cluster:

- complete command matrices: `82 / 151`
- complete documented command-option pairs: `927 / 3175`
- matrix rows: `5332`
- verified rows: `4624`
- invalid-input rows: `679`
- open or partial exact rows: `26`

Latest completed batch:

- `add` now has twenty-seven represented helper-free local documented option
  families promoted into the reviewed-complete doc-option census list,
  bringing the command to `27/31` reviewed-complete documented option pairs.
- The slice adds no new behavior rows; it only makes already closed exact
  stock-Git local pathspec, index-mutation, ignore, mode-bit, filter, and
  parser evidence durable in the reviewed doc-option source list.
- Focused verification was
  `python3 tools/git-compat-census.py --root .`,
  `tools/git-cli-readiness-status.sh`,
  and `tools/git-compat-command-summary.sh --tsv | rg '^(add|summary)\t'`.

Latest in-progress family follow-up:

- `add` now has `27/31` documented option pairs reviewed complete with
  `27/31` represented documented option pairs, `126/126` classified rows, and
  `0` exact-open written rows.
- The next best helper-free follow-up should move to another dense
  census-backed represented family or zero-code command-promotion cluster.

Current durable census after the zero-code reviewed-complete `commit`
documented-option promotion cluster:

- complete command matrices: `82 / 151`
- complete documented command-option pairs: `900 / 3175`
- matrix rows: `5332`
- verified rows: `4624`
- invalid-input rows: `679`
- open or partial exact rows: `26`

Latest completed batch:

- `commit` now has thirty-seven represented helper-free local documented
  option families promoted into the reviewed-complete doc-option census list,
  bringing the command to `37/59` reviewed-complete documented option pairs.
- The slice adds no new behavior rows; it only makes already closed exact
  stock-Git local message, editor, hook, cleanup, trailer, pathspec, amend,
  and alias evidence durable in the reviewed doc-option source list.
- Focused verification was
  `python3 tools/git-compat-census.py --root .`,
  `tools/git-cli-readiness-status.sh`,
  and `tools/git-compat-command-summary.sh --tsv | rg '^(commit|summary)\t'`.

Latest in-progress family follow-up:

- `commit` now has `37/59` documented option pairs reviewed complete with
  `37/59` represented documented option pairs, `80/80` classified rows, and
  `0` exact-open written rows.
- The next best helper-free follow-up should move to another dense
  census-backed represented family or zero-code command-promotion cluster.

Current durable census after the zero-code reviewed-complete `fetch`
documented-option promotion cluster:

- complete command matrices: `82 / 151`
- complete documented command-option pairs: `863 / 3175`
- matrix rows: `5332`
- verified rows: `4624`
- invalid-input rows: `679`
- open or partial exact rows: `26`

Latest completed batch:

- `fetch` now has thirty-nine additional represented documented option
  families promoted into the reviewed-complete doc-option census list,
  bringing the command to `40/63` reviewed-complete documented option pairs.
- The slice adds no new behavior rows; it only makes already closed exact
  stock-Git local, file URL, smart HTTP, SSH, git-daemon, prune/tag,
  shallow-history, and replacement-shim evidence durable in the reviewed
  doc-option source list.
- Focused verification was
  `python3 tools/git-compat-census.py --root .`,
  `tools/git-cli-readiness-status.sh`,
  and `tools/git-compat-command-summary.sh --tsv | rg '^(fetch|summary)\t'`.

Latest in-progress family follow-up:

- `fetch` now has `40/63` documented option pairs reviewed complete with
  `40/63` represented documented option pairs, `356/356` classified rows, and
  `0` exact-open written rows.
- The next best follow-up should move to another dense census-backed
  represented family or zero-code command-promotion cluster.

Current durable census after the zero-code reviewed-complete `diff`
documented-option promotion cluster:

- complete command matrices: `82 / 151`
- complete documented command-option pairs: `824 / 3175`
- matrix rows: `5332`
- verified rows: `4624`
- invalid-input rows: `679`
- open or partial exact rows: `26`

Latest completed batch:

- `diff` now has seventy-three represented helper-free documented option
  families promoted into the reviewed-complete doc-option census list.
- The slice adds no new behavior rows; it only makes already closed exact
  stock-Git evidence durable in the reviewed doc-option source list.
- Focused verification was
  `python3 tools/git-compat-census.py --root .`,
  `tools/git-cli-readiness-status.sh`,
  and `tools/git-compat-command-summary.sh --tsv | rg '^(diff|summary)\t'`.

Latest in-progress family follow-up:

- `diff` now has `73/117` documented option pairs reviewed complete with
  `83/117` represented documented option pairs, `250/250` classified rows, and
  `0` exact-open written rows.
- The next best helper-free follow-up should move to another dense
  census-backed represented family or zero-code command-promotion cluster.

Current durable census after the zero-code reviewed-complete `diff-index`
documented-option promotion cluster:

- complete command matrices: `82 / 151`
- complete documented command-option pairs: `751 / 3175`
- matrix rows: `5332`
- verified rows: `4624`
- invalid-input rows: `679`
- open or partial exact rows: `26`

Latest completed batch:

- `diff-index` now has seventy-eight represented helper-free documented option
  families promoted into the reviewed-complete doc-option census list.
- The slice adds no new behavior rows; it only makes already closed exact
  stock-Git evidence durable in the reviewed doc-option source list.
- Focused verification was
  `python3 tools/git-compat-census.py --root .`,
  `tools/git-cli-readiness-status.sh`,
  and `tools/git-compat-command-summary.sh --tsv | rg '^(diff-index|summary)\t'`.

Latest in-progress family follow-up:

- `diff-index` now has `78/112` documented option pairs reviewed complete with
  `78/112` represented documented option pairs, `105/105` classified rows, and
  `0` exact-open written rows.
- The next best helper-free follow-up should move to another dense
  census-backed represented family or zero-code command-promotion cluster.

Current durable census after the zero-code reviewed-complete `diff-tree`
documented-option promotion cluster:

- complete command matrices: `82 / 151`
- complete documented command-option pairs: `673 / 3175`
- matrix rows: `5332`
- verified rows: `4624`
- invalid-input rows: `679`
- open or partial exact rows: `26`

Latest completed batch:

- `diff-tree` now has eighty-five represented helper-free documented option
  families promoted into the reviewed-complete doc-option census list.
- The slice adds no new behavior rows; it only makes already closed exact
  stock-Git evidence durable in the reviewed doc-option source list.
- Focused verification was
  `python3 tools/git-compat-census.py --root .`,
  `tools/git-cli-readiness-status.sh`,
  and `tools/git-compat-command-summary.sh --tsv | rg '^(diff-tree|summary)\t'`.

Latest in-progress family follow-up:

- `diff-tree` now has `85/132` documented option pairs reviewed complete with
  `85/132` represented documented option pairs, `117/117` classified rows, and
  `0` exact-open written rows.
- The next best helper-free follow-up should move to another dense
  census-backed represented family or zero-code command-promotion cluster.

Current durable census after the zero-code reviewed-complete `diff-files`
documented-option promotion cluster:

- complete command matrices: `82 / 151`
- complete documented command-option pairs: `588 / 3175`
- matrix rows: `5332`
- verified rows: `4624`
- invalid-input rows: `679`
- open or partial exact rows: `26`

Latest completed batch:

- `diff-files` now has seventy-nine represented helper-free documented option
  families promoted into the reviewed-complete doc-option census list.
- The slice adds no new behavior rows; it only makes already closed exact
  stock-Git evidence durable in the reviewed doc-option source list.
- Focused verification was
  `python3 tools/git-compat-census.py --root .`,
  `tools/git-cli-readiness-status.sh`,
  and `tools/git-compat-command-summary.sh --tsv | rg '^(diff-files|summary)\t'`.

Latest in-progress family follow-up:

- `diff-files` now has `79/118` documented option pairs reviewed complete with
  `79/118` represented documented option pairs, `98/98` classified rows, and
  `0` exact-open written rows.
- The next best helper-free follow-up should move to another dense
  census-backed represented family or zero-code command-promotion cluster.

Current durable census after the helper-free `ls-files`
`--no-empty-directory` closure plus reviewed-complete command promotion:

- complete command matrices: `82 / 151`
- complete documented command-option pairs: `509 / 3175`
- matrix rows: `5332`
- verified rows: `4624`
- invalid-input rows: `679`
- open or partial exact rows: `26`

Latest completed batch:

- `ls-files` now covers the final documented `--no-empty-directory` family on
  the helper-free local lane with exact stock-Git behavior, including
  last-option-wins ordering against `--empty-directory`.
- The slice adds one exact closed row, completes the final `ls-files`
  documented option pair, and promotes `ls-files` into the reviewed-complete
  command census list.
- Focused verification was
  `cargo test -p zmin-cli --test git_ls_files_compat ls_files_no_empty_directory_matches_stock_git -- --nocapture`,
  `cargo check -p zmin-cli --bin zmin --profile compat`,
  `python3 tools/git-compat-census.py --root .`,
  `tools/git-cli-readiness-status.sh`,
  and `tools/git-compat-command-summary.sh --tsv | rg '^(ls-files|summary)\t'`.

Latest in-progress family follow-up:

- `ls-files` is now reviewed complete at `39/39` documented option pairs with
  `39/39` represented documented option pairs, `159/159` classified rows, and
  `0` exact-open written rows.
- The next best helper-free follow-up should move to another dense
  census-backed family or zero-code command-promotion cluster.

Previous durable census before the final `ls-files`
`--no-empty-directory` closure:

- complete command matrices: `81 / 151`
- complete documented command-option pairs: `508 / 3175`
- matrix rows: `5331`
- verified rows: `4623`
- invalid-input rows: `679`
- open or partial exact rows: `26`

Latest completed batch:

- `ls-files` now has eleven additional documented option pairs promoted into
  the reviewed-complete doc-option census list.
- The closed subgroup adds no new behavior rows; it only makes already closed
  option-family evidence durable in the reviewed doc-option source list.
- Focused verification was
  `python3 tools/git-compat-census.py --root .`,
  `tools/git-cli-readiness-status.sh`,
  and `tools/git-compat-command-summary.sh --tsv | rg '^(ls-files|summary)\t'`.

Previous in-progress family follow-up:

- `ls-files` now has `38/39` documented option pairs reviewed complete with
  `38/39` represented documented option pairs and `0` open rows.
- The next best helper-free follow-up should either close the final
  `--no-empty-directory` seed or move to another dense census-backed family.

Latest zero-code closure:

- `archive` remains promoted in the reviewed-complete command list.
- The current census still has `14/14` documented option pairs complete,
  `24/24` classified rows, and no remaining checklist items for `archive`.

Next helper-free family candidates now remain:

- another zero-code command promotion or dense evidence-import batch from the
  census after the recent `read-tree` and `repack` durable closures
- 2026-06-28 `rev-list` documented tail batch: promoted twenty-seven
  additional `rev-list` documented option pairs through one helper-free local
  stock-Git oracle batch. The slice covered accepted no-op surfaces
  (`--alternate-refs`, `--commit-header`, `--exclude=*`, `--exclude-hidden`,
  `--exclude-promisor-objects`, `--filter-print-omitted`, `--glob`,
  `--ignore-missing`, `--in-commit-order`, `--indexed-objects`,
  `--no-commit-header`, `--no-filter`, `--remove-empty`,
  `--show-linear-break`, `--show-signature`, `--single-worktree`, `--stdin`,
  `--unpacked`, `--use-bitmap-index`), real traversal behavior
  (`--left-only`, `--right-only`, `--no-walk`, `--since-as-filter`), and the
  stock invalid-input lanes for `--merge`, `--missing`, and `--progress`.
  Focused verification was `cargo check -p zmin-cli -p zmin-cli-schema`,
  `cargo test -p zmin-cli --test git_history_query_compat rev_list_documented_tail_batch_matches_stock_git -- --exact --nocapture`,
  `cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-v2-47-schema.json`,
  `python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-v2-47-schema.json`,
  `tools/git-cli-readiness-status.sh`, `tools/git-compat-command-summary.sh --tsv | rg '^(rev-list|summary)\t'`,
  and `git diff --check`. Current census counts are `146/151` complete command
  matrices, `2651/3212` complete documented option pairs, `6954` written
  rows, `6092` verified rows, `837` invalid-input rows, and `561` remaining
  checklist rows. `rev-list` now sits at `108/117` reviewed-complete
  documented option pairs with `160/160` classified rows and `147/160`
  verified exact rows; the remaining `rev-list` tail is the smaller
  special-output cluster around `--bisect*`, `--disk-usage`, `--graph`,
  `--header`, `--objects-edge*`, and any still-unmodeled `--cherry` surface.
