# Git CLI Performance Benchmark

Date: 2026-05-18

Scope: `crates/skron-cli` on macOS against local stock Git `2.50.1`.

## Large fixture medians

| Scenario | Git seconds | Skron seconds | Time ratio | Git RSS MiB | Skron RSS MiB | RSS ratio |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `status_clean` | `0.21` | `0.21` | `1.00x` | `7.7` | `13.5` | `1.75x` |
| `status_dirty` | `0.20` | `0.17` | `0.85x` | `7.7` | `13.5` | `1.75x` |
| `add_all` | `0.22` | `0.26` | `1.18x` | `7.8` | `12.0` | `1.54x` |
| `add_update` | `0.19` | `0.21` | `1.11x` | `7.7` | `10.9` | `1.42x` |
| `commit` | `0.08` | `0.08` | `1.00x` | `7.9` | `13.9` | `1.77x` |
| `clone_local` | `0.31` | `0.45` | `1.45x` | `7.3` | `12.0` | `1.64x` |
| `pull_ff_only_local` | `0.26` | `0.11` | `0.42x` | `8.0` | `13.5` | `1.69x` |
| `reset_hard` | `0.02` | `0.11` | `5.50x` | `7.5` | `12.1` | `1.61x` |
| `switch_branch` | `0.19` | `0.19` | `1.00x` | `8.2` | `13.0` | `1.59x` |
| `grep_tracked` | `0.10` | `0.16` | `1.60x` | `6.2` | `10.9` | `1.76x` |
| `fetch_local` | `0.13` | `0.02` | `0.15x` | `6.7` | `10.4` | `1.55x` |
| `push_local` | `0.23` | `0.03` | `0.13x` | `6.1` | `10.6` | `1.74x` |

## Current reading

- Fast local transport paths are already ahead of stock Git on this fixture.
- `clone_local` and `reset_hard` are still the clearest remaining slow paths.
- Memory usage is consistently above stock Git, even when wall time is competitive.

The detailed iteration log stayed out of the top-level docs on purpose; this file keeps only the public summary.
