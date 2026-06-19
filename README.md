# Zmin

Zmin is a Git-compatible VCS implementation in Rust.

The CLI is meant to work with normal Git repositories: stock Git can continue
from repositories written by Zmin, and Zmin can continue from repositories
written by stock Git.

`zmin` comes from Ukrainian `змін`, meaning "of changes".

## Install Preview

Preview artifacts are published on GitHub Releases, not in official stores.

Current preview: [`v0.0.1-preview.20260619`](https://github.com/dschewchenko/zmin/releases/tag/v0.0.1-preview.20260619)

macOS ARM64:

```bash
curl -L -o zmin-aarch64-apple-darwin.tar.gz \
  https://github.com/dschewchenko/zmin/releases/download/v0.0.1-preview.20260619/zmin-aarch64-apple-darwin.tar.gz
tar -xzf zmin-aarch64-apple-darwin.tar.gz
install -m 0755 zmin ~/.local/bin/zmin
zmin --version
```

Windows ARM64, PowerShell:

```powershell
Invoke-WebRequest `
  -Uri "https://github.com/dschewchenko/zmin/releases/download/v0.0.1-preview.20260619/zmin-aarch64-pc-windows-gnullvm.zip" `
  -OutFile "zmin-aarch64-pc-windows-gnullvm.zip"
Expand-Archive .\zmin-aarch64-pc-windows-gnullvm.zip -DestinationPath .\zmin
.\zmin\zmin.exe --version
```

Keep `libunwind.dll` next to `zmin.exe`; it is included in the ZIP.

Other platforms:

```bash
cargo build -p zmin-cli --release --bin zmin
./target/release/zmin --version
```

Check downloaded files with `SHA256SUMS` from the same release.

## What Is Included

- `zmin-git-core`: objects, refs, index, pack, checkout, diff, merge-file,
  commit, tag, and reachability primitives.
- `zmin-cli`: Git-style CLI built on top of the core primitives.
- `zmin-primitives`: runtime contracts, transport traits, config helpers, ids,
  and error model.
- `zmin-git-remote-http`: standalone HTTP remote helper crate.
- `zmin-core`: umbrella crate that re-exports the Git-facing crates.

## Compatibility Status

Temporary command coverage table. This is command-list coverage, not a claim
that every option and every edge case is complete.

Baseline: upstream Git `v2.47.1` command list.

| Git command-list category | Covered | Missing | Coverage |
| --- | ---: | ---: | ---: |
| `mainporcelain` | `42/42` | `0` | `100%` |
| `ancillaryinterrogators` | `16/16` | `0` | `100%` |
| `ancillarymanipulators` | `12/12` | `0` | `100%` |
| `foreignscminterface` | `10/10` | `0` | `100%` |
| `plumbinginterrogators` | `21/21` | `0` | `100%` |
| `plumbingmanipulators` | `20/20` | `0` | `100%` |
| `purehelpers` | `18/18` | `0` | `100%` |
| `synchelpers` | `6/6` | `0` | `100%` |
| `synchingrepositories` | `5/5` | `0` | `100%` |
| `complete` | `1/1` | `0` | `100%` |
| Total baseline | `150/150` | `0` | `100%` |
| Raw upstream list, including `help` | `151/151` | `0` | `100%` |

Zmin also has `17` commands outside that upstream baseline.

Current compatibility gates:

| Area | macOS | Windows | Status |
| --- | ---: | ---: | --- |
| Git command-list baseline | `150/150` | `150/150` | covered |
| Raw upstream list including `help` | `151/151` | `151/151` | covered |
| Focused upstream compatibility surface | `16/16` | `16/16` | verified |
| Repositories written by stock Git | yes | yes | verified in tests |
| Repositories written by Zmin | yes | yes | verified in tests |
| Reftable | no | no | explicit unsupported mode |
| Performance on larger Windows clone fixtures | open | open | tracked |

Proof lives in:

- `crates/zmin-cli/tests/`
- `docs/git/parity_evidence_matrix.md`
- `docs/cli/compatibility_acceptance.md`
- `docs/cli/performance_benchmark_2026-05-18.md`
- `tools/git-command-gap.sh`
- `tools/git-cli-readiness-status.sh`

## Performance Snapshot

Temporary median table from current 3-repeat full gates.

`x1.00` means equal to Zmin. `x2.00` means Zmin is 2 times faster. Values below
`x1.00` mean Zmin is slower.

macOS source: `/tmp/zmin-macos-full-gate-refwrite-20260619T-next/comparison.csv`.
Windows source: `C:\Users\skron\zmin-bench-20260619T014310Z-74670-out\comparison.csv`.

| Platform | Operation | Zmin median | Git median | Zmin vs Git | Gitoxide median | Zmin vs Gitoxide |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| macOS | `add` | `1.073s` | `1.491s` | `x1.39` | n/a | n/a |
| macOS | `commit` | `0.041s` | `0.071s` | `x1.71` | n/a | n/a |
| macOS | `status` | `0.023s` | `0.029s` | `x1.28` | `0.028s` | `x1.25` |
| macOS | `clone` | `0.067s` | `0.141s` | `x2.10` | `0.318s` | `x4.73` |
| macOS | `clone-instant` | `0.076s` | `0.131s` | `x1.73` | n/a | n/a |
| macOS | `fetch-incremental` | `0.034s` | `0.073s` | `x2.17` | `0.218s` | `x6.45` |
| macOS | `fetch-batch` | `0.044s` | `0.092s` | `x2.11` | `0.233s` | `x5.35` |
| macOS | `push-batch` | `0.242s` | `0.323s` | `x1.34` | n/a | n/a |
| macOS | `log` | `0.021s` | `0.027s` | `x1.28` | `0.023s` | `x1.07` |
| macOS | `merge-base` | `0.018s` | `0.027s` | `x1.46` | `0.023s` | `x1.27` |
| Windows | `add` | `1.168s` | `2.045s` | `x1.75` | n/a | n/a |
| Windows | `commit` | `0.164s` | `6.845s` | `x41.79` | n/a | n/a |
| Windows | `status` | `0.036s` | `0.094s` | `x2.60` | `0.175s` | `x4.87` |
| Windows | `clone` | `0.660s` | `0.410s` | `x0.62` | `0.669s` | `x1.01` |
| Windows | `clone-instant` | `0.523s` | `0.505s` | `x0.97` | n/a | n/a |
| Windows | `fetch-incremental` | `0.228s` | `0.658s` | `x2.89` | `0.454s` | `x2.00` |
| Windows | `fetch-batch` | `0.113s` | `0.513s` | `x4.52` | `0.406s` | `x3.58` |
| Windows | `push-batch` | `0.054s` | `1.329s` | `x24.57` | n/a | n/a |
| Windows | `log` | `0.036s` | `0.135s` | `x3.76` | `0.026s` | `x0.73` |
| Windows | `merge-base` | `0.022s` | `0.066s` | `x3.04` | `0.027s` | `x1.26` |

Open performance gaps:

- Windows default `clone` is slower than Git in the full gate.
- Windows `clone-instant` is near parity, but still below `x1.00` by median.
- Windows `log` is faster than Git but slower than Gitoxide.
- Larger Windows clone fixtures still need checkout materialization work.

## Build

```bash
cargo build
cargo build -p zmin-cli
cargo build -p zmin-git-remote-http
```

## Verify

Use focused checks first:

```bash
cargo check -p zmin-cli --bin zmin --profile compat
cargo test -p zmin-git-core
cargo test -p zmin-cli --test git_clone_compat --test git_commit_compat --test git_refs_compat --test git_transport_http_compat
tools/git-command-gap.sh
```

Full local gate:

```bash
cargo fmt --all --check
cargo check --all-targets
cargo clippy --all-targets --all-features
cargo test --all
tools/git-cli-readiness-status.sh --require-complete
```

## Download And Install Analytics

GitHub Releases expose per-asset download counts through the GitHub API. Example:

```bash
gh api repos/dschewchenko/zmin/releases/tags/v0.0.1-preview.20260619 \
  --jq '.assets[] | {name, download_count}'
```

There is no install telemetry in the binaries. Direct ZIP/TAR installs can show
downloads, not completed installs.

For install counts later, use one of these:

- a small install script served through a controlled redirect endpoint;
- package-manager analytics after publishing to official stores;
- explicit opt-in telemetry in a future `zmin doctor` or update-check command.

## Trademark Notice

Zmin is not affiliated with the Git Project or Software Freedom Conservancy.

Git is a registered trademark of Software Freedom Conservancy, Inc.

## License

This repository is currently shared publicly for evaluation and reference only.

No permission is granted to use, copy, modify, distribute, sublicense, or create
derivative works from this code without prior written consent.
