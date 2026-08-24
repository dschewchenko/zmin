use super::lfs_reachability_adapter::{CliLfsReachabilityRepository, CliLfsRemoteSelection};
use super::*;
use std::time::Duration;

const LFS_ATTR_SUFFIX: &str = " filter=lfs diff=lfs merge=lfs -text";
const LFS_PRE_PUSH_MARKER: &str = "# zmin-lfs-pre-push";
const LFS_POST_CHECKOUT_MARKER: &str = "# zmin-lfs-post-checkout";
const LFS_POST_COMMIT_MARKER: &str = "# zmin-lfs-post-commit";
const LFS_POST_MERGE_MARKER: &str = "# zmin-lfs-post-merge";
const LFS_ATTRIBUTES_MAX_BYTES: u64 = 1024 * 1024;
const LFS_CONFIG_SNAPSHOT_MAX_BYTES: usize = 64 * 1024;

pub(crate) fn lfs_command(args: Vec<String>) -> Result<()> {
    let Some(subcommand) = args.first().map(String::as_str) else {
        print!("{}", lfs_usage());
        return Ok(());
    };
    match subcommand {
        "version" => lfs_version(),
        "env" => lfs_env(),
        "clean" => lfs_clean(&args[1..]),
        "smudge" => lfs_smudge(&args[1..]),
        "filter-process" => lfs_filter_process(&args[1..]),
        "pointer" => lfs_pointer_command(&args[1..]),
        "fetch" => lfs_fetch(&args[1..]),
        "pull" => lfs_pull(&args[1..]),
        "push" => lfs_push(&args[1..]),
        "install" => lfs_install(&args[1..]),
        "update" => lfs_update(&args[1..]),
        "checkout" => lfs_checkout(&args[1..]),
        "track" => lfs_track(&args[1..]),
        "untrack" => lfs_untrack(&args[1..]),
        "ls-files" => lfs_ls_files(&args[1..]),
        "pre-push" => lfs_pre_push(&args[1..]),
        "post-checkout" => lfs_post_checkout(&args[1..]),
        "post-commit" => lfs_post_commit(&args[1..]),
        "post-merge" => lfs_post_merge(&args[1..]),
        _ => Err(CliError::Stderr {
            code: 1,
            text: format!(
                "Error: unknown command \"{subcommand}\" for \"git-lfs\"\nRun 'git lfs --help' for usage.\n"
            ),
        }),
    }
}

fn lfs_usage() -> String {
    "git lfs <command> [<args>]\n\nBuilt-in Zmin Git LFS local foundation commands:\n\n\
git lfs env\n\
git lfs clean [--] <path>\n\
git lfs smudge [--skip] [--] <path>\n\
git lfs filter-process [--skip]\n\
git lfs pointer --check [--strict] [--stdin|--file <path>]\n\
git lfs checkout [<path>...]\n\
git lfs fetch [<remote> [<ref>...]]\n\
git lfs install [--global|--local|--worktree|--system] [--force] [--skip-repo] [--skip-smudge]\n\
git lfs ls-files [--name-only] [--long] [--size] [<ref> [<ref>]]\n\
git lfs pull [<remote>]\n\
git lfs push <remote> [<ref>...]\n\
git lfs post-checkout [old] [new] [flag]\n\
git lfs post-commit\n\
git lfs post-merge [flag]\n\
git lfs pre-push <remote> [remoteurl]\n\
git lfs track <pattern>...\n\
git lfs untrack <pattern>...\n\
git lfs update [--manual | --force]\n\
git lfs version\n"
        .into()
}

fn lfs_version() -> Result<()> {
    println!(
        "git-lfs/zmin (zmin {}; built-in local foundation)",
        env!("CARGO_PKG_VERSION")
    );
    Ok(())
}

fn lfs_env() -> Result<()> {
    let repo = find_repo()?;
    let config = lfs_runtime_config(&repo)?;
    let common_git_dir = lfs_common_git_dir(&repo)?;
    println!(
        "git-lfs/zmin (zmin {}; built-in local foundation)",
        env!("CARGO_PKG_VERSION")
    );
    println!("git version {}", crate::runtime::GIT_COMPAT_VERSION);
    println!();
    let root = repo.root.display();
    let git_dir = repo.git_dir.display();
    let lfs_dir = config.storage();
    let media_dir = lfs_dir.join("objects");
    let temp_dir = lfs_dir.join("tmp");
    println!("LocalWorkingDir={root}");
    println!("LocalGitDir={git_dir}");
    println!("LocalGitStorageDir={}", common_git_dir.display());
    println!("LocalMediaDir={}", media_dir.display());
    println!("TempDir={}", temp_dir.display());
    println!(
        "ConcurrentTransfers={}",
        config.concurrent_transfers().get()
    );
    for name in [
        "lfs.repositoryformatversion",
        "filter.lfs.process",
        "filter.lfs.smudge",
        "filter.lfs.clean",
        "filter.lfs.required",
    ] {
        if let Some(value) = read_config_value(&repo, name).map_err(CliError::Io)? {
            println!("git config {name} = {value}");
        }
    }
    Ok(())
}

fn lfs_clean(args: &[String]) -> Result<()> {
    let pathname = parse_lfs_filter_path(args, false)?;
    let mut engine = lfs_filter_engine(false, false)?;
    let stdin = io::stdin();
    let mut input = stdin.lock();
    let stdout = io::stdout();
    let mut output = io::BufWriter::new(stdout.lock());
    engine
        .clean(&pathname, &mut input, &mut output)
        .map_err(lfs_filter_error)?;
    output.flush().map_err(CliError::Io)
}

fn lfs_smudge(args: &[String]) -> Result<()> {
    let pathname = parse_lfs_filter_path(args, true)?;
    let skip = args.iter().any(|arg| arg == "--skip");
    let mut engine = lfs_filter_engine(skip, true)?;
    let stdin = io::stdin();
    let mut input = stdin.lock();
    let stdout = io::stdout();
    let mut output = io::BufWriter::new(stdout.lock());
    engine
        .smudge(&pathname, &mut input, &mut output)
        .map_err(lfs_filter_error)?;
    output.flush().map_err(CliError::Io)
}

fn lfs_filter_process(args: &[String]) -> Result<()> {
    let skip = parse_lfs_filter_process_options(args)?;
    let mut engine = lfs_filter_engine(skip, true)?;
    let stdin = io::stdin();
    let stdout = io::stdout();
    engine
        .serve(stdin.lock(), stdout.lock())
        .map_err(lfs_filter_error)
}

fn parse_lfs_filter_path(args: &[String], allow_skip: bool) -> Result<String> {
    let mut pathname = None;
    let mut after_separator = false;
    for arg in args {
        if !after_separator && arg == "--" {
            after_separator = true;
            continue;
        }
        if !after_separator && allow_skip && arg == "--skip" {
            continue;
        }
        if !after_separator && arg.starts_with('-') {
            return Err(CliError::Stderr {
                code: 1,
                text: format!("error: unknown option '{arg}'\n"),
            });
        }
        if pathname.replace(arg.clone()).is_some() {
            return Err(CliError::Stderr {
                code: 1,
                text: "error: expected exactly one pathname\n".into(),
            });
        }
    }
    pathname.ok_or_else(|| CliError::Stderr {
        code: 1,
        text: "error: a pathname is required\n".into(),
    })
}

fn parse_lfs_filter_process_options(args: &[String]) -> Result<bool> {
    let mut skip = false;
    for arg in args {
        match arg.as_str() {
            "--skip" => skip = true,
            _ => {
                return Err(CliError::Stderr {
                    code: 1,
                    text: format!("error: unknown option '{arg}'\n"),
                });
            }
        }
    }
    Ok(skip)
}

