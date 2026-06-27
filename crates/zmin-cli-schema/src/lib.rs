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
#[command(
    name = "zmin",
    version,
    about = "Thin Git-compatible CLI over zmin-git-core"
)]
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
        #[arg(short = 'q', long = "quiet", action = ArgAction::SetTrue)]
        quiet: bool,
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
        #[arg(short = 't', default_value = "blob")]
        object_type: String,
        #[arg(short = 'w', long = "write", action = ArgAction::SetTrue)]
        write: bool,
        #[arg(long = "stdin", action = ArgAction::SetTrue)]
        stdin: bool,
        #[arg(long = "stdin-paths", action = ArgAction::SetTrue)]
        stdin_paths: bool,
        #[arg(long = "no-filters", action = ArgAction::SetTrue)]
        no_filters: bool,
        #[arg(long = "literally", action = ArgAction::SetTrue)]
        literally: bool,
        #[arg(long = "path")]
        path: Option<String>,
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
        #[arg(long = "allow-unknown-type", action = ArgAction::SetTrue)]
        allow_unknown_type: bool,
        #[arg(short = 'e', long = "exists", action = ArgAction::SetTrue)]
        exists: bool,
        #[arg(long = "use-mailmap", action = ArgAction::SetTrue)]
        use_mailmap: bool,
        #[arg(long = "no-use-mailmap", action = ArgAction::SetTrue)]
        no_use_mailmap: bool,
        #[arg(long = "mailmap", action = ArgAction::SetTrue)]
        mailmap: bool,
        #[arg(long = "no-mailmap", action = ArgAction::SetTrue)]
        no_mailmap: bool,
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
        #[arg(short = 'v', long = "verbose", overrides_with = "no_verbose", action = ArgAction::Count)]
        verbose: u8,
        #[arg(long = "no-verbose", overrides_with = "verbose", action = ArgAction::Count)]
        no_verbose: u8,
        #[arg(short = 'H', long = "human-readable", overrides_with = "no_human_readable", action = ArgAction::Count)]
        human_readable: u8,
        #[arg(long = "no-human-readable", overrides_with = "human_readable", action = ArgAction::Count)]
        no_human_readable: u8,
    },
    UnpackFile {
        object: String,
    },
    ShowIndex {
        #[arg(long = "object-format", action = ArgAction::Append)]
        object_format: Vec<String>,
        #[arg(long = "no-object-format", action = ArgAction::Count)]
        no_object_format: u8,
    },
    UpdateServerInfo {
        #[arg(short = 'f', long = "force", action = ArgAction::Count)]
        force: u8,
        #[arg(long = "no-force", action = ArgAction::Count)]
        no_force: u8,
    },
    CheckRefFormat {
        #[arg(long = "allow-onelevel", action = ArgAction::SetTrue)]
        allow_onelevel: bool,
        #[arg(long = "no-allow-onelevel", action = ArgAction::SetTrue)]
        no_allow_onelevel: bool,
        #[arg(long = "normalize", action = ArgAction::SetTrue)]
        normalize: bool,
        #[arg(long = "refspec-pattern", action = ArgAction::SetTrue)]
        refspec_pattern: bool,
        #[arg(long = "branch", allow_hyphen_values = true)]
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
        #[arg(long = "mailmap-file", value_hint = ValueHint::FilePath)]
        mailmap_file: Option<PathBuf>,
        #[arg(long = "mailmap-blob")]
        mailmap_blob: Option<String>,
        #[arg(long = "stdin", action = ArgAction::Count)]
        stdin: u8,
        #[arg(long = "no-stdin", action = ArgAction::SetTrue)]
        no_stdin: bool,
        identities: Vec<String>,
    },
    CheckAttr {
        #[arg(short = 'a', long = "all", action = ArgAction::SetTrue)]
        all: bool,
        #[arg(long = "cached", action = ArgAction::SetTrue)]
        cached: bool,
        #[arg(long = "stdin", action = ArgAction::SetTrue)]
        stdin: bool,
        #[arg(short = 'z', action = ArgAction::SetTrue)]
        nul: bool,
        #[arg(long = "source")]
        source: Option<String>,
        #[arg(allow_hyphen_values = true)]
        args: Vec<String>,
    },
    UnpackObjects {
        #[arg(short = 'n', action = ArgAction::Count)]
        dry_run: u8,
        #[arg(short = 'q', action = ArgAction::Count)]
        quiet: u8,
        #[arg(short = 'r', action = ArgAction::Count)]
        recover: u8,
        #[arg(long = "strict", action = ArgAction::Count)]
        strict: u8,
        #[arg(long = "max-input-size", require_equals = true)]
        max_input_size: Vec<String>,
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
        #[arg(short = 'q', long = "quiet", action = ArgAction::SetTrue)]
        quiet: bool,
        #[arg(long = "no-quiet", action = ArgAction::SetTrue)]
        no_quiet: bool,
        #[arg(long = "progress", action = ArgAction::SetTrue)]
        progress: bool,
        #[arg(long = "no-progress", action = ArgAction::SetTrue)]
        no_progress: bool,
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
        #[arg(long = "check-self-contained-and-connected", hide = true, action = ArgAction::SetTrue)]
        check_self_contained_and_connected: bool,
        #[arg(long = "fix-thin", action = ArgAction::SetTrue)]
        fix_thin: bool,
        #[arg(short = 'v', action = ArgAction::SetTrue)]
        verbose: bool,
        #[arg(long = "index-version")]
        index_version: Option<String>,
        #[arg(long = "threads", hide = true)]
        threads: Vec<usize>,
        #[arg(long = "max-input-size", hide = true, require_equals = true)]
        max_input_size: Vec<String>,
        #[arg(long = "object-format", hide = true, require_equals = true)]
        object_format: Vec<String>,
        #[arg(long = "promisor", hide = true, num_args = 0..=1, require_equals = true, default_missing_value = "")]
        promisor: Option<String>,
        #[arg(value_hint = ValueHint::FilePath)]
        pack_file: Option<PathBuf>,
    },
    Column {
        #[arg(long = "command")]
        command: Option<String>,
        #[arg(long = "no-command", action = ArgAction::SetTrue)]
        no_command: bool,
        #[arg(long = "mode", num_args = 0..=1, default_missing_value = "")]
        mode: Option<String>,
        #[arg(long = "no-mode", action = ArgAction::SetTrue)]
        no_mode: bool,
        #[arg(long = "raw-mode")]
        raw_mode: Option<String>,
        #[arg(long = "width")]
        width: Option<String>,
        #[arg(long = "no-width", action = ArgAction::SetTrue)]
        no_width: bool,
        #[arg(long = "indent")]
        indent: Option<String>,
        #[arg(long = "no-indent", action = ArgAction::SetTrue)]
        no_indent: bool,
        #[arg(long = "nl")]
        nl: Option<String>,
        #[arg(long = "no-nl", action = ArgAction::SetTrue)]
        no_nl: bool,
        #[arg(long = "padding")]
        padding: Option<String>,
        #[arg(long = "no-padding", action = ArgAction::SetTrue)]
        no_padding: bool,
    },
    GetTarCommitId,
    Archive {
        #[arg(allow_hyphen_values = true, trailing_var_arg = true)]
        args: Vec<String>,
    },
    Credential {
        operation: String,
    },
    CredentialStore {
        #[arg(long = "file", value_hint = ValueHint::FilePath, action = ArgAction::Append)]
        file: Vec<PathBuf>,
        #[arg(long = "no-file", action = ArgAction::SetTrue)]
        no_file: bool,
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
        #[arg(long = "where", overrides_with = "no_where")]
        where_: Option<String>,
        #[arg(long = "no-where", overrides_with = "where_", action = ArgAction::SetTrue)]
        no_where: bool,
        #[arg(long = "if-exists", overrides_with = "no_if_exists")]
        if_exists: Option<String>,
        #[arg(long = "no-if-exists", overrides_with = "if_exists", action = ArgAction::SetTrue)]
        no_if_exists: bool,
        #[arg(long = "if-missing", overrides_with = "no_if_missing")]
        if_missing: Option<String>,
        #[arg(long = "no-if-missing", overrides_with = "if_missing", action = ArgAction::SetTrue)]
        no_if_missing: bool,
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
        #[arg(short = 'b', action = ArgAction::Count)]
        keep_from: u8,
        #[arg(long = "keep-cr", action = ArgAction::Count)]
        keep_cr: u8,
        #[arg(long = "mboxrd", action = ArgAction::Count)]
        mboxrd: u8,
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
        #[arg(long = "no-scissors", action = ArgAction::SetTrue)]
        no_scissors: bool,
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
        #[arg(long = "summary", num_args = 0..=1, default_missing_value = "20")]
        summary: Option<usize>,
        #[arg(long = "no-summary", action = ArgAction::SetTrue)]
        no_summary: bool,
        #[arg(short = 'm', long = "message")]
        message: Option<String>,
        #[arg(long = "into-name")]
        into_name: Option<String>,
        #[arg(short = 'F', long = "file", value_hint = ValueHint::FilePath)]
        file: Option<PathBuf>,
    },
    Shortlog {
        #[arg(long = "oneline", action = ArgAction::SetTrue)]
        oneline: bool,
        #[arg(long = "all", action = ArgAction::SetTrue)]
        all: bool,
        #[arg(
            long = "branches",
            num_args = 0..=1,
            require_equals = true,
            default_missing_value = ""
        )]
        branches: Vec<String>,
        #[arg(
            long = "tags",
            num_args = 0..=1,
            require_equals = true,
            default_missing_value = ""
        )]
        tags: Vec<String>,
        #[arg(long = "author")]
        author: Option<String>,
        #[arg(long = "pretty")]
        pretty: Option<String>,
        #[arg(long = "encoding")]
        encoding: Option<String>,
        #[arg(long = "abbrev-commit", action = ArgAction::SetTrue)]
        abbrev_commit: bool,
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
        #[arg(long = "merges", action = ArgAction::SetTrue)]
        merges: bool,
        #[arg(long = "do-walk", action = ArgAction::SetTrue)]
        do_walk: bool,
        #[arg(long = "no-walk", action = ArgAction::SetTrue)]
        no_walk: bool,
        #[arg(long = "topo-order", action = ArgAction::SetTrue)]
        topo_order: bool,
        #[arg(long = "date-order", action = ArgAction::SetTrue)]
        date_order: bool,
        #[arg(long = "author-date-order", action = ArgAction::SetTrue)]
        author_date_order: bool,
        #[arg(long = "reverse", action = ArgAction::SetTrue)]
        reverse: bool,
        #[arg(long = "alternate-refs", action = ArgAction::SetTrue)]
        alternate_refs: bool,
        #[arg(long = "bisect", action = ArgAction::SetTrue)]
        bisect: bool,
        #[arg(long = "bisect-all", action = ArgAction::SetTrue)]
        bisect_all: bool,
        #[arg(long = "bisect-vars", action = ArgAction::SetTrue)]
        bisect_vars: bool,
        #[arg(long = "cherry", action = ArgAction::SetTrue)]
        cherry: bool,
        #[arg(long = "count", action = ArgAction::SetTrue)]
        count: bool,
        #[arg(long = "dense", action = ArgAction::SetTrue)]
        dense: bool,
        #[arg(long = "full-history", action = ArgAction::SetTrue)]
        full_history: bool,
        #[arg(long = "glob")]
        glob: Option<String>,
        #[arg(long = "in-commit-order", action = ArgAction::SetTrue)]
        in_commit_order: bool,
        #[arg(long = "expand-tabs", action = ArgAction::SetTrue)]
        expand_tabs: bool,
        #[arg(long = "show-linear-break", action = ArgAction::SetTrue)]
        show_linear_break: bool,
        #[arg(long = "left-right", action = ArgAction::SetTrue)]
        left_right: bool,
        #[arg(long = "right-only", action = ArgAction::SetTrue)]
        right_only: bool,
        #[arg(long = "cherry-pick", action = ArgAction::SetTrue)]
        cherry_pick: bool,
        #[arg(long = "cherry-mark", action = ArgAction::SetTrue)]
        cherry_mark: bool,
        #[arg(long = "boundary", action = ArgAction::SetTrue)]
        boundary: bool,
        #[arg(long = "children", action = ArgAction::SetTrue)]
        children: bool,
        #[arg(long = "max-parents")]
        max_parents: Option<String>,
        #[arg(long = "no-max-parents", action = ArgAction::SetTrue)]
        no_max_parents: bool,
        #[arg(long = "min-parents")]
        min_parents: Option<String>,
        #[arg(long = "no-min-parents", action = ArgAction::SetTrue)]
        no_min_parents: bool,
        #[arg(long = "first-parent", action = ArgAction::SetTrue)]
        first_parent: bool,
        #[arg(long = "ignore-missing", action = ArgAction::SetTrue)]
        ignore_missing: bool,
        #[arg(long = "indexed-objects", action = ArgAction::SetTrue)]
        indexed_objects: bool,
        #[arg(long = "unpacked", action = ArgAction::SetTrue)]
        unpacked: bool,
        #[arg(long = "remotes", action = ArgAction::SetTrue)]
        remotes: bool,
        #[arg(long = "remove-empty", action = ArgAction::SetTrue)]
        remove_empty: bool,
        #[arg(long = "notes", action = ArgAction::SetTrue)]
        notes: bool,
        #[arg(long = "no-notes", action = ArgAction::SetTrue)]
        no_notes: bool,
        #[arg(long = "show-notes", action = ArgAction::SetTrue)]
        show_notes: bool,
        #[arg(long = "show-notes-by-default", action = ArgAction::SetTrue)]
        show_notes_by_default: bool,
        #[arg(long = "standard-notes", action = ArgAction::SetTrue)]
        standard_notes: bool,
        #[arg(long = "no-standard-notes", action = ArgAction::SetTrue)]
        no_standard_notes: bool,
        #[arg(long = "no-abbrev-commit", action = ArgAction::SetTrue)]
        no_abbrev_commit: bool,
        #[arg(long = "no-expand-tabs", action = ArgAction::SetTrue)]
        no_expand_tabs: bool,
        #[arg(long = "objects-edge", action = ArgAction::SetTrue)]
        objects_edge: bool,
        #[arg(long = "objects-edge-aggressive", action = ArgAction::SetTrue)]
        objects_edge_aggressive: bool,
        #[arg(long = "quiet", action = ArgAction::SetTrue)]
        quiet: bool,
        #[arg(long = "show-pulls", action = ArgAction::SetTrue)]
        show_pulls: bool,
        #[arg(long = "simplify-merges", action = ArgAction::SetTrue)]
        simplify_merges: bool,
        #[arg(long = "sparse", action = ArgAction::SetTrue)]
        sparse: bool,
        #[arg(long = "parents", action = ArgAction::SetTrue)]
        parents: bool,
        #[arg(long = "objects", action = ArgAction::SetTrue)]
        objects: bool,
        #[arg(long = "graph", action = ArgAction::SetTrue)]
        graph: bool,
        #[arg(long = "show-signature", action = ArgAction::SetTrue)]
        show_signature: bool,
        #[arg(long = "format")]
        format: Vec<String>,
        #[arg(long = "date")]
        date: Vec<String>,
        #[arg(long = "relative-date", action = ArgAction::SetTrue)]
        relative_date: bool,
        #[arg(long = "group")]
        group: Vec<String>,
        #[arg(short = 'w', num_args = 0..=1, default_missing_value = "")]
        wrap: Vec<String>,
        #[arg(long = "stdin", hide = true, action = ArgAction::SetTrue)]
        stdin: bool,
        #[arg(long = "reflog", action = ArgAction::SetTrue)]
        reflog: bool,
        #[arg(short = 'g', long = "walk-reflogs", action = ArgAction::SetTrue)]
        walk_reflogs: bool,
        #[arg(long = "grep-reflog")]
        grep_reflog: Vec<String>,
        #[arg(long = "grep")]
        grep: Vec<String>,
        #[arg(long = "invert-grep", action = ArgAction::SetTrue)]
        invert_grep: bool,
        #[arg(long = "all-match", action = ArgAction::SetTrue)]
        all_match: bool,
        #[arg(short = 'i', long = "regexp-ignore-case", action = ArgAction::SetTrue)]
        regexp_ignore_case: bool,
        #[arg(
            long = "basic-regexp",
            action = ArgAction::SetTrue,
            overrides_with_all = ["extended_regexp", "fixed_strings", "perl_regexp"]
        )]
        basic_regexp: bool,
        #[arg(
            short = 'E',
            long = "extended-regexp",
            action = ArgAction::SetTrue,
            overrides_with_all = ["fixed_strings", "perl_regexp"]
        )]
        extended_regexp: bool,
        #[arg(
            short = 'F',
            long = "fixed-strings",
            action = ArgAction::SetTrue,
            overrides_with_all = ["extended_regexp", "perl_regexp"]
        )]
        fixed_strings: bool,
        #[arg(
            short = 'P',
            long = "perl-regexp",
            action = ArgAction::SetTrue,
            overrides_with_all = ["extended_regexp", "fixed_strings"]
        )]
        perl_regexp: bool,
        #[arg(long = "object-names", action = ArgAction::SetTrue)]
        object_names: bool,
        #[arg(long = "no-object-names", action = ArgAction::SetTrue)]
        no_object_names: bool,
        #[arg(long = "mailmap", action = ArgAction::SetTrue)]
        mailmap: bool,
        #[arg(long = "source", action = ArgAction::SetTrue)]
        source: bool,
        #[arg(long = "commit-header", action = ArgAction::SetTrue)]
        commit_header: bool,
        #[arg(long = "disk-usage", action = ArgAction::SetTrue)]
        disk_usage: bool,
        #[arg(long = "single-worktree", action = ArgAction::SetTrue)]
        single_worktree: bool,
        #[arg(long = "filter")]
        filter: Option<String>,
        #[arg(long = "filter-print-omitted", action = ArgAction::SetTrue)]
        filter_print_omitted: bool,
        #[arg(long = "filter-provided-objects", action = ArgAction::SetTrue)]
        filter_provided_objects: bool,
        #[arg(long = "header", action = ArgAction::SetTrue)]
        header: bool,
        #[arg(long = "progress", action = ArgAction::SetTrue)]
        progress: bool,
        #[arg(long = "no-filter", action = ArgAction::SetTrue)]
        no_filter: bool,
        #[arg(long = "missing", action = ArgAction::SetTrue)]
        missing: bool,
        #[arg(long = "use-bitmap-index", action = ArgAction::SetTrue)]
        use_bitmap_index: bool,
        #[arg(long = "timestamp", action = ArgAction::SetTrue)]
        timestamp: bool,
        #[arg(long = "max-count")]
        max_count: Option<String>,
        #[arg(long = "max-age")]
        max_age: Option<String>,
        #[arg(long = "skip")]
        skip: Option<usize>,
        #[arg(long = "min-age")]
        min_age: Option<String>,
        #[arg(long = "since", alias = "after")]
        since: Option<String>,
        #[arg(long = "until", alias = "before")]
        until: Option<String>,
        #[arg(allow_hyphen_values = true)]
        #[arg(allow_hyphen_values = true)]
        revs: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    Blame {
        #[arg(short = 'h', long = "help", action = ArgAction::SetTrue)]
        help: bool,
        #[arg(short = 'l', action = ArgAction::SetTrue)]
        long: bool,
        #[arg(long = "root", action = ArgAction::SetTrue)]
        root: bool,
        #[arg(long = "contents", value_hint = ValueHint::FilePath)]
        contents: Option<PathBuf>,
        #[arg(long = "encoding")]
        encoding: Option<String>,
        #[arg(long = "first-parent", action = ArgAction::SetTrue)]
        first_parent: bool,
        #[arg(long = "ignore-rev")]
        ignore_rev: Vec<String>,
        #[arg(long = "ignore-revs-file", value_hint = ValueHint::FilePath)]
        ignore_revs_file: Vec<PathBuf>,
        #[arg(long = "reverse")]
        reverse: Option<String>,
        #[arg(short = 'S', value_hint = ValueHint::FilePath)]
        revs_file: Option<PathBuf>,
        #[arg(allow_hyphen_values = true)]
        args: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    Annotate {
        #[arg(short = 'h', long = "help", action = ArgAction::SetTrue)]
        help: bool,
        #[arg(short = 'l', action = ArgAction::SetTrue)]
        long: bool,
        #[arg(short = 'p', long = "porcelain", action = ArgAction::SetTrue)]
        porcelain: bool,
        #[arg(long = "incremental", action = ArgAction::SetTrue)]
        incremental: bool,
        #[arg(long = "line-porcelain", action = ArgAction::SetTrue)]
        line_porcelain: bool,
        #[arg(long = "contents", value_hint = ValueHint::FilePath)]
        contents: Option<PathBuf>,
        #[arg(long = "date")]
        date: Option<String>,
        #[arg(long = "encoding")]
        encoding: Option<String>,
        #[arg(long = "first-parent", action = ArgAction::SetTrue)]
        first_parent: bool,
        #[arg(long = "ignore-rev")]
        ignore_rev: Vec<String>,
        #[arg(long = "ignore-revs-file", value_hint = ValueHint::FilePath)]
        ignore_revs_file: Vec<PathBuf>,
        #[arg(long = "progress", action = ArgAction::SetTrue)]
        progress: bool,
        #[arg(long = "no-progress", action = ArgAction::SetTrue)]
        no_progress: bool,
        #[arg(long = "color-lines", action = ArgAction::SetTrue)]
        color_lines: bool,
        #[arg(long = "color-by-age", action = ArgAction::SetTrue)]
        color_by_age: bool,
        #[arg(long = "reverse")]
        reverse: Option<String>,
        #[arg(long = "root", action = ArgAction::SetTrue)]
        root: bool,
        #[arg(long = "show-stats", action = ArgAction::SetTrue)]
        show_stats: bool,
        #[arg(short = 'C', action = ArgAction::Count)]
        copies: u8,
        #[arg(short = 'L')]
        line_ranges: Vec<String>,
        #[arg(short = 'M', action = ArgAction::Count)]
        moves: u8,
        #[arg(short = 'S', value_hint = ValueHint::FilePath)]
        revs_file: Option<PathBuf>,
        #[arg(short = 'b', action = ArgAction::SetTrue)]
        blank_boundary: bool,
        #[arg(short = 't', action = ArgAction::SetTrue)]
        raw_timestamp: bool,
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
        #[arg(long = "topo-order", action = ArgAction::SetTrue)]
        topo_order: bool,
        #[arg(long = "date-order", action = ArgAction::SetTrue)]
        date_order: bool,
        #[arg(long = "sparse", action = ArgAction::SetTrue)]
        sparse: bool,
        #[arg(long = "color", num_args = 0..=1, require_equals = true, default_missing_value = "always")]
        color: Option<String>,
        #[arg(long = "no-color", action = ArgAction::SetTrue)]
        no_color: bool,
        #[arg(long = "more", require_equals = true)]
        more: Option<isize>,
        #[arg(long = "list", action = ArgAction::SetTrue)]
        list: bool,
        #[arg(long = "independent", action = ArgAction::SetTrue)]
        independent: bool,
        #[arg(long = "merge-base", action = ArgAction::SetTrue)]
        merge_base: bool,
        #[arg(long = "sha1-name", action = ArgAction::SetTrue)]
        sha1_name: bool,
        #[arg(long = "no-name", action = ArgAction::SetTrue)]
        no_name: bool,
        #[arg(long = "topics", action = ArgAction::SetTrue)]
        topics: bool,
        #[arg(
            short = 'g',
            long = "reflog",
            num_args = 0..=1,
            require_equals = true,
            default_missing_value = "",
            allow_hyphen_values = true
        )]
        reflog: Option<String>,
        #[arg(allow_hyphen_values = true)]
        revs: Vec<String>,
    },
    Cherry {
        #[arg(short = 'v', long = "verbose", action = ArgAction::Count)]
        verbose: u8,
        #[arg(long = "no-verbose", action = ArgAction::SetTrue)]
        no_verbose: bool,
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
        #[arg(short = 'p', action = ArgAction::Count)]
        patch: u8,
        start: String,
        url: String,
        end: Option<String>,
    },
    Describe {
        #[arg(long = "all", action = ArgAction::SetTrue)]
        all: bool,
        #[arg(long = "tags", action = ArgAction::SetTrue)]
        tags: bool,
        #[arg(long = "contains", action = ArgAction::SetTrue)]
        contains: bool,
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
        #[arg(
            long = "broken",
            num_args = 0..=1,
            default_missing_value = "-broken",
            require_equals = true
        )]
        broken: Option<String>,
        #[arg(long = "candidates")]
        candidates: Option<usize>,
        #[arg(long = "debug", action = ArgAction::SetTrue)]
        debug: bool,
        #[arg(long = "first-parent", action = ArgAction::SetTrue)]
        first_parent: bool,
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
        #[arg(long = "no-undefined", action = ArgAction::SetTrue)]
        no_undefined: bool,
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
        #[command(subcommand)]
        command: Option<ReflogCommand>,
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
        #[arg(short = 'v', long = "verbose", action = ArgAction::Count)]
        verbose: u8,
        #[arg(long = "no-verbose", action = ArgAction::Count)]
        no_verbose: u8,
        #[arg(short = 's', long = "stat-only", action = ArgAction::Count)]
        stat_only: u8,
        #[arg(long = "no-stat-only", action = ArgAction::Count)]
        no_stat_only: u8,
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
        #[arg(long = "format")]
        format: Option<String>,
        tags: Vec<String>,
    },
    UpdateIndex {
        #[arg(long = "add", action = ArgAction::Count)]
        add: u8,
        #[arg(long = "remove", action = ArgAction::Count)]
        remove: u8,
        #[arg(long = "force-remove", action = ArgAction::Count)]
        force_remove: u8,
        #[arg(long = "replace", action = ArgAction::Count)]
        replace: u8,
        #[arg(short = 'g', long = "again", action = ArgAction::Count)]
        again: u8,
        #[arg(short = 'q', action = ArgAction::Count)]
        quiet: u8,
        #[arg(long = "refresh", action = ArgAction::Count)]
        refresh: u8,
        #[arg(long = "really-refresh", action = ArgAction::Count)]
        really_refresh: u8,
        #[arg(long = "ignore-submodules", action = ArgAction::Count)]
        ignore_submodules: u8,
        #[arg(long = "ignore-missing", action = ArgAction::Count)]
        ignore_missing: u8,
        #[arg(long = "unmerged", action = ArgAction::Count)]
        unmerged: u8,
        #[arg(long = "ignore-skip-worktree-entries", action = ArgAction::Count)]
        ignore_skip_worktree_entries: u8,
        #[arg(long = "no-ignore-skip-worktree-entries", action = ArgAction::Count)]
        no_ignore_skip_worktree_entries: u8,
        #[arg(long = "unresolve", action = ArgAction::Count)]
        unresolve: u8,
        #[arg(long = "info-only", action = ArgAction::Count)]
        info_only: u8,
        #[arg(long = "cacheinfo")]
        cacheinfo: Vec<String>,
        #[arg(long = "index-info", action = ArgAction::Count)]
        index_info_mode: u8,
        #[arg(long = "index-version")]
        index_version: Option<String>,
        #[arg(long = "show-index-version", action = ArgAction::Count)]
        show_index_version: u8,
        #[arg(long = "split-index", action = ArgAction::Count)]
        split_index: u8,
        #[arg(long = "no-split-index", action = ArgAction::Count)]
        no_split_index: u8,
        #[arg(long = "untracked-cache", action = ArgAction::Count)]
        untracked_cache: u8,
        #[arg(long = "no-untracked-cache", action = ArgAction::Count)]
        no_untracked_cache: u8,
        #[arg(long = "force-untracked-cache", action = ArgAction::Count)]
        force_untracked_cache: u8,
        #[arg(long = "test-untracked-cache", action = ArgAction::Count)]
        test_untracked_cache: u8,
        #[arg(long = "fsmonitor", action = ArgAction::Count)]
        fsmonitor: u8,
        #[arg(long = "no-fsmonitor", action = ArgAction::Count)]
        no_fsmonitor: u8,
        #[arg(long = "fsmonitor-valid", action = ArgAction::Count)]
        fsmonitor_valid: u8,
        #[arg(long = "no-fsmonitor-valid", action = ArgAction::Count)]
        no_fsmonitor_valid: u8,
        #[arg(long = "verbose", action = ArgAction::Count)]
        verbose: u8,
        #[arg(long = "chmod")]
        chmod: Option<String>,
        #[arg(long = "assume-unchanged", action = ArgAction::Count)]
        assume_unchanged: u8,
        #[arg(long = "no-assume-unchanged", action = ArgAction::Count)]
        no_assume_unchanged: u8,
        #[arg(long = "skip-worktree", action = ArgAction::Count)]
        skip_worktree: u8,
        #[arg(long = "no-skip-worktree", action = ArgAction::Count)]
        no_skip_worktree: u8,
        #[arg(long = "stdin", action = ArgAction::Count)]
        stdin: u8,
        #[arg(short = 'z', action = ArgAction::Count)]
        nul_terminated: u8,
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
        #[arg(long = "no-diagnose", action = ArgAction::SetTrue)]
        no_diagnose: bool,
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
        #[arg(long = "stable", action = ArgAction::Count)]
        stable: u8,
        #[arg(long = "unstable", action = ArgAction::Count)]
        unstable: u8,
        #[arg(long = "verbatim", action = ArgAction::Count)]
        verbatim: u8,
    },
    Stripspace {
        #[arg(short = 's', long = "strip-comments", action = ArgAction::Count)]
        strip_comments: u8,
        #[arg(short = 'c', long = "comment-lines", action = ArgAction::Count)]
        comment_lines: u8,
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
        #[arg(long = "no-branch", overrides_with = "branch", action = ArgAction::SetTrue)]
        no_branch: bool,
        #[arg(long = "ahead-behind", overrides_with = "no_ahead_behind", action = ArgAction::SetTrue)]
        ahead_behind: bool,
        #[arg(long = "no-ahead-behind", overrides_with = "ahead_behind", action = ArgAction::SetTrue)]
        no_ahead_behind: bool,
        #[arg(long = "show-stash", overrides_with = "no_show_stash", action = ArgAction::SetTrue)]
        show_stash: bool,
        #[arg(long = "no-show-stash", overrides_with = "show_stash", action = ArgAction::SetTrue)]
        no_show_stash: bool,
        #[arg(short = 'v', long = "verbose", overrides_with = "no_verbose", action = ArgAction::Count)]
        verbose: u8,
        #[arg(long = "no-verbose", overrides_with = "verbose", action = ArgAction::SetTrue)]
        no_verbose: bool,
        #[arg(long = "long", overrides_with = "short", action = ArgAction::SetTrue)]
        long: bool,
        #[arg(long = "no-long", overrides_with = "short", action = ArgAction::SetTrue)]
        no_long: bool,
        #[arg(
            long = "column",
            overrides_with = "no_column",
            num_args = 0..=1,
            default_missing_value = "always",
            require_equals = true
        )]
        column: Option<String>,
        #[arg(long = "no-column", overrides_with = "column", action = ArgAction::SetTrue)]
        no_column: bool,
        #[arg(long = "renames", overrides_with = "no_renames", action = ArgAction::SetTrue)]
        renames: bool,
        #[arg(long = "no-renames", overrides_with = "renames", action = ArgAction::SetTrue)]
        no_renames: bool,
        #[arg(
            short = 'M',
            long = "find-renames",
            overrides_with = "no_renames",
            num_args = 0..=1,
            require_equals = true,
            default_missing_value = ""
        )]
        find_renames: Option<String>,
        #[arg(
            long = "ignore-submodules",
            num_args = 0..=1,
            default_missing_value = "all"
        )]
        ignore_submodules: Option<String>,
        #[arg(long = "untracked-cache", hide = true, action = ArgAction::SetTrue)]
        untracked_cache: bool,
        #[arg(long = "no-untracked-cache", hide = true, action = ArgAction::SetTrue)]
        no_untracked_cache: bool,
        #[arg(long = "split-index", hide = true, action = ArgAction::SetTrue)]
        split_index: bool,
        #[arg(long = "no-split-index", hide = true, action = ArgAction::SetTrue)]
        no_split_index: bool,
        #[arg(
            short = 's',
            long = "short",
            overrides_with_all = ["long", "no_long"],
            action = ArgAction::SetTrue
        )]
        short: bool,
        #[arg(short = 'z', long = "null", action = ArgAction::SetTrue)]
        null: bool,
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
        #[arg(short = 'z', long = "null", action = ArgAction::SetTrue)]
        null: bool,
        #[arg(long = "all", action = ArgAction::SetTrue)]
        all: bool,
        #[arg(long = "blob")]
        blob: Option<String>,
        #[arg(long = "comment")]
        comment: Option<String>,
        #[arg(long = "fixed-value", action = ArgAction::SetTrue)]
        fixed_value: bool,
        #[arg(long = "get", action = ArgAction::SetTrue)]
        get: bool,
        #[arg(long = "get-all", action = ArgAction::SetTrue)]
        get_all: bool,
        #[arg(long = "get-colorbool", action = ArgAction::SetTrue)]
        get_colorbool: bool,
        #[arg(long = "get-regexp", action = ArgAction::SetTrue)]
        get_regexp: bool,
        #[arg(long = "list", short = 'l', action = ArgAction::SetTrue)]
        list: bool,
        #[arg(long = "name-only", action = ArgAction::SetTrue)]
        name_only: bool,
        #[arg(long = "no-includes", action = ArgAction::SetTrue)]
        no_includes: bool,
        #[arg(long = "no-type", action = ArgAction::SetTrue)]
        no_type: bool,
        #[arg(long = "regexp", action = ArgAction::SetTrue)]
        regexp: bool,
        #[arg(long = "replace-all", action = ArgAction::SetTrue)]
        replace_all: bool,
        #[arg(long = "system", action = ArgAction::SetTrue)]
        system: bool,
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
        #[arg(long = "url")]
        url: Option<String>,
        #[arg(long = "value")]
        value_pattern: Option<String>,
        arg0: Option<String>,
        #[arg(allow_hyphen_values = true)]
        arg1: Option<String>,
        #[arg(allow_hyphen_values = true)]
        arg2: Option<String>,
    },
    Var {
        #[arg(short = 'l', action = ArgAction::SetTrue)]
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
        #[arg(short = 'm', long = "module-path")]
        module_path: Option<String>,
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
        #[arg(long = "no-all", action = ArgAction::SetTrue)]
        no_all: bool,
        #[arg(long = "multiple", action = ArgAction::SetTrue)]
        multiple: bool,
        #[arg(long = "prefetch", action = ArgAction::SetTrue)]
        prefetch: bool,
        #[arg(short = 'q', long = "quiet", action = ArgAction::SetTrue)]
        quiet: bool,
        #[arg(short = 'v', long = "verbose", action = ArgAction::SetTrue)]
        verbose: bool,
        #[arg(long = "progress", action = ArgAction::SetTrue)]
        progress: bool,
        #[arg(short = 'n', long = "dry-run", action = ArgAction::SetTrue)]
        dry_run: bool,
        #[arg(short = 'f', long = "force", action = ArgAction::SetTrue)]
        force: bool,
        #[arg(long = "auto-gc", action = ArgAction::SetTrue)]
        auto_gc: bool,
        #[arg(long = "auto-maintenance", action = ArgAction::SetTrue)]
        auto_maintenance: bool,
        #[arg(long = "no-auto-gc", action = ArgAction::SetTrue)]
        no_auto_gc: bool,
        #[arg(long = "no-auto-maintenance", action = ArgAction::SetTrue)]
        no_auto_maintenance: bool,
        #[arg(long = "set-upstream", action = ArgAction::SetTrue)]
        set_upstream: bool,
        #[arg(short = 'a', long = "append", action = ArgAction::SetTrue)]
        append: bool,
        #[arg(short = 'p', long = "prune", overrides_with = "no_prune", action = ArgAction::SetTrue)]
        prune: bool,
        #[arg(long = "no-prune", overrides_with = "prune", action = ArgAction::SetTrue)]
        no_prune: bool,
        #[arg(short = 'P', long = "prune-tags", action = ArgAction::SetTrue)]
        prune_tags: bool,
        #[arg(long = "no-tags", action = ArgAction::SetTrue)]
        no_tags: bool,
        #[arg(short = 't', long = "tags", action = ArgAction::SetTrue)]
        tags: bool,
        #[arg(long = "atomic", action = ArgAction::SetTrue)]
        atomic: bool,
        #[arg(short = 'k', long = "keep", action = ArgAction::SetTrue)]
        keep: bool,
        #[arg(short = '4', long = "ipv4", action = ArgAction::SetTrue)]
        ipv4: bool,
        #[arg(short = '6', long = "ipv6", action = ArgAction::SetTrue)]
        ipv6: bool,
        #[arg(
            long = "recurse-submodules",
            num_args = 0..=1,
            require_equals = true,
            default_missing_value = "yes",
            action = ArgAction::Append
        )]
        recurse_submodules: Vec<String>,
        #[arg(long = "no-recurse-submodules", action = ArgAction::SetTrue)]
        no_recurse_submodules: bool,
        #[arg(short = 'j', long = "jobs", allow_hyphen_values = true)]
        jobs: Option<String>,
        #[arg(short = 'u', long = "update-head-ok", action = ArgAction::SetTrue)]
        update_head_ok: bool,
        #[arg(long = "write-fetch-head", overrides_with = "no_write_fetch_head", action = ArgAction::SetTrue)]
        write_fetch_head: bool,
        #[arg(long = "no-write-fetch-head", overrides_with = "write_fetch_head", action = ArgAction::SetTrue)]
        no_write_fetch_head: bool,
        #[arg(long = "write-commit-graph", action = ArgAction::SetTrue)]
        write_commit_graph: bool,
        #[arg(long = "no-write-commit-graph", action = ArgAction::SetTrue)]
        no_write_commit_graph: bool,
        #[arg(long = "refmap", num_args = 0..=1, default_missing_value = "", require_equals = true)]
        refmap: Vec<String>,
        #[arg(long = "depth")]
        depth: Option<String>,
        #[arg(long = "deepen")]
        deepen: Option<String>,
        #[arg(long = "unshallow", action = ArgAction::SetTrue)]
        unshallow: bool,
        #[arg(long = "update-shallow", action = ArgAction::SetTrue)]
        update_shallow: bool,
        #[arg(long = "shallow-since")]
        shallow_since: Option<String>,
        #[arg(long = "shallow-exclude")]
        shallow_exclude: Vec<String>,
        #[arg(long = "negotiation-tip")]
        negotiation_tip: Vec<String>,
        #[arg(long = "negotiate-only", action = ArgAction::SetTrue)]
        negotiate_only: bool,
        #[arg(short = 'o', long = "server-option")]
        server_option: Vec<String>,
        #[arg(long = "show-forced-updates", action = ArgAction::SetTrue)]
        show_forced_updates: bool,
        #[arg(long = "no-show-forced-updates", action = ArgAction::SetTrue)]
        no_show_forced_updates: bool,
        #[arg(long = "upload-pack")]
        upload_pack: Option<String>,
        #[arg(long = "filter")]
        filter: Option<String>,
        #[arg(long = "stdin", action = ArgAction::SetTrue)]
        stdin: bool,
        #[arg(long = "porcelain", action = ArgAction::SetTrue)]
        porcelain: bool,
        #[arg(long = "recurse-submodules-default")]
        recurse_submodules_default: Option<String>,
        #[arg(long = "refetch", action = ArgAction::SetTrue)]
        refetch: bool,
        #[arg(long = "submodule-prefix")]
        submodule_prefix: Option<String>,
        remote: Option<String>,
        branch: Vec<String>,
    },
    Pull {
        #[arg(long = "all", action = ArgAction::SetTrue)]
        all: bool,
        #[arg(long = "no-all", action = ArgAction::SetTrue)]
        no_all: bool,
        #[arg(short = 'v', long = "verbose", action = ArgAction::SetTrue)]
        verbose: bool,
        #[arg(short = 'p', long = "prune", action = ArgAction::SetTrue)]
        prune: bool,
        #[arg(long = "no-tags", action = ArgAction::SetTrue)]
        no_tags: bool,
        #[arg(short = 't', long = "tags", action = ArgAction::SetTrue)]
        tags: bool,
        #[arg(short = 'j', long = "jobs", allow_hyphen_values = true)]
        jobs: Option<String>,
        #[arg(short = 'k', long = "keep", action = ArgAction::SetTrue)]
        keep: bool,
        #[arg(short = '4', long = "ipv4", action = ArgAction::SetTrue)]
        ipv4: bool,
        #[arg(short = '6', long = "ipv6", action = ArgAction::SetTrue)]
        ipv6: bool,
        #[arg(
            long = "recurse-submodules",
            num_args = 0..=1,
            require_equals = true,
            default_missing_value = "yes",
            action = ArgAction::Append
        )]
        recurse_submodules: Vec<String>,
        #[arg(long = "no-recurse-submodules", action = ArgAction::SetTrue)]
        no_recurse_submodules: bool,
        #[arg(short = 'o', long = "server-option")]
        server_option: Vec<String>,
        #[arg(long = "show-forced-updates", action = ArgAction::SetTrue)]
        show_forced_updates: bool,
        #[arg(long = "no-show-forced-updates", action = ArgAction::SetTrue)]
        no_show_forced_updates: bool,
        #[arg(long = "ff", action = ArgAction::SetTrue)]
        ff: bool,
        #[arg(long = "ff-only", action = ArgAction::SetTrue)]
        ff_only: bool,
        #[arg(long = "no-ff", action = ArgAction::SetTrue)]
        no_ff: bool,
        #[arg(short = 's', long = "strategy")]
        strategies: Vec<String>,
        #[arg(long = "rebase", num_args = 0..=1, default_missing_value = "true")]
        rebase: Option<String>,
        #[arg(long = "no-rebase", action = ArgAction::SetTrue)]
        no_rebase: bool,
        #[arg(long = "depth")]
        depth: Option<String>,
        #[arg(long = "deepen")]
        deepen: Option<String>,
        #[arg(long = "unshallow", action = ArgAction::SetTrue)]
        unshallow: bool,
        #[arg(long = "update-shallow", action = ArgAction::SetTrue)]
        update_shallow: bool,
        #[arg(long = "shallow-since")]
        shallow_since: Option<String>,
        #[arg(long = "shallow-exclude")]
        shallow_exclude: Vec<String>,
        #[arg(long = "upload-pack")]
        upload_pack: Option<String>,
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
        #[arg(
            long = "empty-directory",
            action = ArgAction::SetTrue,
            overrides_with = "no_empty_directory"
        )]
        empty_directory: bool,
        #[arg(
            long = "no-empty-directory",
            action = ArgAction::SetTrue,
            overrides_with = "empty_directory"
        )]
        no_empty_directory: bool,
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
        #[arg(short = 'A', long = "all", action = ArgAction::Count)]
        all: u8,
        #[arg(long = "no-all", alias = "ignore-removal", action = ArgAction::Count)]
        ignore_removal: u8,
        #[arg(long = "no-ignore-removal", action = ArgAction::Count)]
        no_ignore_removal: u8,
        #[arg(short = 'f', long = "force", action = ArgAction::Count)]
        force: u8,
        #[arg(short = 'u', long = "update", action = ArgAction::Count)]
        update: u8,
        #[arg(long = "no-update", action = ArgAction::Count)]
        no_update: u8,
        #[arg(long = "renormalize", action = ArgAction::Count)]
        renormalize: u8,
        #[arg(long = "no-renormalize", action = ArgAction::Count)]
        no_renormalize: u8,
        #[arg(short = 'N', long = "intent-to-add", action = ArgAction::Count)]
        intent_to_add: u8,
        #[arg(long = "no-intent-to-add", action = ArgAction::Count)]
        no_intent_to_add: u8,
        #[arg(long = "refresh", action = ArgAction::Count)]
        refresh: u8,
        #[arg(long = "no-refresh", action = ArgAction::Count)]
        no_refresh: u8,
        #[arg(short = 'v', long = "verbose", action = ArgAction::Count)]
        verbose: u8,
        #[arg(long = "no-verbose", action = ArgAction::Count)]
        no_verbose: u8,
        #[arg(long = "ignore-errors", action = ArgAction::Count)]
        ignore_errors: u8,
        #[arg(long = "no-ignore-errors", action = ArgAction::Count)]
        no_ignore_errors: u8,
        #[arg(long = "ignore-missing", action = ArgAction::Count)]
        ignore_missing: u8,
        #[arg(long = "no-ignore-missing", action = ArgAction::Count)]
        no_ignore_missing: u8,
        #[arg(long = "sparse", action = ArgAction::Count)]
        sparse: u8,
        #[arg(long = "no-sparse", action = ArgAction::Count)]
        no_sparse: u8,
        #[arg(long = "no-warn-embedded-repo", action = ArgAction::Count)]
        no_warn_embedded_repo: u8,
        #[arg(short = 'i', long = "interactive", action = ArgAction::Count)]
        interactive: u8,
        #[arg(short = 'p', long = "patch", action = ArgAction::Count)]
        patch: u8,
        #[arg(short = 'e', long = "edit", action = ArgAction::Count)]
        edit: u8,
        #[arg(long = "chmod")]
        chmod: Vec<String>,
        #[arg(long = "no-chmod", action = ArgAction::Count)]
        no_chmod: u8,
        #[arg(short = 'n', long = "dry-run", action = ArgAction::Count)]
        dry_run: u8,
        #[arg(long = "no-dry-run", action = ArgAction::Count)]
        no_dry_run: u8,
        #[arg(long = "pathspec-from-file", value_hint = ValueHint::FilePath)]
        pathspec_from_file: Option<PathBuf>,
        #[arg(long = "no-pathspec-from-file", action = ArgAction::Count)]
        no_pathspec_from_file: u8,
        #[arg(long = "pathspec-file-nul", action = ArgAction::Count)]
        pathspec_file_nul: u8,
        #[arg(long = "no-pathspec-file-nul", action = ArgAction::Count)]
        no_pathspec_file_nul: u8,
        #[arg(value_hint = ValueHint::AnyPath)]
        paths: Vec<PathBuf>,
    },
    Stage {
        #[arg(short = 'A', long = "all", action = ArgAction::Count)]
        all: u8,
        #[arg(long = "no-all", alias = "ignore-removal", action = ArgAction::Count)]
        ignore_removal: u8,
        #[arg(long = "no-ignore-removal", action = ArgAction::Count)]
        no_ignore_removal: u8,
        #[arg(short = 'f', long = "force", action = ArgAction::Count)]
        force: u8,
        #[arg(short = 'u', long = "update", action = ArgAction::Count)]
        update: u8,
        #[arg(long = "no-update", action = ArgAction::Count)]
        no_update: u8,
        #[arg(long = "renormalize", action = ArgAction::Count)]
        renormalize: u8,
        #[arg(long = "no-renormalize", action = ArgAction::Count)]
        no_renormalize: u8,
        #[arg(short = 'N', long = "intent-to-add", action = ArgAction::Count)]
        intent_to_add: u8,
        #[arg(long = "no-intent-to-add", action = ArgAction::Count)]
        no_intent_to_add: u8,
        #[arg(long = "refresh", action = ArgAction::Count)]
        refresh: u8,
        #[arg(long = "no-refresh", action = ArgAction::Count)]
        no_refresh: u8,
        #[arg(short = 'v', long = "verbose", action = ArgAction::Count)]
        verbose: u8,
        #[arg(long = "no-verbose", action = ArgAction::Count)]
        no_verbose: u8,
        #[arg(long = "ignore-errors", action = ArgAction::Count)]
        ignore_errors: u8,
        #[arg(long = "no-ignore-errors", action = ArgAction::Count)]
        no_ignore_errors: u8,
        #[arg(long = "ignore-missing", action = ArgAction::Count)]
        ignore_missing: u8,
        #[arg(long = "no-ignore-missing", action = ArgAction::Count)]
        no_ignore_missing: u8,
        #[arg(long = "sparse", action = ArgAction::Count)]
        sparse: u8,
        #[arg(long = "no-sparse", action = ArgAction::Count)]
        no_sparse: u8,
        #[arg(long = "no-warn-embedded-repo", action = ArgAction::Count)]
        no_warn_embedded_repo: u8,
        #[arg(short = 'i', long = "interactive", action = ArgAction::Count)]
        interactive: u8,
        #[arg(short = 'p', long = "patch", action = ArgAction::Count)]
        patch: u8,
        #[arg(short = 'e', long = "edit", action = ArgAction::Count)]
        edit: u8,
        #[arg(long = "chmod")]
        chmod: Vec<String>,
        #[arg(long = "no-chmod", action = ArgAction::Count)]
        no_chmod: u8,
        #[arg(short = 'n', long = "dry-run", action = ArgAction::Count)]
        dry_run: u8,
        #[arg(long = "no-dry-run", action = ArgAction::Count)]
        no_dry_run: u8,
        #[arg(long = "pathspec-from-file", value_hint = ValueHint::FilePath)]
        pathspec_from_file: Option<PathBuf>,
        #[arg(long = "no-pathspec-from-file", action = ArgAction::Count)]
        no_pathspec_from_file: u8,
        #[arg(long = "pathspec-file-nul", action = ArgAction::Count)]
        pathspec_file_nul: u8,
        #[arg(long = "no-pathspec-file-nul", action = ArgAction::Count)]
        no_pathspec_file_nul: u8,
        #[arg(value_hint = ValueHint::AnyPath)]
        paths: Vec<PathBuf>,
    },
    Rm {
        #[arg(short = 'f', long = "force", action = ArgAction::Count)]
        force: u8,
        #[arg(long = "no-force", action = ArgAction::Count)]
        no_force: u8,
        #[arg(short = 'n', long = "dry-run", action = ArgAction::Count)]
        dry_run: u8,
        #[arg(long = "no-dry-run", action = ArgAction::Count)]
        no_dry_run: u8,
        #[arg(short = 'q', long = "quiet", action = ArgAction::Count)]
        quiet: u8,
        #[arg(long = "no-quiet", action = ArgAction::Count)]
        no_quiet: u8,
        #[arg(short = 'r', action = ArgAction::Count)]
        recursive: u8,
        #[arg(long = "cached", action = ArgAction::Count)]
        cached: u8,
        #[arg(long = "no-cached", action = ArgAction::Count)]
        no_cached: u8,
        #[arg(long = "ignore-unmatch", action = ArgAction::Count)]
        ignore_unmatch: u8,
        #[arg(long = "no-ignore-unmatch", action = ArgAction::Count)]
        no_ignore_unmatch: u8,
        #[arg(long = "sparse", action = ArgAction::Count)]
        sparse: u8,
        #[arg(long = "no-sparse", action = ArgAction::Count)]
        no_sparse: u8,
        #[arg(long = "pathspec-from-file", value_hint = ValueHint::FilePath)]
        pathspec_from_file: Option<PathBuf>,
        #[arg(long = "pathspec-file-nul", action = ArgAction::Count)]
        pathspec_file_nul: u8,
        #[arg(long = "no-pathspec-file-nul", action = ArgAction::Count)]
        no_pathspec_file_nul: u8,
        #[arg(value_hint = ValueHint::AnyPath)]
        paths: Vec<PathBuf>,
    },
    Mv {
        #[arg(short = 'f', long = "force", action = ArgAction::Count)]
        force: u8,
        #[arg(long = "no-force", action = ArgAction::Count)]
        no_force: u8,
        #[arg(short = 'n', long = "dry-run", action = ArgAction::Count)]
        dry_run: u8,
        #[arg(long = "no-dry-run", action = ArgAction::Count)]
        no_dry_run: u8,
        #[arg(short = 'v', long = "verbose", action = ArgAction::Count)]
        verbose: u8,
        #[arg(long = "no-verbose", action = ArgAction::Count)]
        no_verbose: u8,
        #[arg(short = 'k', action = ArgAction::Count)]
        skip_errors: u8,
        #[arg(long = "sparse", action = ArgAction::Count)]
        sparse: u8,
        #[arg(long = "no-sparse", action = ArgAction::Count)]
        no_sparse: u8,
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
        #[arg(long = "dry-run", action = ArgAction::SetTrue)]
        dry_run: bool,
        #[arg(long = "short", action = ArgAction::SetTrue)]
        short: bool,
        #[arg(long = "branch", action = ArgAction::SetTrue)]
        branch: bool,
        #[arg(short = 'z', long = "null", action = ArgAction::SetTrue)]
        null: bool,
        #[arg(long = "porcelain", action = ArgAction::SetTrue)]
        porcelain: bool,
        #[arg(long = "long", action = ArgAction::SetTrue)]
        long: bool,
        #[arg(short = 'n', long = "no-verify", action = ArgAction::SetTrue)]
        no_verify: bool,
        #[arg(long = "status", action = ArgAction::SetTrue, overrides_with = "no_status")]
        status: bool,
        #[arg(long = "no-status", action = ArgAction::SetTrue, overrides_with = "status")]
        no_status: bool,
        #[arg(
            short = 'u',
            long = "untracked-files",
            num_args = 0..=1,
            default_missing_value = "all"
        )]
        untracked_files: Option<String>,
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
        prefix: Vec<String>,
        #[arg(long = "no-prefix", action = ArgAction::Count)]
        no_prefix: u8,
        #[arg(long = "missing-ok", action = ArgAction::Count)]
        missing_ok: u8,
        #[arg(long = "no-missing-ok", action = ArgAction::Count)]
        no_missing_ok: u8,
    },
    CommitTree {
        tree: String,
        #[arg(short = 'p')]
        parents: Vec<String>,
        #[arg(short = 'm')]
        messages: Vec<String>,
        #[arg(short = 'F')]
        message_files: Vec<PathBuf>,
        #[arg(
            short = 'S',
            long = "gpg-sign",
            num_args = 0..=1,
            require_equals = true,
            default_missing_value = "",
            overrides_with = "no_gpg_sign"
        )]
        gpg_sign: Option<String>,
        #[arg(long = "no-gpg-sign", action = ArgAction::Count, overrides_with = "gpg_sign")]
        no_gpg_sign: u8,
    },
    Mktree {
        #[arg(short = 'z', action = ArgAction::Count)]
        nul_terminated: u8,
        #[arg(long = "missing", overrides_with = "no_missing", action = ArgAction::Count)]
        missing: u8,
        #[arg(long = "no-missing", overrides_with = "missing", action = ArgAction::Count)]
        no_missing: u8,
        #[arg(long = "batch", overrides_with = "no_batch", action = ArgAction::Count)]
        batch: u8,
        #[arg(long = "no-batch", overrides_with = "batch", action = ArgAction::Count)]
        no_batch: u8,
    },
    Mktag {
        #[arg(long = "strict", action = ArgAction::SetTrue)]
        strict: bool,
        #[arg(long = "no-strict", action = ArgAction::SetTrue)]
        no_strict: bool,
    },
    PackRefs {
        #[arg(long = "all", action = ArgAction::SetTrue)]
        all: bool,
        #[arg(long = "auto", action = ArgAction::SetTrue)]
        auto: bool,
        #[arg(long = "include")]
        include: Vec<String>,
        #[arg(long = "exclude")]
        exclude: Vec<String>,
        #[arg(long = "prune", overrides_with = "no_prune", action = ArgAction::SetTrue)]
        prune: bool,
        #[arg(long = "no-prune", overrides_with = "prune", action = ArgAction::SetTrue)]
        no_prune: bool,
    },
    PrunePacked {
        #[arg(short = 'n', long = "dry-run", action = ArgAction::Count)]
        dry_run: u8,
        #[arg(long = "no-dry-run", action = ArgAction::Count)]
        no_dry_run: u8,
        #[arg(short = 'q', long = "quiet", action = ArgAction::Count)]
        quiet: u8,
        #[arg(long = "no-quiet", action = ArgAction::Count)]
        no_quiet: u8,
    },
    Repack {
        #[arg(short = 'a', action = ArgAction::Count)]
        all: u8,
        #[arg(short = 'A', action = ArgAction::Count)]
        all_and_loosen_unreachable: u8,
        #[arg(short = 'd', action = ArgAction::Count)]
        delete_redundant: u8,
        #[arg(short = 'q', long = "quiet", action = ArgAction::Count)]
        quiet: u8,
        #[arg(short = 'n', action = ArgAction::Count)]
        no_update_server_info: u8,
        #[arg(short = 'f', action = ArgAction::Count)]
        no_reuse_delta: u8,
        #[arg(short = 'F', action = ArgAction::Count)]
        no_reuse_object: u8,
        #[arg(long = "delta-islands", action = ArgAction::SetTrue)]
        delta_islands: bool,
        #[arg(short = 'i', action = ArgAction::Count)]
        delta_islands_short: u8,
        #[arg(short = 'k', action = ArgAction::Count)]
        keep_unreachable_short: u8,
        #[arg(long = "keep-unreachable", action = ArgAction::SetTrue)]
        keep_unreachable: bool,
        #[arg(long = "cruft", action = ArgAction::Count)]
        cruft: u8,
        #[arg(long = "cruft-expiration")]
        cruft_expiration: Vec<String>,
        #[arg(long = "expire-to", value_hint = ValueHint::DirPath)]
        expire_to: Vec<PathBuf>,
        #[arg(short = 'l', long = "local", action = ArgAction::Count)]
        local: u8,
        #[arg(long = "pack-kept-objects", action = ArgAction::SetTrue)]
        pack_kept_objects: bool,
        #[arg(short = 'b', long = "write-bitmap-index", action = ArgAction::Count)]
        write_bitmap_index: u8,
        #[arg(long = "no-write-bitmap-index", action = ArgAction::SetTrue)]
        no_write_bitmap_index: bool,
        #[arg(short = 'm', long = "write-midx", action = ArgAction::Count)]
        write_midx: u8,
        #[arg(long = "no-write-midx", action = ArgAction::SetTrue)]
        no_write_midx: bool,
        #[arg(long = "window")]
        window: Option<usize>,
        #[arg(long = "window-memory")]
        window_memory: Option<String>,
        #[arg(long = "depth")]
        depth: Option<usize>,
        #[arg(short = 'g', long = "geometric")]
        geometric: Vec<String>,
        #[arg(long = "threads")]
        threads: Option<usize>,
        #[arg(long = "max-pack-size")]
        max_pack_size: Option<String>,
        #[arg(long = "max-cruft-size")]
        max_cruft_size: Vec<String>,
        #[arg(long = "filter")]
        filter: Vec<String>,
        #[arg(long = "filter-to", value_hint = ValueHint::DirPath)]
        filter_to: Vec<PathBuf>,
        #[arg(long = "unpack-unreachable")]
        unpack_unreachable: Vec<String>,
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
        #[arg(long = "detach", action = ArgAction::SetTrue, overrides_with = "no_detach")]
        detach: bool,
        #[arg(long = "no-detach", action = ArgAction::SetTrue, overrides_with = "detach")]
        no_detach: bool,
        #[arg(long = "cruft", action = ArgAction::SetTrue, overrides_with = "no_cruft")]
        cruft: bool,
        #[arg(long = "no-cruft", action = ArgAction::SetTrue, overrides_with = "cruft")]
        no_cruft: bool,
        #[arg(long = "max-cruft-size")]
        max_cruft_size: Vec<String>,
        #[arg(long = "aggressive", action = ArgAction::SetTrue)]
        aggressive: bool,
        #[arg(short = 'q', long = "quiet", action = ArgAction::SetTrue)]
        quiet: bool,
        #[arg(long = "force", action = ArgAction::SetTrue)]
        force: bool,
        #[arg(long = "keep-largest-pack", action = ArgAction::SetTrue)]
        keep_largest_pack: bool,
    },
    Maintenance {
        operation: String,
        #[arg(long = "auto", action = ArgAction::SetTrue, overrides_with = "no_auto")]
        auto: bool,
        #[arg(long = "no-auto", action = ArgAction::SetTrue, overrides_with = "auto")]
        no_auto: bool,
        #[arg(long = "schedule")]
        schedule: Option<String>,
        #[arg(long = "no-schedule", action = ArgAction::SetTrue)]
        no_schedule: bool,
        #[arg(long = "scheduler")]
        scheduler: Option<String>,
        #[arg(long = "config-file", value_hint = ValueHint::FilePath)]
        config_file: Option<PathBuf>,
        #[arg(short = 'f', long = "force", action = ArgAction::SetTrue)]
        force: bool,
        #[arg(long = "quiet", action = ArgAction::SetTrue, overrides_with = "no_quiet")]
        quiet: bool,
        #[arg(long = "no-quiet", action = ArgAction::SetTrue, overrides_with = "quiet")]
        no_quiet: bool,
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
        #[arg(long = "empty", action = ArgAction::Count)]
        empty: u8,
        #[arg(short = 'm', action = ArgAction::Count)]
        merge: u8,
        #[arg(long = "trivial", action = ArgAction::Count)]
        trivial: u8,
        #[arg(long = "aggressive", action = ArgAction::Count)]
        aggressive: u8,
        #[arg(long = "reset", action = ArgAction::Count)]
        reset: u8,
        #[arg(short = 'u', action = ArgAction::Count)]
        update_worktree: u8,
        #[arg(short = 'i', action = ArgAction::Count)]
        index_only: u8,
        #[arg(short = 'n', long = "dry-run", action = ArgAction::Count)]
        dry_run: u8,
        #[arg(short = 'v', action = ArgAction::Count)]
        verbose: u8,
        #[arg(short = 'q', long = "quiet", action = ArgAction::Count)]
        quiet: u8,
        #[arg(long = "index-output")]
        index_output: Vec<PathBuf>,
        #[arg(long = "prefix")]
        prefix: Option<String>,
        #[arg(long = "recurse-submodules", action = ArgAction::Count)]
        recurse_submodules: u8,
        #[arg(long = "no-recurse-submodules", action = ArgAction::Count)]
        no_recurse_submodules: u8,
        #[arg(long = "no-sparse-checkout", action = ArgAction::Count)]
        no_sparse_checkout: u8,
        treeish: Option<String>,
    },
    Checkout {
        #[arg(short = 'f', long = "force", action = ArgAction::SetTrue)]
        force: bool,
        #[arg(short = 'q', long = "quiet", action = ArgAction::SetTrue)]
        quiet: bool,
        #[arg(long = "guess", action = ArgAction::SetTrue)]
        guess: bool,
        #[arg(long = "no-guess", action = ArgAction::SetTrue)]
        no_guess: bool,
        #[arg(short = 'm', long = "merge", action = ArgAction::Count)]
        merge: u8,
        #[arg(long = "conflict")]
        conflict: Option<String>,
        #[arg(long = "progress", action = ArgAction::SetTrue)]
        progress: bool,
        #[arg(long = "no-progress", action = ArgAction::SetTrue)]
        no_progress: bool,
        #[arg(short = 'p', long = "patch", action = ArgAction::SetTrue)]
        patch: bool,
        #[arg(short = 'd', long = "detach", action = ArgAction::SetTrue)]
        detach: bool,
        #[arg(long = "recurse-submodules", action = ArgAction::SetTrue)]
        recurse_submodules: bool,
        #[arg(long = "no-recurse-submodules", action = ArgAction::SetTrue)]
        no_recurse_submodules: bool,
        #[arg(short = '2', long = "ours", action = ArgAction::SetTrue)]
        ours: bool,
        #[arg(short = '3', long = "theirs", action = ArgAction::SetTrue)]
        theirs: bool,
        #[arg(long = "overlay", action = ArgAction::SetTrue)]
        overlay: bool,
        #[arg(long = "no-overlay", action = ArgAction::SetTrue)]
        no_overlay: bool,
        #[arg(long = "overwrite-ignore", action = ArgAction::SetTrue)]
        overwrite_ignore: bool,
        #[arg(long = "no-overwrite-ignore", action = ArgAction::SetTrue)]
        no_overwrite_ignore: bool,
        #[arg(long = "ignore-other-worktrees", action = ArgAction::SetTrue)]
        ignore_other_worktrees: bool,
        #[arg(long = "ignore-skip-worktree-bits", action = ArgAction::SetTrue)]
        ignore_skip_worktree_bits: bool,
        #[arg(short = 't', long = "track", num_args = 0..=1, require_equals = true, default_missing_value = "direct")]
        track: Option<String>,
        #[arg(long = "no-track", action = ArgAction::SetTrue)]
        no_track: bool,
        #[arg(short = 'b')]
        create: Option<String>,
        #[arg(short = 'B')]
        reset_create: Option<String>,
        #[arg(short = 'l', action = ArgAction::SetTrue)]
        create_reflog: bool,
        #[arg(long = "orphan")]
        orphan: Option<String>,
        #[arg(long = "pathspec-from-file", value_hint = ValueHint::FilePath)]
        pathspec_from_file: Option<PathBuf>,
        #[arg(long = "no-pathspec-from-file", action = ArgAction::Count)]
        no_pathspec_from_file: u8,
        #[arg(long = "pathspec-file-nul", action = ArgAction::SetTrue)]
        pathspec_file_nul: bool,
        #[arg(long = "no-pathspec-file-nul", action = ArgAction::Count)]
        no_pathspec_file_nul: u8,
        #[arg(value_hint = ValueHint::AnyPath, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    CheckoutIndex {
        #[arg(short = 'a', long = "all", action = ArgAction::Count)]
        all: u8,
        #[arg(short = 'f', long = "force", action = ArgAction::Count)]
        force: u8,
        #[arg(short = 'q', long = "quiet", action = ArgAction::Count)]
        quiet: u8,
        #[arg(short = 'u', long = "index", action = ArgAction::Count)]
        index: u8,
        #[arg(short = 'n', long = "no-create", action = ArgAction::Count)]
        no_create: u8,
        #[arg(long = "stage")]
        stage: Option<String>,
        #[arg(long = "temp", action = ArgAction::SetTrue)]
        temp: bool,
        #[arg(long = "ignore-skip-worktree-bits", action = ArgAction::SetTrue)]
        ignore_skip_worktree_bits: bool,
        #[arg(long = "stdin", action = ArgAction::SetTrue)]
        stdin: bool,
        #[arg(short = 'z', action = ArgAction::SetTrue)]
        nul: bool,
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
        #[arg(short = 'C', long = "force-create")]
        force_create: Option<String>,
        #[arg(short = 'c', long = "create")]
        create: Option<String>,
        #[arg(short = 'm', long = "merge", action = ArgAction::SetTrue)]
        merge: bool,
        #[arg(long = "conflict")]
        conflict: Option<String>,
        #[arg(long = "guess", action = ArgAction::SetTrue)]
        guess: bool,
        #[arg(long = "no-guess", action = ArgAction::SetTrue)]
        no_guess: bool,
        #[arg(short = 'q', long = "quiet", action = ArgAction::SetTrue)]
        quiet: bool,
        #[arg(long = "progress", action = ArgAction::SetTrue)]
        progress: bool,
        #[arg(long = "no-progress", action = ArgAction::SetTrue)]
        no_progress: bool,
        #[arg(long = "recurse-submodules", action = ArgAction::SetTrue)]
        recurse_submodules: bool,
        #[arg(long = "no-recurse-submodules", action = ArgAction::SetTrue)]
        no_recurse_submodules: bool,
        #[arg(long = "ignore-other-worktrees", action = ArgAction::SetTrue)]
        ignore_other_worktrees: bool,
        #[arg(long = "orphan")]
        orphan: Option<String>,
        #[arg(short = 'd', long = "detach", action = ArgAction::SetTrue)]
        detach: bool,
        #[arg(short = 't', long = "track")]
        track: Option<String>,
        #[arg(long = "no-track", action = ArgAction::SetTrue)]
        no_track: bool,
        target: Option<String>,
    },
    Restore {
        #[arg(short = 's', long = "source")]
        source: Vec<String>,
        #[arg(short = 'q', long = "quiet", action = ArgAction::SetTrue)]
        quiet: bool,
        #[arg(long = "progress", action = ArgAction::SetTrue)]
        progress: bool,
        #[arg(long = "no-progress", action = ArgAction::SetTrue)]
        no_progress: bool,
        #[arg(short = 'S', long = "staged", overrides_with = "no_staged", action = ArgAction::Count)]
        staged: u8,
        #[arg(long = "no-staged", overrides_with = "staged", action = ArgAction::SetTrue)]
        no_staged: bool,
        #[arg(short = 'W', long = "worktree", overrides_with = "no_worktree", action = ArgAction::Count)]
        worktree: u8,
        #[arg(long = "no-worktree", overrides_with = "worktree", action = ArgAction::SetTrue)]
        no_worktree: bool,
        #[arg(short = 'm', long = "merge", action = ArgAction::SetTrue)]
        merge: bool,
        #[arg(long = "conflict")]
        conflict: Option<String>,
        #[arg(long = "ours", action = ArgAction::SetTrue)]
        ours: bool,
        #[arg(long = "theirs", action = ArgAction::SetTrue)]
        theirs: bool,
        #[arg(long = "overlay", action = ArgAction::SetTrue)]
        overlay: bool,
        #[arg(long = "no-overlay", action = ArgAction::Count)]
        no_overlay: u8,
        #[arg(long = "ignore-unmerged", action = ArgAction::SetTrue)]
        ignore_unmerged: bool,
        #[arg(long = "ignore-skip-worktree-bits", action = ArgAction::SetTrue)]
        ignore_skip_worktree_bits: bool,
        #[arg(long = "recurse-submodules", action = ArgAction::SetTrue)]
        recurse_submodules: bool,
        #[arg(long = "no-recurse-submodules", action = ArgAction::SetTrue)]
        no_recurse_submodules: bool,
        #[arg(short = 'p', long = "patch", action = ArgAction::SetTrue)]
        patch: bool,
        #[arg(long = "pathspec-from-file", value_hint = ValueHint::FilePath)]
        pathspec_from_file: Option<PathBuf>,
        #[arg(long = "pathspec-file-nul", action = ArgAction::SetTrue)]
        pathspec_file_nul: bool,
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
        #[arg(short = 'R', action = ArgAction::SetTrue)]
        reverse: bool,
        #[arg(long = "reverse", action = ArgAction::SetTrue)]
        reverse_long: bool,
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
        #[arg(short = 'd', long = "dir-diff", action = ArgAction::SetTrue)]
        dir_diff: bool,
        #[arg(short = 't', long = "tool")]
        tool: Option<String>,
        #[arg(short = 'g', long = "gui", action = ArgAction::SetTrue)]
        gui: bool,
        #[arg(long = "no-gui", action = ArgAction::SetTrue)]
        no_gui: bool,
        #[arg(long = "symlinks", action = ArgAction::SetTrue)]
        symlinks: bool,
        #[arg(long = "no-symlinks", action = ArgAction::SetTrue)]
        no_symlinks: bool,
        #[arg(short = 'x', long = "extcmd")]
        extcmd: Option<String>,
        #[arg(short = 'y', long = "no-prompt", action = ArgAction::SetTrue)]
        no_prompt: bool,
        #[arg(long = "prompt", action = ArgAction::SetTrue)]
        prompt: bool,
        #[arg(long = "tool-help", action = ArgAction::SetTrue)]
        tool_help: bool,
        #[arg(long = "trust-exit-code", action = ArgAction::SetTrue)]
        trust_exit_code: bool,
        #[arg(long = "no-trust-exit-code", action = ArgAction::SetTrue)]
        no_trust_exit_code: bool,
        #[arg(long = "rotate-to")]
        rotate_to: Option<String>,
        #[arg(long = "skip-to")]
        skip_to: Option<String>,
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
        #[arg(short = 'R', action = ArgAction::SetTrue)]
        reverse: bool,
        #[arg(long = "reverse", action = ArgAction::SetTrue)]
        reverse_long: bool,
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
        #[arg(short = 'B', long = "break-rewrites", num_args = 0..=1, require_equals = true, default_missing_value = "")]
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
        #[arg(short = 'R', action = ArgAction::SetTrue)]
        reverse: bool,
        #[arg(long = "reverse", action = ArgAction::SetTrue)]
        reverse_long: bool,
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
        #[arg(short = 'R', action = ArgAction::SetTrue)]
        reverse: bool,
        #[arg(long = "reverse", action = ArgAction::SetTrue)]
        reverse_long: bool,
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
        #[arg(long = "allow-empty", action = ArgAction::SetTrue)]
        allow_empty: bool,
        #[arg(long = "allow-binary-replacement", action = ArgAction::SetTrue)]
        allow_binary_replacement: bool,
        #[arg(long = "apply", action = ArgAction::SetTrue)]
        apply: bool,
        #[arg(long = "binary", action = ArgAction::SetTrue)]
        binary: bool,
        #[arg(long = "check", action = ArgAction::SetTrue)]
        check: bool,
        #[arg(long = "cached", action = ArgAction::SetTrue)]
        cached: bool,
        #[arg(long = "stat", action = ArgAction::SetTrue)]
        stat: bool,
        #[arg(long = "numstat", action = ArgAction::SetTrue)]
        numstat: bool,
        #[arg(long = "summary", action = ArgAction::SetTrue)]
        summary: bool,
        #[arg(long = "index", action = ArgAction::SetTrue)]
        index: bool,
        #[arg(long = "recount", action = ArgAction::SetTrue)]
        recount: bool,
        #[arg(short = 'q', long = "quiet", action = ArgAction::SetTrue)]
        quiet: bool,
        #[arg(short = 'v', long = "verbose", action = ArgAction::SetTrue)]
        verbose: bool,
        #[arg(long = "unsafe-paths", action = ArgAction::SetTrue)]
        unsafe_paths: bool,
        #[arg(long = "unidiff-zero", action = ArgAction::SetTrue)]
        unidiff_zero: bool,
        #[arg(long = "ignore-space-change", action = ArgAction::SetTrue)]
        ignore_space_change: bool,
        #[arg(long = "ignore-whitespace", action = ArgAction::SetTrue)]
        ignore_whitespace: bool,
        #[arg(long = "whitespace")]
        whitespace: Option<String>,
        #[arg(short = 'p')]
        strip: Option<String>,
        #[arg(short = 'C')]
        context: Option<String>,
        #[arg(short = 'z', action = ArgAction::SetTrue)]
        z: bool,
        #[arg(long = "reject", action = ArgAction::SetTrue)]
        reject: bool,
        #[arg(long = "3way", action = ArgAction::SetTrue)]
        three_way: bool,
        #[arg(long = "ours", action = ArgAction::SetTrue)]
        ours: bool,
        #[arg(long = "theirs", action = ArgAction::SetTrue)]
        theirs: bool,
        #[arg(long = "union", action = ArgAction::SetTrue)]
        union: bool,
        #[arg(short = 'R', long = "reverse", action = ArgAction::SetTrue)]
        reverse: bool,
        #[arg(value_hint = ValueHint::FilePath)]
        patches: Vec<PathBuf>,
    },
    Am {
        #[arg(long = "quiet", short = 'q', action = ArgAction::SetTrue)]
        quiet: bool,
        #[arg(long = "signoff", short = 's', action = ArgAction::SetTrue)]
        signoff: bool,
        #[arg(long = "utf8", short = 'u', action = ArgAction::SetTrue)]
        utf8: bool,
        #[arg(long = "no-utf8", action = ArgAction::SetTrue)]
        no_utf8: bool,
        #[arg(long = "keep", short = 'k', action = ArgAction::SetTrue)]
        keep: bool,
        #[arg(long = "keep-non-patch", action = ArgAction::SetTrue)]
        keep_non_patch: bool,
        #[arg(long = "keep-cr", action = ArgAction::SetTrue)]
        keep_cr: bool,
        #[arg(long = "no-keep-cr", action = ArgAction::SetTrue)]
        no_keep_cr: bool,
        #[arg(long = "message-id", short = 'm', action = ArgAction::SetTrue)]
        message_id: bool,
        #[arg(long = "no-message-id", action = ArgAction::SetTrue)]
        no_message_id: bool,
        #[arg(long = "scissors", short = 'c', action = ArgAction::SetTrue)]
        scissors: bool,
        #[arg(long = "no-scissors", action = ArgAction::SetTrue)]
        no_scissors: bool,
        #[arg(long = "quoted-cr")]
        quoted_cr: Option<String>,
        #[arg(long = "3way", short = '3', action = ArgAction::SetTrue)]
        three_way: bool,
        #[arg(long = "no-3way", action = ArgAction::SetTrue)]
        no_three_way: bool,
        #[arg(long = "ignore-space-change", action = ArgAction::SetTrue)]
        ignore_space_change: bool,
        #[arg(long = "ignore-whitespace", action = ArgAction::SetTrue)]
        ignore_whitespace: bool,
        #[arg(long = "whitespace")]
        whitespace: Option<String>,
        #[arg(short = 'C')]
        context: Option<String>,
        #[arg(short = 'p')]
        strip: Option<String>,
        #[arg(long = "include")]
        include: Vec<String>,
        #[arg(long = "exclude")]
        exclude: Vec<String>,
        #[arg(long = "patch-format")]
        patch_format: Option<String>,
        #[arg(long = "interactive", short = 'i', action = ArgAction::SetTrue)]
        interactive: bool,
        #[arg(long = "empty")]
        empty: Option<String>,
        #[arg(long = "reject", action = ArgAction::SetTrue)]
        reject: bool,
        #[arg(
            short = 'S',
            long = "gpg-sign",
            num_args = 0..=1,
            require_equals = true,
            default_missing_value = "",
            overrides_with = "no_gpg_sign"
        )]
        gpg_sign: Option<String>,
        #[arg(long = "no-gpg-sign", action = ArgAction::Count, overrides_with = "gpg_sign")]
        no_gpg_sign: u8,
        #[arg(long = "rerere-autoupdate", action = ArgAction::SetTrue)]
        rerere_autoupdate: bool,
        #[arg(long = "no-rerere-autoupdate", action = ArgAction::SetTrue)]
        no_rerere_autoupdate: bool,
        #[arg(long = "resolvemsg")]
        resolvemsg: Option<String>,
        #[arg(long = "no-verify", short = 'n', action = ArgAction::SetTrue)]
        no_verify: bool,
        #[arg(long = "committer-date-is-author-date", action = ArgAction::SetTrue)]
        committer_date_is_author_date: bool,
        #[arg(long = "allow-empty", action = ArgAction::SetTrue)]
        allow_empty: bool,
        #[arg(long = "abort", action = ArgAction::SetTrue)]
        abort: bool,
        #[arg(long = "quit", action = ArgAction::SetTrue)]
        quit: bool,
        #[arg(long = "skip", action = ArgAction::SetTrue)]
        skip: bool,
        #[arg(long = "continue", action = ArgAction::SetTrue)]
        continue_: bool,
        #[arg(long = "resolved", short = 'r', action = ArgAction::SetTrue)]
        resolved: bool,
        #[arg(long = "retry", action = ArgAction::SetTrue)]
        retry: bool,
        #[arg(long = "show-current-patch")]
        show_current_patch: Option<String>,
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
        #[arg(long = "no-dual-color", action = ArgAction::Count)]
        no_dual_color: u8,
        #[arg(long = "no-no-dual-color", action = ArgAction::SetTrue)]
        no_no_dual_color: bool,
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
        #[arg(short = 'z', action = ArgAction::SetTrue)]
        zero: bool,
        #[arg(long = "all", action = ArgAction::SetTrue)]
        all: bool,
        #[arg(
            long = "tags",
            num_args = 0..=1,
            require_equals = true,
            default_missing_value = ""
        )]
        tags: Vec<String>,
        #[arg(long = "author")]
        author: Option<String>,
        #[arg(long = "committer")]
        committer: Option<String>,
        #[arg(long = "count", action = ArgAction::SetTrue)]
        count: bool,
        #[arg(long = "skip")]
        skip: Option<usize>,
        #[arg(long = "max-parents")]
        max_parents: Option<String>,
        #[arg(long = "no-max-parents", action = ArgAction::SetTrue)]
        no_max_parents: bool,
        #[arg(long = "merges", action = ArgAction::SetTrue)]
        merges: bool,
        #[arg(long = "max-age")]
        max_age: Option<String>,
        #[arg(long = "min-parents")]
        min_parents: Option<String>,
        #[arg(long = "min-age")]
        min_age: Option<String>,
        #[arg(long = "no-min-parents", action = ArgAction::SetTrue)]
        no_min_parents: bool,
        #[arg(long = "no-merges", action = ArgAction::SetTrue)]
        no_merges: bool,
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
        #[arg(long = "full-history", action = ArgAction::SetTrue)]
        full_history: bool,
        #[arg(long = "ancestry-path", action = ArgAction::SetTrue)]
        ancestry_path: bool,
        #[arg(long = "dense", action = ArgAction::SetTrue)]
        dense: bool,
        #[arg(long = "sparse", action = ArgAction::SetTrue)]
        sparse: bool,
        #[arg(long = "show-pulls", action = ArgAction::SetTrue)]
        show_pulls: bool,
        #[arg(long = "simplify-merges", action = ArgAction::SetTrue)]
        simplify_merges: bool,
        #[arg(long = "simplify-by-decoration", action = ArgAction::SetTrue)]
        simplify_by_decoration: bool,
        #[arg(long = "topo-order", action = ArgAction::SetTrue)]
        topo_order: bool,
        #[arg(long = "date-order", action = ArgAction::SetTrue)]
        date_order: bool,
        #[arg(long = "author-date-order", action = ArgAction::SetTrue)]
        author_date_order: bool,
        #[arg(long = "left-right", action = ArgAction::SetTrue)]
        left_right: bool,
        #[arg(long = "cherry-pick", action = ArgAction::SetTrue)]
        cherry_pick: bool,
        #[arg(long = "cherry-mark", action = ArgAction::SetTrue)]
        cherry_mark: bool,
        #[arg(long = "boundary", action = ArgAction::SetTrue)]
        boundary: bool,
        #[arg(long = "children", action = ArgAction::SetTrue)]
        children: bool,
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
        #[arg(long = "encoding")]
        encoding: Option<String>,
        #[arg(long = "expand-tabs", action = ArgAction::SetTrue)]
        expand_tabs: bool,
        #[arg(long = "no-expand-tabs", action = ArgAction::SetTrue)]
        no_expand_tabs: bool,
        #[arg(long = "notes", action = ArgAction::SetTrue)]
        notes: bool,
        #[arg(long = "no-notes", action = ArgAction::SetTrue)]
        no_notes: bool,
        #[arg(long = "show-notes", action = ArgAction::SetTrue)]
        show_notes: bool,
        #[arg(long = "show-notes-by-default", action = ArgAction::SetTrue)]
        show_notes_by_default: bool,
        #[arg(long = "standard-notes", action = ArgAction::SetTrue)]
        standard_notes: bool,
        #[arg(long = "no-standard-notes", action = ArgAction::SetTrue)]
        no_standard_notes: bool,
        #[arg(
            long = "decorate",
            num_args = 0..=1,
            require_equals = true,
            default_missing_value = "short"
        )]
        decorate: Option<String>,
        #[arg(long = "clear-decorations", action = ArgAction::SetTrue)]
        clear_decorations: bool,
        #[arg(long = "abbrev-commit", action = ArgAction::SetTrue)]
        abbrev_commit: bool,
        #[arg(long = "no-abbrev-commit", action = ArgAction::SetTrue)]
        no_abbrev_commit: bool,
        #[arg(long = "objects", action = ArgAction::SetTrue)]
        objects: bool,
        #[arg(long = "no-object-names", action = ArgAction::SetTrue)]
        no_object_names: bool,
        #[arg(long = "filter")]
        filter: Option<String>,
        #[arg(long = "filter-provided-objects", action = ArgAction::SetTrue)]
        filter_provided_objects: bool,
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
        #[arg(long = "reflog", action = ArgAction::SetTrue)]
        reflog: bool,
        #[arg(long = "do-walk", action = ArgAction::SetTrue)]
        do_walk: bool,
        #[arg(long = "no-walk", action = ArgAction::SetTrue)]
        no_walk: bool,
        #[arg(long = "grep-reflog")]
        grep_reflog: Vec<String>,
        #[arg(long = "grep")]
        grep: Vec<String>,
        #[arg(long = "invert-grep", action = ArgAction::SetTrue)]
        invert_grep: bool,
        #[arg(long = "all-match", action = ArgAction::SetTrue)]
        all_match: bool,
        #[arg(short = 'i', long = "regexp-ignore-case", action = ArgAction::SetTrue)]
        regexp_ignore_case: bool,
        #[arg(
            long = "basic-regexp",
            action = ArgAction::SetTrue,
            overrides_with_all = ["extended_regexp", "fixed_strings", "perl_regexp"]
        )]
        basic_regexp: bool,
        #[arg(
            short = 'E',
            long = "extended-regexp",
            action = ArgAction::SetTrue,
            overrides_with_all = ["basic_regexp", "fixed_strings", "perl_regexp"]
        )]
        extended_regexp: bool,
        #[arg(
            short = 'F',
            long = "fixed-strings",
            action = ArgAction::SetTrue,
            overrides_with_all = ["basic_regexp", "extended_regexp", "perl_regexp"]
        )]
        fixed_strings: bool,
        #[arg(
            short = 'P',
            long = "perl-regexp",
            action = ArgAction::SetTrue,
            overrides_with_all = ["basic_regexp", "extended_regexp", "fixed_strings"]
        )]
        perl_regexp: bool,
        #[arg(long = "format")]
        format: Option<String>,
        #[arg(long = "max-count", short = 'n')]
        max_count: Option<String>,
        #[arg(long = "since", alias = "after")]
        since: Option<String>,
        #[arg(long = "until", alias = "before")]
        until: Option<String>,
        #[arg(long = "date")]
        date: Option<String>,
        #[arg(long = "relative-date", action = ArgAction::SetTrue)]
        relative_date: bool,
        #[arg(long = "pretty")]
        pretty: Option<String>,
        #[arg(long = "quiet", action = ArgAction::SetTrue)]
        quiet: bool,
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
        #[arg(long = "prune-empty", action = ArgAction::SetTrue)]
        prune_empty: bool,
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
        #[arg(long = "all", action = ArgAction::Count)]
        all: u8,
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
        #[arg(long = "http-backend-info-refs", action = ArgAction::SetTrue)]
        http_backend_info_refs: bool,
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
        #[arg(long = "index-pack-args")]
        index_pack_args_plural: Vec<String>,
        args: Vec<String>,
    },
    HttpPush {
        #[arg(long = "all", action = ArgAction::SetTrue)]
        all: bool,
        #[arg(long = "dry-run", action = ArgAction::SetTrue)]
        dry_run: bool,
        #[arg(short = 'd', action = ArgAction::SetTrue)]
        delete: bool,
        #[arg(short = 'D', action = ArgAction::SetTrue)]
        force_delete: bool,
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
        #[arg(long = "exec", require_equals = true)]
        exec: Option<String>,
        #[arg(long = "upload-pack")]
        upload_pack: Option<String>,
        #[arg(long = "depth")]
        depth: Option<usize>,
        #[arg(long = "shallow-since")]
        shallow_since: Option<String>,
        #[arg(long = "shallow-exclude")]
        shallow_exclude: Vec<String>,
        #[arg(long = "deepen-relative", action = ArgAction::SetTrue)]
        deepen_relative: bool,
        #[arg(long = "refetch", action = ArgAction::SetTrue)]
        refetch: bool,
        #[arg(long = "check-self-contained-and-connected", action = ArgAction::SetTrue)]
        check_self_contained_and_connected: bool,
        #[arg(long = "no-progress", action = ArgAction::SetTrue)]
        no_progress: bool,
        #[arg(long = "diag-url", action = ArgAction::SetTrue)]
        diag_url: bool,
        #[arg(short = 'v', action = ArgAction::SetTrue)]
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
        #[arg(long = "receive-pack")]
        receive_pack: Option<String>,
        #[arg(long = "exec")]
        exec: Option<String>,
        #[arg(long = "verbose", short = 'v', action = ArgAction::SetTrue)]
        verbose: bool,
        #[arg(long = "thin", action = ArgAction::SetTrue)]
        thin: bool,
        #[arg(long = "atomic", action = ArgAction::SetTrue)]
        atomic: bool,
        #[arg(
            long = "signed",
            num_args = 0..=1,
            require_equals = true,
            default_missing_value = "true",
            value_parser = ["true", "false", "if-asked"]
        )]
        signed: Option<String>,
        #[arg(long = "no-signed", action = ArgAction::SetTrue)]
        no_signed: bool,
        #[arg(long = "push-option")]
        push_option: Vec<String>,
        #[arg(long = "all", action = ArgAction::SetTrue)]
        all: bool,
        #[arg(long = "stdin", action = ArgAction::SetTrue)]
        stdin: bool,
        directory: String,
        refs: Vec<String>,
    },
    ReceivePack {
        #[arg(long = "http-backend-info-refs", action = ArgAction::SetTrue)]
        http_backend_info_refs: bool,
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
        #[arg(long = "date")]
        date: Option<String>,
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
        #[arg(short = 'z', action = ArgAction::SetTrue)]
        zero: bool,
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
        #[arg(long = "encoding")]
        encoding: Option<String>,
        #[arg(long = "expand-tabs", action = ArgAction::SetTrue)]
        expand_tabs: bool,
        #[arg(long = "no-expand-tabs", action = ArgAction::SetTrue)]
        no_expand_tabs: bool,
        #[arg(long = "notes", action = ArgAction::SetTrue)]
        notes: bool,
        #[arg(long = "no-notes", action = ArgAction::SetTrue)]
        no_notes: bool,
        #[arg(long = "show-notes", action = ArgAction::SetTrue)]
        show_notes: bool,
        #[arg(long = "show-notes-by-default", action = ArgAction::SetTrue)]
        show_notes_by_default: bool,
        #[arg(long = "standard-notes", action = ArgAction::SetTrue)]
        standard_notes: bool,
        #[arg(long = "no-standard-notes", action = ArgAction::SetTrue)]
        no_standard_notes: bool,
        #[arg(long = "abbrev-commit", action = ArgAction::SetTrue)]
        abbrev_commit: bool,
        #[arg(long = "no-abbrev-commit", action = ArgAction::SetTrue)]
        no_abbrev_commit: bool,
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
        #[arg(long, action = ArgAction::SetTrue)]
        oneline: bool,
        #[arg(long = "all", action = ArgAction::SetTrue)]
        all: bool,
        #[arg(long = "author")]
        author: Option<String>,
        #[arg(long = "committer")]
        committer: Option<String>,
        #[arg(long = "encoding")]
        encoding: Option<String>,
        #[arg(long = "expand-tabs", action = ArgAction::SetTrue)]
        expand_tabs: bool,
        #[arg(long = "no-expand-tabs", action = ArgAction::SetTrue)]
        no_expand_tabs: bool,
        #[arg(long = "notes", action = ArgAction::SetTrue)]
        notes: bool,
        #[arg(long = "no-notes", action = ArgAction::SetTrue)]
        no_notes: bool,
        #[arg(long = "show-notes", action = ArgAction::SetTrue)]
        show_notes: bool,
        #[arg(long = "show-notes-by-default", action = ArgAction::SetTrue)]
        show_notes_by_default: bool,
        #[arg(long = "standard-notes", action = ArgAction::SetTrue)]
        standard_notes: bool,
        #[arg(long = "no-standard-notes", action = ArgAction::SetTrue)]
        no_standard_notes: bool,
        #[arg(long = "abbrev-commit", action = ArgAction::SetTrue)]
        abbrev_commit: bool,
        #[arg(long = "no-abbrev-commit", action = ArgAction::SetTrue)]
        no_abbrev_commit: bool,
        #[arg(long = "grep")]
        grep: Vec<String>,
        #[arg(long = "invert-grep", action = ArgAction::SetTrue)]
        invert_grep: bool,
        #[arg(long = "all-match", action = ArgAction::SetTrue)]
        all_match: bool,
        #[arg(short = 'i', long = "regexp-ignore-case", action = ArgAction::SetTrue)]
        regexp_ignore_case: bool,
        #[arg(
            long = "basic-regexp",
            action = ArgAction::SetTrue,
            overrides_with_all = ["extended_regexp", "fixed_strings", "perl_regexp"]
        )]
        basic_regexp: bool,
        #[arg(
            short = 'E',
            long = "extended-regexp",
            action = ArgAction::SetTrue,
            overrides_with_all = ["basic_regexp", "fixed_strings", "perl_regexp"]
        )]
        extended_regexp: bool,
        #[arg(
            short = 'F',
            long = "fixed-strings",
            action = ArgAction::SetTrue,
            overrides_with_all = ["basic_regexp", "extended_regexp", "perl_regexp"]
        )]
        fixed_strings: bool,
        #[arg(
            short = 'P',
            long = "perl-regexp",
            action = ArgAction::SetTrue,
            overrides_with_all = ["basic_regexp", "extended_regexp", "fixed_strings"]
        )]
        perl_regexp: bool,
        #[arg(long = "count", action = ArgAction::SetTrue)]
        count: bool,
        #[arg(long = "skip")]
        skip: Option<usize>,
        #[arg(
            long = "branches",
            num_args = 0..=1,
            require_equals = true,
            default_missing_value = ""
        )]
        branches: Vec<String>,
        #[arg(
            long = "tags",
            num_args = 0..=1,
            require_equals = true,
            default_missing_value = ""
        )]
        tags: Vec<String>,
        #[arg(
            long = "remotes",
            num_args = 0..=1,
            require_equals = true,
            default_missing_value = ""
        )]
        remotes: Vec<String>,
        #[arg(long = "max-parents")]
        max_parents: Option<String>,
        #[arg(long = "max-age")]
        max_age: Option<String>,
        #[arg(long = "no-max-parents", action = ArgAction::SetTrue)]
        no_max_parents: bool,
        #[arg(long = "merges", action = ArgAction::SetTrue)]
        merges: bool,
        #[arg(long = "min-parents")]
        min_parents: Option<String>,
        #[arg(long = "min-age")]
        min_age: Option<String>,
        #[arg(long = "no-min-parents", action = ArgAction::SetTrue)]
        no_min_parents: bool,
        #[arg(long = "no-merges", action = ArgAction::SetTrue)]
        no_merges: bool,
        #[arg(long = "objects", action = ArgAction::SetTrue)]
        objects: bool,
        #[arg(long = "object-names", action = ArgAction::SetTrue)]
        object_names: bool,
        #[arg(long = "no-object-names", action = ArgAction::SetTrue)]
        no_object_names: bool,
        #[arg(long = "filter")]
        filter: Option<String>,
        #[arg(long = "filter-provided-objects", action = ArgAction::SetTrue)]
        filter_provided_objects: bool,
        #[arg(long = "parents", action = ArgAction::SetTrue)]
        parents: bool,
        #[arg(long = "first-parent", action = ArgAction::SetTrue)]
        first_parent: bool,
        #[arg(long = "children", action = ArgAction::SetTrue)]
        children: bool,
        #[arg(short = 'g', long = "walk-reflogs", action = ArgAction::SetTrue)]
        walk_reflogs: bool,
        #[arg(long = "reflog", action = ArgAction::SetTrue)]
        reflog: bool,
        #[arg(long = "do-walk", action = ArgAction::SetTrue)]
        do_walk: bool,
        #[arg(long = "grep-reflog")]
        grep_reflog: Vec<String>,
        #[arg(long = "reverse", action = ArgAction::SetTrue)]
        reverse: bool,
        #[arg(long = "full-history", action = ArgAction::SetTrue)]
        full_history: bool,
        #[arg(long = "ancestry-path", action = ArgAction::SetTrue)]
        ancestry_path: bool,
        #[arg(long = "dense", action = ArgAction::SetTrue)]
        dense: bool,
        #[arg(long = "sparse", action = ArgAction::SetTrue)]
        sparse: bool,
        #[arg(long = "show-pulls", action = ArgAction::SetTrue)]
        show_pulls: bool,
        #[arg(long = "simplify-merges", action = ArgAction::SetTrue)]
        simplify_merges: bool,
        #[arg(long = "simplify-by-decoration", action = ArgAction::SetTrue)]
        simplify_by_decoration: bool,
        #[arg(long = "topo-order", action = ArgAction::SetTrue)]
        topo_order: bool,
        #[arg(long = "date-order", action = ArgAction::SetTrue)]
        date_order: bool,
        #[arg(long = "author-date-order", action = ArgAction::SetTrue)]
        author_date_order: bool,
        #[arg(long = "left-right", action = ArgAction::SetTrue)]
        left_right: bool,
        #[arg(long = "cherry-pick", action = ArgAction::SetTrue)]
        cherry_pick: bool,
        #[arg(long = "cherry-mark", action = ArgAction::SetTrue)]
        cherry_mark: bool,
        #[arg(long = "boundary", action = ArgAction::SetTrue)]
        boundary: bool,
        #[arg(long = "max-count", short = 'n')]
        max_count: Option<usize>,
        #[arg(long = "since", alias = "after")]
        since: Option<String>,
        #[arg(long = "until", alias = "before")]
        until: Option<String>,
        #[arg(long = "relative-date", action = ArgAction::SetTrue)]
        relative_date: bool,
        #[arg(long = "timestamp", action = ArgAction::SetTrue)]
        timestamp: bool,
        #[arg(long = "date")]
        date: Option<String>,
        #[arg(long = "quiet", action = ArgAction::SetTrue)]
        quiet: bool,
        #[arg(long = "format")]
        format: Option<String>,
        #[arg(long = "pretty")]
        pretty: Option<String>,
        #[arg(allow_hyphen_values = true)]
        revs: Vec<String>,
    },
    MergeBase {
        #[arg(short = 'a', long = "all", action = ArgAction::SetTrue)]
        all: bool,
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
        #[arg(long = "tool-help", action = ArgAction::SetTrue)]
        tool_help: bool,
        #[arg(short = 'y', long = "no-prompt", action = ArgAction::SetTrue)]
        no_prompt: bool,
        #[arg(long = "prompt", action = ArgAction::SetTrue)]
        prompt: bool,
        #[arg(short = 'g', long = "gui", action = ArgAction::SetTrue)]
        gui: bool,
        #[arg(long = "no-gui", action = ArgAction::SetTrue)]
        no_gui: bool,
        #[arg(short = 'O')]
        orderfile: Option<PathBuf>,
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
        #[arg(short = 'q', long = "quiet", action = ArgAction::Count)]
        quiet: u8,
        #[arg(long = "no-quiet", action = ArgAction::Count)]
        no_quiet: u8,
        #[arg(long = "ours", action = ArgAction::Count)]
        ours: u8,
        #[arg(long = "no-ours", action = ArgAction::Count)]
        no_ours: u8,
        #[arg(long = "theirs", action = ArgAction::Count)]
        theirs: u8,
        #[arg(long = "no-theirs", action = ArgAction::Count)]
        no_theirs: u8,
        #[arg(long = "union", action = ArgAction::Count)]
        union: u8,
        #[arg(long = "no-union", action = ArgAction::Count)]
        no_union: u8,
        #[arg(long = "diff3", action = ArgAction::Count)]
        diff3: u8,
        #[arg(long = "no-diff3", action = ArgAction::Count)]
        no_diff3: u8,
        #[arg(long = "zdiff3", action = ArgAction::Count)]
        zdiff3: u8,
        #[arg(long = "marker-size")]
        marker_size: Option<String>,
        #[arg(long = "no-marker-size", action = ArgAction::Count)]
        no_marker_size: u8,
        #[arg(long = "diff-algorithm")]
        diff_algorithm: Option<String>,
        #[arg(long = "object-id", action = ArgAction::Count)]
        object_id: u8,
        #[arg(long = "no-object-id", action = ArgAction::Count)]
        no_object_id: u8,
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
        #[arg(short = 'a', action = ArgAction::Count)]
        all: u8,
        paths: Vec<String>,
    },
    UpdateRef {
        #[arg(short = 'd', action = ArgAction::SetTrue)]
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
        #[arg(short = 'q', long = "quiet", action = ArgAction::Count)]
        quiet: u8,
        #[arg(long = "no-quiet", action = ArgAction::Count)]
        no_quiet: u8,
        #[arg(short = 'd', long = "delete", action = ArgAction::Count)]
        delete: u8,
        #[arg(long = "no-delete", action = ArgAction::Count)]
        no_delete: u8,
        #[arg(long = "short", action = ArgAction::Count)]
        short: u8,
        #[arg(long = "no-short", action = ArgAction::Count)]
        no_short: u8,
        #[arg(long = "recurse", action = ArgAction::Count)]
        recurse: u8,
        #[arg(long = "no-recurse", action = ArgAction::Count)]
        no_recurse: u8,
        #[arg(short = 'm')]
        message: Option<String>,
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
        #[arg(long = "all", action = ArgAction::SetTrue)]
        all: bool,
        #[arg(
            long = "branches",
            num_args = 0..=1,
            require_equals = true,
            default_missing_value = ""
        )]
        branches: Vec<String>,
        #[arg(
            long = "tags",
            num_args = 0..=1,
            require_equals = true,
            default_missing_value = ""
        )]
        tags: Vec<String>,
        #[arg(
            long = "remotes",
            num_args = 0..=1,
            require_equals = true,
            default_missing_value = ""
        )]
        remotes: Vec<String>,
        #[arg(long = "glob")]
        glob: Vec<String>,
        #[arg(long = "exclude")]
        exclude: Vec<String>,
        #[arg(long = "local-env-vars", action = ArgAction::SetTrue)]
        local_env_vars: bool,
        #[arg(long = "flags", action = ArgAction::SetTrue)]
        flags: bool,
        #[arg(long = "no-flags", action = ArgAction::SetTrue)]
        no_flags: bool,
        #[arg(long = "revs-only", action = ArgAction::SetTrue)]
        revs_only: bool,
        #[arg(long = "no-revs", action = ArgAction::SetTrue)]
        no_revs: bool,
        #[arg(long = "default")]
        default: Option<String>,
        #[arg(long = "prefix")]
        prefix: Option<String>,
        #[arg(long = "sq", action = ArgAction::SetTrue)]
        sq: bool,
        #[arg(long = "sq-quote", action = ArgAction::SetTrue)]
        sq_quote: bool,
        #[arg(long = "not", action = ArgAction::SetTrue)]
        not: bool,
        #[arg(long = "symbolic", action = ArgAction::SetTrue)]
        symbolic: bool,
        #[arg(long = "parseopt", action = ArgAction::SetTrue)]
        parseopt: bool,
        #[arg(long = "keep-dashdash", action = ArgAction::SetTrue)]
        keep_dashdash: bool,
        #[arg(long = "stop-at-non-option", action = ArgAction::SetTrue)]
        stop_at_non_option: bool,
        #[arg(long = "stuck-long", action = ArgAction::SetTrue)]
        stuck_long: bool,
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
        #[arg(long = "resolve-git-dir")]
        resolve_git_dir: Vec<PathBuf>,
        #[arg(long = "output-object-format", num_args = 1, require_equals = true)]
        output_object_format: Vec<String>,
        #[arg(long = "disambiguate", require_equals = true)]
        disambiguate: Vec<String>,
        #[arg(long = "shared-index-path", action = ArgAction::SetTrue)]
        shared_index_path: bool,
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
        #[arg(short = 'd', long = "dereference", action = ArgAction::SetTrue)]
        dereference: bool,
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
        #[arg(
            long = "exclude-existing",
            num_args = 0..=1,
            require_equals = true,
            default_missing_value = ""
        )]
        exclude_existing: Option<String>,
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
        #[arg(short = 'd', action = ArgAction::SetTrue)]
        directory_only: bool,
        #[arg(short = 'r', action = ArgAction::SetTrue)]
        recursive: bool,
        #[arg(short = 't', action = ArgAction::SetTrue)]
        show_trees: bool,
        #[arg(short = 'l', long = "long", action = ArgAction::SetTrue)]
        long: bool,
        #[arg(short = 'z', action = ArgAction::SetTrue)]
        nul_terminated: bool,
        #[arg(long = "name-only", action = ArgAction::SetTrue)]
        name_only: bool,
        #[arg(long = "name-status", action = ArgAction::SetTrue)]
        name_status: bool,
        #[arg(long = "object-only", action = ArgAction::SetTrue)]
        object_only: bool,
        #[arg(long = "full-name", action = ArgAction::SetTrue)]
        full_name: bool,
        #[arg(long = "full-tree", action = ArgAction::SetTrue)]
        full_tree: bool,
        #[arg(long = "abbrev", num_args = 0..=1, require_equals = true, default_missing_value = "7")]
        abbrev: Option<usize>,
        #[arg(long = "format")]
        format: Option<String>,
        treeish: String,
        paths: Vec<String>,
    },
    #[command(disable_help_flag = true)]
    Branch {
        #[arg(short = 'h', long = "help", action = ArgAction::SetTrue)]
        help: bool,
        #[arg(short = 'r', long = "remotes", action = ArgAction::Count)]
        remotes: u8,
        #[arg(short = 'a', long = "all", action = ArgAction::Count)]
        all: u8,
        #[arg(short = 'l', long = "list", action = ArgAction::Count)]
        list: u8,
        #[arg(long = "no-list", action = ArgAction::Count)]
        no_list: u8,
        #[arg(short = 'f', long = "force", action = ArgAction::SetTrue)]
        force: bool,
        #[arg(short = 'q', long = "quiet", action = ArgAction::Count)]
        quiet: u8,
        #[arg(short = 'v', long = "verbose", action = ArgAction::Count)]
        verbose: u8,
        #[arg(long = "no-verbose", action = ArgAction::Count)]
        no_verbose: u8,
        #[arg(long = "abbrev", num_args = 0..=1, require_equals = true, default_missing_value = "7")]
        abbrev: Option<usize>,
        #[arg(long = "no-abbrev", action = ArgAction::SetTrue)]
        no_abbrev: bool,
        #[arg(long = "column", num_args = 0..=1, require_equals = true, default_missing_value = "column")]
        column: Option<String>,
        #[arg(long = "no-column", action = ArgAction::SetTrue)]
        no_column: bool,
        #[arg(short = 'i', long = "ignore-case", action = ArgAction::Count)]
        ignore_case: u8,
        #[arg(long = "color", num_args = 0..=1, require_equals = true, default_missing_value = "always")]
        color: Option<String>,
        #[arg(long = "no-color", action = ArgAction::Count)]
        no_color: u8,
        #[arg(long = "create-reflog", overrides_with = "no_create_reflog", action = ArgAction::SetTrue)]
        create_reflog: bool,
        #[arg(long = "no-create-reflog", overrides_with = "create_reflog", action = ArgAction::SetTrue)]
        no_create_reflog: bool,
        #[arg(long = "show-current", action = ArgAction::Count)]
        show_current: u8,
        #[arg(long = "no-show-current", action = ArgAction::Count)]
        no_show_current: u8,
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
        #[arg(long = "set-upstream", action = ArgAction::SetTrue)]
        set_upstream: bool,
        #[arg(long = "unset-upstream", action = ArgAction::SetTrue)]
        unset_upstream: bool,
        #[arg(short = 't', long = "track", num_args = 0..=1, require_equals = true, default_missing_value = "direct")]
        track: Option<String>,
        #[arg(long = "no-track", action = ArgAction::SetTrue)]
        no_track: bool,
        #[arg(long = "sort")]
        sort: Vec<String>,
        #[arg(long = "format")]
        format: Option<String>,
        #[arg(long = "no-format", action = ArgAction::SetTrue)]
        no_format: bool,
        #[arg(long = "omit-empty", action = ArgAction::SetTrue)]
        omit_empty: bool,
        #[arg(long = "no-sort", action = ArgAction::SetTrue)]
        no_sort: bool,
        #[arg(long = "recurse-submodules", action = ArgAction::SetTrue)]
        recurse_submodules: bool,
        #[arg(long = "no-recurse-submodules", action = ArgAction::SetTrue)]
        no_recurse_submodules: bool,
        #[arg(long = "contains", num_args = 0..=1, default_missing_value = "HEAD")]
        contains: Option<String>,
        #[arg(long = "no-contains", num_args = 0..=1, default_missing_value = "HEAD")]
        no_contains: Option<String>,
        #[arg(long = "merged", num_args = 0..=1, default_missing_value = "HEAD")]
        merged: Option<String>,
        #[arg(long = "no-merged", num_args = 0..=1, default_missing_value = "HEAD")]
        no_merged: Option<String>,
        #[arg(long = "points-at")]
        points_at: Option<String>,
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
        #[arg(
            long = "column",
            overrides_with = "no_column",
            num_args = 0..=1,
            default_missing_value = "always",
            require_equals = true
        )]
        column: Option<String>,
        #[arg(long = "no-column", overrides_with = "column", action = ArgAction::SetTrue)]
        no_column: bool,
        #[arg(short = 'i', long = "ignore-case", action = ArgAction::SetTrue)]
        ignore_case: bool,
        #[arg(long = "color", num_args = 0..=1, require_equals = true, default_missing_value = "always")]
        color: Option<String>,
        #[arg(long = "no-color", action = ArgAction::SetTrue)]
        no_color: bool,
        #[arg(short = 'f', long = "force", action = ArgAction::SetTrue)]
        force: bool,
        #[arg(short = 'a', long = "annotate", action = ArgAction::SetTrue)]
        annotate: bool,
        #[arg(short = 'm', long = "message")]
        messages: Vec<String>,
        #[arg(short = 'F', long = "file", value_hint = ValueHint::FilePath)]
        message_files: Vec<PathBuf>,
        #[arg(long = "create-reflog", action = ArgAction::SetTrue)]
        create_reflog: bool,
        #[arg(long = "contains", num_args = 0..=1, default_missing_value = "HEAD")]
        contains: Option<String>,
        #[arg(long = "no-contains", num_args = 0..=1, default_missing_value = "HEAD")]
        no_contains: Option<String>,
        #[arg(long = "merged", num_args = 0..=1, default_missing_value = "HEAD")]
        merged: Option<String>,
        #[arg(long = "no-merged", num_args = 0..=1, default_missing_value = "HEAD")]
        no_merged: Option<String>,
        #[arg(long = "omit-empty", action = ArgAction::SetTrue)]
        omit_empty: bool,
        #[arg(long = "sort")]
        sort: Vec<String>,
        #[arg(long = "points-at")]
        points_at: Option<String>,
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
        #[arg(long = "no-strict", action = ArgAction::SetTrue)]
        no_strict: bool,
        #[arg(long = "verbose", action = ArgAction::SetTrue)]
        verbose: bool,
        #[arg(long = "no-verbose", action = ArgAction::SetTrue)]
        no_verbose: bool,
        #[arg(long = "dry-run", action = ArgAction::SetTrue)]
        dry_run: bool,
        #[arg(long = "ref-format", num_args = 0..=1, require_equals = true, default_missing_value = "")]
        ref_format: Option<String>,
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
pub enum ReflogCommand {
    #[command(disable_help_flag = true)]
    Expire(ReflogExpireArgs),
    Delete(ReflogDeleteArgs),
    Drop(ReflogDropArgs),
}

