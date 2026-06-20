use super::*;

const LOCAL_TRANSPORT_INITIAL_CAPACITY_LIMIT: usize = 8192;
const PARALLEL_OBJECT_FILE_THRESHOLD: usize = 64;

#[derive(Debug, Clone)]
pub(crate) struct LocalCloneSource {
    pub(crate) git_dir: PathBuf,
    pub(crate) common_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub(crate) struct PushRef {
    pub(crate) id: Option<ObjectId>,
    pub(crate) destination: String,
    pub(crate) source_display: Option<String>,
    pub(crate) force: bool,
}

pub(crate) fn local_clone_source(path: &std::path::Path) -> Result<LocalCloneSource> {
    if path.is_file() {
        let git_dir = read_gitdir_file(path)?;
        if is_git_dir_or_linked_worktree_git_dir(&git_dir) {
            let common_dir = read_common_git_dir(&git_dir)?;
            return Ok(LocalCloneSource {
                git_dir,
                common_dir,
            });
        }
    }
    let dot_git = path.join(".git");
    if dot_git.is_dir() {
        let common_dir = read_common_git_dir(&dot_git)?;
        return Ok(LocalCloneSource {
            git_dir: dot_git,
            common_dir,
        });
    }
    if dot_git.is_file() {
        let git_dir = read_gitdir_file(&dot_git)?;
        if is_git_dir_or_linked_worktree_git_dir(&git_dir) {
            let common_dir = read_common_git_dir(&git_dir)?;
            return Ok(LocalCloneSource {
                git_dir,
                common_dir,
            });
        }
    }
    if path.join("objects").is_dir() && path.join("HEAD").is_file() {
        let common_dir = read_common_git_dir(path)?;
        return Ok(LocalCloneSource {
            git_dir: path.to_path_buf(),
            common_dir,
        });
    }
    Err(CliError::Fatal {
        code: 128,
        message: format!("repository '{}' does not exist", path.display()),
    })
}

pub(crate) fn default_clone_directory(source: &std::path::Path, bare: bool) -> Result<PathBuf> {
    let source_name = source
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "cannot infer clone directory".into(),
        })?;
    let name = if bare {
        if source_name.ends_with(".git") {
            source_name.to_owned()
        } else {
            format!("{source_name}.git")
        }
    } else {
        source_name.trim_end_matches(".git").to_owned()
    };
    Ok(std::env::current_dir()?.join(name))
}

pub(crate) fn clone_destination_label(
    arg: Option<&std::path::Path>,
    destination: &std::path::Path,
) -> String {
    arg.map(|path| path.display().to_string())
        .unwrap_or_else(|| {
            destination
                .file_name()
                .and_then(|value| value.to_str())
                .map(str::to_owned)
                .unwrap_or_else(|| destination.display().to_string())
        })
}

pub(crate) fn clone_recurse_submodule_specs(
    raw_args: &[String],
    recurse_submodules: Vec<String>,
    recursive: Vec<String>,
    no_recurse_submodules: bool,
) -> Vec<String> {
    if raw_args.first().is_some_and(|arg| arg == "clone") {
        let mut specs = Vec::new();
        for arg in &raw_args[1..] {
            if arg == "--" {
                break;
            }
            if arg == "--no-recurse-submodules" {
                specs.clear();
            } else if arg == "--recurse-submodules" || arg == "--recursive" {
                specs.push(".".to_owned());
            } else if let Some(spec) = arg.strip_prefix("--recurse-submodules=") {
                specs.push(if spec.is_empty() { "." } else { spec }.to_owned());
            } else if let Some(spec) = arg.strip_prefix("--recursive=") {
                specs.push(if spec.is_empty() { "." } else { spec }.to_owned());
            }
        }
        specs
    } else if no_recurse_submodules {
        Vec::new()
    } else {
        recurse_submodules
            .into_iter()
            .chain(recursive)
            .collect::<Vec<_>>()
    }
}

pub(crate) fn clone_reject_shallow(raw_args: &[String], reject: bool, no_reject: bool) -> bool {
    if raw_args.first().is_some_and(|arg| arg == "clone") {
        let mut reject_shallow = false;
        for arg in &raw_args[1..] {
            if arg == "--" {
                break;
            }
            if arg == "--reject-shallow" {
                reject_shallow = true;
            } else if arg == "--no-reject-shallow" {
                reject_shallow = false;
            }
        }
        reject_shallow
    } else {
        reject && !no_reject
    }
}

pub(crate) fn clone_no_tags(raw_args: &[String], no_tags: bool, tags: bool) -> bool {
    if raw_args.first().is_some_and(|arg| arg == "clone") {
        let mut effective_no_tags = false;
        for arg in &raw_args[1..] {
            if arg == "--" {
                break;
            }
            if arg == "--no-tags" {
                effective_no_tags = true;
            } else if arg == "--tags" {
                effective_no_tags = false;
            }
        }
        effective_no_tags
    } else {
        no_tags && !tags
    }
}

pub(crate) fn clone_no_checkout(raw_args: &[String], no_checkout: bool, checkout: bool) -> bool {
    if raw_args.first().is_some_and(|arg| arg == "clone") {
        let mut effective_no_checkout = false;
        for arg in &raw_args[1..] {
            if arg == "--" {
                break;
            }
            if arg == "--no-checkout" || arg == "-n" {
                effective_no_checkout = true;
            } else if arg == "--checkout" {
                effective_no_checkout = false;
            }
        }
        effective_no_checkout
    } else {
        no_checkout && !checkout
    }
}

pub(crate) fn clone_worktree_first(worktree_first: bool, instant: bool) -> bool {
    worktree_first || instant
}