fn lfs_pointer_command(args: &[String]) -> Result<()> {
    let mut check = false;
    let mut strict = false;
    let mut stdin_input = false;
    let mut file_input = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--check" => check = true,
            "--strict" => strict = true,
            "--stdin" => stdin_input = true,
            "--file" => {
                index += 1;
                let Some(path) = args.get(index) else {
                    return Err(CliError::Stderr {
                        code: 1,
                        text: "error: --file requires a pathname\n".into(),
                    });
                };
                file_input = Some(PathBuf::from(path));
            }
            value => {
                return Err(CliError::Stderr {
                    code: 1,
                    text: format!("error: unknown option '{value}'\n"),
                });
            }
        }
        index += 1;
    }
    if !check {
        return Err(CliError::Stderr {
            code: 1,
            text: "error: pointer requires --check\n".into(),
        });
    }
    if stdin_input && file_input.is_some() {
        return Err(CliError::Stderr {
            code: 1,
            text: "error: --stdin and --file are mutually exclusive\n".into(),
        });
    }

    let mut bytes = Vec::new();
    if let Some(path) = file_input {
        let file = fs::File::open(path).map_err(CliError::Io)?;
        file.take((LFS_POINTER_MAX_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(CliError::Io)?;
    } else {
        io::stdin()
            .lock()
            .take((LFS_POINTER_MAX_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(CliError::Io)?;
    }
    if bytes.len() > LFS_POINTER_MAX_BYTES {
        return Err(CliError::Exit(1));
    }
    let parsed = if strict {
        LfsPointer::parse_strict(&bytes)
    } else {
        LfsPointer::parse_current(&bytes)
    };
    parsed.map(|_| ()).map_err(|_| CliError::Exit(1))
}

fn lfs_filter_engine(skip_flag: bool, enable_download: bool) -> Result<LfsFilterEngine> {
    let repo = find_repo()?;
    let config = lfs_runtime_config(&repo)?;
    let policy = LfsPathPolicy::from_fetch_filter(
        skip_flag || config.skip_smudge(),
        config.fetch_filter().clone(),
    );
    let skip = skip_flag || config.skip_smudge();
    let store = Arc::new(LfsStore::new(config.storage().join("objects")).map_err(lfs_store_error)?);
    let engine = LfsFilterEngine::new(Arc::clone(&store), policy);
    if skip || !enable_download {
        return Ok(engine);
    }
    let skip_download_errors = config.skip_download_errors();
    Ok(engine.with_missing_handler(CliLfsMissingObjectHandler {
        repo,
        store,
        config: Some(config),
        source: None,
        remote: None,
        batch_reference: None,
        skip_download_errors,
    }))
}

fn lfs_runtime_config(repo: &GitRepo) -> Result<LfsRuntimeConfig> {
    let entries = read_config_entries(repo).map_err(CliError::Io)?;
    let algorithm = repo_hash_algorithm_from_config(repo).map_err(CliError::Io)?;
    let object_store = LooseObjectStore::new(repo.objects_dir.clone(), algorithm);
    let index = if repo.index_path.is_file() {
        Some(read_index_with_algorithm(&repo.index_path, algorithm).map_err(CliError::Io)?)
    } else {
        None
    };
    let head_index = read_head_index(repo)?;
    let index_blob = index
        .as_ref()
        .map(|value| lfs_config_blob_from_index(value, &object_store))
        .transpose()?
        .flatten();
    let head_blob = lfs_config_blob_from_index(&head_index, &object_store)?;
    let worktree = (!repo_is_bare(repo))
        .then(|| TrustedWorktreeRoot::new(&repo.root))
        .transpose()
        .map_err(lfs_config_error)?;
    let branch = RefStore::new(&repo.git_dir, algorithm);
    let branch = current_branch_ref(&branch)?;
    let skip_smudge = std::env::var("GIT_LFS_SKIP_SMUDGE").ok();
    let skip_download_errors = std::env::var("GIT_LFS_SKIP_DOWNLOAD_ERRORS").ok();
    let http_environment = LfsHttpEnvironmentSnapshot::capture();
    let fetch_head = lfs_read_fetch_head(repo)?;
    let common_git_dir = lfs_common_git_dir(repo)?;
    LfsRuntimeConfig::load(LfsRuntimeConfigInput {
        git_dir: &repo.git_dir,
        default_storage_git_dir: &common_git_dir,
        lfsconfig: LfsConfigSources::new(worktree, index_blob.as_deref(), head_blob.as_deref()),
        entries: &entries,
        branch: branch.as_deref(),
        requested_remote: None,
        skip_smudge: skip_smudge.as_deref(),
        skip_download_errors: skip_download_errors.as_deref(),
        http_environment,
        fetch_head: fetch_head.as_deref(),
        url_rewriter: None,
    })
    .map_err(lfs_config_error)
}

fn lfs_common_git_dir(repo: &GitRepo) -> Result<PathBuf> {
    let common_git_dir = read_common_git_dir(&repo.git_dir)?;
    fs::canonicalize(common_git_dir).map_err(CliError::Io)
}

enum CliLfsTransferSource {
    Local(LfsPullRemote),
    Network(SystemLfsNetworkSession),
    Unavailable,
}

struct CliLfsMissingObjectHandler {
    repo: GitRepo,
    store: Arc<LfsStore>,
    config: Option<LfsRuntimeConfig>,
    source: Option<CliLfsTransferSource>,
    remote: Option<String>,
    batch_reference: Option<LfsBatchRef>,
    skip_download_errors: bool,
}

impl CliLfsMissingObjectHandler {
    fn initialize_source(&mut self) -> LfsFilterResult<()> {
        if self.source.is_some() {
            return Ok(());
        }
        let config = self
            .config
            .as_ref()
            .expect("lazy LFS source retains configuration until initialized");
        let remote = match CliLfsRemoteSelection::select(config, LfsOperation::Fetch, None) {
            CliLfsRemoteSelection::PolicyRequired(_) => {
                return Err(LfsFilterError::UnsupportedRemoteSelectionPolicy(
                    config.remote_policy(),
                ));
            }
            remote => remote,
        };
        let source = lfs_command_transfer_source(
            &self.repo,
            &remote,
            config.clone(),
            Arc::clone(&self.store),
            LfsOperation::Fetch,
        )
        .map_err(|_| LfsFilterError::MissingObject)?;
        self.batch_reference = lfs_smudge_batch_ref(&self.repo, config, &remote)
            .map_err(|_| LfsFilterError::MissingObject)?;
        self.remote = remote.remote_name().map(str::to_owned);
        self.source = Some(source);
        self.config = None;
        Ok(())
    }

    fn fetch_once(&mut self, oid: LfsOid, size: u64) -> LfsFilterResult<LfsMissingObjectOutcome> {
        self.initialize_source()?;
        match self
            .source
            .as_mut()
            .expect("lazy LFS source is initialized")
        {
            CliLfsTransferSource::Local(remote) => lfs_remote_media_path(remote, oid.hex())
                .filter(|path| lfs_is_regular_file(path))
                .ok_or(LfsFilterError::MissingObject)
                .and_then(|path| {
                    fs::File::open(path).map_err(|error| LfsFilterError::Io {
                        operation: LfsFilterIoOperation::Read,
                        kind: error.kind(),
                    })
                })
                .and_then(|file| {
                    self.store
                        .ingest(oid.bytes(), size, file)
                        .map(|_| LfsMissingObjectOutcome::Available)
                        .map_err(LfsFilterError::from)
                }),
            CliLfsTransferSource::Network(session) => {
                let object =
                    LfsTransferObject::new(oid, size).map_err(|_| LfsFilterError::MissingObject)?;
                let mut request = LfsNetworkRequest::new(vec![object]);
                if let Some(remote) = self.remote.as_deref() {
                    request = request.with_remote(remote);
                }
                if let Some(reference) = self.batch_reference.clone() {
                    request = request.with_reference(reference);
                }
                session
                    .download_missing(&request)
                    .map_err(|_| LfsFilterError::MissingObject)
                    .and_then(lfs_filter_network_outcome)
            }
            CliLfsTransferSource::Unavailable => Err(LfsFilterError::MissingObject),
        }
    }
}

impl LfsMissingObjectHandler for CliLfsMissingObjectHandler {
    fn fetch(
        &mut self,
        _context: &LfsFilterContext<'_>,
        oid: [u8; 32],
        size: u64,
        _store: &LfsStore,
    ) -> LfsFilterResult<LfsMissingObjectOutcome> {
        let oid = lfs_oid_from_bytes(oid);
        let result = self.fetch_once(oid, size);
        if self.skip_download_errors && result.is_err() {
            Ok(LfsMissingObjectOutcome::LeavePointer)
        } else {
            result
        }
    }
}

fn lfs_filter_network_outcome(
    outcome: LfsNetworkOutcome,
) -> LfsFilterResult<LfsMissingObjectOutcome> {
    match outcome {
        LfsNetworkOutcome::Transfer(transfer)
            if transfer.disposition() == LfsNetworkDisposition::Complete =>
        {
            Ok(LfsMissingObjectOutcome::Available)
        }
        LfsNetworkOutcome::Transfer(transfer)
            if transfer.disposition() == LfsNetworkDisposition::SkipDownloadErrors =>
        {
            Ok(LfsMissingObjectOutcome::LeavePointer)
        }
        LfsNetworkOutcome::DownloadFailureSkipped(_) => Ok(LfsMissingObjectOutcome::LeavePointer),
        LfsNetworkOutcome::RemoteSelectionRequired(_) | LfsNetworkOutcome::Transfer(_) => {
            Err(LfsFilterError::MissingObject)
        }
    }
}

fn lfs_config_blob_from_index(
    index: &GitIndex,
    store: &LooseObjectStore,
) -> Result<Option<Vec<u8>>> {
    let Some(entry) = index
        .entries()
        .iter()
        .find(|entry| entry.stage == 0 && entry.path.as_slice() == b".lfsconfig")
    else {
        return Ok(None);
    };
    let snapshot = store
        .packed_first()
        .read_object_prefix_or_full(&entry.id, LFS_CONFIG_SNAPSHOT_MAX_BYTES)
        .map_err(CliError::Io)?;
    if snapshot.object.kind != GitObjectKind::Blob {
        return Ok(None);
    }
    if !snapshot.is_complete {
        return Err(CliError::Stderr {
            code: 1,
            text: "error: .lfsconfig is too large\n".into(),
        });
    }
    Ok(Some(snapshot.object.content))
}

fn lfs_read_fetch_head(repo: &GitRepo) -> Result<Option<String>> {
    let path = repo.git_dir.join("FETCH_HEAD");
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(CliError::Io(error)),
    };
    let mut bytes = Vec::new();
    file.take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(CliError::Io)?;
    if bytes.len() > 1024 * 1024 {
        return Err(CliError::Stderr {
            code: 1,
            text: "error: FETCH_HEAD is too large\n".into(),
        });
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| CliError::Stderr {
            code: 1,
            text: "error: FETCH_HEAD is not valid UTF-8\n".into(),
        })
}

fn lfs_config_error(error: LfsConfigError) -> CliError {
    CliError::Stderr {
        code: 1,
        text: format!("error: {error}\n"),
    }
}

fn lfs_reachability_error(error: LfsReachabilityError) -> CliError {
    CliError::Stderr {
        code: 2,
        text: format!("error: {error}\n"),
    }
}

fn lfs_network_session_error(error: LfsNetworkSessionError) -> CliError {
    CliError::Stderr {
        code: 2,
        text: format!("error: {error}\n"),
    }
}

fn lfs_select_remote(
    config: &LfsRuntimeConfig,
    operation: LfsOperation,
    requested: Option<&str>,
) -> Result<CliLfsRemoteSelection> {
    match CliLfsRemoteSelection::select(config, operation, requested) {
        CliLfsRemoteSelection::PolicyRequired(requirement) => Err(CliError::Stderr {
            code: 2,
            text: format!(
                "error: LFS remote policy requires unsupported multi-remote selection (autodetect={}, searchall={})\n",
                requirement.autodetect, requirement.search_all
            ),
        }),
        selection => Ok(selection),
    }
}

fn lfs_validate_explicit_remote(
    repo: &GitRepo,
    config: &LfsRuntimeConfig,
    operation: LfsOperation,
    selection: &CliLfsRemoteSelection,
) -> Result<()> {
    match selection {
        CliLfsRemoteSelection::GlobalEndpoint => Ok(()),
        CliLfsRemoteSelection::Named(remote) => {
            if !lfs_has_endpoint_override(config, remote, operation)
                && resolve_lfs_local_remote(repo, remote)?.is_some()
            {
                return Ok(());
            }
            resolve_lfs_endpoint_for_remote(operation, remote, config.endpoint_inputs())
                .map(|_| ())
                .map_err(|error| CliError::Stderr {
                    code: 2,
                    text: format!("error: {error}\n"),
                })
        }
        CliLfsRemoteSelection::FetchHeadEndpoint | CliLfsRemoteSelection::Missing => {
            Err(CliError::Stderr {
                code: 2,
                text: "error: requested LFS remote is not configured\n".into(),
            })
        }
        CliLfsRemoteSelection::PolicyRequired(_) => Err(CliError::Stderr {
            code: 2,
            text: "error: LFS remote selection requires an explicit remote\n".into(),
        }),
    }
}

fn lfs_validate_pre_push_destination(
    repo: &GitRepo,
    config: &LfsRuntimeConfig,
    selection: &CliLfsRemoteSelection,
    remote_url: Option<&str>,
) -> Result<()> {
    if matches!(selection, CliLfsRemoteSelection::GlobalEndpoint) {
        return Ok(());
    }
    if let CliLfsRemoteSelection::Named(remote) = selection
        && (remote_url.is_none() || lfs_has_endpoint_override(config, remote, LfsOperation::Push))
    {
        return lfs_validate_explicit_remote(repo, config, LfsOperation::Push, selection);
    }
    let Some(remote_url) = remote_url else {
        return lfs_validate_explicit_remote(repo, config, LfsOperation::Push, selection);
    };
    if lfs_remote_git_dir_from_url(remote_url).is_some() {
        return Ok(());
    }

    const VALIDATION_REMOTE: &str = "zmin-explicit-pre-push";
    let mut inputs = config.endpoint_inputs().clone();
    inputs
        .remotes
        .retain(|remote| remote.name != VALIDATION_REMOTE);
    let mut remote = LfsRemoteConfig::named(VALIDATION_REMOTE);
    remote.url = Some(remote_url.to_owned());
    remote.push_url = Some(remote_url.to_owned());
    inputs.remotes.push(remote);
    resolve_lfs_endpoint_for_remote(LfsOperation::Push, VALIDATION_REMOTE, &inputs)
        .map(|_| ())
        .map_err(|error| CliError::Stderr {
            code: 2,
            text: format!("error: {error}\n"),
        })
}

fn lfs_command_transfer_source(
    repo: &GitRepo,
    remote: &CliLfsRemoteSelection,
    config: LfsRuntimeConfig,
    store: Arc<LfsStore>,
    operation: LfsOperation,
) -> Result<CliLfsTransferSource> {
    match remote {
        CliLfsRemoteSelection::Named(remote) => {
            if !lfs_has_endpoint_override(&config, remote, operation)
                && let Some(local) = resolve_lfs_local_remote(repo, remote)?
            {
                return Ok(CliLfsTransferSource::Local(local));
            }
        }
        CliLfsRemoteSelection::GlobalEndpoint | CliLfsRemoteSelection::FetchHeadEndpoint => {}
        CliLfsRemoteSelection::PolicyRequired(_) | CliLfsRemoteSelection::Missing => {
            return Ok(CliLfsTransferSource::Unavailable);
        }
    }
    Ok(CliLfsTransferSource::Network(lfs_system_network_session(
        config, store,
    )?))
}

fn lfs_system_network_session(
    config: LfsRuntimeConfig,
    store: Arc<LfsStore>,
) -> Result<SystemLfsNetworkSession> {
    let http_policy = Arc::clone(config.http_policy());
    let transfer_config = LfsTransferConfig::from_concurrency(config.concurrent_transfers())
        .with_partial_results(LfsPartialResultPolicy::FailFast)
        .with_http_policy(http_policy);
    SystemLfsNetworkSession::system(config, store, transfer_config, Duration::from_secs(30))
        .map_err(lfs_network_session_error)
}

fn lfs_has_endpoint_override(
    config: &LfsRuntimeConfig,
    remote: &str,
    operation: LfsOperation,
) -> bool {
    let inputs = config.endpoint_inputs();
    let selected = inputs.remote(remote);
    inputs.lfs_url.is_some()
        || inputs.lfsconfig.lfs_url().is_some()
        || selected.is_some_and(|remote| remote.lfs_url.is_some())
        || inputs.lfsconfig.remote_lfs_url(remote).is_some()
        || (operation == LfsOperation::Push
            && (inputs.lfs_push_url.is_some()
                || inputs.lfsconfig.lfs_push_url().is_some()
                || selected.is_some_and(|remote| remote.lfs_push_url.is_some())))
}

fn lfs_network_outcome(outcome: LfsNetworkOutcome) -> Result<()> {
    match outcome {
        LfsNetworkOutcome::Transfer(transfer) => match transfer.disposition() {
            LfsNetworkDisposition::Complete
            | LfsNetworkDisposition::SkipDownloadErrors
            | LfsNetworkDisposition::AllowIncompletePush => Ok(()),
            LfsNetworkDisposition::FailureRequired => Err(CliError::Stderr {
                code: 2,
                text: format!(
                    "error: LFS transfer failed for {} object(s)\n",
                    transfer.unresolved_failures()
                ),
            }),
        },
        LfsNetworkOutcome::DownloadFailureSkipped(_) => Ok(()),
        LfsNetworkOutcome::RemoteSelectionRequired(_) => Err(CliError::Stderr {
            code: 2,
            text: "error: LFS remote selection requires an explicit remote\n".into(),
        }),
    }
}

fn lfs_download_outcome(
    outcome: LfsNetworkOutcome,
    downloaded: &mut LfsDownloadedObjects,
) -> Result<()> {
    match outcome {
        LfsNetworkOutcome::Transfer(transfer) => {
            downloaded.mark_transfer(&transfer);
            match transfer.disposition() {
                LfsNetworkDisposition::Complete | LfsNetworkDisposition::SkipDownloadErrors => {
                    Ok(())
                }
                LfsNetworkDisposition::AllowIncompletePush
                | LfsNetworkDisposition::FailureRequired => Err(CliError::Stderr {
                    code: 2,
                    text: format!(
                        "error: LFS transfer failed for {} object(s)\n",
                        transfer.unresolved_failures()
                    ),
                }),
            }
        }
        LfsNetworkOutcome::DownloadFailureSkipped(_) => Ok(()),
        LfsNetworkOutcome::RemoteSelectionRequired(_) => Err(CliError::Stderr {
            code: 2,
            text: "error: LFS remote selection requires an explicit remote\n".into(),
        }),
    }
}

fn lfs_oid_from_bytes(bytes: [u8; 32]) -> LfsOid {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = [0_u8; 64];
    for (index, byte) in bytes.into_iter().enumerate() {
        encoded[index * 2] = HEX[(byte >> 4) as usize];
        encoded[index * 2 + 1] = HEX[(byte & 0x0f) as usize];
    }
    let text = std::str::from_utf8(&encoded).expect("lower hexadecimal is UTF-8");
    LfsOid::from_hex(text).expect("encoded SHA-256 is canonical")
}

fn lfs_store_error(error: LfsStoreError) -> CliError {
    CliError::Stderr {
        code: 1,
        text: format!("error: {error}\n"),
    }
}

fn lfs_filter_error(error: LfsFilterError) -> CliError {
    CliError::Stderr {
        code: 1,
        text: format!("error: {error}\n"),
    }
}

#[derive(Clone, Copy)]
enum LfsInstallScope {
    Global,
    Local,
    Worktree,
    System,
}

#[derive(Default)]
struct LfsInstallOptions {
    force: bool,
    manual: bool,
    skip_repo: bool,
    skip_smudge: bool,
    scope: Option<LfsInstallScope>,
}

fn lfs_install(args: &[String]) -> Result<()> {
    let repo = find_repo()?;
    let options = parse_lfs_install_options(args)?;
    if options.manual {
        print!("{}", lfs_install_manual_text(&repo)?);
        println!("Git LFS initialized.");
        return Ok(());
    }
    let scope = options.scope.unwrap_or(LfsInstallScope::Global);
    let config_snapshot = if options.skip_repo {
        None
    } else {
        Some(lfs_install_config_snapshot(&repo, scope)?)
    };
    if let Err(error) = lfs_install_filter_config(&repo, scope, options.skip_smudge, options.force)
    {
        return Err(lfs_install_rollback_error(error, config_snapshot.as_ref()));
    }
    if options.skip_repo {
        println!("Git LFS initialized.");
        return Ok(());
    }
    if let Err(error) = lfs_install_standard_hooks(&repo, options.force) {
        return Err(lfs_install_rollback_error(error, config_snapshot.as_ref()));
    }
    println!("Updated Git hooks.");
    println!("Git LFS initialized.");
    Ok(())
}

struct LfsInstallConfigSnapshot {
    path: PathBuf,
    bytes: Option<Vec<u8>>,
    permissions: Option<fs::Permissions>,
}

fn lfs_install_rollback_error(
    original: CliError,
    snapshot: Option<&LfsInstallConfigSnapshot>,
) -> CliError {
    let Some(snapshot) = snapshot else {
        return original;
    };
    match lfs_restore_install_config(snapshot) {
        Ok(()) => original,
        Err(_) => CliError::Fatal {
            code: 1,
            message: "LFS install failed and configuration rollback failed".into(),
        },
    }
}

fn lfs_install_config_snapshot(
    repo: &GitRepo,
    scope: LfsInstallScope,
) -> Result<LfsInstallConfigSnapshot> {
    let path = lfs_scope_config_path(repo, scope)?;
    match fs::metadata(&path) {
        Ok(metadata) => Ok(LfsInstallConfigSnapshot {
            path: path.clone(),
            bytes: Some(fs::read(&path)?),
            permissions: Some(metadata.permissions()),
        }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(LfsInstallConfigSnapshot {
            path,
            bytes: None,
            permissions: None,
        }),
        Err(error) => Err(CliError::Io(error)),
    }
}

fn lfs_restore_install_config(snapshot: &LfsInstallConfigSnapshot) -> Result<()> {
    match snapshot.bytes.as_ref() {
        Some(bytes) => {
            fs::write(&snapshot.path, bytes)?;
            if let Some(permissions) = snapshot.permissions.as_ref() {
                fs::set_permissions(&snapshot.path, permissions.clone())?;
            }
        }
        None => match fs::remove_file(&snapshot.path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(CliError::Io(error)),
        },
    }
    Ok(())
}

fn parse_lfs_install_options(args: &[String]) -> Result<LfsInstallOptions> {
    let mut options = LfsInstallOptions::default();
    for arg in args {
        match arg.as_str() {
            "--force" => options.force = true,
            "--manual" => options.manual = true,
            "--skip-repo" => options.skip_repo = true,
            "--skip-smudge" => options.skip_smudge = true,
            "--local" => options.scope = Some(LfsInstallScope::Local),
            "--worktree" => options.scope = Some(LfsInstallScope::Worktree),
            "--global" => options.scope = Some(LfsInstallScope::Global),
            "--system" => options.scope = Some(LfsInstallScope::System),
            value if value.starts_with('-') => {
                print!("{}", lfs_install_usage());
                eprintln!("Error: unknown flag: {value}");
                eprintln!();
                return Err(CliError::Exit(127));
            }
            _other => {}
        }
    }
    Ok(options)
}

fn lfs_install_manual_text(repo: &GitRepo) -> Result<String> {
    lfs_update_manual_text(repo)
}

fn lfs_install_usage() -> &'static str {
    concat!(
        "git lfs install [options]\n\n",
        "Perform the following actions to ensure that Git LFS is setup properly:\n\n",
        "* Set up the clean and smudge filters under the name \"lfs\" in the global\n",
        "  Git config.\n",
        "* Install a pre-push hook to run git lfs pre-push for the current\n",
        "  repository, if run from inside one. If \"core.hooksPath\" is configured in\n",
        "  any Git configuration (and supported, i.e., the installed Git version is\n",
        "  at least 2.9.0), then the pre-push hook will be installed to that\n",
        "  directory instead.\n\n",
        "Options:\n\n",
        "Without any options, git lfs install will only setup the \"lfs\" smudge\n",
        "and clean filters if they are not already set.\n\n",
        "--force:\n",
        "  Sets the \"lfs\" smudge and clean filters, overwriting existing values.\n",
        "--global:\n",
        "  Sets the \"lfs\" smudge and clean filters in the global Git config.\n",
        "--local:\n",
        "  Sets the \"lfs\" smudge and clean filters in the local repository's git config,\n",
        "  instead of the global git config (~/.gitconfig).\n",
        "--worktree:\n",
        "  Sets the \"lfs\" smudge and clean filters in the current working tree's git\n",
        "  config, instead of the global git config (~/.gitconfig) or local repository's\n",
        "  git config ($GIT_DIR/config). If multiple working trees are in use, the Git\n",
        "  config extension worktreeConfig must be enabled to use this option. If only\n",
        "  one working tree is in use, --worktree has the same effect as --local.\n",
        "  This option is only available if the installed Git version is at least 2.20.0\n",
        "  and therefore supports the \"worktreeConfig\" extension.\n",
        "--manual:\n",
        "  Print instructions for manually updating your hooks to include git-lfs\n",
        "  functionality. Use this option if git lfs install fails because of existing\n",
        "  hooks and you want to retain their functionality.\n",
        "--system:\n",
        "  Sets the \"lfs\" smudge and clean filters in the system git config, e.g.\n",
        "  /etc/gitconfig instead of the global git config (~/.gitconfig).\n",
        "--skip-smudge:\n",
        "  Skips automatic downloading of objects on clone or pull. This requires a\n",
        "  manual \"git lfs pull\" every time a new commit is checked out on your\n",
        "  repository.\n",
        "--skip-repo:\n",
        "  Skips installation of hooks into the local repository; use if you want to\n",
        "  install the LFS filters but not make changes to the hooks.  It is valid to use\n",
        "  --local, --global, or --system in conjunction with this option.\n",
    )
}

#[derive(Default)]
struct LfsUpdateOptions {
    force: bool,
    manual: bool,
}

fn lfs_update(args: &[String]) -> Result<()> {
    let repo = find_repo()?;
    let options = parse_lfs_update_options(args)?;
    if options.manual {
        print!("{}", lfs_update_manual_text(&repo)?);
        return Ok(());
    }
    lfs_install_standard_hooks(&repo, options.force)?;
    println!("Updated Git hooks.");
    Ok(())
}

fn parse_lfs_update_options(args: &[String]) -> Result<LfsUpdateOptions> {
    let mut options = LfsUpdateOptions::default();
    for arg in args {
        match arg.as_str() {
            "--manual" | "-m" => options.manual = true,
            "--force" | "-f" => options.force = true,
            value if value.starts_with('-') => {
                print!("{}", lfs_update_usage());
                eprintln!("Error: unknown flag: {value}");
                eprintln!();
                return Err(CliError::Exit(127));
            }
            other => {
                print!("{}", lfs_update_usage());
                eprintln!("Error: unknown argument: {other}");
                eprintln!();
                return Err(CliError::Exit(127));
            }
        }
    }
    Ok(options)
}

fn lfs_update_usage() -> String {
    concat!(
        "git lfs update [--manual | --force]\n\n",
        "Updates the Git hooks used by Git LFS. Silently upgrades known hook\n",
        "contents. If you have your own custom hooks you may need to use one of\n",
        "the extended options below.\n\n",
        "Options:\n\n",
        "--manual:\n",
        "-m:\n",
        "  Print instructions for manually updating your hooks to include git-lfs\n",
        "  functionality. Use this option if git lfs update fails because of existing\n",
        "  hooks and you want to retain their functionality.\n",
        "--force:\n",
        "-f:\n",
        "  Forcibly overwrite any existing hooks with git-lfs hooks. Use this option if\n",
        "  git lfs update fails because of existing hooks but you don't care about\n",
        "  their current contents.\n"
    )
    .into()
}

fn lfs_update_manual_text(repo: &GitRepo) -> Result<String> {
    let hooks_dir = lfs_hooks_dir(repo)?;
    let hooks_root = hooks_dir
        .strip_prefix(&repo.root)
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| hooks_dir.display().to_string());
    let executable = lfs_current_executable()?;
    let mut text = String::new();
    for (index, (hook_name, subcommand)) in [
        ("pre-push", "pre-push"),
        ("post-checkout", "post-checkout"),
        ("post-commit", "post-commit"),
        ("post-merge", "post-merge"),
    ]
    .into_iter()
    .enumerate()
    {
        if index > 0 {
            text.push('\n');
        }
        text.push_str(&format!(
            "Add the following to '{hooks_root}/{hook_name}':\n\n\t#!/bin/sh\n\texec {executable} lfs {subcommand} \"$@\"\n",
        ));
    }
    Ok(text)
}

fn lfs_install_filter_config(
    repo: &GitRepo,
    scope: LfsInstallScope,
    skip_smudge: bool,
    force: bool,
) -> Result<()> {
    let executable = lfs_current_executable()?;
    lfs_set_config_value(repo, scope, "lfs.repositoryformatversion", "0", force)?;
    lfs_set_filter_config_value(
        repo,
        scope,
        "filter.lfs.clean",
        &format!("{executable} lfs clean -- %f"),
        &executable,
        force,
    )?;
    let smudge = if skip_smudge {
        format!("{executable} lfs smudge --skip -- %f")
    } else {
        format!("{executable} lfs smudge -- %f")
    };
    lfs_set_filter_config_value(
        repo,
        scope,
        "filter.lfs.smudge",
        &smudge,
        &executable,
        force,
    )?;
    let process = if skip_smudge {
        format!("{executable} lfs filter-process --skip")
    } else {
        format!("{executable} lfs filter-process")
    };
    lfs_set_filter_config_value(
        repo,
        scope,
        "filter.lfs.process",
        &process,
        &executable,
        force,
    )?;
    lfs_set_config_value(repo, scope, "filter.lfs.required", "true", force)?;
    Ok(())
}

fn lfs_set_filter_config_value(
    repo: &GitRepo,
    scope: LfsInstallScope,
    name: &str,
    value: &str,
    executable: &str,
    force: bool,
) -> Result<()> {
    let existing = lfs_scope_config_entry(repo, scope, name)?;
    if !force
        && existing.is_some_and(|entry| {
            !entry
                .value
                .strip_prefix(executable)
                .is_some_and(|tail| tail.starts_with(" lfs "))
        })
    {
        return Ok(());
    }
    match scope {
        LfsInstallScope::Global | LfsInstallScope::System => {
            lfs_set_config_value_in_scope(repo, scope, name, value)
        }
        LfsInstallScope::Local => set_config_value(repo, name, value),
        LfsInstallScope::Worktree => set_worktree_config_value(repo, name, value),
    }
}

fn lfs_set_config_value(
    repo: &GitRepo,
    scope: LfsInstallScope,
    name: &str,
    value: &str,
    force: bool,
) -> Result<()> {
    let existing = lfs_scope_config_entry(repo, scope, name)?;
    if !force && existing.is_some() {
        return Ok(());
    }
    match scope {
        LfsInstallScope::Global | LfsInstallScope::System => {
            lfs_set_config_value_in_scope(repo, scope, name, value)
        }
        LfsInstallScope::Local => set_config_value(repo, name, value),
        LfsInstallScope::Worktree => set_worktree_config_value(repo, name, value),
    }
}

fn lfs_set_config_value_in_scope(
    repo: &GitRepo,
    scope: LfsInstallScope,
    name: &str,
    value: &str,
) -> Result<()> {
    let path = lfs_scope_config_path(repo, scope)?;
    set_config_value_in_file(&path, name, value)
}

fn lfs_scope_config_path(repo: &GitRepo, scope: LfsInstallScope) -> Result<PathBuf> {
    match scope {
        LfsInstallScope::Global => global_config_path_for_write().ok_or(CliError::Exit(1)),
        LfsInstallScope::Local => local_config_path(repo).map_err(CliError::Io),
        LfsInstallScope::Worktree => worktree_config_path_for_scope(repo),
        LfsInstallScope::System => Ok(explicit_system_config_path()),
    }
}

fn lfs_scope_config_entry(
    repo: &GitRepo,
    scope: LfsInstallScope,
    name: &str,
) -> Result<Option<ConfigEntry>> {
    let path = match scope {
        LfsInstallScope::Global | LfsInstallScope::System => lfs_scope_config_path(repo, scope)?,
        LfsInstallScope::Local => local_config_path(repo)?,
        LfsInstallScope::Worktree => worktree_config_path_for_scope(repo)?,
    };
    let (section, subsection, key) = parse_config_name(name).map_err(CliError::Io)?;
    Ok(read_config_file(&path)?.into_iter().rev().find(|entry| {
        entry.section == section && entry.subsection == subsection && entry.key == key
    }))
}

fn lfs_install_standard_hooks(repo: &GitRepo, force: bool) -> Result<()> {
    let hooks_dir = lfs_hooks_dir(repo)?;
    lfs_prepare_hooks_dir(&hooks_dir)?;
    lfs_install_hook(
        &hooks_dir,
        "pre-push",
        LFS_PRE_PUSH_MARKER,
        "pre-push",
        force,
    )?;
    lfs_install_hook(
        &hooks_dir,
        "post-checkout",
        LFS_POST_CHECKOUT_MARKER,
        "post-checkout",
        force,
    )?;
    lfs_install_hook(
        &hooks_dir,
        "post-commit",
        LFS_POST_COMMIT_MARKER,
        "post-commit",
        force,
    )?;
    lfs_install_hook(
        &hooks_dir,
        "post-merge",
        LFS_POST_MERGE_MARKER,
        "post-merge",
        force,
    )?;
    Ok(())
}

fn lfs_install_hook(
    hooks_dir: &Path,
    hook_name: &str,
    marker: &str,
    lfs_subcommand: &str,
    force: bool,
) -> Result<()> {
    #[cfg(windows)]
    {
        return lfs_install_hook_windows(hooks_dir, hook_name, marker, lfs_subcommand, force);
    }

    #[cfg(not(windows))]
    {
        let hook_path = hooks_dir.join(hook_name);
        let mut replace_existing = force;
        let mut expected_identity = None;
        if let Ok(metadata) = fs::symlink_metadata(&hook_path) {
            if metadata.file_type().is_symlink() {
                if !force {
                    return Err(lfs_hook_refusal(&hook_path));
                }
            } else if !metadata.is_file() {
                return Err(lfs_hook_refusal(&hook_path));
            } else if !lfs_hook_is_owned(&hook_path, marker, lfs_subcommand)? {
                if !force {
                    return Err(lfs_hook_refusal(&hook_path));
                }
            } else {
                replace_existing = true;
                expected_identity = Some(lfs_metadata_identity(&metadata));
            }
        } else if let Err(error) = fs::symlink_metadata(&hook_path)
            && error.kind() != io::ErrorKind::NotFound
        {
            return Err(CliError::Io(error));
        }
        let script = format!(
            "#!/bin/sh\n{marker}\nexec {} lfs {lfs_subcommand} \"$@\"\n",
            lfs_current_executable()?
        );
        lfs_atomic_write_hook(
            &hook_path,
            script.as_bytes(),
            replace_existing,
            expected_identity,
        )?;
        Ok(())
    }
}

#[cfg(windows)]
fn lfs_install_hook_windows(
    hooks_dir: &Path,
    hook_name: &str,
    marker: &str,
    lfs_subcommand: &str,
    force: bool,
) -> Result<()> {
    use super::lfs_windows_fs::{WindowsPublishMode, open_regular_snapshot};

    let hook_path = hooks_dir.join(hook_name);
    let mode = match fs::symlink_metadata(&hook_path) {
        Ok(metadata) if metadata.is_dir() => return Err(lfs_hook_refusal(&hook_path)),
        Ok(metadata) if !lfs_safe_regular_metadata(&metadata) => {
            if !force {
                return Err(lfs_hook_refusal(&hook_path));
            }
            WindowsPublishMode::Replace
        }
        Ok(_) => {
            let mut snapshot = open_regular_snapshot(&hook_path)
                .map_err(CliError::Io)?
                .ok_or_else(|| lfs_hook_refusal(&hook_path))?;
            let bytes = snapshot.read_bounded(1024 * 1024).map_err(CliError::Io)?;
            if bytes.len() > 1024 * 1024 {
                return Err(CliError::Io(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "LFS hook is too large",
                )));
            }
            let contents = String::from_utf8(bytes).map_err(|_| {
                CliError::Io(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "LFS hook is not UTF-8",
                ))
            })?;
            if !lfs_hook_contents_are_owned(&contents, &hook_path, marker, lfs_subcommand) && !force
            {
                return Err(lfs_hook_refusal(&hook_path));
            }
            if force {
                WindowsPublishMode::Replace
            } else {
                WindowsPublishMode::Checked(snapshot)
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => WindowsPublishMode::Absent,
        Err(error) => return Err(CliError::Io(error)),
    };
    let script = format!(
        "#!/bin/sh\n{marker}\nexec {} lfs {lfs_subcommand} \"$@\"\n",
        lfs_current_executable()?
    );
    super::lfs_windows_fs::atomic_write(&hook_path, script.as_bytes(), mode, "hook").map_err(
        |error| {
            if error.kind() == io::ErrorKind::AlreadyExists {
                lfs_hook_refusal(&hook_path)
            } else {
                CliError::Io(error)
            }
        },
    )
}

fn lfs_hook_refusal(path: &Path) -> CliError {
    CliError::Fatal {
        code: 1,
        message: format!("refusing to overwrite existing hook '{}'", path.display()),
    }
}

fn lfs_prepare_hooks_dir(path: &Path) -> Result<()> {
    #[cfg(windows)]
    super::lfs_windows_fs::reject_reparse_components(path, true).map_err(CliError::Io)?;
    let mut current = PathBuf::new();
    for component in path.components() {
        if matches!(component, std::path::Component::ParentDir) {
            return Err(CliError::Fatal {
                code: 1,
                message: format!("refusing hooks path traversal '{}'", path.display()),
            });
        }
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if !lfs_safe_directory_metadata(&metadata) => {
                return Err(CliError::Fatal {
                    code: 1,
                    message: format!("refusing symlink hooks directory '{}'", path.display()),
                });
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err(CliError::Fatal {
                    code: 1,
                    message: format!("hooks path is not a directory: '{}'", path.display()),
                });
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                fs::create_dir(&current)?;
            }
            Err(error) => return Err(CliError::Io(error)),
        }
    }
    #[cfg(windows)]
    super::lfs_windows_fs::reject_reparse_components(path, false).map_err(CliError::Io)?;
    Ok(())
}

