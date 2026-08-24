#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
usage: tools/git-upstream-http-provenance.sh build|validate|validate-source [canonical|with-http-fetch-pinned]

Builds or validates an explicitly selected offline Git HTTP comparator bundle
profile. The source tree, archive, build tools, and bundle are selected by
exact validated identities; no network or ambient helper fallback is used.
EOF
}

mode="${1:-}"
case "$mode" in
  build|validate|validate-source) ;;
  -h|--help) usage; exit 0 ;;
  *) usage; exit 2 ;;
esac

if [[ "$#" -gt 2 ]]; then
  usage
  exit 2
fi
profile="${2:-${ZMIN_HTTP_BUNDLE_PROFILE:-canonical}}"
case "$profile" in
  canonical|with-http-fetch-pinned) ;;
  *)
    echo "unsupported HTTP comparator bundle profile: $profile" >&2
    exit 2
    ;;
esac

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
cache_input="${ZMIN_UPSTREAM_GIT_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/zmin/git-upstream}"
cache_root="$(cd "$cache_input" 2>/dev/null && pwd -P)" || {
  echo "upstream cache root is unavailable: $cache_input" >&2
  exit 2
}
[[ -d "$cache_root" && ! -L "$cache_root" ]] || {
  echo "upstream cache root is not a real directory: $cache_root" >&2
  exit 2
}
contract="$repo_root/tools/git-upstream-compat-contract.tsv"

contract_value() {
  awk -F '\t' -v key="$1" '$1 == key { print $2; count += 1 } END { if (count != 1) exit 1 }' "$contract"
}

tag="$(contract_value upstream_git_tag)"
commit="$(contract_value upstream_git_commit)"
archive_sha="$(contract_value upstream_archive_sha256)"
source_policy="$(contract_value source_identity_policy)"
[[ "$tag" == v2.55.0 && "$commit" == e9019fcafe0040228b8631c30f97ae1adb61bcdc &&
  "$archive_sha" == 72923418db7b26dfddc21e2268660c5118e560bdfaa09b4489b67b38e9b69c49 &&
  "$source_policy" == 'archive_sha256_exact; tag_commit_declared; source_manifest_exact; no_checkout_fallback' ]] || {
  echo "upstream contract identity is not the frozen Git v2.55.0 contract" >&2
  exit 1
}

# The canonical input is the archive-derived pristine tree only. No checkout
# path is accepted here; a future checkout input must prove HEAD==$commit
# before it can participate in this source identity policy.
source_root="$cache_root/git-v2.55.0"
archive="$cache_root/v2.55.0.tar.gz"
marker="$source_root/.zmin-pristine-source.sha256"

if [[ "$profile" == with-http-fetch-pinned ]]; then
  for injected_perl_env in ZMIN_HTTP_PERL PERL5LIB PERL5OPT PERL_LOCAL_LIB_ROOT PERL_MB_OPT PERL_MM_OPT PERL_USE_UNSAFE_INC; do
    [[ -z "${!injected_perl_env+x}" ]] || {
      echo "with-http-fetch-pinned rejects the unbound Perl environment variable: $injected_perl_env" >&2
      exit 2
    }
  done
  parser_perl=/usr/bin/perl
else
  parser_perl="${ZMIN_HTTP_PERL:-$(command -v perl 2>/dev/null || true)}"
