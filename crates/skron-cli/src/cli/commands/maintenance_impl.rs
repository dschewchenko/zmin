use super::*;

const REPACK_CANDIDATE_INITIAL_CAPACITY_LIMIT: usize = 8192;

#[derive(Debug, Clone)]
struct RepackOptions {
    all: bool,
    all_and_loosen_unreachable: bool,
    delete_redundant: bool,
    quiet: bool,
    no_update_server_info: bool,
    no_reuse_delta: bool,
    no_reuse_object: bool,
    local: bool,
    write_bitmap_index: bool,
    no_write_bitmap_index: bool,
    write_midx: bool,
    no_write_midx: bool,
    window: Option<usize>,
    depth: Option<usize>,
    threads: Option<usize>,
    keep_pack: Vec<String>,
}

#[derive(Debug, Clone)]
struct GcOptions {
    prune: Option<String>,
    no_prune: bool,
    auto: bool,
    aggressive: bool,
    quiet: bool,
}

pub(crate) fn prune_packed_command(dry_run: bool, quiet: bool) -> Result<()> {
    prune_packed(dry_run, quiet)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn repack_command(
    all: bool,
    all_and_loosen_unreachable: bool,
    delete_redundant: bool,
    quiet: bool,
    no_update_server_info: bool,
    no_reuse_delta: bool,
    no_reuse_object: bool,
    local: bool,
    write_bitmap_index: bool,
    no_write_bitmap_index: bool,
    write_midx: bool,
    no_write_midx: bool,
    window: Option<usize>,
    depth: Option<usize>,
    threads: Option<usize>,
    keep_pack: Vec<String>,
) -> Result<()> {
    repack(RepackOptions {
        all,
        all_and_loosen_unreachable,
        delete_redundant,
        quiet,
        no_update_server_info,
        no_reuse_delta,
        no_reuse_object,
        local,
        write_bitmap_index,
        no_write_bitmap_index,
        write_midx,
        no_write_midx,
        window,
        depth,
        threads,
        keep_pack,
    })
}

pub(crate) fn gc_command(
    prune: Option<String>,
    no_prune: bool,
    auto: bool,
    aggressive: bool,
    quiet: bool,
) -> Result<()> {
    gc(GcOptions {
        prune,
        no_prune,
        auto,
        aggressive,
        quiet,
    })
}

pub(crate) fn prune_command(args: Vec<String>) -> Result<()> {
    prune(args)
}

pub(crate) struct MaintenanceOptions<'a> {
    pub(crate) operation: &'a str,
    pub(crate) auto: bool,
    pub(crate) schedule: Option<&'a str>,
    pub(crate) scheduler: Option<&'a str>,
    pub(crate) config_file: Option<&'a Path>,
    pub(crate) force: bool,
    pub(crate) quiet: bool,
    pub(crate) tasks: Vec<String>,
}

pub(crate) fn maintenance(options: MaintenanceOptions<'_>) -> Result<()> {
    let MaintenanceOptions {
        operation,
        auto,
        schedule,
        scheduler,
        config_file,
        force,
        quiet,
        tasks,
    } = options;
    if operation == "start" {
        return maintenance_start(scheduler, config_file);
    }
    if operation == "stop" {
        return maintenance_stop();
    }
    if operation == "register" {
        return maintenance_register(config_file);
    }
    if operation == "unregister" {
        return maintenance_unregister(config_file, force);
    }
    if operation != "run" {
        return Err(CliError::Stderr {
            code: 129,
            text: format!(
                "error: unknown subcommand: `{operation}'\nusage: git maintenance <subcommand> [<options>]\n\n"
            ),
        });
    }
    validate_maintenance_schedule(schedule, auto)?;
    let tasks = if tasks.is_empty() {
        maintenance_default_tasks(schedule)?
    } else {
        tasks
    };
    for task in tasks {
        match task.as_str() {
            "gc" => gc(GcOptions {
                prune: Some("now".to_owned()),
                no_prune: false,
                auto,
                aggressive: false,
                quiet,
            })?,
            "commit-graph" => {
                if !auto {
                    pack_commands::commit_graph_write(true)?;
                }
            }
            "pack-refs" => {
                if !auto {
                    reference_commands::pack_refs(true, true, false)?;
                }
            }
            "loose-objects" => {
                if !auto {
                    prune_packed(false, quiet)?;
                }
            }
            "incremental-repack" => {
                if !auto {
                    maintenance_incremental_repack()?;
                }
            }
            "prefetch" => maintenance_prefetch()?,
            _ => {
                return Err(CliError::Stderr {
                    code: 129,
                    text: format!("error: '{task}' is not a valid task\n"),
                });
            }
        }
    }
    Ok(())
}

fn validate_maintenance_schedule(schedule: Option<&str>, auto: bool) -> Result<()> {
    let Some(schedule) = schedule else {
        return Ok(());
    };
    match schedule {
        "hourly" | "daily" | "weekly" => {}
        other => {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("unrecognized --schedule argument '{other}'"),
            });
        }
    }
    if auto {
        return Err(CliError::Fatal {
            code: 128,
            message: "use at most one of --auto and --schedule=<frequency>".into(),
        });
    }
    Ok(())
}

fn maintenance_default_tasks(schedule: Option<&str>) -> Result<Vec<String>> {
    let repo = find_repo()?;
    let Some(schedule) = schedule else {
        return Ok(vec!["gc".to_owned()]);
    };
    if read_config_section_value(&repo, "maintenance", "", "strategy")?.as_deref()
        != Some("incremental")
    {
        return Ok(Vec::new());
    }
    let tasks = match schedule {
        "hourly" => vec!["commit-graph".to_owned()],
        "daily" => vec![
            "loose-objects".to_owned(),
            "incremental-repack".to_owned(),
            "commit-graph".to_owned(),
        ],
        "weekly" => vec![
            "loose-objects".to_owned(),
            "incremental-repack".to_owned(),
            "commit-graph".to_owned(),
            "pack-refs".to_owned(),
        ],
        _ => {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("unsupported maintenance schedule '{schedule}'"),
            });
        }
    };
    Ok(tasks)
}

fn maintenance_start(scheduler: Option<&str>, config_file: Option<&Path>) -> Result<()> {
    let scheduler = maintenance_scheduler_kind(scheduler)?;
    maintenance_register(config_file)?;
    match scheduler {
        #[cfg(target_os = "macos")]
        MaintenanceScheduler::Launchctl => maintenance_launchctl_start(),
        #[cfg(target_os = "linux")]
        MaintenanceScheduler::SystemdTimer => maintenance_systemd_start(),
        #[cfg(windows)]
        MaintenanceScheduler::Schtasks => maintenance_schtasks_start(),
        #[cfg(all(unix, not(target_os = "macos")))]
        MaintenanceScheduler::Crontab => maintenance_crontab_start(),
    }
}

fn maintenance_stop() -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        return maintenance_launchctl_stop();
    }
    #[cfg(target_os = "linux")]
    {
        if maintenance_systemd_units_exist()? || maintenance_systemd_available() {
            maintenance_systemd_stop()?;
        }
        if maintenance_crontab_available() {
            maintenance_crontab_stop()?;
        }
        return Ok(());
    }
    #[cfg(windows)]
    {
        return maintenance_schtasks_stop();
    }
    #[allow(unreachable_code)]
    Ok(())
}

fn maintenance_register(config_file: Option<&Path>) -> Result<()> {
    let repo = find_repo()?;
    set_config_value(&repo, "maintenance.auto", "false")?;
    set_config_value(&repo, "maintenance.strategy", "incremental")?;
    add_config_value_in_file_if_missing(
        &maintenance_config_path(config_file)?,
        "maintenance.repo",
        &maintenance_repo_path(&repo)?,
    )
}

fn maintenance_unregister(config_file: Option<&Path>, force: bool) -> Result<()> {
    let repo = find_repo()?;
    match remove_config_value_from_file(
        &maintenance_config_path(config_file)?,
        "maintenance.repo",
        &maintenance_repo_path(&repo)?,
    ) {
        Ok(()) => Ok(()),
        Err(CliError::Fatal { code: 128, .. }) if force => Ok(()),
        Err(error) => Err(error),
    }
}

fn maintenance_repo_path(repo: &GitRepo) -> Result<String> {
    Ok(fs::canonicalize(&repo.root)?.display().to_string())
}

fn global_config_path() -> Result<PathBuf> {
    Ok(user_global_config_dir()?.join(".gitconfig"))
}

#[cfg(windows)]
fn user_global_config_dir() -> Result<PathBuf> {
    if let Some(home) = std::env::var_os("HOME") {
        return Ok(PathBuf::from(home));
    }
    let Some(profile) = std::env::var_os("USERPROFILE") else {
        return Err(CliError::Fatal {
            code: 128,
            message: "%USERPROFILE% is unset".into(),
        });
    };
    Ok(PathBuf::from(profile))
}

#[cfg(not(windows))]
fn user_global_config_dir() -> Result<PathBuf> {
    let Some(home) = std::env::var_os("HOME") else {
        return Err(CliError::Fatal {
            code: 128,
            message: "$HOME is unset".into(),
        });
    };
    Ok(PathBuf::from(home))
}