#[cfg(not(windows))]
fn lfs_atomic_write_hook(
    path: &Path,
    contents: &[u8],
    replace_existing: bool,
    expected_identity: Option<(u64, u64, u64)>,
) -> Result<()> {
    #[cfg(unix)]
    {
        lfs_atomic_publish_unix(
            path,
            contents,
            0o755,
            replace_existing,
            expected_identity,
            "hook",
        )
        .map_err(|error| {
            if error.kind() == io::ErrorKind::AlreadyExists {
                lfs_hook_refusal(path)
            } else {
                CliError::Io(error)
            }
        })
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = (path, contents, replace_existing, expected_identity);
        Err(lfs_descriptor_safety_error("hooks"))
    }
}

#[cfg(unix)]
fn lfs_atomic_publish_unix(
    path: &Path,
    contents: &[u8],
    mode: u32,
    replace_existing: bool,
    expected_identity: Option<(u64, u64, u64)>,
    description: &str,
) -> io::Result<()> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;

    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{description} path has no parent directory"),
        )
    })?;
    let parent_name = CString::new(parent.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in parent path"))?;
    let directory_fd = unsafe {
        libc::open(
            parent_name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            0,
        )
    };
    if directory_fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let directory = unsafe { fs::File::from_raw_fd(directory_fd) };
    let target_name = CString::new(
        path.file_name()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing target name"))?
            .as_bytes(),
    )
    .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in target name"))?;

    let mut temporary_name = None;
    for attempt in 0..32_u32 {
        let candidate = format!(
            ".{}.zmin-tmp-{}-{attempt}",
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("hook"),
            std::process::id()
        );
        let candidate_name = CString::new(candidate.as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in temporary name"))?;
        let temporary_fd = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                candidate_name.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                mode as libc::mode_t as libc::c_uint,
            )
        };
        if temporary_fd >= 0 {
            let mut file = unsafe { fs::File::from_raw_fd(temporary_fd) };
            let write_result = (|| {
                file.write_all(contents)?;
                file.flush()?;
                file.sync_all()
            })();
            drop(file);
            if let Err(error) = write_result {
                unsafe {
                    libc::unlinkat(directory.as_raw_fd(), candidate_name.as_ptr(), 0);
                }
                return Err(error);
            }
            temporary_name = Some(candidate_name);
            break;
        }
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::AlreadyExists {
            continue;
        }
        return Err(error);
    }
    let temporary_name = temporary_name.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("could not create temporary {description}"),
        )
    })?;

    let cleanup = || unsafe { libc::unlinkat(directory.as_raw_fd(), temporary_name.as_ptr(), 0) };
    let publish_result: io::Result<()> = (|| {
        if replace_existing {
            if let Some(expected) = expected_identity {
                let target_fd = unsafe {
                    libc::openat(
                        directory.as_raw_fd(),
                        target_name.as_ptr(),
                        libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                        0,
                    )
                };
                if target_fd < 0 {
                    Err(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        "target changed before atomic replacement",
                    ))
                } else {
                    let target = unsafe { fs::File::from_raw_fd(target_fd) };
                    let metadata = target.metadata()?;
                    if !lfs_safe_regular_metadata(&metadata)
                        || lfs_metadata_identity(&metadata) != expected
                    {
                        Err(io::Error::new(
                            io::ErrorKind::AlreadyExists,
                            "target changed before atomic replacement",
                        ))
                    } else {
                        Ok(())
                    }
                }
            } else {
                Ok(())
            }
            .and_then(|()| {
                let result = unsafe {
                    libc::renameat(
                        directory.as_raw_fd(),
                        temporary_name.as_ptr(),
                        directory.as_raw_fd(),
                        target_name.as_ptr(),
                    )
                };
                if result == 0 {
                    Ok(())
                } else {
                    Err(io::Error::last_os_error())
                }
            })
        } else {
            let result = unsafe {
                libc::linkat(
                    directory.as_raw_fd(),
                    temporary_name.as_ptr(),
                    directory.as_raw_fd(),
                    target_name.as_ptr(),
                    0,
                )
            };
            if result == 0 {
                let unlink_result = cleanup();
                if unlink_result == 0 {
                    Ok(())
                } else {
                    Err(io::Error::last_os_error())
                }
            } else {
                Err(io::Error::last_os_error())
            }
        }
    })();
    if publish_result.is_err() {
        let _ = cleanup();
    }
    if let Err(error) = publish_result {
        return Err(error);
    }
    directory.sync_all()?;
    Ok(())
}

