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

Zmin is not 100% Git-compatible yet. It has handlers for all `151` Git `2.47.1`
command names, but a handler only proves that the command can be routed.

Real compatibility is measured at behavior-row level:

`command + option + value + option combination + repository state + transport + platform`

Examples that count as different rows: `status -z`, `status --porcelain=v2 -z
--branch`, `fetch --depth=1 origin main`, `fetch --depth=1 origin main next`,
`blame --date=relative -L 1,3 file`.

The real denominator is still being built from Git docs, upstream Git tests and
real tool traces. A command or option is not counted as supported just because
Zmin parses it or because one example row works.

Current state:

| Layer | Count | Meaning |
| --- | ---: | --- |
| Fully complete command matrices | `0/151` | no command has a full Git behavior matrix yet |
| Fully complete documented option matrices | `0/4632` | no documented command-option pair has a full behavior matrix yet |
| Commands with any matrix rows | `73/151` | commands that have started behavior matrices; this is not support |
| Documented option spellings represented by rows | `432/4632` | option spellings that have at least one behavior row; this is not support |
| Written behavior rows | `1727` | explicit rows currently written in command matrices |
| Written rows matching stock Git | `1427/1727` | supported-behavior rows with focused parity evidence |
| Partial written rows | `0/1727` | written rows with incomplete parity |
| Open written rows | `1/1727` | written rows that still do not match stock Git |
| Invalid input rows | `299/1727` | rows where stock Git rejects the input |
| Full Git behavior denominator | not known yet | still being expanded from docs, upstream tests, IDE traces and platform checks |

Do not read `1427/1727` as Git compatibility. It only means `1427` of the `1727`
rows already written down are closed supported-behavior rows. The larger
unexpanded surface is not counted yet, `0/1727` rows are partially matching,
`1/1727` rows are known open mismatches, and `299/1727` additional rows are
stock-compatible invalid inputs. Do not read `432/4632` as option support
either; it only means those option spellings have at least one row in the audit.

Option spellings are only seed data. Each spelling still has to be expanded into
values, missing-value defaults, negations, repeated forms, option order,
positional modes, repository states, transports and platforms. A command is
complete only after that matrix is finished and every supported row has stock
Git evidence.

The compatibility audit now proceeds in this order:

1. Seed all Git `2.47.1` command names and documented option spellings.
2. Expand each option into values, negations, repeats, ordering and positional
   forms.
3. Add repository states, transports, platforms, upstream Git tests and real IDE
   or GUI command traces.
4. Use stock Git as the expected result for each row.
5. Mark a row closed only when Zmin matches that exact output, exit code and
   repository state.

Audit progress by git-scm reference group:

| Git reference group | Git commands | Complete command matrices | Git doc option seed | Complete documented option matrices | Behavior rows written | Written rows matching stock Git | Partial written rows | Open written rows | Invalid input rows |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Setup and Config | `6` | `0` | `276` | `0` | `143` | `129` | `0` | `0` | `14` |
| Getting and Creating Projects | `2` | `0` | `66` | `0` | `51` | `48` | `0` | `1` | `2` |
| Basic Snapshotting | `9` | `0` | `371` | `0` | `148` | `129` | `0` | `0` | `19` |
| Branching and Merging | `9` | `0` | `581` | `0` | `113` | `83` | `0` | `0` | `30` |
| Sharing and Updating Projects | `5` | `0` | `309` | `0` | `346` | `321` | `0` | `0` | `25` |
| Inspection and Comparison | `7` | `0` | `774` | `0` | `265` | `258` | `0` | `0` | `7` |
| Patching | `5` | `0` | `333` | `0` | `11` | `9` | `0` | `0` | `2` |
| Debugging | `3` | `0` | `132` | `0` | `114` | `33` | `0` | `0` | `81` |
| Email | `6` | `0` | `361` | `0` | `4` | `4` | `0` | `0` | `0` |
| External Systems | `2` | `0` | `120` | `0` | `0` | `0` | `0` | `0` | `0` |
| Administration | `8` | `0` | `147` | `0` | `58` | `35` | `0` | `0` | `23` |
| Server Admin | `2` | `0` | `30` | `0` | `0` | `0` | `0` | `0` | `0` |
| Plumbing Commands | `21` | `0` | `650` | `0` | `376` | `313` | `0` | `0` | `63` |
| Other Git 2.47 commands | `70` | `0` | `1069` | `0` | `98` | `65` | `0` | `0` | `33` |
| **Git `2.47.1` unique total** | **`151`** | **`0`** | **`4632`** | **`0`** | **`1727`** | **`1427`** | **`0`** | **`1`** | **`299`** |

The `git` reference entry maps to the binary entry point, not a subcommand in
the Git `2.47` command list. Zmin supports the replacement entry point and
Git-compatible version output.