fn maintenance_config_path(config_file: Option<&Path>) -> Result<PathBuf> {
    Ok(config_file
        .map(Path::to_path_buf)
        .unwrap_or(global_config_path()?))
}

#[derive(Clone, Copy)]
enum MaintenanceScheduler {
    #[cfg(target_os = "macos")]
    Launchctl,
    #[cfg(target_os = "linux")]
    SystemdTimer,
    #[cfg(windows)]
    Schtasks,
    #[cfg(all(unix, not(target_os = "macos")))]
    Crontab,
}

fn maintenance_scheduler_kind(scheduler: Option<&str>) -> Result<MaintenanceScheduler> {
    match scheduler.unwrap_or("auto") {
        "auto" => {
            #[cfg(target_os = "macos")]
            {
                return Ok(MaintenanceScheduler::Launchctl);
            }
            #[cfg(target_os = "linux")]
            {
                if maintenance_systemd_available() {
                    return Ok(MaintenanceScheduler::SystemdTimer);
                }
                if maintenance_crontab_available() {
                    return Ok(MaintenanceScheduler::Crontab);
                }
            }
            #[cfg(all(unix, not(target_os = "macos")))]
            {
                if maintenance_crontab_available() {
                    return Ok(MaintenanceScheduler::Crontab);
                }
            }
            #[cfg(windows)]
            {
                if maintenance_schtasks_available() {
                    return Ok(MaintenanceScheduler::Schtasks);
                }
            }
            #[allow(unreachable_code)]
            Err(CliError::Fatal {
                code: 128,
                message: "neither systemd timers nor crontab are available".into(),
            })
        }
        "launchctl" => {
            #[cfg(target_os = "macos")]
            {
                return Ok(MaintenanceScheduler::Launchctl);
            }
            #[allow(unreachable_code)]
            Err(CliError::Fatal {
                code: 128,
                message: "launchctl scheduler is not available".into(),
            })
        }
        "crontab" => {
            #[cfg(all(unix, not(target_os = "macos")))]
            {
                if maintenance_crontab_available() {
                    return Ok(MaintenanceScheduler::Crontab);
                }
            }
            #[allow(unreachable_code)]
            Err(CliError::Fatal {
                code: 128,
                message: "crontab scheduler is not available".into(),
            })
        }
        "systemd-timer" => {
            #[cfg(target_os = "linux")]
            {
                if maintenance_systemd_available() {
                    return Ok(MaintenanceScheduler::SystemdTimer);
                }
            }
            Err(CliError::Fatal {
                code: 128,
                message: "systemctl scheduler is not available".into(),
            })
        }
        "schtasks" => {
            #[cfg(windows)]
            {
                if maintenance_schtasks_available() {
                    return Ok(MaintenanceScheduler::Schtasks);
                }
            }
            Err(CliError::Fatal {
                code: 128,
                message: "schtasks scheduler is not available".into(),
            })
        }
        other => Err(CliError::Fatal {
            code: 128,
            message: format!("unsupported maintenance scheduler '{other}'"),
        }),
    }
}

fn maintenance_incremental_repack() -> Result<()> {
    let repo = find_repo()?;
    if pack_commands::multi_pack_index_pack_names(&repo.objects_dir.join("pack"))?.is_empty() {
        return Err(CliError::Stderr {
            code: 1,
            text: "error: no pack files to index.\nerror: failed to write multi-pack-index\nerror: task 'incremental-repack' failed\n".into(),
        });
    }
    pack_commands::multi_pack_index_write(&repo.objects_dir, true)
}

fn maintenance_prefetch() -> Result<()> {
    let repo = find_repo()?;
    for remote in remote_names(&repo)? {
        maintenance_prefetch_local_remote(&repo, &remote)?;
    }
    Ok(())
}

fn maintenance_prefetch_local_remote(repo: &GitRepo, remote: &str) -> Result<()> {
    let url = remote_url(repo, remote)?;
    if transport_commands::is_http_transport_url(&url) {
        return maintenance_prefetch_http_remote(repo, remote, &url);
    }
    if transport_commands::is_git_daemon_transport_url(&url) {
        return maintenance_prefetch_daemon_remote(repo, remote, &url);
    }
    if transport_commands::is_ssh_transport_url(&url) {
        return maintenance_prefetch_ssh_remote(repo, remote, &url);
    }
    let Some(source_path) = local_repository_path_from_location(&url)? else {
        let helper = remote_helper_protocol(&url).unwrap_or(url.as_str());
        return Err(CliError::Stderr {
            code: 1,
            text: format!(
                "git: 'remote-{helper}' is not a git command. See 'git --help'.\nfatal: remote helper '{helper}' aborted session\nerror: failed to prefetch remotes\nerror: task 'prefetch' failed\n"
            ),
        });
    };

    let source = local_clone_source(&source_path)?;
    let source_refs = RefStore::new(&source.git_dir, GitHashAlgorithm::Sha1);
    let destination_refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    copy_dir_contents(&source.git_dir.join("objects"), &repo.objects_dir)?;
    copy_prefetch_refs(&source_refs, &destination_refs, remote)
}

fn maintenance_prefetch_daemon_remote(repo: &GitRepo, remote: &str, url: &str) -> Result<()> {
    let rows = transport_commands::daemon_ls_remote_rows(url, false, false, false, &[])?;
    let roots = write_prefetch_refs_from_rows(repo, remote, &rows)?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let haves = transport_commands::collect_upload_pack_haves(&store, &refs)?;
    transport_commands::daemon_fetch_pack_with_haves(url, &repo.objects_dir, &roots, &haves)
}

fn maintenance_prefetch_ssh_remote(repo: &GitRepo, remote: &str, url: &str) -> Result<()> {
    let rows = transport_commands::ssh_ls_remote_rows(url, false, false, false, &[])?;
    let roots = write_prefetch_refs_from_rows(repo, remote, &rows)?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let haves = transport_commands::collect_upload_pack_haves(&store, &refs)?;
    transport_commands::ssh_fetch_pack_with_haves(url, &repo.objects_dir, &roots, &haves)
}

fn maintenance_prefetch_http_remote(repo: &GitRepo, remote: &str, url: &str) -> Result<()> {
    let parsed_url = transport_commands::ParsedHttpUrl::parse(url)?;
    let mut helper = transport_commands::RemoteHttpHelperSession::spawn_for_url(url)?;
    let rows = transport_commands::http_ls_remote_rows_with_helper(
        &parsed_url,
        &mut helper,
        false,
        false,
        false,
        &[],
    )?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let fetch_options = transport_commands::HttpFetchOptions {
        commit: false,
        tags: false,
        all: true,
        verbose: false,
        recover: false,
        write_ref: Vec::new(),
        stdin: false,
        packfile: None,
        index_pack_args: Vec::new(),
        args: Vec::new(),
    };
    let roots = write_prefetch_refs_from_rows(repo, remote, &rows)?;
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let haves = transport_commands::collect_upload_pack_haves(&store, &refs)?;
    let pack_fetched = transport_commands::http_fetch_smart_pack_with_helper(
        &parsed_url,
        &mut helper,
        &repo.objects_dir,
        &roots,
        &haves,
    )?;
    if !pack_fetched {
        let commit_cache = CommitObjectCache::new(&store);
        let tree_cache = TreeObjectCache::new(&store);
        let mut seen = HashSet::new();
        let mut fetch_context = transport_commands::HttpFetchObjectContext::new(
            &parsed_url,
            &mut helper,
            &store,
            &commit_cache,
            &tree_cache,
            &fetch_options,
            &mut seen,
        );
        for id in roots {
            transport_commands::http_fetch_object_recursive(&mut fetch_context, &id)?;
        }
    }
    Ok(())
}

fn write_prefetch_refs_from_rows(
    repo: &GitRepo,
    remote: &str,
    rows: &[transport_commands::LsRemoteRow],
) -> Result<Vec<ObjectId>> {
    let destination_refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let mut roots = Vec::new();
    for row in rows
        .iter()
        .filter(|row| row.name.starts_with("refs/heads/"))
    {
        let branch = row
            .name
            .strip_prefix("refs/heads/")
            .ok_or_else(|| CliError::Fatal {
                code: 128,
                message: format!("invalid source branch ref '{}'", row.name),
            })?;
        destination_refs.write_ref(&format!("refs/prefetch/remotes/{remote}/{branch}"), &row.id)?;
        roots.push(row.id.clone());
    }
    roots.sort_by_key(ObjectId::to_hex);
    roots.dedup_by(|left, right| left == right);
    Ok(roots)
}

fn remote_helper_protocol(url: &str) -> Option<&str> {
    url.find("://")
        .and_then(|index| (index > 0).then_some(&url[..index]))
}

#[cfg(target_os = "macos")]
fn maintenance_launchctl_start() -> Result<()> {
    let result = (|| {
        let agents_dir = maintenance_launch_agents_dir()?;
        fs::create_dir_all(&agents_dir)?;
        let minute = maintenance_schedule_minute();
        for schedule in ["hourly", "daily", "weekly"] {
            let path = maintenance_launchctl_plist_path(schedule)?;
            fs::write(&path, maintenance_launchctl_plist(schedule, minute)?)?;
        }
        Ok(())
    })();
    if result.is_err() {
        maintenance_launchctl_rollback();
    }
    result
}

