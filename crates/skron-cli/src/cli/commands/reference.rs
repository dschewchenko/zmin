use crate::runtime;

pub(crate) fn dispatch(
    command: runtime::Command,
    raw_args: &[String],
) -> std::result::Result<(), runtime::CliError> {
    match command {
        runtime::Command::Remote { verbose, command } => run_remote(verbose, command),
        runtime::Command::PackRefs {
            all,
            prune,
            no_prune,
        } => run_pack_refs(all, prune, no_prune),
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
            short,
            name,
            target,
        } => run_symbolic_ref(quiet, short, name, target),
        runtime::Command::Refs { command } => run_refs(command),
        runtime::Command::Repo { command } => run_repo(command),
        runtime::Command::ShowRef {
            head,
            heads,
            tags,
            hash,
            abbrev,
            verify,
            refs,
        } => run_show_ref(head, heads, tags, hash, abbrev, verify, refs),
        runtime::Command::ForEachRef {
            format,
            sort,
            patterns,
        } => run_for_each_ref(format, sort, patterns),
        runtime::Command::LsTree {
            recursive,
            name_only,
            treeish,
            paths,
        } => run_ls_tree(recursive, name_only, treeish, paths),
        runtime::Command::Branch {
            remotes,
            all,
            list,
            show_current,
            delete,
            force_delete,
            move_branch,
            force_move,
            copy_branch,
            force_copy,
            set_upstream_to,
            unset_upstream,
            contains,
            merged,
            no_merged,
            name,
            start_point,
        } => run_branch(
            remotes,
            all,
            list,
            show_current,
            delete,
            force_delete,
            move_branch,
            force_move,
            copy_branch,
            force_copy,
            set_upstream_to,
            unset_upstream,
            contains,
            merged,
            no_merged,
            name,
            start_point,
        ),
        runtime::Command::Tag {
            delete,
            verify,
            list,
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
        } => run_patch_id(stable, unstable, verbatim),
        runtime::Command::RevParse {
            short,
            abbrev_ref,
            verify,
            show_object_format,
            show_toplevel,
            show_prefix,
            show_cdup,
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
            show_object_format,
            show_toplevel,
            show_prefix,
            show_cdup,
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

pub(crate) fn run_remote(
    verbose: bool,
    command: Option<runtime::RemoteCommand>,
) -> std::result::Result<(), runtime::CliError> {
    runtime::reference_commands::remote_command(verbose, command)
}

pub(crate) fn run_pack_refs(
    all: bool,
    prune: bool,
    no_prune: bool,
) -> std::result::Result<(), runtime::CliError> {
    runtime::reference_commands::pack_refs(all, prune, no_prune)
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
    runtime::reference_commands::update_ref(runtime::reference_commands::UpdateRefCommandOptions {
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
    short: bool,
    name: String,
    target: Vec<String>,
) -> std::result::Result<(), runtime::CliError> {
    runtime::reference_commands::symbolic_ref(quiet, short, &name, target)
}

pub(crate) fn run_refs(
    command: runtime::RefsCommand,
) -> std::result::Result<(), runtime::CliError> {
    runtime::reference_commands::refs_command(command)
}

pub(crate) fn run_repo(
    command: runtime::RepoCommand,
) -> std::result::Result<(), runtime::CliError> {
    runtime::reference_commands::repo_command(command)
}

pub(crate) fn run_show_ref(
    head: bool,
    heads: bool,
    tags: bool,
    hash: Option<usize>,
    abbrev: Option<usize>,
    verify: bool,
    refs: Vec<String>,
) -> std::result::Result<(), runtime::CliError> {
    runtime::reference_commands::show_ref(head, heads, tags, hash, abbrev, verify, refs)
}

pub(crate) fn run_for_each_ref(
    format: Option<String>,
    sort: Vec<String>,
    patterns: Vec<String>,
) -> std::result::Result<(), runtime::CliError> {
    runtime::reference_commands::for_each_ref(format.as_deref(), sort, patterns)
}

pub(crate) fn run_ls_tree(
    recursive: bool,
    name_only: bool,
    treeish: String,
    paths: Vec<String>,
) -> std::result::Result<(), runtime::CliError> {
    runtime::reference_commands::ls_tree_command(recursive, name_only, &treeish, paths)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn run_branch(
    remotes: bool,
    all: bool,
    list: bool,
    show_current: bool,
    delete: bool,
    force_delete: bool,
    move_branch: bool,
    force_move: bool,
    copy_branch: bool,
    force_copy: bool,
    set_upstream_to: Option<String>,
    unset_upstream: bool,
    contains: Option<String>,
    merged: Option<String>,
    no_merged: Option<String>,
    name: Option<String>,
    start_point: Option<String>,
) -> std::result::Result<(), runtime::CliError> {
    runtime::reference_commands::branch_command(
        remotes,
        all,
        list,
        show_current,
        delete,
        force_delete,
        move_branch,
        force_move,
        copy_branch,
        force_copy,
        set_upstream_to,
        unset_upstream,
        contains,
        merged,
        no_merged,
        name,
        start_point,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn run_tag(
    delete: bool,
    verify: bool,
    list: bool,
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
    runtime::reference_commands::tag_command(
        delete,
        verify,
        list,
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
    runtime::reference_commands::replace(runtime::reference_commands::ReplaceOptions {
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
    runtime::reference_commands::patch_id(stable, unstable, verbatim)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn run_rev_parse(
    short: Option<usize>,
    abbrev_ref: Option<String>,
    verify: bool,
    show_object_format: Vec<String>,
    show_toplevel: bool,
    show_prefix: bool,
    show_cdup: bool,
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
    runtime::reference_commands::rev_parse_command(
        short,
        abbrev_ref,
        verify,
        show_object_format,
        show_toplevel,
        show_prefix,
        show_cdup,
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
