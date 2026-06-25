use crate::runtime;

pub(crate) fn dispatch(
    command: runtime::Command,
    raw_args: &[String],
) -> std::result::Result<(), runtime::CliError> {
    match command {
        runtime::Command::Remote { verbose, command } => run_remote(verbose, command),
        runtime::Command::PackRefs {
            all,
            auto,
            include,
            exclude,
            prune,
            no_prune,
        } => run_pack_refs(all, auto, include, exclude, prune, no_prune),
        runtime::Command::UpdateRef {
            delete,
            no_deref,
            deref: _,
            stdin,
            nul_terminated,
            message,
            create_reflog,
            no_create_reflog,
            batch_updates,
            no_batch_updates,
            name,
            newvalue,
        } => run_update_ref(
            delete,
            no_deref,
            stdin,
            nul_terminated,
            message,
            create_reflog && !no_create_reflog,
            batch_updates && !no_batch_updates,
            name,
            newvalue,
        ),
        runtime::Command::SymbolicRef {
            quiet,
            no_quiet,
            delete,
            no_delete,
            short,
            no_short,
            recurse,
            no_recurse,
            message,
            name,
            target,
        } => run_symbolic_ref(
            resolve_symbolic_ref_toggle(
                raw_args,
                &["-q", "--quiet"],
                &["--no-quiet"],
                quiet > 0,
                no_quiet > 0,
            ),
            resolve_symbolic_ref_delete(delete > 0, no_delete > 0, raw_args),
            resolve_symbolic_ref_toggle(
                raw_args,
                &["--short"],
                &["--no-short"],
                short > 0,
                no_short > 0,
            ),
            resolve_symbolic_ref_toggle(
                raw_args,
                &["--no-recurse"],
                &["--recurse"],
                no_recurse > 0,
                recurse > 0,
            ),
            message,
            name,
            target,
        ),
        runtime::Command::Refs { command } => run_refs(command, raw_args),
        runtime::Command::Repo { command } => run_repo(command),
        runtime::Command::ShowRef {
            quiet,
            head,
            heads,
            tags,
            dereference,
            hash,
            abbrev,
            verify,
            exists,
            exclude_existing,
            refs,
        } => run_show_ref(
            quiet,
            head,
            heads,
            tags,
            dereference,
            hash,
            abbrev,
            verify,
            exists,
            exclude_existing,
            refs,
        ),
        runtime::Command::ForEachRef {
            format,
            sort,
            patterns,
        } => run_for_each_ref(format, sort, patterns),
        runtime::Command::LsTree {
            recursive,
            show_trees,
            name_only,
            treeish,
            paths,
        } => run_ls_tree(recursive, show_trees, name_only, treeish, paths),
        runtime::Command::Branch {
            help,
            remotes,
            all,
            list,
            no_list,
            force,
            quiet,
            verbose,
            no_verbose,
            abbrev,
            no_abbrev,
            column,
            no_column,
            ignore_case,
            color,
            no_color,
            create_reflog,
            no_create_reflog,
            show_current,
            no_show_current,
            edit_description,
            delete,
            force_delete,
            move_branch,
            force_move,
            copy_branch,
            force_copy,
            set_upstream_to,
            set_upstream,
            unset_upstream,
            track,
            no_track,
            sort,
            format,
            no_format,
            omit_empty,
            no_sort,
            recurse_submodules,
            no_recurse_submodules,
            contains,
            no_contains,
            merged,
            no_merged,
            points_at,
            name,
            start_point,
            extra_args,
        } => run_branch(
            help,
            remotes > 0,
            all > 0,
            list > 0 && no_list == 0,
            force,
            quiet > 0,
            verbose,
            no_verbose > 0,
            abbrev,
            no_abbrev,
            column,
            no_column,
            ignore_case > 0,
            color,
            no_color > 0,
            create_reflog && !no_create_reflog,
            show_current > 0 && no_show_current == 0,
            edit_description,
            delete,
            force_delete,
            move_branch,
            force_move,
            copy_branch,
            force_copy,
            set_upstream_to,
            set_upstream,
            unset_upstream,
            track,
            no_track,
            sort,
            format,
            no_format,
            omit_empty,
            no_sort,
            recurse_submodules,
            no_recurse_submodules,
            contains,
            no_contains,
            merged,
            no_merged,
            points_at,
            name,
            start_point,
            extra_args,
            raw_args,
        ),
        runtime::Command::Tag {
            delete,
            verify,
            list,
            no_column,
            ignore_case,
            color,
            no_color,
            force,
            annotate,
            messages,
            contains,
            no_contains,
            merged,
            no_merged,
            sort,
            format,
            args,
        } => run_tag(
            delete,
            verify,
            list,
            no_column,
            ignore_case,
            color,
            no_color,
            force,
            annotate,
            messages,
            contains,
            no_contains,
            merged,
            no_merged,
            sort,
            format,
            args,
        ),
        runtime::Command::Replace {
            list,
            delete,
            force,
            format,
            edit,
            graft,
            convert_graft_file,
            raw,
            no_raw,
            args,
        } => run_replace(
            list,
            delete,
            force,
            format,
            edit,
            graft,
            convert_graft_file,
            raw && !no_raw,
            args,
        ),
        runtime::Command::PatchId {
            stable,
            unstable,
            verbatim,
        } => run_patch_id(stable > 0, unstable > 0, verbatim > 0),
        runtime::Command::RevParse {
            short,
            abbrev_ref,
            verify,
            quiet,
            symbolic_full_name,
            bisect,
            path_format,
            since,
            until,
            show_object_format,
            show_ref_format,
            show_toplevel,
            show_prefix,
            show_cdup,
            show_superproject_working_tree,
            git_dir,
            absolute_git_dir,
            git_common_dir,
            git_paths,
            is_inside_git_dir,
            is_inside_work_tree,
            is_bare_repository,
            is_shallow_repository,
            revs,
        } => run_rev_parse(
            short,
            abbrev_ref,
            verify,
            quiet,
            symbolic_full_name,
            bisect,
            path_format,
            since,
            until,
            show_object_format,
            show_ref_format,
            show_toplevel,
            show_prefix,
            show_cdup,
            show_superproject_working_tree,
            git_dir,
            absolute_git_dir,
            git_common_dir,
            git_paths,
            is_inside_git_dir,
            is_inside_work_tree,
            is_bare_repository,
            is_shallow_repository,
            revs,
            raw_args,
        ),
        _ => unreachable!("non-reference command dispatched to reference"),
    }
}