#[cfg(target_os = "macos")]
fn maintenance_launchctl_stop() -> Result<()> {
    maintenance_launchctl_cleanup_plists()
}

#[cfg(target_os = "macos")]
fn maintenance_launchctl_rollback() {
    let _ = maintenance_launchctl_cleanup_plists();
}

#[cfg(target_os = "macos")]
fn maintenance_launchctl_cleanup_plists() -> Result<()> {
    for schedule in ["hourly", "daily", "weekly"] {
        let path = maintenance_launchctl_plist_path(schedule)?;
        remove_file_if_exists(&path)?;
    }
    Ok(())
}

#[cfg(test)]
fn maintenance_launchctl_cleanup_plan() -> Vec<String> {
    ["hourly", "daily", "weekly"]
        .into_iter()
        .map(|schedule| {
            maintenance_launchctl_plist_path(schedule)
                .expect("maintenance launchctl plist path")
                .display()
                .to_string()
        })
        .collect()
}

#[cfg(all(test, windows))]
fn maintenance_launch_agents_dir() -> Result<PathBuf> {
    Ok(windows_test_unix_scheduler_home_dir().join("Library/LaunchAgents"))
}

#[cfg(any(target_os = "macos", all(test, not(windows))))]
fn maintenance_launch_agents_dir() -> Result<PathBuf> {
    let Some(home) = std::env::var_os("HOME") else {
        return Err(CliError::Fatal {
            code: 128,
            message: "$HOME is unset".into(),
        });
    };
    Ok(PathBuf::from(home).join("Library/LaunchAgents"))
}

#[cfg(any(test, target_os = "macos"))]
fn maintenance_launchctl_plist_path(schedule: &str) -> Result<PathBuf> {
    Ok(maintenance_launch_agents_dir()?.join(format!("org.git-scm.git.{schedule}.plist")))
}

#[cfg(target_os = "macos")]
fn maintenance_schedule_minute() -> u8 {
    19
}

#[cfg(target_os = "linux")]
fn maintenance_systemd_start() -> Result<()> {
    let result = (|| {
        fs::create_dir_all(maintenance_systemd_user_dir()?)?;
        let current_exe = std::env::current_exe().map_err(CliError::Io)?;
        fs::write(
            maintenance_systemd_service_path()?,
            maintenance_systemd_service_unit(&current_exe),
        )?;
        for schedule in ["hourly", "daily", "weekly"] {
            fs::write(
                maintenance_systemd_timer_file_path(schedule)?,
                maintenance_systemd_timer_unit(schedule),
            )?;
            maintenance_systemctl_user([
                "enable",
                "--now",
                &format!("git-maintenance@{schedule}.timer"),
            ])?;
        }
        Ok(())
    })();
    if result.is_err() {
        maintenance_systemd_rollback();
    }
    result
}

#[cfg(target_os = "linux")]
fn maintenance_systemd_stop() -> Result<()> {
    if maintenance_systemd_available() {
        for schedule in ["hourly", "daily", "weekly"] {
            maintenance_systemctl_user([
                "disable",
                "--now",
                &format!("git-maintenance@{schedule}.timer"),
            ])?;
        }
    }
    for schedule in ["hourly", "daily", "weekly"] {
        remove_file_if_exists(&maintenance_systemd_timer_file_path(schedule)?)?;
    }
    remove_file_if_exists(&maintenance_systemd_service_path()?)?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn maintenance_systemd_rollback() {
    let _ = maintenance_systemd_cleanup_units();
}

#[cfg(target_os = "linux")]
fn maintenance_systemd_cleanup_units() -> Result<()> {
    for schedule in ["hourly", "daily", "weekly"] {
        remove_file_if_exists(&maintenance_systemd_timer_file_path(schedule)?)?;
    }
    remove_file_if_exists(&maintenance_systemd_service_path()?)?;
    Ok(())
}

#[cfg(test)]
fn maintenance_systemd_cleanup_plan() -> Vec<String> {
    let mut paths = ["hourly", "daily", "weekly"]
        .into_iter()
        .map(|schedule| {
            maintenance_systemd_timer_file_path(schedule)
                .expect("maintenance systemd timer path")
                .display()
                .to_string()
        })
        .collect::<Vec<_>>();
    paths.push(
        maintenance_systemd_service_path()
            .expect("maintenance systemd service path")
            .display()
            .to_string(),
    );
    paths
}

#[cfg(windows)]
fn maintenance_schtasks_start() -> Result<()> {
    let executable = std::env::current_exe().map_err(CliError::Io)?;
    let mut created = Vec::new();
    for schedule in ["hourly", "daily", "weekly"] {
        let xml_path = maintenance_schtasks_xml_path(schedule)?;
        fs::create_dir_all(xml_path.parent().ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "invalid schtasks xml parent path".into(),
        })?)?;
        fs::write(
            &xml_path,
            maintenance_schtasks_task_xml(&executable, schedule),
        )?;
        let create_result = maintenance_schtasks_run([
            "/create",
            "/tn",
            &maintenance_schtasks_task_name(schedule),
            "/f",
            "/xml",
            xml_path.to_str().ok_or_else(|| CliError::Fatal {
                code: 128,
                message: format!("non-utf8 schtasks xml path '{}'", xml_path.display()),
            })?,
        ]);
        if let Err(error) = create_result {
            maintenance_schtasks_rollback(&created);
            let _ = remove_file_if_exists(&xml_path);
            return Err(error);
        }
        created.push(schedule.to_owned());
    }
    Ok(())
}

#[cfg(windows)]
fn maintenance_schtasks_stop() -> Result<()> {
    maintenance_schtasks_cleanup(["hourly", "daily", "weekly"])
}

#[cfg(windows)]
fn maintenance_schtasks_rollback(created: &[String]) {
    let _ = maintenance_schtasks_cleanup(created.iter().map(String::as_str));
}

#[cfg(windows)]
fn maintenance_schtasks_cleanup<'a>(schedules: impl IntoIterator<Item = &'a str>) -> Result<()> {
    for schedule in schedules {
        let _ = maintenance_schtasks_run([
            "/delete",
            "/tn",
            &maintenance_schtasks_task_name(schedule),
            "/f",
        ]);
        remove_file_if_exists(&maintenance_schtasks_xml_path(schedule)?)?;
    }
    Ok(())
}

#[cfg(test)]
fn maintenance_schtasks_cleanup_plan<'a>(
    schedules: impl IntoIterator<Item = &'a str>,
) -> Vec<(String, String)> {
    schedules
        .into_iter()
        .map(|schedule| {
            (
                maintenance_schtasks_task_name(schedule),
                maintenance_schtasks_xml_path(schedule)
                    .expect("maintenance schtasks xml path")
                    .display()
                    .to_string(),
            )
        })
        .collect()
}

#[cfg(windows)]
fn maintenance_schtasks_run<const N: usize>(args: [&str; N]) -> Result<()> {
    let output = ProcessCommand::new("schtasks")
        .args(args)
        .output()
        .map_err(CliError::Io)?;
    if output.status.success() {
        return Ok(());
    }
    Err(CliError::Fatal {
        code: output.status.code().unwrap_or(1),
        message: String::from_utf8_lossy(&output.stderr)
            .trim_end()
            .to_owned(),
    })
}

#[cfg(any(test, windows))]
fn maintenance_schtasks_task_name(schedule: &str) -> String {
    format!("Git Maintenance ({schedule})")
}

#[cfg(any(test, windows, target_os = "macos"))]
fn xml_quote_text(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(any(test, windows))]
fn windows_command_xml_text(path: &Path) -> String {
    xml_quote_text(&format!("\"{}\"", path.display()))
}

#[cfg(any(test, windows))]
fn maintenance_schtasks_task_xml(executable: &Path, schedule: &str) -> String {
    let (start_boundary, repetition) = match schedule {
        "hourly" => (
            "2020-01-01T01:19:00",
            "<ScheduleByDay>\n<DaysInterval>1</DaysInterval>\n</ScheduleByDay>\n<Repetition>\n<Interval>PT1H</Interval>\n<Duration>PT23H</Duration>\n<StopAtDurationEnd>false</StopAtDurationEnd>\n</Repetition>\n".to_owned(),
        ),
        "daily" => (
            "2020-01-01T00:19:00",
            "<ScheduleByWeek>\n<DaysOfWeek>\n<Monday />\n<Tuesday />\n<Wednesday />\n<Thursday />\n<Friday />\n<Saturday />\n</DaysOfWeek>\n<WeeksInterval>1</WeeksInterval>\n</ScheduleByWeek>\n".to_owned(),
        ),
        "weekly" => (
            "2020-01-01T00:19:00",
            "<ScheduleByWeek>\n<DaysOfWeek>\n<Sunday />\n</DaysOfWeek>\n<WeeksInterval>1</WeeksInterval>\n</ScheduleByWeek>\n".to_owned(),
        ),
        other => panic!("unsupported maintenance schedule '{other}'"),
    };
    format!(
        "<?xml version=\"1.0\" ?>\n\
<Task version=\"1.4\" xmlns=\"http://schemas.microsoft.com/windows/2004/02/mit/task\">\n\
<Triggers>\n\
<CalendarTrigger>\n\
<StartBoundary>{start_boundary}</StartBoundary>\n\
<Enabled>true</Enabled>\n\
{repetition}\
</CalendarTrigger>\n\
</Triggers>\n\
<Principals>\n\
<Principal id=\"Author\">\n\
<LogonType>InteractiveToken</LogonType>\n\
<RunLevel>LeastPrivilege</RunLevel>\n\
</Principal>\n\
</Principals>\n\
<Settings>\n\
<MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>\n\
<Enabled>true</Enabled>\n\
<Hidden>true</Hidden>\n\
<UseUnifiedSchedulingEngine>true</UseUnifiedSchedulingEngine>\n\
<WakeToRun>false</WakeToRun>\n\
<ExecutionTimeLimit>PT72H</ExecutionTimeLimit>\n\
<Priority>7</Priority>\n\
</Settings>\n\
<Actions Context=\"Author\">\n\
<Exec>\n\
<Command>{}</Command>\n\
<Arguments>for-each-repo --keep-going --config=maintenance.repo maintenance run --schedule={schedule}</Arguments>\n\
</Exec>\n\
</Actions>\n\
</Task>\n",
        windows_command_xml_text(executable),
    )
}

