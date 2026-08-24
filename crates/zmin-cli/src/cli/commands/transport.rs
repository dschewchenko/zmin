use crate::runtime;

#[derive(Debug, Clone)]
pub(crate) struct CloneFilterCommandOptions {
    pub(crate) filter: runtime::PartialCloneFilterState,
    pub(crate) also_filter_submodules: bool,
}

impl CloneFilterCommandOptions {
    fn from_cli_values(
        filters: Vec<String>,
        no_filter: u8,
        also_filter_submodules: bool,
        raw_args: &[String],
    ) -> std::result::Result<Self, runtime::CliError> {
        let mut filter = runtime::PartialCloneFilterState::for_context(
            runtime::PartialCloneFilterCommandContext::Clone,
        );
        apply_ordered_filter_events(&mut filter, &filters, no_filter, raw_args)?;
        Ok(Self {
            filter,
            also_filter_submodules,
        })
    }

    fn transport_filter(&self) -> Option<String> {
        self.filter.effective_filter_spec()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct FetchFilterCommandOptions {
    pub(crate) filter: runtime::PartialCloneFilterState,
    pub(crate) refetch: bool,
}

/// Named fetch boundary state.  The low-level `FetchPackOptions` already
/// consumes `refetch` to omit haves; keeping that intent here prevents the
/// high-level CLI adapter from collapsing it into the filter scalar while the
/// legacy high-level transport entry point is migrated.
#[derive(Debug, Clone)]
pub(crate) struct FetchTransportCommandOptions {
    pub(crate) filter: Option<String>,
    pub(crate) no_haves_requested: bool,
    pub(crate) no_filter_requested: bool,
}

impl From<&FetchFilterCommandOptions> for FetchTransportCommandOptions {
    fn from(options: &FetchFilterCommandOptions) -> Self {
        Self {
            filter: options.transport_filter(),
            no_haves_requested: options.no_haves_requested(),
            no_filter_requested: options.filter.no_filter_requested(),
        }
    }
}

fn ensure_fetch_refetch_is_preserved(
    options: &FetchTransportCommandOptions,
    transport_args: &[String],
) -> std::result::Result<(), runtime::CliError> {
    if options.no_haves_requested && !transport_args.iter().any(|arg| arg == "--refetch") {
        return Err(runtime::CliError::Fatal {
            code: 128,
            message: "refetch intent was lost before fetch-pack dispatch".to_owned(),
        });
    }
    Ok(())
}

fn validate_clone_filter_submodules(
    options: &CloneFilterCommandOptions,
    recurse_submodules: &[String],
    recursive: &[String],
) -> std::result::Result<(), runtime::CliError> {
    if !options.also_filter_submodules {
        return Ok(());
    }
    if options.transport_filter().is_none() {
        return Err(runtime::CliError::Stderr {
            code: 128,
            text: "fatal: the option '--also-filter-submodules' requires '--filter'\n".into(),
        });
    }
    if recurse_submodules.is_empty() && recursive.is_empty() {
        return Err(runtime::CliError::Stderr {
            code: 128,
            text: "fatal: the option '--also-filter-submodules' requires '--recurse-submodules'\n"
                .into(),
        });
    }
    Ok(())
}

impl FetchFilterCommandOptions {
    fn from_cli_values(
        filters: Vec<String>,
        no_filter: u8,
        refetch: bool,
        raw_args: &[String],
    ) -> std::result::Result<Self, runtime::CliError> {
        let mut filter = runtime::PartialCloneFilterState::for_context(
            runtime::PartialCloneFilterCommandContext::Fetch,
        );
        apply_ordered_filter_events(&mut filter, &filters, no_filter, raw_args)?;
        Ok(Self { filter, refetch })
    }

    fn transport_filter(&self) -> Option<String> {
        self.filter.effective_filter_spec()
    }

    fn no_haves_requested(&self) -> bool {
        self.refetch
    }
}

fn apply_ordered_filter_events(
    filter: &mut runtime::PartialCloneFilterState,
    filters: &[String],
    no_filter: u8,
    raw_args: &[String],
) -> std::result::Result<(), runtime::CliError> {
    let events = runtime::parse_partial_clone_filter_events(raw_args);
    let filter_events = events
        .iter()
        .filter(|event| matches!(event, runtime::PartialCloneFilterCliEvent::Filter(_)))
        .count();
    let no_filter_events = events
        .iter()
        .filter(|event| matches!(event, runtime::PartialCloneFilterCliEvent::NoFilter))
        .count();
    if filter_events != filters.len() || no_filter_events != usize::from(no_filter) {
        return Err(runtime::CliError::Fatal {
            code: 128,
            message: "partial clone filter arguments were not reconstructed in argv order"
                .to_owned(),
        });
    }
    filter
        .apply_cli_events(events)
        .map_err(partial_clone_filter_error)
}

fn partial_clone_filter_error(error: runtime::PartialCloneFilterError) -> runtime::CliError {
    let detail = match error {
        runtime::PartialCloneFilterError::EmptyFilterSpec => "invalid filter-spec ''".to_owned(),
        runtime::PartialCloneFilterError::EmptyCombineFilterSpec => {
            "expected something after combine:".to_owned()
        }
        runtime::PartialCloneFilterError::ExpectedTreeDepth { .. } => {
            "expected 'tree:<depth>'".to_owned()
        }
        runtime::PartialCloneFilterError::SparsePathFiltersDropped { .. } => {
            "sparse:path filters support has been dropped".to_owned()
        }
        runtime::PartialCloneFilterError::ReservedCombineCharacter { raw } => {
            let character = raw
                .chars()
                .find(|character| {
                    !character.is_ascii()
                        || character.is_ascii_whitespace()
                        || matches!(
                            character,
                            '~' | '`'
                                | '!'
                                | '@'
                                | '#'
                                | '$'
                                | '^'
                                | '&'
                                | '*'
                                | '('
                                | ')'
                                | '['
                                | ']'
                                | '{'
                                | '}'
                                | '\\'
                                | ';'
                                | '\''
                                | '"'
                                | ','
                                | '<'
                                | '>'
                                | '?'
                        )
                })
                .unwrap_or('?');
            let character = if character.is_control() {
                format!("\\x{:02x}", character as u32)
            } else {
                character.to_string()
            };
            format!("must escape char in sub-filter-spec: '{character}'")
        }
        runtime::PartialCloneFilterError::AutoFilterNotAllowed { raw }
        | runtime::PartialCloneFilterError::AutoCannotCombine { raw }
        | runtime::PartialCloneFilterError::InvalidFilterSpec { raw }
        | runtime::PartialCloneFilterError::InvalidPercentEscape { raw }
        | runtime::PartialCloneFilterError::InvalidUtf8FilterSpec { raw }
        | runtime::PartialCloneFilterError::LiteralControlCharacter { raw } => {
            format!(
                "invalid filter-spec '{}'",
                sanitize_partial_clone_filter_error(&raw)
            )
        }
        runtime::PartialCloneFilterError::ResourceLimitExceeded {
            limit,
            actual,
            maximum,
        } => {
            let name = match limit {
                runtime::PartialCloneFilterResourceLimit::FilterCount => "filter count",
                runtime::PartialCloneFilterResourceLimit::RawFilterLength => "filter value length",
                runtime::PartialCloneFilterResourceLimit::CombinedEncodedLength => {
                    "combined filter length"
                }
            };
            format!("{name} exceeds maximum ({actual} > {maximum})")
        }
    };
    runtime::CliError::Stderr {
        code: 128,
        text: format!("fatal: {detail}\n"),
    }
}

fn sanitize_partial_clone_filter_error(raw: &str) -> String {
    let mut sanitized = String::with_capacity(raw.len());
    for character in raw.chars() {
        if character.is_control() {
            use std::fmt::Write as _;
            let _ = write!(sanitized, "\\x{:02x}", character as u32);
        } else {
            sanitized.push(character);
        }
    }
    sanitized
}

fn strip_partial_clone_filter_args(raw_args: &[String]) -> Vec<String> {
    let mut stripped = Vec::with_capacity(raw_args.len());
    let mut index = 0;
    while index < raw_args.len() {
        let argument = &raw_args[index];
        if argument == "--" {
            stripped.push(argument.clone());
            index += 1;
            while index < raw_args.len() {
                let literal = &raw_args[index];
                // Clone/fetch positional values are already carried by the
                // typed command input.  Do not let a positional value that
                // happens to look like --filter override the named state in
                // the legacy transport resolver.
                if literal == "--filter" {
                    index += 2;
                    continue;
                }
                if literal.starts_with("--filter=") {
                    index += 1;
                    continue;
                }
                stripped.push(literal.clone());
                index += 1;
            }
            break;
        }
        if argument == "--filter" {
            index += 2;
            continue;
        }
        if argument.starts_with("--filter=") || argument == "--no-filter" {
            index += 1;
            continue;
        }
        stripped.push(argument.clone());
        index += 1;
    }
    stripped
}

pub(crate) fn dispatch(
    command: runtime::Command,
    raw_args: &[String],
) -> std::result::Result<(), runtime::CliError> {
    match command {
        runtime::Command::Clone {
            quiet,
            verbose: _verbose,
            progress: _progress,
            no_progress: _no_progress,
            bare,
            mirror,
            local: _local,
            no_local,
            no_hardlinks,
            hardlinks,
            reject_shallow,
            no_reject_shallow,
            template,
            no_template,
            configs,
            no_checkout,
            checkout,
            worktree_first,
            instant,
            background_fetch,
            demand_hydrate,
            recurse_submodules,
            recursive,
            no_recurse_submodules,
            jobs,
            shallow_submodules,
            no_shallow_submodules,
            remote_submodules,
            no_remote_submodules,
            origin,
            no_tags,
            tags,
            single_branch,
            no_single_branch,
            separate_git_dir,
            references,
            reference_if_able,
            shared,
            dissociate,
            depth,
            shallow_since,
            shallow_exclude,
            branch,
            server_option,
            upload_pack,
            filter,
            no_filter,
            also_filter_submodules,
            bundle_uri,
            sparse,
            ref_format,
            repository,
            directory,
        } => run_clone(
            super::transport_commands::CloneCommandInput {
                quiet,
                reject_shallow,
                no_reject_shallow,
                template,
                no_template,
                configs,
                no_checkout,
                checkout,
                worktree_first,
                instant,
                background_fetch,
                demand_hydrate,
                recurse_submodules,
                recursive,
                no_recurse_submodules,
                jobs,
                shallow_submodules,
                no_shallow_submodules,
                remote_submodules,
                no_remote_submodules,
                origin,
                no_tags,
                tags,
                single_branch,
                no_single_branch,
                separate_git_dir,
                references,
                reference_if_able,
                shared,
                dissociate,
                no_hardlinks,
                hardlinks,
                no_local,
                depth,
                shallow_since,
                shallow_exclude,
                branch,
                server_option,
                upload_pack,
                filter: None,
                also_filter_submodules: false,
                bundle_uri,
                sparse,
                ref_format,
                repository,
                directory,
                bare,
                mirror,
            },
            filter,
            no_filter,
            also_filter_submodules,
            raw_args,
        ),
        runtime::Command::LsRemote {
            branches,
            branches_short,
            heads,
            tags_short,
            tags,
            refs_only,
            quiet,
            get_url,
            symref,
            exit_code,
            server_option,
            sort,
            upload_pack,
            repository,
            patterns,
        } => run_ls_remote(
            heads || branches || branches_short,
            tags || tags_short,
            refs_only,
            quiet,
            get_url,
            symref,
            exit_code,
            server_option,
            sort,
            upload_pack,
            repository,
            patterns,
        ),
        runtime::Command::Fetch {
            all,
            no_all: _,
            multiple,
            prefetch,
            quiet,
            verbose,
            progress: _,
            dry_run,
            force,
            auto_gc: _,
            auto_maintenance: _,
            no_auto_gc: _,
            no_auto_maintenance: _,
            set_upstream,
            append,
            prune,
            no_prune,
            prune_tags,
            no_tags,
            tags,
            atomic,
            keep: _,
            ipv4: _,
            ipv6: _,
            recurse_submodules: _,
            no_recurse_submodules: _,
            jobs: _,
            update_head_ok,
            write_fetch_head,
            no_write_fetch_head,
            write_commit_graph: _,
            no_write_commit_graph: _,
            refmap,
            depth,
            deepen: _,
            unshallow,
            update_shallow,
            shallow_since: _,
            shallow_exclude: _,
            negotiation_tip,
            negotiate_only,
            server_option: _,
            show_forced_updates: _,
            no_show_forced_updates: _,
            upload_pack: _,
            filter,
            no_filter,
            stdin,
            porcelain,
            recurse_submodules_default: _,
            refetch,
            submodule_prefix: _,
            remote,
            branch: refspecs,
        } => run_fetch(
            all,
            multiple,
            prefetch,
            quiet,
            verbose,
            dry_run,
            force,
            set_upstream,
            append,
            prune,
            no_prune,
            prune_tags,
            no_tags,
            tags,
            atomic,
            update_head_ok,
            !no_write_fetch_head || write_fetch_head,
            refmap,
            depth,
            unshallow,
            update_shallow,
            negotiation_tip,
            negotiate_only,
            FetchFilterCommandOptions::from_cli_values(filter, no_filter, refetch, raw_args)?,
            stdin,
            porcelain,
            remote,
            refspecs,
            raw_args,
        ),
        runtime::Command::Pull {
            all,
            no_all,
            verbose: _,
            dry_run,
            force,
            prune,
            no_tags,
            tags,
            jobs: _,
            keep: _,
            ipv4: _,
            ipv6: _,
            recurse_submodules: _,
            no_recurse_submodules: _,
            server_option: _,
            show_forced_updates: _,
            no_show_forced_updates: _,
            set_upstream,
            append,
            ff,
            ff_only,
            no_ff,
            stat,
            no_stat,
            summary,
            no_summary,
            commit,
            no_commit,
            log,
            no_log,
            squash,
            no_squash,
            edit,
            no_edit,
            autostash,
            no_autostash,
            cleanup,
            signoff,
            no_signoff,
            gpg_sign,
            no_gpg_sign,
            verify,
            no_verify,
            verify_signatures,
            no_verify_signatures,
            quiet,
            progress,
            no_progress,
            allow_unrelated_histories,
            strategies,
            strategy_options,
            rebase,
            no_rebase,
            refmap,
            depth,
            deepen,
            unshallow,
            update_shallow,
            shallow_since,
            shallow_exclude,
            negotiation_tip,
            upload_pack,
            remote,
            branch,
        } => run_pull(
            all,
            dry_run,
            force,
            ff,
            ff_only,
            no_ff,
            stat,
            no_stat,
            summary,
            no_summary,
            commit,
            no_commit,
            log,
            no_log,
            squash,
            no_squash,
            edit,
            no_edit,
            autostash,
            no_autostash,
            cleanup,
            signoff,
            no_signoff,
            gpg_sign,
            no_gpg_sign,
            verify,
            no_verify,
            verify_signatures,
            no_verify_signatures,
            quiet,
            progress,
            no_progress,
            allow_unrelated_histories,
            no_all,
            set_upstream,
            append,
            prune,
            no_tags,
            tags,
            strategies,
            strategy_options,
            rebase,
            no_rebase,
            refmap,
            depth,
            deepen,
            unshallow,
            update_shallow,
            shallow_since,
            shallow_exclude,
            negotiation_tip,
            upload_pack,
            remote,
            branch,
            raw_args,
        ),
        runtime::Command::Push {
            force,
            set_upstream,
            remote,
            refspecs,
        } => run_push(force, set_upstream, remote, refspecs),
        runtime::Command::Daemon { options } => {
            super::transport_commands::daemon(super::transport_commands::DaemonOptions {
                verbose: options.verbose,
                syslog: options.syslog,
                export_all: options.export_all,
                timeout: options.timeout,
                init_timeout: options.init_timeout,
                max_connections: options.max_connections,
                strict_paths: options.strict_paths,
                base_path: options.base_path,
                base_path_relaxed: options.base_path_relaxed,
                reuseaddr: options.reuseaddr,
                pid_file: options.pid_file,
                access_hook: options.access_hook,
                detach: options.detach,
                group: options.group,
                enable: options.enable,
                disable: options.disable,
                allow_override: options.allow_override,
                forbid_override: options.forbid_override,
                informative_errors: options.informative_errors,
                no_informative_errors: options.no_informative_errors,
                log_destination: options.log_destination,
                interpolated_path: options.interpolated_path,
                inetd: options.inetd,
                listen: options.listen,
                port: options.port,
                user: options.user,
                user_path: options.user_path,
                directories: options.directories,
            })
        }
        runtime::Command::UploadPack {
            strict,
            no_strict,
            stateless_rpc,
            http_backend_info_refs,
            advertise_refs,
            timeout,
            directory,
        } => super::transport_commands::upload_pack(super::transport_commands::UploadPackOptions {
            strict,
            no_strict,
            stateless_rpc,
            advertise_refs: advertise_refs || http_backend_info_refs,
            timeout,
            directory,
        }),
        runtime::Command::HttpFetch {
            commit,
            tags,
            all,
            verbose,
            recover,
            write_ref,
            stdin,
            packfile,
            index_pack_args,
            index_pack_args_plural: _,
            args,
        } => super::transport_commands::http_fetch(super::transport_commands::HttpFetchOptions {
            commit,
            tags,
            all,
            verbose,
            recover,
            write_ref,
            stdin,
            packfile,
            index_pack_args,
            args,
        }),
        runtime::Command::HttpPush {
            all,
            dry_run,
            delete,
            force_delete,
            force,
            verbose,
            remote,
            heads,
        } => super::transport_commands::http_push(super::transport_commands::HttpPushOptions {
            all,
            dry_run,
            delete,
            force_delete,
            force,
            verbose,
            remote,
            heads,
        }),
        runtime::Command::FetchPack {
            all,
            stdin,
            quiet,
            keep,
            thin,
            include_tag,
            exec,
            upload_pack,
            depth,
            shallow_since,
            shallow_exclude,
            deepen_relative,
            refetch,
            check_self_contained_and_connected,
            no_progress,
            diag_url,
            verbose,
            directory,
            refs,
        } => super::transport_commands::fetch_pack(super::transport_commands::FetchPackOptions {
            all,
            stdin,
            quiet,
            keep,
            thin,
            include_tag,
            exec,
            upload_pack,
            depth,
            shallow_since,
            shallow_exclude,
            deepen_relative,
            refetch,
            check_self_contained_and_connected,
            no_progress,
            diag_url,
            verbose,
            directory,
            refs,
        }),
        runtime::Command::SendPack {
            mirror,
            dry_run,
            force,
            receive_pack,
            exec,
            verbose,
            thin,
            atomic,
            signed,
            no_signed,
            push_option,
            all,
            stdin,
            directory,
            refs,
        } => super::transport_commands::send_pack(super::transport_commands::SendPackOptions {
            mirror,
            dry_run,
            force,
            receive_pack,
            exec,
            verbose,
            thin,
            atomic,
            signed,
            no_signed,
            push_option,
            all,
            stdin,
            directory,
            refs,
        }),
        runtime::Command::HttpBackend => super::transport_commands::http_backend(),
        runtime::Command::ReceivePack {
            http_backend_info_refs,
            quiet,
            directory,
        } => super::transport_commands::receive_pack(http_backend_info_refs, quiet, directory),

        runtime::Command::Shell { command, args } => {
            super::transport_commands::shell(command, args)
        }
        _ => unreachable!("non-transport command dispatched to transport"),
    }
}

pub(crate) fn run_clone(
    mut input: super::transport_commands::CloneCommandInput,
    filters: Vec<String>,
    no_filter: u8,
    also_filter_submodules: bool,
    raw_args: &[String],
) -> std::result::Result<(), runtime::CliError> {
    let filter_options = CloneFilterCommandOptions::from_cli_values(
        filters,
        no_filter,
        also_filter_submodules,
        raw_args,
    )?;
    validate_clone_filter_submodules(&filter_options, &input.recurse_submodules, &input.recursive)?;
    if filter_options.also_filter_submodules {
        input.configs.push(format!(
            "{}=true",
            runtime::CLONE_SUBMODULE_FILTER_CONFIG_KEY
        ));
    }
    input.filter = filter_options.transport_filter();
    input.also_filter_submodules = filter_options.also_filter_submodules;
    let transport_args = strip_partial_clone_filter_args(raw_args);
    super::transport_commands::run_clone(input, &transport_args)
}

pub(crate) fn run_ls_remote(
    heads: bool,
    tags: bool,
    refs_only: bool,
    quiet: bool,
    get_url: bool,
    symref: bool,
    exit_code: bool,
    server_option: Vec<String>,
    sort: Option<String>,
    upload_pack: Option<String>,
    repository: Option<String>,
    patterns: Vec<String>,
) -> std::result::Result<(), runtime::CliError> {
    super::transport_commands::run_ls_remote(
        heads,
        tags,
        refs_only,
        quiet,
        get_url,
        symref,
        exit_code,
        server_option,
        sort,
        upload_pack,
        repository,
        patterns,
    )
}

pub(crate) fn run_fetch(
    all: bool,
    multiple: bool,
    prefetch: bool,
    quiet: bool,
    verbose: bool,
    dry_run: bool,
    force: bool,
    set_upstream: bool,
    append: bool,
    prune: bool,
    no_prune: bool,
    prune_tags: bool,
    no_tags: bool,
    tags: bool,
    atomic: bool,
    update_head_ok: bool,
    write_fetch_head: bool,
    refmap: Vec<String>,
    depth: Option<String>,
    unshallow: bool,
    update_shallow: bool,
    negotiation_tip: Vec<String>,
    negotiate_only: bool,
    filter_options: FetchFilterCommandOptions,
    stdin: bool,
    porcelain: bool,
    remote: Option<String>,
    refspecs: Vec<String>,
    raw_args: &[String],
) -> std::result::Result<(), runtime::CliError> {
    let transport_args = strip_partial_clone_filter_args(raw_args);
    let transport_options = FetchTransportCommandOptions::from(&filter_options);
    ensure_fetch_refetch_is_preserved(&transport_options, &transport_args)?;
    super::transport_commands::run_fetch(
        all,
        multiple,
        prefetch,
        quiet,
        verbose,
        dry_run,
        force,
        set_upstream,
        append,
        prune,
        no_prune,
        prune_tags,
        no_tags,
        tags,
        atomic,
        update_head_ok,
        write_fetch_head,
        refmap,
        depth,
        unshallow,
        update_shallow,
        negotiation_tip,
        negotiate_only,
        transport_options.filter,
        transport_options.no_filter_requested,
        transport_options.no_haves_requested,
        stdin,
        porcelain,
        remote,
        refspecs,
        &transport_args,
    )
}

pub(crate) fn run_pull(
    all: bool,
    dry_run: bool,
    force: bool,
    ff: bool,
    ff_only: bool,
    no_ff: bool,
    stat: bool,
    no_stat: bool,
    summary: bool,
    no_summary: bool,
    commit: bool,
    no_commit: bool,
    log: Option<String>,
    no_log: bool,
    squash: bool,
    no_squash: bool,
    edit: u8,
    no_edit: u8,
    autostash: bool,
    no_autostash: bool,
    cleanup: Option<String>,
    signoff: u8,
    no_signoff: u8,
    gpg_sign: Option<String>,
    no_gpg_sign: u8,
    verify: u8,
    no_verify: u8,
    verify_signatures: u8,
    no_verify_signatures: u8,
    quiet: bool,
    progress: u8,
    no_progress: u8,
    allow_unrelated_histories: bool,
    no_all: bool,
    set_upstream: bool,
    append: bool,
    prune: bool,
    no_tags: bool,
    tags: bool,
    strategies: Vec<String>,
    strategy_options: Vec<String>,
    rebase_mode: Option<String>,
    no_rebase: bool,
    refmap: Vec<String>,
    depth: Option<String>,
    deepen: Option<String>,
    unshallow: bool,
    update_shallow: bool,
    shallow_since: Option<String>,
    shallow_exclude: Vec<String>,
    negotiation_tip: Vec<String>,
    upload_pack: Option<String>,
    remote: Option<String>,
    branch: Option<String>,
    raw_args: &[String],
) -> std::result::Result<(), runtime::CliError> {
    super::transport_commands::run_pull(
        all,
        dry_run,
        force,
        ff,
        ff_only,
        no_ff,
        stat,
        no_stat,
        summary,
        no_summary,
        commit,
        no_commit,
        log,
        no_log,
        squash,
        no_squash,
        edit,
        no_edit,
        autostash,
        no_autostash,
        cleanup,
        signoff,
        no_signoff,
        gpg_sign,
        no_gpg_sign,
        verify,
        no_verify,
        verify_signatures,
        no_verify_signatures,
        quiet,
        progress,
        no_progress,
        allow_unrelated_histories,
        no_all,
        set_upstream,
        append,
        prune,
        no_tags,
        tags,
        strategies,
        strategy_options,
        rebase_mode,
        no_rebase,
        refmap,
        depth,
        deepen,
        unshallow,
        update_shallow,
        shallow_since,
        shallow_exclude,
        negotiation_tip,
        upload_pack,
        remote,
        branch,
        raw_args,
    )
}

pub(crate) fn run_push(
    force: bool,
    set_upstream: bool,
    remote: Option<String>,
    refspecs: Vec<String>,
) -> std::result::Result<(), runtime::CliError> {
    super::transport_commands::run_push(force, set_upstream, remote, refspecs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clone_dispatch_state_preserves_filter_order_and_combines() {
        let options = CloneFilterCommandOptions::from_cli_values(
            vec!["blob:none".to_owned(), "tree:0".to_owned()],
            0,
            false,
            &[
                "clone".to_owned(),
                "--filter=blob:none".to_owned(),
                "--filter=tree:0".to_owned(),
            ],
        )
        .expect("valid clone filters");
        assert_eq!(
            options.transport_filter().as_deref(),
            Some("combine:blob:none+tree:0")
        );
    }

    #[test]
    fn clone_dispatch_state_rejects_reserved_and_invalid_utf8_filter_components() {
        for raw in ["combine:blob:none~", "combine:tree:%ff"] {
            let error = CloneFilterCommandOptions::from_cli_values(
                vec![raw.to_owned()],
                0,
                false,
                &["clone".to_owned(), format!("--filter={raw}")],
            )
            .expect_err("malformed filter should fail before transport");
            assert!(matches!(error, runtime::CliError::Stderr { code: 128, .. }));
        }
    }

    #[test]
    fn clone_dispatch_state_resets_on_no_filter_when_it_is_last() {
        let options = CloneFilterCommandOptions::from_cli_values(
            vec!["blob:none".to_owned()],
            1,
            false,
            &[
                "clone".to_owned(),
                "--filter=blob:none".to_owned(),
                "--no-filter".to_owned(),
            ],
        )
        .expect("no-filter should be accepted");
        assert!(options.filter.no_filter_requested());
        assert!(options.transport_filter().is_none());
        assert!(!options.filter.inherits_configured_filter());
    }

    #[test]
    fn clone_and_fetch_preserve_auto_boundary_and_no_filter_state() {
        let clone = CloneFilterCommandOptions::from_cli_values(
            vec!["auto".to_owned()],
            0,
            false,
            &["clone".to_owned(), "--filter=auto".to_owned()],
        )
        .expect("clone auto is a transport negotiation intent");
        assert!(clone.filter.selection().is_auto());
        assert_eq!(clone.transport_filter().as_deref(), Some("auto"));

        let fetch = FetchFilterCommandOptions::from_cli_values(
            vec!["auto".to_owned()],
            0,
            false,
            &["fetch".to_owned(), "--filter=auto".to_owned()],
        )
        .expect("fetch auto is a transport negotiation intent");
        let fetch_boundary = FetchTransportCommandOptions::from(&fetch);
        assert_eq!(fetch_boundary.filter.as_deref(), Some("auto"));
        assert!(!fetch_boundary.no_filter_requested);

        let no_recommendation = FetchFilterCommandOptions::from_cli_values(
            vec!["auto".to_owned()],
            1,
            false,
            &[
                "fetch".to_owned(),
                "--filter=auto".to_owned(),
                "--no-filter".to_owned(),
            ],
        )
        .expect("a later no-filter suppresses the auto request");
        let no_recommendation_boundary = FetchTransportCommandOptions::from(&no_recommendation);
        assert!(no_recommendation_boundary.filter.is_none());
        assert!(no_recommendation_boundary.no_filter_requested);

        let mut other = runtime::PartialCloneFilterState::for_context(
            runtime::PartialCloneFilterCommandContext::Other,
        );
        assert!(matches!(
            other.apply(runtime::PartialCloneFilterAction::filter("auto".to_owned())),
            Err(runtime::PartialCloneFilterError::AutoFilterNotAllowed { .. })
        ));
    }

    #[test]
    fn fetch_dispatch_state_retains_refetch_no_haves_intent() {
        let options = FetchFilterCommandOptions::from_cli_values(
            vec!["blob:none".to_owned()],
            0,
            true,
            &[
                "fetch".to_owned(),
                "--filter=blob:none".to_owned(),
                "--refetch".to_owned(),
            ],
        )
        .expect("valid fetch options");
        assert!(options.no_haves_requested());
        assert_eq!(options.transport_filter().as_deref(), Some("blob:none"));
        let transport_options = FetchTransportCommandOptions::from(&options);
        assert!(transport_options.no_haves_requested);
        ensure_fetch_refetch_is_preserved(
            &transport_options,
            &[
                "fetch".to_owned(),
                "--filter=blob:none".to_owned(),
                "--refetch".to_owned(),
            ],
        )
        .expect("refetch intent remains on the transport boundary");
    }

    #[test]
    fn fetch_no_filter_explicitly_suppresses_configured_filter_in_state() {
        let options = FetchFilterCommandOptions::from_cli_values(
            Vec::new(),
            1,
            true,
            &[
                "fetch".to_owned(),
                "--refetch".to_owned(),
                "--no-filter".to_owned(),
            ],
        )
        .expect("no-filter should be accepted");
        assert!(options.filter.no_filter_requested());
        assert!(!options.filter.inherits_configured_filter());
        assert!(options.transport_filter().is_none());
    }

    #[test]
    fn transport_filter_args_are_removed_without_touching_after_separator_values() {
        let raw_args = vec![
            "clone".to_owned(),
            "--filter=blob:none".to_owned(),
            "--filter".to_owned(),
            "tree:0".to_owned(),
            "--no-filter".to_owned(),
            "--".to_owned(),
            "--filter=path-is-a-directory".to_owned(),
        ];
        assert_eq!(
            strip_partial_clone_filter_args(&raw_args),
            vec!["clone".to_owned(), "--".to_owned(),]
        );
    }

    #[test]
    fn filter_and_no_filter_follow_argv_order_and_post_reset_filters_combine() {
        let reset_last = CloneFilterCommandOptions::from_cli_values(
            vec!["blob:none".to_owned()],
            1,
            false,
            &[
                "clone".to_owned(),
                "--filter=blob:none".to_owned(),
                "--no-filter".to_owned(),
            ],
        )
        .expect("valid filter sequence");
        assert!(reset_last.transport_filter().is_none());
        assert!(!reset_last.filter.inherits_configured_filter());

        let filter_last = CloneFilterCommandOptions::from_cli_values(
            vec!["blob:none".to_owned()],
            1,
            false,
            &[
                "clone".to_owned(),
                "--no-filter".to_owned(),
                "--filter=blob:none".to_owned(),
            ],
        )
        .expect("valid filter sequence");
        assert_eq!(filter_last.transport_filter().as_deref(), Some("blob:none"));

        let combined = CloneFilterCommandOptions::from_cli_values(
            vec!["tree:0".to_owned(), "blob:none".to_owned()],
            1,
            false,
            &[
                "clone".to_owned(),
                "--no-filter".to_owned(),
                "--filter=tree:0".to_owned(),
                "--filter=blob:none".to_owned(),
            ],
        )
        .expect("valid filter sequence");
        assert_eq!(
            combined.transport_filter().as_deref(),
            Some("combine:tree:0+blob:none")
        );
    }

    #[test]
    fn filter_event_parser_stops_at_separator() {
        let options = CloneFilterCommandOptions::from_cli_values(
            vec!["blob:none".to_owned()],
            0,
            false,
            &[
                "clone".to_owned(),
                "--filter=blob:none".to_owned(),
                "--".to_owned(),
                "--filter=auto".to_owned(),
            ],
        )
        .expect("post-separator path is literal");
        assert_eq!(options.transport_filter().as_deref(), Some("blob:none"));
    }

    #[test]
    fn also_filter_submodules_validates_before_transport() {
        let no_filter = CloneFilterCommandOptions::from_cli_values(
            Vec::new(),
            1,
            true,
            &["clone".to_owned(), "--no-filter".to_owned()],
        )
        .expect("no-filter is valid by itself");
        let error = validate_clone_filter_submodules(&no_filter, &[], &["yes".to_owned()])
            .expect_err("submodule filter requires a filter");
        assert!(matches!(error, runtime::CliError::Stderr { code: 128, .. }));

        let filter = CloneFilterCommandOptions::from_cli_values(
            vec!["blob:none".to_owned()],
            0,
            true,
            &["clone".to_owned(), "--filter=blob:none".to_owned()],
        )
        .expect("valid filter");
        let error = validate_clone_filter_submodules(&filter, &[], &[])
            .expect_err("submodule filter requires recursion");
        assert!(matches!(error, runtime::CliError::Stderr { code: 128, .. }));
        validate_clone_filter_submodules(&filter, &["yes".to_owned()], &[])
            .expect("filtering recursive submodules is supported");

        let error = validate_clone_filter_submodules(&filter, &[], &[])
            .expect_err("submodule filter requires recursion");
        assert_eq!(
            error_text(error),
            "fatal: the option '--also-filter-submodules' requires '--recurse-submodules'\n"
        );
    }

    fn error_text(error: runtime::CliError) -> String {
        match error {
            runtime::CliError::Stderr { text, .. } => text,
            other => panic!("expected stderr error, got {other:?}"),
        }
    }

    #[test]
    fn hostile_filter_count_is_bounded_before_state_expansion() {
        let mut raw_args = vec!["clone".to_owned()];
        let mut values = Vec::new();
        for index in 0..=runtime::PARTIAL_CLONE_FILTER_MAX_COUNT {
            let value = if index == 0 { "blob:none" } else { "tree:0" };
            values.push(value.to_owned());
            raw_args.push(format!("--filter={value}"));
        }
        let error = CloneFilterCommandOptions::from_cli_values(values, 0, false, &raw_args)
            .expect_err("filter count should be bounded");
        assert!(matches!(error, runtime::CliError::Stderr { code: 128, .. }));
    }
}
