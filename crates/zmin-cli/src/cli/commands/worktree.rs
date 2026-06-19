use crate::runtime;
use std::path::PathBuf;

pub(crate) fn dispatch(command: runtime::Command) -> std::result::Result<(), runtime::CliError> {
    match command {
        runtime::Command::LsFiles {
            cached,
            zero,
            full_name,
            error_unmatch,
            tagged,
            lowercase_assume_valid,
            fsmonitor_clean,
            deduplicate,
            sparse,
            recurse_submodules,
            no_recurse_submodules,
            debug,
            abbrev,
            eol,
            format,
            with_tree,
            resolve_undo,
            stage,
            unmerged,
            deleted,
            modified,
            others,
            killed,
            directory,
            empty_directory,
            ignored,
            excludes,
            exclude_from,
            exclude_per_directory,
            exclude_standard,
            paths,
        } => run_ls_files(runtime::LsFilesOptions {
            cached,
            stage,
            unmerged,
            deleted,
            modified,
            others,
            killed,
            directory,
            empty_directory,
            ignored,
            excludes,
            exclude_from,
            exclude_per_directory,
            exclude_standard,
            zero,
            full_name,
            error_unmatch,
            tagged,
            lowercase_assume_valid,
            fsmonitor_clean,
            deduplicate,
            sparse,
            recurse_submodules,
            no_recurse_submodules,
            debug,
            abbrev,
            eol,
            format,
            with_tree,
            resolve_undo,
            path_args: paths,
        }),
        runtime::Command::Add {
            all,
            force,
            update,
            intent_to_add,
            refresh,
            verbose,
            ignore_errors,
            ignore_missing,
            chmod,
            dry_run,
            pathspec_from_file,
            pathspec_file_nul,
            paths,
        }
        | runtime::Command::Stage {
            all,
            force,
            update,
            intent_to_add,
            refresh,
            verbose,
            ignore_errors,
            ignore_missing,
            chmod,
            dry_run,
            pathspec_from_file,
            pathspec_file_nul,
            paths,
        } => run_add(
            all,
            force,
            update,
            intent_to_add,
            refresh,
            verbose,
            ignore_errors,
            ignore_missing,
            chmod,
            dry_run,
            pathspec_from_file,
            pathspec_file_nul,
            paths,
        ),
        runtime::Command::Rm {
            force,
            dry_run,
            quiet,
            recursive,
            cached,
            ignore_unmatch,
            pathspec_from_file,
            pathspec_file_nul,
            paths,
        } => run_rm(runtime::RmOptions {
            force,
            dry_run,
            quiet,
            recursive,
            cached,
            ignore_unmatch,
            pathspec_from_file,
            pathspec_file_nul,
            paths,
        }),
        runtime::Command::Mv { force, paths } => run_mv(force, paths),
        runtime::Command::Status {
            porcelain,
            branch,
            ahead_behind: _,
            no_ahead_behind,
            show_stash,
            no_show_stash,
            verbose,
            no_verbose,
            long: _,
            no_long: _,
            column,
            no_column,
            short,
            null,
            ignored,
            untracked_files,
            paths,
        } => run_status(
            porcelain,
            branch,
            !no_ahead_behind,
            show_stash && !no_show_stash,
            if no_verbose { 0 } else { verbose },
            column,
            no_column,
            short,
            null,
            ignored,
            untracked_files,
            paths,
        ),
        runtime::Command::ReadTree {
            empty,
            merge,
            prefix,
            treeish,
        } => run_read_tree(empty, merge, prefix, treeish),
        runtime::Command::Checkout {
            force,
            quiet,
            no_progress,
            detach,
            recurse_submodules,
            no_recurse_submodules,
            create,
            reset_create,
            create_reflog,
            orphan,
            args,
        } => super::worktree_commands::checkout(
            force,
            quiet,
            no_progress,
            detach,
            recurse_submodules,
            no_recurse_submodules,
            create,
            reset_create,
            create_reflog,
            orphan,
            args,
        ),
        runtime::Command::CheckoutIndex {
            all,
            force,
            quiet,
            stdin,
            prefix,
            paths,
        } => run_checkout_index(all, force, quiet, stdin, prefix, paths),
        runtime::Command::Switch {
            force,
            discard_changes,
            create,
            orphan,
            detach,
            target,
        } => {
            super::worktree_commands::switch(force, discard_changes, create, orphan, detach, target)
        }
        runtime::Command::Restore {
            source,
            staged,
            worktree,
            paths,
        } => run_restore(source, staged, worktree, paths),
        runtime::Command::Clean { args } => run_clean(args),
        runtime::Command::Reset {
            soft,
            mixed,
            hard,
            args,
        } => run_reset(soft, mixed, hard, args),
        runtime::Command::Stash { args } => run_stash(args),
        runtime::Command::Worktree { args } => run_worktree(args),
        runtime::Command::SparseCheckout { args } => run_sparse_checkout(args),
        runtime::Command::Submodule { args } => run_submodule(args),
        _ => unreachable!("non-worktree command dispatched to worktree"),
    }
}