pub(crate) fn clone_no_hardlinks(raw_args: &[String], no_hardlinks: bool, hardlinks: bool) -> bool {
    if raw_args.first().is_some_and(|arg| arg == "clone") {
        let mut effective_no_hardlinks = false;
        for arg in &raw_args[1..] {
            if arg == "--" {
                break;
            }
            if arg == "--no-hardlinks" {
                effective_no_hardlinks = true;
            } else if arg == "--hardlinks" {
                effective_no_hardlinks = false;
            }
        }
        effective_no_hardlinks
    } else {
        no_hardlinks && !hardlinks
    }
}

pub(crate) fn clone_template_path(
    raw_args: &[String],
    template: Option<PathBuf>,
    no_template: bool,
) -> Option<PathBuf> {
    if raw_args.first().is_some_and(|arg| arg == "clone") {
        let mut effective = None;
        let mut index = 1;
        while index < raw_args.len() {
            let arg = &raw_args[index];
            if arg == "--" {
                break;
            }
            if arg == "--no-template" {
                effective = None;
            } else if arg == "--template" {
                if let Some(path) = raw_args.get(index + 1) {
                    effective = Some(PathBuf::from(path));
                    index += 1;
                }
            } else if let Some(path) = arg.strip_prefix("--template=") {
                effective = Some(PathBuf::from(path));
            }
            index += 1;
        }
        effective
    } else if no_template {
        None
    } else {
        template
    }
}

pub(crate) fn clone_single_branch_flags(
    raw_args: &[String],
    single_branch: bool,
    no_single_branch: bool,
    depth: bool,
) -> (bool, bool) {
    if raw_args.first().is_some_and(|arg| arg == "clone") {
        let mut effective_single_branch = depth;
        for arg in &raw_args[1..] {
            if arg == "--" {
                break;
            }
            if arg == "--single-branch" {
                effective_single_branch = true;
            } else if arg == "--no-single-branch" {
                effective_single_branch = false;
            }
        }
        (effective_single_branch, !effective_single_branch)
    } else {
        (single_branch, no_single_branch)
    }
}

pub(crate) fn validate_clone_jobs(jobs: Option<&str>, raw_args: &[String]) -> Result<()> {
    let Some(jobs) = jobs else {
        return Ok(());
    };
    if is_git_integer_with_optional_suffix(jobs) {
        return Ok(());
    }
    let subject = clone_jobs_error_subject(raw_args, jobs);
    Err(CliError::Stderr {
        code: 129,
        text: format!("error: {subject} expects an integer value with an optional k/m/g suffix\n"),
    })
}

pub(crate) fn is_git_integer_with_optional_suffix(value: &str) -> bool {
    let value = value.trim();
    if value.is_empty() {
        return false;
    }
    let numeric = value
        .strip_suffix(['k', 'm', 'g', 'K', 'M', 'G'])
        .unwrap_or(value);
    let digits = numeric
        .strip_prefix(['+', '-'])
        .filter(|rest| !rest.is_empty())
        .unwrap_or(numeric);
    digits.bytes().all(|byte| byte.is_ascii_digit())
}

fn clone_jobs_error_subject<'a>(raw_args: &'a [String], value: &str) -> &'a str {
    if raw_args.first().is_some_and(|arg| arg == "clone") {
        let args = &raw_args[1..];
        for index in 0..args.len() {
            let arg = &args[index];
            if arg == "--" {
                break;
            }
            if arg == "-j" && args.get(index + 1).is_some_and(|next| next == value) {
                return "switch `j'";
            }
            if arg.starts_with("-j") && !arg.starts_with("--") {
                return "switch `j'";
            }
            if arg == "--jobs" && args.get(index + 1).is_some_and(|next| next == value) {
                return "option `jobs'";
            }
            if arg
                .strip_prefix("--jobs=")
                .is_some_and(|next| next == value)
            {
                return "option `jobs'";
            }
        }
    }
    "option `jobs'"
}

pub(crate) fn relocate_separate_git_dir(
    worktree: &std::path::Path,
    current_git_dir: &std::path::Path,
    requested_git_dir: &std::path::Path,
) -> Result<PathBuf> {
    let git_dir = absolute_path_from_arg(requested_git_dir)?;
    if git_dir.exists() {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("repository path '{}' already exists", git_dir.display()),
        });
    }
    if let Some(parent) = git_dir.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::rename(current_git_dir, &git_dir)?;
    fs::write(
        worktree.join(".git"),
        format!("gitdir: {}\n", git_dir.display()),
    )?;
    Ok(git_dir)
}

pub(crate) fn reference_object_dirs(references: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut object_dirs = Vec::with_capacity(reference_object_dirs_capacity_hint(references));
    for reference in references {
        let path = absolute_path_from_arg(reference)?;
        let source = local_clone_source(&path).map_err(|_| CliError::Fatal {
            code: 128,
            message: format!(
                "reference repository '{}' is not a local repository.",
                reference.display()
            ),
        })?;
        object_dirs.push(canonical_or_absolute(source.common_dir.join("objects")));
    }
    Ok(object_dirs)
}

pub(crate) fn reference_if_able_object_dirs(references: &[PathBuf]) -> Vec<PathBuf> {
    let mut object_dirs = Vec::with_capacity(reference_object_dirs_capacity_hint(references));
    for reference in references {
        let path = match absolute_path_from_arg(reference) {
            Ok(path) => path,
            Err(_) => {
                eprintln!(
                    "info: Could not add alternate for '{}': reference repository '{}' is not a local repository.",
                    reference.display(),
                    reference.display()
                );
                continue;
            }
        };
        match local_clone_source(&path) {
            Ok(source) => {
                object_dirs.push(canonical_or_absolute(source.common_dir.join("objects")))
            }
            Err(_) => {
                eprintln!(
                    "info: Could not add alternate for '{}': reference repository '{}' is not a local repository.",
                    reference.display(),
                    reference.display()
                );
            }
        }
    }
    object_dirs
}