#[cfg(not(unix))]
fn lfs_atomic_publish_unix(
    _path: &Path,
    _contents: &[u8],
    _mode: u32,
    _replace_existing: bool,
    _expected_identity: Option<(u64, u64, u64)>,
    _description: &str,
) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "descriptor-relative publication is unavailable on this platform",
    ))
}

#[cfg(not(any(unix, windows)))]
fn lfs_descriptor_safety_error(kind: &str) -> CliError {
    CliError::Fatal {
        code: 1,
        message: format!(
            "refusing LFS {kind} update: descriptor-relative no-swap safety is unavailable on this platform"
        ),
    }
}

fn lfs_hooks_dir(repo: &GitRepo) -> Result<PathBuf> {
    Ok(
        match read_config_value(repo, "core.hooksPath").map_err(CliError::Io)? {
            Some(path) if Path::new(&path).is_absolute() => PathBuf::from(path),
            Some(path) => repo.root.join(path),
            None => repo.git_dir.join("hooks"),
        },
    )
}

fn lfs_hook_is_owned(path: &Path, marker: &str, lfs_subcommand: &str) -> io::Result<bool> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Ok(false);
    }
    let contents = lfs_read_bounded_text(path)?;
    Ok(lfs_hook_contents_are_owned(
        &contents,
        path,
        marker,
        lfs_subcommand,
    ))
}

fn lfs_hook_contents_are_owned(
    contents: &str,
    path: &Path,
    marker: &str,
    lfs_subcommand: &str,
) -> bool {
    if contents.lines().any(|line| line == marker) {
        return true;
    }
    lfs_hook_matches_stock_git_lfs_script(
        contents,
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default(),
        lfs_subcommand,
    )
}

fn lfs_read_bounded_text(path: &Path) -> io::Result<String> {
    const MAX_TEXT_BYTES: u64 = 1024 * 1024;

    #[cfg(windows)]
    {
        let Some(mut snapshot) = super::lfs_windows_fs::open_regular_snapshot(path)? else {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "LFS hook does not exist",
            ));
        };
        let bytes = snapshot.read_bounded(MAX_TEXT_BYTES)?;
        if bytes.len() as u64 > MAX_TEXT_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "LFS hook is too large",
            ));
        }
        return String::from_utf8(bytes)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "LFS hook is not UTF-8"));
    }

    #[cfg(not(windows))]
    {
        let file = fs::File::open(path)?;
        let mut bytes = Vec::new();
        file.take(MAX_TEXT_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_TEXT_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "LFS hook is too large",
            ));
        }
        String::from_utf8(bytes)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "LFS hook is not UTF-8"))
    }
}

fn lfs_hook_matches_stock_git_lfs_script(
    contents: &str,
    hook_name: &str,
    lfs_subcommand: &str,
) -> bool {
    contents.starts_with("#!/bin/sh\n")
        && contents.contains(
            "This repository is configured for Git LFS but 'git-lfs' was not found on your path.",
        )
        && contents.contains(&format!(
            "deleting the '{hook_name}' file in the hooks directory"
        ))
        && contents.contains(&format!("git lfs {lfs_subcommand} \"$@\""))
}

fn lfs_track(args: &[String]) -> Result<()> {
    let repo = find_repo()?;
    if args.is_empty() {
        return Err(CliError::Fatal {
            code: 1,
            message: "git lfs track requires at least one pattern".into(),
        });
    }
    let attributes_path = lfs_worktree_path(&repo, ".gitattributes")?;
    let mut attributes = read_attributes_lines(&attributes_path)?;
    for pattern in args {
        validate_lfs_pattern(pattern)?;
        let entry = lfs_attribute_line(pattern);
        if attributes.lines.iter().any(|line| line == &entry) {
            println!("\"{pattern}\" already supported");
            continue;
        }
        attributes.lines.push(entry);
        println!("Tracking \"{pattern}\"");
    }
    write_attributes_lines(&attributes_path, attributes)?;
    Ok(())
}

fn lfs_checkout(args: &[String]) -> Result<()> {
    let repo = find_repo()?;
    let config = lfs_runtime_config(&repo)?;
    let algorithm = repo_hash_algorithm_from_config(&repo).map_err(CliError::Io)?;
    let index = read_index_with_algorithm(&repo.index_path, algorithm).map_err(CliError::Io)?;
    let head_index = read_head_index(&repo)?;
    let path_filters = parse_lfs_checkout_paths(args)?;
    lfs_checkout_index_entries(
        &repo,
        index.entries().iter().filter(|entry| entry.stage == 0),
        &path_filters,
        None,
        Some(&head_index),
        true,
        config.storage(),
    )
}

fn lfs_checkout_index_entries<'a, I>(
    repo: &GitRepo,
    entries: I,
    path_filters: &[String],
    pull_selection: Option<&LfsPullCheckoutSelection>,
    head_index: Option<&GitIndex>,
    print_progress: bool,
    storage: &Path,
) -> Result<()>
where
    I: Iterator<Item = &'a IndexEntry>,
{
    let mut candidate_count = 0_usize;
    let mut total_size = 0_u64;
    let mut missing = Vec::new();
    let store = LfsStore::new(storage.join("objects")).map_err(lfs_store_error)?;

    for entry in entries {
        if !path_filters.is_empty()
            && !path_filters
                .iter()
                .any(|candidate| candidate.as_bytes() == entry.path.as_slice())
        {
            continue;
        }
        let worktree_path = lfs_worktree_path_bytes(repo, &entry.path)?;
        let pointer_file = match read_lfs_pointer_file(&worktree_path) {
            Ok(Some(pointer_file)) => pointer_file,
            Ok(None) => continue,
            Err(error) => return Err(CliError::Io(error)),
        };
        let pointer = &pointer_file.pointer;
        if pull_selection.is_some_and(|selection| !selection.allows(&entry.path, pointer)) {
            continue;
        }
        let count_progress = head_index.is_none_or(|head| head.entry(&entry.path, 0).is_some());
        if count_progress {
            candidate_count += 1;
            total_size = total_size.saturating_add(pointer.size());
        }
        match lfs_materialize_verified(&store, pointer_file, &worktree_path) {
            Ok(()) => {}
            Err(LfsCheckoutError::Missing) => {
                missing.push(String::from_utf8_lossy(&entry.path).into_owned())
            }
            Err(LfsCheckoutError::Cli(error)) => return Err(error),
        }
    }

    if print_progress && candidate_count > 0 {
        println!(
            "Checking out LFS objects: 100% ({candidate_count}/{candidate_count}), {total_size} B | 0 B/s, done."
        );
    }
    for path in missing {
        eprintln!("Skipped checkout for \"{path}\", content not local. Use fetch to download.");
    }
    Ok(())
}

enum LfsCheckoutError {
    Missing,
    Cli(CliError),
}