pub(crate) fn run_ls_files(
    options: runtime::LsFilesOptions,
) -> std::result::Result<(), runtime::CliError> {
    super::worktree_commands::ls_files(options)
}

pub(crate) fn run_add(
    all: bool,
    force: bool,
    update: bool,
    intent_to_add: bool,
    refresh: bool,
    verbose: bool,
    ignore_errors: bool,
    ignore_missing: bool,
    chmod: Option<String>,
    dry_run: bool,
    pathspec_from_file: Option<PathBuf>,
    pathspec_file_nul: bool,
    paths: Vec<PathBuf>,
) -> std::result::Result<(), runtime::CliError> {
    super::worktree_commands::add(
        all,
        force,
        update,
        intent_to_add,
        refresh,
        verbose,
        ignore_errors,
        ignore_missing,
        chmod,
        dry_run,
        pathspec_from_file,
        pathspec_file_nul,
        paths,
    )
}

pub(crate) fn run_rm(options: runtime::RmOptions) -> std::result::Result<(), runtime::CliError> {
    super::worktree_commands::rm(options)
}

pub(crate) fn run_mv(
    force: bool,
    paths: Vec<PathBuf>,
) -> std::result::Result<(), runtime::CliError> {
    super::worktree_commands::mv(force, paths)
}

pub(crate) fn run_status(
    porcelain: Option<String>,
    branch: bool,
    ahead_behind: bool,
    show_stash: bool,
    verbose: u8,
    column: Option<String>,
    no_column: bool,
    short: bool,
    null: bool,
    ignored: Option<String>,
    untracked_files: Option<String>,
    paths: Vec<PathBuf>,
) -> std::result::Result<(), runtime::CliError> {
    super::worktree_commands::status(
        porcelain.as_deref(),
        branch,
        ahead_behind,
        show_stash,
        verbose,
        column.as_deref(),
        no_column,
        short,
        null,
        ignored.as_deref(),
        untracked_files.as_deref(),
        paths,
    )
}

pub(crate) fn run_read_tree(
    empty: bool,
    merge: bool,
    prefix: Option<String>,
    treeish: Option<String>,
) -> std::result::Result<(), runtime::CliError> {
    super::worktree_commands::read_tree_command(empty, merge, prefix.as_deref(), treeish.as_deref())
}

pub(crate) fn run_checkout_index(
    all: bool,
    force: bool,
    quiet: bool,
    stdin: bool,
    prefix: Option<PathBuf>,
    paths: Vec<PathBuf>,
) -> std::result::Result<(), runtime::CliError> {
    super::worktree_commands::checkout_index_command(all, force, quiet, stdin, prefix, paths)
}

pub(crate) fn run_restore(
    source: Option<String>,
    staged: bool,
    worktree: bool,
    paths: Vec<PathBuf>,
) -> std::result::Result<(), runtime::CliError> {
    super::worktree_commands::restore(source.as_deref(), staged, worktree, paths).map(|_| ())
}

pub(crate) fn run_clean(args: Vec<String>) -> std::result::Result<(), runtime::CliError> {
    super::worktree_commands::clean(args)
}

pub(crate) fn run_reset(
    soft: bool,
    mixed: bool,
    hard: bool,
    args: Vec<String>,
) -> std::result::Result<(), runtime::CliError> {
    super::worktree_commands::reset(soft, mixed, hard, args)
}

pub(crate) fn run_stash(args: Vec<String>) -> std::result::Result<(), runtime::CliError> {
    super::worktree_commands::stash(args)
}

pub(crate) fn run_worktree(args: Vec<String>) -> std::result::Result<(), runtime::CliError> {
    super::worktree_commands::worktree(args)
}

pub(crate) fn run_sparse_checkout(args: Vec<String>) -> std::result::Result<(), runtime::CliError> {
    super::worktree_commands::sparse_checkout(args)
}

pub(crate) fn run_submodule(args: Vec<String>) -> std::result::Result<(), runtime::CliError> {
    super::worktree_commands::submodule(args)
}