fn reference_object_dirs_capacity_hint(references: &[PathBuf]) -> usize {
    references.len()
}

pub(crate) fn apply_clone_configs(repo: &GitRepo, configs: &[String]) -> Result<()> {
    for config in configs {
        let (name, value) = parse_clone_config_assignment(config)?;
        if let Err(error) = set_config_value(repo, &name, &value) {
            return Err(match error {
                CliError::Fatal { message, .. } => clone_config_error(message),
                other => other,
            });
        }
    }
    Ok(())
}

pub(crate) fn apply_clone_template(repo: &GitRepo, template: &std::path::Path) -> Result<()> {
    let template = absolute_path_from_arg(template)?;
    if !template.is_dir() {
        eprintln!("warning: templates not found in {}", template.display());
        return Ok(());
    }
    copy_template_contents(&template, &repo.git_dir)?;
    merge_template_config(repo, &template.join("config"))?;
    Ok(())
}

fn copy_template_contents(template: &std::path::Path, git_dir: &std::path::Path) -> Result<()> {
    for entry in fs::read_dir(template)? {
        let entry = entry?;
        let source = entry.path();
        let target = git_dir.join(entry.file_name());
        if entry.file_name() == "config" {
            continue;
        }
        let metadata = entry.metadata()?;
        if metadata.is_dir() {
            fs::create_dir_all(&target)?;
            copy_template_contents(&source, &target)?;
        } else if metadata.is_file() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&source, &target)?;
        }
    }
    Ok(())
}

fn merge_template_config(repo: &GitRepo, template_config: &std::path::Path) -> Result<()> {
    if !template_config.exists() {
        return Ok(());
    }
    let mut entries = read_config_file(template_config)?;
    entries.extend(read_common_config_entries(repo)?);
    write_common_config_entries(repo, &entries)?;
    Ok(())
}

fn parse_clone_config_assignment(raw: &str) -> Result<(String, String)> {
    let (name, value) = raw.split_once('=').unwrap_or((raw, "true"));
    validate_clone_config_key(name)?;
    Ok((name.to_owned(), value.to_owned()))
}

fn validate_clone_config_key(name: &str) -> Result<()> {
    let Some((section, variable)) = name.rsplit_once('.') else {
        return Err(clone_config_error(format!(
            "key does not contain a section: {name}"
        )));
    };
    if section.is_empty() || section.starts_with('.') {
        return Err(clone_config_error(format!(
            "key does not contain a section: {name}"
        )));
    }
    if variable.is_empty() {
        return Err(clone_config_error(format!(
            "key does not contain variable name: {name}"
        )));
    }
    if name.contains(['\n', '\r', '\0']) {
        return Err(clone_config_error(format!("invalid config key: {name}")));
    }
    Ok(())
}

fn clone_config_error(message: impl Into<String>) -> CliError {
    CliError::Stderr {
        code: 128,
        text: format!(
            "error: {}\nfatal: unable to write parameters to config file\n",
            message.into()
        ),
    }
}

pub(crate) fn cleanup_failed_clone_config(
    destination: &std::path::Path,
    git_dir: &std::path::Path,
    destination_existed: bool,
) {
    if destination_existed {
        let _ = fs::remove_dir_all(git_dir);
    } else {
        let _ = fs::remove_dir_all(destination);
    }
}

pub(crate) fn is_shallow_git_dir(git_dir: &std::path::Path) -> bool {
    let file = match fs::File::open(git_dir.join("shallow")) {
        Ok(file) => file,
        Err(_) => return false,
    };
    let mut reader = io::BufReader::new(file);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => return false,
            Ok(_) if !line.trim().is_empty() => return true,
            Ok(_) => {}
            Err(_) => return false,
        }
    }
}

pub(crate) fn write_alternates_file(
    objects_dir: &std::path::Path,
    alternates: &[PathBuf],
) -> Result<()> {
    if alternates.is_empty() {
        return Ok(());
    }
    fs::create_dir_all(objects_dir.join("info"))?;
    let mut content = String::with_capacity(alternates_file_content_capacity_hint(alternates));
    use std::fmt::Write as _;
    for alternate in alternates {
        writeln!(&mut content, "{}", alternate_path_line(alternate))
            .expect("writing alternate path to String cannot fail");
    }
    fs::write(objects_dir.join("info/alternates"), content)?;
    Ok(())
}

fn alternate_path_line(path: &Path) -> String {
    let value = path.display().to_string();
    if cfg!(windows) {
        normalize_windows_alternate_path(value.replace('\\', "/"))
    } else {
        value
    }
}

fn normalize_windows_alternate_path(value: String) -> String {
    if let Some(rest) = value.strip_prefix("//?/UNC/") {
        return format!("//{rest}");
    }
    if let Some(rest) = value.strip_prefix("//?/") {
        return rest.to_owned();
    }
    value
}

fn alternates_file_content_capacity_hint(alternates: &[PathBuf]) -> usize {
    alternates.iter().fold(0_usize, |capacity, alternate| {
        capacity.saturating_add(alternate.to_string_lossy().len() + 1)
    })
}