fn lfs_materialize_verified(
    store: &LfsStore,
    pointer_file: LfsPointerFile,
    target: &Path,
) -> std::result::Result<(), LfsCheckoutError> {
    let LfsPointerFile {
        pointer,
        #[cfg(windows)]
        snapshot,
    } = pointer_file;
    let mut reader = match store.open_verified(pointer.oid().bytes(), pointer.size()) {
        Ok(reader) => reader,
        Err(LfsStoreError::MissingObject) => return Err(LfsCheckoutError::Missing),
        Err(error) => return Err(LfsCheckoutError::Cli(lfs_store_error(error))),
    };
    let result = (|| -> Result<()> {
        let parent = target.parent().ok_or_else(|| CliError::Fatal {
            code: 1,
            message: "LFS checkout path has no parent directory".into(),
        })?;
        lfs_validate_directory_path(parent)?;

        #[cfg(windows)]
        {
            let mut temporary =
                super::lfs_windows_fs::WindowsTempFile::create(target, "checkout target")
                    .map_err(CliError::Io)?;
            lfs_copy_verified(&mut reader, &mut temporary)?;
            temporary.sync_all().map_err(CliError::Io)?;
            return temporary
                .publish(
                    target,
                    super::lfs_windows_fs::WindowsPublishMode::Checked(snapshot),
                    "checkout target",
                )
                .map_err(|error| {
                    if error.kind() == io::ErrorKind::AlreadyExists {
                        CliError::Fatal {
                            code: 1,
                            message: format!(
                                "LFS checkout target changed while it was being materialized: '{}'",
                                target.display()
                            ),
                        }
                    } else {
                        CliError::Io(error)
                    }
                });
        }

        #[cfg(not(windows))]
        {
            let existing = match fs::symlink_metadata(target) {
                Ok(metadata) if !lfs_safe_regular_metadata(&metadata) => {
                    return Err(CliError::Fatal {
                        code: 1,
                        message: format!("refusing to replace symlink '{}'", target.display()),
                    });
                }
                Ok(metadata) if metadata.is_file() => Some(metadata),
                Ok(_) => {
                    return Err(CliError::Fatal {
                        code: 1,
                        message: format!(
                            "LFS checkout target is not a regular file: '{}'",
                            target.display()
                        ),
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => None,
                Err(error) => return Err(CliError::Io(error)),
            };
            let file_name = target
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| CliError::Fatal {
                    code: 1,
                    message: "LFS checkout target has an invalid name".into(),
                })?;
            let mut temporary_and_file = None;
            for attempt in 0..32_u32 {
                let temporary = parent.join(format!(
                    ".{file_name}.zmin-lfs-tmp-{}-{attempt}",
                    std::process::id()
                ));
                let mut options = fs::OpenOptions::new();
                options.write(true).create_new(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
                    options.mode(
                        existing
                            .as_ref()
                            .map_or(0o644, |metadata| metadata.permissions().mode() & 0o7777),
                    );
                }
                match options.open(&temporary) {
                    Ok(file) => {
                        temporary_and_file = Some((temporary, file));
                        break;
                    }
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(CliError::Io(error)),
                }
            }
            let Some((temporary, mut file)) = temporary_and_file else {
                return Err(CliError::Fatal {
                    code: 1,
                    message: "could not create temporary LFS checkout file".into(),
                });
            };
            let copy_result = (|| -> Result<()> {
                lfs_copy_verified(&mut reader, &mut file)?;
                file.flush().map_err(CliError::Io)?;
                file.sync_all().map_err(CliError::Io)?;
                drop(file);
                lfs_atomic_replace_file(&temporary, target)?;
                Ok(())
            })();
            if copy_result.is_err() {
                let _ = fs::remove_file(&temporary);
            }
            copy_result
        }
    })();
    result.map_err(LfsCheckoutError::Cli)
}

fn lfs_copy_verified<W: Write>(reader: &mut VerifiedLfsReader, output: &mut W) -> Result<()> {
    let mut buffer = [0_u8; 128 * 1024];
    loop {
        let read = reader.read_verified(&mut buffer).map_err(lfs_store_error)?;
        if read == 0 {
            break;
        }
        output.write_all(&buffer[..read]).map_err(CliError::Io)?;
    }
    reader.finish().map_err(lfs_store_error)
}

fn lfs_validate_directory_path(path: &Path) -> Result<()> {
    #[cfg(windows)]
    super::lfs_windows_fs::reject_reparse_components(path, false).map_err(CliError::Io)?;
    let mut current = PathBuf::new();
    for component in path.components() {
        if matches!(component, std::path::Component::ParentDir) {
            return Err(CliError::Fatal {
                code: 1,
                message: "LFS checkout path escapes the repository".into(),
            });
        }
        if let std::path::Component::Normal(value) = component {
            if value.to_str().is_some_and(lfs_windows_unsafe_component) {
                return Err(CliError::Fatal {
                    code: 1,
                    message: format!("unsafe Windows directory component in '{}'", path.display()),
                });
            }
            #[cfg(not(unix))]
            if value.to_str().is_none() {
                return Err(CliError::Fatal {
                    code: 1,
                    message: format!("invalid non-UTF-8 directory component '{}'", path.display()),
                });
            }
        }
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if !lfs_safe_directory_metadata(&metadata) => {
                return Err(CliError::Fatal {
                    code: 1,
                    message: format!("refusing unsafe directory '{}'", path.display()),
                });
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err(CliError::Fatal {
                    code: 1,
                    message: format!(
                        "LFS checkout parent is not a directory: '{}'",
                        path.display()
                    ),
                });
            }
            Ok(_) => {}
            Err(error) => return Err(CliError::Io(error)),
        }
    }
    Ok(())
}

fn lfs_validate_git_worktree_path(path: &str) -> Result<()> {
    lfs_validate_git_worktree_path_bytes(path.as_bytes())
}

fn lfs_validate_git_worktree_path_bytes(path: &[u8]) -> Result<()> {
    let invalid = path.is_empty()
        || path.starts_with(b"/")
        || path.starts_with(b"\\")
        || path.contains(&b'\\')
        || path.contains(&b':')
        || path.iter().any(|byte| byte.is_ascii_control())
        || std::str::from_utf8(path).is_ok_and(|path| path.chars().any(char::is_control))
        || path.split(|byte| *byte == b'/').any(|component| {
            component.is_empty()
                || component == b"."
                || component == b".."
                || std::str::from_utf8(component).is_ok_and(lfs_windows_unsafe_component)
        });
    if invalid {
        return Err(CliError::Fatal {
            code: 1,
            message: "invalid Git worktree path".into(),
        });
    }
    let native = lfs_native_git_path(path)?;
    for component in native.components() {
        if matches!(
            component,
            std::path::Component::Prefix(_)
                | std::path::Component::RootDir
                | std::path::Component::ParentDir
                | std::path::Component::CurDir
        ) {
            return Err(CliError::Fatal {
                code: 1,
                message: "invalid Git worktree path".into(),
            });
        }
    }
    Ok(())
}

#[cfg(unix)]
fn lfs_native_git_path(path: &[u8]) -> Result<PathBuf> {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    Ok(PathBuf::from(OsString::from_vec(path.to_vec())))
}

#[cfg(not(unix))]
fn lfs_native_git_path(path: &[u8]) -> Result<PathBuf> {
    let path = std::str::from_utf8(path).map_err(|_| CliError::Fatal {
        code: 1,
        message: "Git index contains a non-UTF-8 worktree path".into(),
    })?;
    Ok(PathBuf::from(path))
}

fn lfs_windows_unsafe_component(component: &str) -> bool {
    if component.is_empty() || component.ends_with(['.', ' ']) {
        return true;
    }
    let stem = component
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end_matches(['.', ' ']);
    matches!(
        stem.to_ascii_uppercase().as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
}

fn lfs_worktree_path(repo: &GitRepo, path: &str) -> Result<PathBuf> {
    lfs_validate_git_worktree_path(path)?;
    lfs_worktree_path_native(repo, Path::new(path))
}

fn lfs_worktree_path_bytes(repo: &GitRepo, path: &[u8]) -> Result<PathBuf> {
    lfs_validate_git_worktree_path_bytes(path)?;
    let native = lfs_native_git_path(path)?;
    lfs_worktree_path_native(repo, &native)
}

fn lfs_worktree_path_native(repo: &GitRepo, path: &Path) -> Result<PathBuf> {
    let root = fs::canonicalize(&repo.root)?;
    let target = repo.root.join(path);
    let parent = target.parent().ok_or_else(|| CliError::Fatal {
        code: 1,
        message: "Git worktree path has no parent directory".into(),
    })?;
    lfs_validate_directory_path(parent)?;
    let canonical_parent = fs::canonicalize(parent)?;
    if !canonical_parent.starts_with(&root) {
        return Err(CliError::Fatal {
            code: 1,
            message: "Git worktree path escapes the repository".into(),
        });
    }
    match fs::symlink_metadata(&target) {
        Ok(metadata) if !lfs_safe_regular_metadata(&metadata) => {
            return Err(CliError::Fatal {
                code: 1,
                message: format!("unsafe Git worktree path '{}'", target.display()),
            });
        }
        Ok(_) => {
            let canonical_target = fs::canonicalize(&target)?;
            if !canonical_target.starts_with(&root) {
                return Err(CliError::Fatal {
                    code: 1,
                    message: "Git worktree path escapes the repository".into(),
                });
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(CliError::Io(error)),
    }
    Ok(target)
}

fn lfs_safe_directory_metadata(metadata: &fs::Metadata) -> bool {
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return false;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return false;
        }
    }
    true
}

#[cfg(not(windows))]
fn lfs_atomic_replace_file(temporary: &Path, target: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        fs::rename(temporary, target)?;
    }
    #[cfg(not(unix))]
    {
        return Err(CliError::Fatal {
            code: 1,
            message: "descriptor-relative LFS checkout replacement is unavailable on this platform"
                .into(),
        });
    }
    #[cfg(unix)]
    {
        if let Some(parent) = target.parent() {
            fs::File::open(parent)?.sync_all()?;
        }
    }
    Ok(())
}

struct LfsPointerFile {
    pointer: LfsPointer,
    #[cfg(windows)]
    snapshot: super::lfs_windows_fs::WindowsFileSnapshot,
}

fn read_lfs_pointer_file(path: &Path) -> io::Result<Option<LfsPointerFile>> {
    #[cfg(windows)]
    {
        let Some(mut snapshot) = super::lfs_windows_fs::open_regular_snapshot(path)? else {
            return Ok(None);
        };
        let bytes = snapshot.read_bounded(LFS_POINTER_MAX_BYTES as u64)?;
        if bytes.len() > LFS_POINTER_MAX_BYTES {
            return Ok(None);
        }
        return Ok(parse_lfs_pointer(&bytes).map(|pointer| LfsPointerFile { pointer, snapshot }));
    }

    #[cfg(not(windows))]
    {
        let file = match fs::File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let mut bytes = Vec::new();
        file.take((LFS_POINTER_MAX_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > LFS_POINTER_MAX_BYTES {
            return Ok(None);
        }
        Ok(parse_lfs_pointer(&bytes).map(|pointer| LfsPointerFile { pointer }))
    }
}

fn parse_lfs_checkout_paths(args: &[String]) -> Result<Vec<String>> {
    let mut paths = Vec::new();
    for arg in args {
        if arg.starts_with('-') {
            return Err(CliError::Stderr {
                code: 127,
                text: format!("Error: unknown flag: {arg}\n\n{}\n", lfs_checkout_usage()),
            });
        }
        lfs_validate_git_worktree_path(arg)?;
        paths.push(arg.clone());
    }
    Ok(paths)
}

fn lfs_checkout_usage() -> &'static str {
    concat!(
        "git lfs checkout [<path>...]\n\n",
        "Try to replace file pointers in the working tree with their local object content.\n",
        "Only content already present in the local Git LFS storage is checked out.\n"
    )
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct LfsPullPointerIdentity {
    oid: [u8; 32],
    size: u64,
}

#[derive(Default)]
struct LfsDownloadedObjects {
    available: BTreeSet<[u8; 32]>,
}

impl LfsDownloadedObjects {
    fn mark_available(&mut self, oid: LfsOid) {
        self.available.insert(oid.bytes());
    }

    fn mark_transfer(&mut self, transfer: &LfsNetworkTransferOutcome) {
        for report in transfer.reports() {
            for success in report.succeeded() {
                if matches!(success.outcome(), LfsTransferSuccessKind::Downloaded(_)) {
                    self.mark_available(success.object().oid());
                }
            }
        }
    }
}

struct LfsPullCheckoutSelection {
    pointers: BTreeSet<LfsPullPointerIdentity>,
    available: BTreeSet<[u8; 32]>,
    filter: LfsFetchFilter,
}

impl LfsPullCheckoutSelection {
    fn from_plan(
        plan: &LfsReachabilityPlan,
        downloaded: LfsDownloadedObjects,
        filter: LfsFetchFilter,
    ) -> Self {
        let pointers = plan
            .pointers()
            .iter()
            .map(|pointer| LfsPullPointerIdentity {
                oid: pointer.oid().bytes(),
                size: pointer.size(),
            })
            .collect();
        Self {
            pointers,
            available: downloaded.available,
            filter,
        }
    }

    fn allows(&self, path: &[u8], pointer: &LfsPointer) -> bool {
        let identity = LfsPullPointerIdentity {
            oid: pointer.oid().bytes(),
            size: pointer.size(),
        };
        self.filter.allows_bytes(path)
            && self.pointers.contains(&identity)
            && self.available.contains(&identity.oid)
    }
}

fn lfs_fetch(args: &[String]) -> Result<()> {
    let repo = find_repo()?;
    if let Some(flag) = args.iter().find(|arg| arg.starts_with('-')) {
        return Err(lfs_unsupported_option("fetch", flag));
    }
    let config = lfs_runtime_config(&repo)?;
    let remote = lfs_select_remote(
        &config,
        LfsOperation::Fetch,
        args.first().map(String::as_str),
    )?;
    if !args.is_empty() {
        lfs_validate_explicit_remote(&repo, &config, LfsOperation::Fetch, &remote)?;
    }
    let revisions = args.get(1..).unwrap_or_default();
    let selection = if revisions.is_empty() {
        LfsFetchSelection::Current
    } else {
        LfsFetchSelection::Explicit(
            LfsExplicitFetchSelection::new(revisions.to_vec()).map_err(lfs_reachability_error)?,
        )
    };
    let repository = CliLfsReachabilityRepository::new(&repo, remote.remote_name())?;
    let planner = LfsReachabilityPlanner::new(
        &repository,
        config.fetch_filter(),
        config.remote_policy().into(),
        LfsReachabilityLimits::default(),
    )
    .map_err(lfs_reachability_error)?;
    let plan = planner
        .plan_fetch(&selection)
        .map_err(lfs_reachability_error)?;
    lfs_download_plan(&repo, &remote, config, &plan).map(|_| ())
}

fn lfs_pull(args: &[String]) -> Result<()> {
    if args.len() > 1 {
        return Err(CliError::Fatal {
            code: 2,
            message: "usage: git lfs pull [<remote>]".into(),
        });
    }
    if let Some(flag) = args.first().filter(|arg| arg.starts_with('-')) {
        return Err(lfs_unsupported_option("pull", flag));
    }
    let repo = find_repo()?;
    let config = lfs_runtime_config(&repo)?;
    let remote = lfs_select_remote(
        &config,
        LfsOperation::Fetch,
        args.first().map(String::as_str),
    )?;
    if !args.is_empty() {
        lfs_validate_explicit_remote(&repo, &config, LfsOperation::Fetch, &remote)?;
    }
    let storage = config.storage().to_path_buf();
    let repository = CliLfsReachabilityRepository::new(&repo, remote.remote_name())?;
    let planner = LfsReachabilityPlanner::new(
        &repository,
        config.fetch_filter(),
        config.remote_policy().into(),
        LfsReachabilityLimits::default(),
    )
    .map_err(lfs_reachability_error)?;
    let plan = planner
        .plan_fetch(&LfsFetchSelection::Current)
        .map_err(lfs_reachability_error)?;
    let fetch_filter = config.fetch_filter().clone();
    let downloaded = lfs_download_plan(&repo, &remote, config, &plan)?;
    let pull_selection = LfsPullCheckoutSelection::from_plan(&plan, downloaded, fetch_filter);
    let algorithm = repo_hash_algorithm_from_config(&repo).map_err(CliError::Io)?;
    let index = read_index_with_algorithm(&repo.index_path, algorithm).map_err(CliError::Io)?;
    lfs_checkout_index_entries(
        &repo,
        index.entries().iter().filter(|entry| entry.stage == 0),
        &[],
        Some(&pull_selection),
        None,
        false,
        &storage,
    )?;
    Ok(())
}

fn lfs_download_plan(
    repo: &GitRepo,
    remote: &CliLfsRemoteSelection,
    config: LfsRuntimeConfig,
    plan: &LfsReachabilityPlan,
) -> Result<LfsDownloadedObjects> {
    lfs_require_basic_pointers(plan)?;
    let store = Arc::new(LfsStore::new(config.storage().join("objects")).map_err(lfs_store_error)?);
    let mut downloaded = LfsDownloadedObjects::default();
    let mut missing_objects = vec![None; plan.pointers().len()];
    for (index, pointer) in plan.pointers().iter().enumerate() {
        match store.verify(pointer.oid().bytes(), pointer.size()) {
            Ok(()) => {
                downloaded.mark_available(*pointer.oid());
                continue;
            }
            Err(
                LfsStoreError::MissingObject
                | LfsStoreError::HashMismatch { .. }
                | LfsStoreError::SizeMismatch { .. }
                | LfsStoreError::CorruptObject,
            ) => {}
            Err(error) => return Err(lfs_store_error(error)),
        }
        missing_objects[index] = Some(
            LfsTransferObject::new(*pointer.oid(), pointer.size()).map_err(|error| {
                CliError::Stderr {
                    code: 2,
                    text: format!("error: {error}\n"),
                }
            })?,
        );
    }
    if missing_objects.iter().all(Option::is_none) {
        return Ok(downloaded);
    }
    let skip_download_errors = config.skip_download_errors();
    let source = lfs_command_transfer_source(
        repo,
        remote,
        config,
        Arc::clone(&store),
        LfsOperation::Fetch,
    )?;
    match source {
        CliLfsTransferSource::Local(local) => {
            let mut missing = None;
            for object in missing_objects.into_iter().flatten() {
                let path = lfs_remote_media_path(&local, object.oid().hex())
                    .filter(|path| lfs_is_regular_file(path));
                let Some(path) = path else {
                    missing.get_or_insert(object.oid());
                    continue;
                };
                let file = match fs::File::open(path) {
                    Ok(file) => file,
                    Err(_) if skip_download_errors => continue,
                    Err(error) => return Err(CliError::Io(error)),
                };
                if let Err(error) = store.ingest(object.oid().bytes(), object.size(), file) {
                    if skip_download_errors {
                        continue;
                    }
                    return Err(lfs_store_error(error));
                }
                downloaded.mark_available(object.oid());
            }
            if let Some(oid) = missing
                && !skip_download_errors
            {
                return Err(lfs_pull_missing_objects_error(
                    Some(&local),
                    &[oid.hex().to_owned()],
                ));
            }
            Ok(downloaded)
        }
        CliLfsTransferSource::Network(mut session) => {
            if plan.transfer_groups().is_empty() {
                return Err(CliError::Stderr {
                    code: 2,
                    text: "error: invalid LFS fetch transfer plan\n".into(),
                });
            }
            let mut requested = 0_usize;
            for group in plan.transfer_groups() {
                let objects = group
                    .pointer_indexes()
                    .iter()
                    .filter_map(|index| missing_objects.get(*index).copied().flatten())
                    .collect::<Vec<_>>();
                if objects.is_empty() {
                    continue;
                }
                requested = requested.saturating_add(objects.len());
                let mut request = LfsNetworkRequest::new(objects);
                if let Some(remote) = remote.remote_name() {
                    request = request.with_remote(remote);
                }
                if let Some(reference) = group.remote_ref() {
                    request =
                        request.with_reference(LfsBatchRef::new(reference).map_err(|error| {
                            CliError::Stderr {
                                code: 2,
                                text: format!("error: {error}\n"),
                            }
                        })?);
                }
                let outcome = session
                    .download_missing(&request)
                    .map_err(lfs_network_session_error)?;
                lfs_download_outcome(outcome, &mut downloaded)?;
            }
            if requested != missing_objects.iter().flatten().count() {
                return Err(CliError::Stderr {
                    code: 2,
                    text: "error: invalid LFS fetch transfer plan\n".into(),
                });
            }
            Ok(downloaded)
        }
        CliLfsTransferSource::Unavailable if skip_download_errors => Ok(downloaded),
        CliLfsTransferSource::Unavailable => Err(CliError::Stderr {
            code: 2,
            text: "error: no LFS transfer endpoint is configured\n".into(),
        }),
    }
}

fn lfs_smudge_batch_ref(
    repo: &GitRepo,
    config: &LfsRuntimeConfig,
    selection: &CliLfsRemoteSelection,
) -> Result<Option<LfsBatchRef>> {
    let algorithm = repo_hash_algorithm_from_config(repo).map_err(CliError::Io)?;
    let refs = RefStore::new(&repo.git_dir, algorithm);
    let local_ref = match refs.read_head().map_err(CliError::Io)? {
        RefTarget::Direct(object) => {
            return LfsBatchRef::new(object.to_hex())
                .map(Some)
                .map_err(|error| CliError::Stderr {
                    code: 2,
                    text: format!("error: {error}\n"),
                });
        }
        RefTarget::Symbolic(local_ref) if local_ref.starts_with("refs/heads/") => {
            match refs.read_ref(&local_ref) {
                Ok(_) => local_ref,
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(CliError::Io(error)),
            }
        }
        RefTarget::Symbolic(_) => return Ok(None),
    };
    let Some(branch) = local_ref.strip_prefix("refs/heads/") else {
        return Ok(None);
    };
    let inputs = config.endpoint_inputs();
    let reference = match selection {
        CliLfsRemoteSelection::Named(selected)
            if inputs.current_remote.as_deref() == Some(selected.as_str()) =>
        {
            match read_config_value(repo, &format!("branch.{branch}.merge"))
                .map_err(CliError::Io)?
            {
                Some(upstream) if LfsBatchRef::new(upstream.clone()).is_ok() => upstream,
                _ => local_ref,
            }
        }
        CliLfsRemoteSelection::GlobalEndpoint => {
            let default_remote = inputs
                .current_remote
                .as_deref()
                .or(inputs.default_remote.as_deref())
                .or_else(|| (inputs.remotes.len() == 1).then(|| inputs.remotes[0].name.as_str()))
                .or_else(|| inputs.remote("origin").map(|_| "origin"));
            let push_remote = inputs
                .current_push_remote
                .as_deref()
                .or(inputs.push_default_remote.as_deref())
                .or(default_remote);
            let push_default = read_config_value(repo, "push.default")
                .map_err(CliError::Io)?
                .unwrap_or_default();
            let tracks_upstream = matches!(push_default.as_str(), "upstream" | "tracking")
                || matches!(push_default.as_str(), "" | "simple")
                    && inputs.current_remote.as_deref() == push_remote;
            if tracks_upstream {
                read_config_value(repo, &format!("branch.{branch}.merge"))
                    .map_err(CliError::Io)?
                    .unwrap_or(local_ref)
            } else {
                local_ref
            }
        }
        CliLfsRemoteSelection::Named(_)
        | CliLfsRemoteSelection::FetchHeadEndpoint
        | CliLfsRemoteSelection::PolicyRequired(_)
        | CliLfsRemoteSelection::Missing => local_ref,
    };
    LfsBatchRef::new(reference)
        .map(Some)
        .map_err(|error| CliError::Stderr {
            code: 2,
            text: format!("error: {error}\n"),
        })
}

fn lfs_push(args: &[String]) -> Result<()> {
    let Some(remote_arg) = args.first() else {
        return Err(CliError::Fatal {
            code: 2,
            message: "usage: git lfs push <remote> [<ref>...]".into(),
        });
    };
    if let Some(flag) = args.iter().find(|arg| arg.starts_with('-')) {
        return Err(lfs_unsupported_option("push", flag));
    }
    let repo = find_repo()?;
    let config = lfs_runtime_config(&repo)?;
    let remote = lfs_select_remote(&config, LfsOperation::Push, Some(remote_arg))?;
    lfs_validate_explicit_remote(&repo, &config, LfsOperation::Push, &remote)?;
    let specs = if args.len() == 1 {
        vec![lfs_current_push_spec(&repo)?]
    } else {
        args[1..]
            .iter()
            .map(|value| LfsPushSpec::parse(value).map_err(lfs_reachability_error))
            .collect::<Result<Vec<_>>>()?
    };
    let planner_remote = config
        .endpoint_inputs()
        .remote(remote_arg)
        .map(|_| remote_arg.as_str())
        .or_else(|| remote.remote_name());
    let repository = CliLfsReachabilityRepository::new(&repo, planner_remote)?;
    let planner = LfsReachabilityPlanner::new(
        &repository,
        config.fetch_filter(),
        config.remote_policy().into(),
        LfsReachabilityLimits::default(),
    )
    .map_err(lfs_reachability_error)?;
    let plan = planner
        .plan_push_specs(&specs)
        .map_err(lfs_reachability_error)?;
    lfs_upload_plan(&repo, &remote, config, &plan, None)
}

fn lfs_current_push_spec(repo: &GitRepo) -> Result<LfsPushSpec> {
    let algorithm = repo_hash_algorithm_from_config(repo).map_err(CliError::Io)?;
    let refs = RefStore::new(&repo.git_dir, algorithm);
    let branch = current_branch_ref(&refs)?.ok_or_else(|| CliError::Stderr {
        code: 2,
        text: "error: cannot push LFS objects from a detached HEAD without an explicit ref\n"
            .into(),
    })?;
    LfsPushRefspec::new(branch.clone(), branch, false)
        .map(LfsPushSpec::Refspec)
        .map_err(lfs_reachability_error)
}

fn lfs_upload_plan(
    repo: &GitRepo,
    remote: &CliLfsRemoteSelection,
    config: LfsRuntimeConfig,
    plan: &LfsReachabilityPlan,
    remote_url: Option<&str>,
) -> Result<()> {
    lfs_require_basic_pointers(plan)?;
    let objects = plan
        .pointers()
        .iter()
        .map(|pointer| {
            LfsTransferObject::new(*pointer.oid(), pointer.size()).map_err(|error| {
                CliError::Stderr {
                    code: 2,
                    text: format!("error: {error}\n"),
                }
            })
        })
        .collect::<Result<Vec<_>>>()?;
    if objects.is_empty() {
        return Ok(());
    }
    let allow_incomplete = config.allow_incomplete_push();
    let local_store =
        Arc::new(LfsStore::new(config.storage().join("objects")).map_err(lfs_store_error)?);
    let has_endpoint_override = match remote {
        CliLfsRemoteSelection::GlobalEndpoint => true,
        CliLfsRemoteSelection::Named(remote) => {
            lfs_has_endpoint_override(&config, remote, LfsOperation::Push)
        }
        CliLfsRemoteSelection::FetchHeadEndpoint
        | CliLfsRemoteSelection::PolicyRequired(_)
        | CliLfsRemoteSelection::Missing => false,
    };
    let explicit_local = (!has_endpoint_override)
        .then(|| remote_url.and_then(lfs_remote_git_dir_from_url))
        .flatten()
        .map(|git_dir| LfsPullRemote { git_dir });
    let source = match explicit_local {
        Some(local) => CliLfsTransferSource::Local(local),
        None if remote_url.is_some() => CliLfsTransferSource::Network(lfs_system_network_session(
            config.clone(),
            Arc::clone(&local_store),
        )?),
        None => lfs_command_transfer_source(
            repo,
            remote,
            config.clone(),
            Arc::clone(&local_store),
            LfsOperation::Push,
        )?,
    };
    match source {
        CliLfsTransferSource::Local(local) => {
            lfs_require_supported_push_policy(&config)?;
            let remote_store = LfsStore::new(local.git_dir.join("lfs").join("objects"))
                .map_err(lfs_store_error)?;
            for object in objects {
                let reader = match local_store.open_verified(object.oid().bytes(), object.size()) {
                    Ok(reader) => reader,
                    Err(
                        LfsStoreError::MissingObject
                        | LfsStoreError::HashMismatch { .. }
                        | LfsStoreError::SizeMismatch { .. }
                        | LfsStoreError::CorruptObject,
                    ) if allow_incomplete => continue,
                    Err(error) => return Err(lfs_store_error(error)),
                };
                remote_store
                    .ingest(object.oid().bytes(), object.size(), reader)
                    .map_err(lfs_store_error)?;
            }
            Ok(())
        }
        CliLfsTransferSource::Network(mut session) => {
            if plan.transfer_groups().is_empty() {
                return Err(CliError::Stderr {
                    code: 2,
                    text: "error: invalid LFS destination transfer plan\n".into(),
                });
            }
            let override_url = remote_url
                .map(|url| LfsRemoteUrlOverride::new(url.to_owned()))
                .transpose()
                .map_err(|error| CliError::Stderr {
                    code: 2,
                    text: format!("error: {error}\n"),
                })?;
            for group in plan.transfer_groups() {
                if group.pointer_indexes().is_empty() {
                    continue;
                }
                let group_objects = group
                    .pointer_indexes()
                    .iter()
                    .map(|index| objects[*index])
                    .collect();
                let mut request = LfsNetworkRequest::new(group_objects);
                if let Some(remote) = remote.remote_name() {
                    request = request.with_remote(remote);
                }
                if let Some(reference) = group.remote_ref() {
                    request =
                        request.with_reference(LfsBatchRef::new(reference).map_err(|error| {
                            CliError::Stderr {
                                code: 2,
                                text: format!("error: {error}\n"),
                            }
                        })?);
                }
                if let Some(override_url) = override_url.clone() {
                    request = if let Some(remote) = remote.remote_name() {
                        request.with_remote_url(remote, override_url)
                    } else {
                        request.with_anonymous_remote_url(override_url)
                    };
                }
                let outcome = session
                    .upload(&request)
                    .map_err(lfs_network_session_error)?;
                lfs_network_outcome(outcome)?;
            }
            Ok(())
        }
        CliLfsTransferSource::Unavailable => Err(CliError::Stderr {
            code: 2,
            text: "error: no LFS transfer endpoint is configured\n".into(),
        }),
    }
}

fn lfs_require_basic_pointers(plan: &LfsReachabilityPlan) -> Result<()> {
    if plan
        .pointers()
        .iter()
        .any(|pointer| !pointer.pointer().extensions().is_empty())
    {
        return Err(CliError::Stderr {
            code: 2,
            text: "error: LFS pointer extensions are unsupported\n".into(),
        });
    }
    Ok(())
}

fn lfs_require_supported_push_policy(config: &LfsRuntimeConfig) -> Result<()> {
    if config.locks_verify() == Some(true) {
        return Err(CliError::Stderr {
            code: 2,
            text: "error: lfs.locksverify=true requires the unsupported locking API\n".into(),
        });
    }
    Ok(())
}

fn lfs_skip_push() -> Result<bool> {
    let Some(value) = std::env::var_os("GIT_LFS_SKIP_PUSH") else {
        return Ok(false);
    };
    let value = value.into_string().map_err(|_| CliError::Stderr {
        code: 2,
        text: "error: GIT_LFS_SKIP_PUSH is not valid UTF-8\n".into(),
    })?;
    parse_git_bool(&value).ok_or_else(|| CliError::Stderr {
        code: 2,
        text: "error: GIT_LFS_SKIP_PUSH is not a valid boolean\n".into(),
    })
}

fn lfs_unsupported_option(command: &str, option: &str) -> CliError {
    CliError::Stderr {
        code: 2,
        text: format!("error: git lfs {command} does not support option '{option}'\n"),
    }
}

struct LfsPullRemote {
    git_dir: PathBuf,
}

fn resolve_lfs_local_remote(repo: &GitRepo, remote_name: &str) -> Result<Option<LfsPullRemote>> {
    let url_key = format!("remote.{remote_name}.url");
    let Some(url) = read_config_value(repo, &url_key).map_err(CliError::Io)? else {
        return Ok(None);
    };
    Ok(lfs_remote_git_dir_from_url(&url).map(|git_dir| LfsPullRemote { git_dir }))
}

fn lfs_remote_git_dir_from_url(url: &str) -> Option<PathBuf> {
    let path = if let Some(rest) = url.strip_prefix("file://") {
        PathBuf::from(rest)
    } else {
        PathBuf::from(url)
    };
    if path.join("objects").is_dir() && path.join("refs").exists() {
        return Some(path);
    }
    let dot_git = path.join(".git");
    if dot_git.join("objects").is_dir() && dot_git.join("refs").exists() {
        return Some(dot_git);
    }
    None
}

fn lfs_remote_media_path(remote: &LfsPullRemote, oid: &str) -> Option<PathBuf> {
    if oid.len() < 4 {
        return None;
    }
    Some(
        remote
            .git_dir
            .join("lfs")
            .join("objects")
            .join(&oid[..2])
            .join(&oid[2..4])
            .join(oid),
    )
}

fn lfs_is_regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
        .unwrap_or(false)
}

fn lfs_pull_missing_objects_error(_remote: Option<&LfsPullRemote>, missing: &[String]) -> CliError {
    let detail = if _remote.is_some() {
        format!(
            "error transferring \"{}\": [0] remote missing object {}\nFailed to fetch some objects",
            missing[0], missing[0]
        )
    } else {
        "batch request: missing protocol: \"\"\nFailed to fetch some objects".to_owned()
    };
    CliError::Stderr {
        code: 2,
        text: detail,
    }
}

fn lfs_untrack(args: &[String]) -> Result<()> {
    let repo = find_repo()?;
    if args.is_empty() {
        return Err(CliError::Fatal {
            code: 1,
            message: "git lfs untrack requires at least one pattern".into(),
        });
    }
    let attributes_path = lfs_worktree_path(&repo, ".gitattributes")?;
    if !lfs_path_exists(&attributes_path) {
        return Ok(());
    }
    let mut attributes = read_attributes_lines(&attributes_path)?;
    let mut changed = false;
    for pattern in args {
        validate_lfs_pattern(pattern)?;
        let entry = lfs_attribute_line(pattern);
        let before = attributes.lines.len();
        attributes.lines.retain(|line| line != &entry);
        if attributes.lines.len() != before {
            changed = true;
            println!("Untracking \"{pattern}\"");
        }
    }
    if changed {
        write_attributes_lines(&attributes_path, attributes)?;
    }
    Ok(())
}

fn lfs_ls_files(args: &[String]) -> Result<()> {
    let repo = find_repo()?;
    let config = lfs_runtime_config(&repo)?;
    let options = parse_lfs_ls_files_options(args)?;
    let algorithm = repo_hash_algorithm_from_config(&repo).map_err(CliError::Io)?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), algorithm);
    let records = lfs_ls_files_records(&repo, &store, config.storage(), &options)?;
    if options.json {
        print!("{}", lfs_ls_files_json(&records));
        return Ok(());
    }
    for record in &records {
        if options.debug {
            print!("{}", lfs_ls_files_debug_record(record));
            continue;
        }
        if options.name_only {
            println!("{}", record.path);
            continue;
        }
        let oid = if options.long {
            record.oid.clone()
        } else {
            record.oid[..10].to_owned()
        };
        let marker = if record.downloaded { '*' } else { '-' };
        if options.size {
            println!("{oid} {marker} {} ({} B)", record.path, record.size);
        } else {
            println!("{oid} {marker} {}", record.path);
        }
    }
    Ok(())
}