#[allow(dead_code)]
#[cfg(any(test, windows))]
fn maintenance_schtasks_xml_path(schedule: &str) -> Result<PathBuf> {
    Ok(global_config_path()?.with_file_name(format!("maintenance-{schedule}.xml")))
}

#[allow(dead_code)]
#[cfg(any(test, windows))]
fn maintenance_schtasks_available() -> bool {
    ProcessCommand::new("schtasks")
        .args(["/query"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(target_os = "linux")]
fn maintenance_systemctl_user<const N: usize>(args: [&str; N]) -> Result<()> {
    let output = ProcessCommand::new("systemctl")
        .args(["--user"])
        .args(args)
        .output()
        .map_err(CliError::Io)?;
    if output.status.success() {
        return Ok(());
    }
    Err(CliError::Fatal {
        code: output.status.code().unwrap_or(1),
        message: String::from_utf8_lossy(&output.stderr)
            .trim_end()
            .to_owned(),
    })
}

#[cfg(target_os = "linux")]
fn maintenance_systemd_available() -> bool {
    ProcessCommand::new("systemctl")
        .args(["--user", "list-timers"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[allow(dead_code)]
#[cfg(any(test, all(unix, not(target_os = "macos"))))]
fn maintenance_crontab_available() -> bool {
    let Ok(output) = ProcessCommand::new("crontab").arg("-l").output() else {
        return false;
    };
    output.status.success() || String::from_utf8_lossy(&output.stderr).contains("no crontab for")
}

#[cfg(all(unix, not(target_os = "macos")))]
fn maintenance_crontab_start() -> Result<()> {
    let existing = maintenance_crontab_read()?;
    let minute = maintenance_schedule_minute();
    let replacement = maintenance_crontab_region(&std::env::current_exe()?, minute);
    let updated = maintenance_crontab_replace_region(&existing, &replacement);
    if updated != existing {
        maintenance_crontab_write(&updated)?;
    }
    Ok(())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn maintenance_crontab_stop() -> Result<()> {
    let existing = maintenance_crontab_read()?;
    let updated = maintenance_crontab_remove_region(&existing);
    if updated != existing {
        maintenance_crontab_write(&updated)?;
    }
    Ok(())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn maintenance_crontab_read() -> Result<String> {
    let output = ProcessCommand::new("crontab")
        .arg("-l")
        .output()
        .map_err(CliError::Io)?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("no crontab for") {
        eprint!("{stderr}");
        return Ok(String::new());
    }
    Err(CliError::Fatal {
        code: output.status.code().unwrap_or(1),
        message: stderr.trim_end().to_owned(),
    })
}

#[allow(dead_code)]
#[cfg(all(test, windows))]
fn maintenance_systemd_user_dir() -> Result<PathBuf> {
    Ok(windows_test_unix_scheduler_home_dir().join(".config/systemd/user"))
}

#[allow(dead_code)]
#[cfg(any(target_os = "linux", all(test, not(windows))))]
fn maintenance_systemd_user_dir() -> Result<PathBuf> {
    let Some(home) = std::env::var_os("HOME") else {
        return Err(CliError::Fatal {
            code: 128,
            message: "$HOME is unset".into(),
        });
    };
    Ok(PathBuf::from(home).join(".config/systemd/user"))
}

#[allow(dead_code)]
#[cfg(any(test, target_os = "linux"))]
fn maintenance_systemd_service_path() -> Result<PathBuf> {
    Ok(maintenance_systemd_user_dir()?.join("git-maintenance@.service"))
}

#[allow(dead_code)]
#[cfg(any(test, target_os = "linux"))]
fn maintenance_systemd_timer_file_path(schedule: &str) -> Result<PathBuf> {
    Ok(maintenance_systemd_user_dir()?.join(format!("git-maintenance@{schedule}.timer")))
}

#[cfg(target_os = "linux")]
fn maintenance_systemd_units_exist() -> Result<bool> {
    Ok(maintenance_systemd_service_path()?.exists()
        || ["hourly", "daily", "weekly"].into_iter().any(|schedule| {
            maintenance_systemd_timer_file_path(schedule).is_ok_and(|path| path.exists())
        }))
}

#[cfg(any(test, target_os = "linux"))]
fn systemd_quote_arg(path: &Path) -> String {
    format!(
        "\"{}\"",
        path.to_string_lossy()
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('%', "%%")
    )
}

#[cfg(any(test, target_os = "linux"))]
fn maintenance_systemd_service_unit(executable: &Path) -> String {
    let executable = systemd_quote_arg(executable);
    format!(
        "# This file was created and is maintained by Git.\n\
# Any edits made in this file might be replaced in the future\n\
# by a Git command.\n\
\n\
[Unit]\n\
Description=Optimize Git repositories data\n\
\n\
[Service]\n\
Type=oneshot\n\
ExecStart={executable} for-each-repo --keep-going --config=maintenance.repo maintenance run --schedule=%i\n\
LockPersonality=yes\n\
MemoryDenyWriteExecute=yes\n\
NoNewPrivileges=yes\n\
RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6 AF_VSOCK\n\
RestrictNamespaces=yes\n\
RestrictRealtime=yes\n\
RestrictSUIDSGID=yes\n\
SystemCallArchitectures=native\n\
SystemCallFilter=@system-service\n"
    )
}

#[cfg(any(test, target_os = "linux"))]
fn maintenance_systemd_timer_unit(schedule: &str) -> String {
    let on_calendar = match schedule {
        "hourly" => "*-*-* 1..23:19:00",
        "daily" => "Tue..Sun *-*-* 0:19:00",
        "weekly" => "Mon 0:19:00",
        other => panic!("unsupported maintenance schedule '{other}'"),
    };
    format!(
        "# This file was created and is maintained by Git.\n\
# Any edits made in this file might be replaced in the future\n\
# by a Git command.\n\
\n\
[Unit]\n\
Description=Optimize Git repositories data\n\
\n\
[Timer]\n\
OnCalendar={on_calendar}\n\
Persistent=true\n\
\n\
[Install]\n\
WantedBy=timers.target\n"
    )
}

#[cfg(all(unix, not(target_os = "macos")))]
fn maintenance_crontab_write(content: &str) -> Result<()> {
    let mut child = ProcessCommand::new("crontab")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(CliError::Io)?;
    child
        .stdin
        .as_mut()
        .ok_or_else(|| CliError::Fatal {
            code: 1,
            message: "failed to open crontab stdin".into(),
        })?
        .write_all(content.as_bytes())
        .map_err(CliError::Io)?;
    let output = child.wait_with_output().map_err(CliError::Io)?;
    if output.status.success() {
        return Ok(());
    }
    Err(CliError::Fatal {
        code: output.status.code().unwrap_or(1),
        message: String::from_utf8_lossy(&output.stderr)
            .trim_end()
            .to_owned(),
    })
}

#[cfg(all(unix, not(target_os = "macos")))]
fn maintenance_schedule_minute() -> u8 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| (duration.as_nanos() % 60) as u8)
        .unwrap_or(0)
}

#[cfg(any(test, all(unix, not(target_os = "macos"))))]
fn maintenance_crontab_region(executable: &Path, minute: u8) -> String {
    let executable = maintenance_shell_quote_path(executable);
    format!(
        "# BEGIN GIT MAINTENANCE SCHEDULE\n\
# The following schedule was created by Git\n\
# Any edits made in this region might be\n\
# replaced in the future by a Git command.\n\
\n\
{minute} 1-23 * * * {executable} for-each-repo --keep-going --config=maintenance.repo maintenance run --schedule=hourly\n\
{minute} 0 * * 1-6 {executable} for-each-repo --keep-going --config=maintenance.repo maintenance run --schedule=daily\n\
{minute} 0 * * 0 {executable} for-each-repo --keep-going --config=maintenance.repo maintenance run --schedule=weekly\n\
\n\
# END GIT MAINTENANCE SCHEDULE\n"
    )
}

#[cfg(all(test, windows))]
fn maintenance_shell_quote_path(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}

#[cfg(all(not(windows), any(test, all(unix, not(target_os = "macos")))))]
fn maintenance_shell_quote_path(path: &Path) -> String {
    diff_commands::shell_quote_path(path)
}

#[cfg(all(test, windows))]
fn windows_test_unix_scheduler_home_dir() -> PathBuf {
    PathBuf::from("C:/Users/skron-test")
}

#[cfg(any(test, all(unix, not(target_os = "macos"))))]
fn maintenance_crontab_replace_region(existing: &str, replacement: &str) -> String {
    let stripped = maintenance_crontab_remove_region(existing);
    if stripped.is_empty() {
        return replacement.to_owned();
    }
    let mut updated = stripped;
    if !updated.ends_with('\n') {
        updated.push('\n');
    }
    updated.push_str(replacement);
    updated
}

#[cfg(any(test, all(unix, not(target_os = "macos"))))]
fn maintenance_crontab_remove_region(existing: &str) -> String {
    const BEGIN: &str = "# BEGIN GIT MAINTENANCE SCHEDULE";
    const END: &str = "# END GIT MAINTENANCE SCHEDULE";

    let lines: Vec<&str> = existing.lines().collect();
    let mut kept = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        if lines[index] != BEGIN {
            kept.push(lines[index]);
            index += 1;
            continue;
        }

        let Some(end_offset) = lines[index + 1..].iter().position(|line| *line == END) else {
            kept.extend_from_slice(&lines[index..]);
            break;
        };
        index += end_offset + 2;
        while index < lines.len() && lines[index].is_empty() {
            index += 1;
        }
    }

    let mut cleaned = kept.join("\n");
    if !cleaned.is_empty() {
        cleaned.push('\n');
    }
    cleaned
}

#[cfg(test)]
mod maintenance_scheduler_tests {
    use super::{
        maintenance_crontab_region, maintenance_crontab_remove_region,
        maintenance_crontab_replace_region, maintenance_launchctl_cleanup_plan,
        maintenance_launchctl_plist_for_executable, maintenance_schtasks_cleanup_plan,
        maintenance_schtasks_task_name, maintenance_schtasks_task_xml,
        maintenance_systemd_cleanup_plan, maintenance_systemd_service_unit,
        maintenance_systemd_timer_unit,
    };
    use std::path::Path;

    #[test]
    fn crontab_region_contains_all_frequencies() {
        let region = maintenance_crontab_region(Path::new("/tmp/skron git"), 19);
        assert!(region.contains("# BEGIN GIT MAINTENANCE SCHEDULE"));
        assert!(region.contains("19 1-23 * * * '/tmp/skron git' for-each-repo --keep-going --config=maintenance.repo maintenance run --schedule=hourly"));
        assert!(region.contains("19 0 * * 1-6 '/tmp/skron git' for-each-repo --keep-going --config=maintenance.repo maintenance run --schedule=daily"));
        assert!(region.contains("19 0 * * 0 '/tmp/skron git' for-each-repo --keep-going --config=maintenance.repo maintenance run --schedule=weekly"));
    }

    #[test]
    fn crontab_replace_region_overwrites_existing_git_block() {
        let original = "MAILTO=dev@example.test\n\n# BEGIN GIT MAINTENANCE SCHEDULE\nold\n# END GIT MAINTENANCE SCHEDULE\n";
        let replacement = maintenance_crontab_region(Path::new("/tmp/skron"), 7);
        let updated = maintenance_crontab_replace_region(original, &replacement);
        assert!(
            updated.starts_with("MAILTO=dev@example.test\n\n# BEGIN GIT MAINTENANCE SCHEDULE\n")
        );
        assert!(!updated.contains("\nold\n"));
        assert!(updated.contains("7 1-23 * * * '/tmp/skron'"));
    }

    #[test]
    fn crontab_remove_region_preserves_non_git_lines() {
        let original = "MAILTO=dev@example.test\n# BEGIN GIT MAINTENANCE SCHEDULE\nold\n# END GIT MAINTENANCE SCHEDULE\n0 12 * * * echo keep\n";
        let updated = maintenance_crontab_remove_region(original);
        assert_eq!(updated, "MAILTO=dev@example.test\n0 12 * * * echo keep\n");
    }

    #[test]
    fn crontab_remove_region_preserves_unclosed_git_block() {
        let original = "MAILTO=dev@example.test\n# BEGIN GIT MAINTENANCE SCHEDULE\nold\n0 12 * * * echo keep\n";
        let updated = maintenance_crontab_remove_region(original);
        assert_eq!(updated, original);
    }

    #[test]
    fn systemd_service_template_uses_schedule_instance_and_security_flags() {
        let unit = maintenance_systemd_service_unit(Path::new("/tmp/skron git"));
        assert!(unit.contains("ExecStart=\"/tmp/skron git\" for-each-repo --keep-going --config=maintenance.repo maintenance run --schedule=%i"));
        assert!(unit.contains("LockPersonality=yes"));
        assert!(unit.contains("MemoryDenyWriteExecute=yes"));
        assert!(unit.contains("SystemCallFilter=@system-service"));
    }

    #[test]
    fn systemd_service_template_escapes_executable_specifiers() {
        let unit = maintenance_systemd_service_unit(Path::new("/tmp/skron %i \"git\""));
        assert!(unit.contains("ExecStart=\"/tmp/skron %%i \\\"git\\\"\" for-each-repo --keep-going --config=maintenance.repo maintenance run --schedule=%i"));
        assert!(!unit.contains("ExecStart=\"/tmp/skron %i \"git\"\""));
    }

    #[test]
    fn systemd_partial_start_cleanup_targets_only_git_units() {
        let cleanup = maintenance_systemd_cleanup_plan();
        assert_eq!(cleanup.len(), 4);
        assert!(
            cleanup
                .iter()
                .any(|path| path.ends_with("git-maintenance@hourly.timer"))
        );
        assert!(
            cleanup
                .iter()
                .any(|path| path.ends_with("git-maintenance@daily.timer"))
        );
        assert!(
            cleanup
                .iter()
                .any(|path| path.ends_with("git-maintenance@weekly.timer"))
        );
        assert!(
            cleanup
                .iter()
                .any(|path| path.ends_with("git-maintenance@.service"))
        );
        assert!(!cleanup.iter().any(|path| path.contains("default.target")));
    }

    #[test]
    fn systemd_timer_units_match_git_schedule_patterns() {
        assert!(maintenance_systemd_timer_unit("hourly").contains("OnCalendar=*-*-* 1..23:19:00"));
        assert!(
            maintenance_systemd_timer_unit("daily").contains("OnCalendar=Tue..Sun *-*-* 0:19:00")
        );
        assert!(maintenance_systemd_timer_unit("weekly").contains("OnCalendar=Mon 0:19:00"));
    }

    #[test]
    fn launchctl_plist_escapes_executable_path() {
        let plist = maintenance_launchctl_plist_for_executable(
            Path::new(r#"/tmp/Skron & Co/<bin>"quoted"/skron-git"#),
            "weekly",
            19,
        )
        .expect("launchctl plist");

        assert!(plist.contains(
            "<string>/tmp/Skron &amp; Co/&lt;bin&gt;&quot;quoted&quot;/skron-git</string>"
        ));
        assert!(!plist.contains("<string>/tmp/Skron & Co/<bin>"));
        assert!(plist.contains("<string>--schedule=weekly</string>"));
    }

    #[test]
    fn launchctl_partial_start_cleanup_targets_only_git_plists() {
        let cleanup = maintenance_launchctl_cleanup_plan();
        assert_eq!(cleanup.len(), 3);
        assert!(
            cleanup
                .iter()
                .any(|path| path.ends_with("org.git-scm.git.hourly.plist"))
        );
        assert!(
            cleanup
                .iter()
                .any(|path| path.ends_with("org.git-scm.git.daily.plist"))
        );
        assert!(
            cleanup
                .iter()
                .any(|path| path.ends_with("org.git-scm.git.weekly.plist"))
        );
        assert!(!cleanup.iter().any(|path| path.contains("com.apple.")));
    }

    #[test]
    fn schtasks_task_names_match_git_labels() {
        assert_eq!(
            maintenance_schtasks_task_name("hourly"),
            "Git Maintenance (hourly)"
        );
        assert_eq!(
            maintenance_schtasks_task_name("daily"),
            "Git Maintenance (daily)"
        );
        assert_eq!(
            maintenance_schtasks_task_name("weekly"),
            "Git Maintenance (weekly)"
        );
    }

    #[test]
    fn schtasks_partial_start_cleanup_targets_only_created_git_tasks() {
        let cleanup = maintenance_schtasks_cleanup_plan(["hourly", "daily"]);
        assert_eq!(cleanup.len(), 2);
        assert_eq!(cleanup[0].0, "Git Maintenance (hourly)");
        assert_eq!(cleanup[1].0, "Git Maintenance (daily)");
        assert!(cleanup[0].1.ends_with("maintenance-hourly.xml"));
        assert!(cleanup[1].1.ends_with("maintenance-daily.xml"));
        assert!(!cleanup.iter().any(|(name, path)| {
            name.contains("weekly") || path.ends_with("maintenance-weekly.xml")
        }));
    }

    #[test]
    fn schtasks_xml_contains_expected_schedule_patterns() {
        let hourly = maintenance_schtasks_task_xml(
            Path::new(r"C:\Program Files\Skron\skron-git.exe"),
            "hourly",
        );
        assert!(hourly.contains("<StartBoundary>2020-01-01T01:19:00</StartBoundary>"));
        assert!(hourly.contains("<Interval>PT1H</Interval>"));
        assert!(hourly.contains("<Duration>PT23H</Duration>"));
        assert!(hourly.contains(
            "<Arguments>for-each-repo --keep-going --config=maintenance.repo maintenance run --schedule=hourly</Arguments>"
        ));

        let daily = maintenance_schtasks_task_xml(Path::new(r"C:\skron-git.exe"), "daily");
        assert!(daily.contains("<Monday />"));
        assert!(daily.contains("<Saturday />"));
        assert!(!daily.contains("<Sunday />\n</DaysOfWeek>\n<WeeksInterval>1</WeeksInterval>\n</ScheduleByWeek>\n<Repetition>"));

        let weekly = maintenance_schtasks_task_xml(Path::new(r"C:\skron-git.exe"), "weekly");
        assert!(weekly.contains("<Sunday />"));
        assert!(weekly.contains("<UseUnifiedSchedulingEngine>true</UseUnifiedSchedulingEngine>"));
        assert!(weekly.contains("<Command>&quot;C:\\skron-git.exe&quot;</Command>"));
    }

    #[test]
    fn schtasks_xml_escapes_executable_path_and_keeps_arguments_static() {
        let xml = maintenance_schtasks_task_xml(
            Path::new(r#"C:\Skron & Co\<bin>"quoted"\skron-git.exe"#),
            "daily",
        );

        assert!(xml.contains(
            "<Command>&quot;C:\\Skron &amp; Co\\&lt;bin&gt;&quot;quoted&quot;\\skron-git.exe&quot;</Command>"
        ));
        assert!(xml.contains(
            "<Arguments>for-each-repo --keep-going --config=maintenance.repo maintenance run --schedule=daily</Arguments>"
        ));
        assert!(!xml.contains("<Command>\"C:\\Skron & Co\\<bin>"));
    }
}

#[cfg(target_os = "macos")]
fn maintenance_launchctl_plist(schedule: &str, minute: u8) -> Result<String> {
    let current_exe = std::env::current_exe().map_err(CliError::Io)?;
    maintenance_launchctl_plist_for_executable(&current_exe, schedule, minute)
}

#[cfg(any(test, target_os = "macos"))]
fn maintenance_launchctl_plist_for_executable(
    executable: &Path,
    schedule: &str,
    minute: u8,
) -> Result<String> {
    let executable = xml_quote_text(&executable.display().to_string());
    let start_intervals = match schedule {
        "hourly" => (1u8..=23)
            .map(|hour| format!("<dict>\n<key>Hour</key><integer>{hour}</integer>\n<key>Minute</key><integer>{minute}</integer>\n</dict>"))
            .collect::<Vec<_>>()
            .join("\n"),
        "daily" => format!(
            "<dict>\n<key>Hour</key><integer>0</integer>\n<key>Minute</key><integer>{minute}</integer>\n</dict>"
        ),
        "weekly" => format!(
            "<dict>\n<key>Weekday</key><integer>0</integer>\n<key>Hour</key><integer>0</integer>\n<key>Minute</key><integer>{minute}</integer>\n</dict>"
        ),
        other => {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("unsupported maintenance schedule '{other}'"),
            });
        }
    };
    Ok(format!(
        "<?xml version=\"1.0\"?>\n\
<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
<plist version=\"1.0\"><dict>\n\
<key>Label</key><string>org.git-scm.git.{schedule}</string>\n\
<key>ProgramArguments</key>\n\
<array>\n\
<string>{executable}</string>\n\
<string>for-each-repo</string>\n\
<string>--keep-going</string>\n\
<string>--config=maintenance.repo</string>\n\
<string>maintenance</string>\n\
<string>run</string>\n\
<string>--schedule={schedule}</string>\n\
</array>\n\
<key>StartCalendarInterval</key>\n\
<array>\n\
{start_intervals}\n\
</array>\n\
</dict>\n\
</plist>\n"
    ))
}

fn prune_packed(dry_run: bool, _quiet: bool) -> Result<()> {
    let repo = find_repo()?;
    let store = LooseObjectStore::new(&repo.objects_dir, GitHashAlgorithm::Sha1);
    let pruned = store.prune_packed(dry_run)?;
    if dry_run {
        for id in pruned {
            let path = store.loose_object_path(&id)?;
            println!("rm -f {}", git_relative_display(&repo, &path)?);
        }
    }
    Ok(())
}

fn repack(options: RepackOptions) -> Result<()> {
    let write_bitmap_index = options.write_bitmap_index && !options.no_write_bitmap_index;
    let write_midx = options.write_midx && !options.no_write_midx;
    if write_bitmap_index || options.threads.is_some() {
        return Err(CliError::Fatal {
            code: 129,
            message: "repack currently supports -a, -A, -d, -q, -n, -f, -F, -l, -m, --window, --depth, --no-write-bitmap-index, --no-write-midx and --keep-pack"
                .into(),
        });
    }
    let _ = (options.no_reuse_delta, options.no_reuse_object);
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let old_pack_names =
        PackedObjectStore::new(&repo.objects_dir, GitHashAlgorithm::Sha1).pack_names()?;
    let pack_dir = repo.objects_dir.join("pack");
    let keep_pack_names = normalize_keep_pack_names(&options.keep_pack);
    let keep_pack_object_ids = kept_pack_object_ids(&pack_dir, &old_pack_names, &keep_pack_names)?;
    let all_reachable = options.all || options.all_and_loosen_unreachable;
    let ids: Vec<ObjectId> = if all_reachable {
        let reachable = collect_reachable_objects(&repo, &store, &[])?;
        if options.all_and_loosen_unreachable {
            loosen_unreachable_packed_objects(&store, &reachable)?;
        }
        collect_repack_candidate_ids(
            &repo,
            &store,
            options.local,
            &reachable,
            &keep_pack_object_ids,
        )?
    } else {
        let mut ids = Vec::new();
        store.for_each_loose_object_id(&mut |id| {
            if !keep_pack_object_ids.contains(id) {
                ids.push(id.clone());
            }
            Ok(())
        })?;
        ids
    };
    if ids.is_empty() {
        if write_midx {
            pack_commands::multi_pack_index_write(&repo.objects_dir, false)?;
        }
        return Ok(());
    }
    fs::create_dir_all(&pack_dir)?;
    let packed_first_store = store.packed_first();
    let temp_pack = unique_temp_sibling(&pack_dir.join("pack-repack.pack"));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_pack)?;
        write_pack_from_store_with_options(
            &packed_first_store,
            GitHashAlgorithm::Sha1,
            &ids,
            pack_encode_options(options.window, options.depth),
            &mut file,
        )?;
        file.flush()?;
        Ok::<_, CliError>(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp_pack);
    }
    result?;
    let indexed = match index_pack_file(GitHashAlgorithm::Sha1, &temp_pack) {
        Ok(indexed) => indexed,
        Err(error) => {
            let _ = fs::remove_file(&temp_pack);
            return Err(CliError::Io(error));
        }
    };
    let pack_name = format!("pack-{}", indexed.pack_id.to_hex());
    install_temp_repack_file(
        &pack_dir.join(format!("{pack_name}.pack")),
        &temp_pack,
        &indexed,
    )?;
    write_content_addressed_file(&pack_dir.join(format!("{pack_name}.idx")), &indexed.index)?;
    write_content_addressed_file(
        &pack_dir.join(format!("{pack_name}.rev")),
        &indexed.reverse_index,
    )?;
    if options.delete_redundant {
        remove_replaced_pack_files(
            &pack_dir,
            &old_pack_names,
            &format!("{pack_name}.pack"),
            &keep_pack_names,
        )?;
        let _ = store.prune_packed(false)?;
    }
    if write_midx {
        pack_commands::multi_pack_index_write(&repo.objects_dir, false)?;
    } else if options.delete_redundant {
        remove_multi_pack_index(&pack_dir)?;
    }
    if !options.no_update_server_info {
        update_server_info()?;
    }
    let _ = options.quiet;
    Ok(())
}

fn install_temp_repack_file(
    path: &std::path::Path,
    temp_pack_path: &std::path::Path,
    indexed: &skron_git_core::IndexedPack,
) -> Result<()> {
    match fs::hard_link(temp_pack_path, path) {
        Ok(()) => {
            let _ = fs::remove_file(temp_pack_path);
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            let _ = fs::remove_file(temp_pack_path);
            if index_pack_file(GitHashAlgorithm::Sha1, path)
                .is_ok_and(|existing| existing.pack_id == indexed.pack_id)
            {
                Ok(())
            } else {
                Err(CliError::Io(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!("{} already exists with different contents", path.display()),
                )))
            }
        }
        Err(error) => {
            let _ = fs::remove_file(temp_pack_path);
            Err(CliError::Io(error))
        }
    }
}

fn kept_pack_object_ids(
    pack_dir: &std::path::Path,
    old_pack_names: &[String],
    keep_pack_names: &HashSet<String>,
) -> Result<HashSet<ObjectId>> {
    let mut ids = HashSet::new();
    for pack_name in old_pack_names {
        if !keep_pack_names.contains(pack_name) {
            continue;
        }
        let mut insert_id = |id: &ObjectId| {
            ids.insert(id.clone());
            Ok(())
        };
        for_each_pack_index_object_id_from_path(
            GitHashAlgorithm::Sha1,
            &pack_dir.join(pack_name).with_extension("idx"),
            &mut insert_id,
        )?;
    }
    Ok(ids)
}

fn normalize_keep_pack_names(keep_pack: &[String]) -> HashSet<String> {
    keep_pack
        .iter()
        .filter(|name| {
            name.ends_with(".pack")
                && !name.contains('/')
                && !name.contains(std::path::MAIN_SEPARATOR)
        })
        .cloned()
        .collect()
}

fn remove_multi_pack_index(pack_dir: &std::path::Path) -> Result<()> {
    match fs::remove_file(pack_dir.join("multi-pack-index")) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(CliError::Io(error)),
    }
}

