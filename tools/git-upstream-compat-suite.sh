#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
usage: tools/git-upstream-compat-suite.sh [quick|standard|exhaustive]

Runs selected upstream Git t-suite tests against zmin (or ZMIN_BIN).

Environment:
  ZMIN_BIN                    Path to zmin. Builds release when omitted.
  ZMIN_UPSTREAM_GIT_TAG       Git tag to test against. Default: v2.54.0.
  ZMIN_UPSTREAM_GIT_CACHE     Cache dir for upstream Git source/build.
  ZMIN_UPSTREAM_TEST_LIST     Test allowlist TSV. Default: tools/git-upstream-compat-tests.txt.
  ZMIN_UPSTREAM_OUT_DIR       Output dir for logs and summary.
  ZMIN_UPSTREAM_CARGO_PROFILE Cargo profile used when ZMIN_BIN is omitted.
                              Default: release. Use compat for faster
                              behavior-only iteration.
  ZMIN_UPSTREAM_ALLOW_FAILURES=1  Report failures but exit 0.
  ZMIN_UPSTREAM_TEST_FLAGS    Flags passed to each upstream test. Default: -q.
  ZMIN_UPSTREAM_STOCK_GIT_CONTROL=1
                              Run the selected upstream tests against stock git
                              from PATH instead of a zmin shim.
  ZMIN_UPSTREAM_BOUNDED_RUN=1
                              Stop an upstream test after the max numeric
                              --run selector. Use only for focused parity
                              slices where full skip-heavy Windows/MSYS loops
                              are not stable evidence.
  ZMIN_UPSTREAM_SKIP_UNSUPPORTED_REFTABLE=1
                               Skip upstream assertions that require reftable
                               ref storage, which Zmin does not support yet.
EOF
}

mode="${1:-quick}"
case "$mode" in
  quick|standard|exhaustive) ;;
  -h|--help)
    usage
    exit 0
    ;;
  *)
    usage
    exit 2
    ;;
esac

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tag="${ZMIN_UPSTREAM_GIT_TAG:-v2.54.0}"
cache_root="${ZMIN_UPSTREAM_GIT_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/zmin/git-upstream}"
source_dir="$cache_root/git-$tag"
test_list="${ZMIN_UPSTREAM_TEST_LIST:-$repo_root/tools/git-upstream-compat-tests.txt}"
out_dir="${ZMIN_UPSTREAM_OUT_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/zmin-upstream-compat.XXXXXX")}"
jobs="${ZMIN_UPSTREAM_JOBS:-4}"
test_flags="${ZMIN_UPSTREAM_TEST_FLAGS:--q}"
stock_git_control="${ZMIN_UPSTREAM_STOCK_GIT_CONTROL:-0}"
bounded_run="${ZMIN_UPSTREAM_BOUNDED_RUN:-0}"
cargo_profile="${ZMIN_UPSTREAM_CARGO_PROFILE:-release}"

mkdir -p "$cache_root" "$out_dir"

zmin_bin="${ZMIN_BIN:-}"
if [[ "$stock_git_control" == "1" ]]; then
  stock_git="$(command -v git || true)"
  if [[ -z "$stock_git" ]]; then
    echo "missing stock git for ZMIN_UPSTREAM_STOCK_GIT_CONTROL=1" >&2
    exit 2
  fi
  zmin_bin="$stock_git"
elif [[ -z "$zmin_bin" ]]; then
  cargo_args=(build --manifest-path "$repo_root/Cargo.toml" -p zmin-cli --bin zmin)
  if [[ "$cargo_profile" == "release" ]]; then
    cargo_args+=(--release)
  else
    cargo_args+=(--profile "$cargo_profile")
  fi
  rustup run stable cargo "${cargo_args[@]}" >/dev/null
  zmin_bin="$repo_root/target/$cargo_profile/zmin"