pub(crate) fn ensure_clone_destination(path: &std::path::Path, label: &str) -> Result<()> {
    match fs::read_dir(path) {
        Ok(mut entries) => {
            if entries.next().is_some() {
                return Err(CliError::Fatal {
                    code: 128,
                    message: format!(
                        "destination path '{label}' already exists and is not an empty directory."
                    ),
                });
            }
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(_) if path.exists() => Err(CliError::Fatal {
            code: 128,
            message: format!(
                "destination path '{label}' already exists and is not an empty directory."
            ),
        }),
        Err(error) => Err(CliError::Io(error)),
    }
}

pub(crate) fn copy_dir_contents(
    source: &std::path::Path,
    destination: &std::path::Path,
) -> Result<()> {
    reject_symlinked_object_path(source, source)?;
    copy_dir_contents_checked(source, destination)
}

pub(crate) fn copy_dir_contents_to_fresh_destination(
    source: &std::path::Path,
    destination: &std::path::Path,
) -> Result<()> {
    reject_symlinked_object_path(source, source)?;
    copy_dir_contents_fresh_parallel_checked(source, destination)
}

fn copy_dir_contents_checked(
    source: &std::path::Path,
    destination: &std::path::Path,
) -> Result<()> {
    reject_destination_symlink(destination)?;
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let metadata = fs::symlink_metadata(&source_path)?;
        if metadata.file_type().is_symlink() {
            return Err(symlinked_object_path_error(source, &source_path));
        }
        if metadata.is_dir() {
            copy_dir_contents_checked(&source_path, &destination_path)?;
        } else if metadata.is_file() {
            if destination_path_exists_without_symlink(&destination_path)? {
                continue;
            }
            fs::copy(&source_path, &destination_path)?;
        }
    }
    Ok(())
}

pub(crate) fn hardlink_dir_contents_to_fresh_destination(
    source: &std::path::Path,
    destination: &std::path::Path,
) -> Result<()> {
    reject_symlinked_object_path(source, source)?;
    hardlink_dir_contents_fresh_parallel_checked(source, destination)
}

#[derive(Clone, Copy)]
enum FreshObjectFileOpKind {
    Copy,
    Hardlink,
}

struct FreshObjectFileOp {
    source: PathBuf,
    destination: PathBuf,
    kind: FreshObjectFileOpKind,
}

fn copy_dir_contents_fresh_parallel_checked(
    source: &std::path::Path,
    destination: &std::path::Path,
) -> Result<()> {
    let mut ops = Vec::new();
    collect_fresh_object_file_ops(
        source,
        source,
        destination,
        FreshObjectFileOpKind::Copy,
        &mut ops,
    )?;
    run_fresh_object_file_ops(&ops)
}

fn hardlink_dir_contents_fresh_parallel_checked(
    source: &std::path::Path,
    destination: &std::path::Path,
) -> Result<()> {
    let mut ops = Vec::new();
    collect_fresh_object_file_ops(
        source,
        source,
        destination,
        FreshObjectFileOpKind::Hardlink,
        &mut ops,
    )?;
    run_fresh_object_file_ops(&ops)
}

fn collect_fresh_object_file_ops(
    root: &std::path::Path,
    source: &std::path::Path,
    destination: &std::path::Path,
    kind: FreshObjectFileOpKind,
    ops: &mut Vec<FreshObjectFileOp>,
) -> Result<()> {
    reject_destination_symlink(destination)?;
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let metadata = fs::symlink_metadata(&source_path)?;
        if metadata.file_type().is_symlink() {
            return Err(symlinked_object_path_error(root, &source_path));
        }
        if metadata.is_dir() {
            collect_fresh_object_file_ops(root, &source_path, &destination_path, kind, ops)?;
        } else if metadata.is_file() {
            ops.push(FreshObjectFileOp {
                source: source_path,
                destination: destination_path,
                kind,
            });
        }
    }
    Ok(())
}

fn run_fresh_object_file_ops(ops: &[FreshObjectFileOp]) -> Result<()> {
    if ops.len() < PARALLEL_OBJECT_FILE_THRESHOLD {
        return run_fresh_object_file_ops_sequential(ops);
    }
    let workers = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1)
        .min(ops.len());
    if workers <= 1 {
        return run_fresh_object_file_ops_sequential(ops);
    }
    let chunk_len = ops.len().div_ceil(workers);
    let results = std::thread::scope(|scope| {
        ops.chunks(chunk_len)
            .map(|chunk| {
                scope.spawn(move || {
                    for op in chunk {
                        run_fresh_object_file_op(op)?;
                    }
                    Ok::<(), io::Error>(())
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|handle| {
                handle.join().unwrap_or_else(|_| {
                    Err(io::Error::other("parallel object copy worker panicked"))
                })
            })
            .collect::<Vec<_>>()
    });
    for result in results {
        result?;
    }
    Ok(())
}

fn run_fresh_object_file_ops_sequential(ops: &[FreshObjectFileOp]) -> Result<()> {
    for op in ops {
        run_fresh_object_file_op(op)?;
    }
    Ok(())
}

fn run_fresh_object_file_op(op: &FreshObjectFileOp) -> io::Result<()> {
    match op.kind {
        FreshObjectFileOpKind::Copy => {
            fs::copy(&op.source, &op.destination)?;
        }
        FreshObjectFileOpKind::Hardlink => {
            fs::hard_link(&op.source, &op.destination)?;
        }
    }
    Ok(())
}

pub(crate) fn validate_local_clone_security(
    source_git_dir: &std::path::Path,
    destination_git_dir: &std::path::Path,
) -> Result<()> {
    let objects_dir = source_git_dir.join("objects");
    validate_object_store_no_symlinks(&objects_dir)?;
    validate_local_clone_ownership(source_git_dir, destination_git_dir)
}

pub(crate) fn validate_local_clone_ownership(
    source_git_dir: &std::path::Path,
    destination_git_dir: &std::path::Path,
) -> Result<()> {
    let objects_dir = source_git_dir.join("objects");
    reject_cross_owner_local_clone(source_git_dir, &objects_dir, destination_git_dir)
}

pub(crate) fn validate_object_store_no_symlinks(objects_dir: &std::path::Path) -> Result<()> {
    reject_symlinked_object_path(objects_dir, objects_dir)?;
    reject_symlinked_object_entries(objects_dir, objects_dir)
}

pub(crate) fn validate_destination_object_store_no_symlinks(
    objects_dir: &std::path::Path,
) -> Result<()> {
    reject_destination_symlink(objects_dir)?;
    reject_destination_object_entries(objects_dir)
}

fn reject_destination_object_entries(path: &std::path::Path) -> Result<()> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let path = entry.path();
        reject_destination_symlink(&path)?;
        if path.is_dir() {
            reject_destination_object_entries(&path)?;
        }
    }
    Ok(())
}