fn remove_replaced_pack_files(
    pack_dir: &std::path::Path,
    old_pack_names: &[String],
    keep_pack_name: &str,
    keep_old_pack_names: &HashSet<String>,
) -> Result<()> {
    for pack_name in old_pack_names {
        if pack_name == keep_pack_name || keep_old_pack_names.contains(pack_name) {
            continue;
        }
        let pack_path = pack_dir.join(pack_name);
        for path in [
            pack_path.clone(),
            pack_path.with_extension("idx"),
            pack_path.with_extension("rev"),
        ] {
            match fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(CliError::Io(error)),
            }
        }
    }
    Ok(())
}

fn loosen_unreachable_packed_objects(
    store: &LooseObjectStore,
    reachable: &HashSet<ObjectId>,
) -> Result<()> {
    let packed_store = PackedObjectStore::new(store.objects_dir(), GitHashAlgorithm::Sha1);
    let mut loose_callback = |id: &ObjectId| -> std::io::Result<()> {
        if reachable.contains(id) || store.loose_object_path(id)?.is_file() {
            return Ok(());
        }
        let object = packed_store.read_object(id)?;
        store.write_object(object.kind, &object.content)?;
        Ok(())
    };
    packed_store
        .for_each_object_id(&mut loose_callback)
        .map_err(CliError::Io)?;
    Ok(())
}