fn resolve_symbolic_ref_delete(delete: bool, no_delete: bool, raw_args: &[String]) -> bool {
    resolve_symbolic_ref_toggle(
        raw_args,
        &["-d", "--delete"],
        &["--no-delete"],
        delete,
        no_delete,
    )
}

fn resolve_symbolic_ref_toggle(
    raw_args: &[String],
    enabled_flags: &[&str],
    disabled_flags: &[&str],
    enabled: bool,
    disabled: bool,
) -> bool {
    if !enabled && !disabled {
        return false;
    }

    let mut resolved = false;
    let mut saw_toggle = false;
    for arg in raw_args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        let value = arg.as_str();
        if enabled_flags.contains(&value) {
            resolved = true;
            saw_toggle = true;
        } else if disabled_flags.contains(&value) {
            resolved = false;
            saw_toggle = true;
        }
    }

    if saw_toggle {
        resolved
    } else {
        enabled && !disabled
    }
}

pub(crate) fn run_remote(
    verbose: bool,
    command: Option<runtime::RemoteCommand>,
) -> std::result::Result<(), runtime::CliError> {
    super::reference_commands::remote_command(verbose, command)
}

pub(crate) fn run_pack_refs(
    all: bool,
    auto: bool,
    include: Vec<String>,
    exclude: Vec<String>,
    prune: bool,
    no_prune: bool,
) -> std::result::Result<(), runtime::CliError> {
    super::reference_commands::pack_refs(all, auto, include, exclude, prune, no_prune)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn run_update_ref(
    delete: bool,
    no_deref: bool,
    stdin: bool,
    nul_terminated: bool,
    message: Option<String>,
    create_reflog: bool,
    batch_updates: bool,
    name: Option<String>,
    newvalue: Option<String>,
) -> std::result::Result<(), runtime::CliError> {
    super::reference_commands::update_ref(super::reference_commands::UpdateRefCommandOptions {
        delete,
        no_deref,
        stdin,
        nul_terminated,
        message: message.as_deref(),
        create_reflog,
        batch_updates,
        name: name.as_deref(),
        newvalue: newvalue.as_deref(),
    })
}

pub(crate) fn run_symbolic_ref(
    quiet: bool,
    delete: bool,
    short: bool,
    no_recurse: bool,
    message: Option<String>,
    name: String,
    target: Vec<String>,
) -> std::result::Result<(), runtime::CliError> {
    super::reference_commands::symbolic_ref(
        quiet,
        delete,
        short,
        no_recurse,
        message.as_deref(),
        &name,
        target,
    )
}

pub(crate) fn run_refs(
    command: runtime::RefsCommand,
    raw_args: &[String],
) -> std::result::Result<(), runtime::CliError> {
    super::reference_commands::refs_command(resolve_refs_command_toggles(command, raw_args))
}

fn resolve_refs_command_toggles(
    command: runtime::RefsCommand,
    raw_args: &[String],
) -> runtime::RefsCommand {
    match command {
        runtime::RefsCommand::Verify {
            strict,
            no_strict,
            verbose,
            no_verbose,
            dry_run,
            ref_format,
        } => runtime::RefsCommand::Verify {
            strict: resolve_refs_toggle(
                raw_args,
                &["--strict"],
                &["--no-strict"],
                strict,
                no_strict,
            ),
            no_strict: false,
            verbose: resolve_refs_toggle(
                raw_args,
                &["--verbose"],
                &["--no-verbose"],
                verbose,
                no_verbose,
            ),
            no_verbose: false,
            dry_run,
            ref_format,
        },
    }
}

fn resolve_refs_toggle(
    raw_args: &[String],
    enabled_spells: &[&str],
    disabled_spells: &[&str],
    enabled_present: bool,
    disabled_present: bool,
) -> bool {
    if !enabled_present && !disabled_present {
        return false;
    }

    let mut resolved = false;
    for arg in raw_args {
        let arg = arg.as_str();
        if enabled_spells.contains(&arg) {
            resolved = true;
        } else if disabled_spells.contains(&arg) {
            resolved = false;
        }
    }
    resolved
}

pub(crate) fn run_repo(
    command: runtime::RepoCommand,
) -> std::result::Result<(), runtime::CliError> {
    super::reference_commands::repo_command(command)
}

pub(crate) fn run_show_ref(
    quiet: bool,
    head: bool,
    heads: bool,
    tags: bool,
    dereference: bool,
    hash: Option<usize>,
    abbrev: Option<usize>,
    verify: bool,
    exists: bool,
    exclude_existing: Option<String>,
    refs: Vec<String>,
) -> std::result::Result<(), runtime::CliError> {
    super::reference_commands::show_ref(
        quiet,
        head,
        heads,
        tags,
        dereference,
        hash,
        abbrev,
        verify,
        exists,
        exclude_existing.as_deref(),
        refs,
    )
}

pub(crate) fn run_for_each_ref(
    format: Option<String>,
    sort: Vec<String>,
    patterns: Vec<String>,
) -> std::result::Result<(), runtime::CliError> {
    super::reference_commands::for_each_ref(format.as_deref(), sort, patterns)
}

pub(crate) fn run_ls_tree(
    recursive: bool,
    show_trees: bool,
    name_only: bool,
    treeish: String,
    paths: Vec<String>,
) -> std::result::Result<(), runtime::CliError> {
    super::reference_commands::ls_tree_command(recursive, show_trees, name_only, &treeish, paths)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn run_branch(
    help: bool,
    remotes: bool,
    all: bool,
    list: bool,
    force: bool,
    quiet: bool,
    verbose: u8,
    no_verbose: bool,
    abbrev: Option<usize>,
    no_abbrev: bool,
    column: Option<String>,
    no_column: bool,
    ignore_case: bool,
    color: Option<String>,
    no_color: bool,
    create_reflog: bool,
    show_current: bool,
    edit_description: bool,
    delete: bool,
    force_delete: bool,
    move_branch: bool,
    force_move: bool,
    copy_branch: bool,
    force_copy: bool,
    set_upstream_to: Option<String>,
    set_upstream: bool,
    unset_upstream: bool,
    track: Option<String>,
    no_track: bool,
    sort: Vec<String>,
    format: Option<String>,
    no_format: bool,
    omit_empty: bool,
    no_sort: bool,
    recurse_submodules: bool,
    no_recurse_submodules: bool,
    contains: Option<String>,
    no_contains: Option<String>,
    merged: Option<String>,
    no_merged: Option<String>,
    points_at: Option<String>,
    name: Option<String>,
    start_point: Option<String>,
    extra_args: Vec<String>,
    raw_args: &[String],
) -> std::result::Result<(), runtime::CliError> {
    super::reference_commands::branch_command(
        help,
        remotes,
        all,
        list,
        force,
        quiet,
        verbose,
        no_verbose,
        abbrev,
        no_abbrev,
        column,
        no_column,
        ignore_case,
        color,
        no_color,
        create_reflog,
        show_current,
        edit_description,
        delete,
        force_delete,
        move_branch,
        force_move,
        copy_branch,
        force_copy,
        set_upstream_to,
        set_upstream,
        unset_upstream,
        track,
        no_track,
        sort,
        format,
        no_format,
        omit_empty,
        no_sort,
        recurse_submodules,
        no_recurse_submodules,
        contains,
        no_contains,
        merged,
        no_merged,
        points_at,
        name,
        start_point,
        extra_args,
        raw_args,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn run_tag(
    delete: bool,
    verify: bool,
    list: bool,
    no_column: bool,
    ignore_case: bool,
    color: Option<String>,
    no_color: bool,
    force: bool,
    annotate: bool,
    messages: Vec<String>,
    contains: Option<String>,
    no_contains: Option<String>,
    merged: Option<String>,
    no_merged: Option<String>,
    sort: Vec<String>,
    format: Option<String>,
    args: Vec<String>,
) -> std::result::Result<(), runtime::CliError> {
    super::reference_commands::tag_command(
        delete,
        verify,
        list,
        no_column,
        ignore_case,
        color,
        no_color,
        force,
        annotate,
        messages,
        contains,
        no_contains,
        merged,
        no_merged,
        sort,
        format,
        args,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn run_replace(
    list: bool,
    delete: bool,
    force: bool,
    format: Option<String>,
    edit: bool,
    graft: bool,
    convert_graft_file: bool,
    raw: bool,
    args: Vec<String>,
) -> std::result::Result<(), runtime::CliError> {
    super::reference_commands::replace(super::reference_commands::ReplaceOptions {
        list,
        delete,
        force,
        format,
        edit,
        graft,
        convert_graft_file,
        raw,
        args,
    })
}

pub(crate) fn run_patch_id(
    stable: bool,
    unstable: bool,
    verbatim: bool,
) -> std::result::Result<(), runtime::CliError> {
    super::reference_commands::patch_id(stable, unstable, verbatim)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn run_rev_parse(
    short: Option<usize>,
    abbrev_ref: Option<String>,
    verify: bool,
    quiet: bool,
    symbolic_full_name: bool,
    bisect: bool,
    path_format: Vec<String>,
    since: Vec<String>,
    until: Vec<String>,
    show_object_format: Vec<String>,
    show_ref_format: bool,
    show_toplevel: bool,
    show_prefix: bool,
    show_cdup: bool,
    show_superproject_working_tree: bool,
    git_dir: bool,
    absolute_git_dir: bool,
    git_common_dir: bool,
    git_paths: Vec<std::path::PathBuf>,
    is_inside_git_dir: bool,
    is_inside_work_tree: bool,
    is_bare_repository: bool,
    is_shallow_repository: bool,
    revs: Vec<String>,
    raw_args: &[String],
) -> std::result::Result<(), runtime::CliError> {
    super::reference_commands::rev_parse_command(
        short,
        abbrev_ref,
        verify,
        quiet,
        symbolic_full_name,
        bisect,
        path_format,
        since,
        until,
        show_object_format,
        show_ref_format,
        show_toplevel,
        show_prefix,
        show_cdup,
        show_superproject_working_tree,
        git_dir,
        absolute_git_dir,
        git_common_dir,
        git_paths,
        is_inside_git_dir,
        is_inside_work_tree,
        is_bare_repository,
        is_shallow_repository,
        revs,
        raw_args,
    )
}