#[derive(Default)]
struct LfsLsFilesOptions {
    all: bool,
    debug: bool,
    deleted: bool,
    json: bool,
    long: bool,
    name_only: bool,
    ref_args: Vec<String>,
    size: bool,
}

fn parse_lfs_ls_files_options(args: &[String]) -> Result<LfsLsFilesOptions> {
    let mut options = LfsLsFilesOptions::default();
    for arg in args {
        match arg.as_str() {
            "-a" | "--all" => options.all = true,
            "-d" | "--debug" => options.debug = true,
            "-l" | "--long" => options.long = true,
            "-n" | "--name-only" => options.name_only = true,
            "-s" | "--size" => options.size = true,
            "--deleted" => options.deleted = true,
            "--json" => options.json = true,
            value if value.starts_with('-') => {
                print!("{}", lfs_ls_files_usage());
                eprintln!("Error: unknown flag: {value}");
                eprintln!();
                return Err(CliError::Exit(127));
            }
            other => {
                if options.ref_args.len() >= 2 {
                    options.ref_args.push(other.to_owned());
                    break;
                }
                options.ref_args.push(other.to_owned());
            }
        }
    }
    if options.all && !options.ref_args.is_empty() {
        return Err(CliError::Stderr {
            code: 2,
            text: "Cannot use --all with explicit reference\n".into(),
        });
    }
    if options.deleted && options.ref_args.len() == 2 {
        return Err(CliError::Stderr {
            code: 2,
            text: "Cannot use --deleted with reference range\n".into(),
        });
    }
    Ok(options)
}

