# Zmin

Zmin is an experimental Git-compatible version control tool for normal `.git`
repositories.

It keeps the repository format that Git already uses: a repository created or
updated with Zmin can still be opened with `git`, and a repository created with
Git can be opened with `zmin`.

The name comes from the Ukrainian word `змін`.

Zmin is not just Git rewritten in Rust. The preview focuses on faster local
workflows, explicit clone modes, and simpler everyday commands while staying
compatible with existing Git hosting.

Zmin is pre-1.0. Preview builds are experimental and are not yet published
through Homebrew, apt, winget, Chocolatey, crates.io, or other package channels.

## Install

Current preview: `v0.0.1-preview.20260619`

Current preview downloads are available for macOS ARM64 and Windows ARM64.
Other platforms can build from source for now.

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

## Verify Download

Optional checksums:
[`SHA256SUMS`](https://github.com/dschewchenko/zmin/raw/artifacts-v0.0.1-preview.20260619/SHA256SUMS)

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

## Command Reference

Use these as `zmin <command>`. Git-compatible commands are listed separately
from Zmin-specific workflow commands.

### Setup And Config

`config` · `help` · `version` · `bugreport` · `diagnose` · `credential` ·
`credential-cache` · `credential-store`

### Getting And Creating Projects

`init` · `clone`

### Basic Snapshotting

`add` · `stage` · `status` · `diff` · `commit` · `notes` · `restore` ·
`reset` · `rm` · `mv`

### Branching And Merging

`branch` · `checkout` · `switch` · `merge` · `mergetool` · `merge-tree` ·
`merge-file` · `cherry` · `stash` · `tag` · `worktree` · `rerere`

### Sharing And Updating Projects

`fetch` · `pull` · `push` · `remote` · `submodule` · `ls-remote`

### Inspection And Comparison

`show` · `log` · `diff` · `difftool` · `range-diff` · `shortlog` ·
`describe` · `whatchanged` · `grep` · `blame` · `annotate` · `bisect`

### Patching

`apply` · `am` · `cherry-pick` · `rebase` · `revert` · `format-patch`

### Email

`am` · `apply` · `format-patch` · `imap-send` · `mailinfo` · `mailsplit` ·
`request-pull` · `send-email`

### External Systems

`archimport` · `cvsexportcommit` · `cvsimport` · `cvsserver` · `fast-export` ·
`fast-import` · `p4` · `quiltimport` · `svn`

### Administration

`archive` · `bundle` · `citool` · `clean` · `filter-branch` · `fsck` ·
`gc` · `gui` · `gitk` · `gitweb` · `hook` · `instaweb` · `maintenance` ·
`pack-refs` · `prune` · `reflog` · `refs` · `repack` · `replace` · `scalar`

### Server Admin

`daemon` · `fetch-pack` · `http-backend` · `http-fetch` · `http-push` ·
`receive-pack` · `send-pack` · `shell` · `update-server-info` ·
`upload-archive` · `upload-pack`

### Plumbing Commands

`cat-file` · `check-attr` · `check-ignore` · `check-mailmap` ·
`check-ref-format` · `checkout-index` · `column` · `commit-graph` ·
`commit-tree` · `count-objects` · `diff-files` · `diff-index` · `diff-tree` ·
`for-each-ref` · `for-each-repo` · `fmt-merge-msg` · `get-tar-commit-id` ·
`hash-object` · `index-pack` · `interpret-trailers` · `ls-files` · `ls-tree` ·
`merge-base` · `merge-index` · `merge-one-file` · `mktag` · `mktree` ·
`multi-pack-index` · `name-rev` · `pack-objects` · `pack-redundant` ·
`patch-id` · `prune-packed` · `read-tree` · `replay` · `rev-list` ·
`rev-parse` · `show-branch` · `show-index` · `show-ref` · `sh-i18n` ·
`sh-setup` · `sparse-checkout` · `stripspace` · `symbolic-ref` ·
`unpack-file` · `unpack-objects` · `update-index` · `update-ref` · `var` ·
`verify-commit` · `verify-pack` · `verify-tag` · `write-tree`

### Zmin Workflow Commands

`save` · `changes` · `publish` · `update` · `undo` · `timeline` · `recover` ·
`hooks` · `compatibility` · `repo` · `history` · `backfill` · `last-modified` ·
`diff-pairs`

## Preview Limits

Zmin works with regular Git repositories and existing Git remotes. This preview
does not include Git LFS, reftable repositories, or official package-manager
installs yet.

Command availability means the command is present in the preview command
surface. Some edge-case options and environments are still being hardened. Keep
a current backup before using preview builds on important repositories.

## Speed Snapshot

Median seconds on current preview fixtures. Values in parentheses show how fast
Zmin is against that tool by median; values below `x1.00` mean Zmin is slower.

| Platform | Operation | Zmin | Git | Gitoxide |
| --- | --- | ---: | ---: | ---: |
| macOS | `status` | `0.023s` | `0.029s` (`x1.28`) | `0.028s` (`x1.25`) |
| macOS | `add` | `1.073s` | `1.491s` (`x1.39`) | n/a |
| macOS | `commit` | `0.041s` | `0.071s` (`x1.71`) | n/a |
| macOS | `clone` | `0.067s` | `0.141s` (`x2.10`) | `0.318s` (`x4.73`) |
| macOS | `clone-large` | `0.158s` | `0.288s` (`x1.83`) | `0.503s` (`x3.19`) |
| macOS | `fetch-incremental` | `0.034s` | `0.073s` (`x2.17`) | `0.218s` (`x6.45`) |
| macOS | `push-batch` | `0.242s` | `0.323s` (`x1.34`) | n/a |
| macOS | `log` | `0.021s` | `0.027s` (`x1.28`) | `0.023s` (`x1.07`) |
| macOS | `merge-base` | `0.018s` | `0.027s` (`x1.46`) | `0.023s` (`x1.27`) |
| Windows | `clone` | `1.136s` | `1.413s` (`x1.24`) | `1.152s` (`x1.01`) |
| Windows | `clone-large` | `4.257s` | `2.515s` (`x0.59`) | `2.755s` (`x0.65`) |

## Trademark Notice

Zmin is not affiliated with the Git Project or Software Freedom Conservancy.

Git is a registered trademark of Software Freedom Conservancy, Inc.

## License

This repository is currently shared publicly for evaluation and reference only.

No permission is granted to use, copy, modify, distribute, sublicense, or create
derivative works from this code without prior written consent.