fi
[[ "$parser_perl" == /* && -f "$parser_perl" && ! -L "$parser_perl" && -x "$parser_perl" ]] || {
  echo "ZMIN_HTTP_PERL must select an absolute regular Perl executable" >&2
  exit 2
}
parser_perl="$(cd "$(dirname "$parser_perl")" && pwd -P)/$(basename "$parser_perl")"
if ! "$parser_perl" -MDigest::SHA -e 'Digest::SHA::sha256_hex("")' >/dev/null 2>&1; then
  echo "ZMIN_HTTP_PERL must provide Digest::SHA" >&2
  exit 2
fi

member_sha() {
  "$parser_perl" -MDigest::SHA -e '
    use strict;
    use warnings;
    my ($path) = @ARGV;
    open my $in, "<", $path or die "cannot read $path: $!\n";
    binmode $in;
    my $sha = Digest::SHA->new(256);
    my $buffer;
    while (read($in, $buffer, 1024 * 1024)) { $sha->add($buffer); }
    close $in or die "cannot close $path: $!\n";
    print $sha->hexdigest, "\n";
  ' "$1"
}

trusted_helper_sha256() {
  local path="$1"
  [[ -f /usr/bin/shasum && ! -L /usr/bin/shasum && -x /usr/bin/shasum ]] || {
    echo "pinned helper reporting requires the frozen /usr/bin/shasum" >&2
    return 1
  }
  /usr/bin/shasum -a 256 "$path" | /usr/bin/awk '{ print $1 }'
}

write_source_manifest() {
  local root="$1" output="$2"
  "$parser_perl" -MFile::Find -MDigest::SHA -e '
    use strict;
    use warnings;
    my ($root, $output) = @ARGV;
    my %excluded = (
      "./.zmin-pristine-source.sha256" => 1,
      "./.zmin-test-tool.provenance.tsv" => 1,
    );
    my @paths;
    find({ no_chdir => 1, follow => 0, wanted => sub { push @paths, $File::Find::name } }, $root);
    @paths = sort @paths;
    open my $out, ">", $output or die "cannot write source manifest: $!\n";
    binmode $out;
    for my $path (@paths) {
      (my $relative = $path) =~ s/^\Q$root\E\/?//;
      next if $relative eq "";
      $relative = "./$relative";
      next if $excluded{$relative};
      if (-l $path) {
        my $target = readlink $path;
        die "cannot read symlink $path: $!\n" unless defined $target;
        print {$out} "L\t$relative\t$target\n";
      } elsif (-f $path) {
        open my $in, "<", $path or die "cannot read $path: $!\n";
        binmode $in;
        if ($relative eq "./t/test-lib.sh") {
          local $/;
          my $data = <$in>;
          $data =~ s{
\n\tif test -n "\$ZMIN_UPSTREAM_STOP_AFTER_TEST" &&
\t   test "\$test_count" -ge "\$ZMIN_UPSTREAM_STOP_AFTER_TEST"
\tthen
\t\ttest_done
\tfi
}{\n}g;
          $data =~ s/\*MINGW\*\|\*MSYS\*\)/\*MINGW\*\)/g;
          $data =~ s/GIT_TEST_CMP="GIT_DIR=\/dev\/null git diff --no-index --ignore-cr-at-eol --"/GIT_TEST_CMP="diff -u"/g;
          $data =~ s/GIT_TEST_CMP="\$DIFF -u"/GIT_TEST_CMP="diff -u"/g;
          $data =~ s/GIT_TEST_CMP=" +-u"/GIT_TEST_CMP="diff -u"/g;
          print {$out} "F\t$relative\t", Digest::SHA::sha256_hex($data), "\n";
          close $in or die "cannot close $path: $!\n";
          next;
        }
        my $sha = Digest::SHA->new(256);
        my $buffer;
        while (read($in, $buffer, 1024 * 1024)) { $sha->add($buffer); }
        close $in or die "cannot close $path: $!\n";
        print {$out} "F\t$relative\t", $sha->hexdigest, "\n";
      } elsif (-d $path) {
        print {$out} "D\t$relative\n";
      } else {
        die "unsupported source entry: $path\n";
      }
    }
    close $out or die "cannot close source manifest: $!\n";
  ' "$root" "$output"
}

source_manifest_sha256() {
  local root="$1" manifest digest
  manifest="$(mktemp "$cache_root/.zmin-http-source-manifest.XXXXXX")"
  write_source_manifest "$root" "$manifest"
  digest="$(member_sha "$manifest")"
  rm -f "$manifest"
  printf '%s\n' "$digest"
}

fail_source() {
  echo "validated pristine Git v2.55.0 source root is incomplete or mutable: $source_root" >&2
  return 1
}

validate_source() {
  [[ "$source_root" == "$cache_root/git-v2.55.0" && -d "$source_root" && ! -L "$source_root" &&
    -d "$source_root/Documentation" && -f "$source_root/command-list.txt" &&
    -f "$source_root/GIT-VERSION-GEN" && -f "$marker" && ! -L "$marker" ]] || fail_source
  [[ ! -w "$source_root" && -z "$(find "$source_root" \( -type f -o -type d \) -perm -u+w -print -quit)" ]] || {
    echo "validated pristine Git source root is writable: $source_root" >&2
    return 1
  }
  [[ -f "$archive" && ! -L "$archive" ]] || {
    echo "validated Git v2.55.0 archive is missing or mutable: $archive" >&2
    return 1
  }
  [[ "$(member_sha "$archive")" == "$archive_sha" ]] || {
    echo "validated Git archive checksum does not match the frozen archive" >&2
    return 1
  }
  [[ "$(tr -d '[:space:]' <"$marker")" == "$archive_sha" ]] || {
    echo "validated Git source marker does not match the frozen archive" >&2
    return 1
  }
  grep -qx 'DEF_VER=v2.55.0' "$source_root/GIT-VERSION-GEN" || {
    echo "validated Git source version is not v2.55.0" >&2
    return 1
  }
  source_manifest_value="$(source_manifest_sha256 "$source_root")" || return 1
  archive_extract="$(mktemp -d "$cache_root/.zmin-http-archive.XXXXXX")"
  trap 'rm -rf "$archive_extract"' RETURN
  tar -xzf "$archive" -C "$archive_extract"
  [[ -d "$archive_extract/git-2.55.0" && ! -L "$archive_extract/git-2.55.0" ]] || {
    echo "frozen Git archive has an unexpected source root" >&2
    return 1
  }
  archive_manifest_value="$(source_manifest_sha256 "$archive_extract/git-2.55.0")"
  [[ "$source_manifest_value" == "$archive_manifest_value" ]] || {
    echo "validated pristine source manifest mismatch" >&2
    return 1
  }
  trap - RETURN
  rm -rf "$archive_extract"
}

platform="$(uname -s)"
arch="$(uname -m)"
exe_suffix=""
case "${RUNNER_OS:-${OS:-}}:$platform" in
  Windows*:*|*:MINGW*|*:MSYS*|*:CYGWIN*) exe_suffix=".exe" ;;
esac
git_relative="git$exe_suffix"
remote_relative="git-remote-http$exe_suffix"
backend_relative="git-http-backend$exe_suffix"
fetch_relative="git-http-fetch$exe_suffix"
build_flags='NO_GETTEXT=YesPlease NO_PERL=YesPlease NO_PYTHON=YesPlease NO_REGEX=YesPlease'
deterministic_build_prefix=/usr/src/git-v2.55.0
case "$profile" in
  canonical)
    manifest_version=1
    bundle_role=canonical_git_v2_55_http_bundle
    bundle_name="http-bundle-$tag-$commit-$platform-$arch"
    ;;
  with-http-fetch-pinned)
    [[ "$platform" == Darwin && "$arch" == arm64 ]] || {
      echo "with-http-fetch-pinned is frozen for Darwin arm64 only" >&2
      exit 2
    }
    manifest_version=3
    bundle_role=pinned_git_v2.55_http_bundle_with_source_built_http_fetch
    bundle_name="http-bundle-$tag-$commit-$platform-$arch-with-http-fetch-pinned"
    expected_fetch_sha=fc2e8b9e47cafb39140ca90f56fbc6cb09912c6d715feb020157733868f08b0e
    expected_macho_uuid=119DE956-A0F3-EFE0-6530-CF7B0E065587
    expected_link_dependencies=$'/System/Library/Frameworks/CoreServices.framework/Versions/A/CoreServices\n/usr/lib/libcurl.4.dylib\n/usr/lib/libz.1.dylib\n/usr/lib/libiconv.2.dylib\n/usr/lib/libSystem.B.dylib'
    deterministic_build_command='/usr/bin/make CC=/usr/bin/cc AR=/usr/bin/ar RANLIB=/usr/bin/ranlib CFLAGS=-ffile-prefix-map=<build-root>=/usr/src/git-v2.55.0 -fdebug-prefix-map=<build-root>=/usr/src/git-v2.55.0 LDFLAGS=-Wl,-no_uuid,-headerpad_max_install_names RUSTFLAGS=--remap-path-prefix=<build-root>=/usr/src/git-v2.55.0 SOURCE_DATE_EPOCH=0 ZERO_AR_DATE=1 NO_GETTEXT=YesPlease NO_PERL=YesPlease NO_PYTHON=YesPlease NO_REGEX=YesPlease -j2 V=1 git-http-fetch'
    ;;
esac
bundle="$cache_root/$bundle_name"
case "$profile" in
  canonical)
    manifest_name="bundle.tsv"
    manifest_sidecar_name="bundle.tsv.sha256"
    ;;
  with-http-fetch-pinned)
    manifest_name="manifest.tsv"
    manifest_sidecar_name="manifest.tsv.sha256"
    ;;
esac
# bundle.tsv and its checksum provide cache-local integrity and reproducibility
# under the trusted local-user/cache model; they are not an authenticity or
# security boundary against a same-user rewrite of binaries and sidecars.
lock_dir="$cache_root/.zmin-http-bundle.lock"
lock_token=""
lock_held=0
build_cleanup_root=""
build_cleanup_tmp=""

manifest_field() {
  local manifest="$1" key="$2"
  awk -F '\t' -v key="$key" '$1 == key { print $2; count += 1 } END { if (count != 1) exit 1 }' "$manifest"
}

member_manifest_sha() {
  local manifest="$1" name="$2"
  if [[ "$profile" == canonical ]]; then
    awk -F '\t' -v name="$name" '$1 == "member" && $2 == name { print $4; count += 1 } END { if (count != 1) exit 1 }' "$manifest"
  else
    awk -F '\t' -v name="$name" '$1 == "member" && $2 == name { print $5; count += 1 } END { if (count != 1) exit 1 }' "$manifest"
  fi
}

member_manifest_mode() {
  local manifest="$1" name="$2"
  awk -F '\t' -v name="$name" '$1 == "member" && $2 == name { print (NF == 5 ? $3 : ""); count += 1 } END { if (count != 1) exit 1 }' "$manifest"
}

member_manifest_size() {
  local manifest="$1" name="$2"
  awk -F '\t' -v name="$name" '$1 == "member" && $2 == name { print (NF == 5 ? $4 : ""); count += 1 } END { if (count != 1) exit 1 }' "$manifest"
}

regular_executable() {
  local path="$1"
  [[ -f "$path" && ! -L "$path" && -x "$path" && ! -w "$path" ]]
}

build_executable() {
  local path="$1"
  [[ -f "$path" && ! -L "$path" && -x "$path" ]]
}

add_deterministic_macho_uuid() {
  local path="$1"
  "$parser_perl" -e '
    use strict;
    use warnings;
    my ($path, $uuid_hex) = @ARGV;
    open my $in, "<", $path or die "cannot read $path: $!\n";
    binmode $in;
    local $/;
    my $data = <$in>;
    close $in or die "cannot close $path: $!\n";
    die "not a 64-bit little-endian Mach-O file\n"
      unless unpack("L<", substr($data, 0, 4)) == 0xfeedfacf;
    my $ncmds = unpack("L<", substr($data, 16, 4));
    my $sizeofcmds = unpack("L<", substr($data, 20, 4));
    my $offset = 32 + $sizeofcmds;
    die "Mach-O header padding is insufficient for deterministic UUID\n"
      unless $offset + 24 <= length($data);
    my $uuid = pack("H*", $uuid_hex);
    die "invalid deterministic UUID\n" unless length($uuid) == 16;
    substr($data, $offset, 24) = pack("L<2", 0x1b, 24) . $uuid;
    substr($data, 16, 4) = pack("L<", $ncmds + 1);
    substr($data, 20, 4) = pack("L<", $sizeofcmds + 24);
    open my $out, ">", $path or die "cannot write $path: $!\n";
    binmode $out;
    print {$out} $data;
    close $out or die "cannot close $path: $!\n";
  ' "$path" "${expected_macho_uuid//-/}"
}

validate_pinned_macho() {
  local path="$1" manifest="$2" otool_path=/usr/bin/otool actual_deps manifest_deps actual_commands actual_dyld
  [[ -f "$otool_path" && ! -L "$otool_path" && -x "$otool_path" ]] || {
    echo "pinned git-http-fetch requires the frozen /usr/bin/otool" >&2
    return 1
  }
  manifest_deps="$(awk -F '\t' '$1 == "link_dependency" { print $2 }' "$manifest")"
  [[ "$manifest_deps" == "$expected_link_dependencies" ]] || {
    echo "pinned git-http-fetch manifest has unexpected link dependencies" >&2
    return 1
  }
  actual_deps="$("$otool_path" -L "$path" | awk 'NR > 1 { print $1 }')" || {
    echo "pinned git-http-fetch load dependency inspection failed" >&2
    return 1
  }
  [[ "$actual_deps" == "$expected_link_dependencies" ]] || {
    echo "pinned git-http-fetch has unexpected Mach-O link dependencies" >&2
    return 1
  }
  actual_dyld="$("$otool_path" -l "$path" | awk '/cmd LC_LOAD_DYLINKER/ { found = 1; next } found && $1 == "name" { print $2; exit }')"
  [[ "$actual_dyld" == /usr/lib/dyld ]] || {
    echo "pinned git-http-fetch has an unexpected Mach-O dynamic linker" >&2
    return 1
  }
  actual_commands="$("$otool_path" -l "$path" | awk '$1 == "cmd" { print $2 }')" || {
    echo "pinned git-http-fetch load-command inspection failed" >&2
    return 1
  }
  [[ "$actual_commands" == $'LC_SEGMENT_64\nLC_SEGMENT_64\nLC_SEGMENT_64\nLC_SEGMENT_64\nLC_SEGMENT_64\nLC_DYLD_CHAINED_FIXUPS\nLC_DYLD_EXPORTS_TRIE\nLC_SYMTAB\nLC_DYSYMTAB\nLC_LOAD_DYLINKER\nLC_BUILD_VERSION\nLC_SOURCE_VERSION\nLC_MAIN\nLC_LOAD_DYLIB\nLC_LOAD_DYLIB\nLC_LOAD_DYLIB\nLC_LOAD_DYLIB\nLC_LOAD_DYLIB\nLC_FUNCTION_STARTS\nLC_DATA_IN_CODE\nLC_CODE_SIGNATURE\nLC_UUID' ]] || {
    echo "pinned git-http-fetch has unexpected Mach-O load commands" >&2
    return 1
  }
  [[ "$("$otool_path" -l "$path" | awk '/cmd LC_UUID/ { found = 1; next } found && $1 == "uuid" { print $2; exit }')" == "$expected_macho_uuid" ]] || {
    echo "pinned git-http-fetch has an unexpected deterministic Mach-O UUID" >&2
    return 1
  }
}

validate_canonical_manifest_shape() {
  local manifest="$1"
  [[ "$(wc -l <"$manifest" | tr -d '[:space:]')" == 12 ]] || return 1
  awk -F '\t' -v tag="$tag" -v commit="$commit" -v archive_sha="$archive_sha" \
    -v source_sha="$source_manifest_value" -v platform="$platform" -v arch="$arch" \
    -v suffix="$exe_suffix" '
    function sha(v) { return length(v) == 64 && v ~ /^[0-9a-f]+$/ }
    NR == 1 { ok = ($1 == "schema_version" && NF == 2 && $2 == "1"); next }
    NR == 2 { ok = ok && ($1 == "upstream_git_tag" && NF == 2 && $2 == tag); next }
    NR == 3 { ok = ok && ($1 == "upstream_git_commit" && NF == 2 && $2 == commit); next }
    NR == 4 { ok = ok && ($1 == "source_marker_sha256" && NF == 2 && $2 == archive_sha); next }
    NR == 5 { ok = ok && ($1 == "source_manifest_sha256" && NF == 2 && $2 == source_sha && sha($2)); next }
    NR == 6 { ok = ok && ($1 == "build_flags" && NF == 2 && $2 == "NO_GETTEXT=YesPlease NO_PERL=YesPlease NO_PYTHON=YesPlease NO_REGEX=YesPlease"); next }
    NR == 7 { ok = ok && ($1 == "platform" && NF == 2 && $2 == platform); next }
    NR == 8 { ok = ok && ($1 == "arch" && NF == 2 && $2 == arch); next }
    NR == 9 { ok = ok && ($1 == "template_dir" && NF == 2 && $2 == "templates"); next }
    NR == 10 { ok = ok && ($1 == "member" && NF == 4 && $2 == "git" && $3 == "git" suffix && sha($4)); next }
    NR == 11 { ok = ok && ($1 == "member" && NF == 4 && $2 == "git-remote-http" && $3 == "git-remote-http" suffix && sha($4)); next }
    NR == 12 { ok = ok && ($1 == "member" && NF == 4 && $2 == "git-http-backend" && $3 == "git-http-backend" suffix && sha($4)); next }
    { ok = 0 }
    END { exit !ok }
  ' "$manifest"
}

validate_pinned_manifest_shape() {
  local manifest="$1"
  [[ "$(wc -l <"$manifest" | tr -d '[:space:]')" == 35 ]] || return 1
  [[ "$(manifest_field "$manifest" manifest_version)" == 3 ]] || return 1
  [[ "$(manifest_field "$manifest" bundle_role)" == "$bundle_role" ]] || return 1
  [[ "$(manifest_field "$manifest" upstream_git_tag)" == "$tag" ]] || return 1
  [[ "$(manifest_field "$manifest" upstream_git_commit)" == "$commit" ]] || return 1
  [[ "$(manifest_field "$manifest" upstream_archive_sha256)" == "$archive_sha" ]] || return 1
  [[ "$(manifest_field "$manifest" source_root)" == "$source_root" ]] || return 1
  [[ "$(manifest_field "$manifest" source_marker_sha256)" == "$archive_sha" ]] || return 1
  [[ "$(manifest_field "$manifest" source_manifest_sha256)" == "$source_manifest_value" ]] || return 1
  [[ "$(manifest_field "$manifest" source_commit_binding)" == 'archive_sha256+tag+DEF_VER;commit_not_embedded_in_archive' ]] || return 1
  [[ "$(manifest_field "$manifest" build_host)" == 'Darwin arm64' ]] || return 1
  [[ "$(manifest_field "$manifest" build_flags)" == "$build_flags" ]] || return 1
  [[ "$(manifest_field "$manifest" build_command)" == "$deterministic_build_command" ]] || return 1
  [[ "$(awk -F '\t' '$1 == "toolchain" { count += 1 } END { print count + 0 }' "$manifest")" == 5 ]] || return 1
  [[ "$(awk -F '\t' '$1 == "toolchain" && $2 == "/usr/bin/make" && $3 == "0755" && $4 == "118928" && $5 == "179301dcb41ea78accc3fa0048a7e6f6710d891945a751a34addd622020c1818" { count += 1 } END { print count + 0 }' "$manifest")" == 1 ]] || return 1
  [[ "$(awk -F '\t' '$1 == "toolchain" && $2 == "/usr/bin/cc" && $3 == "0755" && $4 == "118928" && $5 == "179301dcb41ea78accc3fa0048a7e6f6710d891945a751a34addd622020c1818" { count += 1 } END { print count + 0 }' "$manifest")" == 1 ]] || return 1
  [[ "$(awk -F '\t' '$1 == "toolchain" && $2 == "/usr/bin/ar" && $3 == "0755" && $4 == "118928" && $5 == "179301dcb41ea78accc3fa0048a7e6f6710d891945a751a34addd622020c1818" { count += 1 } END { print count + 0 }' "$manifest")" == 1 ]] || return 1
  [[ "$(awk -F '\t' '$1 == "toolchain" && $2 == "/usr/bin/ranlib" && $3 == "0755" && $4 == "118928" && $5 == "179301dcb41ea78accc3fa0048a7e6f6710d891945a751a34addd622020c1818" { count += 1 } END { print count + 0 }' "$manifest")" == 1 ]] || return 1
  [[ "$(awk -F '\t' '$1 == "toolchain" && $2 == "/usr/bin/codesign" && $3 == "0755" && $4 == 459824 && $5 == "214d455584d19abc0d74d02b9cbc7d3da6bdcb0596c235e6156dd9ed2f4e1ba7" { count += 1 } END { print count + 0 }' "$manifest")" == 1 ]] || return 1
  [[ "$(awk -F '\t' '$1 == "toolchain_version" && $2 == "/usr/bin/cc" && $3 == "Apple clang version 21.0.0 (clang-2100.1.1.101)" { count += 1 } END { print count + 0 }' "$manifest")" == 1 ]] || return 1
  [[ "$(awk -F '\t' '$1 == "build_flags_file" && $2 == "GIT-CFLAGS" && $3 == 815 && $4 == "0606987c5c01e29468644fa30f204d40c6cfd2318951056a4dd8c619b9d694df" { count += 1 } END { print count + 0 }' "$manifest")" == 1 ]] || return 1
  [[ "$(awk -F '\t' '$1 == "build_flags_file" && $2 == "GIT-LDFLAGS" && $3 == 101 && $4 == "fa2706bb59ca20b52d32adeda65b75f9ed37e0d78db80e8160151b25e40eb6ee" { count += 1 } END { print count + 0 }' "$manifest")" == 1 ]] || return 1
  [[ "$(awk -F '\t' '$1 == "source_member" && $2 == "http-fetch.c" && $3 == 4755 && $4 == "39e3148ce613b9bde0a3d7d8114145a028b354f29bb562f5cd30ffec1c36e8c3" { count += 1 } END { print count + 0 }' "$manifest")" == 1 ]] || return 1
  [[ "$(awk -F '\t' '$1 == "source_member" && $2 == "http.c" && $3 == 83261 && $4 == "e3cb7bde58647c6d982a7400f3d27af8bc89a5bd6be1b6921f460ade165005ab" { count += 1 } END { print count + 0 }' "$manifest")" == 1 ]] || return 1
  [[ "$(awk -F '\t' '$1 == "source_member" && $2 == "http-walker.c" && $3 == 15564 && $4 == "e30794ca97d95bf3d2a18894fa2498d4a12bbe46d2d36a482125f191808c9795" { count += 1 } END { print count + 0 }' "$manifest")" == 1 ]] || return 1
  [[ "$(awk -F '\t' '$1 == "source_member" && $2 == "common-main.c" && $3 == 236 && $4 == "498db0a58a4a996c1c00e491c03977d459d41c4fc7c16b3a51c3e7f358977040" { count += 1 } END { print count + 0 }' "$manifest")" == 1 ]] || return 1
  [[ "$(awk -F '\t' '$1 == "link_dependency" { count += 1 } END { print count + 0 }' "$manifest")" == 5 ]] || return 1
  [[ "$(awk -F '\t' '$1 == "link_dependency" && $2 == "/System/Library/Frameworks/CoreServices.framework/Versions/A/CoreServices" { count += 1 } END { print count + 0 }' "$manifest")" == 1 ]] || return 1
  [[ "$(awk -F '\t' '$1 == "link_dependency" && $2 == "/usr/lib/libcurl.4.dylib" { count += 1 } END { print count + 0 }' "$manifest")" == 1 ]] || return 1
  [[ "$(awk -F '\t' '$1 == "link_dependency" && $2 == "/usr/lib/libz.1.dylib" { count += 1 } END { print count + 0 }' "$manifest")" == 1 ]] || return 1
  [[ "$(awk -F '\t' '$1 == "link_dependency" && $2 == "/usr/lib/libiconv.2.dylib" { count += 1 } END { print count + 0 }' "$manifest")" == 1 ]] || return 1
  [[ "$(awk -F '\t' '$1 == "link_dependency" && $2 == "/usr/lib/libSystem.B.dylib" { count += 1 } END { print count + 0 }' "$manifest")" == 1 ]] || return 1
  [[ "$(awk -F '\t' '$1 == "member" { count += 1 } END { print count + 0 }' "$manifest")" == 5 ]] || return 1
  [[ "$(awk -F '\t' '$1 == "member" && $2 == "bundle.tsv" && $3 == "0444" && $4 == 707 && $5 == "cc295dc42051d204e2505acfab55d43767260fdd0cb016c6e5d3f507cf74bfdb" { count += 1 } END { print count + 0 }' "$manifest")" == 1 ]] || return 1
  [[ "$(awk -F '\t' '$1 == "member" && $2 == "git" && $3 == "0555" && $4 == 4493136 && $5 == "ca63eda87df1aaffa2b80710c4a9de6212eba6c84e8dfb3011a2498b36e841cb" { count += 1 } END { print count + 0 }' "$manifest")" == 1 ]] || return 1
  [[ "$(awk -F '\t' '$1 == "member" && $2 == "git-remote-http" && $3 == "0555" && $4 == 2735808 && $5 == "6ba041e1c71c11eb3d5f66579ea29734135446a4575ec3ecf0574437b795b640" { count += 1 } END { print count + 0 }' "$manifest")" == 1 ]] || return 1
  [[ "$(awk -F '\t' '$1 == "member" && $2 == "git-http-backend" && $3 == "0555" && $4 == 2649984 && $5 == "558316c4ea88b9e50327def4dc234c7404e11790ab77da1c2a6bf52879bd1b43" { count += 1 } END { print count + 0 }' "$manifest")" == 1 ]] || return 1
  [[ "$(awk -F '\t' '$1 == "member" && $2 == "git-http-fetch" && $3 == "0555" && $4 == 3310192 && $5 ~ /^[0-9a-f]{64}$/ { count += 1 } END { print count + 0 }' "$manifest")" == 1 ]] || return 1
  helper_manifest_sha="$(member_manifest_sha "$manifest" git-http-fetch)"
  [[ "$helper_manifest_sha" =~ ^[0-9a-f]{64}$ ]] || return 1
  [[ "$(awk -F '\t' '$1 == "template_dir" && $2 == "templates" && $3 == "0555" && $4 == "empty" { count += 1 } END { print count + 0 }' "$manifest")" == 1 ]] || return 1
}

validate_manifest_shape() {
  case "$profile" in
    canonical) validate_canonical_manifest_shape "$1" ;;
    with-http-fetch-pinned) validate_pinned_manifest_shape "$1" ;;
  esac
}

validate_pinned_rebuild() (
  local published="$1" verify_root verify_build verify_helper
  local verify_cflags verify_rustflags verify_ldflags verify_tool
  for verify_tool in /usr/bin/make /usr/bin/cc /usr/bin/ar /usr/bin/ranlib /usr/bin/codesign; do
    [[ -f "$verify_tool" && ! -L "$verify_tool" && -x "$verify_tool" ]] || {
      echo "pinned helper rebuild requires the frozen tool: $verify_tool" >&2
      return 1
    }
  done
  verify_root="$(mktemp -d "$cache_root/.zmin-http-fetch-verify.XXXXXX")"
  trap 'chmod -R u+w "$verify_root" 2>/dev/null || :; rm -rf "$verify_root"' EXIT
  verify_build="$verify_root/build"
  mkdir "$verify_build"
  cp -R "$source_root/." "$verify_build/"
  chmod -R u+w "$verify_build"
  verify_cflags="-ffile-prefix-map=$verify_build=$deterministic_build_prefix -fdebug-prefix-map=$verify_build=$deterministic_build_prefix"
  verify_rustflags="--remap-path-prefix=$verify_build=$deterministic_build_prefix"
  verify_ldflags='-Wl,-no_uuid,-headerpad_max_install_names'
  (cd "$verify_build" && SOURCE_DATE_EPOCH=0 ZERO_AR_DATE=1 RUSTFLAGS="$verify_rustflags" \
    /usr/bin/make CC=/usr/bin/cc AR=/usr/bin/ar RANLIB=/usr/bin/ranlib CFLAGS="$verify_cflags" \
    LDFLAGS="$verify_ldflags" $build_flags -j2 V=1 git-http-fetch) >&2
  verify_helper="$verify_build/$fetch_relative"
  build_executable "$verify_helper" || {
    echo "pinned helper rebuild did not produce $fetch_relative" >&2
    return 1
  }
  add_deterministic_macho_uuid "$verify_helper"
  /usr/bin/codesign --force --sign - --timestamp=none "$verify_helper" >/dev/null 2>&1 || {
    echo "pinned helper rebuild could not receive its deterministic signature" >&2
    return 1
  }
  /usr/bin/cmp -s "$verify_helper" "$published" || {
    echo "pinned helper does not match a fresh deterministic source/toolchain rebuild" >&2
    return 1
  }
)

validate_bundle() {
  local root_input="${ZMIN_GIT_HTTP_BUNDLE:-}" git_input="${ZMIN_STOCK_GIT:-}"
  local expected_root="$bundle" root manifest sidecar git_path name relative path expected actual entry
  [[ "$root_input" == "$expected_root" && "$git_input" == "$expected_root/$git_relative" ]] || {
    echo "HTTP comparator paths are not the canonical derived bundle paths" >&2
    return 1
  }
  [[ -d "$root_input" && ! -L "$root_input" ]] || {
    echo "HTTP comparator bundle directory is mutable or invalid" >&2
    return 1
  }
  root="$(cd "$root_input" && pwd -P)"
  [[ "$root" == "$expected_root" && ! -w "$root" ]] || {
    echo "HTTP comparator bundle directory is mutable or invalid" >&2
    return 1
  }
  manifest="$root/$manifest_name"
  sidecar="$root/$manifest_sidecar_name"
  git_path="$root/$git_relative"
  [[ -f "$manifest" && ! -L "$manifest" && ! -w "$manifest" &&
    -f "$sidecar" && ! -L "$sidecar" && ! -w "$sidecar" ]] || {
    echo "HTTP comparator bundle manifest is missing or mutable" >&2
    return 1
  }
  [[ "$(tr -d '[:space:]' <"$sidecar")" == "$(member_sha "$manifest")" ]] || {
    echo "HTTP comparator bundle manifest checksum mismatch" >&2
    return 1
  }
  validate_manifest_shape "$manifest" || {
    echo "HTTP comparator bundle manifest shape or identity mismatch" >&2
    return 1
  }
  while IFS= read -r entry; do
    case "$entry" in
      "$root"|"$root/$manifest_name"|"$root/$manifest_sidecar_name"|"$root/templates"|"$root/$git_relative"|"$root/$remote_relative"|"$root/$backend_relative") ;;
      "$root/bundle.tsv"|"$root/bundle.tsv.sha256")
        [[ "$profile" == with-http-fetch-pinned ]] || {
          echo "HTTP comparator bundle contains unexpected entry: ${entry#"$root/"}" >&2
          return 1
        }
        ;;
      "$root/$fetch_relative")
        [[ "$profile" == with-http-fetch-pinned ]] || {
          echo "HTTP comparator bundle contains unexpected entry: ${entry#"$root/"}" >&2
          return 1
        }
        ;;
      *) echo "HTTP comparator bundle contains unexpected entry: ${entry#"$root/"}" >&2; return 1 ;;
    esac
  done < <(find "$root" -mindepth 1 -print)
  [[ -d "$root/templates" && ! -L "$root/templates" && ! -w "$root/templates" &&
    -z "$(find "$root/templates" -mindepth 1 -print -prune)" ]] || {
    echo "HTTP comparator bundle templates are not the integrity-checked empty directory" >&2
    return 1
  }
  if [[ "$profile" == with-http-fetch-pinned ]]; then
    path="$root/bundle.tsv"
    [[ -f "$path" && ! -L "$path" && ! -w "$path" ]] || {
      echo "pinned HTTP comparator base manifest is missing or mutable" >&2
      return 1
    }
    [[ "$(member_manifest_sha "$manifest" bundle.tsv)" == "$(member_sha "$path")" ]] || {
      echo "pinned HTTP comparator base manifest checksum mismatch" >&2
      return 1
    }
    [[ "$(member_manifest_mode "$manifest" bundle.tsv)" == "$(stat -f '%04Lp' "$path")" &&
      "$(member_manifest_size "$manifest" bundle.tsv)" == "$(stat -f '%z' "$path")" ]] || {
      echo "pinned HTTP comparator base manifest mode or size mismatch" >&2
      return 1
    }
  fi
  names=(git git-remote-http git-http-backend)
  if [[ "$profile" == with-http-fetch-pinned ]]; then
    names+=(git-http-fetch)
  fi
  for name in "${names[@]}"; do
    case "$name" in
      git) relative="$git_relative" ;;
      git-remote-http) relative="$remote_relative" ;;
      git-http-backend) relative="$backend_relative" ;;
      git-http-fetch) relative="$fetch_relative" ;;
    esac
    path="$root/$relative"
    member_validator=regular_executable
    "$member_validator" "$path" || {
      echo "HTTP comparator bundle member is missing or mutable: $relative" >&2
      return 1
    }
    if [[ "$profile" != with-http-fetch-pinned || "$name" != git-http-fetch ]]; then
      expected="$(member_manifest_sha "$manifest" "$name")"
      actual="$(member_sha "$path")"
      [[ "$expected" == "$actual" ]] || {
        echo "HTTP comparator bundle member checksum mismatch: $relative" >&2
        return 1
      }
    fi
    if [[ "$profile" == with-http-fetch-pinned ]]; then
      expected_mode="$(member_manifest_mode "$manifest" "$name")"
      actual_mode="$(stat -f '%04Lp' "$path")"
      [[ "$expected_mode" == "$actual_mode" ]] || {
        echo "HTTP comparator bundle member mode mismatch: $relative" >&2
        return 1
      }
      expected_size="$(member_manifest_size "$manifest" "$name")"
      actual_size="$(stat -f '%z' "$path")"
      [[ "$expected_size" == "$actual_size" ]] || {
        echo "HTTP comparator bundle member size mismatch: $relative" >&2
        return 1
      }
    fi
  done
  if [[ "$profile" == with-http-fetch-pinned ]]; then
    manifest_helper_sha="$(member_manifest_sha "$manifest" git-http-fetch)"
    [[ "$manifest_helper_sha" == "$expected_fetch_sha" ]] || {
      echo "pinned git-http-fetch manifest digest is not the trusted source-built helper" >&2
      return 1
    }
    validate_pinned_macho "$root/$fetch_relative" "$manifest" || return 1
    validate_pinned_rebuild "$root/$fetch_relative" || return 1
    actual_helper_sha="$(trusted_helper_sha256 "$root/$fetch_relative")"
    [[ "$actual_helper_sha" == "$expected_fetch_sha" ]] || {
      echo "pinned git-http-fetch helper digest is not the trusted source-built helper" >&2
      return 1
    }
    dispatch_output=""
    dispatch_status=0
    set +e
    dispatch_output="$(GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null \
      GIT_EXEC_PATH="$root" GIT_TRACE=1 "$git_path" http-fetch -h 2>&1)"
    dispatch_status=$?
    set -e
    [[ "$dispatch_status" -ne 0 && "$dispatch_output" == *"start_command: $root/$fetch_relative -h"* &&
      "$dispatch_output" == *'usage: git http-fetch'* ]] || {
      echo "pinned Git dispatch did not invoke the bundled git-http-fetch" >&2
      return 1
    }
  fi
  [[ "$(GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null \
    GIT_EXEC_PATH="$root" "$git_path" --version 2>/dev/null)" == 'git version 2.55.0' ]] || {
    echo "HTTP comparator Git version mismatch" >&2
    return 1
  }
  [[ "$(GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null \
    GIT_EXEC_PATH="$root" "$git_path" --exec-path 2>/dev/null)" == "$root" ]] || {
    echo "HTTP comparator Git exec path is not the bundle" >&2
    return 1
  }
  printf '%s\n' "$root"
}

acquire_lock() {
  local timeout="${ZMIN_HTTP_LOCK_TIMEOUT_SECONDS:-60}" started now
  [[ "$timeout" =~ ^[0-9]+$ ]] || { echo "invalid HTTP bundle lock timeout" >&2; return 1; }
  started="$(date +%s)"
  while :; do
    if [[ -L "$lock_dir" || ( -e "$lock_dir" && ! -d "$lock_dir" ) ]]; then
      echo "HTTP comparator bundle lock path is invalid: $lock_dir" >&2
      return 1
    fi
    if mkdir "$lock_dir" 2>/dev/null; then
      lock_token="$$.$RANDOM.$(date +%s)"
      printf '%s\n' "$lock_token" >"$lock_dir/owner"
      lock_held=1
      return 0
    fi
    now="$(date +%s)"
    if (( now - started >= timeout )); then
      echo "timed out waiting for HTTP comparator bundle lock: $lock_dir" >&2
      return 1
    fi
    sleep 1
  done
}

release_lock() {
  if [[ "$lock_held" == 1 && -f "$lock_dir/owner" && ! -L "$lock_dir/owner" &&
    "$(cat "$lock_dir/owner")" == "$lock_token" ]]; then
    rm -f "$lock_dir/owner"
    rmdir "$lock_dir" 2>/dev/null || :
  fi
  lock_held=0
}

build_canonical_bundle() {
  local make_path="${ZMIN_HTTP_MAKE:-}" make_input build_root bundle_tmp relative name
  [[ "$make_path" == /* && -f "$make_path" && ! -L "$make_path" && -x "$make_path" ]] || {
    echo "ZMIN_HTTP_MAKE must select an absolute regular executable" >&2
    return 1
  }
  make_input="$make_path"
  make_path="$(cd "$(dirname "$make_path")" && pwd -P)/$(basename "$make_path")"
  [[ "$make_path" == "$make_input" ]] || {
    echo "ZMIN_HTTP_MAKE must not contain symlinked path components" >&2
    return 1
  }
  validate_source
  acquire_lock
  trap release_lock RETURN
  validate_source
  if [[ -e "$bundle" || -L "$bundle" ]]; then
    [[ -d "$bundle" && ! -L "$bundle" ]] || {
      echo "canonical HTTP comparator bundle path is occupied by a non-directory" >&2
      return 1
    }
    ZMIN_GIT_HTTP_BUNDLE="$bundle" ZMIN_STOCK_GIT="$bundle/$git_relative" validate_bundle >/dev/null
    printf '%s\n' "$bundle"
    return 0
  fi
  build_root="$(mktemp -d "$cache_root/.zmin-http-build.XXXXXX")"
  bundle_tmp="$(mktemp -d "$cache_root/.zmin-http-bundle.XXXXXX")"
  build_cleanup_root="$build_root"
  build_cleanup_tmp="$bundle_tmp"
  trap 'release_lock; chmod -R u+w "$build_cleanup_root" "$build_cleanup_tmp" 2>/dev/null || :; rm -rf "$build_cleanup_root" "$build_cleanup_tmp"' RETURN
  cp -R "$source_root/." "$build_root/"
  chmod -R u+w "$build_root"
  (cd "$build_root" && "$make_path" NO_GETTEXT=YesPlease NO_PERL=YesPlease NO_PYTHON=YesPlease NO_REGEX=YesPlease \
    -j"${ZMIN_HTTP_BUILD_JOBS:-2}" git git-remote-http git-http-backend)
  for name in git git-remote-http git-http-backend; do
    case "$name" in
      git) relative="$git_relative" ;;
      git-remote-http) relative="$remote_relative" ;;
      git-http-backend) relative="$backend_relative" ;;
    esac
    build_executable "$build_root/$relative" || {
      echo "offline Git build did not produce $name" >&2
      return 1
    }
    cp "$build_root/$relative" "$bundle_tmp/$relative"
  done
  mkdir "$bundle_tmp/templates"
  {
    printf 'schema_version\t1\n'
    printf 'upstream_git_tag\t%s\n' "$tag"
    printf 'upstream_git_commit\t%s\n' "$commit"
    printf 'source_marker_sha256\t%s\n' "$archive_sha"
    printf 'source_manifest_sha256\t%s\n' "$source_manifest_value"
    printf 'build_flags\tNO_GETTEXT=YesPlease NO_PERL=YesPlease NO_PYTHON=YesPlease NO_REGEX=YesPlease\n'
    printf 'platform\t%s\n' "$platform"
    printf 'arch\t%s\n' "$arch"
    printf 'template_dir\ttemplates\n'
    for name in git git-remote-http git-http-backend; do
      case "$name" in
        git) relative="$git_relative" ;;
        git-remote-http) relative="$remote_relative" ;;
        git-http-backend) relative="$backend_relative" ;;
      esac
      printf 'member\t%s\t%s\t%s\n' "$name" "$relative" "$(member_sha "$bundle_tmp/$relative")"
    done
  } >"$bundle_tmp/$manifest_name"
  chmod a-w "$bundle_tmp/$manifest_name"
  member_sha "$bundle_tmp/$manifest_name" >"$bundle_tmp/$manifest_sidecar_name"
  chmod a-w "$bundle_tmp/$manifest_sidecar_name"
  chmod -R a-w "$bundle_tmp"
  [[ ! -e "$bundle" && ! -L "$bundle" ]] || {
    echo "canonical HTTP comparator bundle appeared during publication" >&2
    return 1
  }
  mv "$bundle_tmp" "$bundle"
  bundle_tmp=""
  build_cleanup_tmp=""
  ZMIN_GIT_HTTP_BUNDLE="$bundle" ZMIN_STOCK_GIT="$bundle/$git_relative" validate_bundle >/dev/null
  printf '%s\n' "$bundle"
}

build_pinned_http_fetch_bundle() {
  local make_path="${ZMIN_HTTP_MAKE:-}" cc_path="${ZMIN_HTTP_CC:-}" ar_path="${ZMIN_HTTP_AR:-}"
  local ranlib_path="${ZMIN_HTTP_RANLIB:-}" make_input cc_input ar_input ranlib_input
  local codesign_path=/usr/bin/codesign
  local build_root bundle_tmp base_bundle base_git relative name helper_sha
  local build_command cc_version flags_file source_file source_name link_path
  local deterministic_cflags deterministic_rustflags deterministic_ldflags normalized_flags_path
  local normalized_flags_size normalized_flags_sha
  [[ "$make_path" == /* && -f "$make_path" && ! -L "$make_path" && -x "$make_path" ]] || {
    echo "with-http-fetch-pinned requires ZMIN_HTTP_MAKE as an absolute regular executable" >&2
    return 1
  }
  [[ "$cc_path" == /* && -f "$cc_path" && ! -L "$cc_path" && -x "$cc_path" ]] || {
    echo "with-http-fetch-pinned requires ZMIN_HTTP_CC as an absolute regular executable" >&2
    return 1
  }
  [[ "$ar_path" == /* && -f "$ar_path" && ! -L "$ar_path" && -x "$ar_path" ]] || {
    echo "with-http-fetch-pinned requires ZMIN_HTTP_AR as an absolute regular executable" >&2
    return 1
  }
  [[ "$ranlib_path" == /* && -f "$ranlib_path" && ! -L "$ranlib_path" && -x "$ranlib_path" ]] || {
    echo "with-http-fetch-pinned requires ZMIN_HTTP_RANLIB as an absolute regular executable" >&2
    return 1
  }
  [[ -f "$codesign_path" && ! -L "$codesign_path" && -x "$codesign_path" ]] || {
    echo "with-http-fetch-pinned requires the frozen Darwin codesign tool" >&2
    return 1
  }
  make_input="$make_path"
  cc_input="$cc_path"
  ar_input="$ar_path"
  ranlib_input="$ranlib_path"
  make_path="$(cd "$(dirname "$make_path")" && pwd -P)/$(basename "$make_path")"
  cc_path="$(cd "$(dirname "$cc_path")" && pwd -P)/$(basename "$cc_path")"
  ar_path="$(cd "$(dirname "$ar_path")" && pwd -P)/$(basename "$ar_path")"
  ranlib_path="$(cd "$(dirname "$ranlib_path")" && pwd -P)/$(basename "$ranlib_path")"
  codesign_path="$(cd "$(dirname "$codesign_path")" && pwd -P)/$(basename "$codesign_path")"
  [[ "$make_path" == "$make_input" && "$cc_path" == "$cc_input" &&
    "$ar_path" == "$ar_input" && "$ranlib_path" == "$ranlib_input" &&
    "$codesign_path" == /usr/bin/codesign ]] || {
    echo "with-http-fetch-pinned build tools must not contain symlinked path components" >&2
    return 1
  }
  [[ "$make_path" == /usr/bin/make && "$cc_path" == /usr/bin/cc &&
    "$ar_path" == /usr/bin/ar && "$ranlib_path" == /usr/bin/ranlib &&
    "$codesign_path" == /usr/bin/codesign ]] || {
    echo "with-http-fetch-pinned requires the frozen Darwin toolchain paths" >&2
    return 1
  }
  validate_source
  acquire_lock
  trap release_lock RETURN
  validate_source
  if [[ -e "$bundle" || -L "$bundle" ]]; then
    [[ -d "$bundle" && ! -L "$bundle" ]] || {
      echo "pinned HTTP comparator bundle path is occupied by a non-directory" >&2
      return 1
    }
    ZMIN_GIT_HTTP_BUNDLE="$bundle" ZMIN_STOCK_GIT="$bundle/$git_relative" validate_bundle >/dev/null
    printf '%s\n' "$bundle"
    return 0
  fi

  base_bundle="$cache_root/http-bundle-$tag-$commit-$platform-$arch"
  base_git="$base_bundle/$git_relative"
  ZMIN_HTTP_BUNDLE_PROFILE=canonical ZMIN_GIT_HTTP_BUNDLE="$base_bundle" \
    ZMIN_STOCK_GIT="$base_git" "$repo_root/tools/git-upstream-http-provenance.sh" validate canonical >/dev/null || {
    echo "with-http-fetch-pinned requires the separately validated canonical bundle: $base_bundle" >&2
    return 1
  }
  build_root="$(mktemp -d "$cache_root/.zmin-http-fetch-build.XXXXXX")"
  bundle_tmp="$(mktemp -d "$cache_root/.zmin-http-fetch-bundle.XXXXXX")"
  build_cleanup_root="$build_root"
  build_cleanup_tmp="$bundle_tmp"
  trap 'release_lock; chmod -R u+w "$build_cleanup_root" "$build_cleanup_tmp" 2>/dev/null || :; rm -rf "$build_cleanup_root" "$build_cleanup_tmp"' RETURN
  cp -R "$source_root/." "$build_root/"
  chmod -R u+w "$build_root"
  deterministic_cflags="-ffile-prefix-map=$build_root=$deterministic_build_prefix -fdebug-prefix-map=$build_root=$deterministic_build_prefix"
  deterministic_rustflags="--remap-path-prefix=$build_root=$deterministic_build_prefix"
  deterministic_ldflags='-Wl,-no_uuid,-headerpad_max_install_names'
  build_command="$deterministic_build_command"
  (cd "$build_root" && SOURCE_DATE_EPOCH=0 ZERO_AR_DATE=1 RUSTFLAGS="$deterministic_rustflags" \
    "$make_path" CC="$cc_path" AR="$ar_path" RANLIB="$ranlib_path" CFLAGS="$deterministic_cflags" \
    LDFLAGS="$deterministic_ldflags" $build_flags -j2 V=1 git-http-fetch) >&2
  build_executable "$build_root/$fetch_relative" || {
    echo "archive-bound Git source build did not produce $fetch_relative" >&2
    return 1
  }
  add_deterministic_macho_uuid "$build_root/$fetch_relative"
  "$codesign_path" --force --sign - --timestamp=none "$build_root/$fetch_relative" >/dev/null 2>&1 || {
    echo "archive-bound Git source helper could not receive a deterministic ad hoc signature" >&2
    return 1
  }
  helper_sha="$(trusted_helper_sha256 "$build_root/$fetch_relative")"
  [[ "$helper_sha" == "$expected_fetch_sha" ]] || {
    echo "archive-bound Git source produced an untrusted git-http-fetch helper" >&2
    return 1
  }
  for name in git git-remote-http git-http-backend; do
    case "$name" in
      git) relative="$git_relative" ;;
      git-remote-http) relative="$remote_relative" ;;
      git-http-backend) relative="$backend_relative" ;;
    esac
    cp "$base_bundle/$relative" "$bundle_tmp/$relative"
    chmod 555 "$bundle_tmp/$relative"
  done
  cp "$base_bundle/bundle.tsv" "$bundle_tmp/bundle.tsv"
  cp "$base_bundle/bundle.tsv.sha256" "$bundle_tmp/bundle.tsv.sha256"
  chmod 444 "$bundle_tmp/bundle.tsv" "$bundle_tmp/bundle.tsv.sha256"
  cp "$build_root/$fetch_relative" "$bundle_tmp/$fetch_relative"
  chmod 555 "$bundle_tmp/$fetch_relative"
  mkdir "$bundle_tmp/templates"
  chmod 555 "$bundle_tmp/templates"
  cc_version="$($cc_path --version | head -n 1)"
  [[ "$cc_version" == 'Apple clang version 21.0.0 (clang-2100.1.1.101)' ]] || {
    echo "with-http-fetch-pinned compiler version is not the frozen Darwin compiler" >&2
    return 1
  }
  {
    printf 'manifest_version\t3\n'
    printf 'bundle_role\t%s\n' "$bundle_role"
    printf 'upstream_git_tag\t%s\n' "$tag"
    printf 'upstream_git_commit\t%s\n' "$commit"
    printf 'upstream_archive_sha256\t%s\n' "$archive_sha"
    printf 'source_root\t%s\n' "$source_root"
    printf 'source_marker_sha256\t%s\n' "$archive_sha"
    printf 'source_manifest_sha256\t%s\n' "$source_manifest_value"
    printf 'source_commit_binding\tarchive_sha256+tag+DEF_VER;commit_not_embedded_in_archive\n'
    printf 'build_host\t%s %s\n' "$platform" "$arch"
    printf 'build_flags\t%s\n' "$build_flags"
    printf 'build_command\t%s\n' "$build_command"
    for name in make cc ar ranlib codesign; do
      case "$name" in
        make) tool_path="$make_path" ;;
        cc) tool_path="$cc_path" ;;
        ar) tool_path="$ar_path" ;;
        ranlib) tool_path="$ranlib_path" ;;
        codesign) tool_path="$codesign_path" ;;
      esac
      printf 'toolchain\t%s\t%s\t%s\t%s\n' "$tool_path" "$(stat -f '%04Lp' "$tool_path")" \
        "$(stat -f '%z' "$tool_path")" "$(member_sha "$tool_path")"
    done
    printf 'toolchain_version\t%s\t%s\n' "$cc_path" "$cc_version"
    for flags_file in GIT-CFLAGS GIT-LDFLAGS; do
      flags_path="$build_root/$flags_file"
      [[ -f "$flags_path" && ! -L "$flags_path" ]] || {
        echo "archive-bound Git build did not produce $flags_file" >&2
        return 1
      }
      normalized_flags_path="$bundle_tmp/.zmin-normalized-$flags_file"
      sed "s#${build_root}#${deterministic_build_prefix}#g" "$flags_path" >"$normalized_flags_path"
      normalized_flags_size="$(stat -f '%z' "$normalized_flags_path")"
      normalized_flags_sha="$(member_sha "$normalized_flags_path")"
      rm -f "$normalized_flags_path"
      printf 'build_flags_file\t%s\t%s\t%s\n' "$flags_file" "$normalized_flags_size" "$normalized_flags_sha"
    done
    for source_name in http-fetch.c http.c http-walker.c common-main.c; do
      source_file="$source_root/$source_name"
      printf 'source_member\t%s\t%s\t%s\n' "$source_name" "$(stat -f '%z' "$source_file")" \
        "$(member_sha "$source_file")"
    done
    printf 'member\tbundle.tsv\t0444\t%s\t%s\n' "$(stat -f '%z' "$bundle_tmp/bundle.tsv")" \
      "$(member_sha "$bundle_tmp/bundle.tsv")"
    printf 'member\tgit\t0555\t%s\t%s\n' "$(stat -f '%z' "$bundle_tmp/$git_relative")" \
      "$(member_sha "$bundle_tmp/$git_relative")"
    printf 'member\tgit-remote-http\t0555\t%s\t%s\n' "$(stat -f '%z' "$bundle_tmp/$remote_relative")" \
      "$(member_sha "$bundle_tmp/$remote_relative")"
    printf 'member\tgit-http-backend\t0555\t%s\t%s\n' "$(stat -f '%z' "$bundle_tmp/$backend_relative")" \
      "$(member_sha "$bundle_tmp/$backend_relative")"
    printf 'member\tgit-http-fetch\t0555\t%s\t%s\n' "$(stat -f '%z' "$bundle_tmp/$fetch_relative")" \
      "$(trusted_helper_sha256 "$bundle_tmp/$fetch_relative")"
    for link_path in "/System/Library/Frameworks/CoreServices.framework/Versions/A/CoreServices" \
      /usr/lib/libcurl.4.dylib /usr/lib/libz.1.dylib /usr/lib/libiconv.2.dylib \
      /usr/lib/libSystem.B.dylib; do
      printf 'link_dependency\t%s\n' "$link_path"
    done
    printf 'template_dir\ttemplates\t0555\tempty\n'
  } >"$bundle_tmp/manifest.tsv"
  chmod 444 "$bundle_tmp/manifest.tsv"
  member_sha "$bundle_tmp/manifest.tsv" >"$bundle_tmp/manifest.tsv.sha256"
  chmod 444 "$bundle_tmp/manifest.tsv.sha256"
  chmod a-w "$bundle_tmp"
  [[ ! -e "$bundle" && ! -L "$bundle" ]] || {
    echo "pinned HTTP comparator bundle appeared during publication" >&2
    return 1
  }
  mv "$bundle_tmp" "$bundle"
  bundle_tmp=""
  build_cleanup_tmp=""
  ZMIN_GIT_HTTP_BUNDLE="$bundle" ZMIN_STOCK_GIT="$bundle/$git_relative" validate_bundle >/dev/null
  printf '%s\n' "$bundle"
}

build_bundle() {
  case "$profile" in
    canonical) build_canonical_bundle ;;
    with-http-fetch-pinned) build_pinned_http_fetch_bundle ;;
  esac
}

case "$mode" in
  build) build_bundle ;;
  validate)
    validate_source
    ZMIN_GIT_HTTP_BUNDLE="${ZMIN_GIT_HTTP_BUNDLE:-$bundle}" \
      ZMIN_STOCK_GIT="${ZMIN_STOCK_GIT:-$bundle/$git_relative}" validate_bundle
    ;;
  validate-source)
    validate_source
    printf 'source_manifest_sha256\t%s\n' "$source_manifest_value"
    ;;
esac