fn reject_symlinked_object_entries(root: &std::path::Path, path: &std::path::Path) -> Result<()> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let source_path = entry.path();
        let metadata = fs::symlink_metadata(&source_path)?;
        if metadata.file_type().is_symlink() {
            return Err(symlinked_object_path_error(root, &source_path));
        }
        if metadata.is_dir() {
            reject_symlinked_object_entries(root, &source_path)?;
        }
    }
    Ok(())
}

fn reject_symlinked_object_path(root: &std::path::Path, path: &std::path::Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(symlinked_object_path_error(root, path));
    }
    Ok(())
}

fn symlinked_object_path_error(root: &std::path::Path, path: &std::path::Path) -> CliError {
    let message = if path == root {
        format!(
            "'{}' is a symlink, refusing to clone with --local",
            path.display()
        )
    } else {
        let relative = path.strip_prefix(root).unwrap_or(path);
        format!(
            "symlink '{}' exists, refusing to clone with --local",
            relative.display()
        )
    };
    CliError::Fatal { code: 128, message }
}

fn destination_path_exists_without_symlink(path: &std::path::Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return Err(CliError::Fatal {
                    code: 128,
                    message: format!(
                        "destination object path '{}' is a symbolic link",
                        path.display()
                    ),
                });
            }
            Ok(true)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(CliError::Io(error)),
    }
}

fn reject_destination_symlink(path: &std::path::Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(CliError::Fatal {
            code: 128,
            message: format!(
                "destination object path '{}' is a symbolic link",
                path.display()
            ),
        }),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(CliError::Io(error)),
    }
}

#[cfg(unix)]
fn reject_cross_owner_local_clone(
    source_git_dir: &std::path::Path,
    objects_dir: &std::path::Path,
    destination_git_dir: &std::path::Path,
) -> Result<()> {
    use std::os::unix::fs::MetadataExt;

    let destination_uid = fs::symlink_metadata(destination_git_dir)?.uid();
    let source_uid = fs::symlink_metadata(source_git_dir)?.uid();
    let objects_uid = fs::symlink_metadata(objects_dir)?.uid();
    if source_uid != destination_uid || objects_uid != destination_uid {
        return Err(CliError::Fatal {
            code: 128,
            message: format!(
                "repository '{}' is owned by someone else, refusing to clone with --local",
                source_git_dir.display()
            ),
        });
    }
    Ok(())
}

#[cfg(not(unix))]
fn reject_cross_owner_local_clone(
    _source_git_dir: &std::path::Path,
    _objects_dir: &std::path::Path,
    _destination_git_dir: &std::path::Path,
) -> Result<()> {
    Ok(())
}

pub(crate) fn copy_remote_refs(
    source: &RefStore,
    destination: &RefStore,
    remote: &str,
    head_branch: Option<&str>,
    copy_tags: bool,
) -> Result<()> {
    source.for_each_resolved_ref("refs/heads/", |ref_name, id| {
        let branch = ref_name
            .strip_prefix("refs/heads/")
            .ok_or_else(|| CliError::Fatal {
                code: 128,
                message: format!("invalid source branch ref '{ref_name}'"),
            })?;
        destination.write_ref(&format!("refs/remotes/{remote}/{branch}"), id)?;
        Ok::<(), CliError>(())
    })?;
    if let Some(branch) = head_branch {
        destination.write_symbolic_ref(
            &format!("refs/remotes/{remote}/HEAD"),
            &format!("refs/remotes/{remote}/{branch}"),
        )?;
    }
    if copy_tags {
        source.for_each_ref_name("refs/tags/", |ref_name| {
            match source.read_ref(ref_name)? {
                RefTarget::Direct(id) => destination.write_ref(ref_name, &id)?,
                RefTarget::Symbolic(target) => destination.write_symbolic_ref(ref_name, &target)?,
            }
            Ok::<(), CliError>(())
        })?;
    }
    Ok(())
}

pub(crate) fn write_fresh_clone_remote_refs(
    source: &RefStore,
    destination: &RefStore,
    remote: &str,
    head_branch: Option<&str>,
    copy_tags: bool,
) -> Result<()> {
    let mut direct_refs = Vec::new();
    let mut symbolic_refs = Vec::new();
    source.for_each_resolved_ref("refs/heads/", |ref_name, id| {
        let branch = ref_name
            .strip_prefix("refs/heads/")
            .ok_or_else(|| CliError::Fatal {
                code: 128,
                message: format!("invalid source branch ref '{ref_name}'"),
            })?;
        direct_refs.push((format!("refs/remotes/{remote}/{branch}"), id.clone()));
        Ok::<(), CliError>(())
    })?;
    if let Some(branch) = head_branch {
        symbolic_refs.push((
            format!("refs/remotes/{remote}/HEAD"),
            format!("refs/remotes/{remote}/{branch}"),
        ));
    }
    if copy_tags {
        source.for_each_ref_name("refs/tags/", |ref_name| {
            match source.read_ref(ref_name)? {
                RefTarget::Direct(id) => direct_refs.push((ref_name.to_owned(), id)),
                RefTarget::Symbolic(target) => {
                    symbolic_refs.push((ref_name.to_owned(), target));
                }
            }
            Ok::<(), CliError>(())
        })?;
    }
    destination.write_fresh_packed_refs(&direct_refs, &symbolic_refs)?;
    Ok(())
}

