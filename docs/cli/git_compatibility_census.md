# Git Compatibility Census

This is the census-first checkpoint for Git `2.47.1` compatibility work.

Do not add more behavior rows from `docs/cli/existing_oracle_test_inventory.tsv`
until this census has been refreshed and the next slice is selected from the
generated checklist. The oracle inventory is now an evidence layer, not the
primary backlog or source of truth.

## Source Layers

`tools/git-compat-census.py` builds the census from these sources:

- upstream Git `v2.47.1` `command-list.txt`
- upstream Git `Documentation/git-*.txt` option spellings, including includes
- Zmin CLI schema from `zmin compat --profile v2-47 --format json`
- existing `docs/cli/matrices/*_v2_47.tsv` behavior rows
- existing stock-oracle test inventory as an evidence layer
- `docs/cli/zmin_extensions_inventory.md`
- `docs/cli/oracle_test_deferrals.md`
- source hard-fail scan for `unsupported`, `not supported yet` and
  `not implemented yet`

## Refresh

Normal refresh:

```bash
python3 tools/git-compat-census.py --root .
```

If the worktree has unrelated Rust WIP, capture the Zmin schema from a clean
worktree and pass it explicitly:

```bash
cargo run -q -p zmin-cli --bin zmin -- compat --profile v2-47 --format json > /tmp/zmin-compat-v2-47.json
python3 tools/git-compat-census.py --root . --zmin-schema-json /tmp/zmin-compat-v2-47.json
```

## Generated Files

| File | Purpose |
| --- | --- |
| `docs/cli/census/summary.tsv` | top-level counts for source layers and buckets |
| `docs/cli/census/all_items.tsv` | union of generated census rows |
| `docs/cli/census/verified_behavior.tsv` | exact verified behavior rows safe to skip unless code or evidence changes |
| `docs/cli/census/invalid_input_parity.tsv` | exact invalid-input rows where stock Git and Zmin rejections match |
| `docs/cli/census/implemented_but_unverified.tsv` | Zmin schema surfaces with parser/handler presence but no exact stock-Git row evidence |
| `docs/cli/census/remaining_to_fix_or_verify.tsv` | command/doc-option expansion, exact open rows and unclassified hard-fail guards |
| `docs/cli/census/zmin_extension_or_deferred.tsv` | Zmin-only additions and deferred/non-Git-2.47.1 evidence |
| `docs/cli/census/oracle_evidence_layer.tsv` | existing focused stock-oracle tests, evidence only |
| `docs/cli/census/hard_fail_scan.tsv` | source guard scan with documented/unclassified status |

## Current Snapshot

Generated on 2026-06-22 from committed branch state `4c53dd7`.

| Metric | Count | Meaning |
| --- | ---: | --- |
| Git `2.47.1` commands | `151` | upstream command-list seed |
| Git doc option seed rows | `4632` | documented option spelling seed, not final denominator |
| Zmin schema baseline commands | `151` | command entry points present in schema |
| Zmin schema additional commands | `52` | outside Git `2.47.1` baseline |
| Existing matrix rows | `2665` | evidence layer, not full denominator |
| Verified exact rows | `2287` | closed behavior variants safe to skip exactly |
| Invalid-input parity rows | `377` | stock-compatible rejection variants |
| Exact open or partial matrix rows | `1` | row exists but is not closed |
| Implemented but unverified rows | `960` | schema args and additional schema paths without exact matrix evidence |
| Remaining checklist rows | `4693` | doc-option expansion, exact opens and unclassified guards |
| Zmin-only or deferred rows | `33` | extension and deferral classifications outside the denominator |
| Oracle evidence layer rows | `961` | existing tests, not the primary backlog |
| Source hard-fail rows | `90` | raw guard hits in source scan |
| Unclassified hard-fail rows | `11` | source guard hits not matched to classification docs |

## Bucket Rules

- `verified`: exact command/option/value/combination/state/transport/platform
  rows with stock-Git evidence. These may be skipped in future work unless the
  implementation or evidence changes.
- `implemented but unverified`: parser or handler surface appears in the Zmin
  schema, but no exact stock-Git oracle row closes it.
- `not implemented / broken / open`: exact open/partial rows, documented Git
  option seeds still needing expansion, command matrices not started, or source
  guards not yet classified.
- `invalid-input parity`: stock Git rejects the input and Zmin rejection is
  verified for the exact row.
- `Zmin-only extension or deferred/non-Git-2.47.1 scope`: outside the Git
  compatibility denominator unless explicitly reclassified.

## Next Work

Future fix/verify slices should start from
`docs/cli/census/remaining_to_fix_or_verify.tsv`. Select one exact row or one
small coherent expansion group, then add stock-Git evidence and matrix rows.
Only after that selection should `docs/cli/existing_oracle_test_inventory.tsv`
be consulted to find whether an existing focused oracle test can serve as
evidence for the chosen row shape.