fn lfs_ls_files_usage() -> &'static str {
    concat!(
        "git lfs ls-files [<ref>]\n",
        "git lfs ls-files <ref> <ref>\n\n",
        "Display paths of Git LFS files that are found in the tree at the given\n",
        "reference. If no reference is given, scan the currently checked-out\n",
        "branch. If two references are given, the LFS files that are modified\n",
        "between the two references are shown; deletions are not listed.\n\n",
        "An asterisk (*) after the OID indicates a full object, a minus (-)\n",
        "indicates an LFS pointer.\n\n",
        "Options:\n\n",
        "-l:\n",
        "--long:\n",
        "   Show the entire 64 character OID, instead of just first 10.\n",
        "-s:\n",
        "--size:\n",
        "   Show the size of the LFS object between parenthesis at the end of a line.\n",
        "-d:\n",
        "--debug:\n",
        "   Show as much information as possible about a LFS file. This is intended for\n",
        "   manual inspection; the exact format may change at any time.\n",
        "-a:\n",
        "--all:\n",
        "   Inspects the full history of the repository, not the current HEAD (or other\n",
        "   provided reference). This will include previous versions of LFS objects that\n",
        "   are no longer found in the current tree.\n",
        "--deleted:\n",
        "  Shows the full history of the given reference, including objects that have\n",
        "  been deleted.\n",
        "-I <paths>:\n",
        "--include=<paths>:\n",
        "   Include paths matching only these patterns; see \"Fetch settings\".\n",
        "-X <paths>:\n",
        "--exclude=<paths>:\n",
        "   Exclude paths matching any of these patterns; see \"Fetch settings\".\n",
        "-n:\n",
        "--name-only:\n",
        "   Show only the lfs tracked file names.\n"
    )
}

#[derive(Clone)]
struct LfsLsFilesRecord {
    path: String,
    oid: String,
    size: u64,
    downloaded: bool,
}

fn lfs_ls_files_records(
    repo: &GitRepo,
    store: &LooseObjectStore,
    storage: &Path,
    options: &LfsLsFilesOptions,
) -> Result<Vec<LfsLsFilesRecord>> {
    if options.all {
        return lfs_ls_files_all_records(repo, store, storage);
    }
    if options.deleted {
        return lfs_ls_files_deleted_records(
            repo,
            store,
            storage,
            options.ref_args.first().map(String::as_str),
        );
    }
    if options.ref_args.len() == 2 {
        let revs = collect_rev_list_revs(
            repo,
            store,
            false,
            vec![format!("{}..{}", options.ref_args[0], options.ref_args[1])],
        )?;
        let commits = collect_commits_with_exclusions(repo, store, &revs, None)?;
        let commit_cache = CommitObjectCache::new(store);
        let tree_cache = TreeObjectCache::new(store);
        let mut entries = Vec::new();
        for commit_id in commits {
            let commit = commit_cache.read_commit(&commit_id)?;
            let parent_index = if let Some(parent_id) = commit.parents.first() {
                tree_cache
                    .read_tree_to_index(&commit_cache.read_commit(parent_id)?.tree)
                    .map_err(CliError::Io)?
            } else {
                GitIndex::new()
            };
            let commit_index = tree_cache
                .read_tree_to_index(&commit.tree)
                .map_err(CliError::Io)?;
            let diff = zmin_git_core::diff::diff_indexes(&parent_index, &commit_index)
                .map_err(CliError::Io)?;
            for row in diff {
                if !matches!(
                    row.status,
                    zmin_git_core::diff::IndexDiffStatus::Added
                        | zmin_git_core::diff::IndexDiffStatus::Modified
                ) {
                    continue;
                }
                if let Some(entry) = commit_index.entry(&row.path, 0) {
                    if let Some(record) = lfs_ls_files_record(store, storage, entry) {
                        entries.push(record);
                    }
                }
            }
        }
        return Ok(entries);
    }
    if let Some(treeish) = options.ref_args.first().map(String::as_str) {
        return lfs_ls_files_tree_records(repo, store, storage, treeish);
    }
    if !repo.index_path.exists() {
        return Ok(Vec::new());
    }
    let algorithm = repo_hash_algorithm_from_config(repo).map_err(CliError::Io)?;
    let index = read_index_with_algorithm(&repo.index_path, algorithm).map_err(CliError::Io)?;
    Ok(index
        .entries()
        .iter()
        .filter(|entry| entry.stage == 0)
        .filter_map(|entry| lfs_ls_files_record(store, storage, entry))
        .collect())
}

fn lfs_ls_files_tree_records(
    repo: &GitRepo,
    store: &LooseObjectStore,
    storage: &Path,
    treeish: &str,
) -> Result<Vec<LfsLsFilesRecord>> {
    let tree = resolve_treeish_or_invalid_object(repo, store, treeish).map_err(|error| {
        let detail = match error {
            CliError::Fatal { message, .. } => message,
            CliError::Stderr { text, .. } => text.trim_end().to_owned(),
            CliError::Message(message) => message,
            CliError::Io(error) => error.to_string(),
            CliError::Exit(code) => format!("exit status {code}"),
        };
        CliError::Stderr {
            code: 2,
            text: format!(
                "Could not scan for Git LFS tree: error in `git ls-tree`: exit status 128 fatal: {detail}"
            ),
        }
    });
    let tree_cache = TreeObjectCache::new(store);
    let index = tree.and_then(|tree| tree_cache.read_tree_to_index(&tree).map_err(CliError::Io))?;
    Ok(index
        .entries()
        .iter()
        .filter(|entry| entry.stage == 0)
        .filter_map(|entry| lfs_ls_files_record(store, storage, entry))
        .collect())
}

fn lfs_ls_files_all_records(
    repo: &GitRepo,
    store: &LooseObjectStore,
    storage: &Path,
) -> Result<Vec<LfsLsFilesRecord>> {
    let revs = collect_rev_list_revs(repo, store, true, Vec::new())?;
    let commits = collect_commits_with_exclusions(repo, store, &revs, None)?;
    let commit_cache = CommitObjectCache::new(store);
    let tree_cache = TreeObjectCache::new(store);
    let mut seen = HashSet::new();
    let mut records = Vec::new();
    for commit_id in commits {
        let commit = commit_cache.read_commit(&commit_id)?;
        let index = tree_cache
            .read_tree_to_index(&commit.tree)
            .map_err(CliError::Io)?;
        for entry in index.entries().iter().filter(|entry| entry.stage == 0) {
            let Some(record) = lfs_ls_files_record(store, storage, entry) else {
                continue;
            };
            let key = (record.path.clone(), record.oid.clone());
            if seen.insert(key) {
                records.push(record);
            }
        }
    }
    Ok(records)
}

fn lfs_ls_files_deleted_records(
    repo: &GitRepo,
    store: &LooseObjectStore,
    storage: &Path,
    treeish: Option<&str>,
) -> Result<Vec<LfsLsFilesRecord>> {
    let target = treeish.unwrap_or("HEAD");
    let revs = collect_rev_list_revs(repo, store, false, vec![target.to_owned()])?;
    let commits = collect_commits_with_exclusions(repo, store, &revs, None)?;
    let commit_cache = CommitObjectCache::new(store);
    let tree_cache = TreeObjectCache::new(store);
    let mut seen_paths = HashSet::new();
    let mut records = Vec::new();
    for commit_id in commits {
        let commit = commit_cache.read_commit(&commit_id)?;
        let index = tree_cache
            .read_tree_to_index(&commit.tree)
            .map_err(CliError::Io)?;
        for entry in index.entries().iter().filter(|entry| entry.stage == 0) {
            let Some(record) = lfs_ls_files_record(store, storage, entry) else {
                continue;
            };
            if seen_paths.insert(record.path.clone()) {
                records.push(record);
            }
        }
    }
    Ok(records)
}

