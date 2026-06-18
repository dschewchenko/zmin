use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;

use clap::{ArgAction, Args as ClapArgs, Parser, Subcommand, ValueHint};

#[derive(clap::ValueEnum, Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompatProfile {
    #[value(name = "v2-32")]
    V2_32,
    #[value(name = "v2-47")]
    V2_47,
    #[value(name = "modern")]
    Modern,
}

impl fmt::Display for CompatProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CompatProfile::V2_32 => write!(f, "v2-32"),
            CompatProfile::V2_47 => write!(f, "v2-47"),
            CompatProfile::Modern => write!(f, "modern"),
        }
    }
}

#[derive(clap::ValueEnum, Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompatFormat {
    #[value(name = "text")]
    Text,
    #[value(name = "json")]
    Json,
}

#[derive(Parser, Debug)]
#[command(version, about = "Thin Git-compatible CLI over zmin-git-core")]
pub struct Args {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    #[command(name = "compatibility", aliases = ["compat"])]
    Compatibility {
        #[arg(long, value_enum, default_value_t = CompatProfile::V2_32)]
        profile: CompatProfile,
        #[arg(long, value_enum, default_value_t = CompatFormat::Text)]
        format: CompatFormat,
    },
    Save {
        message: String,
    },
    Publish,
    Update,
    Undo,
    Changes,
    Timeline,
    Recover {
        #[arg(value_hint = ValueHint::AnyPath)]
        paths: Vec<PathBuf>,
    },
    Init {
        #[arg(long = "bare", action = ArgAction::SetTrue)]
        bare: bool,
        #[arg(
            long = "template",
            value_hint = ValueHint::DirPath,
            num_args = 0..=1,
            require_equals = true,
            default_missing_value = ""
        )]
        template: Option<PathBuf>,
        #[arg(long = "separate-git-dir", value_hint = ValueHint::DirPath)]
        separate_git_dir: Option<PathBuf>,
        #[arg(
            long = "shared",
            num_args = 0..=1,
            require_equals = true,
            default_missing_value = "group"
        )]
        shared: Option<String>,
        #[arg(short = 'b', long = "initial-branch")]
        initial_branch: Option<String>,
        #[arg(long = "object-format")]
        object_format: Option<String>,
        #[arg(long = "ref-format")]
        ref_format: Option<String>,
        #[arg(value_hint = ValueHint::DirPath)]
        directory: Option<PathBuf>,
    },
    Clone {
        #[arg(short = 'q', long = "quiet", action = ArgAction::SetTrue)]
        quiet: bool,
        #[arg(short = 'v', long = "verbose", action = ArgAction::SetTrue)]
        verbose: bool,
        #[arg(long = "progress", action = ArgAction::SetTrue)]
        progress: bool,
        #[arg(long = "no-progress", action = ArgAction::SetTrue)]
        no_progress: bool,
        #[arg(long = "bare", action = ArgAction::SetTrue)]
        bare: bool,
        #[arg(long = "mirror", action = ArgAction::SetTrue)]
        mirror: bool,
        #[arg(short = 'l', long = "local", action = ArgAction::SetTrue)]
        local: bool,
        #[arg(long = "no-local", action = ArgAction::SetTrue)]
        no_local: bool,
        #[arg(long = "no-hardlinks", action = ArgAction::SetTrue)]
        no_hardlinks: bool,
        #[arg(long = "hardlinks", action = ArgAction::SetTrue)]
        hardlinks: bool,
        #[arg(long = "reject-shallow", action = ArgAction::SetTrue)]
        reject_shallow: bool,
        #[arg(long = "no-reject-shallow", action = ArgAction::SetTrue)]
        no_reject_shallow: bool,
        #[arg(long = "template", value_hint = ValueHint::DirPath)]
        template: Option<PathBuf>,
        #[arg(long = "no-template", action = ArgAction::SetTrue)]
        no_template: bool,
        #[arg(short = 'c', long = "config")]
        configs: Vec<String>,
        #[arg(short = 'n', long = "no-checkout", action = ArgAction::SetTrue)]
        no_checkout: bool,
        #[arg(long = "checkout", action = ArgAction::SetTrue)]
        checkout: bool,
        #[arg(long = "worktree-first", action = ArgAction::SetTrue)]
        worktree_first: bool,
        #[arg(long = "instant", action = ArgAction::SetTrue)]
        instant: bool,
        #[arg(long = "background-fetch", action = ArgAction::SetTrue)]
        background_fetch: bool,
        #[arg(long = "demand-hydrate", action = ArgAction::SetTrue)]
        demand_hydrate: bool,
        #[arg(
            long = "recurse-submodules",
            num_args = 0..=1,
            require_equals = true,
            default_missing_value = ".",
            action = ArgAction::Append
        )]
        recurse_submodules: Vec<String>,
        #[arg(
            long = "recursive",
            num_args = 0..=1,
            require_equals = true,
            default_missing_value = ".",
            action = ArgAction::Append
        )]
        recursive: Vec<String>,
        #[arg(long = "no-recurse-submodules", action = ArgAction::SetTrue)]
        no_recurse_submodules: bool,
        #[arg(short = 'j', long = "jobs", allow_hyphen_values = true)]
        jobs: Option<String>,
        #[arg(long = "shallow-submodules", action = ArgAction::SetTrue)]
        shallow_submodules: bool,
        #[arg(long = "remote-submodules", action = ArgAction::SetTrue)]
        remote_submodules: bool,
        #[arg(short = 'o', long = "origin", default_value = "origin")]
        origin: String,
        #[arg(long = "no-tags", action = ArgAction::SetTrue)]
        no_tags: bool,
        #[arg(long = "tags", action = ArgAction::SetTrue)]
        tags: bool,
        #[arg(long = "single-branch", action = ArgAction::SetTrue)]
        single_branch: bool,
        #[arg(long = "no-single-branch", action = ArgAction::SetTrue)]
        no_single_branch: bool,
        #[arg(long = "separate-git-dir", value_hint = ValueHint::DirPath)]
        separate_git_dir: Option<PathBuf>,
        #[arg(long = "reference", value_hint = ValueHint::DirPath)]
        references: Vec<PathBuf>,
        #[arg(long = "reference-if-able", value_hint = ValueHint::DirPath)]
        reference_if_able: Vec<PathBuf>,
        #[arg(short = 's', long = "shared", action = ArgAction::SetTrue)]
        shared: bool,
        #[arg(long = "dissociate", action = ArgAction::SetTrue)]
        dissociate: bool,
        #[arg(long = "depth")]
        depth: Option<String>,
        #[arg(short = 'b', long = "branch")]
        branch: Option<String>,
        #[arg(long = "ref-format")]
        ref_format: Option<String>,
        repository: String,
        #[arg(value_hint = ValueHint::DirPath)]
        directory: Option<PathBuf>,
    },
    HashObject {
        #[arg(short = 't', long = "type", default_value = "blob")]
        object_type: String,
        #[arg(short = 'w', long = "write", action = ArgAction::SetTrue)]
        write: bool,
        #[arg(long = "stdin", action = ArgAction::SetTrue)]
        stdin: bool,
        #[arg(value_hint = ValueHint::FilePath)]
        paths: Vec<PathBuf>,
    },
    CatFile {
        #[arg(short = 't', long = "type", action = ArgAction::SetTrue)]
        type_only: bool,
        #[arg(short = 'p', long = "pretty", action = ArgAction::SetTrue)]
        pretty: bool,
        #[arg(short = 's', long = "size", action = ArgAction::SetTrue)]
        size: bool,
        #[arg(short = 'e', long = "exists", action = ArgAction::SetTrue)]
        exists: bool,
        #[arg(long = "textconv", action = ArgAction::SetTrue)]
        textconv: bool,
        #[arg(long = "filters", action = ArgAction::SetTrue)]
        filters: bool,
        #[arg(long = "path")]
        path: Option<String>,
        #[arg(
            long = "batch-check",
            num_args = 0..=1,
            require_equals = true,
            default_missing_value = ""
        )]
        batch_check: Option<String>,
        #[arg(
            long = "batch",
            num_args = 0..=1,
            require_equals = true,
            default_missing_value = ""
        )]
        batch: Option<String>,
        #[arg(
            long = "batch-command",
            num_args = 0..=1,
            require_equals = true,
            default_missing_value = ""
        )]
        batch_command: Option<String>,
        #[arg(long = "batch-all-objects", action = ArgAction::SetTrue)]
        batch_all_objects: bool,
        #[arg(long = "buffer", action = ArgAction::SetTrue)]
        buffer: bool,
        #[arg(long = "no-buffer", action = ArgAction::SetTrue)]
        no_buffer: bool,
        #[arg(long = "follow-symlinks", action = ArgAction::SetTrue)]
        follow_symlinks: bool,
        #[arg(short = 'z', action = ArgAction::SetTrue)]
        nul: bool,
        #[arg(short = 'Z', action = ArgAction::SetTrue)]
        full_nul: bool,
        #[arg(long = "unordered", action = ArgAction::SetTrue)]
        unordered: bool,
        #[arg(long = "no-unordered", action = ArgAction::SetTrue)]
        no_unordered: bool,
        #[arg(long = "filter")]
        filter: Option<String>,
        #[arg(long = "no-filter", action = ArgAction::SetTrue)]
        no_filter: bool,
        objects: Vec<String>,
    },
    CountObjects {
        #[arg(short = 'v', long = "verbose", action = ArgAction::SetTrue)]
        verbose: bool,
        #[arg(short = 'H', long = "human-readable", action = ArgAction::SetTrue)]
        human_readable: bool,
    },
    UnpackFile {
        object: String,
    },
    ShowIndex,
    UpdateServerInfo {
        #[arg(short = 'f', long = "force", action = ArgAction::SetTrue)]
        force: bool,
    },
    CheckRefFormat {
        #[arg(long = "allow-onelevel", action = ArgAction::SetTrue)]
        allow_onelevel: bool,
        #[arg(long = "normalize", action = ArgAction::SetTrue)]
        normalize: bool,
        #[arg(long = "branch")]
        branch: Option<String>,
        refname: Option<String>,
    },
    CheckIgnore {
        #[arg(short = 'q', long = "quiet", action = ArgAction::Count)]
        quiet: u8,
        #[arg(short = 'v', long = "verbose", action = ArgAction::Count)]
        verbose: u8,
        #[arg(short = 'n', long = "non-matching", action = ArgAction::Count)]
        non_matching: u8,
        #[arg(long = "stdin", action = ArgAction::Count)]
        stdin: u8,
        #[arg(short = 'z', action = ArgAction::Count)]
        nul: u8,
        #[arg(long = "no-index", action = ArgAction::Count)]
        no_index: u8,
        #[arg(value_hint = ValueHint::AnyPath)]
        paths: Vec<PathBuf>,
    },
    CheckMailmap {
        #[arg(long = "stdin", action = ArgAction::SetTrue)]
        stdin: bool,
        identities: Vec<String>,
    },
    CheckAttr {
        #[arg(short = 'a', long = "all", action = ArgAction::SetTrue)]
        all: bool,
        #[arg(long = "stdin", action = ArgAction::SetTrue)]
        stdin: bool,
        #[arg(allow_hyphen_values = true)]
        args: Vec<String>,
    },
    UnpackObjects {
        #[arg(short = 'n', action = ArgAction::SetTrue)]
        dry_run: bool,
        #[arg(short = 'q', action = ArgAction::SetTrue)]
        quiet: bool,
        #[arg(short = 'r', action = ArgAction::SetTrue)]
        recover: bool,
        #[arg(long = "strict", action = ArgAction::SetTrue)]
        strict: bool,
    },
    PackObjects {
        #[arg(long = "stdout", action = ArgAction::SetTrue)]
        stdout: bool,
        #[arg(long = "revs", action = ArgAction::SetTrue)]
        revs: bool,
        #[arg(long = "all", action = ArgAction::SetTrue)]
        all: bool,
        #[arg(long = "progress", action = ArgAction::SetTrue)]
        progress: bool,
        #[arg(long = "no-progress", action = ArgAction::SetTrue)]
        no_progress: bool,
        #[arg(long = "index-version")]
        index_version: Option<String>,
        #[arg(long = "no-reuse-delta", action = ArgAction::SetTrue)]
        no_reuse_delta: bool,
        #[arg(long = "no-reuse-object", action = ArgAction::SetTrue)]
        no_reuse_object: bool,
        #[arg(long = "delta-base-offset", action = ArgAction::SetTrue)]
        delta_base_offset: bool,
        #[arg(long = "window")]
        window: Option<usize>,
        #[arg(long = "depth")]
        depth: Option<usize>,
        #[arg(value_hint = ValueHint::FilePath)]
        base_name: Option<PathBuf>,
    },
    Bundle {
        operation: String,
        #[arg(long = "version")]
        version: Option<String>,
        #[arg(value_hint = ValueHint::FilePath)]
        file: PathBuf,
        #[arg(allow_hyphen_values = true)]
        args: Vec<String>,
    },
    IndexPack {
        #[arg(long = "stdin", action = ArgAction::SetTrue)]
        stdin: bool,
        #[arg(short = 'o', value_hint = ValueHint::FilePath)]
        output: Option<PathBuf>,
        #[arg(long = "keep", num_args = 0..=1, default_missing_value = "")]
        keep: Option<String>,
        #[arg(long = "rev-index", action = ArgAction::SetTrue)]
        rev_index: bool,
        #[arg(long = "no-rev-index", action = ArgAction::SetTrue)]
        no_rev_index: bool,
        #[arg(long = "verify", action = ArgAction::SetTrue)]
        verify: bool,
        #[arg(long = "strict", num_args = 0..=1, require_equals = true, default_missing_value = "")]
        strict: Option<String>,
        #[arg(long = "fsck-objects", num_args = 0..=1, require_equals = true, default_missing_value = "")]
        fsck_objects: Option<String>,
        #[arg(long = "fix-thin", action = ArgAction::SetTrue)]
        fix_thin: bool,
        #[arg(short = 'v', action = ArgAction::SetTrue)]
        verbose: bool,
        #[arg(long = "index-version")]
        index_version: Option<String>,
        #[arg(value_hint = ValueHint::FilePath)]
        pack_file: Option<PathBuf>,
    },
    Column {
        #[arg(long = "mode")]
        mode: Option<String>,
        #[arg(long = "raw-mode")]
        raw_mode: Option<u32>,
        #[arg(long = "width")]
        width: Option<usize>,
        #[arg(long = "padding")]
        padding: Option<usize>,
    },
    GetTarCommitId,
    Archive {
        #[arg(long = "format")]
        format: Option<String>,
        #[arg(long = "prefix")]
        prefix: Option<String>,
        #[arg(short = 'o', long = "output", value_hint = ValueHint::FilePath)]
        output: Option<PathBuf>,
        #[arg(long = "add-file", value_hint = ValueHint::FilePath)]
        add_files: Vec<PathBuf>,
        #[arg(long = "add-virtual-file")]
        add_virtual_files: Vec<String>,
        #[arg(long = "mtime")]
        mtime: Option<String>,
        #[arg(short = 'l', long = "list", action = ArgAction::SetTrue)]
        list: bool,
        #[arg(short = 'v', long = "verbose", action = ArgAction::SetTrue)]
        verbose: bool,
        treeish: Option<String>,
        #[arg(value_hint = ValueHint::AnyPath)]
        paths: Vec<String>,
    },
    Credential {
        operation: String,
    },
    CredentialStore {
        #[arg(long = "file", value_hint = ValueHint::FilePath)]
        file: Option<PathBuf>,
        action: String,
    },
    CredentialCache {
        #[arg(long = "timeout")]
        timeout: Option<u64>,
        #[arg(long = "socket", value_hint = ValueHint::FilePath)]
        socket: Option<PathBuf>,
        #[arg(long = "daemon-internal", hide = true, action = ArgAction::SetTrue)]
        daemon_internal: bool,
        action: Option<String>,
    },
    InterpretTrailers {
        #[arg(long = "in-place", action = ArgAction::SetTrue)]
        in_place: bool,
        #[arg(long = "trim-empty", action = ArgAction::SetTrue)]
        trim_empty: bool,
        #[arg(long = "where")]
        where_: Option<String>,
        #[arg(long = "if-exists")]
        if_exists: Option<String>,
        #[arg(long = "if-missing")]
        if_missing: Option<String>,
        #[arg(long = "only-trailers", action = ArgAction::SetTrue)]
        only_trailers: bool,
        #[arg(long = "only-input", action = ArgAction::SetTrue)]
        only_input: bool,
        #[arg(long = "unfold", action = ArgAction::SetTrue)]
        unfold: bool,
        #[arg(long = "parse", action = ArgAction::SetTrue)]
        parse: bool,
        #[arg(long = "no-divider", action = ArgAction::SetTrue)]
        no_divider: bool,
        #[arg(long = "divider", action = ArgAction::SetTrue)]
        divider: bool,
        #[arg(long = "trailer")]
        trailers: Vec<String>,
        #[arg(value_hint = ValueHint::FilePath)]
        files: Vec<PathBuf>,
    },
    Mailsplit {
        #[arg(short = 'd')]
        precision: Option<usize>,
        #[arg(short = 'f')]
        first: Option<usize>,
        #[arg(short = 'b', action = ArgAction::SetTrue)]
        keep_from: bool,
        #[arg(long = "keep-cr", action = ArgAction::SetTrue)]
        keep_cr: bool,
        #[arg(short = 'o', value_hint = ValueHint::DirPath)]
        output: PathBuf,
        #[arg(value_hint = ValueHint::AnyPath)]
        paths: Vec<PathBuf>,
    },
    Mailinfo {
        #[arg(short = 'k', action = ArgAction::SetTrue)]
        keep_subject: bool,
        #[arg(short = 'b', action = ArgAction::SetTrue)]
        keep_non_patch_brackets: bool,
        #[arg(short = 'm', long = "message-id", action = ArgAction::SetTrue)]
        message_id: bool,
        #[arg(short = 'u', action = ArgAction::SetTrue)]
        recode: bool,
        #[arg(short = 'n', action = ArgAction::SetTrue)]
        no_recode: bool,
        #[arg(long = "encoding")]
        encoding: Option<String>,
        #[arg(long = "scissors", action = ArgAction::SetTrue)]
        scissors: bool,
        #[arg(long = "quoted-cr")]
        quoted_cr: Option<String>,
        #[arg(value_hint = ValueHint::FilePath)]
        msg: PathBuf,
        #[arg(value_hint = ValueHint::FilePath)]
        patch: PathBuf,
    },
    FmtMergeMsg {
        #[arg(long = "log", num_args = 0..=1, default_missing_value = "20")]
        log: Option<usize>,
        #[arg(long = "no-log", action = ArgAction::SetTrue)]
        no_log: bool,
        #[arg(short = 'm', long = "message")]
        message: Option<String>,
        #[arg(long = "into-name")]
        into_name: Option<String>,
        #[arg(short = 'F', long = "file", value_hint = ValueHint::FilePath)]
        file: Option<PathBuf>,
    },
    Shortlog {
        #[arg(short = 'c', long = "committer", action = ArgAction::SetTrue)]
        committer: bool,
        #[arg(short = 'n', long = "numbered", action = ArgAction::SetTrue)]
        numbered: bool,
        #[arg(short = 's', long = "summary", action = ArgAction::SetTrue)]
        summary: bool,
        #[arg(short = 'e', long = "email", action = ArgAction::SetTrue)]
        email: bool,
        #[arg(long = "no-merges", action = ArgAction::SetTrue)]
        no_merges: bool,
        #[arg(allow_hyphen_values = true)]
        revs: Vec<String>,
    },
    Blame {
        #[arg(short = 'l', action = ArgAction::SetTrue)]
        long: bool,
        #[arg(long = "root", action = ArgAction::SetTrue)]
        root: bool,
        #[arg(allow_hyphen_values = true)]
        args: Vec<String>,
    },
    Annotate {
        #[arg(allow_hyphen_values = true)]
        args: Vec<String>,
    },
    ShowBranch {
        #[arg(short = 'a', long = "all", action = ArgAction::SetTrue)]
        all: bool,
        #[arg(short = 'r', long = "remotes", action = ArgAction::SetTrue)]
        remotes: bool,
        #[arg(long = "current", action = ArgAction::SetTrue)]
        current: bool,
        #[arg(long = "sha1-name", action = ArgAction::SetTrue)]
        sha1_name: bool,
        #[arg(long = "no-name", action = ArgAction::SetTrue)]
        no_name: bool,
        #[arg(allow_hyphen_values = true)]
        revs: Vec<String>,
    },
    Cherry {
        #[arg(short = 'v', long = "verbose", action = ArgAction::SetTrue)]
        verbose: bool,
        #[arg(
            long = "abbrev",
            num_args = 0..=1,
            default_missing_value = "7",
            require_equals = true
        )]
        abbrev: Option<usize>,
        upstream: Option<String>,
        head: Option<String>,
        limit: Option<String>,
    },
    CherryPick {
        #[arg(long = "abort", action = ArgAction::SetTrue)]
        abort: bool,
        #[arg(long = "continue", action = ArgAction::SetTrue)]
        continue_: bool,
        #[arg(short = 'n', long = "no-commit", action = ArgAction::SetTrue)]
        no_commit: bool,
        #[arg(short = 'm', long = "mainline")]
        mainline: Option<usize>,
        commits: Vec<String>,
    },
    Revert {
        #[arg(long = "abort", action = ArgAction::SetTrue)]
        abort: bool,
        #[arg(long = "continue", action = ArgAction::SetTrue)]
        continue_: bool,
        #[arg(short = 'n', long = "no-commit", action = ArgAction::SetTrue)]
        no_commit: bool,
        #[arg(short = 'm', long = "mainline")]
        mainline: Option<usize>,
        commits: Vec<String>,
    },
    RequestPull {
        start: String,
        url: String,
        end: Option<String>,
    },
    Describe {
        #[arg(long = "all", action = ArgAction::SetTrue)]
        all: bool,
        #[arg(long = "tags", action = ArgAction::SetTrue)]
        tags: bool,
        #[arg(long = "long", action = ArgAction::SetTrue)]
        long: bool,
        #[arg(
            long = "abbrev",
            num_args = 0..=1,
            default_missing_value = "7",
            require_equals = true
        )]
        abbrev: Option<usize>,
        #[arg(long = "exact-match", action = ArgAction::SetTrue)]
        exact_match: bool,
        #[arg(long = "always", action = ArgAction::SetTrue)]
        always: bool,
        #[arg(
            long = "dirty",
            num_args = 0..=1,
            default_missing_value = "-dirty",
            require_equals = true
        )]
        dirty: Option<String>,
        #[arg(long = "match")]
        matches: Vec<String>,
        #[arg(long = "exclude")]
        excludes: Vec<String>,
        commits: Vec<String>,
    },
    NameRev {
        #[arg(long = "name-only", action = ArgAction::SetTrue)]
        name_only: bool,
        #[arg(long = "tags", action = ArgAction::SetTrue)]
        tags: bool,
        #[arg(long = "refs")]
        refs: Vec<String>,
        #[arg(long = "exclude")]
        excludes: Vec<String>,
        #[arg(long = "all", action = ArgAction::SetTrue)]
        all: bool,
        #[arg(long = "annotate-stdin", action = ArgAction::SetTrue)]
        annotate_stdin: bool,
        #[arg(long = "undefined", action = ArgAction::SetTrue)]
        undefined: bool,
        #[arg(long = "always", action = ArgAction::SetTrue)]
        always: bool,
        commits: Vec<String>,
    },
    ForEachRepo {
        #[arg(long = "config")]
        config: String,
        #[arg(long = "keep-going", action = ArgAction::SetTrue)]
        keep_going: bool,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        arguments: Vec<String>,
    },
    Reflog {
        #[arg(allow_hyphen_values = true)]
        args: Vec<String>,
    },
    Fsck {
        #[arg(long = "unreachable", action = ArgAction::SetTrue)]
        unreachable: bool,
        #[arg(long = "dangling", action = ArgAction::SetTrue)]
        dangling: bool,
        #[arg(long = "no-dangling", action = ArgAction::SetTrue)]
        no_dangling: bool,
        #[arg(long = "strict", action = ArgAction::SetTrue)]
        strict: bool,
        #[arg(long = "full", action = ArgAction::SetTrue)]
        full: bool,
        #[arg(long = "connectivity-only", action = ArgAction::SetTrue)]
        connectivity_only: bool,
        #[arg(long = "no-reflogs", action = ArgAction::SetTrue)]
        no_reflogs: bool,
        #[arg(long = "cache", action = ArgAction::SetTrue)]
        cache: bool,
        #[arg(long = "tags", action = ArgAction::SetTrue)]
        tags: bool,
        #[arg(long = "root", action = ArgAction::SetTrue)]
        root: bool,
        #[arg(short = 'v', long = "verbose", action = ArgAction::SetTrue)]
        verbose: bool,
        #[arg(long = "lost-found", action = ArgAction::SetTrue)]
        lost_found: bool,
        #[arg(long = "progress", action = ArgAction::SetTrue)]
        progress: bool,
        #[arg(long = "no-progress", action = ArgAction::SetTrue)]
        no_progress: bool,
        #[arg(long = "name-objects", action = ArgAction::SetTrue)]
        name_objects: bool,
        #[arg(long = "references", action = ArgAction::SetTrue)]
        references: bool,
        #[arg(long = "no-references", action = ArgAction::SetTrue)]
        no_references: bool,
        objects: Vec<String>,
    },
    VerifyPack {
        #[arg(short = 'v', long = "verbose", action = ArgAction::SetTrue)]
        verbose: bool,
        #[arg(short = 's', long = "stat-only", action = ArgAction::SetTrue)]
        stat_only: bool,
        #[arg(long = "object-format")]
        object_format: Option<String>,
        packs: Vec<PathBuf>,
    },
    PackRedundant {
        #[arg(long = "verbose", action = ArgAction::SetTrue)]
        verbose: bool,
        #[arg(long = "alt-odb", action = ArgAction::SetTrue)]
        alt_odb: bool,
        #[arg(long = "all", action = ArgAction::SetTrue)]
        all: bool,
        #[arg(long = "i-still-use-this", hide = true, action = ArgAction::SetTrue)]
        i_still_use_this: bool,
        #[arg(value_hint = ValueHint::FilePath)]
        packs: Vec<PathBuf>,
    },
    VerifyCommit {
        #[arg(short = 'v', long = "verbose", action = ArgAction::SetTrue)]
        verbose: bool,
        #[arg(long = "raw", action = ArgAction::SetTrue)]
        raw: bool,
        commits: Vec<String>,
    },
    VerifyTag {
        #[arg(short = 'v', long = "verbose", action = ArgAction::SetTrue)]
        verbose: bool,
        #[arg(long = "raw", action = ArgAction::SetTrue)]
        raw: bool,
        tags: Vec<String>,
    },
    UpdateIndex {
        #[arg(long = "add", action = ArgAction::SetTrue)]
        add: bool,
        #[arg(long = "remove", action = ArgAction::SetTrue)]
        remove: bool,
        #[arg(long = "force-remove", action = ArgAction::SetTrue)]
        force_remove: bool,
        #[arg(long = "replace", action = ArgAction::SetTrue)]
        replace: bool,
        #[arg(long = "refresh", action = ArgAction::SetTrue)]
        refresh: bool,
        #[arg(long = "really-refresh", action = ArgAction::SetTrue)]
        really_refresh: bool,
        #[arg(long = "cacheinfo")]
        cacheinfo: Vec<String>,
        #[arg(long = "index-info", action = ArgAction::SetTrue)]
        index_info_mode: bool,
        #[arg(long = "chmod")]
        chmod: Option<String>,
        #[arg(long = "assume-unchanged", action = ArgAction::SetTrue)]
        assume_unchanged: bool,
        #[arg(long = "no-assume-unchanged", action = ArgAction::SetTrue)]
        no_assume_unchanged: bool,
        #[arg(long = "skip-worktree", action = ArgAction::SetTrue)]
        skip_worktree: bool,
        #[arg(long = "no-skip-worktree", action = ArgAction::SetTrue)]
        no_skip_worktree: bool,
        #[arg(long = "stdin", action = ArgAction::SetTrue)]
        stdin: bool,
        #[arg(short = 'z', action = ArgAction::SetTrue)]
        nul_terminated: bool,
        paths: Vec<PathBuf>,
    },
    Bugreport {
        #[arg(short = 'o', long = "output-directory", value_hint = ValueHint::DirPath)]
        output_directory: Option<PathBuf>,
        #[arg(short = 's', long = "suffix")]
        suffix: Option<String>,
        #[arg(long = "no-suffix", action = ArgAction::SetTrue)]
        no_suffix: bool,
        #[arg(long = "diagnose", num_args = 0..=1, default_missing_value = "stats", require_equals = true)]
        diagnose: Option<String>,
    },
    Diagnose {
        #[arg(short = 'o', long = "output-directory", value_hint = ValueHint::DirPath)]
        output_directory: Option<PathBuf>,
        #[arg(short = 's', long = "suffix")]
        suffix: Option<String>,
        #[arg(long = "mode", default_value = "stats")]
        mode: String,
    },
    Backfill {
        #[arg(long = "min-batch-size")]
        min_batch_size: Option<usize>,
        #[arg(long = "sparse", action = ArgAction::SetTrue)]
        sparse: bool,
        #[arg(long = "no-sparse", action = ArgAction::SetTrue)]
        no_sparse: bool,
        #[arg(allow_hyphen_values = true)]
        revs: Vec<String>,
    },
    Replay {
        #[arg(long = "contained", action = ArgAction::SetTrue)]
        contained: bool,
        #[arg(long = "advance")]
        advance: Option<String>,
        #[arg(long = "onto")]
        onto: Option<String>,
        #[arg(allow_hyphen_values = true)]
        revision_ranges: Vec<String>,
    },
    History {
        #[command(subcommand)]
        command: HistoryCommand,
    },
    Replace {
        #[arg(short = 'l', long = "list", action = ArgAction::SetTrue)]
        list: bool,
        #[arg(short = 'd', long = "delete", action = ArgAction::SetTrue)]
        delete: bool,
        #[arg(short = 'f', long = "force", action = ArgAction::SetTrue)]
        force: bool,
        #[arg(long = "format")]
        format: Option<String>,
        #[arg(short = 'e', long = "edit", action = ArgAction::SetTrue)]
        edit: bool,
        #[arg(short = 'g', long = "graft", action = ArgAction::SetTrue)]
        graft: bool,
        #[arg(long = "convert-graft-file", action = ArgAction::SetTrue)]
        convert_graft_file: bool,
        #[arg(long = "raw", action = ArgAction::SetTrue)]
        raw: bool,
        #[arg(long = "no-raw", action = ArgAction::SetTrue)]
        no_raw: bool,
        args: Vec<String>,
    },
    PatchId {
        #[arg(long = "stable", action = ArgAction::SetTrue)]
        stable: bool,
        #[arg(long = "unstable", action = ArgAction::SetTrue)]
        unstable: bool,
        #[arg(long = "verbatim", action = ArgAction::SetTrue)]
        verbatim: bool,
    },
    Stripspace {
        #[arg(short = 's', long = "strip-comments", action = ArgAction::SetTrue)]
        strip_comments: bool,
        #[arg(short = 'c', long = "comment-lines", action = ArgAction::SetTrue)]
        comment_lines: bool,
    },
    Status {
        #[arg(
            long = "porcelain",
            num_args = 0..=1,
            default_missing_value = "v1",
            require_equals = true
        )]
        porcelain: Option<String>,
        #[arg(short = 'b', long = "branch", action = ArgAction::SetTrue)]
        branch: bool,
        #[arg(short = 's', long = "short", action = ArgAction::SetTrue)]
        short: bool,
        #[arg(
            long = "ignored",
            num_args = 0..=1,
            default_missing_value = "traditional"
        )]
        ignored: Option<String>,
        #[arg(
            short = 'u',
            long = "untracked-files",
            num_args = 0..=1,
            default_missing_value = "all"
        )]
        untracked_files: Option<String>,
        #[arg(value_hint = ValueHint::AnyPath)]
        paths: Vec<PathBuf>,
    },
    Config {
        #[arg(long = "get", action = ArgAction::SetTrue)]
        get: bool,
        #[arg(long = "get-all", action = ArgAction::SetTrue)]
        get_all: bool,
        #[arg(long = "list", short = 'l', action = ArgAction::SetTrue)]
        list: bool,
        #[arg(long = "unset", action = ArgAction::SetTrue)]
        unset: bool,
        #[arg(long = "unset-all", action = ArgAction::SetTrue)]
        unset_all: bool,
        #[arg(long = "add", action = ArgAction::SetTrue)]
        add: bool,
        #[arg(long = "append", action = ArgAction::SetTrue)]
        append: bool,
        #[arg(long = "bool", action = ArgAction::SetTrue)]
        bool_value: bool,
        #[arg(long = "int", action = ArgAction::SetTrue)]
        int_value: bool,
        #[arg(long = "bool-or-int", action = ArgAction::SetTrue)]
        bool_or_int_value: bool,
        #[arg(long = "bool-or-str", action = ArgAction::SetTrue)]
        bool_or_str_value: bool,
        #[arg(long = "path", action = ArgAction::SetTrue)]
        path_value: bool,
        #[arg(long = "expiry-date", action = ArgAction::SetTrue)]
        expiry_date_value: bool,
        #[arg(long = "type")]
        value_type: Option<String>,
        #[arg(long = "default")]
        default: Option<String>,
        #[arg(long = "worktree", action = ArgAction::SetTrue)]
        worktree: bool,
        #[arg(long = "local", action = ArgAction::SetTrue)]
        local: bool,
        #[arg(long = "global", action = ArgAction::SetTrue)]
        global: bool,
        #[arg(short = 'f', long = "file", value_hint = ValueHint::FilePath)]
        file: Option<PathBuf>,
        #[arg(long = "includes", action = ArgAction::SetTrue)]
        includes: bool,
        #[arg(long = "show-origin", action = ArgAction::SetTrue)]
        show_origin: bool,
        #[arg(long = "show-scope", action = ArgAction::SetTrue)]
        show_scope: bool,
        arg0: Option<String>,
        #[arg(allow_hyphen_values = true)]
        arg1: Option<String>,
        #[arg(allow_hyphen_values = true)]
        arg2: Option<String>,
    },
    Var {
        #[arg(short = 'l', long = "list", action = ArgAction::SetTrue)]
        list: bool,
        variable: Option<String>,
    },
    Version {
        #[arg(long = "build-options", action = ArgAction::SetTrue)]
        build_options: bool,
    },
    Hook {
        #[command(subcommand)]
        command: HookCommand,
    },
    Hooks {
        #[command(subcommand)]
        command: ManagedHooksCommand,
    },
    #[command(name = "sh-i18n", disable_help_flag = true)]
    ShI18n {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    #[command(name = "sh-setup", disable_help_flag = true)]
    ShSetup {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    #[command(name = "cvsserver", disable_help_flag = true)]
    Cvsserver {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    #[command(name = "cvsexportcommit", disable_help_flag = true)]
    Cvsexportcommit {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    #[command(name = "cvsimport", disable_help_flag = true)]
    Cvsimport {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    #[command(name = "archimport", disable_help_flag = true)]
    Archimport {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    #[command(name = "p4", disable_help_flag = true)]
    P4 {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    #[command(name = "svn", disable_help_flag = true)]
    Svn {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    Instaweb {
        #[arg(long = "start", action = ArgAction::SetTrue)]
        start: bool,
        #[arg(long = "stop", action = ArgAction::SetTrue)]
        stop: bool,
        #[arg(long = "restart", action = ArgAction::SetTrue)]
        restart: bool,
        #[arg(short = 'l', long = "local", action = ArgAction::SetTrue)]
        local: bool,
        #[arg(short = 'p', long = "port", default_value_t = 1234)]
        port: u16,
        #[arg(short = 'd', long = "httpd")]
        httpd: Option<String>,
        #[arg(short = 'b', long = "browser")]
        browser: Option<String>,
        #[arg(long = "daemon-internal", hide = true, action = ArgAction::SetTrue)]
        daemon_internal: bool,
        #[arg(long = "git-dir", hide = true, value_hint = ValueHint::DirPath)]
        git_dir: Option<PathBuf>,
        #[arg(long = "work-tree", hide = true, value_hint = ValueHint::DirPath)]
        work_tree: Option<PathBuf>,
    },
    Remote {
        #[arg(short = 'v', long = "verbose", action = ArgAction::SetTrue)]
        verbose: bool,
        #[command(subcommand)]
        command: Option<RemoteCommand>,
    },
    LsRemote {
        #[arg(long = "heads", alias = "branches", action = ArgAction::SetTrue)]
        heads: bool,
        #[arg(long = "tags", action = ArgAction::SetTrue)]
        tags: bool,
        #[arg(long = "refs", action = ArgAction::SetTrue)]
        refs_only: bool,
        #[arg(long = "upload-pack")]
        upload_pack: Option<String>,
        repository: Option<String>,
        patterns: Vec<String>,
    },
    Fetch {
        #[arg(long = "all", action = ArgAction::SetTrue)]
        all: bool,
        #[arg(short = 'q', long = "quiet", action = ArgAction::SetTrue)]
        quiet: bool,
        #[arg(short = 'v', long = "verbose", action = ArgAction::SetTrue)]
        verbose: bool,
        #[arg(short = 'n', long = "dry-run", action = ArgAction::SetTrue)]
        dry_run: bool,
        #[arg(short = 'f', long = "force", action = ArgAction::SetTrue)]
        force: bool,
        #[arg(long = "set-upstream", action = ArgAction::SetTrue)]
        set_upstream: bool,
        #[arg(short = 'a', long = "append", action = ArgAction::SetTrue)]
        append: bool,
        #[arg(short = 'p', long = "prune", overrides_with = "no_prune", action = ArgAction::SetTrue)]
        prune: bool,
        #[arg(long = "no-prune", overrides_with = "prune", action = ArgAction::SetTrue)]
        no_prune: bool,
        #[arg(long = "prune-tags", action = ArgAction::SetTrue)]
        prune_tags: bool,
        #[arg(short = 't', long = "tags", action = ArgAction::SetTrue)]
        tags: bool,
        #[arg(long = "atomic", action = ArgAction::SetTrue)]
        atomic: bool,
        #[arg(long = "recurse-submodules", action = ArgAction::SetTrue)]
        recurse_submodules: bool,
        #[arg(long = "update-head-ok", action = ArgAction::SetTrue)]
        update_head_ok: bool,
        #[arg(long = "write-fetch-head", overrides_with = "no_write_fetch_head", action = ArgAction::SetTrue)]
        write_fetch_head: bool,
        #[arg(long = "no-write-fetch-head", overrides_with = "write_fetch_head", action = ArgAction::SetTrue)]
        no_write_fetch_head: bool,
        #[arg(long = "refmap", num_args = 0..=1, default_missing_value = "", require_equals = true)]
        refmap: Vec<String>,
        #[arg(long = "depth")]
        depth: Option<String>,
        #[arg(long = "negotiation-tip")]
        negotiation_tip: Vec<String>,
        remote: Option<String>,
        branch: Vec<String>,
    },
    Pull {
        #[arg(long = "ff-only", action = ArgAction::SetTrue)]
        ff_only: bool,
        #[arg(short = 's', long = "strategy")]
        strategies: Vec<String>,
        #[arg(long = "rebase", num_args = 0..=1, default_missing_value = "true")]
        rebase: Option<String>,
        #[arg(long = "no-rebase", action = ArgAction::SetTrue)]
        no_rebase: bool,
        remote: Option<String>,
        branch: Option<String>,
    },
    Push {
        #[arg(short = 'f', long = "force", action = ArgAction::SetTrue)]
        force: bool,
        #[arg(short = 'u', long = "set-upstream", action = ArgAction::SetTrue)]
        set_upstream: bool,
        remote: Option<String>,
        refspecs: Vec<String>,
    },
    LsFiles {
        #[arg(short = 'c', long = "cached", action = ArgAction::SetTrue)]
        cached: bool,
        #[arg(short = 'z', action = ArgAction::SetTrue)]
        zero: bool,
        #[arg(long = "full-name", action = ArgAction::SetTrue)]
        full_name: bool,
        #[arg(long = "error-unmatch", action = ArgAction::SetTrue)]
        error_unmatch: bool,
        #[arg(short = 't', action = ArgAction::SetTrue)]
        tagged: bool,
        #[arg(short = 'v', action = ArgAction::SetTrue)]
        lowercase_assume_valid: bool,
        #[arg(short = 'f', action = ArgAction::SetTrue)]
        fsmonitor_clean: bool,
        #[arg(long = "deduplicate", action = ArgAction::SetTrue)]
        deduplicate: bool,
        #[arg(long = "sparse", action = ArgAction::SetTrue)]
        sparse: bool,
        #[arg(long = "recurse-submodules", action = ArgAction::SetTrue)]
        recurse_submodules: bool,
        #[arg(long = "no-recurse-submodules", action = ArgAction::SetTrue)]
        no_recurse_submodules: bool,
        #[arg(long = "debug", action = ArgAction::SetTrue)]
        debug: bool,
        #[arg(long = "abbrev", num_args = 0..=1, require_equals = true, default_missing_value = "7")]
        abbrev: Option<usize>,
        #[arg(long = "eol", action = ArgAction::SetTrue)]
        eol: bool,
        #[arg(long = "format")]
        format: Option<String>,
        #[arg(long = "with-tree")]
        with_tree: Option<String>,
        #[arg(long = "resolve-undo", action = ArgAction::SetTrue)]
        resolve_undo: bool,
        #[arg(short = 's', long = "stage", action = ArgAction::SetTrue)]
        stage: bool,
        #[arg(short = 'u', long = "unmerged", action = ArgAction::SetTrue)]
        unmerged: bool,
        #[arg(short = 'd', long = "deleted", action = ArgAction::SetTrue)]
        deleted: bool,
        #[arg(short = 'm', long = "modified", action = ArgAction::SetTrue)]
        modified: bool,
        #[arg(short = 'o', long = "others", action = ArgAction::SetTrue)]
        others: bool,
        #[arg(short = 'k', long = "killed", action = ArgAction::SetTrue)]
        killed: bool,
        #[arg(long = "directory", action = ArgAction::SetTrue)]
        directory: bool,
        #[arg(long = "empty-directory", action = ArgAction::SetTrue)]
        empty_directory: bool,
        #[arg(short = 'i', long = "ignored", action = ArgAction::SetTrue)]
        ignored: bool,
        #[arg(short = 'x', long = "exclude")]
        excludes: Vec<String>,
        #[arg(short = 'X', long = "exclude-from", value_hint = ValueHint::FilePath)]
        exclude_from: Vec<PathBuf>,
        #[arg(long = "exclude-per-directory")]
        exclude_per_directory: Option<String>,
        #[arg(long = "exclude-standard", action = ArgAction::SetTrue)]
        exclude_standard: bool,
        #[arg(value_hint = ValueHint::AnyPath)]
        paths: Vec<PathBuf>,
    },
    Add {
        #[arg(short = 'A', long = "all", action = ArgAction::SetTrue)]
        all: bool,
        #[arg(short = 'f', long = "force", action = ArgAction::SetTrue)]
        force: bool,
        #[arg(short = 'u', long = "update", action = ArgAction::SetTrue)]
        update: bool,
        #[arg(short = 'N', long = "intent-to-add", action = ArgAction::SetTrue)]
        intent_to_add: bool,
        #[arg(long = "refresh", action = ArgAction::SetTrue)]
        refresh: bool,
        #[arg(short = 'v', long = "verbose", action = ArgAction::SetTrue)]
        verbose: bool,
        #[arg(long = "ignore-errors", action = ArgAction::SetTrue)]
        ignore_errors: bool,
        #[arg(long = "ignore-missing", action = ArgAction::SetTrue)]
        ignore_missing: bool,
        #[arg(long = "chmod")]
        chmod: Option<String>,
        #[arg(short = 'n', long = "dry-run", action = ArgAction::SetTrue)]
        dry_run: bool,
        #[arg(long = "pathspec-from-file", value_hint = ValueHint::FilePath)]
        pathspec_from_file: Option<PathBuf>,
        #[arg(long = "pathspec-file-nul", action = ArgAction::SetTrue)]
        pathspec_file_nul: bool,
        #[arg(value_hint = ValueHint::AnyPath)]
        paths: Vec<PathBuf>,
    },
    Stage {
        #[arg(short = 'A', long = "all", action = ArgAction::SetTrue)]
        all: bool,
        #[arg(short = 'f', long = "force", action = ArgAction::SetTrue)]
        force: bool,
        #[arg(short = 'u', long = "update", action = ArgAction::SetTrue)]
        update: bool,
        #[arg(short = 'N', long = "intent-to-add", action = ArgAction::SetTrue)]
        intent_to_add: bool,
        #[arg(long = "refresh", action = ArgAction::SetTrue)]
        refresh: bool,
        #[arg(short = 'v', long = "verbose", action = ArgAction::SetTrue)]
        verbose: bool,
        #[arg(long = "ignore-errors", action = ArgAction::SetTrue)]
        ignore_errors: bool,
        #[arg(long = "ignore-missing", action = ArgAction::SetTrue)]
        ignore_missing: bool,
        #[arg(long = "chmod")]
        chmod: Option<String>,
        #[arg(short = 'n', long = "dry-run", action = ArgAction::SetTrue)]
        dry_run: bool,
        #[arg(long = "pathspec-from-file", value_hint = ValueHint::FilePath)]
        pathspec_from_file: Option<PathBuf>,
        #[arg(long = "pathspec-file-nul", action = ArgAction::SetTrue)]
        pathspec_file_nul: bool,
        #[arg(value_hint = ValueHint::AnyPath)]
        paths: Vec<PathBuf>,
    },
    Rm {
        #[arg(short = 'f', long = "force", action = ArgAction::SetTrue)]
        force: bool,
        #[arg(short = 'n', long = "dry-run", action = ArgAction::SetTrue)]
        dry_run: bool,
        #[arg(short = 'q', long = "quiet", action = ArgAction::SetTrue)]
        quiet: bool,
        #[arg(short = 'r', action = ArgAction::SetTrue)]
        recursive: bool,
        #[arg(long = "cached", action = ArgAction::SetTrue)]
        cached: bool,
        #[arg(long = "ignore-unmatch", action = ArgAction::SetTrue)]
        ignore_unmatch: bool,
        #[arg(long = "pathspec-from-file", value_hint = ValueHint::FilePath)]
        pathspec_from_file: Option<PathBuf>,
        #[arg(long = "pathspec-file-nul", action = ArgAction::SetTrue)]
        pathspec_file_nul: bool,
        #[arg(value_hint = ValueHint::AnyPath)]
        paths: Vec<PathBuf>,
    },
    Mv {
        #[arg(short = 'f', long = "force", action = ArgAction::SetTrue)]
        force: bool,
        #[arg(value_hint = ValueHint::AnyPath)]
        paths: Vec<PathBuf>,
    },
    Commit {
        #[arg(short = 'a', long = "all", action = ArgAction::SetTrue)]
        all: bool,
        #[arg(short = 'o', long = "only", action = ArgAction::SetTrue)]
        only: bool,
        #[arg(short = 's', long = "signoff", action = ArgAction::SetTrue)]
        signoff: bool,
        #[arg(short = 'q', long = "quiet", action = ArgAction::SetTrue)]
        quiet: bool,
        #[arg(short = 'v', long = "verbose", action = ArgAction::Count)]
        verbose: u8,
        #[arg(short = 'n', long = "no-verify", action = ArgAction::SetTrue)]
        no_verify: bool,
        #[arg(long = "status", action = ArgAction::SetTrue, overrides_with = "no_status")]
        status: bool,
        #[arg(long = "no-status", action = ArgAction::SetTrue, overrides_with = "status")]
        no_status: bool,
        #[arg(long = "allow-empty", action = ArgAction::SetTrue)]
        allow_empty: bool,
        #[arg(long = "amend", action = ArgAction::SetTrue)]
        amend: bool,
        #[arg(short = 'e', long = "edit", action = ArgAction::SetTrue, overrides_with = "no_edit")]
        edit: bool,
        #[arg(long = "no-edit", action = ArgAction::SetTrue)]
        no_edit: bool,
        #[arg(long = "cleanup")]
        cleanup: Option<String>,
        #[arg(long = "no-cleanup", action = ArgAction::SetTrue)]
        no_cleanup: bool,
        #[arg(long = "allow-empty-message", action = ArgAction::SetTrue)]
        allow_empty_message: bool,
        #[arg(long = "author")]
        author: Option<String>,
        #[arg(long = "date")]
        date: Option<String>,
        #[arg(long = "squash")]
        squash: Option<String>,
        #[arg(short = 't', long = "template", value_hint = ValueHint::FilePath)]
        template: Option<PathBuf>,
        #[arg(long = "reset-author", action = ArgAction::SetTrue)]
        reset_author: bool,
        #[arg(short = 'C', long = "reuse-message")]
        reuse_message: Option<String>,
        #[arg(short = 'c', long = "reedit-message")]
        reedit_message: Option<String>,
        #[arg(long = "fixup")]
        fixup: Option<String>,
        #[arg(short = 'F', long = "file", value_hint = ValueHint::FilePath)]
        message_file: Option<PathBuf>,
        #[arg(short = 'm', long = "message")]
        messages: Vec<String>,
        #[arg(long = "trailer")]
        trailers: Vec<String>,
        #[arg(value_hint = ValueHint::AnyPath)]
        paths: Vec<PathBuf>,
    },
    #[command(name = "citool", disable_help_flag = true)]
    Citool {
        #[arg(long = "amend", action = ArgAction::SetTrue)]
        amend: bool,
        #[arg(long = "nocommit", action = ArgAction::SetTrue)]
        nocommit: bool,
        #[arg(short = 'F', long = "file", value_hint = ValueHint::FilePath)]
        message_file: Option<PathBuf>,
        #[arg(short = 'm')]
        messages: Vec<String>,
    },
    #[command(name = "gui", disable_help_flag = true)]
    Gui {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    #[command(name = "gitk", disable_help_flag = true)]
    Gitk {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    #[command(name = "gitweb", disable_help_flag = true)]
    Gitweb {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    #[command(
        name = "scalar",
        disable_help_flag = true,
        disable_help_subcommand = true
    )]
    Scalar {
        #[arg(short = 'C', value_hint = ValueHint::DirPath)]
        directories: Vec<PathBuf>,
        #[arg(short = 'c')]
        configs: Vec<String>,
        #[arg(short = 'h', long = "help", action = ArgAction::SetTrue)]
        help: bool,
        #[command(subcommand)]
        command: Option<ScalarCommand>,
    },
    WriteTree {
        #[arg(long = "prefix")]
        prefix: Option<String>,
        #[arg(long = "missing-ok", action = ArgAction::SetTrue)]
        missing_ok: bool,
    },
    CommitTree {
        tree: String,
        #[arg(short = 'p')]
        parents: Vec<String>,
        #[arg(short = 'm')]
        messages: Vec<String>,
    },
    Mktree {
        #[arg(short = 'z', action = ArgAction::SetTrue)]
        nul_terminated: bool,
        #[arg(long = "missing", action = ArgAction::SetTrue)]
        missing: bool,
        #[arg(long = "batch", action = ArgAction::SetTrue)]
        batch: bool,
    },
    Mktag {
        #[arg(long = "strict", action = ArgAction::SetTrue)]
        strict: bool,
    },
    PackRefs {
        #[arg(long = "all", action = ArgAction::SetTrue)]
        all: bool,
        #[arg(long = "prune", overrides_with = "no_prune", action = ArgAction::SetTrue)]
        prune: bool,
        #[arg(long = "no-prune", overrides_with = "prune", action = ArgAction::SetTrue)]
        no_prune: bool,
    },
    PrunePacked {
        #[arg(short = 'n', long = "dry-run", action = ArgAction::SetTrue)]
        dry_run: bool,
        #[arg(short = 'q', long = "quiet", action = ArgAction::SetTrue)]
        quiet: bool,
    },
    Repack {
        #[arg(short = 'a', action = ArgAction::SetTrue)]
        all: bool,
        #[arg(short = 'A', action = ArgAction::SetTrue)]
        all_and_loosen_unreachable: bool,
        #[arg(short = 'd', action = ArgAction::SetTrue)]
        delete_redundant: bool,
        #[arg(short = 'q', long = "quiet", action = ArgAction::SetTrue)]
        quiet: bool,
        #[arg(short = 'n', action = ArgAction::SetTrue)]
        no_update_server_info: bool,
        #[arg(short = 'f', action = ArgAction::SetTrue)]
        no_reuse_delta: bool,
        #[arg(short = 'F', action = ArgAction::SetTrue)]
        no_reuse_object: bool,
        #[arg(short = 'l', long = "local", action = ArgAction::SetTrue)]
        local: bool,
        #[arg(short = 'b', long = "write-bitmap-index", action = ArgAction::SetTrue)]
        write_bitmap_index: bool,
        #[arg(long = "no-write-bitmap-index", action = ArgAction::SetTrue)]
        no_write_bitmap_index: bool,
        #[arg(short = 'm', long = "write-midx", action = ArgAction::SetTrue)]
        write_midx: bool,
        #[arg(long = "no-write-midx", action = ArgAction::SetTrue)]
        no_write_midx: bool,
        #[arg(long = "window")]
        window: Option<usize>,
        #[arg(long = "depth")]
        depth: Option<usize>,
        #[arg(long = "threads")]
        threads: Option<usize>,
        #[arg(long = "keep-pack")]
        keep_pack: Vec<String>,
    },
    Gc {
        #[arg(long = "prune", num_args = 0..=1, default_missing_value = "now")]
        prune: Option<String>,
        #[arg(long = "no-prune", action = ArgAction::SetTrue)]
        no_prune: bool,
        #[arg(long = "auto", action = ArgAction::SetTrue)]
        auto: bool,
        #[arg(long = "aggressive", action = ArgAction::SetTrue)]
        aggressive: bool,
        #[arg(short = 'q', long = "quiet", action = ArgAction::SetTrue)]
        quiet: bool,
    },
    Maintenance {
        operation: String,
        #[arg(long = "auto", action = ArgAction::SetTrue)]
        auto: bool,
        #[arg(long = "schedule")]
        schedule: Option<String>,
        #[arg(long = "scheduler")]
        scheduler: Option<String>,
        #[arg(long = "config-file", value_hint = ValueHint::FilePath)]
        config_file: Option<PathBuf>,
        #[arg(short = 'f', long = "force", action = ArgAction::SetTrue)]
        force: bool,
        #[arg(short = 'q', long = "quiet", action = ArgAction::SetTrue)]
        quiet: bool,
        #[arg(long = "task")]
        tasks: Vec<String>,
    },
    Notes {
        #[arg(allow_hyphen_values = true, trailing_var_arg = true)]
        args: Vec<String>,
    },
    Prune {
        #[arg(allow_hyphen_values = true, trailing_var_arg = true)]
        args: Vec<String>,
    },
    ReadTree {
        #[arg(long = "empty", action = ArgAction::SetTrue)]
        empty: bool,
        #[arg(short = 'm', action = ArgAction::SetTrue)]
        merge: bool,
        #[arg(long = "prefix")]
        prefix: Option<String>,
        treeish: Option<String>,
    },
    Checkout {
        #[arg(short = 'f', long = "force", action = ArgAction::SetTrue)]
        force: bool,
        #[arg(short = 'q', long = "quiet", action = ArgAction::SetTrue)]
        quiet: bool,
        #[arg(long = "no-progress", action = ArgAction::SetTrue)]
        no_progress: bool,
        #[arg(long = "detach", action = ArgAction::SetTrue)]
        detach: bool,
        #[arg(long = "recurse-submodules", action = ArgAction::SetTrue)]
        recurse_submodules: bool,
        #[arg(long = "no-recurse-submodules", action = ArgAction::SetTrue)]
        no_recurse_submodules: bool,
        #[arg(short = 'b')]
        create: Option<String>,
        #[arg(short = 'B')]
        reset_create: Option<String>,
        #[arg(short = 'l', action = ArgAction::SetTrue)]
        create_reflog: bool,
        #[arg(long = "orphan")]
        orphan: Option<String>,
        #[arg(value_hint = ValueHint::AnyPath, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    CheckoutIndex {
        #[arg(short = 'a', long = "all", action = ArgAction::SetTrue)]
        all: bool,
        #[arg(short = 'f', long = "force", action = ArgAction::SetTrue)]
        force: bool,
        #[arg(short = 'q', long = "quiet", action = ArgAction::SetTrue)]
        quiet: bool,
        #[arg(long = "stdin", action = ArgAction::SetTrue)]
        stdin: bool,
        #[arg(long = "prefix")]
        prefix: Option<PathBuf>,
        #[arg(value_hint = ValueHint::AnyPath)]
        paths: Vec<PathBuf>,
    },
    Switch {
        #[arg(short = 'f', long = "force", action = ArgAction::SetTrue)]
        force: bool,
        #[arg(long = "discard-changes", action = ArgAction::SetTrue)]
        discard_changes: bool,
        #[arg(short = 'c', long = "create")]
        create: Option<String>,
        #[arg(long = "orphan")]
        orphan: Option<String>,
        #[arg(long = "detach", action = ArgAction::SetTrue)]
        detach: bool,
        target: Option<String>,
    },
    Restore {
        #[arg(short = 's', long = "source")]
        source: Option<String>,
        #[arg(long = "staged", action = ArgAction::SetTrue)]
        staged: bool,
        #[arg(short = 'W', long = "worktree", action = ArgAction::SetTrue)]
        worktree: bool,
        #[arg(value_hint = ValueHint::AnyPath)]
        paths: Vec<PathBuf>,
    },
    Diff {
        #[arg(long = "no-index", action = ArgAction::SetTrue)]
        no_index: bool,
        #[arg(short = 'r', action = ArgAction::SetTrue)]
        recursive: bool,
        #[arg(short = 'z', action = ArgAction::SetTrue)]
        nul_terminated: bool,
        #[arg(long = "cached", alias = "staged", action = ArgAction::SetTrue)]
        cached: bool,
        #[arg(short = 'R', long = "reverse", action = ArgAction::SetTrue)]
        reverse: bool,
        #[arg(long = "check", action = ArgAction::SetTrue)]
        check: bool,
        #[arg(short = 'p', short_alias = 'u', long = "patch", action = ArgAction::SetTrue)]
        patch: bool,
        #[arg(long = "patch-with-raw", action = ArgAction::SetTrue)]
        patch_with_raw: bool,
        #[arg(long = "patch-with-stat", action = ArgAction::SetTrue)]
        patch_with_stat: bool,
        #[arg(short = 's', long = "no-patch", action = ArgAction::SetTrue)]
        no_patch: bool,
        #[arg(long = "binary", action = ArgAction::SetTrue)]
        binary: bool,
        #[arg(long = "stat", action = ArgAction::SetTrue)]
        stat: bool,
        #[arg(long = "compact-summary", action = ArgAction::SetTrue)]
        compact_summary: bool,
        #[arg(long = "numstat", action = ArgAction::SetTrue)]
        numstat: bool,
        #[arg(long = "shortstat", action = ArgAction::SetTrue)]
        shortstat: bool,
        #[arg(long = "dirstat", num_args = 0..=1, require_equals = true, default_missing_value = "")]
        dirstat: Option<String>,
        #[arg(long = "dirstat-by-file", action = ArgAction::SetTrue)]
        dirstat_by_file: bool,
        #[arg(long = "raw", action = ArgAction::SetTrue)]
        raw: bool,
        #[arg(long = "summary", action = ArgAction::SetTrue)]
        summary: bool,
        #[arg(long = "name-status", action = ArgAction::SetTrue)]
        name_status: bool,
        #[arg(long = "name-only", action = ArgAction::SetTrue)]
        name_only: bool,
        #[arg(short = 'M', long = "find-renames", num_args = 0..=1, default_missing_value = "")]
        find_renames: Option<String>,
        #[arg(short = 'B', long = "break-rewrites", num_args = 0..=1, default_missing_value = "")]
        break_rewrites: Option<String>,
        #[arg(short = 'D', long = "irreversible-delete", action = ArgAction::SetTrue)]
        irreversible_delete: bool,
        #[arg(long = "submodule", num_args = 0..=1, require_equals = true, default_missing_value = "log")]
        submodule: Option<String>,
        #[arg(long = "ignore-submodules", num_args = 0..=1, require_equals = true, default_missing_value = "all")]
        ignore_submodules: Option<String>,
        #[arg(short = 'C', long = "find-copies", num_args = 0..=1, default_missing_value = "")]
        find_copies: Option<String>,
        #[arg(long = "find-copies-harder", action = ArgAction::SetTrue)]
        find_copies_harder: bool,
        #[arg(long = "no-renames", action = ArgAction::SetTrue)]
        no_renames: bool,
        #[arg(long = "cc", action = ArgAction::SetTrue)]
        dense_combined: bool,
        #[arg(short = 'S')]
        pickaxe_string: Option<String>,
        #[arg(short = 'G')]
        pickaxe_regex: Option<String>,
        #[arg(long = "pickaxe-regex", action = ArgAction::SetTrue)]
        pickaxe_regex_mode: bool,
        #[arg(long = "pickaxe-all", action = ArgAction::SetTrue)]
        pickaxe_all: bool,
        #[arg(short = 'O', value_hint = ValueHint::FilePath)]
        order_file: Option<PathBuf>,
        #[arg(long = "skip-to")]
        skip_to: Option<String>,
        #[arg(long = "rotate-to")]
        rotate_to: Option<String>,
        #[arg(long = "diff-filter")]
        diff_filter: Option<String>,
        #[arg(long = "word-diff", num_args = 0..=1, default_missing_value = "plain")]
        word_diff: Option<String>,
        #[arg(long = "abbrev", num_args = 0..=1, require_equals = true, default_missing_value = "")]
        abbrev: Option<String>,
        #[arg(long = "no-abbrev", action = ArgAction::SetTrue)]
        no_abbrev: bool,
        #[arg(long = "full-index", action = ArgAction::SetTrue)]
        full_index: bool,
        #[arg(long = "no-full-index", action = ArgAction::SetTrue)]
        no_full_index: bool,
        #[arg(long = "no-prefix", action = ArgAction::SetTrue)]
        no_prefix: bool,
        #[arg(long = "default-prefix", action = ArgAction::SetTrue)]
        default_prefix: bool,
        #[arg(long = "src-prefix")]
        src_prefix: Option<String>,
        #[arg(long = "dst-prefix")]
        dst_prefix: Option<String>,
        #[arg(long = "relative", num_args = 0..=1, default_missing_value = "")]
        relative: Option<String>,
        #[arg(long = "no-relative", action = ArgAction::SetTrue)]
        no_relative: bool,
        #[arg(short = 'U', long = "unified", num_args = 0..=1, default_missing_value = "3")]
        unified: Option<String>,
        #[arg(long = "inter-hunk-context")]
        inter_hunk_context: Option<String>,
        #[arg(long = "minimal", action = ArgAction::SetTrue)]
        minimal: bool,
        #[arg(long = "patience", action = ArgAction::SetTrue)]
        patience: bool,
        #[arg(long = "histogram", action = ArgAction::SetTrue)]
        histogram: bool,
        #[arg(long = "diff-algorithm")]
        diff_algorithm: Option<String>,
        #[arg(long = "anchored")]
        anchored: Vec<String>,
        #[arg(long = "output-indicator-new")]
        output_indicator_new: Option<String>,
        #[arg(long = "output-indicator-old")]
        output_indicator_old: Option<String>,
        #[arg(long = "output-indicator-context")]
        output_indicator_context: Option<String>,
        #[arg(long = "line-prefix")]
        line_prefix: Option<String>,
        #[arg(long = "ignore-space-at-eol", action = ArgAction::SetTrue)]
        ignore_space_at_eol: bool,
        #[arg(long = "ignore-cr-at-eol", action = ArgAction::SetTrue)]
        ignore_cr_at_eol: bool,
        #[arg(short = 'b', long = "ignore-space-change", action = ArgAction::SetTrue)]
        ignore_space_change: bool,
        #[arg(short = 'w', long = "ignore-all-space", action = ArgAction::SetTrue)]
        ignore_all_space: bool,
        #[arg(long = "ignore-blank-lines", action = ArgAction::SetTrue)]
        ignore_blank_lines: bool,
        #[arg(short = 'I', long = "ignore-matching-lines")]
        ignore_matching_lines: Vec<String>,
        #[arg(long = "no-ext-diff", action = ArgAction::SetTrue)]
        no_ext_diff: bool,
        #[arg(long = "no-textconv", action = ArgAction::SetTrue)]
        no_textconv: bool,
        #[arg(short = 'a', long = "text", action = ArgAction::SetTrue)]
        text: bool,
        #[arg(long = "color", num_args = 0..=1, default_missing_value = "always")]
        color: Option<String>,
        #[arg(long = "no-color", action = ArgAction::SetTrue)]
        no_color: bool,
        #[arg(long = "no-color-moved", action = ArgAction::SetTrue)]
        no_color_moved: bool,
        #[arg(long = "no-color-moved-ws", action = ArgAction::SetTrue)]
        no_color_moved_ws: bool,
        #[arg(long = "quiet", action = ArgAction::SetTrue)]
        quiet: bool,
        #[arg(long = "exit-code", action = ArgAction::SetTrue)]
        exit_code: bool,
        #[arg(value_hint = ValueHint::AnyPath)]
        paths: Vec<PathBuf>,
    },
    Difftool {
        #[arg(long = "cached", alias = "staged", action = ArgAction::SetTrue)]
        cached: bool,
        #[arg(short = 't', long = "tool")]
        tool: Option<String>,
        #[arg(short = 'x', long = "extcmd")]
        extcmd: Option<String>,
        #[arg(short = 'y', long = "no-prompt", action = ArgAction::SetTrue)]
        no_prompt: bool,
        #[arg(long = "prompt", action = ArgAction::SetTrue)]
        prompt: bool,
        #[arg(value_hint = ValueHint::AnyPath)]
        paths: Vec<PathBuf>,
    },
    DiffFiles {
        #[arg(short = 'z', action = ArgAction::SetTrue)]
        nul_terminated: bool,
        #[arg(short = 'p', short_alias = 'u', long = "patch", action = ArgAction::SetTrue)]
        patch: bool,
        #[arg(long = "patch-with-raw", action = ArgAction::SetTrue)]
        patch_with_raw: bool,
        #[arg(long = "patch-with-stat", action = ArgAction::SetTrue)]
        patch_with_stat: bool,
        #[arg(short = 's', long = "no-patch", action = ArgAction::SetTrue)]
        no_patch: bool,
        #[arg(long = "binary", action = ArgAction::SetTrue)]
        binary: bool,
        #[arg(long = "stat", action = ArgAction::SetTrue)]
        stat: bool,
        #[arg(long = "compact-summary", action = ArgAction::SetTrue)]
        compact_summary: bool,
        #[arg(long = "numstat", action = ArgAction::SetTrue)]
        numstat: bool,
        #[arg(long = "shortstat", action = ArgAction::SetTrue)]
        shortstat: bool,
        #[arg(long = "raw", action = ArgAction::SetTrue)]
        raw: bool,
        #[arg(long = "summary", action = ArgAction::SetTrue)]
        summary: bool,
        #[arg(long = "name-status", action = ArgAction::SetTrue)]
        name_status: bool,
        #[arg(long = "name-only", action = ArgAction::SetTrue)]
        name_only: bool,
        #[arg(short = 'M', long = "find-renames", num_args = 0..=1, default_missing_value = "")]
        find_renames: Option<String>,
        #[arg(short = 'B', long = "break-rewrites", num_args = 0..=1, default_missing_value = "")]
        break_rewrites: Option<String>,
        #[arg(short = 'D', long = "irreversible-delete", action = ArgAction::SetTrue)]
        irreversible_delete: bool,
        #[arg(long = "submodule", num_args = 0..=1, require_equals = true, default_missing_value = "log")]
        submodule: Option<String>,
        #[arg(long = "ignore-submodules", num_args = 0..=1, require_equals = true, default_missing_value = "all")]
        ignore_submodules: Option<String>,
        #[arg(short = 'C', long = "find-copies", num_args = 0..=1, default_missing_value = "")]
        find_copies: Option<String>,
        #[arg(long = "find-copies-harder", action = ArgAction::SetTrue)]
        find_copies_harder: bool,
        #[arg(short = 'm', action = ArgAction::SetTrue)]
        merge: bool,
        #[arg(short = 'R', long = "reverse", action = ArgAction::SetTrue)]
        reverse: bool,
        #[arg(short = 'S')]
        pickaxe_string: Option<String>,
        #[arg(short = 'G')]
        pickaxe_regex: Option<String>,
        #[arg(long = "pickaxe-regex", action = ArgAction::SetTrue)]
        pickaxe_regex_mode: bool,
        #[arg(long = "pickaxe-all", action = ArgAction::SetTrue)]
        pickaxe_all: bool,
        #[arg(short = 'O', value_hint = ValueHint::FilePath)]
        order_file: Option<PathBuf>,
        #[arg(long = "skip-to")]
        skip_to: Option<String>,
        #[arg(long = "rotate-to")]
        rotate_to: Option<String>,
        #[arg(long = "diff-filter")]
        diff_filter: Option<String>,
        #[arg(long = "word-diff", num_args = 0..=1, default_missing_value = "plain")]
        word_diff: Option<String>,
        #[arg(long = "abbrev", num_args = 0..=1, require_equals = true, default_missing_value = "")]
        abbrev: Option<String>,
        #[arg(long = "no-abbrev", action = ArgAction::SetTrue)]
        no_abbrev: bool,
        #[arg(long = "full-index", action = ArgAction::SetTrue)]
        full_index: bool,
        #[arg(long = "no-full-index", action = ArgAction::SetTrue)]
        no_full_index: bool,
        #[arg(long = "no-prefix", action = ArgAction::SetTrue)]
        no_prefix: bool,
        #[arg(long = "default-prefix", action = ArgAction::SetTrue)]
        default_prefix: bool,
        #[arg(long = "src-prefix")]
        src_prefix: Option<String>,
        #[arg(long = "dst-prefix")]
        dst_prefix: Option<String>,
        #[arg(long = "relative", num_args = 0..=1, default_missing_value = "")]
        relative: Option<String>,
        #[arg(long = "no-relative", action = ArgAction::SetTrue)]
        no_relative: bool,
        #[arg(short = 'U', long = "unified", num_args = 0..=1, default_missing_value = "3")]
        unified: Option<String>,
        #[arg(long = "inter-hunk-context")]
        inter_hunk_context: Option<String>,
        #[arg(long = "minimal", action = ArgAction::SetTrue)]
        minimal: bool,
        #[arg(long = "patience", action = ArgAction::SetTrue)]
        patience: bool,
        #[arg(long = "histogram", action = ArgAction::SetTrue)]
        histogram: bool,
        #[arg(long = "diff-algorithm")]
        diff_algorithm: Option<String>,
        #[arg(long = "anchored")]
        anchored: Vec<String>,
        #[arg(long = "output-indicator-new")]
        output_indicator_new: Option<String>,
        #[arg(long = "output-indicator-old")]
        output_indicator_old: Option<String>,
        #[arg(long = "output-indicator-context")]
        output_indicator_context: Option<String>,
        #[arg(long = "ignore-space-at-eol", action = ArgAction::SetTrue)]
        ignore_space_at_eol: bool,
        #[arg(long = "ignore-cr-at-eol", action = ArgAction::SetTrue)]
        ignore_cr_at_eol: bool,
        #[arg(short = 'b', long = "ignore-space-change", action = ArgAction::SetTrue)]
        ignore_space_change: bool,
        #[arg(short = 'w', long = "ignore-all-space", action = ArgAction::SetTrue)]
        ignore_all_space: bool,
        #[arg(long = "ignore-blank-lines", action = ArgAction::SetTrue)]
        ignore_blank_lines: bool,
        #[arg(short = 'I', long = "ignore-matching-lines")]
        ignore_matching_lines: Vec<String>,
        #[arg(short = 'a', long = "text", action = ArgAction::SetTrue)]
        text: bool,
        #[arg(long = "no-ext-diff", action = ArgAction::SetTrue)]
        no_ext_diff: bool,
        #[arg(long = "no-textconv", action = ArgAction::SetTrue)]
        no_textconv: bool,
        #[arg(long = "color", num_args = 0..=1, default_missing_value = "always")]
        color: Option<String>,
        #[arg(long = "no-color", action = ArgAction::SetTrue)]
        no_color: bool,
        #[arg(long = "no-color-moved", action = ArgAction::SetTrue)]
        no_color_moved: bool,
        #[arg(long = "no-color-moved-ws", action = ArgAction::SetTrue)]
        no_color_moved_ws: bool,
        #[arg(long = "quiet", action = ArgAction::SetTrue)]
        quiet: bool,
        #[arg(long = "exit-code", action = ArgAction::SetTrue)]
        exit_code: bool,
        #[arg(short = 'q', action = ArgAction::SetTrue)]
        quiet_unmerged: bool,
        #[arg(value_hint = ValueHint::AnyPath)]
        paths: Vec<PathBuf>,
    },
    DiffIndex {
        #[arg(short = 'z', action = ArgAction::SetTrue)]
        nul_terminated: bool,
        #[arg(long = "cached", action = ArgAction::SetTrue)]
        cached: bool,
        #[arg(short = 'p', short_alias = 'u', long = "patch", action = ArgAction::SetTrue)]
        patch: bool,
        #[arg(long = "patch-with-raw", action = ArgAction::SetTrue)]
        patch_with_raw: bool,
        #[arg(long = "patch-with-stat", action = ArgAction::SetTrue)]
        patch_with_stat: bool,
        #[arg(short = 's', long = "no-patch", action = ArgAction::SetTrue)]
        no_patch: bool,
        #[arg(long = "binary", action = ArgAction::SetTrue)]
        binary: bool,
        #[arg(long = "stat", action = ArgAction::SetTrue)]
        stat: bool,
        #[arg(long = "compact-summary", action = ArgAction::SetTrue)]
        compact_summary: bool,
        #[arg(long = "numstat", action = ArgAction::SetTrue)]
        numstat: bool,
        #[arg(long = "shortstat", action = ArgAction::SetTrue)]
        shortstat: bool,
        #[arg(long = "raw", action = ArgAction::SetTrue)]
        raw: bool,
        #[arg(long = "summary", action = ArgAction::SetTrue)]
        summary: bool,
        #[arg(long = "name-status", action = ArgAction::SetTrue)]
        name_status: bool,
        #[arg(long = "name-only", action = ArgAction::SetTrue)]
        name_only: bool,
        #[arg(short = 'M', long = "find-renames", num_args = 0..=1, default_missing_value = "")]
        find_renames: Option<String>,
        #[arg(short = 'B', long = "break-rewrites", num_args = 0..=1, default_missing_value = "")]
        break_rewrites: Option<String>,
        #[arg(short = 'D', long = "irreversible-delete", action = ArgAction::SetTrue)]
        irreversible_delete: bool,
        #[arg(long = "submodule", num_args = 0..=1, require_equals = true, default_missing_value = "log")]
        submodule: Option<String>,
        #[arg(long = "ignore-submodules", num_args = 0..=1, require_equals = true, default_missing_value = "all")]
        ignore_submodules: Option<String>,
        #[arg(short = 'C', long = "find-copies", num_args = 0..=1, default_missing_value = "")]
        find_copies: Option<String>,
        #[arg(long = "find-copies-harder", action = ArgAction::SetTrue)]
        find_copies_harder: bool,
        #[arg(short = 'm', action = ArgAction::SetTrue)]
        merge: bool,
        #[arg(short = 'R', long = "reverse", action = ArgAction::SetTrue)]
        reverse: bool,
        #[arg(long = "root", action = ArgAction::SetTrue)]
        root: bool,
        #[arg(short = 'S')]
        pickaxe_string: Option<String>,
        #[arg(short = 'G')]
        pickaxe_regex: Option<String>,
        #[arg(long = "pickaxe-regex", action = ArgAction::SetTrue)]
        pickaxe_regex_mode: bool,
        #[arg(long = "pickaxe-all", action = ArgAction::SetTrue)]
        pickaxe_all: bool,
        #[arg(short = 'O', value_hint = ValueHint::FilePath)]
        order_file: Option<PathBuf>,
        #[arg(long = "skip-to")]
        skip_to: Option<String>,
        #[arg(long = "rotate-to")]
        rotate_to: Option<String>,
        #[arg(long = "diff-filter")]
        diff_filter: Option<String>,
        #[arg(long = "word-diff", num_args = 0..=1, default_missing_value = "plain")]
        word_diff: Option<String>,
        #[arg(long = "abbrev", num_args = 0..=1, require_equals = true, default_missing_value = "")]
        abbrev: Option<String>,
        #[arg(long = "no-abbrev", action = ArgAction::SetTrue)]
        no_abbrev: bool,
        #[arg(long = "full-index", action = ArgAction::SetTrue)]
        full_index: bool,
        #[arg(long = "no-full-index", action = ArgAction::SetTrue)]
        no_full_index: bool,
        #[arg(long = "no-prefix", action = ArgAction::SetTrue)]
        no_prefix: bool,
        #[arg(long = "default-prefix", action = ArgAction::SetTrue)]
        default_prefix: bool,
        #[arg(long = "src-prefix")]
        src_prefix: Option<String>,
        #[arg(long = "dst-prefix")]
        dst_prefix: Option<String>,
        #[arg(long = "relative", num_args = 0..=1, default_missing_value = "")]
        relative: Option<String>,
        #[arg(long = "no-relative", action = ArgAction::SetTrue)]
        no_relative: bool,
        #[arg(short = 'U', long = "unified", num_args = 0..=1, default_missing_value = "3")]
        unified: Option<String>,
        #[arg(long = "inter-hunk-context")]
        inter_hunk_context: Option<String>,
        #[arg(long = "minimal", action = ArgAction::SetTrue)]
        minimal: bool,
        #[arg(long = "patience", action = ArgAction::SetTrue)]
        patience: bool,
        #[arg(long = "histogram", action = ArgAction::SetTrue)]
        histogram: bool,
        #[arg(long = "diff-algorithm")]
        diff_algorithm: Option<String>,
        #[arg(long = "anchored")]
        anchored: Vec<String>,
        #[arg(long = "output-indicator-new")]
        output_indicator_new: Option<String>,
        #[arg(long = "output-indicator-old")]
        output_indicator_old: Option<String>,
        #[arg(long = "output-indicator-context")]
        output_indicator_context: Option<String>,
        #[arg(long = "ignore-space-at-eol", action = ArgAction::SetTrue)]
        ignore_space_at_eol: bool,
        #[arg(long = "ignore-cr-at-eol", action = ArgAction::SetTrue)]
        ignore_cr_at_eol: bool,
        #[arg(short = 'b', long = "ignore-space-change", action = ArgAction::SetTrue)]
        ignore_space_change: bool,
        #[arg(short = 'w', long = "ignore-all-space", action = ArgAction::SetTrue)]
        ignore_all_space: bool,
        #[arg(long = "ignore-blank-lines", action = ArgAction::SetTrue)]
        ignore_blank_lines: bool,
        #[arg(short = 'I', long = "ignore-matching-lines")]
        ignore_matching_lines: Vec<String>,
        #[arg(short = 'a', long = "text", action = ArgAction::SetTrue)]
        text: bool,
        #[arg(long = "no-ext-diff", action = ArgAction::SetTrue)]
        no_ext_diff: bool,
        #[arg(long = "no-textconv", action = ArgAction::SetTrue)]
        no_textconv: bool,
        #[arg(long = "color", num_args = 0..=1, default_missing_value = "always")]
        color: Option<String>,
        #[arg(long = "no-color", action = ArgAction::SetTrue)]
        no_color: bool,
        #[arg(long = "no-color-moved", action = ArgAction::SetTrue)]
        no_color_moved: bool,
        #[arg(long = "no-color-moved-ws", action = ArgAction::SetTrue)]
        no_color_moved_ws: bool,
        #[arg(long = "quiet", action = ArgAction::SetTrue)]
        quiet: bool,
        #[arg(long = "exit-code", action = ArgAction::SetTrue)]
        exit_code: bool,
        treeish: String,
        #[arg(value_hint = ValueHint::AnyPath)]
        paths: Vec<PathBuf>,
    },
    DiffTree {
        #[arg(long = "stdin", action = ArgAction::SetTrue)]
        stdin: bool,
        #[arg(short = 'z', action = ArgAction::SetTrue)]
        nul_terminated: bool,
        #[arg(short = 'r', action = ArgAction::SetTrue)]
        recursive: bool,
        #[arg(short = 'p', short_alias = 'u', long = "patch", action = ArgAction::SetTrue)]
        patch: bool,
        #[arg(long = "patch-with-raw", action = ArgAction::SetTrue)]
        patch_with_raw: bool,
        #[arg(long = "patch-with-stat", action = ArgAction::SetTrue)]
        patch_with_stat: bool,
        #[arg(short = 's', long = "no-patch", action = ArgAction::SetTrue)]
        no_patch: bool,
        #[arg(long = "binary", action = ArgAction::SetTrue)]
        binary: bool,
        #[arg(long = "stat", action = ArgAction::SetTrue)]
        stat: bool,
        #[arg(long = "compact-summary", action = ArgAction::SetTrue)]
        compact_summary: bool,
        #[arg(long = "numstat", action = ArgAction::SetTrue)]
        numstat: bool,
        #[arg(long = "shortstat", action = ArgAction::SetTrue)]
        shortstat: bool,
        #[arg(long = "raw", action = ArgAction::SetTrue)]
        raw: bool,
        #[arg(long = "summary", action = ArgAction::SetTrue)]
        summary: bool,
        #[arg(long = "name-status", action = ArgAction::SetTrue)]
        name_status: bool,
        #[arg(long = "name-only", action = ArgAction::SetTrue)]
        name_only: bool,
        #[arg(short = 'M', long = "find-renames", num_args = 0..=1, default_missing_value = "")]
        find_renames: Option<String>,
        #[arg(short = 'B', long = "break-rewrites", num_args = 0..=1, default_missing_value = "")]
        break_rewrites: Option<String>,
        #[arg(short = 'D', long = "irreversible-delete", action = ArgAction::SetTrue)]
        irreversible_delete: bool,
        #[arg(long = "submodule", num_args = 0..=1, require_equals = true, default_missing_value = "log")]
        submodule: Option<String>,
        #[arg(long = "ignore-submodules", num_args = 0..=1, require_equals = true, default_missing_value = "all")]
        ignore_submodules: Option<String>,
        #[arg(short = 'C', long = "find-copies", num_args = 0..=1, default_missing_value = "")]
        find_copies: Option<String>,
        #[arg(long = "find-copies-harder", action = ArgAction::SetTrue)]
        find_copies_harder: bool,
        #[arg(short = 'm', action = ArgAction::SetTrue)]
        merge: bool,
        #[arg(short = 'c', action = ArgAction::SetTrue)]
        combined: bool,
        #[arg(long = "cc", action = ArgAction::SetTrue)]
        dense_combined: bool,
        #[arg(short = 'R', long = "reverse", action = ArgAction::SetTrue)]
        reverse: bool,
        #[arg(long = "root", action = ArgAction::SetTrue)]
        root: bool,
        #[arg(short = 'S')]
        pickaxe_string: Option<String>,
        #[arg(short = 'G')]
        pickaxe_regex: Option<String>,
        #[arg(long = "pickaxe-regex", action = ArgAction::SetTrue)]
        pickaxe_regex_mode: bool,
        #[arg(long = "pickaxe-all", action = ArgAction::SetTrue)]
        pickaxe_all: bool,
        #[arg(short = 'O', value_hint = ValueHint::FilePath)]
        order_file: Option<PathBuf>,
        #[arg(long = "skip-to")]
        skip_to: Option<String>,
        #[arg(long = "rotate-to")]
        rotate_to: Option<String>,
        #[arg(long = "diff-filter")]
        diff_filter: Option<String>,
        #[arg(long = "word-diff", num_args = 0..=1, default_missing_value = "plain")]
        word_diff: Option<String>,
        #[arg(long = "abbrev", num_args = 0..=1, require_equals = true, default_missing_value = "")]
        abbrev: Option<String>,
        #[arg(long = "no-abbrev", action = ArgAction::SetTrue)]
        no_abbrev: bool,
        #[arg(long = "full-index", action = ArgAction::SetTrue)]
        full_index: bool,
        #[arg(long = "no-full-index", action = ArgAction::SetTrue)]
        no_full_index: bool,
        #[arg(long = "no-prefix", action = ArgAction::SetTrue)]
        no_prefix: bool,
        #[arg(long = "default-prefix", action = ArgAction::SetTrue)]
        default_prefix: bool,
        #[arg(long = "src-prefix")]
        src_prefix: Option<String>,
        #[arg(long = "dst-prefix")]
        dst_prefix: Option<String>,
        #[arg(long = "relative", num_args = 0..=1, default_missing_value = "")]
        relative: Option<String>,
        #[arg(long = "no-relative", action = ArgAction::SetTrue)]
        no_relative: bool,
        #[arg(short = 'U', long = "unified", num_args = 0..=1, default_missing_value = "3")]
        unified: Option<String>,
        #[arg(long = "inter-hunk-context")]
        inter_hunk_context: Option<String>,
        #[arg(long = "minimal", action = ArgAction::SetTrue)]
        minimal: bool,
        #[arg(long = "patience", action = ArgAction::SetTrue)]
        patience: bool,
        #[arg(long = "histogram", action = ArgAction::SetTrue)]
        histogram: bool,
        #[arg(long = "diff-algorithm")]
        diff_algorithm: Option<String>,
        #[arg(long = "anchored")]
        anchored: Vec<String>,
        #[arg(long = "output-indicator-new")]
        output_indicator_new: Option<String>,
        #[arg(long = "output-indicator-old")]
        output_indicator_old: Option<String>,
        #[arg(long = "output-indicator-context")]
        output_indicator_context: Option<String>,
        #[arg(long = "ignore-space-at-eol", action = ArgAction::SetTrue)]
        ignore_space_at_eol: bool,
        #[arg(long = "ignore-cr-at-eol", action = ArgAction::SetTrue)]
        ignore_cr_at_eol: bool,
        #[arg(short = 'b', long = "ignore-space-change", action = ArgAction::SetTrue)]
        ignore_space_change: bool,
        #[arg(short = 'w', long = "ignore-all-space", action = ArgAction::SetTrue)]
        ignore_all_space: bool,
        #[arg(long = "ignore-blank-lines", action = ArgAction::SetTrue)]
        ignore_blank_lines: bool,
        #[arg(short = 'I', long = "ignore-matching-lines")]
        ignore_matching_lines: Vec<String>,
        #[arg(short = 'a', long = "text", action = ArgAction::SetTrue)]
        text: bool,
        #[arg(long = "no-ext-diff", action = ArgAction::SetTrue)]
        no_ext_diff: bool,
        #[arg(long = "no-textconv", action = ArgAction::SetTrue)]
        no_textconv: bool,
        #[arg(long = "color", num_args = 0..=1, default_missing_value = "always")]
        color: Option<String>,
        #[arg(long = "no-color", action = ArgAction::SetTrue)]
        no_color: bool,
        #[arg(long = "no-color-moved", action = ArgAction::SetTrue)]
        no_color_moved: bool,
        #[arg(long = "no-color-moved-ws", action = ArgAction::SetTrue)]
        no_color_moved_ws: bool,
        #[arg(long = "quiet", action = ArgAction::SetTrue)]
        quiet: bool,
        #[arg(long = "exit-code", action = ArgAction::SetTrue)]
        exit_code: bool,
        #[arg(long = "pretty", num_args = 0..=1, require_equals = true, default_missing_value = "")]
        pretty: Option<String>,
        #[arg(long = "notes", action = ArgAction::SetTrue)]
        notes: bool,
        #[arg(long = "format")]
        format: Option<String>,
        old: Option<String>,
        new: Option<String>,
        #[arg(value_hint = ValueHint::AnyPath)]
        paths: Vec<PathBuf>,
    },
    DiffPairs {
        #[arg(short = 'z', action = ArgAction::SetTrue)]
        nul_terminated: bool,
        #[arg(short = 'p', long = "patch", action = ArgAction::SetTrue)]
        patch: bool,
        #[arg(short = 's', long = "no-patch", action = ArgAction::SetTrue)]
        no_patch: bool,
        #[arg(long = "stat", action = ArgAction::SetTrue)]
        stat: bool,
        #[arg(long = "numstat", action = ArgAction::SetTrue)]
        numstat: bool,
        #[arg(long = "shortstat", action = ArgAction::SetTrue)]
        shortstat: bool,
        #[arg(long = "raw", action = ArgAction::SetTrue)]
        raw: bool,
        #[arg(long = "summary", action = ArgAction::SetTrue)]
        summary: bool,
        #[arg(long = "name-status", action = ArgAction::SetTrue)]
        name_status: bool,
        #[arg(long = "name-only", action = ArgAction::SetTrue)]
        name_only: bool,
        #[arg(long = "word-diff", num_args = 0..=1, default_missing_value = "plain")]
        word_diff: Option<String>,
        #[arg(long = "quiet", action = ArgAction::SetTrue)]
        quiet: bool,
    },
    Apply {
        #[arg(long = "check", action = ArgAction::SetTrue)]
        check: bool,
        #[arg(long = "cached", action = ArgAction::SetTrue)]
        cached: bool,
        #[arg(long = "index", action = ArgAction::SetTrue)]
        index: bool,
        #[arg(short = 'R', long = "reverse", action = ArgAction::SetTrue)]
        reverse: bool,
        #[arg(value_hint = ValueHint::FilePath)]
        patches: Vec<PathBuf>,
    },
    Am {
        #[arg(value_hint = ValueHint::FilePath)]
        patches: Vec<PathBuf>,
    },
    Clean {
        #[arg(allow_hyphen_values = true, trailing_var_arg = true)]
        args: Vec<String>,
    },
    Reset {
        #[arg(long = "soft", action = ArgAction::SetTrue)]
        soft: bool,
        #[arg(long = "mixed", action = ArgAction::SetTrue)]
        mixed: bool,
        #[arg(long = "hard", action = ArgAction::SetTrue)]
        hard: bool,
        #[arg(value_hint = ValueHint::AnyPath, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    Stash {
        #[arg(allow_hyphen_values = true, trailing_var_arg = true)]
        args: Vec<String>,
    },
    RangeDiff {
        #[arg(long = "no-dual-color", action = ArgAction::SetTrue)]
        no_dual_color: bool,
        ranges: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    Bisect {
        #[arg(allow_hyphen_values = true, trailing_var_arg = true)]
        args: Vec<String>,
    },
    Rerere {
        #[arg(allow_hyphen_values = true, trailing_var_arg = true)]
        args: Vec<String>,
    },
    Rebase {
        #[arg(long = "abort", action = ArgAction::SetTrue)]
        abort: bool,
        #[arg(long = "continue", action = ArgAction::SetTrue)]
        continue_: bool,
        #[arg(short = 'i', long = "interactive", action = ArgAction::SetTrue)]
        interactive: bool,
        #[arg(long = "onto")]
        onto: Option<String>,
        #[arg(value_hint = ValueHint::AnyPath, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    Worktree {
        #[arg(allow_hyphen_values = true, trailing_var_arg = true)]
        args: Vec<String>,
    },
    SparseCheckout {
        #[arg(allow_hyphen_values = true, trailing_var_arg = true)]
        args: Vec<String>,
    },
    Submodule {
        #[arg(allow_hyphen_values = true, trailing_var_arg = true)]
        args: Vec<String>,
    },
    Log {
        #[arg(long, action = ArgAction::SetTrue)]
        oneline: bool,
        #[arg(long = "all", action = ArgAction::SetTrue)]
        all: bool,
        #[arg(long = "parents", action = ArgAction::SetTrue)]
        parents: bool,
        #[arg(long = "first-parent", action = ArgAction::SetTrue)]
        first_parent: bool,
        #[arg(long = "no-diff-merges", action = ArgAction::SetTrue)]
        no_diff_merges: bool,
        #[arg(long = "diff-merges")]
        diff_merges: Option<String>,
        #[arg(short = 'm', action = ArgAction::SetTrue)]
        separate_merges: bool,
        #[arg(long = "dd", action = ArgAction::SetTrue)]
        dd: bool,
        #[arg(long = "reverse", action = ArgAction::SetTrue)]
        reverse: bool,
        #[arg(long = "root", action = ArgAction::SetTrue)]
        root: bool,
        #[arg(short = 'p', long = "patch", action = ArgAction::SetTrue)]
        patch: bool,
        #[arg(long = "patch-with-stat", action = ArgAction::SetTrue)]
        patch_with_stat: bool,
        #[arg(short = 'c', action = ArgAction::SetTrue)]
        combined: bool,
        #[arg(long = "cc", action = ArgAction::SetTrue)]
        dense_combined: bool,
        #[arg(long = "stat", action = ArgAction::SetTrue)]
        stat: bool,
        #[arg(long = "numstat", action = ArgAction::SetTrue)]
        numstat: bool,
        #[arg(long = "shortstat", action = ArgAction::SetTrue)]
        shortstat: bool,
        #[arg(long = "raw", action = ArgAction::SetTrue)]
        raw: bool,
        #[arg(long = "summary", action = ArgAction::SetTrue)]
        summary: bool,
        #[arg(long = "name-only", action = ArgAction::SetTrue)]
        name_only: bool,
        #[arg(long = "name-status", action = ArgAction::SetTrue)]
        name_status: bool,
        #[arg(
            long = "decorate",
            num_args = 0..=1,
            require_equals = true,
            default_missing_value = "short"
        )]
        decorate: Option<String>,
        #[arg(long = "clear-decorations", action = ArgAction::SetTrue)]
        clear_decorations: bool,
        #[arg(short = 'S')]
        pickaxe_string: Option<String>,
        #[arg(short = 'G')]
        pickaxe_regex: Option<String>,
        #[arg(long = "pickaxe-regex", action = ArgAction::SetTrue)]
        pickaxe_regex_mode: bool,
        #[arg(long = "pickaxe-all", action = ArgAction::SetTrue)]
        pickaxe_all: bool,
        #[arg(short = 'I', long = "ignore-matching-lines")]
        ignore_matching_lines: Vec<String>,
        #[arg(short = 'g', long = "walk-reflogs", action = ArgAction::SetTrue)]
        walk_reflogs: bool,
        #[arg(long = "no-walk", action = ArgAction::SetTrue)]
        no_walk: bool,
        #[arg(long = "format")]
        format: Option<String>,
        #[arg(long = "max-count", short = 'n')]
        max_count: Option<String>,
        #[arg(long = "since", alias = "after")]
        since: Option<String>,
        #[arg(long = "pretty")]
        pretty: Option<String>,
        #[arg(allow_hyphen_values = true)]
        revs: Vec<String>,
    },
    FormatPatch {
        #[arg(short = 'o', long = "output-directory", value_hint = ValueHint::DirPath)]
        output_directory: Option<PathBuf>,
        #[arg(long = "stdout", action = ArgAction::SetTrue)]
        stdout: bool,
        #[arg(long = "attach", action = ArgAction::SetTrue)]
        attach: bool,
        #[arg(long = "inline", action = ArgAction::SetTrue)]
        inline: bool,
        #[arg(long = "suffix")]
        suffix: Option<String>,
        #[arg(long = "subject-prefix")]
        subject_prefix: Option<String>,
        #[arg(long = "no-numbered", action = ArgAction::SetTrue)]
        no_numbered: bool,
        #[arg(short = 'n', long = "numbered", action = ArgAction::SetTrue)]
        numbered: bool,
        #[arg(long = "numbered-files", action = ArgAction::SetTrue)]
        numbered_files: bool,
        #[arg(long = "cover-letter", action = ArgAction::SetTrue)]
        cover_letter: bool,
        #[arg(short = '1', action = ArgAction::SetTrue)]
        one: bool,
        revs: Vec<String>,
    },
    SendEmail {
        #[arg(long = "dump-aliases", action = ArgAction::SetTrue)]
        dump_aliases: bool,
        #[arg(long = "translate-aliases", action = ArgAction::SetTrue)]
        translate_aliases: bool,
        args: Vec<String>,
    },
    ImapSend {
        #[arg(short = 'v', long = "verbose", action = ArgAction::SetTrue)]
        verbose: bool,
        #[arg(short = 'q', long = "quiet", action = ArgAction::SetTrue)]
        quiet: bool,
        #[arg(short = 'f', long = "folder")]
        folder: Option<String>,
        #[arg(long = "list", action = ArgAction::SetTrue)]
        list: bool,
        #[arg(long = "curl", action = ArgAction::SetTrue)]
        curl: bool,
        #[arg(long = "no-curl", action = ArgAction::SetTrue)]
        no_curl: bool,
    },
    FilterBranch {
        #[arg(short = 'f', long = "force", action = ArgAction::SetTrue)]
        force: bool,
        #[arg(long = "msg-filter")]
        msg_filter: Option<String>,
        #[arg(long = "tree-filter")]
        tree_filter: Option<String>,
        #[arg(long = "index-filter")]
        index_filter: Option<String>,
        #[arg(long = "env-filter")]
        env_filter: Option<String>,
        #[arg(long = "parent-filter")]
        parent_filter: Option<String>,
        #[arg(long = "commit-filter")]
        commit_filter: Option<String>,
        #[arg(long = "tag-name-filter")]
        tag_name_filter: Option<String>,
        #[arg(long = "subdirectory-filter")]
        subdirectory_filter: Option<String>,
        #[arg(long = "original")]
        original: Option<String>,
        #[arg(short = 'd', value_hint = ValueHint::DirPath)]
        temp_dir: Option<PathBuf>,
        #[arg(long = "setup")]
        setup: Option<String>,
        #[arg(long = "state-branch")]
        state_branch: Option<String>,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        revs: Vec<String>,
    },
    Quiltimport {
        #[arg(short = 'n', long = "dry-run", action = ArgAction::SetTrue)]
        dry_run: bool,
        #[arg(long = "author")]
        author: Option<String>,
        #[arg(long = "patches", value_hint = ValueHint::DirPath)]
        patches: Option<PathBuf>,
        #[arg(long = "series", value_hint = ValueHint::FilePath)]
        series: Option<PathBuf>,
        #[arg(long = "keep-non-patch", action = ArgAction::SetTrue)]
        keep_non_patch: bool,
    },
    FastExport {
        #[arg(long = "all", action = ArgAction::SetTrue)]
        all: bool,
        refs: Vec<String>,
    },
    FastImport {
        #[arg(long = "date-format")]
        date_format: Option<String>,
    },
    CommitGraph {
        #[command(subcommand)]
        command: CommitGraphCommand,
    },
    MultiPackIndex {
        #[arg(long = "object-dir", value_hint = ValueHint::DirPath)]
        object_dir: Option<PathBuf>,
        #[command(subcommand)]
        command: MultiPackIndexCommand,
    },
    Daemon {
        #[arg(long = "verbose", action = ArgAction::SetTrue)]
        verbose: bool,
        #[arg(long = "export-all", action = ArgAction::SetTrue)]
        export_all: bool,
        #[arg(long = "timeout")]
        timeout: Option<u64>,
        #[arg(long = "init-timeout")]
        init_timeout: Option<u64>,
        #[arg(long = "max-connections")]
        max_connections: Option<usize>,
        #[arg(long = "strict-paths", action = ArgAction::SetTrue)]
        strict_paths: bool,
        #[arg(long = "base-path", value_hint = ValueHint::DirPath)]
        base_path: Option<PathBuf>,
        #[arg(long = "base-path-relaxed", action = ArgAction::SetTrue)]
        base_path_relaxed: bool,
        #[arg(long = "reuseaddr", action = ArgAction::SetTrue)]
        reuseaddr: bool,
        #[arg(long = "pid-file", value_hint = ValueHint::FilePath)]
        pid_file: Option<PathBuf>,
        #[arg(long = "inetd", action = ArgAction::SetTrue)]
        inetd: bool,
        #[arg(long = "listen")]
        listen: Vec<String>,
        #[arg(long = "port")]
        port: Option<u16>,
        #[arg(value_hint = ValueHint::DirPath)]
        directories: Vec<PathBuf>,
    },
    UploadPack {
        #[arg(long = "strict", action = ArgAction::SetTrue)]
        strict: bool,
        #[arg(long = "no-strict", action = ArgAction::SetTrue)]
        no_strict: bool,
        #[arg(long = "stateless-rpc", action = ArgAction::SetTrue)]
        stateless_rpc: bool,
        #[arg(long = "advertise-refs", action = ArgAction::SetTrue)]
        advertise_refs: bool,
        #[arg(long = "timeout")]
        timeout: Option<u64>,
        #[arg(value_hint = ValueHint::DirPath)]
        directory: PathBuf,
    },
    UploadArchive {
        #[arg(value_hint = ValueHint::DirPath)]
        repository: PathBuf,
    },
    HttpBackend,
    HttpFetch {
        #[arg(short = 'c', action = ArgAction::SetTrue)]
        commit: bool,
        #[arg(short = 't', action = ArgAction::SetTrue)]
        tags: bool,
        #[arg(short = 'a', action = ArgAction::SetTrue)]
        all: bool,
        #[arg(short = 'v', action = ArgAction::SetTrue)]
        verbose: bool,
        #[arg(long = "recover", action = ArgAction::SetTrue)]
        recover: bool,
        #[arg(short = 'w')]
        write_ref: Vec<String>,
        #[arg(long = "stdin", action = ArgAction::SetTrue)]
        stdin: bool,
        #[arg(long = "packfile")]
        packfile: Option<String>,
        #[arg(long = "index-pack-arg")]
        index_pack_args: Vec<String>,
        args: Vec<String>,
    },
    HttpPush {
        #[arg(long = "all", action = ArgAction::SetTrue)]
        all: bool,
        #[arg(long = "dry-run", action = ArgAction::SetTrue)]
        dry_run: bool,
        #[arg(long = "force", action = ArgAction::SetTrue)]
        force: bool,
        #[arg(long = "verbose", action = ArgAction::SetTrue)]
        verbose: bool,
        remote: String,
        #[arg(allow_hyphen_values = true)]
        heads: Vec<String>,
    },
    FetchPack {
        #[arg(long = "all", action = ArgAction::SetTrue)]
        all: bool,
        #[arg(long = "stdin", action = ArgAction::SetTrue)]
        stdin: bool,
        #[arg(short = 'q', long = "quiet", action = ArgAction::SetTrue)]
        quiet: bool,
        #[arg(short = 'k', long = "keep", action = ArgAction::SetTrue)]
        keep: bool,
        #[arg(long = "thin", action = ArgAction::SetTrue)]
        thin: bool,
        #[arg(long = "include-tag", action = ArgAction::SetTrue)]
        include_tag: bool,
        #[arg(long = "upload-pack")]
        upload_pack: Option<String>,
        #[arg(long = "depth")]
        depth: Option<usize>,
        #[arg(long = "no-progress", action = ArgAction::SetTrue)]
        no_progress: bool,
        #[arg(long = "diag-url", action = ArgAction::SetTrue)]
        diag_url: bool,
        #[arg(short = 'v', long = "verbose", action = ArgAction::SetTrue)]
        verbose: bool,
        directory: String,
        refs: Vec<String>,
    },
    SendPack {
        #[arg(long = "mirror", action = ArgAction::SetTrue)]
        mirror: bool,
        #[arg(long = "dry-run", short = 'n', action = ArgAction::SetTrue)]
        dry_run: bool,
        #[arg(long = "force", short = 'f', action = ArgAction::SetTrue)]
        force: bool,
        #[arg(long = "receive-pack", alias = "exec")]
        receive_pack: Option<String>,
        #[arg(long = "verbose", short = 'v', action = ArgAction::SetTrue)]
        verbose: bool,
        #[arg(long = "thin", action = ArgAction::SetTrue)]
        thin: bool,
        #[arg(long = "atomic", action = ArgAction::SetTrue)]
        atomic: bool,
        #[arg(long = "all", action = ArgAction::SetTrue)]
        all: bool,
        #[arg(long = "stdin", action = ArgAction::SetTrue)]
        stdin: bool,
        directory: String,
        refs: Vec<String>,
    },
    ReceivePack {
        #[arg(short = 'q', long = "quiet", action = ArgAction::SetTrue)]
        quiet: bool,
        #[arg(value_hint = ValueHint::DirPath)]
        directory: PathBuf,
    },
    Shell {
        #[arg(short = 'c')]
        command: Option<String>,
        args: Vec<String>,
    },
    Whatchanged {
        #[arg(long, action = ArgAction::SetTrue)]
        oneline: bool,
        #[arg(long = "all", action = ArgAction::SetTrue)]
        all: bool,
        #[arg(long = "parents", action = ArgAction::SetTrue)]
        parents: bool,
        #[arg(long = "reverse", action = ArgAction::SetTrue)]
        reverse: bool,
        #[arg(short = 'p', long = "patch", action = ArgAction::SetTrue)]
        patch: bool,
        #[arg(long = "patch-with-stat", action = ArgAction::SetTrue)]
        patch_with_stat: bool,
        #[arg(long = "root", action = ArgAction::SetTrue)]
        root: bool,
        #[arg(short = 'c', action = ArgAction::SetTrue)]
        combined: bool,
        #[arg(long = "cc", action = ArgAction::SetTrue)]
        dense_combined: bool,
        #[arg(long = "stat", action = ArgAction::SetTrue)]
        stat: bool,
        #[arg(long = "numstat", action = ArgAction::SetTrue)]
        numstat: bool,
        #[arg(long = "shortstat", action = ArgAction::SetTrue)]
        shortstat: bool,
        #[arg(long = "raw", action = ArgAction::SetTrue)]
        raw: bool,
        #[arg(long = "summary", action = ArgAction::SetTrue)]
        summary: bool,
        #[arg(long = "name-only", action = ArgAction::SetTrue)]
        name_only: bool,
        #[arg(long = "name-status", action = ArgAction::SetTrue)]
        name_status: bool,
        #[arg(short = 'S')]
        pickaxe_string: Option<String>,
        #[arg(short = 'G')]
        pickaxe_regex: Option<String>,
        #[arg(long = "pickaxe-regex", action = ArgAction::SetTrue)]
        pickaxe_regex_mode: bool,
        #[arg(long = "pickaxe-all", action = ArgAction::SetTrue)]
        pickaxe_all: bool,
        #[arg(long = "format")]
        format: Option<String>,
        #[arg(long = "max-count", short = 'n')]
        max_count: Option<String>,
        #[arg(long = "since", alias = "after")]
        since: Option<String>,
        #[arg(long = "pretty")]
        pretty: Option<String>,
        #[arg(long = "i-still-use-this", hide = true, action = ArgAction::SetTrue)]
        i_still_use_this: bool,
        revs: Vec<String>,
    },
    Show {
        #[arg(short = 's', long = "no-patch", action = ArgAction::SetTrue)]
        no_patch: bool,
        #[arg(long, action = ArgAction::SetTrue)]
        oneline: bool,
        #[arg(long = "stat", action = ArgAction::SetTrue)]
        stat: bool,
        #[arg(long = "patch-with-raw", action = ArgAction::SetTrue)]
        patch_with_raw: bool,
        #[arg(long = "patch-with-stat", action = ArgAction::SetTrue)]
        patch_with_stat: bool,
        #[arg(long = "numstat", action = ArgAction::SetTrue)]
        numstat: bool,
        #[arg(long = "shortstat", action = ArgAction::SetTrue)]
        shortstat: bool,
        #[arg(long = "raw", action = ArgAction::SetTrue)]
        raw: bool,
        #[arg(long = "summary", action = ArgAction::SetTrue)]
        summary: bool,
        #[arg(long = "name-only", action = ArgAction::SetTrue)]
        name_only: bool,
        #[arg(long = "name-status", action = ArgAction::SetTrue)]
        name_status: bool,
        #[arg(long = "root", action = ArgAction::SetTrue)]
        root: bool,
        #[arg(short = 'c', action = ArgAction::SetTrue)]
        combined: bool,
        #[arg(short = 'm', action = ArgAction::SetTrue)]
        separate_merges: bool,
        #[arg(long = "first-parent", action = ArgAction::SetTrue)]
        first_parent: bool,
        #[arg(long = "format")]
        format: Option<String>,
        #[arg(long = "pretty")]
        pretty: Option<String>,
        #[arg(value_hint = ValueHint::AnyPath, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    Grep {
        #[arg(long = "cached", action = ArgAction::SetTrue)]
        cached: bool,
        #[arg(short = 'n', long = "line-number", action = ArgAction::SetTrue)]
        line_number: bool,
        #[arg(short = 'l', long = "files-with-matches", action = ArgAction::SetTrue)]
        files_with_matches: bool,
        #[arg(short = 'F', long = "fixed-strings", action = ArgAction::SetTrue)]
        fixed_strings: bool,
        pattern: String,
        #[arg(value_hint = ValueHint::AnyPath, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    RevList {
        #[arg(long = "all", action = ArgAction::SetTrue)]
        all: bool,
        #[arg(long = "count", action = ArgAction::SetTrue)]
        count: bool,
        #[arg(long = "objects", action = ArgAction::SetTrue)]
        objects: bool,
        #[arg(long = "no-object-names", action = ArgAction::SetTrue)]
        no_object_names: bool,
        #[arg(long = "filter")]
        filter: Option<String>,
        #[arg(long = "filter-provided-objects", action = ArgAction::SetTrue)]
        filter_provided_objects: bool,
        #[arg(long = "parents", action = ArgAction::SetTrue)]
        parents: bool,
        #[arg(long = "children", action = ArgAction::SetTrue)]
        children: bool,
        #[arg(long = "reverse", action = ArgAction::SetTrue)]
        reverse: bool,
        #[arg(long = "max-count", short = 'n')]
        max_count: Option<usize>,
        #[arg(allow_hyphen_values = true)]
        revs: Vec<String>,
    },
    MergeBase {
        #[arg(long = "is-ancestor", action = ArgAction::SetTrue)]
        is_ancestor: bool,
        #[arg(long = "octopus", action = ArgAction::SetTrue)]
        octopus: bool,
        commits: Vec<String>,
    },
    Merge {
        #[arg(long = "abort", action = ArgAction::SetTrue)]
        abort: bool,
        #[arg(long = "continue", action = ArgAction::SetTrue)]
        continue_: bool,
        #[arg(long = "ff-only", action = ArgAction::SetTrue)]
        ff_only: bool,
        #[arg(long = "no-ff", action = ArgAction::SetTrue)]
        no_ff: bool,
        #[arg(long = "no-commit", action = ArgAction::SetTrue)]
        no_commit: bool,
        #[arg(long = "squash", action = ArgAction::SetTrue)]
        squash: bool,
        #[arg(short = 's', long = "strategy")]
        strategies: Vec<String>,
        commits: Vec<String>,
    },
    Mergetool {
        #[arg(short = 't', long = "tool")]
        tool: Option<String>,
        #[arg(short = 'y', long = "no-prompt", action = ArgAction::SetTrue)]
        no_prompt: bool,
        #[arg(long = "prompt", action = ArgAction::SetTrue)]
        prompt: bool,
        #[arg(value_hint = ValueHint::AnyPath)]
        paths: Vec<PathBuf>,
    },
    MergeTree {
        #[arg(long = "write-tree", action = ArgAction::SetTrue)]
        write_tree: bool,
        #[arg(long = "trivial-merge", action = ArgAction::SetTrue)]
        trivial_merge: bool,
        #[arg(long = "messages", action = ArgAction::SetTrue)]
        messages: bool,
        #[arg(long = "no-messages", action = ArgAction::SetTrue)]
        no_messages: bool,
        #[arg(long = "quiet", action = ArgAction::SetTrue)]
        quiet: bool,
        #[arg(short = 'z', action = ArgAction::SetTrue)]
        nul_terminated: bool,
        #[arg(long = "name-only", action = ArgAction::SetTrue)]
        name_only: bool,
        #[arg(long = "allow-unrelated-histories", action = ArgAction::SetTrue)]
        allow_unrelated_histories: bool,
        #[arg(long = "stdin", action = ArgAction::SetTrue)]
        stdin: bool,
        #[arg(long = "merge-base")]
        merge_base: Option<String>,
        #[arg(short = 'X', long = "strategy-option")]
        strategy_options: Vec<String>,
        args: Vec<String>,
    },
    MergeFile {
        #[arg(short = 'p', long = "stdout", action = ArgAction::SetTrue)]
        stdout: bool,
        #[arg(short = 'L')]
        labels: Vec<String>,
        current: PathBuf,
        base: PathBuf,
        other: PathBuf,
    },
    MergeOneFile {
        orig_blob: String,
        our_blob: String,
        their_blob: String,
        path: String,
        orig_mode: String,
        our_mode: String,
        their_mode: String,
    },
    MergeIndex {
        #[arg(short = 'o', action = ArgAction::SetTrue)]
        one_shot: bool,
        #[arg(short = 'q', action = ArgAction::SetTrue)]
        quiet: bool,
        merge_program: String,
        #[arg(short = 'a', action = ArgAction::SetTrue)]
        all: bool,
        paths: Vec<String>,
    },
    UpdateRef {
        #[arg(short = 'd', long = "delete", action = ArgAction::SetTrue)]
        delete: bool,
        #[arg(long = "no-deref", overrides_with = "deref", action = ArgAction::SetTrue)]
        no_deref: bool,
        #[arg(long = "deref", overrides_with = "no_deref", action = ArgAction::SetTrue)]
        deref: bool,
        #[arg(long = "stdin", action = ArgAction::SetTrue)]
        stdin: bool,
        #[arg(short = 'z', action = ArgAction::SetTrue)]
        nul_terminated: bool,
        #[arg(short = 'm')]
        message: Option<String>,
        #[arg(long = "create-reflog", overrides_with = "no_create_reflog", action = ArgAction::SetTrue)]
        create_reflog: bool,
        #[arg(long = "no-create-reflog", overrides_with = "create_reflog", action = ArgAction::SetTrue)]
        no_create_reflog: bool,
        #[arg(short = '0', long = "batch-updates", overrides_with = "no_batch_updates", action = ArgAction::SetTrue)]
        batch_updates: bool,
        #[arg(long = "no-batch-updates", overrides_with = "batch_updates", action = ArgAction::SetTrue)]
        no_batch_updates: bool,
        name: Option<String>,
        newvalue: Option<String>,
    },
    SymbolicRef {
        #[arg(short = 'q', long = "quiet", action = ArgAction::SetTrue)]
        quiet: bool,
        #[arg(long = "short", action = ArgAction::SetTrue)]
        short: bool,
        #[arg(long = "no-recurse", action = ArgAction::SetTrue)]
        no_recurse: bool,
        name: String,
        target: Vec<String>,
    },
    Refs {
        #[command(subcommand)]
        command: RefsCommand,
    },
    Repo {
        #[command(subcommand)]
        command: RepoCommand,
    },
    LastModified {
        #[arg(short = 'r', long = "recursive", action = ArgAction::SetTrue)]
        recursive: bool,
        #[arg(short = 't', long = "show-trees", action = ArgAction::SetTrue)]
        show_trees: bool,
        #[arg(long = "max-depth")]
        max_depth: Option<i32>,
        #[arg(short = 'z', action = ArgAction::SetTrue)]
        nul_terminated: bool,
        #[arg(allow_hyphen_values = true)]
        args: Vec<String>,
    },
    RevParse {
        #[arg(
            long = "short",
            num_args = 0..=1,
            require_equals = true,
            default_missing_value = "7"
        )]
        short: Option<usize>,
        #[arg(
            long = "abbrev-ref",
            num_args = 0..=1,
            require_equals = true,
            default_missing_value = "loose"
        )]
        abbrev_ref: Option<String>,
        #[arg(long = "verify", action = ArgAction::SetTrue)]
        verify: bool,
        #[arg(short = 'q', long = "quiet", action = ArgAction::SetTrue)]
        quiet: bool,
        #[arg(long = "symbolic-full-name", action = ArgAction::SetTrue)]
        symbolic_full_name: bool,
        #[arg(long = "bisect", action = ArgAction::SetTrue)]
        bisect: bool,
        #[arg(long = "path-format")]
        path_format: Vec<String>,
        #[arg(long = "since", alias = "after")]
        since: Vec<String>,
        #[arg(long = "until", alias = "before")]
        until: Vec<String>,
        #[arg(
            long = "show-object-format",
            num_args = 0..=1,
            require_equals = true,
            default_missing_value = "storage"
        )]
        show_object_format: Vec<String>,
        #[arg(long = "show-ref-format", action = ArgAction::SetTrue)]
        show_ref_format: bool,
        #[arg(long = "show-toplevel", action = ArgAction::SetTrue)]
        show_toplevel: bool,
        #[arg(long = "show-prefix", action = ArgAction::SetTrue)]
        show_prefix: bool,
        #[arg(long = "show-cdup", action = ArgAction::SetTrue)]
        show_cdup: bool,
        #[arg(long = "show-superproject-working-tree", action = ArgAction::SetTrue)]
        show_superproject_working_tree: bool,
        #[arg(long = "git-dir", action = ArgAction::SetTrue)]
        git_dir: bool,
        #[arg(long = "absolute-git-dir", action = ArgAction::SetTrue)]
        absolute_git_dir: bool,
        #[arg(long = "git-common-dir", action = ArgAction::SetTrue)]
        git_common_dir: bool,
        #[arg(long = "git-path")]
        git_paths: Vec<PathBuf>,
        #[arg(long = "is-inside-git-dir", action = ArgAction::SetTrue)]
        is_inside_git_dir: bool,
        #[arg(long = "is-inside-work-tree", action = ArgAction::SetTrue)]
        is_inside_work_tree: bool,
        #[arg(long = "is-bare-repository", action = ArgAction::SetTrue)]
        is_bare_repository: bool,
        #[arg(long = "is-shallow-repository", action = ArgAction::SetTrue)]
        is_shallow_repository: bool,
        revs: Vec<String>,
    },
    ShowRef {
        #[arg(short = 'q', long = "quiet", action = ArgAction::SetTrue)]
        quiet: bool,
        #[arg(long = "head", action = ArgAction::SetTrue)]
        head: bool,
        #[arg(long = "heads", alias = "branches", action = ArgAction::SetTrue)]
        heads: bool,
        #[arg(long = "tags", action = ArgAction::SetTrue)]
        tags: bool,
        #[arg(
            short = 's',
            long = "hash",
            num_args = 0..=1,
            require_equals = true,
            default_missing_value = "40"
        )]
        hash: Option<usize>,
        #[arg(
            long = "abbrev",
            num_args = 0..=1,
            require_equals = true,
            default_missing_value = "7"
        )]
        abbrev: Option<usize>,
        #[arg(long = "verify", action = ArgAction::SetTrue)]
        verify: bool,
        #[arg(long = "exists", action = ArgAction::SetTrue)]
        exists: bool,
        refs: Vec<String>,
    },
    ForEachRef {
        #[arg(long = "format")]
        format: Option<String>,
        #[arg(long = "sort")]
        sort: Vec<String>,
        patterns: Vec<String>,
    },
    LsTree {
        #[arg(short = 'r', action = ArgAction::SetTrue)]
        recursive: bool,
        #[arg(short = 't', action = ArgAction::SetTrue)]
        show_trees: bool,
        #[arg(long = "name-only", action = ArgAction::SetTrue)]
        name_only: bool,
        treeish: String,
        paths: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    Branch {
        #[arg(short = 'h', long = "help", action = ArgAction::SetTrue)]
        help: bool,
        #[arg(short = 'r', long = "remotes", action = ArgAction::SetTrue)]
        remotes: bool,
        #[arg(short = 'a', long = "all", action = ArgAction::SetTrue)]
        all: bool,
        #[arg(short = 'l', long = "list", action = ArgAction::SetTrue)]
        list: bool,
        #[arg(short = 'f', long = "force", action = ArgAction::SetTrue)]
        force: bool,
        #[arg(short = 'v', long = "verbose", action = ArgAction::Count)]
        verbose: u8,
        #[arg(long = "abbrev", num_args = 0..=1, require_equals = true, default_missing_value = "7")]
        abbrev: Option<usize>,
        #[arg(long = "no-abbrev", action = ArgAction::SetTrue)]
        no_abbrev: bool,
        #[arg(long = "column", num_args = 0..=1, require_equals = true, default_missing_value = "column")]
        column: Option<String>,
        #[arg(long = "create-reflog", overrides_with = "no_create_reflog", action = ArgAction::SetTrue)]
        create_reflog: bool,
        #[arg(long = "no-create-reflog", overrides_with = "create_reflog", action = ArgAction::SetTrue)]
        no_create_reflog: bool,
        #[arg(long = "show-current", action = ArgAction::SetTrue)]
        show_current: bool,
        #[arg(long = "edit-description", action = ArgAction::SetTrue)]
        edit_description: bool,
        #[arg(short = 'd', long = "delete", action = ArgAction::SetTrue)]
        delete: bool,
        #[arg(short = 'D', action = ArgAction::SetTrue)]
        force_delete: bool,
        #[arg(short = 'm', long = "move", action = ArgAction::SetTrue)]
        move_branch: bool,
        #[arg(short = 'M', action = ArgAction::SetTrue)]
        force_move: bool,
        #[arg(short = 'c', long = "copy", action = ArgAction::SetTrue)]
        copy_branch: bool,
        #[arg(short = 'C', action = ArgAction::SetTrue)]
        force_copy: bool,
        #[arg(short = 'u', long = "set-upstream-to")]
        set_upstream_to: Option<String>,
        #[arg(long = "unset-upstream", action = ArgAction::SetTrue)]
        unset_upstream: bool,
        #[arg(short = 't', long = "track", num_args = 0..=1, require_equals = true, default_missing_value = "direct")]
        track: Option<String>,
        #[arg(long = "no-track", action = ArgAction::SetTrue)]
        no_track: bool,
        #[arg(long = "sort")]
        sort: Vec<String>,
        #[arg(long = "no-sort", action = ArgAction::SetTrue)]
        no_sort: bool,
        #[arg(long = "contains", num_args = 0..=1, default_missing_value = "HEAD")]
        contains: Option<String>,
        #[arg(long = "merged", num_args = 0..=1, default_missing_value = "HEAD")]
        merged: Option<String>,
        #[arg(long = "no-merged", num_args = 0..=1, default_missing_value = "HEAD")]
        no_merged: Option<String>,
        name: Option<String>,
        start_point: Option<String>,
        extra_args: Vec<String>,
    },
    Tag {
        #[arg(short = 'd', long = "delete", action = ArgAction::SetTrue)]
        delete: bool,
        #[arg(short = 'v', long = "verify", action = ArgAction::SetTrue)]
        verify: bool,
        #[arg(short = 'l', long = "list", action = ArgAction::SetTrue)]
        list: bool,
        #[arg(short = 'f', long = "force", action = ArgAction::SetTrue)]
        force: bool,
        #[arg(short = 'a', long = "annotate", action = ArgAction::SetTrue)]
        annotate: bool,
        #[arg(short = 'm')]
        messages: Vec<String>,
        #[arg(long = "contains", num_args = 0..=1, default_missing_value = "HEAD")]
        contains: Option<String>,
        #[arg(long = "no-contains", num_args = 0..=1, default_missing_value = "HEAD")]
        no_contains: Option<String>,
        #[arg(long = "merged", num_args = 0..=1, default_missing_value = "HEAD")]
        merged: Option<String>,
        #[arg(long = "no-merged", num_args = 0..=1, default_missing_value = "HEAD")]
        no_merged: Option<String>,
        #[arg(long = "sort")]
        sort: Vec<String>,
        #[arg(long = "format")]
        format: Option<String>,
        args: Vec<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum ScalarCommand {
    #[command(disable_help_flag = true)]
    Clone(Box<ScalarCloneArgs>),
    #[command(disable_help_flag = true)]
    List {
        #[arg(short = 'h', long = "help", action = ArgAction::SetTrue)]
        help: bool,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        extra: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    Register {
        #[arg(short = 'h', long = "help", action = ArgAction::SetTrue)]
        help: bool,
        #[arg(long = "maintenance", num_args = 0..=1, default_missing_value = "", require_equals = true, overrides_with = "no_maintenance")]
        maintenance: Option<String>,
        #[arg(long = "no-maintenance", num_args = 0..=1, default_missing_value = "", require_equals = true, overrides_with = "maintenance")]
        no_maintenance: Option<String>,
        #[arg(value_hint = ValueHint::DirPath, allow_hyphen_values = true)]
        enlistment: Option<PathBuf>,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        extra: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    Unregister {
        #[arg(short = 'h', long = "help", action = ArgAction::SetTrue)]
        help: bool,
        #[arg(value_hint = ValueHint::DirPath, allow_hyphen_values = true)]
        enlistment: Option<PathBuf>,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        extra: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    Run {
        #[arg(short = 'h', long = "help", action = ArgAction::SetTrue)]
        help: bool,
        #[arg(allow_hyphen_values = true)]
        task: Option<String>,
        #[arg(value_hint = ValueHint::DirPath, allow_hyphen_values = true)]
        enlistment: Option<PathBuf>,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        extra: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    Reconfigure {
        #[arg(short = 'h', long = "help", action = ArgAction::SetTrue)]
        help: bool,
        #[arg(long = "maintenance", num_args = 0..=1, default_missing_value = "", action = ArgAction::Append)]
        maintenance: Vec<String>,
        #[arg(long = "all", num_args = 0..=1, default_missing_value = "", require_equals = true, overrides_with = "no_all")]
        all: Option<String>,
        #[arg(long = "no-all", num_args = 0..=1, default_missing_value = "", require_equals = true, overrides_with = "all")]
        no_all: Option<String>,
        #[arg(value_hint = ValueHint::DirPath, allow_hyphen_values = true)]
        enlistment: Option<PathBuf>,
    },
    #[command(disable_help_flag = true)]
    Diagnose {
        #[arg(short = 'h', long = "help", action = ArgAction::SetTrue)]
        help: bool,
        #[arg(long = "mode", num_args = 0..=1, default_missing_value = "", require_equals = true)]
        mode: Option<String>,
        #[arg(value_hint = ValueHint::DirPath, allow_hyphen_values = true)]
        enlistment: Option<PathBuf>,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        extra: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    Delete {
        #[arg(short = 'h', long = "help", action = ArgAction::SetTrue)]
        help: bool,
        #[arg(value_hint = ValueHint::DirPath, allow_hyphen_values = true)]
        enlistment: Option<PathBuf>,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        extra: Vec<String>,
    },
    Help {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    Version {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    #[allow(dead_code)]
    #[command(external_subcommand)]
    Unknown(Vec<OsString>),
}

#[derive(ClapArgs, Debug)]
pub struct ScalarCloneArgs {
    #[arg(short = 'h', long = "help", action = ArgAction::SetTrue)]
    pub help: bool,
    #[arg(long = "single-branch", num_args = 0..=1, default_missing_value = "", require_equals = true, overrides_with = "no_single_branch")]
    pub single_branch: Option<String>,
    #[arg(long = "no-single-branch", num_args = 0..=1, default_missing_value = "", require_equals = true, overrides_with = "single_branch")]
    pub no_single_branch: Option<String>,
    #[arg(short = 'b', long = "branch")]
    pub branch: Option<String>,
    #[arg(long = "no-branch", num_args = 0..=1, default_missing_value = "", require_equals = true, overrides_with = "branch")]
    pub no_branch: Option<String>,
    #[arg(long = "full-clone", num_args = 0..=1, default_missing_value = "", require_equals = true, overrides_with = "no_full_clone")]
    pub full_clone: Option<String>,
    #[arg(long = "no-full-clone", num_args = 0..=1, default_missing_value = "", require_equals = true, overrides_with = "full_clone")]
    pub no_full_clone: Option<String>,
    #[arg(long = "src", num_args = 0..=1, default_missing_value = "", require_equals = true, overrides_with = "no_src")]
    pub src: Option<String>,
    #[arg(long = "no-src", num_args = 0..=1, default_missing_value = "", require_equals = true, overrides_with = "src")]
    pub no_src: Option<String>,
    #[arg(long = "tags", num_args = 0..=1, default_missing_value = "", require_equals = true, overrides_with = "no_tags")]
    pub tags: Option<String>,
    #[arg(long = "no-tags", num_args = 0..=1, default_missing_value = "", require_equals = true, overrides_with = "tags")]
    pub no_tags: Option<String>,
    #[arg(long = "maintenance", num_args = 0..=1, default_missing_value = "", require_equals = true, overrides_with = "no_maintenance")]
    pub maintenance: Option<String>,
    #[arg(long = "no-maintenance", num_args = 0..=1, default_missing_value = "", require_equals = true, overrides_with = "maintenance")]
    pub no_maintenance: Option<String>,
    pub url: Option<String>,
    #[arg(value_hint = ValueHint::DirPath, allow_hyphen_values = true)]
    pub enlistment: Option<PathBuf>,
}

#[derive(Subcommand, Debug)]
pub enum HookCommand {
    Run {
        #[arg(long = "ignore-missing", action = ArgAction::SetTrue)]
        ignore_missing: bool,
        #[arg(long = "to-stdin", value_hint = ValueHint::FilePath)]
        to_stdin: Option<PathBuf>,
        hook_name: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum ManagedHooksCommand {
    Init,
    Add {
        #[arg(short = 'f', long = "force", action = ArgAction::SetTrue)]
        force: bool,
        hook_name: String,
        command: String,
    },
    List,
    Remove {
        hook_name: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum RefsCommand {
    Verify {
        #[arg(long = "strict", action = ArgAction::SetTrue)]
        strict: bool,
        #[arg(long = "verbose", action = ArgAction::SetTrue)]
        verbose: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum RepoCommand {
    Info {
        #[arg(long = "format")]
        format: Option<String>,
        #[arg(short = 'z', action = ArgAction::SetTrue)]
        nul_terminated: bool,
        #[arg(long = "all", action = ArgAction::SetTrue)]
        all: bool,
        #[arg(long = "keys", action = ArgAction::SetTrue)]
        keys: bool,
        keys_or_values: Vec<String>,
    },
    Structure {
        #[arg(long = "format")]
        format: Option<String>,
        #[arg(short = 'z', action = ArgAction::SetTrue)]
        nul_terminated: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum HistoryCommand {
    Reword {
        commit: String,
        #[arg(long = "dry-run", action = ArgAction::SetTrue)]
        dry_run: bool,
        #[arg(long = "update-refs")]
        update_refs: Option<String>,
    },
    Split {
        commit: String,
        #[arg(long = "dry-run", action = ArgAction::SetTrue)]
        dry_run: bool,
        #[arg(long = "update-refs")]
        update_refs: Option<String>,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        pathspecs: Vec<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum RemoteCommand {
    Add {
        #[arg(short = 'm')]
        master: Option<String>,
        name: String,
        url: String,
    },
    #[command(name = "get-url")]
    GetUrl {
        name: String,
    },
    #[command(name = "set-url")]
    SetUrl {
        #[arg(long = "push", action = ArgAction::SetTrue)]
        push: bool,
        #[arg(long = "add", action = ArgAction::SetTrue)]
        add: bool,
        #[arg(long = "delete", action = ArgAction::SetTrue)]
        delete: bool,
        name: String,
        url: String,
        old_url: Option<String>,
    },
    #[command(alias = "rm")]
    Remove {
        name: String,
    },
    Rename {
        old: String,
        new: String,
    },
    #[command(name = "set-head")]
    SetHead {
        name: String,
        #[arg(allow_hyphen_values = true)]
        args: Vec<String>,
    },
    Show {
        #[arg(short = 'n', action = ArgAction::SetTrue)]
        no_query: bool,
        name: String,
    },
    Prune {
        #[arg(short = 'n', long = "dry-run", action = ArgAction::SetTrue)]
        dry_run: bool,
        name: String,
    },
    #[command(name = "set-branches")]
    SetBranches {
        #[arg(long = "add", action = ArgAction::SetTrue)]
        add: bool,
        name: String,
        branches: Vec<String>,
    },
    Update {
        #[arg(short = 'p', long = "prune", action = ArgAction::SetTrue)]
        prune: bool,
        remotes: Vec<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum CommitGraphCommand {
    Write {
        #[arg(long = "reachable", action = ArgAction::SetTrue)]
        reachable: bool,
    },
    Verify,
}

#[derive(Subcommand, Debug)]
pub enum MultiPackIndexCommand {
    Write {
        #[arg(long = "progress", action = ArgAction::SetTrue)]
        progress: bool,
        #[arg(long = "no-progress", action = ArgAction::SetTrue)]
        no_progress: bool,
    },
    Verify {
        #[arg(long = "progress", action = ArgAction::SetTrue)]
        progress: bool,
        #[arg(long = "no-progress", action = ArgAction::SetTrue)]
        no_progress: bool,
    },
    Expire {
        #[arg(long = "progress", action = ArgAction::SetTrue)]
        progress: bool,
        #[arg(long = "no-progress", action = ArgAction::SetTrue)]
        no_progress: bool,
    },
    Repack {
        #[arg(long = "batch-size")]
        batch_size: Option<u64>,
        #[arg(long = "progress", action = ArgAction::SetTrue)]
        progress: bool,
        #[arg(long = "no-progress", action = ArgAction::SetTrue)]
        no_progress: bool,
    },
}

pub struct LsFilesOptions {
    pub cached: bool,
    pub stage: bool,
    pub unmerged: bool,
    pub deleted: bool,
    pub modified: bool,
    pub others: bool,
    pub killed: bool,
    pub directory: bool,
    pub empty_directory: bool,
    pub ignored: bool,
    pub excludes: Vec<String>,
    pub exclude_from: Vec<PathBuf>,
    pub exclude_per_directory: Option<String>,
    pub exclude_standard: bool,
    pub zero: bool,
    pub full_name: bool,
    pub error_unmatch: bool,
    pub tagged: bool,
    pub lowercase_assume_valid: bool,
    pub fsmonitor_clean: bool,
    pub deduplicate: bool,
    pub sparse: bool,
    pub recurse_submodules: bool,
    pub no_recurse_submodules: bool,
    pub debug: bool,
    pub abbrev: Option<usize>,
    pub eol: bool,
    pub format: Option<String>,
    pub with_tree: Option<String>,
    pub resolve_undo: bool,
    pub path_args: Vec<PathBuf>,
}

pub struct RmOptions {
    pub force: bool,
    pub dry_run: bool,
    pub quiet: bool,
    pub recursive: bool,
    pub cached: bool,
    pub ignore_unmatch: bool,
    pub pathspec_from_file: Option<PathBuf>,
    pub pathspec_file_nul: bool,
    pub paths: Vec<PathBuf>,
}

pub struct ConfigArgs {
    pub get: bool,
    pub get_all: bool,
    pub list: bool,
    pub unset: bool,
    pub unset_all: bool,
    pub append: bool,
    pub bool_value: bool,
    pub int_value: bool,
    pub bool_or_int_value: bool,
    pub bool_or_str_value: bool,
    pub path_value: bool,
    pub expiry_date_value: bool,
    pub value_type: Option<String>,
    pub default: Option<String>,
    pub worktree: bool,
    pub local: bool,
    pub global: bool,
    pub file: Option<PathBuf>,
    pub includes: bool,
    pub show_origin: bool,
    pub show_scope: bool,
    pub name: Option<String>,
    pub value: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigValueType {
    Bool,
    Int,
    BoolOrInt,
    BoolOrStr,
    Path,
    ExpiryDate,
    Color,
}

#[derive(Debug, Clone)]
pub struct DiffOptions {
    pub no_index: bool,
    pub nul_terminated: bool,
    pub cached: bool,
    pub reverse: bool,
    pub check: bool,
    pub patch_with_raw: bool,
    pub patch_with_stat: bool,
    pub stat: bool,
    pub compact_summary: bool,
    pub no_patch: bool,
    pub binary: bool,
    pub numstat: bool,
    pub shortstat: bool,
    pub dirstat: Option<String>,
    pub dirstat_by_file: bool,
    pub raw: bool,
    pub summary: bool,
    pub name_status: bool,
    pub name_only: bool,
    pub find_renames: Option<String>,
    pub break_rewrites: Option<String>,
    pub irreversible_delete: bool,
    pub submodule: Option<String>,
    pub ignore_submodules: Option<String>,
    pub find_copies: Option<String>,
    pub find_copies_harder: bool,
    pub no_renames: bool,
    pub dense_combined: bool,
    pub pickaxe_string: Option<String>,
    pub pickaxe_regex: Option<String>,
    pub pickaxe_regex_mode: bool,
    pub pickaxe_all: bool,
    pub order_file: Option<PathBuf>,
    pub skip_to: Option<String>,
    pub rotate_to: Option<String>,
    pub diff_filter: Option<String>,
    pub word_diff: Option<String>,
    pub abbrev: Option<String>,
    pub no_abbrev: bool,
    pub full_index: bool,
    pub no_full_index: bool,
    pub no_prefix: bool,
    pub default_prefix: bool,
    pub src_prefix: Option<String>,
    pub dst_prefix: Option<String>,
    pub relative: Option<String>,
    pub no_relative: bool,
    pub unified: Option<String>,
    pub inter_hunk_context: Option<String>,
    pub minimal: bool,
    pub patience: bool,
    pub histogram: bool,
    pub diff_algorithm: Option<String>,
    pub anchored: Vec<String>,
    pub output_indicator_new: Option<String>,
    pub output_indicator_old: Option<String>,
    pub output_indicator_context: Option<String>,
    pub line_prefix: Option<String>,
    pub ignore_space_at_eol: bool,
    pub ignore_cr_at_eol: bool,
    pub ignore_space_change: bool,
    pub ignore_all_space: bool,
    pub ignore_blank_lines: bool,
    pub ignore_matching_lines: Vec<String>,
    pub no_ext_diff: bool,
    pub no_textconv: bool,
    pub text: bool,
    pub color: Option<String>,
    pub no_color: bool,
    pub no_color_moved: bool,
    pub no_color_moved_ws: bool,
    pub quiet: bool,
    pub exit_code: bool,
    pub paths: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct PlumbingDiffOptions {
    pub recursive: bool,
    pub nul_terminated: bool,
    pub patch: bool,
    pub patch_with_raw: bool,
    pub patch_with_stat: bool,
    pub no_patch: bool,
    pub binary: bool,
    pub stat: bool,
    pub compact_summary: bool,
    pub numstat: bool,
    pub shortstat: bool,
    pub raw: bool,
    pub summary: bool,
    pub name_status: bool,
    pub name_only: bool,
    pub find_renames: Option<String>,
    pub break_rewrites: Option<String>,
    pub irreversible_delete: bool,
    pub submodule: Option<String>,
    pub ignore_submodules: Option<String>,
    pub find_copies: Option<String>,
    pub find_copies_harder: bool,
    pub merge: bool,
    pub combined: bool,
    pub dense_combined: bool,
    pub reverse: bool,
    pub root: bool,
    pub pickaxe_string: Option<String>,
    pub pickaxe_regex: Option<String>,
    pub pickaxe_regex_mode: bool,
    pub pickaxe_all: bool,
    pub order_file: Option<PathBuf>,
    pub skip_to: Option<String>,
    pub rotate_to: Option<String>,
    pub diff_filter: Option<String>,
    pub word_diff: Option<String>,
    pub abbrev: Option<String>,
    pub no_abbrev: bool,
    pub full_index: bool,
    pub no_full_index: bool,
    pub no_prefix: bool,
    pub default_prefix: bool,
    pub src_prefix: Option<String>,
    pub dst_prefix: Option<String>,
    pub relative: Option<String>,
    pub no_relative: bool,
    pub unified: Option<String>,
    pub inter_hunk_context: Option<String>,
    pub minimal: bool,
    pub patience: bool,
    pub histogram: bool,
    pub diff_algorithm: Option<String>,
    pub anchored: Vec<String>,
    pub output_indicator_new: Option<String>,
    pub output_indicator_old: Option<String>,
    pub output_indicator_context: Option<String>,
    pub ignore_space_at_eol: bool,
    pub ignore_cr_at_eol: bool,
    pub ignore_space_change: bool,
    pub ignore_all_space: bool,
    pub ignore_blank_lines: bool,
    pub ignore_matching_lines: Vec<String>,
    pub text: bool,
    pub no_ext_diff: bool,
    pub no_textconv: bool,
    pub color: Option<String>,
    pub no_color: bool,
    pub no_color_moved: bool,
    pub no_color_moved_ws: bool,
    pub quiet: bool,
    pub exit_code: bool,
    pub pretty: Option<String>,
    pub notes: bool,
    pub format: Option<String>,
    pub stdin: bool,
    pub treeish: Option<String>,
    pub new_treeish: Option<String>,
    pub cached: bool,
    pub paths: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct DiffPairsOptions {
    pub nul_terminated: bool,
    pub patch: bool,
    pub no_patch: bool,
    pub stat: bool,
    pub numstat: bool,
    pub shortstat: bool,
    pub raw: bool,
    pub summary: bool,
    pub name_status: bool,
    pub name_only: bool,
    pub word_diff: Option<String>,
    pub quiet: bool,
}

#[derive(Debug, Clone)]
pub struct InterpretTrailersOptions<'a> {
    pub in_place: bool,
    pub trim_empty: bool,
    pub where_: Option<&'a str>,
    pub if_exists: Option<&'a str>,
    pub if_missing: Option<&'a str>,
    pub only_trailers: bool,
    pub only_input: bool,
    pub unfold: bool,
    pub no_divider: bool,
    pub trailers: Vec<String>,
    pub files: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct MergeTreeOptions {
    pub write_tree: bool,
    pub trivial_merge: bool,
    pub messages: bool,
    pub no_messages: bool,
    pub quiet: bool,
    pub nul_terminated: bool,
    pub name_only: bool,
    pub allow_unrelated_histories: bool,
    pub stdin: bool,
    pub merge_base: Option<String>,
    pub strategy_options: Vec<String>,
    pub args: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct IndexPackOptions {
    pub stdin: bool,
    pub output: Option<PathBuf>,
    pub keep: Option<String>,
    pub rev_index: bool,
    pub no_rev_index: bool,
    pub verify: bool,
    pub strict: Option<String>,
    pub fsck_objects: Option<String>,
    pub fix_thin: bool,
    pub verbose: bool,
    pub index_version: Option<String>,
    pub pack_file: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct PackObjectsOptions {
    pub stdout: bool,
    pub revs: bool,
    pub all: bool,
    pub progress: bool,
    pub no_progress: bool,
    pub index_version: Option<String>,
    pub no_reuse_delta: bool,
    pub no_reuse_object: bool,
    pub delta_base_offset: bool,
    pub window: Option<usize>,
    pub depth: Option<usize>,
    pub base_name: Option<PathBuf>,
}