fn collect_repack_candidate_ids(
    repo: &GitRepo,
    store: &LooseObjectStore,
    local: bool,
    reachable: &HashSet<ObjectId>,
    keep_pack_object_ids: &HashSet<ObjectId>,
) -> Result<Vec<ObjectId>> {
    if !local {
        return collect_repack_candidate_ids_in_walk_order(
            repo,
            store,
            reachable,
            keep_pack_object_ids,
        );
    }

    let mut ids = Vec::with_capacity(repack_candidate_initial_capacity(reachable.len()));
    let mut push_repack_candidate = |id: &ObjectId| -> io::Result<()> {
        if reachable.contains(id) && !keep_pack_object_ids.contains(id) {
            ids.push(id.clone());
        }
        Ok(())
    };
    if local {
        store.for_each_loose_object_id(&mut push_repack_candidate)?;
        ids.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
        ids.dedup_by(|left, right| left.as_bytes() == right.as_bytes());
        let loose_candidates = ids.iter().cloned().collect::<HashSet<_>>();
        PackedObjectStore::new(store.objects_dir(), GitHashAlgorithm::Sha1).for_each_object_id(
            &mut |id| {
                if reachable.contains(id)
                    && !keep_pack_object_ids.contains(id)
                    && !loose_candidates.contains(id)
                {
                    ids.push(id.clone());
                }
                Ok(())
            },
        )?;
    } else {
        store.for_each_object_id(&mut push_repack_candidate)?;
    }
    ids.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
    ids.dedup_by(|left, right| left.as_bytes() == right.as_bytes());
    Ok(ids)
}