Reference group rows follow the git-scm command sections. The total row is
unique. These rows are audit progress, not support percentages.

Current command-level matrices:

| Command | Git doc option seed | Complete documented option matrices | Doc spellings represented by rows | Behavior rows written | Written rows matching stock Git | Partial rows | Open rows | Invalid input rows | Complete matrix |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| `add` | `34` | `0` | `2` | `6` | `3` | `0` | `0` | `3` | no |
| `apply` | `39` | `0` | `4` | `10` | `9` | `0` | `0` | `1` | no |
| `archive` | `17` | `0` | `3` | `11` | `10` | `0` | `0` | `1` | no |
| `bisect` | `18` | `0` | `0` | `1` | `0` | `0` | `0` | `1` | no |
| `blame` | `39` | `0` | `6` | `101` | `21` | `0` | `0` | `80` | no |
| `branch` | `51` | `0` | `13` | `31` | `18` | `0` | `0` | `13` | no |
| `bundle` | `15` | `0` | `1` | `11` | `3` | `0` | `0` | `8` | no |
| `cat-file` | `21` | `0` | `2` | `9` | `1` | `0` | `0` | `8` | no |
| `check-attr` | `6` | `0` | `1` | `5` | `5` | `0` | `0` | `0` | no |
| `check-ignore` | `10` | `0` | `7` | `19` | `9` | `0` | `0` | `10` | no |
| `check-mailmap` | `3` | `0` | `1` | `6` | `6` | `0` | `0` | `0` | no |
| `check-ref-format` | `6` | `0` | `3` | `11` | `5` | `0` | `0` | `6` | no |
| `cherry` | `6` | `0` | `1` | `6` | `6` | `0` | `0` | `0` | no |
| `checkout` | `43` | `0` | `0` | `1` | `0` | `0` | `0` | `1` | no |
| `checkout-index` | `19` | `0` | `0` | `1` | `0` | `0` | `0` | `1` | no |
| `clean` | `13` | `0` | `3` | `12` | `8` | `0` | `0` | `4` | no |
| `clone` | `56` | `0` | `22` | `49` | `46` | `0` | `1` | `2` | no |
| `column` | `10` | `0` | `1` | `1` | `0` | `0` | `0` | `1` | no |
| `commit-graph` | `18` | `0` | `0` | `3` | `0` | `0` | `0` | `3` | no |
| `config` | `243` | `0` | `17` | `120` | `110` | `0` | `0` | `10` | no |
| `credential` | `0` | `0` | `0` | `4` | `3` | `0` | `0` | `1` | no |
| `credential-cache` | `2` | `0` | `1` | `4` | `4` | `0` | `0` | `0` | no |
| `credential-store` | `1` | `0` | `0` | `3` | `3` | `0` | `0` | `0` | no |
| `describe` | `18` | `0` | `8` | `10` | `10` | `0` | `0` | `0` | no |
| `diff` | `133` | `0` | `56` | `151` | `149` | `0` | `0` | `2` | no |
| `diff-files` | `130` | `0` | `4` | `6` | `6` | `0` | `0` | `0` | no |
| `diff-index` | `124` | `0` | `3` | `3` | `3` | `0` | `0` | `0` | no |
| `diff-tree` | `151` | `0` | `47` | `74` | `74` | `0` | `0` | `0` | no |
| `fast-import` | `25` | `0` | `1` | `7` | `4` | `0` | `0` | `3` | no |
| `fetch` | `73` | `0` | `30` | `304` | `294` | `0` | `0` | `10` | no |
| `filter-branch` | `37` | `0` | `11` | `14` | `14` | `0` | `0` | `0` | no |
| `for-each-ref` | `22` | `0` | `2` | `34` | `23` | `0` | `0` | `11` | no |
| `grep` | `75` | `0` | `4` | `12` | `12` | `0` | `0` | `0` | no |
| `http-fetch` | `10` | `0` | `0` | `1` | `0` | `0` | `0` | `1` | no |
| `index-pack` | `18` | `0` | `1` | `3` | `0` | `0` | `0` | `3` | no |
| `init` | `10` | `0` | `2` | `2` | `2` | `0` | `0` | `0` | no |
| `interpret-trailers` | `24` | `0` | `6` | `10` | `10` | `0` | `0` | `0` | no |
| `mailinfo` | `10` | `0` | `3` | `4` | `4` | `0` | `0` | `0` | no |
| `log` | `282` | `0` | `37` | `98` | `93` | `0` | `0` | `5` | no |
| `ls-files` | `42` | `0` | `28` | `120` | `102` | `0` | `0` | `18` | no |
| `ls-remote` | `16` | `0` | `0` | `2` | `1` | `0` | `0` | `1` | no |
| `ls-tree` | `15` | `0` | `2` | `4` | `3` | `0` | `0` | `1` | no |
| `maintenance` | `14` | `0` | `3` | `5` | `0` | `0` | `0` | `5` | no |
| `merge` | `69` | `0` | `1` | `1` | `0` | `0` | `0` | `1` | no |
| `merge-base` | `27` | `0` | `2` | `12` | `10` | `0` | `0` | `2` | no |
| `multi-pack-index` | `10` | `0` | `0` | `4` | `1` | `0` | `0` | `3` | no |
| `notes` | `33` | `0` | `0` | `6` | `0` | `0` | `0` | `6` | no |
| `p4` | `40` | `0` | `0` | `1` | `1` | `0` | `0` | `0` | no |
| `pack-objects` | `44` | `0` | `1` | `10` | `4` | `0` | `0` | `6` | no |
| `patch-id` | `4` | `0` | `3` | `6` | `6` | `0` | `0` | `0` | no |
| `prune` | `7` | `0` | `1` | `7` | `6` | `0` | `0` | `1` | no |
| `push` | `57` | `0` | `0` | `1` | `0` | `0` | `0` | `1` | no |
| `rebase` | `103` | `0` | `1` | `1` | `0` | `0` | `0` | `1` | no |
| `reflog` | `13` | `0` | `0` | `2` | `0` | `0` | `0` | `2` | no |
| `remote` | `23` | `0` | `1` | `32` | `26` | `0` | `0` | `6` | no |
| `rerere` | `7` | `0` | `0` | `1` | `0` | `0` | `0` | `1` | no |
| `rev-list` | `165` | `0` | `8` | `10` | `10` | `0` | `0` | `0` | no |
| `rev-parse` | `72` | `0` | `24` | `64` | `58` | `0` | `0` | `6` | no |
| `rm` | `20` | `0` | `1` | `1` | `1` | `0` | `0` | `0` | no |
| `show-index` | `1` | `0` | `0` | `1` | `0` | `0` | `0` | `1` | no |
| `show-ref` | `14` | `0` | `5` | `11` | `8` | `0` | `0` | `3` | no |
| `shortlog` | `164` | `0` | `4` | `6` | `6` | `0` | `0` | `0` | no |
| `sparse-checkout` | `11` | `0` | `0` | `8` | `3` | `0` | `0` | `5` | no |
| `stash` | `30` | `0` | `0` | `52` | `48` | `0` | `0` | `4` | no |
| `status` | `26` | `0` | `23` | `135` | `125` | `0` | `0` | `10` | no |
| `stripspace` | `5` | `0` | `4` | `5` | `5` | `0` | `0` | `0` | no |
| `submodule` | `35` | `0` | `0` | `7` | `0` | `0` | `0` | `7` | no |
| `symbolic-ref` | `8` | `0` | `3` | `8` | `7` | `0` | `0` | `1` | no |
| `tag` | `40` | `0` | `11` | `27` | `17` | `0` | `0` | `10` | no |
| `var` | `3` | `0` | `1` | `12` | `9` | `0` | `0` | `3` | no |
| `verify-pack` | `4` | `0` | `0` | `2` | `0` | `0` | `0` | `2` | no |
| `version` | `2` | `0` | `2` | `4` | `2` | `0` | `0` | `2` | no |
| `worktree` | `28` | `0` | `0` | `1` | `0` | `0` | `0` | `1` | no |