pub(crate) fn copy_prefetch_refs(
    source: &RefStore,
    destination: &RefStore,
    remote: &str,
) -> Result<()> {
    source.for_each_resolved_ref("refs/heads/", |ref_name, id| {
        let branch = ref_name
            .strip_prefix("refs/heads/")
            .ok_or_else(|| CliError::Fatal {
                code: 128,
                message: format!("invalid source branch ref '{ref_name}'"),
            })?;
        destination.write_ref(&format!("refs/prefetch/remotes/{remote}/{branch}"), id)?;
        Ok::<(), CliError>(())
    })?;
    Ok(())
}

pub(crate) fn write_fresh_bare_clone_refs(
    source: &RefStore,
    destination: &RefStore,
    copy_tags: bool,
) -> Result<()> {
    let mut direct_refs = Vec::new();
    let mut symbolic_refs = Vec::new();
    source.for_each_ref_name("refs/heads/", |ref_name| {
        match source.read_ref(ref_name)? {
            RefTarget::Direct(id) => direct_refs.push((ref_name.to_owned(), id)),
            RefTarget::Symbolic(target) => symbolic_refs.push((ref_name.to_owned(), target)),
        }
        Ok::<(), CliError>(())
    })?;
    if copy_tags {
        source.for_each_ref_name("refs/tags/", |ref_name| {
            match source.read_ref(ref_name)? {
                RefTarget::Direct(id) => direct_refs.push((ref_name.to_owned(), id)),
                RefTarget::Symbolic(target) => symbolic_refs.push((ref_name.to_owned(), target)),
            }
            Ok::<(), CliError>(())
        })?;
    }
    destination.write_fresh_packed_refs(&direct_refs, &symbolic_refs)?;
    Ok(())
}

pub(crate) fn write_fresh_mirror_clone_refs(
    source: &RefStore,
    destination: &RefStore,
) -> Result<()> {
    let mut direct_refs = Vec::new();
    let mut symbolic_refs = Vec::new();
    source.for_each_ref_name("refs/", |ref_name| {
        match source.read_ref(ref_name)? {
            RefTarget::Direct(id) => direct_refs.push((ref_name.to_owned(), id)),
            RefTarget::Symbolic(target) => symbolic_refs.push((ref_name.to_owned(), target)),
        }
        Ok::<(), CliError>(())
    })?;
    destination.write_fresh_packed_refs(&direct_refs, &symbolic_refs)?;
    Ok(())
}

pub(crate) fn copy_single_tag_ref(
    source: &RefStore,
    destination: &RefStore,
    tag: &str,
) -> Result<()> {
    let ref_name = tag_ref_name(tag)?;
    match source.read_ref(&ref_name)? {
        RefTarget::Direct(id) => destination.write_ref(&ref_name, &id)?,
        RefTarget::Symbolic(target) => destination.write_symbolic_ref(&ref_name, &target)?,
    }
    Ok(())
}

pub(crate) fn write_fresh_head_remote_ref(
    source: &RefStore,
    destination: &RefStore,
    remote: &str,
    head_branch: Option<&str>,
    write_remote_head: bool,
    copy_tags: bool,
) -> Result<()> {
    let mut direct_refs = Vec::new();
    let mut symbolic_refs = Vec::new();
    if let Some(branch) = head_branch {
        let ref_name = format!("refs/heads/{branch}");
        let id = source.resolve(&ref_name)?;
        direct_refs.push((format!("refs/remotes/{remote}/{branch}"), id));
        if write_remote_head {
            symbolic_refs.push((
                format!("refs/remotes/{remote}/HEAD"),
                format!("refs/remotes/{remote}/{branch}"),
            ));
        }
    }
    if copy_tags {
        source.for_each_ref_name("refs/tags/", |tag_ref| {
            match source.read_ref(tag_ref)? {
                RefTarget::Direct(id) => direct_refs.push((tag_ref.to_owned(), id)),
                RefTarget::Symbolic(target) => symbolic_refs.push((tag_ref.to_owned(), target)),
            }
            Ok::<(), CliError>(())
        })?;
    }
    destination.write_fresh_packed_refs(&direct_refs, &symbolic_refs)?;
    Ok(())
}

pub(crate) fn shallow_boundaries(
    store: &LooseObjectStore,
    roots: &[ObjectId],
    depth: usize,
) -> Result<Vec<ObjectId>> {
    let commit_cache = CommitObjectCache::new(store);
    let root_capacity = local_transport_root_capacity_hint(roots.len());
    let mut pending = VecDeque::with_capacity(root_capacity);
    pending.extend(roots.iter().cloned().map(|id| (id, 1usize)));
    let mut seen = HashSet::with_capacity(root_capacity);
    let mut boundaries = Vec::with_capacity(root_capacity);
    while let Some((id, level)) = pending.pop_front() {
        if !seen.insert(id.clone()) {
            continue;
        }
        if level >= depth {
            boundaries.push(id);
            continue;
        }
        let commit = commit_cache.read_commit(&id)?;
        for parent in &commit.parents {
            let parent = parent.clone();
            pending.push_back((parent, level + 1));
        }
    }
    boundaries.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
    Ok(boundaries)
}

