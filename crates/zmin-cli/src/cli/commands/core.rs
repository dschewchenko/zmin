use crate::runtime;
use std::path::PathBuf;

pub(crate) fn dispatch(command: runtime::Command) -> std::result::Result<(), runtime::CliError> {
    match command {
        runtime::Command::Init {
            bare,
            template,
            separate_git_dir,
            shared,
            initial_branch,
            object_format,
            ref_format,
            directory,
        } => run_init(
            directory,
            bare,
            template,
            separate_git_dir,
            shared,
            initial_branch,
            object_format,
            ref_format,
        ),
        runtime::Command::HashObject {
            object_type,
            write,
            stdin,
            paths,
        } => run_hash_object(object_type, write, stdin, paths),
        runtime::Command::CatFile {
            type_only,
            pretty,
            size,
            exists,
            textconv,
            filters,
            path,
            batch_check,
            batch,
            batch_command,
            batch_all_objects,
            buffer,
            no_buffer,
            follow_symlinks,
            nul,
            full_nul,
            unordered,
            no_unordered,
            filter,
            no_filter,
            objects,
        } => run_cat_file(
            type_only,
            pretty,
            size,
            exists,
            textconv,
            filters,
            path,
            batch_check,
            batch,
            batch_command,
            batch_all_objects,
            buffer,
            no_buffer,
            follow_symlinks,
            nul,
            full_nul,
            unordered,
            no_unordered,
            filter,
            no_filter,
            objects,
        ),
        runtime::Command::CountObjects {
            verbose,
            human_readable,
        } => run_count_objects(verbose, human_readable),
        runtime::Command::UnpackFile { object } => run_unpack_file(object),
        runtime::Command::ShowIndex => run_show_index(),
        runtime::Command::UpdateServerInfo { force: _ } => run_update_server_info(),
        runtime::Command::CheckRefFormat {
            allow_onelevel,
            normalize,
            branch,
            refname,
        } => run_check_ref_format(allow_onelevel, normalize, branch, refname),
        runtime::Command::CheckIgnore {
            quiet,
            verbose,
            non_matching,
            stdin,
            nul,
            no_index,
            paths,
        } => run_check_ignore(
            quiet > 0,
            verbose > 0,
            non_matching > 0,
            stdin > 0,
            nul > 0,
            no_index > 0,
            paths,
        ),
        runtime::Command::CheckMailmap { stdin, identities } => {
            run_check_mailmap(stdin, identities)
        }
        runtime::Command::CheckAttr { all, stdin, args } => run_check_attr(all, stdin, args),
        runtime::Command::UnpackObjects {
            dry_run,
            quiet,
            recover,
            strict,
        } => run_unpack_objects(dry_run, quiet, recover, strict),
        _ => unreachable!("non-core command dispatched to core"),
    }
}

pub(crate) fn run_init(
    directory: Option<PathBuf>,
    bare: bool,
    template: Option<PathBuf>,
    separate_git_dir: Option<PathBuf>,
    shared: Option<String>,
    initial_branch: Option<String>,
    object_format: Option<String>,
    ref_format: Option<String>,
) -> std::result::Result<(), runtime::CliError> {
    super::core_commands::init_command(
        directory,
        bare,
        template,
        separate_git_dir,
        shared,
        initial_branch,
        object_format,
        ref_format,
    )
}

pub(crate) fn run_hash_object(
    object_type: String,
    write: bool,
    stdin: bool,
    paths: Vec<PathBuf>,
) -> std::result::Result<(), runtime::CliError> {
    super::core_commands::hash_object_command(&object_type, write, stdin, paths)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn run_cat_file(
    type_only: bool,
    pretty: bool,
    size: bool,
    exists: bool,
    textconv: bool,
    filters: bool,
    path: Option<String>,
    batch_check: Option<String>,
    batch: Option<String>,
    batch_command: Option<String>,
    batch_all_objects: bool,
    buffer: bool,
    no_buffer: bool,
    follow_symlinks: bool,
    nul: bool,
    full_nul: bool,
    unordered: bool,
    no_unordered: bool,
    filter: Option<String>,
    no_filter: bool,
    objects: Vec<String>,
) -> std::result::Result<(), runtime::CliError> {
    super::core_commands::cat_file(
        type_only,
        pretty,
        size,
        exists,
        textconv,
        filters,
        path,
        batch_check,
        batch,
        batch_command,
        batch_all_objects,
        buffer,
        no_buffer,
        follow_symlinks,
        nul,
        full_nul,
        unordered,
        no_unordered,
        filter,
        no_filter,
        objects,
    )
}

pub(crate) fn run_count_objects(
    verbose: bool,
    human_readable: bool,
) -> std::result::Result<(), runtime::CliError> {
    super::core_commands::count_objects(verbose, human_readable)
}

pub(crate) fn run_unpack_file(object: String) -> std::result::Result<(), runtime::CliError> {
    super::core_commands::unpack_file(&object)
}

pub(crate) fn run_show_index() -> std::result::Result<(), runtime::CliError> {
    super::core_commands::show_index()
}

pub(crate) fn run_update_server_info() -> std::result::Result<(), runtime::CliError> {
    super::core_commands::update_server_info()
}

pub(crate) fn run_check_ref_format(
    allow_onelevel: bool,
    normalize: bool,
    branch: Option<String>,
    refname: Option<String>,
) -> std::result::Result<(), runtime::CliError> {
    super::core_commands::check_ref_format_command(
        allow_onelevel,
        normalize,
        branch.as_deref(),
        refname.as_deref(),
    )
}

pub(crate) fn run_check_ignore(
    quiet: bool,
    verbose: bool,
    non_matching: bool,
    stdin: bool,
    nul: bool,
    no_index: bool,
    paths: Vec<PathBuf>,
) -> std::result::Result<(), runtime::CliError> {
    super::core_commands::check_ignore(quiet, verbose, non_matching, stdin, nul, no_index, paths)
}

pub(crate) fn run_check_mailmap(
    stdin: bool,
    identities: Vec<String>,
) -> std::result::Result<(), runtime::CliError> {
    super::core_commands::check_mailmap(stdin, identities)
}

pub(crate) fn run_check_attr(
    all: bool,
    stdin: bool,
    args: Vec<String>,
) -> std::result::Result<(), runtime::CliError> {
    super::core_commands::check_attr(all, stdin, args)
}

pub(crate) fn run_unpack_objects(
    dry_run: bool,
    quiet: bool,
    recover: bool,
    strict: bool,
) -> std::result::Result<(), runtime::CliError> {
    super::core_commands::unpack_objects(dry_run, quiet, recover, strict)
}