fn collect_repack_candidate_ids_in_walk_order(
    repo: &GitRepo,
    store: &LooseObjectStore,
    reachable: &HashSet<ObjectId>,
    keep_pack_object_ids: &HashSet<ObjectId>,
) -> Result<Vec<ObjectId>> {
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let commit_cache = CommitObjectCache::new(store);
    let mut roots = Vec::with_capacity(repack_candidate_initial_capacity(reachable.len()));
    let mut extra_objects = Vec::with_capacity(repack_candidate_initial_capacity(reachable.len()));

    if let Ok(head) = refs.resolve("HEAD") {
        collect_repack_root_or_extra(store, &head, &mut roots, &mut extra_objects);
    }
    refs.for_each_resolved_ref("refs/", |_, id| {
        collect_repack_root_or_extra(store, id, &mut roots, &mut extra_objects);
        Ok::<(), CliError>(())
    })?;
    collect_reflog_roots(repo, &mut roots)?;

    let commits = collect_commits_from_ids_cached(repo, &commit_cache, &roots, None)?;
    let mut ids = Vec::with_capacity(repack_candidate_initial_capacity(reachable.len()));
    let mut seen = HashSet::with_capacity(repack_candidate_initial_capacity(reachable.len()));
    for commit in &commits {
        if reachable.contains(commit)
            && !keep_pack_object_ids.contains(commit)
            && seen.insert(commit.clone())
        {
            ids.push(commit.clone());
        }
    }

    let extra_objects = extra_objects
        .into_iter()
        .filter(|id| reachable.contains(id) && !keep_pack_object_ids.contains(id))
        .collect::<Vec<_>>();
    collect_rev_list_object_ids_into_cached(
        store,
        &commit_cache,
        &commits,
        &extra_objects,
        &[],
        &mut seen,
        &mut ids,
    )?;

    if repo.index_path.exists() {
        let index = read_index(&repo.index_path)?;
        for entry in index.entries() {
            if entry.mode != IndexMode::Gitlink
                && reachable.contains(&entry.id)
                && !keep_pack_object_ids.contains(&entry.id)
                && seen.insert(entry.id.clone())
            {
                ids.push(entry.id.clone());
            }
        }
    }

    Ok(ids)
}

fn collect_repack_root_or_extra(
    store: &LooseObjectStore,
    id: &ObjectId,
    roots: &mut Vec<ObjectId>,
    extra_objects: &mut Vec<ObjectId>,
) {
    if let Ok(kind) = store.read_object(id).map(|object| object.kind) {
        if kind == GitObjectKind::Commit {
            roots.push(id.clone());
        } else {
            extra_objects.push(id.clone());
        }
    }
}

fn repack_candidate_initial_capacity(count: usize) -> usize {
    count.min(REPACK_CANDIDATE_INITIAL_CAPACITY_LIMIT)
}

fn gc(options: GcOptions) -> Result<()> {
    if options.auto {
        return Ok(());
    }
    repack(RepackOptions {
        all: true,
        all_and_loosen_unreachable: false,
        delete_redundant: true,
        quiet: options.quiet,
        no_update_server_info: false,
        no_reuse_delta: false,
        no_reuse_object: false,
        local: false,
        write_bitmap_index: false,
        no_write_bitmap_index: false,
        write_midx: false,
        no_write_midx: false,
        window: options.aggressive.then_some(250),
        depth: options.aggressive.then_some(250),
        threads: None,
        keep_pack: Vec::new(),
    })?;
    if !options.no_prune {
        let mut args = Vec::new();
        if let Some(expire) = &options.prune {
            args.push("--expire".to_string());
            args.push(expire.clone());
        }
        prune(args)?;
    }
    Ok(())
}

#[derive(Default)]
struct PruneOptions {
    dry_run: bool,
    verbose: bool,
    expire: Option<String>,
    exclude_promisor_objects: bool,
    heads: Vec<String>,
}

fn prune(args: Vec<String>) -> Result<()> {
    let options = parse_prune_args(args)?;
    if options.exclude_promisor_objects {
        return Err(CliError::Fatal {
            code: 129,
            message: "--exclude-promisor-objects requires promisor pack support".into(),
        });
    }
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let Some(cutoff) = prune_expire_cutoff(options.expire.as_deref())? else {
        return Ok(());
    };
    let reachable = collect_reachable_objects(&repo, &store, &options.heads)?;
    store.for_each_loose_object_id(&mut |id| {
        if reachable.contains(id) {
            return Ok(());
        }
        let path = store.loose_object_path(id)?;
        if !object_is_expired(&path, cutoff)? {
            return Ok(());
        }
        let object = store.read_object(id)?;
        if options.dry_run || options.verbose {
            println!("{} {}", id.to_hex(), object.kind.as_str());
        }
        if !options.dry_run {
            fs::remove_file(&path)?;
            remove_empty_object_fanout_dir(&path)?;
        }
        Ok(())
    })?;
    Ok(())
}