pub(crate) fn write_shallow_file(repo: &GitRepo, boundaries: Vec<ObjectId>) -> Result<()> {
    let path = repo.git_dir.join("shallow");
    if boundaries.is_empty() {
        match fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(CliError::Io(error)),
        }
        return Ok(());
    }
    let mut out = String::with_capacity(shallow_file_content_capacity(&boundaries));
    for id in boundaries {
        id.write_hex(&mut out)
            .expect("writing object id hex to String cannot fail");
        out.push('\n');
    }
    fs::write(path, out)?;
    Ok(())
}

fn shallow_file_content_capacity(boundaries: &[ObjectId]) -> usize {
    boundaries.iter().fold(0_usize, |capacity, id| {
        capacity.saturating_add(id.hex_len() + 1)
    })
}

fn local_transport_root_capacity_hint(roots_len: usize) -> usize {
    roots_len.min(LOCAL_TRANSPORT_INITIAL_CAPACITY_LIMIT)
}

pub(crate) fn default_push_refspec(refs: &RefStore) -> Result<String> {
    current_branch_ref(refs)?
        .map(|name| branch_display_name(&name))
        .ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "cannot push detached HEAD without an explicit refspec".into(),
        })
}

pub(crate) fn parse_push_refspec(
    repo: &GitRepo,
    refs: &RefStore,
    spec: &str,
    remote_url: &str,
) -> Result<PushRef> {
    let force = spec.starts_with('+');
    let spec = spec.strip_prefix('+').unwrap_or(spec);
    let (source, destination) = spec
        .split_once(':')
        .map(|(source, destination)| (source, Some(destination)))
        .unwrap_or((spec, None));
    if source.is_empty() {
        let Some(destination) = destination.filter(|value| !value.is_empty()) else {
            return Err(CliError::Stderr {
                code: 1,
                text: format!(
                    "error: dst refspec {spec} matches more than one\n\
                     error: failed to push some refs to '{remote_url}'\n"
                ),
            });
        };
        return Ok(PushRef {
            id: None,
            destination: push_destination_ref(destination)?,
            source_display: None,
            force,
        });
    }
    let id = resolve_objectish(repo, source).map_err(|_| CliError::Stderr {
        code: 1,
        text: format!(
            "error: src refspec {source} does not match any\n\
             error: failed to push some refs to '{remote_url}'\n"
        ),
    })?;
    let destination = match destination {
        Some(value) if !value.is_empty() => push_destination_ref(value)?,
        Some(_) | None if source == "HEAD" => {
            current_branch_ref(refs)?.ok_or_else(|| CliError::Fatal {
                code: 128,
                message: "cannot infer remote branch for detached HEAD".into(),
            })?
        }
        Some(_) | None => push_destination_ref(source)?,
    };
    Ok(PushRef {
        id: Some(id),
        destination,
        source_display: Some(source.to_owned()),
        force,
    })
}

pub(crate) fn validate_push_update(
    refs: &RefStore,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    push_ref: &PushRef,
    force: bool,
) -> Result<()> {
    let current = match refs.read_ref(&push_ref.destination) {
        Ok(RefTarget::Direct(id)) => Some(id),
        Ok(RefTarget::Symbolic(target)) => Some(refs.resolve(&target)?),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(CliError::Io(error)),
    };
    let Some(new_id) = &push_ref.id else {
        return Ok(());
    };
    if let Some(current) = current
        && !force
        && !is_ancestor_commit_cached(commit_cache, &current, new_id)?
    {
        return Err(CliError::Fatal {
            code: 1,
            message: format!(
                "failed to push some refs to '{}': non-fast-forward",
                push_ref.destination
            ),
        });
    }
    Ok(())
}

pub(crate) fn validate_push_delete(refs: &RefStore, destination: &str) -> Result<()> {
    if !ref_exists(refs, destination)? {
        return Err(CliError::Stderr {
            code: 1,
            text: format!(
                "error: unable to delete '{}': remote ref does not exist\n\
                 error: failed to push some refs\n",
                destination
                    .strip_prefix("refs/heads/")
                    .unwrap_or(destination)
            ),
        });
    }
    if let Ok(RefTarget::Symbolic(target)) = refs.read_head()
        && target == destination
        && !receive_allows_deleting_current_branch(refs.git_dir())?
    {
        return Err(CliError::Stderr {
            code: 1,
            text: format!(
                "remote: error: refusing to delete the current branch: {destination}\n\
                 error: failed to push some refs\n"
            ),
        });
    }
    Ok(())
}

fn receive_allows_deleting_current_branch(git_dir: &std::path::Path) -> Result<bool> {
    let entries = read_config_file(&git_dir.join("config"))?;
    let value = entries
        .into_iter()
        .rev()
        .find(|entry| {
            entry.section == "receive"
                && entry.subsection.is_empty()
                && entry.key == "denyDeleteCurrent"
        })
        .map(|entry| entry.value.to_ascii_lowercase());
    Ok(matches!(value.as_deref(), Some("warn" | "ignore")))
}

