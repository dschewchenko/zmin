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

Current preview: [`v0.0.1-preview.20260619T231023Z`](https://github.com/dschewchenko/zmin/releases/tag/v0.0.1-preview.20260619T231023Z)

Download a preview archive for your platform or build from source.

### macOS

Apple Silicon:

```bash
mkdir -p ~/.local/bin
curl -L https://github.com/dschewchenko/zmin/releases/download/v0.0.1-preview.20260619T231023Z/zmin-aarch64-apple-darwin.tar.gz | tar -xz
install -m 0755 zmin ~/.local/bin/zmin
zmin --version
```

Intel:

```bash
mkdir -p ~/.local/bin
curl -L https://github.com/dschewchenko/zmin/releases/download/v0.0.1-preview.20260619T231023Z/zmin-x86_64-apple-darwin.tar.gz | tar -xz
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
curl -L https://github.com/dschewchenko/zmin/releases/download/v0.0.1-preview.20260619T231023Z/zmin-x86_64-unknown-linux-gnu.tar.gz | tar -xz
install -m 0755 zmin ~/.local/bin/zmin
zmin --version
```

aarch64:

```bash
mkdir -p ~/.local/bin
curl -L https://github.com/dschewchenko/zmin/releases/download/v0.0.1-preview.20260619T231023Z/zmin-aarch64-unknown-linux-gnu.tar.gz | tar -xz
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
$Version = "v0.0.1-preview.20260619T231023Z"
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
- [`zmin-x86_64-unknown-linux-gnu.tar.gz`](https://github.com/dschewchenko/zmin/releases/download/v0.0.1-preview.20260619T231023Z/zmin-x86_64-unknown-linux-gnu.tar.gz)
- [`zmin-aarch64-unknown-linux-gnu.tar.gz`](https://github.com/dschewchenko/zmin/releases/download/v0.0.1-preview.20260619T231023Z/zmin-aarch64-unknown-linux-gnu.tar.gz)
- [`zmin-x86_64-apple-darwin.tar.gz`](https://github.com/dschewchenko/zmin/releases/download/v0.0.1-preview.20260619T231023Z/zmin-x86_64-apple-darwin.tar.gz)
- [`zmin-aarch64-apple-darwin.tar.gz`](https://github.com/dschewchenko/zmin/releases/download/v0.0.1-preview.20260619T231023Z/zmin-aarch64-apple-darwin.tar.gz)
- [`zmin-x86_64-pc-windows-msvc.zip`](https://github.com/dschewchenko/zmin/releases/download/v0.0.1-preview.20260619T231023Z/zmin-x86_64-pc-windows-msvc.zip)
- [`zmin-aarch64-pc-windows-msvc.zip`](https://github.com/dschewchenko/zmin/releases/download/v0.0.1-preview.20260619T231023Z/zmin-aarch64-pc-windows-msvc.zip)

Checksums: [`SHA256SUMS`](https://github.com/dschewchenko/zmin/releases/download/v0.0.1-preview.20260619T231023Z/SHA256SUMS)
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

## Compatibility Audit

Use commands as `zmin <command>`.

Zmin wires all `151` Git `2.47` command names to a command handler. That only
proves dispatch. Real compatibility is counted by behavior variants: command,
option, value, option combination, repository state, transport and platform.
The table below is audit progress, not a full support percentage. A group with
no matrix rows still needs a command-level option/value/state matrix before it
can be called complete.

| Git reference group | Git commands | Git doc option seed rows | Matrix rows | Matrix closed | Matrix open | Matrix invalid input | Closed block variants |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Setup and Config | `6` | `76` | `0` | `0` | `0` | `0` | `0` |
| Getting and Creating Projects | `2` | `57` | `0` | `0` | `0` | `0` | `2` |
| Basic Snapshotting | `9` | `252` | `60` | `56` | `0` | `4` | `64` |
| Branching and Merging | `9` | `267` | `0` | `0` | `0` | `0` | `30` |
| Sharing and Updating Projects | `5` | `119` | `44` | `30` | `14` | `0` | `0` |
| Inspection and Comparison | `7` | `108` | `0` | `0` | `0` | `0` | `8` |
| Patching | `5` | `179` | `0` | `0` | `0` | `0` | `0` |
| Debugging | `3` | `127` | `0` | `0` | `0` | `0` | `52` |
| Email | `6` | `229` | `0` | `0` | `0` | `0` | `0` |
| External Systems | `2` | `95` | `0` | `0` | `0` | `0` | `0` |
| Administration | `8` | `134` | `0` | `0` | `0` | `0` | `17` |
| Server Admin | `2` | `28` | `0` | `0` | `0` | `0` | `0` |
| Plumbing Commands | `20` | `332` | `0` | `0` | `0` | `0` | `76` |
| Other Git `2.47` commands | `71` | `600` | `0` | `0` | `0` | `0` | `4` |
| **Git `2.47` unique total** | **`151`** | **`2500`** | **`104`** | **`86`** | **`14`** | **`4`** | **`253`** |

The `git` reference entry maps to the binary entry point, not a subcommand in
the Git `2.47` command list. Zmin supports the replacement entry point and
Git-compatible version output.

Do not read command inventory or doc option rows as implemented support. The
option rows are a seed extracted from Git `2.47.1` docs. They still need to be
split into values, negations, repeated options, order-sensitive combinations,
repository states, transports and platforms, then checked against stock Git.
Reference group rows follow the git-scm command sections and are not meant to
be added together; the total row is unique.

Current command-level matrices:

| Command | Rows | Closed | Partial | Open | Invalid input | Closed rate |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `status` | `60` | `56` | `0` | `0` | `4` | `93.3%` |
| `fetch` | `44` | `30` | `0` | `14` | `0` | `68.2%` |

A global percentage will be published only after every Git `2.47.1` command has
the same option/value/state matrix.

<details>
<summary>Commands counted in each group</summary>

- Setup and Config: `config`, `help`, `bugreport`, `credential`, `credential-cache`, `credential-store`
- Getting and Creating Projects: `init`, `clone`
- Basic Snapshotting: `add`, `status`, `diff`, `commit`, `notes`, `restore`, `reset`, `rm`, `mv`
- Branching and Merging: `branch`, `checkout`, `switch`, `merge`, `mergetool`, `log`, `stash`, `tag`, `worktree`
- Sharing and Updating Projects: `fetch`, `pull`, `push`, `remote`, `submodule`
- Inspection and Comparison: `show`, `log`, `diff`, `difftool`, `range-diff`, `shortlog`, `describe`
- Patching: `apply`, `cherry-pick`, `diff`, `rebase`, `revert`
- Debugging: `bisect`, `blame`, `grep`
- Email: `am`, `apply`, `imap-send`, `format-patch`, `send-email`, `request-pull`
- External Systems: `svn`, `fast-import`
- Administration: `clean`, `gc`, `fsck`, `reflog`, `filter-branch`, `instaweb`, `archive`, `bundle`
- Server Admin: `daemon`, `update-server-info`
- Plumbing Commands: `cat-file`, `check-ignore`, `checkout-index`, `commit-tree`, `count-objects`, `diff-index`, `for-each-ref`, `hash-object`, `ls-files`, `ls-tree`, `merge-base`, `read-tree`, `rev-list`, `rev-parse`, `show-ref`, `symbolic-ref`, `update-index`, `update-ref`, `verify-pack`, `write-tree`
- Other Git `2.47` commands: `annotate`, `archimport`, `check-attr`, `check-mailmap`, `check-ref-format`, `cherry`, `citool`, `column`, `commit-graph`, `cvsexportcommit`, `cvsimport`, `cvsserver`, `diagnose`, `diff-files`, `diff-tree`, `fast-export`, `fetch-pack`, `fmt-merge-msg`, `for-each-repo`, `get-tar-commit-id`, `gui`, `hook`, `http-backend`, `http-fetch`, `http-push`, `index-pack`, `interpret-trailers`, `ls-remote`, `mailinfo`, `mailsplit`, `maintenance`, `merge-file`, `merge-index`, `merge-one-file`, `merge-tree`, `mktag`, `mktree`, `multi-pack-index`, `name-rev`, `p4`, `pack-objects`, `pack-redundant`, `pack-refs`, `patch-id`, `prune`, `prune-packed`, `quiltimport`, `receive-pack`, `refs`, `repack`, `replace`, `replay`, `rerere`, `send-pack`, `sh-i18n`, `sh-setup`, `shell`, `show-branch`, `show-index`, `sparse-checkout`, `stage`, `stripspace`, `unpack-file`, `unpack-objects`, `upload-archive`, `upload-pack`, `var`, `verify-commit`, `verify-tag`, `version`, `whatchanged`

</details>

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
