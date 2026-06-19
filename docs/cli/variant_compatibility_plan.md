# Variant Compatibility Plan

Command-name coverage is not full Git compatibility. A supported item must be
counted as a variant:

`command + option + mode + repository state + transport/workflow`.

Examples:

- `blame --date=iso` and `blame --date=relative` are two variants.
- `fetch --depth=1 <remote> <refspec>` and `fetch --depth=1 <remote> <refspec> <refspec>` are separate variants.
- Parser acceptance does not count. The behavior must match stock Git output,
  exit code and repository state.

## Counting Rules

- Count only stock-Git-supported behavior unless there is an explicit Zmin-only
  command.
- Do not count corrupt repository formats, reftable, Git LFS, legacy external
  bridges or package-manager install channels as closed unless they are
  implemented and tested.
- A variant is closed only with focused parity evidence: local compat test,
  upstream Git test slice or dogfood reproduction.
- Public docs may show command-name presence. They must not call it full support.

## Current Burn-Down

| Block | Closed | Open | Evidence |
| --- | ---: | ---: | --- |
| `blame --date` stock modes | `11` | `0` | `git_history_query_compat` |
| `blame --date` newly closed modes | `2` | `0` | `relative`, `human` |
| `blame` normal display flags | `4` | `0` | `-s`, `-t`, `-b`, `-c` |
| `blame` no-toggle/reset forms | `15` | `0` | standalone `--no-*` plus positive-then-no forms, excluding nuanced `--root --no-root` |
| `blame --no-abbrev` forms | `3` | `0` | `--no-abbrev`, `--abbrev=N --no-abbrev`, `--no-abbrev --abbrev=N` |
| `init` quiet forms | `2` | `0` | `-q`, `--quiet` |
| `notes add` empty editor forms | `2` | `0` | `--allow-empty`, `--allow-empty --no-edit` |
| `notes copy` stdin/no-stdin toggle forms | `2` | `0` | `--stdin --no-stdin`, `--no-stdin --stdin` |
| `notes edit` message-source forms | `8` | `0` | `-m`, `--message=`, `-F`, `--file=`, `-C`, `--reuse-message=`, `-c`, `--reedit-message=` |
| `notes edit` compact short source forms | `4` | `0` | `-mmsg`, `-Ffile`, `-C<object>`, `-c<object>` |
| `notes merge` no-strategy toggle forms | `7` | `0` | merge order variants plus `--commit`/`--abort` state variants |
| `notes remove` stdin/no-stdin toggle forms | `2` | `0` | `--stdin --no-stdin`, `--no-stdin --stdin` |
| `clean` no-interactive toggle forms | `3` | `0` | `--no-interactive -n`, `-n --no-interactive`, `--interactive --no-interactive -n` |

The global denominator is still being audited. Until then, do not publish a
global compatibility percentage.

## Audit Order

1. Local git-replacement blockers from IDE/GUI dogfood.
2. Commands with live `unsupported` branches that stock Git accepts.
3. High-use porcelain variants: `status`, `add`, `commit`, `diff`, `log`,
   `blame`, `stash`, `branch`, `checkout`, `switch`, `restore`.
4. Transport variants: local/file, smart HTTP, SSH, git daemon, depth,
   explicit refspecs, tags, prune, proxy/auth.
5. Plumbing variants used by tools: `cat-file`, `rev-parse`, `for-each-ref`,
   `ls-files`, `update-index`, `read-tree`, `write-tree`.
6. Platform variants: macOS, Linux, Windows path/process behavior.

## Reporting Format

For each slice, report:

`block: closed/total variants`

Do not mix command counts with variant counts.