pub(crate) fn set_push_upstream(repo: &GitRepo, push_ref: &PushRef, remote: &str) -> Result<()> {
    let Some(branch) = current_branch_ref(&RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1))?
    else {
        return Ok(());
    };
    let branch = branch_display_name(&branch);
    set_config_value(repo, &format!("branch.{branch}.remote"), remote)?;
    set_config_value(
        repo,
        &format!("branch.{branch}.merge"),
        &push_ref.destination,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oid(hex: &str) -> ObjectId {
        ObjectId::from_hex(GitHashAlgorithm::Sha1, hex).expect("object id")
    }

    fn make_local_reference_repo() -> tempfile::TempDir {
        let dir = tempfile::TempDir::new().expect("temp dir");
        fs::create_dir_all(dir.path().join(".git/objects")).expect("objects");
        fs::write(dir.path().join(".git/HEAD"), b"ref: refs/heads/main\n").expect("head");
        dir
    }

    #[test]
    fn reference_object_dirs_capacity_hint_tracks_input_len() {
        assert_eq!(reference_object_dirs_capacity_hint(&[]), 0);
        assert_eq!(
            reference_object_dirs_capacity_hint(&[
                PathBuf::from("/repo/a"),
                PathBuf::from("/repo/b"),
                PathBuf::from("/repo/c"),
            ]),
            3
        );
    }

    #[test]
    fn reference_object_dirs_preallocates_and_keeps_order() {
        let first = make_local_reference_repo();
        let second = make_local_reference_repo();
        let references = vec![first.path().to_path_buf(), second.path().to_path_buf()];

        let object_dirs = reference_object_dirs(&references).expect("reference object dirs");

        assert!(object_dirs.capacity() >= reference_object_dirs_capacity_hint(&references));
        assert_eq!(
            object_dirs,
            vec![
                canonical_or_absolute(first.path().join(".git/objects")),
                canonical_or_absolute(second.path().join(".git/objects")),
            ]
        );
    }

    #[test]
    fn alternates_file_content_capacity_hint_counts_path_lines() {
        let alternates = vec![
            PathBuf::from("/repo/a/objects"),
            PathBuf::from("/repo/b/objects"),
        ];

        assert_eq!(
            alternates_file_content_capacity_hint(&alternates),
            "/repo/a/objects\n/repo/b/objects\n".len()
        );
    }

    #[test]
    fn write_alternates_file_writes_each_path_line() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let objects_dir = dir.path().join("objects");
        let alternates = vec![
            PathBuf::from("/repo/a/objects"),
            PathBuf::from("/repo/b/objects"),
        ];

        write_alternates_file(&objects_dir, &alternates).expect("write alternates");

        assert_eq!(
            fs::read_to_string(objects_dir.join("info/alternates")).expect("alternates file"),
            "/repo/a/objects\n/repo/b/objects\n"
        );
    }

    #[test]
    fn shallow_git_dir_check_streams_until_first_non_empty_line() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        fs::write(dir.path().join("shallow"), b"\n  \nabc123\nignored\n").expect("shallow");

        assert!(is_shallow_git_dir(dir.path()));
    }

    #[test]
    fn shallow_git_dir_check_treats_missing_or_empty_file_as_not_shallow() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        assert!(!is_shallow_git_dir(dir.path()));

        fs::write(dir.path().join("shallow"), b"\n  \n").expect("empty shallow");

        assert!(!is_shallow_git_dir(dir.path()));
    }

    #[test]
    fn shallow_file_content_capacity_matches_hex_lines() {
        let first = oid("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        let second = oid("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");

        assert_eq!(
            shallow_file_content_capacity(&[first.clone(), second.clone()]),
            first.hex_len() + 1 + second.hex_len() + 1
        );
    }

    #[test]
    fn local_transport_root_capacity_hint_is_bounded() {
        assert_eq!(local_transport_root_capacity_hint(0), 0);
        assert_eq!(local_transport_root_capacity_hint(3), 3);
        assert_eq!(
            local_transport_root_capacity_hint(usize::MAX),
            LOCAL_TRANSPORT_INITIAL_CAPACITY_LIMIT
        );
    }

    #[test]
    fn write_shallow_file_writes_hex_lines_without_temporary_hex_strings() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let repo = GitRepo {
            root: dir.path().to_path_buf(),
            git_dir: dir.path().to_path_buf(),
            objects_dir: dir.path().join("objects"),
            index_path: dir.path().join("index"),
        };
        let first = oid("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        let second = oid("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");

        write_shallow_file(&repo, vec![first, second]).expect("write shallow");

        assert_eq!(
            fs::read_to_string(dir.path().join("shallow")).expect("shallow file"),
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\nbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\n"
        );
    }

    #[test]
    fn copy_remote_refs_uses_loose_ref_over_stale_packed_ref() {
        let source = tempfile::TempDir::new().expect("source repo");
        let source_git_dir = source.path().join(".git");
        let source_objects_dir = source_git_dir.join("objects");
        fs::create_dir_all(&source_objects_dir).expect("source objects");
        let source_store = LooseObjectStore::new(&source_objects_dir, GitHashAlgorithm::Sha1);
        let stale_id = source_store
            .write_object(GitObjectKind::Blob, b"stale branch target\n")
            .expect("write stale object");
        let live_id = source_store
            .write_object(GitObjectKind::Blob, b"live branch target\n")
            .expect("write live object");
        fs::write(
            source_git_dir.join("packed-refs"),
            format!("{} refs/heads/main\n", stale_id.to_hex()),
        )
        .expect("write packed refs");
        let source_refs = RefStore::new(&source_git_dir, GitHashAlgorithm::Sha1);
        source_refs
            .write_ref("refs/heads/main", &live_id)
            .expect("write loose branch ref");

        let destination = tempfile::TempDir::new().expect("destination repo");
        let destination_git_dir = destination.path().join(".git");
        fs::create_dir_all(destination_git_dir.join("objects")).expect("destination objects");
        let destination_refs = RefStore::new(&destination_git_dir, GitHashAlgorithm::Sha1);

        copy_remote_refs(&source_refs, &destination_refs, "origin", None, false)
            .expect("copy remote refs");

        assert_eq!(
            destination_refs
                .resolve("refs/remotes/origin/main")
                .expect("remote branch"),
            live_id
        );
    }
}