elif [[ "$zmin_bin" != /* && "$zmin_bin" != [A-Za-z]:* ]]; then
  zmin_bin="$(cd "$repo_root" && pwd)/$zmin_bin"
fi

if [[ "${RUNNER_OS:-}" == "Windows" || "${OS:-}" == "Windows_NT" ]]; then
  if [[ ! -x "$zmin_bin" && -x "${zmin_bin}.exe" ]]; then
    zmin_bin="${zmin_bin}.exe"
  fi
else
  if [[ ! -x "$zmin_bin" && -x "${zmin_bin}.exe" ]]; then
    zmin_bin="${zmin_bin}.exe"
  fi
fi

if [[ ! -x "$zmin_bin" ]]; then
  echo "missing executable ZMIN_BIN: $zmin_bin" >&2
  exit 2
fi

ensure_git_http_backend() {
  local backend_dir
  backend_dir="$(dirname "$zmin_bin")"
  if [[ "${RUNNER_OS:-}" == "Windows" || "${OS:-}" == "Windows_NT" ]]; then
    local backend="$backend_dir/git-http-backend.exe"
    if [[ ! -x "$backend" ]]; then
      cp "$zmin_bin" "$backend"
    fi
  else
    local backend="$backend_dir/git-http-backend"
    if [[ ! -x "$backend" ]]; then
      cat >"$backend" <<EOF
#!/usr/bin/env sh
exec "$zmin_bin" http-backend "\$@"
EOF
      chmod +x "$backend"
    fi
  fi
}

download_upstream() {
  local archive="$cache_root/$tag.tar.gz"
  if [[ ! -d "$source_dir" ]]; then
    rm -rf "$source_dir.tmp"
    mkdir -p "$source_dir.tmp"
    if [[ ! -s "$archive" ]]; then
      curl -fsSL "https://github.com/git/git/archive/refs/tags/${tag}.tar.gz" -o "$archive"
    fi
    tar -xzf "$archive" -C "$source_dir.tmp" --strip-components=1
    mv "$source_dir.tmp" "$source_dir"
  fi
}

prepare_upstream_harness() {
  download_upstream
  (
    cd "$source_dir"
    if [[ "${RUNNER_OS:-}" == "Windows" || "${OS:-}" == "Windows_NT" ]]; then
      perl -0pi -e 's/\*MINGW\*\)/\*MINGW\*|\*MSYS\*\)/g' t/test-lib.sh
      perl -0pi -e 's/GIT_TEST_CMP="GIT_DIR=\/dev\/null git diff --no-index --ignore-cr-at-eol --"/GIT_TEST_CMP="diff -u"/g' t/test-lib.sh
      perl -0pi -e 's/GIT_TEST_CMP="\$DIFF -u"/GIT_TEST_CMP="diff -u"/g' t/test-lib.sh
      perl -0pi -e 's/GIT_TEST_CMP=" +-u"/GIT_TEST_CMP="diff -u"/g' t/test-lib.sh
    fi
    if command -v make >/dev/null 2>&1 && [[ ! -f GIT-BUILD-OPTIONS || ! -x t/helper/test-tool ]]; then
      make -j"$jobs" NO_GETTEXT=1 GIT-BUILD-OPTIONS t/helper/test-tool templates
    fi
    if [[ ! -d templates/blt ]]; then
      mkdir -p templates/blt
      cp -R templates/hooks templates/info templates/blt/
    fi
    if [[ ! -f GIT-BUILD-OPTIONS ]]; then
      local shell_path perl_path x_suffix
      shell_path="$(command -v sh)"
      perl_path="$(command -v perl || printf perl)"
      x_suffix=""
      if [[ "${RUNNER_OS:-}" == "Windows" || "${OS:-}" == "Windows_NT" ]]; then
        x_suffix=".exe"
      fi
      cat >GIT-BUILD-OPTIONS <<EOF
BROKEN_PATH_FIX='/^# @BROKEN_PATH_FIX@$/d'
DIFF='diff'
GIT_SOURCE_DIR='$source_dir'
GIT_TEST_CMP='diff -u'
GIT_TEST_CMP_USE_COPIED_CONTEXT=''
GIT_TEST_GITPERLLIB='$source_dir/perl/build/lib'
GIT_TEST_INDEX_VERSION=''
GIT_TEST_OPTS=''
GIT_TEST_PERL_FATAL_WARNINGS=''
GIT_TEST_TEMPLATE_DIR='$source_dir/templates/blt'
GIT_TEST_TEXTDOMAINDIR='$source_dir/po/build/locale'
GIT_TEST_UTF8_LOCALE=''
NO_CURL='1'
NO_EXPAT='1'
NO_GETTEXT='1'
NO_PERL=''
NO_PYTHON='1'
PERL_PATH='$perl_path'
SHELL_PATH='$shell_path'
TEST_OUTPUT_DIRECTORY=''
TEST_SHELL_PATH='$shell_path'
X='$x_suffix'
EOF
    fi
    if [[ -f GIT-BUILD-OPTIONS ]]; then
      perl -0pi -e "s/GIT_TEST_CMP='[^']*'/GIT_TEST_CMP='diff -u'/" GIT-BUILD-OPTIONS
    fi
    perl -0pi -e 's/\n\tif test -n "(?:\$ZMIN_UPSTREAM_STOP_AFTER_TEST)?" &&\n\t   test "(?:\$ZMIN_UPSTREAM_STOP_AFTER_TEST)?" -ge "(?:\$ZMIN_UPSTREAM_STOP_AFTER_TEST)?"\n\tthen\n\t\ttest_done\n\tfi\n/\n/' t/test-lib.sh
    if ! grep -q 'ZMIN_UPSTREAM_STOP_AFTER_TEST' t/test-lib.sh; then
      perl -0pi -e 's/test_finish_ \(\) \{\n/test_finish_ () {\n\tif test -n "\$ZMIN_UPSTREAM_STOP_AFTER_TEST" &&\n\t   test "\$test_count" -ge "\$ZMIN_UPSTREAM_STOP_AFTER_TEST"\n\tthen\n\t\ttest_done\n\tfi\n/' t/test-lib.sh
    fi
    if [[ ! -x t/helper/test-tool && ! -x t/helper/test-tool.exe ]] ||
      grep -qs "upstream test-tool helper is not available" t/helper/test-tool t/helper/test-tool.exe 2>/dev/null
    then
      mkdir -p t/helper
      cat >t/helper/test-tool <<'EOF'
#!/usr/bin/env sh
if test "$1" = "path-utils" && test "$2" = "absolute_path"
then
  shift 2
  for path
  do
    case "$path" in
      /* | [A-Za-z]:*) printf '%s\n' "$path" ;;
      *) printf '%s/%s\n' "$(pwd -W 2>/dev/null || pwd)" "$path" ;;
    esac
  done
  exit 0
fi
if test "$1" = "path-utils" && test "$2" = "file-size"
then
  shift 2
  perl -e '
    use strict;
    use warnings;
    my $status = 0;
    for my $path (@ARGV) {
      my @st = stat($path);
      if (!@st) {
        warn "Cannot stat '\''$path'\'': $!\n";
        $status = 1;
      } else {
        print $st[7], "\n";
      }
    }
    exit $status;
  ' "$@"
  exit $?
fi
if test "$1" = "genrandom"
then
  shift
  seed="${1-}"
  count="${2-}"
  if test -z "$seed" || test $# -gt 2
  then
    echo "usage: test-tool genrandom <seed_string> [<size>]" >&2
    exit 1
  fi
  perl -e '
    use strict;
    use warnings;
    binmode STDOUT;
    my ($seed, $count) = @ARGV;
    my $next = 0;
    for my $byte (unpack("C*", $seed . "\0")) {
      $next = (($next * 11) + $byte) & 0xffff_ffff;
    }
    if (!defined($count) || $count eq "") {
      $count = 0xffff_ffff;
    } elsif ($count =~ /^([0-9]+)([kKmMgG]?)$/) {
      my %suffix = (
        "" => 1,
        k => 1024, K => 1024,
        m => 1024 * 1024, M => 1024 * 1024,
        g => 1024 * 1024 * 1024, G => 1024 * 1024 * 1024,
      );
      $count = $1 * $suffix{$2};
    } else {
      die "cannot parse argument '$count'\n";
    }
    while ($count-- > 0) {
      $next = (($next * 1103515245) + 12345) & 0xffff_ffff;
      print chr(($next >> 16) & 0xff);
    }
  ' "$seed" "$count"
  exit $?
fi
if test "$1" = "env-helper"
then
  shift
  type=
  default=
  exit_code=
  while test $# -gt 0
  do
    case "$1" in
      --type=*) type="${1#--type=}"; shift ;;
      --default=*) default="${1#--default=}"; shift ;;
      --exit-code) exit_code=1; shift ;;
      --) shift; break ;;
      *) break ;;
    esac
  done
  name="${1-}"
  if test "$type" != "bool" || test -z "$exit_code" || test -z "$name"
  then
    echo "unsupported env-helper invocation" >&2
    exit 127
  fi
  case "$default" in
    true|TRUE|yes|YES|on|ON|1|false|FALSE|no|NO|off|OFF|0|'') ;;
    *) exit 128 ;;
  esac
  if eval "test \"\${$name+set}\" = set"
  then
    eval "value=\${$name}"
  else
    value="$default"
  fi
  case "$value" in
    true|TRUE|yes|YES|on|ON|1) exit 0 ;;
    false|FALSE|no|NO|off|OFF|0|'') exit 1 ;;
    *) exit 128 ;;
  esac
fi
if test "$1" = "rot13-filter"
then
  shift
  log=
  while test $# -gt 0
  do
    case "$1" in
      --log=*) log="${1#--log=}"; shift ;;
      clean|smudge) break ;;
      *) shift ;;
    esac
  done
  perl -e '
    use strict;
    use warnings;
    binmode STDIN;
    binmode STDOUT;
    select(STDOUT);
    $| = 1;

    use Cwd qw(abs_path);

    my $log_path = shift @ARGV;
    my %allowed = map { $_ => 1 } @ARGV;
    open my $init_log, ">>", $log_path or die "rot13-filter: cannot open $log_path: $!\n";
    close $init_log;
    $log_path = abs_path($log_path) || $log_path;
    chdir "/" or die "rot13-filter: cannot chdir away from repository: $!\n";

    sub log_line {
      my ($path, $line) = @_;
      open my $log, ">>", $path or die "rot13-filter: cannot open $path: $!\n";
      print {$log} $line;
      close $log;
    }

    log_line($log_path, "START\n");

    sub read_pkt {
      my $hdr = "";
      my $n = read(STDIN, $hdr, 4);
      return undef if !defined($n) || $n == 0;
      die "short pkt-line header\n" if $n != 4;
      my $len = hex($hdr);
      return undef if $len == 0;
      die "invalid pkt-line length\n" if $len < 4;
      my $data = "";
      my $need = $len - 4;
      while (length($data) < $need) {
        my $chunk = "";
        my $got = read(STDIN, $chunk, $need - length($data));
        die "short pkt-line payload\n" if !defined($got) || $got == 0;
        $data .= $chunk;
      }
      return $data;
    }

    sub read_list {
      my @items;
      while (1) {
        my $pkt = read_pkt();
        last if !defined($pkt);
        chomp $pkt;
        push @items, $pkt;
      }
      return @items;
    }

    sub write_pkt {
      my ($data) = @_;
      printf STDOUT "%04x%s", length($data) + 4, $data;
    }

    sub write_flush {
      print STDOUT "0000";
    }

    sub rot13 {
      my ($data) = @_;
      $data =~ tr/A-Za-z/N-ZA-Mn-za-m/;
      return $data;
    }

    my @hello = read_list();
    if (!@hello || $hello[0] ne "git-filter-client") {
      die "unexpected filter hello\n";
    }
    write_pkt("git-filter-server\n");
    write_pkt("version=2\n");
    write_flush();

    my @client_caps = read_list();
    for my $cap (@client_caps) {
      if ($cap =~ /^capability=(.+)$/ && $allowed{$1}) {
        write_pkt("capability=$1\n");
      }
    }
    write_flush();
    log_line($log_path, "init handshake complete\n");

    my %delayed;
    my $list_available_calls = 0;

    while (1) {
      my @headers = read_list();
      last if !@headers;
      my ($command, $pathname);
      my @meta;
      for my $header (@headers) {
        if ($header =~ /^command=(.*)$/) {
          $command = $1;
        } elsif ($header =~ /^pathname=(.*)$/) {
          $pathname = $1;
        } elsif ($header =~ /^(ref|treeish|blob)=/) {
          push @meta, $header;
        }
      }

      if ($command eq "list_available_blobs") {
        $list_available_calls++;
        my @available = sort grep {
          ($list_available_calls == 1 && /test-delay1[01]\./) ||
          ($list_available_calls == 2 && /test-delay20\./)
        } keys %delayed;
        if (grep { /invalid-delay\./ } keys %delayed) {
          @available = ("unfiltered");
        }
        my $available_text = @available ? " " . join(" ", @available) : "";
        log_line($log_path, "IN: list_available_blobs$available_text [OK]\n");
        for my $path (@available) {
          write_pkt("pathname=$path\n");
        }
        write_flush();
        write_pkt("status=success\n");
        write_flush();
        next;
      }

      my $input = "";
      while (1) {
        my $pkt = read_pkt();
        last if !defined($pkt);
        $input .= $pkt;
      }
      my $meta = @meta ? join(" ", @meta) . " " : "";
      if ($command eq "clean" && $pathname eq "clean-write-fail.r") {
        log_line(
          $log_path,
          "IN: $command $pathname ${meta}" . length($input) . " [OK] -- [WRITE FAIL]\n"
        );
        print STDERR "clean write error\n";
        exit 1;
      }
      if ($command eq "smudge" && $pathname eq "smudge-write-fail.r") {
        log_line(
          $log_path,
          "IN: $command $pathname ${meta}" . length($input) . " [OK] -- [WRITE FAIL]\n"
        );
        print STDERR "smudge write error\n";
        exit 1;
      }
      if ($pathname eq "error.r") {
        log_line(
          $log_path,
          "IN: $command $pathname ${meta}" . length($input) . " [OK] -- [ERROR]\n"
        );
        write_pkt("status=error\n");
        write_flush();
        next;
      }
      if ($pathname eq "abort.r") {
        log_line(
          $log_path,
          "IN: $command $pathname ${meta}" . length($input) . " [OK] -- [ABORT]\n"
        );
        write_pkt("status=abort\n");
        write_flush();
        next;
      }
      my $output;
      if ($command eq "smudge" && exists $delayed{$pathname} && !length($input)) {
        $output = delete $delayed{$pathname};
      } elsif ($command eq "smudge" && $pathname =~ /(?:test-delay(?:10|11|20)|missing-delay|invalid-delay)\./) {
        $delayed{$pathname} = rot13($input);
        log_line(
          $log_path,
          "IN: $command $pathname ${meta}" . length($input) . " [OK] -- [DELAYED]\n"
        );
        write_pkt("status=delayed\n");
        write_flush();
        next;
      } else {
        $output = rot13($input);
      }
      my $out_packets = length($output) ? int((length($output) + 65515) / 65516) : 0;
      my $out_marker = "." x $out_packets;
      log_line(
        $log_path,
        "IN: $command $pathname ${meta}" . length($input) .
          " [OK] -- OUT: " . length($output) . " $out_marker [OK]\n"
      );

      write_pkt("status=success\n");
      write_flush();
      my $offset = 0;
      while ($offset < length($output)) {
        my $chunk = substr($output, $offset, 65516);
        write_pkt($chunk);
        $offset += length($chunk);
      }
      write_flush();
      write_pkt("status=success\n");
      write_flush();
    }

    log_line($log_path, "STOP\n");
  ' "$log" "$@"
  exit $?
fi
if test "$1" = "sha1"
then
  perl -MDigest::SHA=sha1_hex -e '
    use strict;
    use warnings;
    binmode STDIN;
    local $/;
    print sha1_hex(<STDIN>), "\n";
  '
  exit $?
fi
if test "$1" = "zlib" && test "${2-}" = "deflate"
then
  perl -MCompress::Zlib=compress -e '
    use strict;
    use warnings;
    binmode STDIN;
    binmode STDOUT;
    local $/;
    my $input = <STDIN>;
    my $compressed = compress($input);
    die "zlib deflate failed\n" unless defined $compressed;
    print $compressed;
  '
  exit $?
fi
if test "$1" = "chmtime"
then
  shift
  get=
  verbose=
  while test $# -gt 0
  do
    case "$1" in
      --get|-g) get=1; shift ;;
      --verbose) verbose=1; shift ;;
      --) shift; break ;;
      *) break ;;
    esac
  done
  spec=
  case "${1-}" in
    =*|+*|-*) spec="$1"; shift ;;
  esac
  perl -e '
    use strict;
    use warnings;
    my ($get, $verbose, $spec, @paths) = @ARGV;
    die "chmtime: missing path\n" unless @paths;
    for my $path (@paths) {
      my @st = stat($path) or die "chmtime: cannot stat $path: $!\n";
      my $mtime = $st[9];
      my $target = $mtime;
      if (defined($spec) && length($spec)) {
        if ($spec =~ /^=([+-]\d+)$/) {
          $target = time() + $1;
        } elsif ($spec =~ /^=(\d+)$/) {
          $target = $1;
        } elsif ($spec =~ /^([+-]\d+)$/) {
          $target = $mtime + $1;
        } else {
          die "chmtime: unsupported time spec $spec\n";
        }
        utime($target, $target, $path) or die "chmtime: cannot update $path: $!\n";
      }
      if ($get) {
        if ($verbose) {
          print "$path $target\n";
        } else {
          print "$target\n";
        }
      }
    }
  ' "${get:-}" "${verbose:-}" "$spec" "$@"
  exit $?
fi
ref_store_gitdir () {
  store="$1"
  case "$store" in
    main)
      worktree=.
      ;;
    worktree:*)
      worktree="${store#worktree:}"
      if test -d ".git/worktrees/$worktree"
      then
        printf '%s\n' ".git/worktrees/$worktree"
        return 0
      fi
      ;;
    *)
      return 1
      ;;
  esac
  gitfile="$worktree/.git"
  if test -f "$gitfile"
  then
    gitdir="$(sed -n 's/^gitdir: //p' "$gitfile")"
    case "$gitdir" in
      /* | [A-Za-z]:*) ;;
      *) gitdir="$worktree/$gitdir" ;;
    esac
  else
    gitdir="$gitfile"
  fi
  printf '%s\n' "$gitdir"
}
if test "$1" = "ref-store" && test "${3-}" = "create-reflog"
then
  gitdir="$(ref_store_gitdir "${2-}")" || exit 1
  ref="${4-}"
  log_path="$gitdir/logs/$ref"
  mkdir -p "$(dirname "$log_path")" &&
    : >"$log_path"
  exit $?
fi
if test "$1" = "ref-store" && test "${3-}" = "update-ref"
then
  gitdir="$(ref_store_gitdir "${2-}")" || exit 1
  message="${4-}"
  ref="${5-}"
  new_oid="${6-}"
  old_oid="${7-}"
  ref_path="$gitdir/$ref"
  log_path="$gitdir/logs/$ref"
  ident="${GIT_COMMITTER_NAME:-A U Thor} <${GIT_COMMITTER_EMAIL:-author@example.com}> 1112911993 -0700"
  mkdir -p "$(dirname "$ref_path")" "$(dirname "$log_path")" &&
    printf '%s\n' "$new_oid" >"$ref_path" &&
    printf '%s %s %s\t%s\n' "$old_oid" "$new_oid" "$ident" "$message" >"$log_path"
  exit $?
fi
if test "$1" = "ref-store" && test "${3-}" = "reflog-exists"
then
  gitdir="$(ref_store_gitdir "${2-}")" || exit 1
  ref="${4-}"
  test -f "$gitdir/logs/$ref"
  exit $?
fi
if test "$1" = "ref-store" && test "${2-}" != "${2#worktree:}" &&
  test "${3-}" = "for-each-reflog-ent"
then
  gitdir="$(ref_store_gitdir "$2")" || exit 1
  ref="${4-}"
  log_path="$gitdir/logs/$ref"
  if test -f "$log_path"
  then
    cat "$log_path"
  fi
  exit 0
fi
echo "upstream test-tool helper is not available in this installed-binary audit" >&2
exit 127
EOF
      chmod +x t/helper/test-tool
      cp t/helper/test-tool t/helper/test-tool.exe 2>/dev/null || true
      chmod +x t/helper/test-tool.exe 2>/dev/null || true
    fi
    if [[ -s "$cache_root/$tag.tar.gz" ]]; then
      tar -xOzf "$cache_root/$tag.tar.gz" "git-${tag#v}/t/t5510-fetch.sh" >t/t5510-fetch.sh
    fi
    if [[ "${ZMIN_UPSTREAM_SKIP_UNSUPPORTED_REFTABLE:-0}" == "1" ]]; then
      perl -0pi -e '
        s/\Q &&
	git clone --ref-format=reftable . case_sensitive &&
	(
		cd case_sensitive &&
		git branch branch1 &&
		git branch bRanch1
	) &&
	git clone --ref-format=reftable . case_sensitive_fd &&
	(
		cd case_sensitive_fd &&
		git branch foo\/bar &&
		git branch Foo
	) &&
	git clone --ref-format=reftable . case_sensitive_df &&
	(
		cd case_sensitive_df &&
		git branch Foo\/bar &&
		git branch foo
	)\E//;
        s/test_expect_success CASE_INSENSITIVE_FS,REFFILES /test_expect_success REFTABLE,CASE_INSENSITIVE_FS,REFFILES /g;
        s/test_expect_success REFFILES '\''existing reference lock/test_expect_success REFTABLE,REFFILES '\''existing reference lock/g;
        s/test_expect_success REFFILES '\''D\/F conflict on case sensitive filesystem with lock/test_expect_success REFTABLE,REFFILES '\''D\/F conflict on case sensitive filesystem with lock/g;
      ' t/t5510-fetch.sh
    fi
  )
}

mode_rank() {
  case "$1" in
    quick) echo 1 ;;
    standard) echo 2 ;;
    exhaustive) echo 3 ;;
    *) echo 99 ;;
  esac
}

selected_tests() {
  local max_rank
  max_rank="$(mode_rank "$mode")"
  awk -F '\t' -v max_rank="$max_rank" '
    /^#/ || NF < 2 { next }
    {
      rank = ($1 == "quick" ? 1 : ($1 == "standard" ? 2 : ($1 == "exhaustive" ? 3 : 99)))
      if (rank <= max_rank) {
        print $2 "\t" $1 "\t" $3
      }
    }
  ' "$test_list"
}

run_list_from_flags() {
  local arg want_next=0
  for arg in $test_flags; do
    if [[ "$want_next" == "1" ]]; then
      printf '%s\n' "$arg"
      return 0
    fi
    case "$arg" in
      --run=*)
        printf '%s\n' "${arg#--run=}"
        return 0
        ;;
      -r)
        want_next=1
        ;;
    esac
  done
  return 1
}

max_numeric_run_selector() {
  local run_list="$1"
  awk -v run_list="$run_list" '
    BEGIN {
      max = 0
      n = split(run_list, parts, /[,[:space:]]+/)
      for (i = 1; i <= n; i++) {
        part = parts[i]
        if (part == "") {
          continue
        }
        if (part ~ /^[0-9]+$/) {
          value = part + 0
        } else if (part ~ /^[0-9]+-[0-9]+$/) {
          split(part, range, "-")
          value = range[2] + 0
        } else {
          printf "unsupported bounded --run selector: %s\n", part > "/dev/stderr"
          exit 2
        }
        if (value > max) {
          max = value
        }
      }
      if (max <= 0) {
        print "bounded run requires a numeric --run selector" > "/dev/stderr"
        exit 2
      }
      print max
    }
  '
}

make_git_shim() {
  local shim_dir="$1"
  mkdir -p "$shim_dir"
  if [[ "${RUNNER_OS:-}" == "Windows" || "${OS:-}" == "Windows_NT" ]]; then
    cp "$zmin_bin" "$shim_dir/git.exe"
  else
    cat >"$shim_dir/git" <<EOF
#!/usr/bin/env sh
exec "$zmin_bin" "\$@"
EOF
    chmod +x "$shim_dir/git"
  fi
}

if [[ "$stock_git_control" != "1" ]]; then
  ensure_git_http_backend
fi
prepare_upstream_harness

if [[ "$stock_git_control" == "1" ]]; then
  shim_dir="$(dirname "$zmin_bin")"
else
  shim_dir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-upstream-git-shim.XXXXXX")"
  trap 'rm -rf "$shim_dir"' EXIT
  make_git_shim "$shim_dir"
fi

summary="$out_dir/summary.tsv"
: >"$summary"
printf 'mode\ttest\tstatus\treason\tlog\n' >>"$summary"

total=0
passed=0
failed=0
bounded_stop_after=""

if [[ "$bounded_run" == "1" ]]; then
  run_list="$(run_list_from_flags)" || {
    echo "ZMIN_UPSTREAM_BOUNDED_RUN=1 requires ZMIN_UPSTREAM_TEST_FLAGS to include --run" >&2
    exit 2
  }
  bounded_stop_after="$(max_numeric_run_selector "$run_list")"
  printf 'bounded_run_stop_after=%s\n' "$bounded_stop_after"
fi

while IFS=$'\t' read -r test_name test_mode reason; do
  [[ -n "$test_name" ]] || continue
  total=$((total + 1))
  log="$out_dir/${test_name%.sh}.log"
  trash_dir="$source_dir/t/trash directory.${test_name%.sh}"
  printf 'upstream git compat: %s (%s)\n' "$test_name" "$reason"
  if [[ -e "$trash_dir" ]]; then
    chmod -R u+w "$trash_dir" 2>/dev/null || true
    rm -rf "$trash_dir" || {
      echo "failed to remove stale upstream trash dir: $trash_dir" >&2
      exit 1
    }
  fi
  set +e
  if [[ "${RUNNER_OS:-}" == "Windows" || "${OS:-}" == "Windows_NT" ]]; then
    (
      cd "$source_dir/t"
      ZMIN_UPSTREAM_STOP_AFTER_TEST="$bounded_stop_after" \
      GIT_TEST_DEFAULT_HASH=sha1 \
      GIT_TEST_INSTALLED="$shim_dir" \
      sh "$test_name" $test_flags
    ) >"$log" 2>&1
    rc=$?
  else
  (
    cd "$source_dir/t"
    ZMIN_UPSTREAM_STOP_AFTER_TEST="$bounded_stop_after" \
    GIT_TEST_DEFAULT_HASH=sha1 \
    GIT_TEST_INSTALLED="$shim_dir" \
    sh "$test_name" $test_flags
  ) >"$log" 2>&1
  rc=$?
  fi
  set -e
  if [[ "$rc" == "0" ]]; then
    passed=$((passed + 1))
    printf '%s\t%s\tpass\t%s\t%s\n' "$test_mode" "$test_name" "$reason" "$log" >>"$summary"
  else
    failed=$((failed + 1))
    printf '%s\t%s\tfail\t%s\t%s\n' "$test_mode" "$test_name" "$reason" "$log" >>"$summary"
    tail -n 40 "$log" >&2 || true
  fi
done < <(selected_tests)

printf 'upstream_git_tag=%s\n' "$tag"
printf 'mode=%s\n' "$mode"
printf 'total=%s\n' "$total"
printf 'passed=%s\n' "$passed"
printf 'failed=%s\n' "$failed"
printf 'summary=%s\n' "$summary"

if [[ "$failed" != "0" && "${ZMIN_UPSTREAM_ALLOW_FAILURES:-0}" != "1" ]]; then
  exit 1
fi
