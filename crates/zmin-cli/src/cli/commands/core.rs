use crate::runtime;
use std::path::PathBuf;

pub(crate) fn dispatch(
    command: runtime::Command,
    raw_args: &[String],
) -> std::result::Result<(), runtime::CliError> {
    match command {
        runtime::Command::Init {
            quiet,
            bare,
            template,
            separate_git_dir,
            shared,
            initial_branch,
            object_format,
            ref_format,
            directory,
        } => run_init(
            quiet,
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
            stdin_paths,
            no_filters,
            literally,
            path,
            paths,
        } => run_hash_object(
            object_type,
            write,
            stdin,
            stdin_paths,
            no_filters,
            literally,
            path,
            paths,
        ),
        runtime::Command::CatFile {
            type_only,
            pretty,
            size,
            allow_unknown_type,
            exists,
            use_mailmap,
            no_use_mailmap,
            mailmap,
            no_mailmap,
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
        } => {
            let resolved = resolve_cat_file_args(
                raw_args,
                allow_unknown_type,
                use_mailmap,
                no_use_mailmap,
                mailmap,
                no_mailmap,
            );
            run_cat_file(
                type_only,
                pretty,
                size,
                resolved.allow_unknown_type,
                exists,
                resolved.use_mailmap,
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
        runtime::Command::CountObjects {
            verbose,
            no_verbose: _,
            human_readable,
            no_human_readable: _,
        } => run_count_objects(verbose > 0, human_readable > 0),
        runtime::Command::UnpackFile { object } => run_unpack_file(object),
        runtime::Command::ShowIndex {
            object_format,
            no_object_format,
        } => run_show_index(object_format, no_object_format, raw_args),
        runtime::Command::UpdateServerInfo {
            force: _,
            no_force: _,
        } => run_update_server_info(),
        runtime::Command::CheckRefFormat {
            allow_onelevel,
            no_allow_onelevel,
            normalize,
            refspec_pattern,
            branch,
            refname,
        } => run_check_ref_format(
            allow_onelevel,
            no_allow_onelevel,
            normalize,
            refspec_pattern,
            branch,
            refname,
        ),
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
        runtime::Command::CheckMailmap {
            mailmap_file,
            mailmap_blob,
            stdin,
            no_stdin,
            identities,
        } => {
            let resolved =
                resolve_check_mailmap_args(raw_args, mailmap_file, mailmap_blob, stdin, no_stdin);
            run_check_mailmap(
                resolved.mailmap_file,
                resolved.mailmap_blob,
                resolved.stdin_enabled,
                resolved.empty_requires_no_contacts,
                identities,
            )
        }
        runtime::Command::CheckAttr {
            all,
            cached,
            stdin,
            nul,
            source,
            args,
        } => run_check_attr(all, cached, stdin, nul, source, args),
        runtime::Command::UnpackObjects {
            dry_run,
            quiet,
            recover,
            strict,
            max_input_size,
        } => run_unpack_objects(
            dry_run > 0,
            quiet > 0,
            recover > 0,
            strict > 0,
            max_input_size,
        ),
        _ => unreachable!("non-core command dispatched to core"),
    }
}

pub(crate) fn run_init(
    quiet: bool,
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
        quiet,
    )
}

pub(crate) fn run_hash_object(
    object_type: String,
    write: bool,
    stdin: bool,
    stdin_paths: bool,
    no_filters: bool,
    literally: bool,
    path: Option<String>,
    paths: Vec<PathBuf>,
) -> std::result::Result<(), runtime::CliError> {
    super::core_commands::hash_object_command(
        &object_type,
        write,
        stdin,
        stdin_paths,
        no_filters,
        literally,
        path,
        paths,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn run_cat_file(
    type_only: bool,
    pretty: bool,
    size: bool,
    allow_unknown_type: bool,
    exists: bool,
    use_mailmap: bool,
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
        allow_unknown_type,
        exists,
        use_mailmap,
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

struct CatFileArgs {
    allow_unknown_type: bool,
    use_mailmap: bool,
}

fn resolve_cat_file_args(
    raw_args: &[String],
    allow_unknown_type: bool,
    use_mailmap: bool,
    no_use_mailmap: bool,
    mailmap: bool,
    no_mailmap: bool,
) -> CatFileArgs {
    let mut resolved_use_mailmap = use_mailmap || mailmap;
    let mut resolved_allow_unknown_type = allow_unknown_type;

    for arg in raw_args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        match arg.as_str() {
            "--allow-unknown-type" => resolved_allow_unknown_type = true,
            "--use-mailmap" | "--mailmap" => resolved_use_mailmap = true,
            "--no-use-mailmap" | "--no-mailmap" => resolved_use_mailmap = false,
            _ => {}
        }
    }

    if !allow_unknown_type {
        resolved_allow_unknown_type = false;
    }
    if !(use_mailmap || no_use_mailmap || mailmap || no_mailmap) {
        resolved_use_mailmap = false;
    }

    CatFileArgs {
        allow_unknown_type: resolved_allow_unknown_type,
        use_mailmap: resolved_use_mailmap,
    }
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

pub(crate) fn run_show_index(
    object_format: Vec<String>,
    no_object_format: u8,
    raw_args: &[String],
) -> std::result::Result<(), runtime::CliError> {
    super::core_commands::show_index(
        show_index_object_format(object_format, no_object_format, raw_args).as_deref(),
    )
}

fn show_index_object_format(
    object_format: Vec<String>,
    no_object_format: u8,
    raw_args: &[String],
) -> Option<String> {
    if object_format.is_empty() {
        return None;
    }
    if no_object_format == 0 {
        return object_format.last().cloned();
    }

    let mut object_formats = object_format.into_iter();
    let mut remaining_no = usize::from(no_object_format);
    let mut resolved = None;
    for arg in raw_args {
        if arg == "--no-object-format" {
            remaining_no = remaining_no.saturating_sub(1);
            resolved = None;
            continue;
        }
        if arg == "--object-format" {
            if let Some(value) = object_formats.next() {
                resolved = Some(value);
            }
            continue;
        }
        if let Some(value) = arg.strip_prefix("--object-format=") {
            if !value.is_empty() {
                let _ = object_formats.next();
                resolved = Some(value.to_owned());
            }
        }
    }

    if remaining_no == 0 && object_formats.len() == 0 {
        return resolved;
    }

    resolved.or_else(|| object_formats.last())
}

pub(crate) fn run_update_server_info() -> std::result::Result<(), runtime::CliError> {
    super::core_commands::update_server_info()
}

pub(crate) fn run_check_ref_format(
    allow_onelevel: bool,
    no_allow_onelevel: bool,
    normalize: bool,
    refspec_pattern: bool,
    branch: Option<String>,
    refname: Option<String>,
) -> std::result::Result<(), runtime::CliError> {
    let allow_onelevel_option_present = allow_onelevel || no_allow_onelevel;
    let allow_onelevel = resolve_check_ref_format_allow_onelevel(allow_onelevel, no_allow_onelevel);
    super::core_commands::check_ref_format_command(
        allow_onelevel,
        allow_onelevel_option_present,
        normalize,
        refspec_pattern,
        branch.as_deref(),
        refname.as_deref(),
    )
}

fn resolve_check_ref_format_allow_onelevel(allow_onelevel: bool, no_allow_onelevel: bool) -> bool {
    if !allow_onelevel && !no_allow_onelevel {
        return false;
    }

    let mut resolved = false;
    for arg in std::env::args_os() {
        if arg == "--allow-onelevel" {
            resolved = true;
        } else if arg == "--no-allow-onelevel" {
            resolved = false;
        }
    }
    resolved
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
    mailmap_file: Option<PathBuf>,
    mailmap_blob: Option<String>,
    stdin: bool,
    empty_requires_no_contacts: bool,
    identities: Vec<String>,
) -> std::result::Result<(), runtime::CliError> {
    super::core_commands::check_mailmap(
        mailmap_file,
        mailmap_blob,
        stdin,
        empty_requires_no_contacts,
        identities,
    )
}

struct CheckMailmapArgs {
    mailmap_file: Option<PathBuf>,
    mailmap_blob: Option<String>,
    stdin_enabled: bool,
    empty_requires_no_contacts: bool,
}

fn resolve_check_mailmap_args(
    raw_args: &[String],
    mailmap_file: Option<PathBuf>,
    mailmap_blob: Option<String>,
    stdin: u8,
    no_stdin: bool,
) -> CheckMailmapArgs {
    let fallback_file = mailmap_file.clone();
    let fallback_blob = mailmap_blob.clone();
    let mut stdin_enabled = stdin > 0;
    let mut empty_requires_no_contacts = no_stdin && stdin == 0;
    let mut next_file = mailmap_file.into_iter();
    let mut next_blob = mailmap_blob.into_iter();
    let mut resolved_file = None;
    let mut resolved_blob = None;
    let mut saw_file_option = false;
    let mut saw_blob_option = false;

    for arg in raw_args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        match arg.as_str() {
            "--stdin" => {
                stdin_enabled = true;
                empty_requires_no_contacts = false;
            }
            "--no-stdin" => {
                stdin_enabled = false;
                empty_requires_no_contacts = true;
            }
            "--mailmap-file" => {
                if let Some(value) = next_file.next() {
                    saw_file_option = true;
                    resolved_file = Some(value);
                    resolved_blob = None;
                }
            }
            "--mailmap-blob" => {
                if let Some(value) = next_blob.next() {
                    saw_blob_option = true;
                    resolved_blob = Some(value);
                    resolved_file = None;
                }
            }
            _ if arg.starts_with("--mailmap-file=") => {
                if let Some(value) = next_file.next() {
                    saw_file_option = true;
                    resolved_file = Some(value);
                    resolved_blob = None;
                }
            }
            _ if arg.starts_with("--mailmap-blob=") => {
                if let Some(value) = next_blob.next() {
                    saw_blob_option = true;
                    resolved_blob = Some(value);
                    resolved_file = None;
                }
            }
            _ => {}
        }
    }

    CheckMailmapArgs {
        mailmap_file: if saw_file_option || saw_blob_option {
            resolved_file
        } else {
            fallback_file
        },
        mailmap_blob: if saw_file_option || saw_blob_option {
            resolved_blob
        } else {
            fallback_blob
        },
        stdin_enabled,
        empty_requires_no_contacts,
    }
}

pub(crate) fn run_check_attr(
    all: bool,
    cached: bool,
    stdin: bool,
    nul: bool,
    source: Option<String>,
    args: Vec<String>,
) -> std::result::Result<(), runtime::CliError> {
    super::core_commands::check_attr(all, cached, stdin, nul, source, args)
}

pub(crate) fn run_unpack_objects(
    dry_run: bool,
    quiet: bool,
    recover: bool,
    strict: bool,
    max_input_size: Vec<String>,
) -> std::result::Result<(), runtime::CliError> {
    super::core_commands::unpack_objects(dry_run, quiet, recover, strict, max_input_size)
}

#[cfg(test)]
mod tests {
    use super::show_index_object_format;

    #[test]
    fn show_index_object_format_respects_last_occurrence() {
        assert_eq!(
            show_index_object_format(
                vec!["sha1".into()],
                1,
                &[
                    "show-index".into(),
                    "--object-format=sha1".into(),
                    "--no-object-format".into(),
                ],
            ),
            None
        );
        assert_eq!(
            show_index_object_format(
                vec!["bogus".into()],
                1,
                &[
                    "show-index".into(),
                    "--no-object-format".into(),
                    "--object-format=bogus".into(),
                ],
            ),
            Some("bogus".into())
        );
        assert_eq!(
            show_index_object_format(
                vec!["sha1".into(), "sha256".into()],
                1,
                &[
                    "show-index".into(),
                    "--object-format=sha1".into(),
                    "--no-object-format".into(),
                    "--object-format=sha256".into(),
                ],
            ),
            Some("sha256".into())
        );
    }
}
