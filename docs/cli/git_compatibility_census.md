# Git Compatibility Census

## Current authority

The machine-readable current contract is
`tools/git-upstream-compat-contract.tsv`; the canonical explanation is
`docs/git/upstream_compatibility_baseline.md`. It freezes Git `v2.55.0` at
commit `e9019fcafe0040228b8631c30f97ae1adb61bcdc` and defines
`all-nondeprecated` as 1045 of 1046 top-level shell tests. The sole whole-file
exclusion is `t5323-pack-redundant.sh`. The current external groups
git-svn, git-cvsserver, gitweb, cvsimport and git-p4 remain included. Mixed
deprecated assertions inside retained files remain in scope.

The separate machine contract `tools/zmin-extensions-contract.tsv` contains
40 stable primary rows and 7 relationship-neutral rows: 47 tracked contract
rows in total. These are not 47 Git APIs or a claim of arbitrary Git LFS
ecosystem parity. The relationship projection is
`docs/cli/census/zmin_api_relationships.tsv`; neither relationships nor
Zmin-only rows inflate the Git denominator. The Markdown extension inventory
is a human projection only. The one `command.lfs` primary row covers its
defined Git LFS v3.7.1-compatible slice; its explicit mTLS/client-identity
transport exclusion fails before network access and is not a Git-denominator
exclusion.

`100%` is a target definition for the entire frozen current/nondeprecated
surface. A local census, command seed, selected oracle evidence, or `26/26`
historical test run does not claim that target is achieved. Historical
v2.47.1 tables and matrices remain retained evidence below and are not current
authority.

## Source layers

`tools/git-compat-census.py` consumes the current contracts directly, the
pinned Git v2.55.0 `command-list.txt` and `Documentation`, the historical
v2.47 schema JSON and matrix rows, the existing oracle inventory as evidence,
and source hard-fail scans. `tools/git-existing-oracle-inventory.py` is an
evidence indexer over historical matrices/classification docs; it does not
classify the current contract.

## Refresh

Use the validated offline source and an explicit historical schema input. This
is the same fail-closed recipe used by the option inventory: no download,
fallback or implicit v2.47 source is allowed.

```bash
CACHE_ROOT="${ZMIN_UPSTREAM_GIT_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/zmin/git-upstream}"
SOURCE_ROOT="$CACHE_ROOT/git-v2.55.0"
COMMAND_LIST="$SOURCE_ROOT/command-list.txt"
HISTORICAL_ZMIN_SCHEMA_JSON=/path/to/historical-v2-47-schema.json
ARCHIVE_SHA256="$(awk -F '\t' '$1 == "upstream_archive_sha256" { print $2; exit }' \
  tools/git-upstream-compat-contract.tsv)"
test -d "$SOURCE_ROOT/Documentation"
test -f "$COMMAND_LIST" -a -f "$HISTORICAL_ZMIN_SCHEMA_JSON"
test -n "$ARCHIVE_SHA256"
ZMIN_UPSTREAM_GIT_CACHE="$CACHE_ROOT" \
ZMIN_GIT_BASELINE=v2.55.0 \
ZMIN_GIT_DOC_CACHE="$SOURCE_ROOT" \
ZMIN_GIT_COMMAND_LIST="$COMMAND_LIST" \
ZMIN_GIT_SOURCE_ARCHIVE_SHA256="$ARCHIVE_SHA256" \
python3 tools/git-compat-census.py --root . \
  --historical-zmin-schema-json "$HISTORICAL_ZMIN_SCHEMA_JSON" \
  --out-dir docs/cli/census
```

The generator must fail closed for missing or mismatched v2.55.0 source
identity. It must not invoke the contract gate, audit, manifest, network, or a
v2.47 source fallback.

The checked-in `docs/cli/existing_oracle_test_inventory.tsv` remains a frozen
historical 1466/1248/218 evidence snapshot (found/represented or classified/
missing or unclassified). `docs/cli/census/oracle_evidence_layer.tsv` is the
corresponding 1466-row historical projection, not current v2.55.0 authority.

## Generated projection

See `docs/cli/census/README.md` for the generated file map. The current
contract-derived summary includes the Git tag/commit, the 1045 denominator,
the current command seed, 40 stable primary extension rows, and 7 relationship-
neutral rows (47 tracked contract rows). Generated behavior counts remain
evidence/checklist metrics, not a parity percentage.

## Historical v2.47.1 evidence retained

The older v2.47.1 census tables, selected oracle rows and historical counts are
retained for traceability. They are not the current denominator, do not
reclassify the seven current Git names `backfill`, `diff-pairs`, `format-rev`,
`history`, `last-modified`, `repo`, and `url-parse`, and do not establish a
drop-in claim.
