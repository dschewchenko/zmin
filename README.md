# Zmin

Zmin is an experimental Git-compatible version control tool for normal `.git`
repositories.

It keeps the repository format that Git already uses: a repository created or
updated with Zmin can still be opened with `git`, and a repository created with
Git can be opened with `zmin`.

Zmin is not just Git rewritten in Rust. The preview focuses on faster local
workflows, explicit clone modes, and simpler everyday commands while staying
compatible with existing Git hosting.

Zmin is pre-1.0. Preview builds are experimental and are not yet published
through Homebrew, apt, winget, Chocolatey, crates.io, or other package channels.

## Install

Current preview: `v0.0.1-preview.20260619`

Current preview downloads are available for macOS ARM64 and Windows ARM64.
Other platforms can build from source for now.

Check downloaded files with
[`SHA256SUMS`](https://github.com/dschewchenko/zmin/raw/artifacts-v0.0.1-preview.20260619/SHA256SUMS).

### macOS

ARM64:

```bash
curl -L -o zmin-aarch64-apple-darwin.tar.gz \
  https://github.com/dschewchenko/zmin/raw/artifacts-v0.0.1-preview.20260619/zmin-aarch64-apple-darwin.tar.gz
tar -xzf zmin-aarch64-apple-darwin.tar.gz
mkdir -p ~/.local/bin
install -m 0755 zmin ~/.local/bin/zmin
zmin --version
```

### Windows

ARM64, PowerShell:

```powershell
Invoke-WebRequest `
  -Uri "https://github.com/dschewchenko/zmin/raw/artifacts-v0.0.1-preview.20260619/zmin-aarch64-pc-windows-gnullvm.zip" `
  -OutFile "zmin-aarch64-pc-windows-gnullvm.zip"
Expand-Archive .\zmin-aarch64-pc-windows-gnullvm.zip -DestinationPath .\zmin
.\zmin\zmin.exe --version
```

Keep `libunwind.dll` next to `zmin.exe`; it is included in the ZIP.

### Linux And Other Platforms

Build from source for platforms that do not have a preview download yet.
Requires the Rust toolchain:

```bash
cargo build -p zmin-cli --release --bin zmin
./target/release/zmin --version
```

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

## Git Compatibility

Zmin works with regular Git repositories and existing Git remotes.

| Workflow | Preview support |
| --- | --- |
| Open an existing `.git` repository | Supported |
| Create commits readable by Git | Supported |
| Clone, fetch, pull, and push normal Git remotes | Supported |
| Status, add, commit, branch, tag, log, diff, and merge-base workflows | Supported |
| Git LFS | Not included yet |
| Reftable repositories | Not included yet |
| Official package-manager installs | Not published yet |

This preview aims to be useful for everyday Git-compatible work, but it is still
experimental. Keep a current backup before using it on important repositories.

## Zmin Commands

Zmin also includes higher-level commands for common workflows:

| Command | Purpose |
| --- | --- |
| `zmin save` | Record local work with fewer steps |
| `zmin changes` | Review what changed |
| `zmin publish` | Share local work with the remote |
| `zmin update` | Bring the repository up to date |
| `zmin undo` | Undo the last local action when possible |
| `zmin timeline` | Browse recent repository history |
| `zmin recover` | Inspect recoverable local state |

Run `zmin help` for the full command list.

## Trademark Notice

Zmin is not affiliated with the Git Project or Software Freedom Conservancy.

Git is a registered trademark of Software Freedom Conservancy, Inc.

## License

This repository is currently shared publicly for evaluation and reference only.

No permission is granted to use, copy, modify, distribute, sublicense, or create
derivative works from this code without prior written consent.