fn parse_prune_args(args: Vec<String>) -> Result<PruneOptions> {
    let mut options = PruneOptions::default();
    let mut args = args.into_iter().peekable();
    let mut end_of_options = false;

    while let Some(arg) = args.next() {
        if end_of_options {
            options.heads.push(arg);
            continue;
        }
        match arg.as_str() {
            "--" => {
                end_of_options = true;
            }
            "-n" | "--dry-run" => options.dry_run = true,
            "--no-dry-run" => options.dry_run = false,
            "-v" | "--verbose" => options.verbose = true,
            "--no-verbose" => options.verbose = false,
            "--progress" | "--no-progress" => {}
            "--exclude-promisor-objects" => options.exclude_promisor_objects = true,
            "--no-exclude-promisor-objects" => options.exclude_promisor_objects = false,
            "--expire" => {
                let Some(value) = args.next() else {
                    return Err(CliError::Fatal {
                        code: 129,
                        message: "option `expire' requires a value".into(),
                    });
                };
                options.expire = Some(value);
            }
            "--no-expire" => options.expire = Some("never".into()),
            value if value.starts_with("--expire=") => {
                options.expire = Some(value["--expire=".len()..].to_string());
            }
            value if value.starts_with("--") => {
                return Err(CliError::Fatal {
                    code: 129,
                    message: format!("unknown option `{value}'"),
                });
            }
            value if value.starts_with('-') && value.len() > 1 => {
                for flag in value[1..].chars() {
                    match flag {
                        'n' => options.dry_run = true,
                        'v' => options.verbose = true,
                        _ => {
                            return Err(CliError::Fatal {
                                code: 129,
                                message: format!("unknown switch `{flag}'"),
                            });
                        }
                    }
                }
            }
            _ => options.heads.push(arg),
        }
    }

    Ok(options)
}

fn prune_expire_cutoff(expire: Option<&str>) -> Result<Option<std::time::SystemTime>> {
    let now = std::time::SystemTime::now();
    match expire {
        None => Ok(Some(
            now.checked_sub(std::time::Duration::from_secs(14 * 24 * 60 * 60))
                .unwrap_or(std::time::UNIX_EPOCH),
        )),
        Some("now" | "all") => Ok(Some(now)),
        Some("never") => Ok(None),
        Some(value) => Err(CliError::Fatal {
            code: 129,
            message: format!("unsupported prune expiry '{value}'"),
        }),
    }
}

fn object_is_expired(path: &std::path::Path, cutoff: std::time::SystemTime) -> io::Result<bool> {
    Ok(path.metadata()?.modified()? <= cutoff)
}

fn remove_empty_object_fanout_dir(path: &std::path::Path) -> io::Result<()> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    match fs::remove_dir(parent) {
        Ok(()) => Ok(()),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::DirectoryNotEmpty
            ) =>
        {
            Ok(())
        }
        Err(error) => Err(error),
    }
}

fn collect_reachable_objects(
    repo: &GitRepo,
    store: &LooseObjectStore,
    heads: &[String],
) -> Result<HashSet<ObjectId>> {
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let mut roots = Vec::new();
    if let Ok(id) = refs.resolve("HEAD") {
        roots.push(id);
    }
    refs.for_each_resolved_ref("refs/", |_, id| {
        roots.push(id.clone());
        Ok::<(), CliError>(())
    })?;
    collect_reflog_roots(repo, &mut roots)?;
    for head in heads {
        roots.push(resolve_objectish(repo, head)?);
    }
    if repo.index_path.exists() {
        let index = read_index(&repo.index_path)?;
        for entry in index.entries() {
            if entry.mode != IndexMode::Gitlink {
                roots.push(entry.id.clone());
            }
        }
    }

    Ok(collect_reachable_object_ids_from_roots(store, &roots)?
        .into_iter()
        .collect())
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn collect_reachable_objects_uses_loose_ref_over_packed_ref() {
        let repo = git_init();
        std::fs::write(repo.path().join("README.md"), b"base\n").expect("write file");
        git_env(&repo, ["add", "README.md"]);
        git_env(&repo, ["commit", "-m", "base"]);
        git(&repo, ["branch", "feature"]);
        git(&repo, ["pack-refs", "--all", "--prune"]);
        let packed_feature = git(&repo, ["rev-parse", "refs/heads/feature"]);

        git(&repo, ["checkout", "feature"]);
        std::fs::write(repo.path().join("README.md"), b"feature\n").expect("write file");
        git_env(&repo, ["commit", "-am", "feature"]);
        let loose_feature = git(&repo, ["rev-parse", "refs/heads/feature"]);
        assert_ne!(loose_feature, packed_feature);

        let git_repo = GitRepo {
            root: repo.path().to_path_buf(),
            git_dir: repo.path().join(".git"),
            objects_dir: repo.path().join(".git/objects"),
            index_path: repo.path().join(".git/index"),
        };
        let store = LooseObjectStore::new(git_repo.objects_dir.clone(), GitHashAlgorithm::Sha1);

        let reachable = collect_reachable_objects(&git_repo, &store, &[]).expect("reachable");

        assert!(reachable.contains(
            &ObjectId::from_hex(GitHashAlgorithm::Sha1, &loose_feature).expect("loose feature")
        ));
    }

    #[test]
    fn local_repack_candidates_skip_packed_duplicate_of_loose_object() {
        let repo = git_init();
        std::fs::write(repo.path().join("README.md"), b"base\n").expect("write file");
        git_env(&repo, ["add", "README.md"]);
        git_env(&repo, ["commit", "-m", "base"]);
        let id = duplicate_packed_head_as_loose(&repo);
        let object_id = ObjectId::from_hex(GitHashAlgorithm::Sha1, &id).expect("object id");
        let reachable = HashSet::from([object_id.clone()]);
        let git_repo = GitRepo {
            root: repo.path().to_path_buf(),
            git_dir: repo.path().join(".git"),
            objects_dir: repo.path().join(".git/objects"),
            index_path: repo.path().join(".git/index"),
        };
        let store = LooseObjectStore::new(git_repo.objects_dir.clone(), GitHashAlgorithm::Sha1);

        let candidates =
            collect_repack_candidate_ids(&git_repo, &store, true, &reachable, &HashSet::new())
                .expect("candidate ids");

        assert_eq!(candidates, vec![object_id]);
    }

    #[test]
    fn loosen_unreachable_reads_candidates_from_packed_store() {
        let repo = git_init();
        std::fs::write(repo.path().join("README.md"), b"base\n").expect("write file");
        git_env(&repo, ["add", "README.md"]);
        git_env(&repo, ["commit", "-m", "base"]);
        git(&repo, ["checkout", "-b", "tmpbranch"]);
        std::fs::write(repo.path().join("tmp.txt"), b"tmp\n").expect("write tmp");
        git_env(&repo, ["add", "tmp.txt"]);
        git_env(&repo, ["commit", "-m", "tmp"]);
        let unreachable = git(&repo, ["rev-parse", "HEAD"]);
        git(&repo, ["checkout", "main"]);
        git(&repo, ["repack", "-adq"]);
        git(&repo, ["branch", "-D", "tmpbranch"]);
        git(
            &repo,
            [
                "reflog",
                "expire",
                "--expire=now",
                "--expire-unreachable=now",
                "--all",
            ],
        );
        let store = LooseObjectStore::new(repo.path().join(".git/objects"), GitHashAlgorithm::Sha1);
        assert!(!loose_object_path(repo.path(), &unreachable).is_file());

        loosen_unreachable_packed_objects(&store, &HashSet::new()).expect("loosen packed objects");

        assert!(loose_object_path(repo.path(), &unreachable).is_file());
    }

    #[test]
    fn repack_candidate_initial_capacity_is_bounded() {
        assert_eq!(
            repack_candidate_initial_capacity(usize::MAX),
            REPACK_CANDIDATE_INITIAL_CAPACITY_LIMIT
        );
        assert_eq!(repack_candidate_initial_capacity(2), 2);
        assert_eq!(repack_candidate_initial_capacity(0), 0);
    }

    fn duplicate_packed_head_as_loose(repo: &TempDir) -> String {
        let id = git(repo, ["rev-parse", "HEAD"]);
        let loose_path = loose_object_path(repo.path(), &id);
        let copy_path = repo.path().join("duplicate-head-copy");
        std::fs::copy(&loose_path, &copy_path).expect("copy loose object");
        git(repo, ["repack", "-adq"]);
        std::fs::create_dir_all(loose_path.parent().expect("loose object parent"))
            .expect("create loose object dir");
        std::fs::copy(copy_path, &loose_path).expect("restore duplicate loose object");
        id
    }

    fn loose_object_path(repo: &std::path::Path, id: &str) -> std::path::PathBuf {
        repo.join(".git/objects").join(&id[..2]).join(&id[2..])
    }

    fn git_init() -> TempDir {
        let repo = TempDir::new().expect("temp repo");
        let output = Command::new("git")
            .arg("init")
            .args(["-b", "main"])
            .arg("--quiet")
            .current_dir(repo.path())
            .output()
            .expect("run git init");
        assert!(
            output.status.success(),
            "git init failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        repo
    }

    fn git<const N: usize>(repo: &TempDir, args: [&str; N]) -> String {
        let output = Command::new("git")
            .args(["-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(repo.path())
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout)
            .expect("git stdout utf8")
            .trim_end_matches('\n')
            .to_owned()
    }

    fn git_env<const N: usize>(repo: &TempDir, args: [&str; N]) {
        let output = Command::new("git")
            .args(["-c", "commit.gpgsign=false"])
            .args(args)
            .env("GIT_AUTHOR_NAME", "Skron Test")
            .env("GIT_AUTHOR_EMAIL", "skron@example.invalid")
            .env("GIT_AUTHOR_DATE", "1700000000 +0000")
            .env("GIT_COMMITTER_NAME", "Skron Test")
            .env("GIT_COMMITTER_EMAIL", "skron@example.invalid")
            .env("GIT_COMMITTER_DATE", "1700000000 +0000")
            .current_dir(repo.path())
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