fn lfs_ls_files_record(
    store: &LooseObjectStore,
    storage: &Path,
    entry: &IndexEntry,
) -> Option<LfsLsFilesRecord> {
    let snapshot = store
        .read_object_prefix_or_full(&entry.id, LFS_POINTER_MAX_BYTES)
        .ok()?;
    if !snapshot.is_complete || snapshot.object.kind != GitObjectKind::Blob {
        return None;
    }
    let pointer = parse_lfs_pointer(&snapshot.object.content)?;
    let path = std::str::from_utf8(&entry.path).ok()?;
    lfs_validate_git_worktree_path(path).ok()?;
    Some(LfsLsFilesRecord {
        path: path.to_owned(),
        downloaded: lfs_local_object_exists(storage, pointer.oid().hex()),
        oid: pointer.oid().hex().to_owned(),
        size: pointer.size(),
    })
}

fn lfs_ls_files_debug_record(record: &LfsLsFilesRecord) -> String {
    format!(
        "filepath: {}\n    size: {}\ncheckout: false\ndownload: {}\n     oid: sha256 {}\n version: https://git-lfs.github.com/spec/v1\n\n",
        record.path, record.size, record.downloaded, record.oid
    )
}

fn lfs_ls_files_json(records: &[LfsLsFilesRecord]) -> String {
    if records.is_empty() {
        return "{\n \"files\": null\n}\n".into();
    }
    let mut out = String::from("{\n \"files\": [\n");
    for (index, record) in records.iter().enumerate() {
        if index > 0 {
            out.push_str(",\n");
        }
        out.push_str("  {\n");
        out.push_str(&format!(
            "   \"name\": \"{}\",\n   \"size\": {},\n   \"checkout\": false,\n   \"downloaded\": {},\n   \"oid_type\": \"sha256\",\n   \"oid\": \"{}\",\n   \"version\": \"https://git-lfs.github.com/spec/v1\"\n",
            lfs_json_escape(&record.path),
            record.size,
            if record.downloaded { "true" } else { "false" },
            record.oid
        ));
        out.push_str("  }");
    }
    out.push_str("\n ]\n}\n");
    out
}

fn lfs_json_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out
}

fn lfs_pre_push(args: &[String]) -> Result<()> {
    if args.is_empty() {
        println!(
            "This should be run through Git's pre-push hook.  Run `git lfs update` to install it."
        );
        return Err(CliError::Exit(1));
    }
    if args.len() > 2 {
        return Err(CliError::Fatal {
            code: 1,
            message: "usage: git lfs pre-push <remote> [remoteurl]".into(),
        });
    }
    if lfs_skip_push()? {
        return Ok(());
    }
    let repo = find_repo()?;
    let config = lfs_runtime_config(&repo)?;
    let named_remote = config
        .endpoint_inputs()
        .remote(&args[0])
        .map(|_| args[0].as_str());
    // Git's hook argument is an explicit transport destination even when it is
    // a URL rather than a configured remote name. Passing it as the requested
    // selector preserves global endpoint precedence while preventing implicit
    // autodetect/search-all policy from overriding that explicit destination.
    let remote = lfs_select_remote(&config, LfsOperation::Push, Some(&args[0]))?;
    let transport_url = if named_remote.is_some() {
        args.get(1).map(String::as_str)
    } else {
        args.get(1).or(args.first()).map(String::as_str)
    };
    lfs_validate_pre_push_destination(&repo, &config, &remote, transport_url)?;
    let repository =
        CliLfsReachabilityRepository::new(&repo, named_remote.or_else(|| remote.remote_name()))?;
    let planner = LfsReachabilityPlanner::new(
        &repository,
        config.fetch_filter(),
        config.remote_policy().into(),
        LfsReachabilityLimits::default(),
    )
    .map_err(lfs_reachability_error)?;
    let mut stdin = io::BufReader::new(io::stdin().lock());
    let plan = planner
        .plan_pre_push(&mut stdin)
        .map_err(lfs_reachability_error)?;
    lfs_upload_plan(&repo, &remote, config, &plan, transport_url)
}

fn lfs_post_checkout(args: &[String]) -> Result<()> {
    if args.len() != 3 {
        println!(
            "This should be run through Git's post-checkout hook.  Run `git lfs update` to install it."
        );
        return Err(CliError::Exit(1));
    }
    Ok(())
}

fn lfs_post_commit(_args: &[String]) -> Result<()> {
    Ok(())
}

fn lfs_post_merge(args: &[String]) -> Result<()> {
    if args.len() != 1 {
        println!(
            "This should be run through Git's post-merge hook.  Run `git lfs update` to install it."
        );
        return Err(CliError::Exit(1));
    }
    Ok(())
}

struct LfsAttributesDocument {
    lines: Vec<String>,
    #[cfg(windows)]
    snapshot: Option<super::lfs_windows_fs::WindowsFileSnapshot>,
    #[cfg(not(windows))]
    expected_identity: Option<(u64, u64, u64)>,
}

fn read_attributes_lines(path: &Path) -> Result<LfsAttributesDocument> {
    #[cfg(windows)]
    let (bytes, snapshot) = match super::lfs_windows_fs::open_regular_snapshot(path)? {
        Some(mut snapshot) => {
            let bytes = snapshot.read_bounded(LFS_ATTRIBUTES_MAX_BYTES)?;
            (bytes, Some(snapshot))
        }
        None => (Vec::new(), None),
    };

    #[cfg(not(windows))]
    let (bytes, expected_identity) = {
        let Some((mut file, initial_identity)) = lfs_open_regular_snapshot(path)? else {
            return Ok(LfsAttributesDocument {
                lines: Vec::new(),
                expected_identity: None,
            });
        };
        let mut bytes = Vec::new();
        (&mut file)
            .take(LFS_ATTRIBUTES_MAX_BYTES + 1)
            .read_to_end(&mut bytes)?;
        let final_metadata = file.metadata()?;
        let final_identity = lfs_open_file_identity(&file, &final_metadata)?;
        if final_identity != initial_identity || final_metadata.len() != bytes.len() as u64 {
            return Err(CliError::Fatal {
                code: 1,
                message: ".gitattributes changed while it was being read".into(),
            });
        }
        (bytes, Some(initial_identity))
    };

    if bytes.len() as u64 > LFS_ATTRIBUTES_MAX_BYTES {
        return Err(CliError::Fatal {
            code: 1,
            message: ".gitattributes is too large".into(),
        });
    }
    let content = String::from_utf8(bytes).map_err(|_| CliError::Fatal {
        code: 1,
        message: ".gitattributes is not valid UTF-8".into(),
    })?;
    Ok(LfsAttributesDocument {
        lines: content.lines().map(str::to_owned).collect(),
        #[cfg(windows)]
        snapshot,
        #[cfg(not(windows))]
        expected_identity,
    })
}

fn write_attributes_lines(path: &Path, document: LfsAttributesDocument) -> Result<()> {
    let content = if document.lines.is_empty() {
        String::new()
    } else {
        let mut content = document.lines.join("\n");
        content.push('\n');
        content
    };
    if content.len() as u64 > LFS_ATTRIBUTES_MAX_BYTES {
        return Err(CliError::Fatal {
            code: 1,
            message: ".gitattributes is too large".into(),
        });
    }

    #[cfg(windows)]
    {
        let mode = match document.snapshot {
            Some(snapshot) => super::lfs_windows_fs::WindowsPublishMode::Checked(snapshot),
            None => super::lfs_windows_fs::WindowsPublishMode::Absent,
        };
        return super::lfs_windows_fs::atomic_write(
            path,
            content.as_bytes(),
            mode,
            ".gitattributes",
        )
        .map_err(|error| {
            if error.kind() == io::ErrorKind::AlreadyExists {
                CliError::Fatal {
                    code: 1,
                    message: ".gitattributes changed while it was being updated".into(),
                }
            } else {
                CliError::Io(error)
            }
        });
    }
    #[cfg(not(windows))]
    {
        let parent = path.parent().ok_or_else(|| CliError::Fatal {
            code: 1,
            message: ".gitattributes has no parent directory".into(),
        })?;
        lfs_validate_directory_path(parent)?;
        let before = document.expected_identity;
        lfs_atomic_publish_unix(
            path,
            content.as_bytes(),
            0o644,
            before.is_some(),
            before,
            ".gitattributes",
        )
        .map_err(|error| {
            if error.kind() == io::ErrorKind::AlreadyExists {
                CliError::Fatal {
                    code: 1,
                    message: ".gitattributes changed while it was being updated".into(),
                }
            } else {
                CliError::Io(error)
            }
        })
    }
}

fn lfs_path_exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

#[cfg(not(windows))]
fn lfs_open_regular_snapshot(path: &Path) -> io::Result<Option<(fs::File, (u64, u64, u64))>> {
    #[cfg(unix)]
    {
        use std::ffi::CString;
        use std::os::fd::{AsRawFd, FromRawFd};
        use std::os::unix::ffi::OsStrExt;

        let parent = path.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "missing .gitattributes parent")
        })?;
        let parent_name = CString::new(parent.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in parent path"))?;
        let directory_fd = unsafe {
            libc::open(
                parent_name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                0,
            )
        };
        if directory_fd < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::NotFound {
                return Ok(None);
            }
            return Err(error);
        }
        let directory = unsafe { fs::File::from_raw_fd(directory_fd) };
        let name = CString::new(
            path.file_name()
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing filename"))?
                .as_bytes(),
        )
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in filename"))?;
        let file_fd = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                0,
            )
        };
        if file_fd < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::NotFound {
                return Ok(None);
            }
            return Err(error);
        }
        let file = unsafe { fs::File::from_raw_fd(file_fd) };
        let metadata = file.metadata()?;
        if !lfs_safe_regular_metadata(&metadata) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "unsafe .gitattributes file",
            ));
        }
        return Ok(Some((file, lfs_metadata_identity(&metadata))));
    }

    #[cfg(not(any(unix, windows)))]
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "descriptor-relative .gitattributes safety is unavailable on this platform",
    ))
}

fn lfs_safe_regular_metadata(metadata: &fs::Metadata) -> bool {
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return false;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return false;
        }
    }
    true
}

fn lfs_metadata_identity(metadata: &fs::Metadata) -> (u64, u64, u64) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        return (metadata.dev(), metadata.ino(), metadata.len());
    }
    #[cfg(windows)]
    {
        return (0, 0, metadata.len());
    }
    #[cfg(not(any(unix, windows)))]
    (metadata.len(), 0, 0)
}

#[cfg(not(windows))]
fn lfs_open_file_identity(file: &fs::File, metadata: &fs::Metadata) -> io::Result<(u64, u64, u64)> {
    let _ = file;
    Ok(lfs_metadata_identity(metadata))
}

fn validate_lfs_pattern(pattern: &str) -> Result<()> {
    if pattern.is_empty() || pattern.len() > 8192 || pattern.chars().any(char::is_control) {
        return Err(CliError::Fatal {
            code: 1,
            message: "invalid LFS path pattern".into(),
        });
    }
    Ok(())
}

fn lfs_attribute_line(pattern: &str) -> String {
    format!("{pattern}{LFS_ATTR_SUFFIX}")
}

fn parse_lfs_pointer(content: &[u8]) -> Option<LfsPointer> {
    LfsPointer::parse_current(content).ok()
}

fn lfs_local_object_exists(storage: &Path, oid: &str) -> bool {
    lfs_is_regular_file(&lfs_local_media_path(storage, oid))
}

fn lfs_local_media_path(storage: &Path, oid: &str) -> PathBuf {
    if oid.len() < 4 {
        return storage.join("objects").join(oid);
    }
    storage
        .join("objects")
        .join(&oid[..2])
        .join(&oid[2..4])
        .join(oid)
}

fn lfs_shell_quote_single(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn lfs_current_executable() -> Result<String> {
    let path = std::env::current_exe().map_err(CliError::Io)?;
    Ok(lfs_shell_quote_single(&path.display().to_string()))
}

#[cfg(test)]
mod tests {
    use super::{lfs_shell_quote_single, lfs_windows_unsafe_component};

    #[test]
    fn git_shell_quote_escapes_unix_apostrophes() {
        assert_eq!(
            lfs_shell_quote_single("/tmp/zmin's/bin"),
            "'/tmp/zmin'\\''s/bin'"
        );
    }

    #[test]
    fn git_shell_quote_preserves_windows_drive_backslashes() {
        assert_eq!(
            lfs_shell_quote_single(r"C:\Program Files\Zmin\zmin.exe"),
            r"'C:\Program Files\Zmin\zmin.exe'"
        );
    }

    #[test]
    fn git_shell_quote_preserves_windows_unc_backslashes() {
        assert_eq!(
            lfs_shell_quote_single(r"\\server\share\Zmin.exe"),
            r"'\\server\share\Zmin.exe'"
        );
    }

    #[test]
    fn windows_device_names_and_trimmed_components_are_rejected() {
        for component in [
            "CON",
            "con.txt",
            "PRN.log",
            "AUX",
            "NUL.data",
            "COM1.port",
            "com9",
            "LPT1.txt",
            "lpt9",
            "CON .txt",
            "name.",
            "name ",
        ] {
            assert!(
                lfs_windows_unsafe_component(component),
                "component should be rejected: {component:?}"
            );
        }
        for component in ["CONSOLE", "COM10", "LPT0", "normal.txt", "name"] {
            assert!(
                !lfs_windows_unsafe_component(component),
                "component should be accepted: {component:?}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn descriptor_relative_publish_rejects_changed_target() {
        use std::fs;
        use std::time::{SystemTime, UNIX_EPOCH};

        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock before epoch")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "zmin-lfs-publish-test-{}-{suffix}",
            std::process::id()
        ));
        fs::create_dir(&directory).expect("create test directory");
        let target = directory.join("target");
        fs::write(&target, b"custom").expect("write target");
        let result = super::lfs_atomic_publish_unix(
            &target,
            b"replacement",
            0o755,
            true,
            Some((0, 0, 0)),
            "test file",
        );
        assert!(result.is_err());
        assert_eq!(fs::read(&target).expect("read target"), b"custom");
        let no_replace = super::lfs_atomic_publish_unix(
            &target,
            b"replacement",
            0o755,
            false,
            None,
            "test file",
        );
        assert!(no_replace.is_err());
        assert_eq!(fs::read(&target).expect("read target"), b"custom");
        fs::remove_dir_all(&directory).expect("remove test directory");
    }

    #[cfg(unix)]
    #[test]
    fn attributes_publish_uses_the_identity_captured_during_read() {
        use std::fs;

        let directory = tempfile::TempDir::new().expect("temp directory");
        let target = directory.path().join(".gitattributes");
        fs::write(&target, b"old filter=lfs\n").expect("write attributes");
        let mut document = super::read_attributes_lines(&target).expect("read attributes");
        document.lines.push("new filter=lfs".into());
        fs::write(&target, b"concurrent edit\n").expect("concurrent edit");

        assert!(super::write_attributes_lines(&target, document).is_err());
        assert_eq!(
            fs::read(&target).expect("read concurrent attributes"),
            b"concurrent edit\n"
        );
    }
}