#[derive(ClapArgs, Debug)]
pub struct ReflogExpireArgs {
    #[arg(short = 'h', long = "help", action = ArgAction::SetTrue)]
    pub help: bool,
    #[arg(long = "expire")]
    pub expire: Option<String>,
    #[arg(long = "expire-unreachable")]
    pub expire_unreachable: Option<String>,
    #[arg(long = "rewrite", action = ArgAction::SetTrue)]
    pub rewrite: bool,
    #[arg(long = "updateref", action = ArgAction::SetTrue)]
    pub updateref: bool,
    #[arg(long = "stale-fix", action = ArgAction::SetTrue)]
    pub stale_fix: bool,
    #[arg(short = 'n', long = "dry-run", action = ArgAction::SetTrue)]
    pub dry_run: bool,
    #[arg(long = "verbose", action = ArgAction::SetTrue)]
    pub verbose: bool,
    #[arg(long = "all", action = ArgAction::SetTrue)]
    pub all: bool,
    #[arg(long = "single-worktree", action = ArgAction::SetTrue)]
    pub single_worktree: bool,
    #[arg(allow_hyphen_values = true)]
    pub refs: Vec<String>,
}

#[derive(ClapArgs, Debug)]
pub struct ReflogDeleteArgs {
    #[arg(long = "rewrite", action = ArgAction::SetTrue)]
    pub rewrite: bool,
    #[arg(long = "updateref", action = ArgAction::SetTrue)]
    pub updateref: bool,
    #[arg(short = 'n', long = "dry-run", action = ArgAction::SetTrue)]
    pub dry_run: bool,
    #[arg(long = "verbose", action = ArgAction::SetTrue)]
    pub verbose: bool,
    #[arg(allow_hyphen_values = true)]
    pub selectors: Vec<String>,
}