Rows with `0` open items are still not complete command compatibility. They
only mean no open item remains among the rows currently written. Unwritten
values, option combinations, repository states, transports and platform cases
are still unknown.

A global percentage will be published only after every Git `2.47.1` command has
a complete matrix built from Git docs, upstream Git tests and real tool traces.

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

## Zmin-Only Extensions

Zmin has additive features that are not counted as Git `2.47.1` compatibility.
They are tracked separately in
[`docs/cli/zmin_extensions_inventory.md`](docs/cli/zmin_extensions_inventory.md).

Current extension inventory:

| Layer | Count |
| --- | ---: |
| Zmin-only commands | `9` |
| Zmin-only options on Git commands | `4` |
| Zmin-only environment controls | `1` |
| Stable extensions | `3` |
| Experimental extensions | `2` |
| Planned extensions | `1` |

Implemented extensions include `zmin clone --instant`, managed `zmin hooks`
commands, `zmin repo` metadata summaries and CMS-style porcelain such as
`zmin save`, `zmin changes`, `zmin publish` and `zmin update`. Transport
tuning through `ZMIN_GIT_HTTP_VERSION` is tracked as a Zmin-only environment
control.

The staged-file hook runner is planned as a Zmin-only extension, with an API
shape like `zmin hooks run pre-commit --staged -- command ...`. It will be
tracked below the extension inventory, not in the Git compatibility matrix.

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
