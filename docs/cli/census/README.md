# Git Compatibility Census Outputs

The committed contracts are the only authorities:

- `tools/git-upstream-compat-contract.tsv` defines the frozen Git v2.55.0
  current/nondeprecated scope, including 1045 of 1046 top-level shell tests
  and the sole whole-file `t5323-pack-redundant` exclusion.
- `tools/zmin-extensions-contract.tsv` defines 40 stable primary Zmin rows and
  7 relationship-neutral rows: 47 tracked contract rows, not 47 Git APIs.
  Relationships never enter the Git or Zmin extension denominator.

Generate the projections with the validated offline v2.55.0 source. The cache
root is portable and the archive identity is read from the committed contract;
the source and command list must already exist locally.

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

The schema JSON is historical v2.47 evidence input only. The generator reads
the current command list and Documentation from the pinned Git source and
passes current source identity to the option and command seed helpers. It must
not use a v2.47 source cache, network fallback, Markdown authority, or the
contract gate/audit/manifest scripts.

Generated files in this directory are projections, not sources of truth:

| File | Meaning |
| --- | --- |
| `summary.tsv` | Current contract/source counts and generated census metrics |
| `command_progress.tsv` | Current command seed and behavior-row progress |
| `all_items.tsv` | Union of generated evidence/checklist rows |
| `verified_behavior.tsv` | Exact verified behavior evidence rows |
| `invalid_input_parity.tsv` | Exact matching rejection rows |
| `exact_open_oracle_gaps.tsv` | Open rows blocked by a local stock oracle |
| `implemented_but_unverified.tsv` | Schema surfaces without exact parity evidence |
| `remaining_to_fix_or_verify.tsv` | Remaining expansion, open and guard rows |
| `zmin_extension_or_deferred.tsv` | Primary Zmin extension projection only (legacy filename) |
| `zmin_api_relationships.tsv` | Exactly seven neutral relationship rows |
| `oracle_evidence_layer.tsv` | Existing stock-oracle evidence, not primary backlog |
| `hard_fail_scan.tsv` | Source guard classifications |

The durable reviewed input lists `reviewed_complete_command_matrices.tsv`,
`reviewed_complete_doc_option_pairs.tsv`, and `deferred_doc_option_pairs.tsv`
are inputs to the generator, not generated outputs. The separate
`tools/git-existing-oracle-inventory.py` indexes historical matrix and
classification evidence; it does not define current Git scope.

The checked-in `docs/cli/existing_oracle_test_inventory.tsv` is a frozen
historical evidence snapshot: 1466 stock-oracle functions found, 1248
represented or classified, and 218 missing or unclassified. The generated
`docs/cli/census/oracle_evidence_layer.tsv` is its 1466-row historical
projection. Neither file is the current v2.55.0 denominator or backlog
authority, and the inventory script is a historical evidence indexer only.

No generated census count proves 100% drop-in compatibility. The 100% target
is the entire frozen current/nondeprecated Git v2.55.0 surface and requires
upstream, stock-Git differential, and applicable platform evidence.