#[derive(ClapArgs, Debug)]
pub struct ReflogDropArgs {
    #[arg(long = "all", action = ArgAction::SetTrue)]
    pub all: bool,
    #[arg(long = "single-worktree", action = ArgAction::SetTrue)]
    pub single_worktree: bool,
    #[arg(allow_hyphen_values = true)]
    pub refs: Vec<String>,
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
        #[arg(long = "object-dir", value_hint = ValueHint::DirPath)]
        object_dir: Option<PathBuf>,
        #[arg(long = "reachable", action = ArgAction::SetTrue)]
        reachable: bool,
        #[arg(long = "progress", action = ArgAction::SetTrue)]
        progress: bool,
        #[arg(long = "no-progress", action = ArgAction::SetTrue)]
        no_progress: bool,
    },
    Verify {
        #[arg(long = "object-dir", value_hint = ValueHint::DirPath)]
        object_dir: Option<PathBuf>,
        #[arg(long = "progress", action = ArgAction::SetTrue)]
        progress: bool,
        #[arg(long = "no-progress", action = ArgAction::SetTrue)]
        no_progress: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum MultiPackIndexCommand {
    Write {
        #[arg(long = "bitmap", action = ArgAction::SetTrue)]
        bitmap: bool,
        #[arg(long = "preferred-pack")]
        preferred_pack: Option<String>,
        #[arg(long = "no-bitmap", action = ArgAction::SetTrue)]
        no_bitmap: bool,
        #[arg(long = "refs-snapshot", value_hint = ValueHint::FilePath)]
        refs_snapshot: Option<PathBuf>,
        #[arg(long = "incremental", action = ArgAction::SetTrue)]
        incremental: bool,
        #[arg(long = "stdin-packs", action = ArgAction::SetTrue)]
        stdin_packs: bool,
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
    pub no_empty_directory: bool,
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
    pub sparse: bool,
    pub pathspec_from_file: Option<PathBuf>,
    pub pathspec_file_nul: bool,
    pub paths: Vec<PathBuf>,
}

pub struct ConfigArgs {
    pub null: bool,
    pub all: bool,
    pub blob: Option<String>,
    pub comment: Option<String>,
    pub fixed_value: bool,
    pub get: bool,
    pub get_all: bool,
    pub get_colorbool: bool,
    pub get_regexp: bool,
    pub list: bool,
    pub name_only: bool,
    pub no_includes: bool,
    pub no_type: bool,
    pub regexp: bool,
    pub replace_all: bool,
    pub system: bool,
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
    pub modern_get: bool,
    pub show_origin: bool,
    pub show_scope: bool,
    pub url: Option<String>,
    pub value_pattern: Option<String>,
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

impl Default for DiffOptions {
    fn default() -> Self {
        Self {
            no_index: false,
            nul_terminated: false,
            cached: false,
            reverse: false,
            check: false,
            patch_with_raw: false,
            patch_with_stat: false,
            stat: false,
            compact_summary: false,
            no_patch: false,
            binary: false,
            numstat: false,
            shortstat: false,
            dirstat: None,
            dirstat_by_file: false,
            raw: false,
            summary: false,
            name_status: false,
            name_only: false,
            find_renames: None,
            break_rewrites: None,
            irreversible_delete: false,
            submodule: None,
            ignore_submodules: None,
            find_copies: None,
            find_copies_harder: false,
            no_renames: false,
            dense_combined: false,
            pickaxe_string: None,
            pickaxe_regex: None,
            pickaxe_regex_mode: false,
            pickaxe_all: false,
            order_file: None,
            skip_to: None,
            rotate_to: None,
            diff_filter: None,
            word_diff: None,
            abbrev: None,
            no_abbrev: false,
            full_index: false,
            no_full_index: false,
            no_prefix: false,
            default_prefix: false,
            src_prefix: None,
            dst_prefix: None,
            relative: None,
            no_relative: false,
            unified: None,
            inter_hunk_context: None,
            minimal: false,
            patience: false,
            histogram: false,
            diff_algorithm: None,
            anchored: Vec::new(),
            output_indicator_new: None,
            output_indicator_old: None,
            output_indicator_context: None,
            line_prefix: None,
            ignore_space_at_eol: false,
            ignore_cr_at_eol: false,
            ignore_space_change: false,
            ignore_all_space: false,
            ignore_blank_lines: false,
            ignore_matching_lines: Vec::new(),
            no_ext_diff: false,
            no_textconv: false,
            text: false,
            color: None,
            no_color: false,
            no_color_moved: false,
            no_color_moved_ws: false,
            quiet: false,
            exit_code: false,
            paths: Vec::new(),
        }
    }
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
    pub check_self_contained_and_connected: bool,
    pub fix_thin: bool,
    pub verbose: bool,
    pub index_version: Option<String>,
    pub threads: Vec<usize>,
    pub max_input_size: Vec<String>,
    pub object_format: Vec<String>,
    pub promisor: Option<String>,
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
