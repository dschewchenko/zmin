# Zmin

Zmin is a pre-1.0 Git-compatible command line tool for existing `.git`
repositories.

It does not replace Git storage. You can use `zmin` and `git` in the same
checkout.

The name comes from `зміни`, Ukrainian for "changes". For this project that
means the practical work around changes: see what changed, choose what belongs
in a commit, share it and get back to a known state when needed.

The goal is Git compatibility with less waiting in the commands people run all
day: `status`, `add`, `commit`, `clone`, `fetch` and `push`.

It is not published through Homebrew, apt, winget, Chocolatey, crates.io or
official stores yet.

## Install

Current preview: [`v0.0.1-preview.20260619T134737Z`](https://github.com/dschewchenko/zmin/releases/tag/v0.0.1-preview.20260619T134737Z)

Download a preview archive for your platform or build from source.

### macOS

Apple Silicon:

```bash
mkdir -p ~/.local/bin
curl -L https://github.com/dschewchenko/zmin/releases/download/v0.0.1-preview.20260619T134737Z/zmin-aarch64-apple-darwin.tar.gz | tar -xz
install -m 0755 zmin ~/.local/bin/zmin
zmin --version
```

Intel:

```bash
mkdir -p ~/.local/bin
curl -L https://github.com/dschewchenko/zmin/releases/download/v0.0.1-preview.20260619T134737Z/zmin-x86_64-apple-darwin.tar.gz | tar -xz
install -m 0755 zmin ~/.local/bin/zmin
zmin --version
```

Build from source:

```bash
mkdir -p ~/.local/bin
cargo build -p zmin-cli --release --bin zmin
install -m 0755 target/release/zmin ~/.local/bin/zmin
zmin --version
```

### Linux

x86_64:

```bash
mkdir -p ~/.local/bin
curl -L https://github.com/dschewchenko/zmin/releases/download/v0.0.1-preview.20260619T134737Z/zmin-x86_64-unknown-linux-gnu.tar.gz | tar -xz
install -m 0755 zmin ~/.local/bin/zmin
zmin --version
```

aarch64:

```bash
mkdir -p ~/.local/bin
curl -L https://github.com/dschewchenko/zmin/releases/download/v0.0.1-preview.20260619T134737Z/zmin-aarch64-unknown-linux-gnu.tar.gz | tar -xz
install -m 0755 zmin ~/.local/bin/zmin
zmin --version
```

Build from source:

```bash
mkdir -p ~/.local/bin
cargo build -p zmin-cli --release --bin zmin
install -m 0755 target/release/zmin ~/.local/bin/zmin
zmin --version
```

### Windows

PowerShell, x86_64:

```powershell
$Version = "v0.0.1-preview.20260619T134737Z"
Invoke-WebRequest "https://github.com/dschewchenko/zmin/releases/download/$Version/zmin-x86_64-pc-windows-msvc.zip" -OutFile zmin.zip
Expand-Archive -Force zmin.zip .
.\zmin.exe --version
```

Use `zmin-aarch64-pc-windows-msvc.zip` on Windows ARM.

Build from source:

```powershell
cargo build -p zmin-cli --release --bin zmin
.\target\release\zmin.exe --version
```

### Preview Archives

Current binary preview archives:

<!-- zmin-release-assets:start -->
- [`zmin-x86_64-unknown-linux-gnu.tar.gz`](https://github.com/dschewchenko/zmin/releases/download/v0.0.1-preview.20260619T134737Z/zmin-x86_64-unknown-linux-gnu.tar.gz)
- [`zmin-aarch64-unknown-linux-gnu.tar.gz`](https://github.com/dschewchenko/zmin/releases/download/v0.0.1-preview.20260619T134737Z/zmin-aarch64-unknown-linux-gnu.tar.gz)
- [`zmin-x86_64-apple-darwin.tar.gz`](https://github.com/dschewchenko/zmin/releases/download/v0.0.1-preview.20260619T134737Z/zmin-x86_64-apple-darwin.tar.gz)
- [`zmin-aarch64-apple-darwin.tar.gz`](https://github.com/dschewchenko/zmin/releases/download/v0.0.1-preview.20260619T134737Z/zmin-aarch64-apple-darwin.tar.gz)
- [`zmin-x86_64-pc-windows-msvc.zip`](https://github.com/dschewchenko/zmin/releases/download/v0.0.1-preview.20260619T134737Z/zmin-x86_64-pc-windows-msvc.zip)
- [`zmin-aarch64-pc-windows-msvc.zip`](https://github.com/dschewchenko/zmin/releases/download/v0.0.1-preview.20260619T134737Z/zmin-aarch64-pc-windows-msvc.zip)
- [`SHA256SUMS`](https://github.com/dschewchenko/zmin/releases/download/v0.0.1-preview.20260619T134737Z/SHA256SUMS)
<!-- zmin-release-assets:end -->

## Start

Clone a repository:

```bash
zmin clone https://github.com/example/project.git
cd project
```

Work in an existing repository:

```bash
zmin status
zmin add .
zmin commit -m "Update project"
zmin push
```

You can use `git` and `zmin` in the same repository.

Fast clone checks out the selected `HEAD` first. Use it when you want a working
tree quickly and can hydrate the rest of the remote state later:

```bash
zmin clone --instant https://github.com/example/project.git
zmin clone --instant --background-fetch https://github.com/example/project.git
zmin clone --instant --demand-hydrate https://github.com/example/project.git
```

## Command Reference

Use commands as `zmin <command>`.

| Group | Covered | Support |
| --- | ---: | ---: |
| Setup and Config | `8/8` | `100%` |
| Getting and Creating Projects | `2/2` | `100%` |
| Basic Snapshotting | `10/10` | `100%` |
| Branching and Merging | `12/12` | `100%` |
| Sharing and Updating Projects | `6/6` | `100%` |
| Inspection and Comparison | `12/12` | `100%` |
| Patching | `6/6` | `100%` |
| Debugging | `3/3` | `100%` |
| Email | `8/8` | `100%` |
| External Systems | `9/9` | `100%` |
| Administration | `20/20` | `100%` |
| Server Admin | `11/11` | `100%` |
| Plumbing Commands | `54/54` | `100%` |
| Zmin Workflow Commands | `14/14` | `100%` |
| Unique command surface | `168/168` | `100%` |

## Preview Limits

Zmin works with regular Git repositories and existing Git remotes. This preview
does not include Git LFS, reftable repositories or official package-manager
installs.

Some edge-case options and environments still need more coverage. Keep a
current backup before using preview builds on important repositories.

## Speed Snapshot

Median seconds on preview fixtures. Values in parentheses show how fast Zmin is
against that tool by median; values below `x1.00` mean Zmin is slower.

| Platform | Operation | Zmin | Git | Gitoxide |
| --- | --- | ---: | ---: | ---: |
| macOS | `init` | `0.01s` | `0.05s` (`x5.00`) | n/a |
| macOS | `status` | `0.01s` | `0.05s` (`x5.00`) | `0.02s` (`x2.00`) |
| macOS | `log` | `0.01s` | `0.05s` (`x5.00`) | `0.01s` (`x1.00`) |
| macOS | `rev-list` | `0.03s` | `0.05s` (`x1.67`) | n/a |
| macOS | `merge-base` | `0.01s` | `0.05s` (`x5.00`) | `0.01s` (`x1.00`) |
| macOS | `pack-objects` | `0.01s` | `0.05s` (`x5.00`) | n/a |
| macOS | `index-pack` | `0.08s` | `0.11s` (`x1.38`) | n/a |
| macOS | `add` | `1.02s` | `1.19s` (`x1.17`) | n/a |
| macOS | `commit` | `0.05s` | `0.14s` (`x2.80`) | n/a |
| macOS | `add-dirty` | `0.12s` | `0.15s` (`x1.25`) | n/a |
| macOS | `commit-dirty` | `0.05s` | `0.15s` (`x3.00`) | n/a |
| macOS | `clone` | `0.10s` | `0.23s` (`x2.30`) | `0.27s` (`x2.70`) |
| macOS | `push-noop` | `0.01s` | `0.15s` (`x15.00`) | n/a |
| macOS | `push-incremental` | `0.08s` | `0.44s` (`x5.50`) | n/a |
| macOS | `push-batch` | `0.13s` | `0.67s` (`x5.15`) | n/a |
| macOS | `fetch-noop` | `0.02s` | `0.26s` (`x13.00`) | `0.11s` (`x5.50`) |
| macOS | `fetch-incremental` | `0.08s` | `0.35s` (`x4.38`) | `0.21s` (`x2.63`) |
| macOS | `fetch-batch` | `0.09s` | `0.36s` (`x4.00`) | `0.21s` (`x2.33`) |
| macOS | `clone-large` | `0.158s` | `0.288s` (`x1.83`) | `0.503s` (`x3.19`) |
| Windows | `clone` | `1.136s` | `1.413s` (`x1.24`) | `1.152s` (`x1.01`) |
| Windows | `clone-large` | `2.431s` | `2.135s` (`x0.88`) | `2.158s` (`x0.89`) |

## Package Size Snapshot

Compressed download size. Git packages include extra tools and runtime files;
this table does not compare installed size.

| Platform | Zmin archive | Git package | Git package size | Zmin archive |
| --- | ---: | --- | ---: | ---: |
| macOS Intel | `3.74 MB` | Homebrew `git 2.54.0` Sonoma bottle | `23.52 MB` | `x6.30` smaller |
| macOS Apple Silicon | `3.33 MB` | Homebrew package varies by macOS release | n/a | n/a |
| Linux x86_64 | `3.78 MB` | Git `2.54.0` source `.tar.xz` | `8 MB` | `x2.12` smaller |
| Linux aarch64 | `3.50 MB` | Git `2.54.0` source `.tar.xz` | `8 MB` | `x2.29` smaller |
| Windows x86_64 | `3.84 MB` | Git for Windows `2.54.0` setup | `65.2 MB` | `x17.00` smaller |
| Windows x86_64 | `3.84 MB` | MinGit `2.54.0` zip | `40.0 MB` | `x10.43` smaller |
| Windows ARM64 | `3.57 MB` | Git for Windows `2.54.0` setup | `63.4 MB` | `x17.74` smaller |
| Windows ARM64 | `3.57 MB` | MinGit `2.54.0` zip | `39.8 MB` | `x11.14` smaller |

## Trademark Notice

Zmin is not affiliated with the Git Project or Software Freedom Conservancy.

Git is a registered trademark of Software Freedom Conservancy, Inc.

## License

This repository is currently shared publicly for evaluation and reference only.

No permission is granted to use, copy, modify, distribute, sublicense or create
derivative works from this code without prior written consent.
