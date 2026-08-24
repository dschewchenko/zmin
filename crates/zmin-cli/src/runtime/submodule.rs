use super::*;
use crate::cli::commands::transport_commands::{
    SubmoduleTargetFetchOptions, fetch_submodule_target_with_options, is_git_daemon_transport_url,
    is_http_transport_url, is_ssh_transport_url,
};
use crate::cli::commands::{
    merge_commands::{MergeOptions, merge},
    sequencer_commands::rebase,
    worktree_commands::standard_repo_ignore,
};
use zmin_git_core::{CommitLinksCache, decode_commit_links};

#[derive(Clone, Debug)]
struct GitmodulesEntry {
    name: String,
    path: String,
    url: String,
    branch: Option<String>,
}

#[derive(Clone, Debug)]
struct ReadTreeSubmoduleModuleDescriptor {
    name: String,
    path: String,
}

impl ReadTreeSubmoduleModuleDescriptor {
    fn from_module(module: &GitmodulesEntry) -> Self {
        Self {
            name: module.name.clone(),
            path: module.path.clone(),
        }
    }
}

#[derive(Clone, Debug)]
struct ReadTreeSubmoduleTarget {
    module: ReadTreeSubmoduleModuleDescriptor,
    id: ObjectId,
}

#[derive(Clone, Debug)]
struct ReadTreeSubmoduleCheckoutTarget {
    module: ReadTreeSubmoduleModuleDescriptor,
    id: ObjectId,
    transition: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ReadTreeSubmoduleRemovalPathId(usize);

#[derive(Debug)]
struct ReadTreeSubmoduleRemovalPathNode {
    parent: Option<ReadTreeSubmoduleRemovalPathId>,
    segment: String,
}

#[derive(Debug)]
struct ReadTreeSubmoduleRemovalPathArena {
    nodes: Vec<ReadTreeSubmoduleRemovalPathNode>,
}

impl ReadTreeSubmoduleRemovalPathArena {
    fn new(prefix: &str) -> Self {
        Self {
            nodes: vec![ReadTreeSubmoduleRemovalPathNode {
                parent: None,
                segment: prefix.to_owned(),
            }],
        }
    }

    fn push(
        &mut self,
        parent: ReadTreeSubmoduleRemovalPathId,
        segment: &str,
    ) -> ReadTreeSubmoduleRemovalPathId {
        let id = ReadTreeSubmoduleRemovalPathId(self.nodes.len());
        self.nodes.push(ReadTreeSubmoduleRemovalPathNode {
            parent: Some(parent),
            segment: segment.to_owned(),
        });
        id
    }

    fn segment(&self, id: ReadTreeSubmoduleRemovalPathId) -> &str {
        &self.nodes[id.0].segment
    }

    fn display(&self, id: ReadTreeSubmoduleRemovalPathId) -> String {
        let mut segments = Vec::new();
        let mut current = Some(id);
        while let Some(path_id) = current {
            let node = &self.nodes[path_id.0];
            if !node.segment.is_empty() {
                segments.push(node.segment.as_str());
            }
            current = node.parent;
        }
        segments.reverse();
        segments.join("/")
    }
}

#[derive(Debug)]
struct ReadTreeSubmoduleRemovalChild {
    repo: GitRepo,
    path_id: ReadTreeSubmoduleRemovalPathId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SubmoduleUpdateStrategy {
    Checkout,
    Merge,
    Rebase,
}

pub(crate) const CLONE_SUBMODULE_FILTER_CONFIG_KEY: &str = "zmin.alsoFilterSubmodules";
const SUBMODULE_RECURSION_DEPTH_CONFIG_KEY: &str = "zmin.submoduleRecursionDepth";
const SUBMODULE_RECURSION_MODULES_CONFIG_KEY: &str = "zmin.submoduleRecursionModules";
const SUBMODULE_RECURSION_PATH_BYTES_CONFIG_KEY: &str = "zmin.submoduleRecursionPathBytes";
const SUBMODULE_RECURSION_INPUT_BYTES_CONFIG_KEY: &str = "zmin.submoduleRecursionInputBytes";
const SUBMODULE_RECURSION_KEYS_CONFIG_KEY: &str = "zmin.submoduleRecursionKeys";
const SUBMODULE_RECURSION_REF_FORMAT_CONFIG_KEY: &str = "zmin.submoduleRecursionRefFormat";

pub(crate) const SUBMODULE_MAX_RECURSION_DEPTH: usize = 64;
pub(crate) const SUBMODULE_MAX_TOTAL_MODULES: usize = 4096;
pub(crate) const SUBMODULE_MAX_PATH_BYTES: usize = 1 << 20;
pub(crate) const SUBMODULE_MAX_INPUT_BYTES: usize = 4 << 20;
const SUBMODULE_MAX_STATE_VALUE_BYTES: usize = 1 << 20;
const SUBMODULE_MAX_DIAGNOSTIC_BYTES: usize = 96;

#[derive(Clone, Debug, Eq, PartialEq)]
struct SubmoduleCycleKey {
    source: String,
    path: String,
    gitlink: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct SubmoduleRecursionState {
    depth: usize,
    total_modules: usize,
    path_bytes: usize,
    input_bytes: usize,
    active_keys: Vec<SubmoduleCycleKey>,
    ref_format: Option<String>,
}

impl SubmoduleRecursionState {
    fn from_parent_repository(repo: &GitRepo) -> Result<Self> {
        let mut state = Self {
            ref_format: Some(submodule_ref_format(repo)?),
            ..Self::default()
        };
        state.depth = read_bounded_state_number(repo, SUBMODULE_RECURSION_DEPTH_CONFIG_KEY)?
            .unwrap_or_default();
        state.total_modules =
            read_bounded_state_number(repo, SUBMODULE_RECURSION_MODULES_CONFIG_KEY)?
                .unwrap_or_default();
        state.path_bytes =
            read_bounded_state_number(repo, SUBMODULE_RECURSION_PATH_BYTES_CONFIG_KEY)?
                .unwrap_or_default();
        state.input_bytes =
            read_bounded_state_number(repo, SUBMODULE_RECURSION_INPUT_BYTES_CONFIG_KEY)?
                .unwrap_or_default();
        if let Some(value) = read_config_value(repo, SUBMODULE_RECURSION_REF_FORMAT_CONFIG_KEY)? {
            state.ref_format = Some(validate_submodule_ref_format(&value)?);
        }
        if let Some(value) = read_config_value(repo, SUBMODULE_RECURSION_KEYS_CONFIG_KEY)? {
            state.active_keys = decode_submodule_cycle_keys(&value)?;
        }
        state.validate_bounds()?;
        Ok(state)
    }

    fn account_context(
        &mut self,
        parent_repository: &str,
        active_specs: &[String],
        filter: Option<&str>,
    ) -> Result<()> {
        let mut additional = parent_repository.len();
        for spec in active_specs {
            additional = additional
                .checked_add(spec.len())
                .ok_or_else(|| submodule_limit_error("input size"))?;
        }
        if let Some(filter) = filter {
            additional = additional
                .checked_add(filter.len())
                .ok_or_else(|| submodule_limit_error("input size"))?;
        }
        self.input_bytes = self
            .input_bytes
            .checked_add(additional)
            .ok_or_else(|| submodule_limit_error("input size"))?;
        self.validate_bounds()
    }

    fn enter(
        &mut self,
        parent_repository: &str,
        module: &GitmodulesEntry,
        gitlink: &ObjectId,
    ) -> Result<()> {
        let next_depth = self
            .depth
            .checked_add(1)
            .ok_or_else(|| submodule_limit_error("recursion depth"))?;
        if next_depth > SUBMODULE_MAX_RECURSION_DEPTH {
            return Err(submodule_limit_error("recursion depth"));
        }
        let next_total = self
            .total_modules
            .checked_add(1)
            .ok_or_else(|| submodule_limit_error("module count"))?;
        if next_total > SUBMODULE_MAX_TOTAL_MODULES {
            return Err(submodule_limit_error("module count"));
        }
        let next_path_bytes = self
            .path_bytes
            .checked_add(module.path.len())
            .ok_or_else(|| submodule_limit_error("path size"))?;
        if next_path_bytes > SUBMODULE_MAX_PATH_BYTES {
            return Err(submodule_limit_error("path size"));
        }
        let next_input_bytes = self
            .input_bytes
            .checked_add(module.name.len())
            .and_then(|value| value.checked_add(module.path.len()))
            .and_then(|value| value.checked_add(module.url.len()))
            .and_then(|value| value.checked_add(parent_repository.len()))
            .ok_or_else(|| submodule_limit_error("input size"))?;
        if next_input_bytes > SUBMODULE_MAX_INPUT_BYTES {
            return Err(submodule_limit_error("input size"));
        }
        let key = SubmoduleCycleKey {
            source: canonical_submodule_source(parent_repository, &module.url),
            path: module.path.clone(),
            gitlink: gitlink.to_hex(),
        };
        if self.active_keys.iter().any(|active| active == &key) {
            return Err(submodule_limit_error("cycle"));
        }
        self.depth = next_depth;
        self.total_modules = next_total;
        self.path_bytes = next_path_bytes;
        self.input_bytes = next_input_bytes;
        self.active_keys.push(key);
        Ok(())
    }

    fn leave(&mut self) {
        let _ = self.active_keys.pop();
        self.depth = self.depth.saturating_sub(1);
    }

    fn child_configs(&self) -> Vec<String> {
        let mut configs = vec![
            format!("{SUBMODULE_RECURSION_DEPTH_CONFIG_KEY}={}", self.depth),
            format!(
                "{SUBMODULE_RECURSION_MODULES_CONFIG_KEY}={}",
                self.total_modules
            ),
            format!(
                "{SUBMODULE_RECURSION_PATH_BYTES_CONFIG_KEY}={}",
                self.path_bytes
            ),
            format!(
                "{SUBMODULE_RECURSION_INPUT_BYTES_CONFIG_KEY}={}",
                self.input_bytes
            ),
            format!(
                "{SUBMODULE_RECURSION_KEYS_CONFIG_KEY}={}",
                encode_submodule_cycle_keys(&self.active_keys)
            ),
        ];
        if let Some(ref_format) = &self.ref_format {
            configs.push(format!(
                "{SUBMODULE_RECURSION_REF_FORMAT_CONFIG_KEY}={ref_format}"
            ));
        }
        configs
    }

    fn validate_bounds(&self) -> Result<()> {
        if self.depth > SUBMODULE_MAX_RECURSION_DEPTH {
            return Err(submodule_limit_error("recursion depth"));
        }
        if self.total_modules > SUBMODULE_MAX_TOTAL_MODULES {
            return Err(submodule_limit_error("module count"));
        }
        if self.path_bytes > SUBMODULE_MAX_PATH_BYTES {
            return Err(submodule_limit_error("path size"));
        }
        if self.input_bytes > SUBMODULE_MAX_INPUT_BYTES {
            return Err(submodule_limit_error("input size"));
        }
        if self.active_keys.len() > SUBMODULE_MAX_TOTAL_MODULES {
            return Err(submodule_limit_error("cycle state"));
        }
        Ok(())
    }
}

fn submodule_limit_error(limit: &str) -> CliError {
    CliError::Fatal {
        code: 128,
        message: format!("submodule recursion exceeds {limit} limit"),
    }
}

fn read_bounded_state_number(repo: &GitRepo, key: &str) -> Result<Option<usize>> {
    let Some(value) = read_config_value(repo, key)? else {
        return Ok(None);
    };
    if value.len() > 32 {
        return Err(submodule_limit_error("state value"));
    }
    value
        .parse::<usize>()
        .map(Some)
        .map_err(|_| CliError::Fatal {
            code: 128,
            message: "invalid submodule recursion state".into(),
        })
}

fn encode_submodule_text(value: &str) -> String {
    value
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn decode_submodule_text(value: &str) -> Result<String> {
    if value.len() > SUBMODULE_MAX_STATE_VALUE_BYTES || value.len() % 2 != 0 {
        return Err(submodule_limit_error("state value"));
    }
    let mut bytes = Vec::with_capacity(value.len() / 2);
    for pair in value.as_bytes().chunks_exact(2) {
        let high = (pair[0] as char).to_digit(16);
        let low = (pair[1] as char).to_digit(16);
        let (Some(high), Some(low)) = (high, low) else {
            return Err(CliError::Fatal {
                code: 128,
                message: "invalid submodule recursion state".into(),
            });
        };
        bytes.push((high * 16 + low) as u8);
    }
    String::from_utf8(bytes).map_err(|_| CliError::Fatal {
        code: 128,
        message: "invalid submodule recursion state".into(),
    })
}

fn encode_submodule_cycle_keys(keys: &[SubmoduleCycleKey]) -> String {
    keys.iter()
        .map(|key| {
            format!(
                "{}:{}:{}",
                encode_submodule_text(&key.source),
                encode_submodule_text(&key.path),
                encode_submodule_text(&key.gitlink)
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn decode_submodule_cycle_keys(value: &str) -> Result<Vec<SubmoduleCycleKey>> {
    if value.len() > SUBMODULE_MAX_STATE_VALUE_BYTES {
        return Err(submodule_limit_error("state value"));
    }
    if value.is_empty() {
        return Ok(Vec::new());
    }
    let mut keys = Vec::new();
    for encoded in value.split(',') {
        let fields = encoded.split(':').collect::<Vec<_>>();
        if fields.len() != 3 {
            return Err(CliError::Fatal {
                code: 128,
                message: "invalid submodule recursion state".into(),
            });
        }
        keys.push(SubmoduleCycleKey {
            source: decode_submodule_text(fields[0])?,
            path: decode_submodule_text(fields[1])?,
            gitlink: decode_submodule_text(fields[2])?,
        });
    }
    if keys.len() > SUBMODULE_MAX_TOTAL_MODULES {
        return Err(submodule_limit_error("cycle state"));
    }
    Ok(keys)
}

fn validate_submodule_ref_format(value: &str) -> Result<String> {
    if value.eq_ignore_ascii_case("files") {
        Ok("files".to_owned())
    } else if value.eq_ignore_ascii_case("reftable") {
        Ok("reftable".to_owned())
    } else {
        Err(CliError::Fatal {
            code: 128,
            message: "invalid submodule ref storage format".into(),
        })
    }
}

fn submodule_ref_format(repo: &GitRepo) -> Result<String> {
    read_config_value(repo, "extensions.refStorage")?.map_or_else(
        || Ok("files".to_owned()),
        |value| validate_submodule_ref_format(&value),
    )
}

fn canonical_submodule_source(parent_repository: &str, url: &str) -> String {
    let resolved = resolve_submodule_clone_url(parent_repository, url);
    if looks_like_remote_url(&resolved) {
        resolved
    } else {
        local_repository_path_from_location(&resolved)
            .ok()
            .flatten()
            .map(|path| canonical_or_absolute(path).display().to_string())
            .unwrap_or(resolved)
    }
}

fn local_clone_source_repo(source: &LocalCloneSource) -> GitRepo {
    let root = source
        .git_dir
        .file_name()
        .and_then(|name| (name == ".git").then_some(()))
        .and_then(|_| source.git_dir.parent())
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| source.git_dir.clone());
    GitRepo {
        root,
        git_dir: source.git_dir.clone(),
        objects_dir: source.common_dir.join("objects"),
        index_path: source.git_dir.join("index"),
    }
}

fn bounded_submodule_path_display(path: &str) -> String {
    let mut display = String::new();
    for character in path.chars() {
        let escaped = match character {
            '\0' => "\\0".to_owned(),
            '\n' => "\\n".to_owned(),
            '\r' => "\\r".to_owned(),
            '\t' => "\\t".to_owned(),
            character if character.is_control() => format!("\\u{{{:x}}}", character as u32),
            character => character.to_string(),
        };
        if display.len() + escaped.len() > SUBMODULE_MAX_DIAGNOSTIC_BYTES {
            display.push_str("...");
            break;
        }
        display.push_str(&escaped);
    }
    display
}

fn invalid_submodule_path(path: &str) -> CliError {
    CliError::Fatal {
        code: 128,
        message: format!(
            "invalid submodule path '{}'",
            bounded_submodule_path_display(path)
        ),
    }
}

fn is_git_directory_alias(component: &str) -> bool {
    let lower = component.to_ascii_lowercase();
    let trimmed = lower.trim_end_matches(['.', ' ']);
    trimmed == ".git" || trimmed == "git~1"
}

fn validate_submodule_path(repo: &GitRepo, raw_path: &str) -> Result<PathBuf> {
    if raw_path.is_empty()
        || raw_path.len() > SUBMODULE_MAX_PATH_BYTES
        || raw_path
            .bytes()
            .any(|byte| byte == 0 || byte < b' ' || byte == b'\\')
        || raw_path.contains("//")
        || raw_path.starts_with('/')
        || raw_path.ends_with('/')
        || (raw_path.len() >= 2
            && raw_path.as_bytes()[0].is_ascii_alphabetic()
            && raw_path.as_bytes()[1] == b':')
    {
        return Err(invalid_submodule_path(raw_path));
    }

    let mut components = Vec::new();
    for component in std::path::Path::new(raw_path).components() {
        let std::path::Component::Normal(component) = component else {
            return Err(invalid_submodule_path(raw_path));
        };
        let Some(component) = component.to_str() else {
            return Err(invalid_submodule_path(raw_path));
        };
        if component.is_empty() || is_git_directory_alias(component) {
            return Err(invalid_submodule_path(raw_path));
        }
        components.push(component.to_owned());
    }
    if components.is_empty() {
        return Err(invalid_submodule_path(raw_path));
    }

    let root = fs::canonicalize(&repo.root)?;
    let mut current = root.clone();
    let mut missing = false;
    for component in &components {
        if missing {
            break;
        }
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(_) => {
                let resolved =
                    fs::canonicalize(&current).map_err(|_| invalid_submodule_path(raw_path))?;
                if !resolved.starts_with(&root) {
                    return Err(invalid_submodule_path(raw_path));
                }
                current = resolved;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => missing = true,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(repo.root.join(raw_path))
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct SubmoduleCloneFilterOptions {
    pub(crate) filter: Option<String>,
    pub(crate) also_filter_submodules: bool,
}

impl SubmoduleCloneFilterOptions {
    fn from_parent_repository(repo: &GitRepo) -> Result<Self> {
        let enabled = read_config_value(repo, CLONE_SUBMODULE_FILTER_CONFIG_KEY)?
            .is_some_and(|value| value.eq_ignore_ascii_case("true"));
        if !enabled {
            return Ok(Self::default());
        }
        let filter = configured_partial_clone_filter(repo)?;
        let Some(filter) = filter else {
            return Err(CliError::Fatal {
                code: 128,
                message: "partial clone submodule propagation has no effective filter".into(),
            });
        };
        Ok(Self {
            filter: Some(filter),
            also_filter_submodules: true,
        })
    }

    fn for_update(filter: Option<String>, recursive: bool) -> Self {
        Self {
            also_filter_submodules: recursive && filter.is_some(),
            filter,
        }
    }

    fn child_also_filter_submodules(&self, recurse_submodules: &[String]) -> bool {
        self.also_filter_submodules && self.filter.is_some() && !recurse_submodules.is_empty()
    }

    fn child_configs(&self, recurse_submodules: &[String]) -> Vec<String> {
        if self.child_also_filter_submodules(recurse_submodules) {
            vec![format!("{CLONE_SUBMODULE_FILTER_CONFIG_KEY}=true")]
        } else {
            Vec::new()
        }
    }

    fn child_configs_with_state(
        &self,
        recurse_submodules: &[String],
        state: &SubmoduleRecursionState,
    ) -> Vec<String> {
        let mut configs = self.child_configs(recurse_submodules);
        if !recurse_submodules.is_empty() {
            configs.extend(state.child_configs());
        }
        configs
    }
}

fn configured_partial_clone_filter(repo: &GitRepo) -> Result<Option<String>> {
    let configured_remote = read_config_value(repo, "extensions.partialClone")?;
    if let Some(remote) = configured_remote {
        return read_config_section_value(repo, "remote", &remote, "partialclonefilter")
            .map_err(CliError::Io);
    }
    let config_path = local_config_path(repo).map_err(CliError::Io)?;
    let entries = read_config_file(&config_path).map_err(CliError::Io)?;
    Ok(entries
        .into_iter()
        .rev()
        .find(|entry| entry.section == "remote" && entry.key == "partialclonefilter")
        .map(|entry| entry.value))
}

fn submodule_usage() -> &'static str {
    "usage: git submodule [--quiet] [--cached]
   or: git submodule [--quiet] add [-b <branch>] [-f|--force] [--name <name>] [--reference <repository>] [--] <repository> [<path>]
   or: git submodule [--quiet] status [--cached] [--recursive] [--] [<path>...]
   or: git submodule [--quiet] init [--] [<path>...]
   or: git submodule [--quiet] deinit [-f|--force] (--all| [--] <path>...)
   or: git submodule [--quiet] update [--init [--filter=<filter-spec>]] [--remote] [-N|--no-fetch] [-f|--force] [--checkout|--merge|--rebase] [--[no-]recommend-shallow] [--reference <repository>] [--recursive] [--[no-]single-branch] [--] [<path>...]
   or: git submodule [--quiet] set-branch (--default|--branch <branch>) [--] <path>
   or: git submodule [--quiet] set-url [--] <path> <newurl>
   or: git submodule [--quiet] summary [--cached|--files] [--summary-limit <n>] [commit] [--] [<path>...]
   or: git submodule [--quiet] foreach [--recursive] <command>
   or: git submodule [--quiet] sync [--recursive] [--] [<path>...]
   or: git submodule [--quiet] absorbgitdirs [--] [<path>...]
"
}

fn submodule_usage_error() -> CliError {
    CliError::Stderr {
        code: 1,
        text: submodule_usage().to_owned(),
    }
}

pub(crate) fn clone_submodules(
    repo: &GitRepo,
    parent_repository: &str,
    active_specs: &[String],
    remote_submodules: bool,
    shallow_submodules: bool,
) -> Result<()> {
    let filter_options = SubmoduleCloneFilterOptions::from_parent_repository(repo)?;
    clone_submodules_with_filter(
        repo,
        parent_repository,
        active_specs,
        remote_submodules,
        shallow_submodules,
        &filter_options,
    )
}

pub(crate) fn clone_submodules_with_filter(
    repo: &GitRepo,
    parent_repository: &str,
    active_specs: &[String],
    remote_submodules: bool,
    shallow_submodules: bool,
    filter_options: &SubmoduleCloneFilterOptions,
) -> Result<()> {
    let mut state = SubmoduleRecursionState::from_parent_repository(repo)?;
    clone_submodules_with_filter_and_state(
        repo,
        parent_repository,
        active_specs,
        remote_submodules,
        shallow_submodules,
        filter_options,
        &mut state,
    )
}

fn clone_submodules_with_filter_and_state(
    repo: &GitRepo,
    parent_repository: &str,
    active_specs: &[String],
    remote_submodules: bool,
    shallow_submodules: bool,
    filter_options: &SubmoduleCloneFilterOptions,
    state: &mut SubmoduleRecursionState,
) -> Result<()> {
    state.account_context(
        parent_repository,
        active_specs,
        filter_options.filter.as_deref(),
    )?;
    let modules = read_gitmodules(repo)?;
    if modules.is_empty() {
        return Ok(());
    }
    set_config_value(
        repo,
        "submodule.active",
        &submodule_active_value(active_specs),
    )?;
    let index = read_repo_index(repo)?;
    for module in modules {
        let destination = validate_submodule_path(repo, &module.path)?;
        if !submodule_selected(&module.path, active_specs) {
            continue;
        }
        let path_bytes = module.path.as_bytes();
        let Some(entry) = index
            .entries()
            .iter()
            .find(|entry| entry.mode == IndexMode::Gitlink && entry.path.as_slice() == path_bytes)
        else {
            continue;
        };
        let url = resolve_submodule_clone_url(parent_repository, &module.url);
        state.enter(parent_repository, &module, &entry.id)?;
        set_config_value(repo, &format!("submodule.{}.url", module.name), &url)?;
        let recurse_submodules = nested_submodule_specs(&module.path, active_specs);
        let clone_was_absent = !destination.exists();
        crate::cli::commands::register_runtime_services();
        let clone_result = run_clone_service(CloneOptions {
            quiet: false,
            configs: filter_options.child_configs_with_state(&recurse_submodules, state),
            template: None,
            reject_shallow: false,
            recurse_submodules: recurse_submodules.clone(),
            remote_submodules,
            shallow_submodules,
            sparse: false,
            bare: false,
            mirror: false,
            no_checkout: false,
            worktree_first: false,
            background_fetch: false,
            demand_hydrate: false,
            remote_name: "origin".to_owned(),
            no_tags: false,
            single_branch: false,
            no_single_branch: false,
            separate_git_dir: None,
            references: Vec::new(),
            reference_if_able: Vec::new(),
            shared: false,
            dissociate: false,
            no_hardlinks: false,
            no_local: false,
            depth: shallow_submodules.then(|| "1".to_owned()),
            shallow_since: None,
            shallow_exclude: Vec::new(),
            branch: None,
            server_options: Vec::new(),
            upload_pack: None,
            filter: filter_options.filter.clone(),
            also_filter_submodules: filter_options
                .child_also_filter_submodules(&recurse_submodules),
            bundle_uri: None,
            ref_format: state.ref_format.clone(),
            keep_partial_on_missing_branch: false,
            repository: url,
            directory: Some(destination.clone()),
        });
        if let Err(error) = clone_result {
            if clone_was_absent {
                let _ = remove_path_if_exists(&destination);
            }
            return Err(error);
        }
        if !remote_submodules {
            if let Err(error) = checkout_submodule_gitlink(&destination, &entry.id) {
                if clone_was_absent {
                    let _ = remove_path_if_exists(&destination);
                }
                return Err(error);
            }
        }
        state.leave();
    }
    Ok(())
}

pub(crate) fn fetch_submodules_on_demand(repo: &GitRepo, remote: &str) -> Result<()> {
    let modules = read_gitmodules(repo)?;
    if modules.is_empty() {
        return Ok(());
    }

    let algorithm = repo_hash_algorithm_from_config(repo)?;
    let refs = RefStore::new(&repo.git_dir, algorithm);
    let store = LooseObjectStore::new(repo.objects_dir.clone(), algorithm);
    let tree_cache = TreeObjectCache::new(&store);
    let mut targets = BTreeMap::<String, ObjectId>::new();
    let prefix = format!("refs/remotes/{remote}/");
    refs.for_each_resolved_ref(&prefix, |ref_name, id| {
        if ref_name == format!("{prefix}HEAD") {
            return Ok::<(), CliError>(());
        }
        let tree = read_commit_tree_uncached(&store, id)?;
        let index = tree_cache.read_tree_to_index(&tree)?;
        for module in &modules {
            let Some(entry) = submodule_gitlink_entry(&index, &module.path) else {
                continue;
            };
            targets
                .entry(module.path.clone())
                .or_insert_with(|| entry.id.clone());
        }
        Ok::<(), CliError>(())
    })?;
    fetch_submodule_targets(repo, modules, targets)
}

pub(crate) fn fetch_submodules_for_commits_on_demand(
    repo: &GitRepo,
    commits: &[ObjectId],
) -> Result<()> {
    let modules = read_gitmodules(repo)?;
    if modules.is_empty() {
        return Ok(());
    }
    let algorithm = repo_hash_algorithm_from_config(repo)?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), algorithm);
    let tree_cache = TreeObjectCache::new(&store);
    let mut targets = BTreeMap::<String, ObjectId>::new();
    for id in commits {
        let Ok(tree) = read_commit_tree_uncached(&store, id) else {
            continue;
        };
        let index = tree_cache.read_tree_to_index(&tree)?;
        for module in &modules {
            let Some(entry) = submodule_gitlink_entry(&index, &module.path) else {
                continue;
            };
            targets
                .entry(module.path.clone())
                .or_insert_with(|| entry.id.clone());
        }
    }
    fetch_submodule_targets(repo, modules, targets)
}

fn fetch_submodule_targets(
    repo: &GitRepo,
    modules: Vec<GitmodulesEntry>,
    targets: BTreeMap<String, ObjectId>,
) -> Result<()> {
    if targets.is_empty() {
        return Ok(());
    }
    let parent_repository = submodule_parent_repository(repo);
    let filter_options = SubmoduleCloneFilterOptions::from_parent_repository(repo)?;
    for module in modules {
        let Some(target) = targets.get(&module.path) else {
            continue;
        };
        let path = repo.root.join(&module.path);
        if exact_repo_at(&path).is_none() {
            continue;
        }
        fetch_submodule_target(
            repo,
            &module,
            &path,
            &parent_repository,
            target,
            &filter_options,
        )?;
        let submodule_repo = find_repo_at(&path)?;
        fetch_submodules_on_demand(&submodule_repo, "origin")?;
    }
    Ok(())
}

pub(crate) fn init_submodules(args: &[String]) -> Result<()> {
    let (quiet, paths) = parse_submodule_quiet_paths(args);
    let repo = find_repo()?;
    let modules = selected_gitmodules(&repo, &paths)?;
    let parent_repository = submodule_parent_repository(&repo);
    for module in modules {
        let url_key = format!("submodule.{}.url", module.name);
        if read_config_value(&repo, &url_key)?.is_some() {
            continue;
        }
        let url = resolve_submodule_clone_url(&parent_repository, &module.url);
        set_config_value(&repo, &url_key, &url)?;
        set_config_value(&repo, &format!("submodule.{}.active", module.name), "true")?;
        if !quiet {
            eprintln!(
                "Submodule '{}' ({}) registered for path '{}'",
                module.name, url, module.path
            );
        }
    }
    Ok(())
}

pub(crate) fn sync_submodules(args: &[String]) -> Result<()> {
    let (quiet, paths) = parse_submodule_quiet_paths(args);
    let repo = find_repo()?;
    let modules = selected_gitmodules(&repo, &paths)?;
    let parent_repository = submodule_parent_repository(&repo);
    for module in modules {
        let url = resolve_submodule_clone_url(&parent_repository, &module.url);
        set_config_value(&repo, &format!("submodule.{}.url", module.name), &url)?;
        if !quiet {
            println!("Synchronizing submodule url for '{}'", module.path);
        }
    }
    Ok(())
}

pub(crate) fn update_submodules(args: &[String]) -> Result<()> {
    let mut init = false;
    let mut recursive = false;
    let mut quiet = false;
    let mut depth = None;
    let mut dissociate = false;
    let mut single_branch = false;
    let mut no_single_branch = false;
    let mut remote = false;
    let mut no_fetch = false;
    let mut filter = None;
    let mut strategy = SubmoduleUpdateStrategy::Checkout;
    let mut references = Vec::new();
    let mut paths = Vec::new();
    let mut path_args = false;
    let mut cursor = 0usize;
    while cursor < args.len() {
        let arg = &args[cursor];
        if !path_args && arg == "--" {
            path_args = true;
        } else if !path_args && arg == "--init" {
            init = true;
        } else if !path_args && arg == "--recursive" {
            recursive = true;
        } else if !path_args && arg == "--remote" {
            remote = true;
        } else if !path_args && (arg == "-N" || arg == "--no-fetch") {
            no_fetch = true;
        } else if !path_args && (arg == "-q" || arg == "--quiet") {
            quiet = true;
        } else if !path_args && arg == "--no-quiet" {
            quiet = false;
        } else if !path_args && (arg == "--progress" || arg == "--no-progress") {
        } else if !path_args && arg == "--single-branch" {
            single_branch = true;
            no_single_branch = false;
        } else if !path_args && arg == "--no-single-branch" {
            single_branch = false;
            no_single_branch = true;
        } else if !path_args && arg == "--checkout" {
            strategy = SubmoduleUpdateStrategy::Checkout;
        } else if !path_args && arg == "--merge" {
            strategy = SubmoduleUpdateStrategy::Merge;
        } else if !path_args && arg == "--rebase" {
            strategy = SubmoduleUpdateStrategy::Rebase;
        } else if !path_args && arg == "--dissociate" {
            dissociate = true;
        } else if !path_args && arg == "--depth" {
            cursor += 1;
            let Some(value) = args.get(cursor) else {
                return Err(CliError::Fatal {
                    code: 129,
                    message: "--depth requires a value".into(),
                });
            };
            depth = Some(value.clone());
        } else if !path_args && arg.starts_with("--depth=") {
            depth = Some(arg["--depth=".len()..].to_owned());
        } else if !path_args && arg == "--filter" {
            cursor += 1;
            let Some(value) = args.get(cursor) else {
                return Err(CliError::Fatal {
                    code: 129,
                    message: "--filter requires a value".into(),
                });
            };
            filter = Some(value.clone());
        } else if !path_args && arg.starts_with("--filter=") {
            filter = Some(arg["--filter=".len()..].to_owned());
        } else if !path_args
            && matches!(
                arg.as_str(),
                "--force" | "-f" | "--recommend-shallow" | "--no-recommend-shallow"
            )
        {
        } else if !path_args && arg == "--reference" {
            cursor += 1;
            let Some(value) = args.get(cursor) else {
                return Err(CliError::Fatal {
                    code: 129,
                    message: "--reference requires a value".into(),
                });
            };
            references.push(PathBuf::from(value));
        } else if !path_args && arg.starts_with("--reference=") {
            references.push(PathBuf::from(arg["--reference=".len()..].to_owned()));
        } else if !path_args && (arg == "--jobs" || arg == "-j") {
            cursor += 1;
            if cursor >= args.len() {
                return Err(CliError::Fatal {
                    code: 129,
                    message: format!("{arg} requires a value"),
                });
            }
        } else if !path_args && (arg.starts_with("--jobs=") || arg.starts_with("-j")) {
        } else if !path_args && arg.starts_with('-') {
            return Err(submodule_usage_error());
        } else {
            paths.push(arg.clone());
        }
        cursor += 1;
    }
    if init {
        init_submodules(&paths)?;
    }
    let repo = find_repo()?;
    let modules = selected_gitmodules(&repo, &paths)?;
    let index = read_repo_index(&repo)?;
    let parent_repository = submodule_parent_repository(&repo);
    let mut state = SubmoduleRecursionState::from_parent_repository(&repo)?;
    let filter_options = if filter.is_some() {
        SubmoduleCloneFilterOptions::for_update(filter.clone(), recursive)
    } else if recursive {
        SubmoduleCloneFilterOptions::from_parent_repository(&repo)?
    } else {
        SubmoduleCloneFilterOptions::default()
    };
    state.account_context(&parent_repository, &paths, filter.as_deref())?;
    for module in modules {
        let path = validate_submodule_path(&repo, &module.path)?;
        let Some(entry) = submodule_gitlink_entry(&index, &module.path) else {
            continue;
        };
        state.enter(&parent_repository, &module, &entry.id)?;
        let clone_was_absent = !path.exists();
        if exact_repo_at(&path).is_none() {
            let url = read_config_value(&repo, &format!("submodule.{}.url", module.name))?
                .unwrap_or_else(|| resolve_submodule_clone_url(&parent_repository, &module.url));
            crate::cli::commands::register_runtime_services();
            let clone_result = run_clone_service(CloneOptions {
                quiet,
                configs: if recursive {
                    state.child_configs()
                } else {
                    Vec::new()
                },
                template: None,
                reject_shallow: false,
                recurse_submodules: Vec::new(),
                remote_submodules: false,
                shallow_submodules: false,
                sparse: false,
                bare: false,
                mirror: false,
                no_checkout: false,
                worktree_first: false,
                background_fetch: false,
                demand_hydrate: false,
                remote_name: "origin".to_owned(),
                no_tags: false,
                single_branch,
                no_single_branch,
                separate_git_dir: None,
                references: references.clone(),
                reference_if_able: Vec::new(),
                shared: false,
                dissociate,
                no_hardlinks: false,
                no_local: false,
                depth: depth.clone(),
                shallow_since: None,
                shallow_exclude: Vec::new(),
                branch: None,
                server_options: Vec::new(),
                upload_pack: None,
                filter: filter_options.filter.clone(),
                also_filter_submodules: false,
                bundle_uri: None,
                ref_format: state.ref_format.clone(),
                keep_partial_on_missing_branch: false,
                repository: url,
                directory: Some(path.clone()),
            });
            if let Err(error) = clone_result {
                if clone_was_absent {
                    let _ = remove_path_if_exists(&path);
                }
                return Err(error);
            }
        }
        let update_result = (|| {
            let checkout_id = if remote {
                update_submodule_remote_head(&repo, &module, &path, &parent_repository, no_fetch)?
            } else {
                entry.id.clone()
            };
            update_submodule_checkout(&path, &checkout_id, strategy)?;
            absorb_submodule_gitdir(&repo, &module.path, &module.name)?;
            Ok::<ObjectId, CliError>(checkout_id)
        })();
        let checkout_id = match update_result {
            Ok(checkout_id) => checkout_id,
            Err(error) => {
                if clone_was_absent {
                    let _ = remove_path_if_exists(&path);
                }
                return Err(error);
            }
        };
        if !quiet {
            println!(
                "Submodule path '{}': checked out '{}'",
                module.path,
                checkout_id.to_hex()
            );
        }
        if recursive {
            let submodule_repo = find_repo_at(&path)?;
            clone_submodules_with_filter_and_state(
                &submodule_repo,
                &module.url,
                &[".".to_owned()],
                false,
                false,
                &filter_options,
                &mut state,
            )?;
        }
        state.leave();
    }
    Ok(())
}

pub(crate) fn foreach_submodules(args: &[String]) -> Result<()> {
    let mut quiet = false;
    let mut recursive = false;
    let mut command = Vec::new();
    let mut cursor = 0usize;
    while cursor < args.len() {
        let arg = &args[cursor];
        if command.is_empty() && arg == "--quiet" {
            quiet = true;
        } else if command.is_empty() && arg == "--recursive" {
            recursive = true;
        } else {
            command.extend(args[cursor..].iter().cloned());
            break;
        }
        cursor += 1;
    }
    if command.is_empty() {
        return Err(CliError::Fatal {
            code: 129,
            message: "submodule foreach requires a command".into(),
        });
    }
    let repo = find_repo()?;
    foreach_submodules_for_repo(&repo, &command.join(" "), quiet, recursive, "")
}

pub(crate) fn deinit_submodules(args: &[String]) -> Result<()> {
    let mut force = false;
    let mut all = false;
    let mut quiet = false;
    let mut paths = Vec::new();
    let mut path_args = false;
    for arg in args {
        match arg.as_str() {
            "--" if !path_args => path_args = true,
            "-f" | "--force" if !path_args => force = true,
            "-q" | "--quiet" if !path_args => quiet = true,
            "--no-quiet" if !path_args => quiet = false,
            "--all" if !path_args => all = true,
            option if !path_args && option.starts_with('-') => {
                return Err(submodule_usage_error());
            }
            path => paths.push(path.to_owned()),
        }
    }
    let repo = find_repo()?;
    let modules = if all {
        selected_gitmodules(&repo, &[])?
    } else {
        if paths.is_empty() {
            return Err(CliError::Fatal {
                code: 129,
                message: "submodule deinit requires a path or --all".into(),
            });
        }
        selected_gitmodules(&repo, &paths)?
    };
    for module in modules {
        let path = repo.root.join(&module.path);
        if path.exists() {
            if !force && path.read_dir()?.next().is_some() {
                return Err(CliError::Fatal {
                    code: 128,
                    message: format!(
                        "Submodule work tree '{}' contains local modifications; use '-f' to discard them",
                        module.path
                    ),
                });
            }
            fs::remove_dir_all(&path)?;
            fs::create_dir_all(&path)?;
            if !quiet {
                println!("Cleared directory '{}'", module.path);
            }
        }
        let _ = unset_config_value(&repo, &format!("submodule.{}.url", module.name));
        let _ = unset_config_value(&repo, &format!("submodule.{}.active", module.name));
        if !quiet {
            println!(
                "Submodule '{}' ({}) unregistered for path '{}'",
                module.name, module.url, module.path
            );
        }
    }
    Ok(())
}

pub(crate) fn set_submodule_branch(args: &[String]) -> Result<()> {
    let mut default = false;
    let mut branch = None;
    let mut paths = Vec::new();
    let mut path_args = false;
    let mut cursor = 0usize;
    while cursor < args.len() {
        let arg = &args[cursor];
        if !path_args && arg == "--" {
            path_args = true;
        } else if !path_args && (arg == "-q" || arg == "--quiet" || arg == "--no-quiet") {
        } else if !path_args && arg == "--default" {
            default = true;
        } else if !path_args && (arg == "-b" || arg == "--branch") {
            cursor += 1;
            let Some(value) = args.get(cursor) else {
                return Err(CliError::Fatal {
                    code: 129,
                    message: format!("{arg} requires a value"),
                });
            };
            branch = Some(value.clone());
        } else if !path_args && arg.starts_with("--branch=") {
            branch = Some(arg["--branch=".len()..].to_owned());
        } else if !path_args && arg.starts_with('-') {
            return Err(submodule_usage_error());
        } else {
            paths.push(arg.clone());
        }
        cursor += 1;
    }
    if default == branch.is_some() {
        return Err(CliError::Fatal {
            code: 129,
            message: "submodule set-branch requires exactly one of --default or --branch".into(),
        });
    }
    if paths.len() != 1 {
        return Err(CliError::Fatal {
            code: 129,
            message: "submodule set-branch requires a path".into(),
        });
    }
    let repo = find_repo()?;
    let modules = selected_gitmodules(&repo, &paths)?;
    let module = modules.first().ok_or_else(|| CliError::Fatal {
        code: 128,
        message: format!(
            "no submodule mapping found in .gitmodules for path '{}'",
            paths[0]
        ),
    })?;
    let gitmodules = repo.root.join(".gitmodules");
    let key = format!("submodule.{}.branch", module.name);
    if let Some(branch) = branch {
        set_config_value_in_file(&gitmodules, &key, &branch)?;
    } else {
        let _ = unset_config_value_in_file(&gitmodules, &key);
    }
    Ok(())
}

pub(crate) fn set_submodule_url(args: &[String]) -> Result<()> {
    let (quiet, values) = parse_submodule_quiet_paths(args);
    if values.len() != 2 {
        return Err(CliError::Fatal {
            code: 129,
            message: "submodule set-url requires <path> <newurl>".into(),
        });
    }
    let repo = find_repo()?;
    let modules = selected_gitmodules(&repo, &[values[0].clone()])?;
    let module = modules.first().ok_or_else(|| CliError::Fatal {
        code: 128,
        message: format!(
            "no submodule mapping found in .gitmodules for path '{}'",
            values[0]
        ),
    })?;
    let resolved_url = resolve_submodule_set_url(&repo, &values[1])?;
    set_config_value_in_file(
        &repo.root.join(".gitmodules"),
        &format!("submodule.{}.url", module.name),
        &values[1],
    )?;
    set_config_value(
        &repo,
        &format!("submodule.{}.url", module.name),
        &resolved_url,
    )?;
    if !quiet {
        println!("Synchronizing submodule url for '{}'", module.path);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SubmoduleSummaryMode {
    Worktree,
    Cached,
    Files,
}

struct SubmoduleSummaryOptions {
    mode: SubmoduleSummaryMode,
    summary_limit: usize,
    positionals: Vec<String>,
    paths: Vec<String>,
}

pub(crate) fn summary_submodules(args: &[String]) -> Result<()> {
    let options = parse_submodule_summary_options(args)?;
    let repo = find_repo()?;
    let algorithm = repo_hash_algorithm_from_config(&repo).map_err(CliError::Io)?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), algorithm);
    let (commit, paths) = submodule_summary_commit_and_paths(&repo, &store, &options)?;
    let index = read_repo_index(&repo)?;
    let base_index = if options.mode != SubmoduleSummaryMode::Files {
        Some(submodule_summary_base_index(
            &repo,
            &store,
            commit.as_deref(),
        )?)
    } else {
        None
    };
    let modules = selected_gitmodules(&repo, &paths)?;
    for module in modules {
        let path = repo.root.join(&module.path);
        let path_bytes = module.path.as_bytes();
        let old_id = if let Some(base_index) = base_index.as_ref() {
            find_index_entry(base_index, path_bytes)
                .map(|entry| entry.id.clone())
                .unwrap_or_else(zero_object_id)
        } else {
            let Some(entry) = submodule_gitlink_entry(&index, &module.path) else {
                continue;
            };
            entry.id
        };
        let new_id = if options.mode == SubmoduleSummaryMode::Cached {
            find_index_entry(&index, path_bytes)
                .map(|entry| entry.id.clone())
                .unwrap_or_else(zero_object_id)
        } else {
            let Some(state) = submodule_head_state(&path, &old_id, false) else {
                continue;
            };
            state.id
        };
        if old_id == new_id {
            continue;
        }
        print_submodule_summary(&path, &module.path, &old_id, &new_id, options.summary_limit)?;
    }
    Ok(())
}

fn submodule_summary_commit_and_paths(
    repo: &GitRepo,
    store: &LooseObjectStore,
    options: &SubmoduleSummaryOptions,
) -> Result<(Option<String>, Vec<String>)> {
    let Some(first) = options.positionals.first() else {
        return Ok((None, options.paths.clone()));
    };
    if submodule_summary_resolves_treeish(repo, store, first) {
        let mut paths = options.positionals[1..].to_vec();
        paths.extend(options.paths.iter().cloned());
        Ok((Some(first.clone()), paths))
    } else {
        let mut paths = options.positionals.clone();
        paths.extend(options.paths.iter().cloned());
        Ok((None, paths))
    }
}

fn submodule_summary_resolves_treeish(
    repo: &GitRepo,
    store: &LooseObjectStore,
    value: &str,
) -> bool {
    let tree_cache = TreeObjectCache::new(store);
    read_treeish_index_cached(repo, store, &tree_cache, value).is_ok()
}

fn submodule_summary_base_index(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit: Option<&str>,
) -> Result<GitIndex> {
    let Some(commit) = commit else {
        return read_head_index(repo);
    };
    let tree_cache = TreeObjectCache::new(store);
    read_treeish_index_cached(repo, store, &tree_cache, commit)
}

fn parse_submodule_summary_options(args: &[String]) -> Result<SubmoduleSummaryOptions> {
    let mut mode = SubmoduleSummaryMode::Worktree;
    let mut summary_limit = 10usize;
    let mut positionals = Vec::new();
    let mut paths = Vec::new();
    let mut path_args = false;
    let mut cursor = 0usize;
    while cursor < args.len() {
        let arg = &args[cursor];
        if !path_args && arg == "--" {
            path_args = true;
        } else if !path_args && (arg == "-q" || arg == "--quiet" || arg == "--no-quiet") {
        } else if !path_args && arg == "--cached" {
            mode = SubmoduleSummaryMode::Cached;
        } else if !path_args && arg == "--files" {
            mode = SubmoduleSummaryMode::Files;
        } else if !path_args && arg == "--summary-limit" {
            cursor += 1;
            let Some(value) = args.get(cursor) else {
                return Err(CliError::Fatal {
                    code: 129,
                    message: "--summary-limit requires a value".into(),
                });
            };
            summary_limit = parse_submodule_summary_limit(value)?;
        } else if !path_args && arg == "-n" {
            cursor += 1;
            let Some(value) = args.get(cursor) else {
                return Err(CliError::Fatal {
                    code: 129,
                    message: "-n requires a value".into(),
                });
            };
            summary_limit = parse_submodule_summary_limit(value)?;
        } else if !path_args && arg.starts_with("--summary-limit=") {
            summary_limit = parse_submodule_summary_limit(&arg["--summary-limit=".len()..])?;
        } else if !path_args && arg.starts_with('-') {
            return Err(submodule_usage_error());
        } else if path_args {
            paths.push(arg.clone());
        } else {
            positionals.push(arg.clone());
        }
        cursor += 1;
    }
    Ok(SubmoduleSummaryOptions {
        mode,
        summary_limit,
        positionals,
        paths,
    })
}

fn parse_submodule_summary_limit(value: &str) -> Result<usize> {
    value.parse::<usize>().map_err(|_| CliError::Fatal {
        code: 129,
        message: format!("invalid summary-limit '{value}'"),
    })
}

fn print_submodule_summary(
    path: &std::path::Path,
    display_path: &str,
    old_id: &ObjectId,
    new_id: &ObjectId,
    summary_limit: usize,
) -> Result<()> {
    let submodule_repo = find_repo_at(path)?;
    let algorithm = repo_hash_algorithm_from_config(&submodule_repo).map_err(CliError::Io)?;
    let store = LooseObjectStore::new(submodule_repo.objects_dir.clone(), algorithm);
    let commits = submodule_commit_range(&submodule_repo, &store, old_id, new_id)?;
    println!(
        "* {display_path} {}...{} ({}):",
        old_id.short_hex(7),
        new_id.short_hex(7),
        commits.len()
    );
    let commit_cache = CommitObjectCache::new(&store);
    for id in commits.iter().rev().take(summary_limit) {
        let commit = commit_cache.read_commit(id)?;
        println!("  > {}", commit_subject(&commit.message));
    }
    println!();
    Ok(())
}

fn resolve_submodule_set_url(repo: &GitRepo, url: &str) -> Result<String> {
    if !(url.starts_with("./") || url.starts_with("../")) {
        return Ok(url.to_owned());
    }
    let resolved = lexical_normalize_path(&repo.root.join(url));
    #[cfg(windows)]
    {
        return Ok(resolved.to_string_lossy().replace('\\', "/"));
    }
    #[cfg(not(windows))]
    {
        Ok(resolved.display().to_string())
    }
}

fn lexical_normalize_path(path: &std::path::Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            std::path::Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            std::path::Component::RootDir => normalized.push(component.as_os_str()),
            std::path::Component::Normal(part) => normalized.push(part),
        }
    }
    normalized
}

pub(crate) fn absorb_submodule_gitdirs(args: &[String]) -> Result<()> {
    let repo = find_repo()?;
    let (_, paths) = parse_submodule_quiet_paths(args);
    for module in selected_gitmodules(&repo, &paths)? {
        absorb_submodule_gitdir(&repo, &module.path, &module.name)?;
    }
    Ok(())
}

fn submodule_active_value(active_specs: &[String]) -> String {
    active_specs
        .first()
        .cloned()
        .unwrap_or_else(|| ".".to_owned())
}

fn parse_submodule_quiet_paths(args: &[String]) -> (bool, Vec<String>) {
    let mut quiet = false;
    let mut paths = Vec::new();
    let mut path_args = false;
    for arg in args {
        if !path_args && arg == "--" {
            path_args = true;
        } else if !path_args && (arg == "-q" || arg == "--quiet") {
            quiet = true;
        } else if !path_args && arg == "--no-quiet" {
            quiet = false;
        } else {
            paths.push(arg.clone());
        }
    }
    (quiet, paths)
}

fn selected_gitmodules(repo: &GitRepo, paths: &[String]) -> Result<Vec<GitmodulesEntry>> {
    let pathspecs = paths
        .iter()
        .map(|path| path.as_bytes().to_vec())
        .collect::<Vec<_>>();
    let mut modules = read_gitmodules(repo)?
        .into_iter()
        .filter(|module| pathspec_matches(module.path.as_bytes(), &pathspecs))
        .collect::<Vec<_>>();
    modules.sort_by(|left, right| left.path.cmp(&right.path));
    if !paths.is_empty() && modules.is_empty() {
        return Err(CliError::Message(format!(
            "pathspec '{}' did not match any file(s) known to git",
            paths[0]
        )));
    }
    Ok(modules)
}

pub(crate) fn submodule_parent_repository(repo: &GitRepo) -> String {
    read_config_value(repo, "remote.origin.url")
        .ok()
        .flatten()
        .unwrap_or_else(|| repo.root.display().to_string())
}

fn submodule_gitlink_entry(index: &GitIndex, path: &str) -> Option<IndexEntry> {
    index
        .entries()
        .iter()
        .find(|entry| entry.mode == IndexMode::Gitlink && entry.path.as_slice() == path.as_bytes())
        .cloned()
}

fn submodule_selected(path: &str, active_specs: &[String]) -> bool {
    active_specs
        .iter()
        .any(|spec| spec == "." || pathspec_matches(path.as_bytes(), &[spec.as_bytes().to_vec()]))
}

fn nested_submodule_specs(path: &str, active_specs: &[String]) -> Vec<String> {
    if active_specs
        .iter()
        .any(|spec| spec == "." || pathspec_matches(path.as_bytes(), &[spec.as_bytes().to_vec()]))
    {
        vec![".".to_owned()]
    } else {
        Vec::new()
    }
}

fn read_gitmodules(repo: &GitRepo) -> Result<Vec<GitmodulesEntry>> {
    let modules =
        parse_gitmodules_config_entries(read_config_file(&repo.root.join(".gitmodules"))?)?;
    validate_gitmodules_entries(repo, &modules)?;
    Ok(modules)
}

fn read_gitmodules_from_index(repo: &GitRepo, index: &GitIndex) -> Result<Vec<GitmodulesEntry>> {
    let Some(entry) = index.entries().iter().find(|entry| {
        entry.stage == 0
            && entry.path.as_slice() == b".gitmodules"
            && entry.mode != IndexMode::Gitlink
    }) else {
        return Ok(Vec::new());
    };
    let modules = parse_gitmodules_config_entries(parse_config_blob_entries(
        repo,
        &entry.id.to_hex(),
        false,
    )?)?;
    validate_gitmodules_entries(repo, &modules)?;
    Ok(modules)
}

fn validate_gitmodules_entries(repo: &GitRepo, modules: &[GitmodulesEntry]) -> Result<()> {
    if modules.len() > SUBMODULE_MAX_TOTAL_MODULES {
        return Err(submodule_limit_error("module count"));
    }
    let mut path_bytes = 0usize;
    let mut input_bytes = 0usize;
    for module in modules {
        validate_submodule_path(repo, &module.path)?;
        path_bytes = path_bytes
            .checked_add(module.path.len())
            .ok_or_else(|| submodule_limit_error("path size"))?;
        input_bytes = input_bytes
            .checked_add(module.name.len())
            .and_then(|value| value.checked_add(module.path.len()))
            .and_then(|value| value.checked_add(module.url.len()))
            .ok_or_else(|| submodule_limit_error("input size"))?;
    }
    if path_bytes > SUBMODULE_MAX_PATH_BYTES {
        return Err(submodule_limit_error("path size"));
    }
    if input_bytes > SUBMODULE_MAX_INPUT_BYTES {
        return Err(submodule_limit_error("input size"));
    }
    Ok(())
}

fn parse_gitmodules_config_entries(entries: Vec<ConfigEntry>) -> Result<Vec<GitmodulesEntry>> {
    let mut by_name = BTreeMap::<String, (Option<String>, Option<String>, Option<String>)>::new();
    for entry in entries {
        if entry.section != "submodule" || entry.subsection.is_empty() {
            continue;
        }
        let values = by_name.entry(entry.subsection.clone()).or_default();
        match entry.key.as_str() {
            "path" => values.0 = Some(entry.value),
            "url" => values.1 = Some(entry.value),
            "branch" => values.2 = Some(entry.value),
            _ => {}
        }
    }
    Ok(by_name
        .into_iter()
        .filter_map(|(name, (path, url, branch))| {
            Some(GitmodulesEntry {
                name,
                path: path?,
                url: url?,
                branch,
            })
        })
        .collect())
}

pub(crate) fn resolve_submodule_clone_url(parent_repository: &str, url: &str) -> String {
    if !(url.starts_with("./") || url.starts_with("../")) {
        return url.to_owned();
    }
    let Ok(Some(parent)) = local_repository_path_from_location(parent_repository) else {
        return url.to_owned();
    };
    let resolved = canonical_or_absolute(parent.join(url));
    #[cfg(windows)]
    {
        return resolved.to_string_lossy().replace('\\', "/");
    }
    #[cfg(not(windows))]
    {
        resolved.display().to_string()
    }
}

fn checkout_submodule_gitlink(path: &std::path::Path, id: &ObjectId) -> Result<()> {
    let repo = find_repo_at(path)?;
    let algorithm = repo_hash_algorithm_from_config(&repo)?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), algorithm);
    let refs = RefStore::new(&repo.git_dir, algorithm);
    checkout_worktree(&repo, &store, id)?;
    refs.write_head_direct(id)?;
    Ok(())
}

fn missing_read_tree_submodule_target_error(path: &str, id: &ObjectId) -> CliError {
    CliError::Stderr {
        code: 0,
        text: format!(
            "fatal: failed to unpack tree object {}\n\
error: Submodule '{path}' could not be updated.\n\
error: Submodule '{path}' cannot checkout new HEAD.\n\
error: Entry '{path}' not uptodate. Cannot merge.\n",
            id.to_hex()
        ),
    }
}

fn read_tree_submodule_display_path(super_prefix: Option<&str>, path: &str) -> String {
    match super_prefix {
        Some(prefix) if !prefix.is_empty() => format!("{prefix}{path}"),
        _ => path.to_owned(),
    }
}

fn read_tree_submodule_nested_prefix(super_prefix: Option<&str>, path: &str) -> String {
    format!("{}/", read_tree_submodule_display_path(super_prefix, path))
}

fn read_tree_submodule_repository_missing_error(
    module_name: &str,
    super_prefix: Option<&str>,
) -> CliError {
    let display_path = read_tree_submodule_display_path(super_prefix, module_name);
    CliError::Stderr {
        code: 128,
        text: format!(
            "fatal: not a git repository: ../.git/modules/{display_path}\n\
fatal: could not reset submodule index\n"
        ),
    }
}

fn read_tree_nested_submodule_repository_missing_error(
    parent_path: &str,
    missing_path: &str,
    reset_index: bool,
    super_prefix: Option<&str>,
) -> CliError {
    let reset = if reset_index {
        "fatal: could not reset submodule index\n"
    } else {
        ""
    };
    let parent_checkout_path = read_tree_submodule_display_path(super_prefix, parent_path);
    let checkout = if reset_index {
        String::new()
    } else {
        format!("error: Submodule '{parent_checkout_path}' cannot checkout new HEAD.\n")
    };
    CliError::Stderr {
        code: 128,
        text: format!(
            "fatal: not a git repository: {missing_path}\n\
{reset}\
error: Submodule '{parent_path}' could not be updated.\n\
{checkout}"
        ),
    }
}

fn read_tree_nested_submodule_missing_path(
    worktree: &Path,
    admin_dir: &Path,
    module_path: &str,
) -> String {
    if let Ok(contents) = fs::read_to_string(worktree.join(".git"))
        && let Some(target) = contents.strip_prefix("gitdir:")
    {
        return format!("{module_path}/{}", target.trim());
    }
    relative_path_between(worktree, admin_dir)
        .unwrap_or_else(|| admin_dir.to_path_buf())
        .display()
        .to_string()
}

fn missing_read_tree_submodule_transition_error(path: &str, id: &ObjectId) -> CliError {
    CliError::Stderr {
        code: 128,
        text: format!(
            "fatal: failed to unpack tree object {}\n\
error: Submodule '{path}' could not be updated.\n\
error: Submodule '{path}' cannot checkout new HEAD.\n",
            id.to_hex()
        ),
    }
}

fn warn_read_tree_submodule_gitdir_migration(
    module_path: &str,
    embedded_gitdir: &std::path::Path,
    admin_gitdir: &std::path::Path,
) {
    eprintln!(
        "Migrating git directory of '{module_path}' from\n'{}' to\n'{}'",
        embedded_gitdir.display(),
        admin_gitdir.display()
    );
}

fn read_tree_submodule_dirty_worktree_error(
    module_path: &str,
    changed_path: Option<&[u8]>,
    super_prefix: Option<&str>,
) -> CliError {
    let entry = changed_path
        .map(|path| format!("{module_path}/{}", String::from_utf8_lossy(path)))
        .unwrap_or_else(|| module_path.to_owned());
    let entry = read_tree_submodule_display_path(super_prefix, &entry);
    let module_path = read_tree_submodule_display_path(super_prefix, module_path);
    CliError::Stderr {
        code: 128,
        text: format!(
            "error: Entry '{entry}' not uptodate. Cannot merge.\n\
error: Submodule '{module_path}' could not be updated.\n\
error: Submodule '{module_path}' cannot checkout new HEAD.\n"
        ),
    }
}

fn read_tree_nested_submodule_dirty_worktree_error(
    module_path: &str,
    changed_path: Option<&[u8]>,
    super_prefix: Option<&str>,
) -> CliError {
    let entry = changed_path
        .map(|path| format!("{module_path}/{}", String::from_utf8_lossy(path)))
        .unwrap_or_else(|| module_path.to_owned());
    let entry = read_tree_submodule_display_path(super_prefix, &entry);
    CliError::Stderr {
        code: 128,
        text: format!(
            "error: Entry '{entry}' not uptodate. Cannot merge.\n\
error: Submodule '{module_path}' could not be updated.\n\
error: Submodule '{module_path}' cannot checkout new HEAD.\n"
        ),
    }
}

fn read_tree_submodule_dirty_index_error(
    module_path: &str,
    super_prefix: Option<&str>,
) -> CliError {
    let module_path = read_tree_submodule_display_path(super_prefix, module_path);
    CliError::Stderr {
        code: 128,
        text: format!(
            "error: submodule '{module_path}' has dirty index\n\
error: Submodule '{module_path}' cannot checkout new HEAD.\n\
error: Entry '{module_path}' not uptodate. Cannot merge.\n"
        ),
    }
}

fn read_tree_nested_submodule_dirty_index_error(
    module_path: &str,
    parent_path: &str,
    super_prefix: Option<&str>,
) -> CliError {
    let display_path =
        read_tree_submodule_display_path(super_prefix, &format!("{parent_path}/{module_path}"));
    let parent_update_path = parent_path.to_owned();
    let parent_checkout_path = read_tree_submodule_display_path(super_prefix, parent_path);
    CliError::Stderr {
        code: 128,
        text: format!(
            "error: submodule '{module_path}' has dirty index\n\
error: Submodule '{display_path}' cannot checkout new HEAD.\n\
error: Entry '{display_path}' not uptodate. Cannot merge.\n\
error: Submodule '{parent_update_path}' could not be updated.\n\
error: Submodule '{parent_checkout_path}' cannot checkout new HEAD.\n"
        ),
    }
}

fn validate_read_tree_gitmodules_transition(
    repo: &GitRepo,
    original_index: &GitIndex,
    result_index: &GitIndex,
    force: bool,
) -> Result<()> {
    let Some(original) = find_index_entry(original_index, b".gitmodules") else {
        return Ok(());
    };
    let Some(result) = find_index_entry(result_index, b".gitmodules") else {
        return Ok(());
    };
    if original.id == result.id && original.mode == result.mode && original.stage == result.stage {
        return Ok(());
    }
    let path = repo.root.join(".gitmodules");
    if !force && worktree_entry_modified(repo, &path, original)? {
        return Err(CliError::Stderr {
            code: 128,
            text: "error: Entry '.gitmodules' not uptodate. Cannot merge.\n".into(),
        });
    }
    Ok(())
}

pub(crate) fn validate_read_tree_submodule_targets(
    repo: &GitRepo,
    original_index: &GitIndex,
    index: &GitIndex,
    recurse: bool,
    force: bool,
    root_operation: ReadTreeSubmoduleRootOperation,
    root_contexts: Option<&ReadTreeSubmoduleRootContexts>,
    super_prefix: Option<&str>,
) -> Result<()> {
    if !recurse {
        return Ok(());
    }
    validate_read_tree_gitmodules_transition(repo, original_index, index, force)?;
    let frame = root_contexts
        .expect("read-tree recursive submodule contexts")
        .result();
    let targets = read_tree_submodule_validation_targets(
        repo,
        index,
        Some(original_index),
        &frame,
        true,
        Some(root_operation),
    )?;
    validate_read_tree_submodule_target_list(
        repo,
        targets,
        force,
        true,
        None,
        super_prefix,
        super_prefix,
    )
}

fn read_tree_submodule_validation_targets(
    repo: &GitRepo,
    index: &GitIndex,
    current_index: Option<&GitIndex>,
    frame: &ReadTreeSubmoduleFrameContext,
    include_missing_repository: bool,
    root_operation: Option<ReadTreeSubmoduleRootOperation>,
) -> Result<Vec<ReadTreeSubmoduleValidationTarget>> {
    let mut targets = Vec::new();
    for entry in index
        .entries()
        .iter()
        .filter(|entry| entry.stage == 0 && entry.mode == IndexMode::Gitlink)
    {
        let Some(module) = frame.module_for_path(&entry.path) else {
            continue;
        };
        if !frame.activation.is_active(module) {
            continue;
        }
        let transition = current_index.is_none_or(|current| {
            find_index_entry(current, module.path.as_bytes())
                .is_none_or(|current| current.mode != IndexMode::Gitlink || current.id != entry.id)
        });
        let worktree = repo.root.join(&module.path);
        let populated = exact_repo_at(&worktree).is_some();
        let admin_dir = read_tree_submodule_admin_dir(repo, module)?;
        let admin_dir_exists = admin_dir.exists();
        let worktree_state = read_tree_submodule_worktree_state(&worktree, &admin_dir)?;
        if root_operation.is_some_and(|operation| {
            operation.skips_unpopulated_root_validation()
                && matches!(
                    worktree_state,
                    ReadTreeSubmoduleWorktreeState::Absent
                        | ReadTreeSubmoduleWorktreeState::AdminOnly
                )
        }) {
            continue;
        }
        let missing_repository = !populated && !admin_dir_exists;
        if transition
            || populated
            || worktree_state == ReadTreeSubmoduleWorktreeState::Stale
            || (include_missing_repository && missing_repository)
        {
            targets.push(ReadTreeSubmoduleValidationTarget {
                module_name: module.name.clone(),
                module_path: module.path.clone(),
                id: entry.id.clone(),
                transition,
                worktree_state,
                admin_dir_exists,
            });
        }
    }
    Ok(targets)
}

fn validate_read_tree_submodule_target_list(
    repo: &GitRepo,
    targets: Vec<ReadTreeSubmoduleValidationTarget>,
    force: bool,
    root_level: bool,
    parent_path: Option<&str>,
    super_prefix: Option<&str>,
    root_super_prefix: Option<&str>,
) -> Result<()> {
    for validation_target in targets {
        let worktree = repo.root.join(&validation_target.module_path);
        let admin_dir = read_common_git_dir(&repo.git_dir)
            .map(|git_dir| git_dir.join("modules").join(&validation_target.module_name))?;
        let worktree_state = validation_target.worktree_state;
        if !root_level && worktree_state == ReadTreeSubmoduleWorktreeState::AdminOnly {
            continue;
        }
        if !root_level && worktree_state == ReadTreeSubmoduleWorktreeState::Absent {
            continue;
        }
        if !root_level
            && validation_target.transition
            && worktree_state == ReadTreeSubmoduleWorktreeState::Stale
            && !validation_target.admin_dir_exists
        {
            continue;
        }
        if root_level
            && worktree_state == ReadTreeSubmoduleWorktreeState::Absent
            && !validation_target.admin_dir_exists
        {
            continue;
        }
        let submodule_repo = if let Some(submodule_repo) = exact_repo_at(&worktree) {
            submodule_repo
        } else if validation_target.admin_dir_exists {
            GitRepo {
                root: worktree,
                index_path: admin_dir.join("index"),
                objects_dir: admin_dir.join("objects"),
                git_dir: admin_dir,
            }
        } else {
            if root_level {
                return Err(read_tree_submodule_repository_missing_error(
                    &validation_target.module_name,
                    super_prefix,
                ));
            }
            if worktree_state == ReadTreeSubmoduleWorktreeState::Stale {
                let parent_path = parent_path.ok_or_else(|| {
                    read_tree_submodule_repository_missing_error(
                        &validation_target.module_name,
                        super_prefix,
                    )
                })?;
                return Err(read_tree_nested_submodule_repository_missing_error(
                    parent_path,
                    &read_tree_nested_submodule_missing_path(
                        &worktree,
                        &admin_dir,
                        &validation_target.module_path,
                    ),
                    !worktree.join(".git").is_file(),
                    root_super_prefix,
                ));
            }
            continue;
        };
        let nested_targets = {
            let current_worktree_index = exact_repo_at(&submodule_repo.root)
                .map(|worktree_repo| read_repo_index(&worktree_repo))
                .transpose()?;
            let target_index = read_read_tree_submodule_target_index(
                &submodule_repo,
                &validation_target.module_path,
                &validation_target.id,
                validation_target.transition,
            )?;
            let frame = ReadTreeSubmoduleFrameContext::read(&submodule_repo, &target_index)?;
            let nested_targets = read_tree_submodule_validation_targets(
                &submodule_repo,
                &target_index,
                current_worktree_index.as_ref(),
                &frame,
                false,
                None,
            )?;
            if let Some(worktree_repo) = exact_repo_at(&submodule_repo.root) {
                let current_index = current_worktree_index
                    .as_ref()
                    .expect("populated submodule has a worktree index");
                let nested_super_prefix =
                    read_tree_submodule_nested_prefix(super_prefix, &validation_target.module_path);
                validate_read_tree_submodule_worktrees_with_context(
                    &worktree_repo,
                    current_index,
                    &target_index,
                    true,
                    force,
                    &frame,
                    Some(&validation_target.module_path),
                    Some(&nested_super_prefix),
                    root_super_prefix,
                )?;
                if validation_target.transition && parent_path.is_some() {
                    validate_read_tree_submodule_worktree_changes(
                        &worktree_repo,
                        &current_index,
                        &frame,
                        &validation_target.module_path,
                        Some(&nested_super_prefix),
                    )?;
                }
            }
            nested_targets
        };

        // Recurse using only compact module/id descriptors. The target index and
        // its tree cache are scoped above and are dropped before descending.
        validate_read_tree_submodule_target_list(
            &submodule_repo,
            nested_targets,
            force,
            false,
            Some(&validation_target.module_path),
            Some(&read_tree_submodule_nested_prefix(
                super_prefix,
                &validation_target.module_path,
            )),
            root_super_prefix,
        )?;
    }
    Ok(())
}

fn read_read_tree_submodule_target_index(
    submodule_repo: &GitRepo,
    module_path: &str,
    target_id: &ObjectId,
    transition: bool,
) -> Result<GitIndex> {
    let algorithm = repo_hash_algorithm_from_config(submodule_repo)?;
    let store = LooseObjectStore::new(submodule_repo.objects_dir.clone(), algorithm);
    let kind = store.object_kind_hint(target_id).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            if transition {
                missing_read_tree_submodule_transition_error(module_path, target_id)
            } else {
                missing_read_tree_submodule_target_error(module_path, target_id)
            }
        } else {
            CliError::Io(error)
        }
    })?;
    if kind != Some(GitObjectKind::Commit) {
        if kind.is_none() {
            return Err(if transition {
                missing_read_tree_submodule_transition_error(module_path, target_id)
            } else {
                missing_read_tree_submodule_target_error(module_path, target_id)
            });
        }
        return Err(CliError::Fatal {
            code: 128,
            message: format!(
                "submodule '{}' object {} is not a commit",
                module_path,
                target_id.to_hex()
            ),
        });
    }
    let mut links_cache = CommitLinksCache::new(&store);
    let tree = links_cache
        .read_links(target_id)
        .map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                if transition {
                    missing_read_tree_submodule_transition_error(module_path, target_id)
                } else {
                    missing_read_tree_submodule_target_error(module_path, target_id)
                }
            } else {
                CliError::Io(error)
            }
        })?
        .tree
        .clone();
    let tree_cache = TreeObjectCache::transient(&store);
    Ok(tree_cache.read_tree_to_index(&tree)?)
}

pub(crate) fn validate_read_tree_submodule_worktrees(
    repo: &GitRepo,
    original_index: &GitIndex,
    result_index: &GitIndex,
    recurse: bool,
    force: bool,
    root_contexts: Option<&ReadTreeSubmoduleRootContexts>,
    super_prefix: Option<&str>,
) -> Result<()> {
    if !recurse || force {
        return Ok(());
    }
    let frame = root_contexts
        .expect("read-tree recursive submodule contexts")
        .original()?;
    validate_read_tree_submodule_worktrees_with_context(
        repo,
        original_index,
        result_index,
        recurse,
        force,
        &frame,
        None,
        super_prefix,
        super_prefix,
    )
}

fn validate_read_tree_submodule_worktrees_with_context(
    repo: &GitRepo,
    original_index: &GitIndex,
    result_index: &GitIndex,
    recurse: bool,
    force: bool,
    frame: &ReadTreeSubmoduleFrameContext,
    parent_path: Option<&str>,
    super_prefix: Option<&str>,
    root_super_prefix: Option<&str>,
) -> Result<()> {
    if force {
        return Ok(());
    }
    if recurse {
        for entry in original_index
            .entries()
            .iter()
            .filter(|entry| entry.stage == 0 && entry.mode == IndexMode::Gitlink)
        {
            let result = find_index_entry(result_index, &entry.path);
            if result
                .is_some_and(|result| result.mode == IndexMode::Gitlink && result.id == entry.id)
            {
                continue;
            }
            let removed = result.is_none();
            let Some(module) = frame.module_for_path(&entry.path) else {
                continue;
            };
            let path = module.path.as_str();
            if !frame.activation.is_active(module) {
                continue;
            }
            let worktree = repo.root.join(&path);
            let admin_dir = read_tree_submodule_admin_dir(repo, module)?;
            if read_tree_submodule_worktree_state(&worktree, &admin_dir)?
                == ReadTreeSubmoduleWorktreeState::Stale
            {
                if !removed && !admin_dir.exists() && parent_path.is_some() {
                    continue;
                }
                let missing_path =
                    read_tree_nested_submodule_missing_path(&worktree, &admin_dir, &path);
                if let Some(parent_path) = parent_path {
                    return Err(read_tree_nested_submodule_repository_missing_error(
                        parent_path,
                        &missing_path,
                        !worktree.join(".git").is_file(),
                        root_super_prefix,
                    ));
                }
                return Err(read_tree_submodule_repository_missing_error(
                    &module.name,
                    super_prefix,
                ));
            }
            let Some(submodule_repo) = exact_repo_at(&worktree) else {
                if !removed
                    && read_tree_submodule_worktree_state(&worktree, &admin_dir)?
                        == ReadTreeSubmoduleWorktreeState::Absent
                    && !admin_dir.exists()
                {
                    continue;
                }
                if read_tree_submodule_worktree_state(&worktree, &admin_dir)?
                    == ReadTreeSubmoduleWorktreeState::Absent
                    && let Some(parent_path) = parent_path
                {
                    return Err(read_tree_nested_submodule_repository_missing_error(
                        parent_path,
                        &read_tree_nested_submodule_missing_path(&worktree, &admin_dir, &path),
                        true,
                        root_super_prefix,
                    ));
                }
                if let Ok(metadata) = fs::symlink_metadata(&worktree) {
                    if metadata.is_dir() {
                        if directory_contains_untracked_nonignored(repo, &worktree, original_index)?
                        {
                            return Err(CliError::Stderr {
                                code: 128,
                                text: format!(
                                    "error: Updating '{}' would lose untracked files in it\n",
                                    read_tree_submodule_display_path(super_prefix, path)
                                ),
                            });
                        }
                    } else {
                        return Err(CliError::Fatal {
                            code: 128,
                            message: format!(
                                "Untracked working tree file '{}' would be overwritten by merge.",
                                read_tree_submodule_display_path(super_prefix, path)
                            ),
                        });
                    }
                }
                continue;
            };
            let submodule_index = read_repo_index(&submodule_repo)?;
            let head_index = read_head_index(&submodule_repo)?;
            if !diff_indexes(&head_index, &submodule_index)?.is_empty() {
                return Err(read_tree_submodule_dirty_index_error(&path, super_prefix));
            }
            let submodule_frame =
                ReadTreeSubmoduleFrameContext::read(&submodule_repo, &submodule_index)?;
            if removed {
                validate_read_tree_submodule_worktree_changes(
                    &submodule_repo,
                    &submodule_index,
                    &submodule_frame,
                    &path,
                    super_prefix,
                )?;
                validate_read_tree_submodule_removal_worktree(
                    &submodule_repo,
                    submodule_index,
                    &path,
                    submodule_frame,
                    super_prefix,
                )?;
                continue;
            }
            for (changed_path, _) in worktree_status(&submodule_repo, &submodule_index)? {
                if read_tree_submodule_change_is_ignorable(
                    &submodule_repo,
                    &submodule_frame,
                    &changed_path,
                )? {
                    continue;
                }
                let error = if parent_path.is_some() {
                    read_tree_nested_submodule_dirty_worktree_error(
                        &path,
                        Some(&changed_path),
                        super_prefix,
                    )
                } else {
                    read_tree_submodule_dirty_worktree_error(
                        &path,
                        Some(&changed_path),
                        super_prefix,
                    )
                };
                return Err(error);
            }
        }
    }
    for target in read_tree_submodule_targets_from_frame(result_index, recurse, frame) {
        let worktree = repo.root.join(&target.module.path);
        let admin_dir = read_tree_submodule_admin_dir_for_name(repo, &target.module.name)?;
        let Some(current) = find_index_entry(original_index, target.module.path.as_bytes()) else {
            validate_read_tree_submodule_target_worktree(
                repo,
                original_index,
                &target.module,
                super_prefix,
            )?;
            continue;
        };
        if current.mode == IndexMode::Gitlink && current.id == target.id {
            continue;
        }
        if parent_path.is_some()
            && read_tree_submodule_worktree_state(&worktree, &admin_dir)?
                == ReadTreeSubmoduleWorktreeState::Stale
            && !admin_dir.exists()
        {
            continue;
        }
        validate_read_tree_submodule_target_worktree(
            repo,
            original_index,
            &target.module,
            super_prefix,
        )?;
    }
    Ok(())
}

fn validate_read_tree_submodule_worktree_changes(
    repo: &GitRepo,
    index: &GitIndex,
    frame: &ReadTreeSubmoduleFrameContext,
    parent_path: &str,
    super_prefix: Option<&str>,
) -> Result<()> {
    for (changed_path, _) in worktree_status(repo, index)? {
        if read_tree_submodule_change_is_ignorable(repo, frame, &changed_path)? {
            continue;
        }
        return Err(read_tree_nested_submodule_dirty_worktree_error(
            parent_path,
            Some(&changed_path),
            super_prefix,
        ));
    }
    Ok(())
}

fn read_tree_submodule_change_is_ignorable(
    repo: &GitRepo,
    frame: &ReadTreeSubmoduleFrameContext,
    changed_path: &[u8],
) -> Result<bool> {
    let Some(module) = frame.module_for_path(changed_path) else {
        return Ok(false);
    };
    if !frame.activation.is_active(module) {
        return Ok(false);
    }
    let worktree = repo.root.join(&module.path);
    let admin_dir = read_tree_submodule_admin_dir(repo, module)?;
    Ok(matches!(
        read_tree_submodule_worktree_state(&worktree, &admin_dir)?,
        ReadTreeSubmoduleWorktreeState::Absent | ReadTreeSubmoduleWorktreeState::AdminOnly
    ))
}

fn validate_read_tree_submodule_removal_worktree(
    repo: &GitRepo,
    index: GitIndex,
    prefix: &str,
    frame: ReadTreeSubmoduleFrameContext,
    super_prefix: Option<&str>,
) -> Result<()> {
    let mut path_arena = ReadTreeSubmoduleRemovalPathArena::new(prefix);
    let root_path_id = ReadTreeSubmoduleRemovalPathId(0);
    let mut pending = Vec::new();
    for entry in index
        .entries()
        .iter()
        .filter(|entry| entry.stage == 0 && entry.mode == IndexMode::Gitlink)
    {
        let Some(module) = frame.module_for_path(&entry.path) else {
            continue;
        };
        if !frame.activation.is_active(module) {
            continue;
        }
        let worktree = repo.root.join(&module.path);
        let Some(submodule_repo) = exact_repo_at(&worktree) else {
            continue;
        };
        pending.push(ReadTreeSubmoduleRemovalChild {
            repo: submodule_repo,
            path_id: path_arena.push(root_path_id, &module.path),
        });
    }
    drop(index);
    drop(frame);

    while let Some(child) = pending.pop() {
        let module_path = path_arena.segment(child.path_id).to_owned();
        let parent_path_id = path_arena.nodes[child.path_id.0]
            .parent
            .expect("removal child has a parent path");
        let child_index = read_repo_index(&child.repo)?;
        let head_index = read_head_index(&child.repo)?;
        if !diff_indexes(&head_index, &child_index)?.is_empty() {
            return Err(read_tree_nested_submodule_dirty_index_error(
                &module_path,
                &path_arena.display(parent_path_id),
                super_prefix,
            ));
        }
        let child_frame = ReadTreeSubmoduleFrameContext::read(&child.repo, &child_index)?;
        for entry in child_index
            .entries()
            .iter()
            .filter(|entry| entry.stage == 0 && entry.mode == IndexMode::Gitlink)
        {
            let Some(module) = child_frame.module_for_path(&entry.path) else {
                continue;
            };
            if !child_frame.activation.is_active(module) {
                continue;
            }
            let worktree = child.repo.root.join(&module.path);
            let Some(submodule_repo) = exact_repo_at(&worktree) else {
                continue;
            };
            pending.push(ReadTreeSubmoduleRemovalChild {
                repo: submodule_repo,
                path_id: path_arena.push(child.path_id, &module.path),
            });
        }
        drop(child_index);
        drop(child_frame);
    }
    Ok(())
}

pub(crate) fn checkout_read_tree_submodules(
    repo: &GitRepo,
    original_index: &GitIndex,
    index: &GitIndex,
    recurse: bool,
    force: bool,
    root_operation: ReadTreeSubmoduleRootOperation,
    root_contexts: &ReadTreeSubmoduleRootContexts,
    super_prefix: Option<&str>,
) -> Result<()> {
    if !recurse {
        return Ok(());
    }
    checkout_read_tree_submodules_inner(
        repo,
        original_index,
        index,
        recurse,
        force,
        true,
        None,
        root_operation,
        root_contexts.result(),
        root_operation.unpopulated_policy(),
        root_operation,
        true,
        super_prefix,
        super_prefix,
    )
}

fn checkout_read_tree_submodules_inner(
    repo: &GitRepo,
    original_index: &GitIndex,
    index: &GitIndex,
    recurse: bool,
    force: bool,
    root_level: bool,
    parent_path: Option<&str>,
    root_operation: ReadTreeSubmoduleRootOperation,
    frame: &ReadTreeSubmoduleFrameContext,
    unpopulated_policy: ReadTreeSubmoduleUnpopulatedPolicy,
    root_operation_origin: ReadTreeSubmoduleRootOperation,
    parent_worktree_was_populated: bool,
    super_prefix: Option<&str>,
    root_super_prefix: Option<&str>,
) -> Result<()> {
    let targets =
        read_tree_submodule_checkout_targets_from_frame(original_index, index, recurse, frame);
    for target in targets {
        let worktree = repo.root.join(&target.module.path);
        let admin_dir = read_tree_submodule_admin_dir_for_name(repo, &target.module.name)?;
        let worktree_state = read_tree_submodule_worktree_state(&worktree, &admin_dir)?;
        if root_level
            && root_operation == ReadTreeSubmoduleRootOperation::OneWayMerge
            && target.transition
            && worktree_state == ReadTreeSubmoduleWorktreeState::Stale
            && admin_dir.exists()
            && !worktree.join(".git").exists()
        {
            remove_empty_read_tree_submodule_worktree(&worktree)?;
            if !path_exists(&worktree) {
                continue;
            }
        }
        if root_level
            && match root_operation {
                ReadTreeSubmoduleRootOperation::Merge => !target.transition,
                ReadTreeSubmoduleRootOperation::OneWayMerge => {
                    !target.transition && admin_dir.exists()
                }
                ReadTreeSubmoduleRootOperation::Reset
                | ReadTreeSubmoduleRootOperation::SingleTree => false,
            }
            && matches!(
                worktree_state,
                ReadTreeSubmoduleWorktreeState::Absent | ReadTreeSubmoduleWorktreeState::AdminOnly
            )
        {
            continue;
        }
        if !root_level
            && root_operation == ReadTreeSubmoduleRootOperation::Merge
            && root_operation_origin == ReadTreeSubmoduleRootOperation::OneWayMerge
            && !target.transition
            && worktree_state == ReadTreeSubmoduleWorktreeState::Absent
            && !admin_dir.exists()
        {
            return apply_missing_read_tree_submodule_transition(
                repo,
                &target.module,
                parent_path.ok_or_else(|| {
                    read_tree_submodule_repository_missing_error(&target.module.name, super_prefix)
                })?,
                root_super_prefix,
            );
        }
        let preserve_unpopulated_nested = parent_worktree_was_populated
            && !root_level
            && unpopulated_policy == ReadTreeSubmoduleUnpopulatedPolicy::Preserve
            && matches!(
                worktree_state,
                ReadTreeSubmoduleWorktreeState::Absent | ReadTreeSubmoduleWorktreeState::AdminOnly
            )
            && (worktree_state == ReadTreeSubmoduleWorktreeState::Absent
                || root_operation_origin != ReadTreeSubmoduleRootOperation::OneWayMerge)
            && !target.transition;
        if preserve_unpopulated_nested {
            continue;
        }
        if !root_level
            && root_operation == ReadTreeSubmoduleRootOperation::Reset
            && !target.transition
            && worktree_state == ReadTreeSubmoduleWorktreeState::Absent
            && !admin_dir.exists()
        {
            let missing_path = relative_path_between(&worktree, &admin_dir)
                .unwrap_or_else(|| admin_dir.clone())
                .display()
                .to_string();
            fs::create_dir_all(&admin_dir)?;
            connect_read_tree_submodule_failure_worktree(&admin_dir, &worktree)?;
            return Err(read_tree_nested_submodule_repository_missing_error(
                parent_path.ok_or_else(|| {
                    read_tree_submodule_repository_missing_error(&target.module.name, super_prefix)
                })?,
                &missing_path,
                true,
                root_super_prefix,
            ));
        }
        if root_level
            && root_operation == ReadTreeSubmoduleRootOperation::Reset
            && worktree_state == ReadTreeSubmoduleWorktreeState::Populated
        {
            preserve_read_tree_submodule_worktree_config(repo, &target.module, &worktree)?;
        }
        if !root_level
            && target.transition
            && worktree_state == ReadTreeSubmoduleWorktreeState::Absent
            && !admin_dir.exists()
        {
            return apply_missing_read_tree_submodule_transition(
                repo,
                &target.module,
                parent_path.ok_or_else(|| {
                    read_tree_submodule_repository_missing_error(&target.module.name, super_prefix)
                })?,
                root_super_prefix,
            );
        }
        if !root_level
            && target.transition
            && worktree_state == ReadTreeSubmoduleWorktreeState::Stale
            && !admin_dir.exists()
        {
            return apply_stale_read_tree_submodule_transition(
                &worktree,
                &admin_dir,
                parent_path.ok_or_else(|| {
                    read_tree_submodule_repository_missing_error(&target.module.name, super_prefix)
                })?,
                &target.module.path,
                root_super_prefix,
            );
        }
        if root_level
            && worktree_state == ReadTreeSubmoduleWorktreeState::Absent
            && !admin_dir.exists()
        {
            return apply_missing_read_tree_submodule_root_transition(
                repo,
                &target.module,
                super_prefix,
            );
        }
        let previous_submodule_index = if let Some(submodule_repo) = exact_repo_at(&worktree) {
            Some(read_repo_index(&submodule_repo)?)
        } else if admin_dir.exists() {
            let admin_repo = GitRepo {
                root: worktree.clone(),
                index_path: admin_dir.join("index"),
                objects_dir: admin_dir.join("objects"),
                git_dir: admin_dir.clone(),
            };
            Some(read_repo_index(&admin_repo)?)
        } else {
            None
        };
        let worktree_was_populated = worktree_state == ReadTreeSubmoduleWorktreeState::Populated;
        let previously_absent_nested_paths = if parent_worktree_was_populated
            && unpopulated_policy == ReadTreeSubmoduleUnpopulatedPolicy::Preserve
        {
            previous_submodule_index
                .as_ref()
                .map(|previous_index| {
                    read_tree_submodule_paths_absent_before_checkout(&worktree, previous_index)
                })
                .transpose()?
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        prepare_read_tree_submodule_worktree(repo, original_index, &target.module)?;
        if !recurse {
            if !path_exists(&worktree) {
                fs::create_dir_all(&worktree)?;
            }
            continue;
        }
        connect_read_tree_submodule_worktree(repo, &target.module, &worktree)?;
        checkout_submodule_gitlink(&worktree, &target.id)?;
        if root_level && root_operation == ReadTreeSubmoduleRootOperation::OneWayMerge {
            preserve_read_tree_submodule_worktree_config(repo, &target.module, &worktree)?;
        }

        let submodule_repo = find_repo_at(&worktree)?;
        let submodule_index = read_repo_index(&submodule_repo)?;
        let nested_original_index = previous_submodule_index
            .as_ref()
            .unwrap_or(&submodule_index);
        let nested_parent_path = target.module.path.clone();
        let nested_super_prefix =
            read_tree_submodule_nested_prefix(super_prefix, &target.module.path);
        let child_frame = ReadTreeSubmoduleFrameContext::read(&submodule_repo, &submodule_index)?;
        let nested_operation = root_operation.nested_operation();
        if let Err(error) = checkout_read_tree_submodules_inner(
            &submodule_repo,
            nested_original_index,
            &submodule_index,
            true,
            force,
            false,
            Some(&nested_parent_path),
            nested_operation,
            &child_frame,
            nested_operation.unpopulated_policy(),
            root_operation_origin,
            worktree_was_populated,
            Some(&nested_super_prefix),
            root_super_prefix,
        ) {
            if let (Some(previous_index), Some(previous_entry)) = (
                previous_submodule_index.as_ref(),
                find_index_entry(original_index, target.module.path.as_bytes()),
            ) {
                restore_read_tree_submodule_transition_after_nested_failure(
                    &submodule_repo,
                    previous_index,
                    &previous_entry.id,
                )?;
            }
            if root_operation == ReadTreeSubmoduleRootOperation::OneWayMerge
                || read_tree_nested_failure_resets_index(&error)
            {
                preserve_read_tree_submodule_worktree_config(repo, &target.module, &worktree)?;
            }
            let preservation_index = previous_submodule_index
                .as_ref()
                .unwrap_or(&submodule_index);
            preserve_read_tree_submodule_failure_worktree_config(
                &submodule_repo,
                preservation_index,
            )?;
            return Err(error);
        }
        if unpopulated_policy == ReadTreeSubmoduleUnpopulatedPolicy::Resolve {
            remove_empty_read_tree_submodule_worktrees(&submodule_repo, &submodule_index)?;
        } else {
            for path in previously_absent_nested_paths {
                remove_empty_read_tree_submodule_worktree(&submodule_repo.root.join(path))?;
            }
        }
    }
    Ok(())
}

fn read_tree_submodule_paths_absent_before_checkout(
    worktree: &Path,
    index: &GitIndex,
) -> Result<Vec<String>> {
    let Some(submodule_repo) = exact_repo_at(worktree) else {
        return Ok(Vec::new());
    };
    let frame = ReadTreeSubmoduleFrameContext::read(&submodule_repo, index)?;
    let mut absent = Vec::new();
    for entry in index
        .entries()
        .iter()
        .filter(|entry| entry.stage == 0 && entry.mode == IndexMode::Gitlink)
    {
        let Some(module) = frame.module_for_path(&entry.path) else {
            continue;
        };
        if !frame.activation.is_active(module) {
            continue;
        }
        let nested_worktree = submodule_repo.root.join(&module.path);
        if !path_exists(&nested_worktree) {
            absent.push(module.path.clone());
        }
    }
    Ok(absent)
}

fn preserve_read_tree_submodule_worktree_config(
    repo: &GitRepo,
    module: &ReadTreeSubmoduleModuleDescriptor,
    worktree: &Path,
) -> Result<()> {
    let config = read_tree_submodule_admin_dir_for_name(repo, &module.name)?.join("config");
    if !config.is_file()
        || !read_config_file(&config)?
            .iter()
            .any(|entry| entry.section == "core" && entry.key == "worktree")
    {
        if !config.is_file() {
            return Ok(());
        }
    }
    let admin_dir = config
        .parent()
        .expect("submodule config has an admin directory");
    let worktree_path =
        relative_path_between(admin_dir, worktree).unwrap_or_else(|| worktree.to_path_buf());
    set_config_value_in_file(
        &config,
        "core.worktree",
        &worktree_path.display().to_string(),
    )?;
    Ok(())
}

fn read_tree_nested_failure_resets_index(error: &CliError) -> bool {
    matches!(
        error,
        CliError::Stderr { text, .. } if text.contains("fatal: could not reset submodule index\n")
    )
}

fn apply_missing_read_tree_submodule_transition(
    repo: &GitRepo,
    module: &ReadTreeSubmoduleModuleDescriptor,
    parent_path: &str,
    super_prefix: Option<&str>,
) -> Result<()> {
    let worktree = repo.root.join(&module.path);
    let admin_dir = read_tree_submodule_admin_dir_for_name(repo, &module.name)?;
    fs::create_dir_all(&admin_dir)?;
    connect_read_tree_submodule_failure_worktree(&admin_dir, &worktree)?;
    let git_dir =
        relative_path_between(&worktree, &admin_dir).unwrap_or_else(|| admin_dir.to_path_buf());
    let missing_path = git_dir.display().to_string();
    Err(read_tree_nested_submodule_repository_missing_error(
        parent_path,
        &missing_path,
        true,
        super_prefix,
    ))
}

fn apply_missing_read_tree_submodule_root_transition(
    repo: &GitRepo,
    module: &ReadTreeSubmoduleModuleDescriptor,
    super_prefix: Option<&str>,
) -> Result<()> {
    let worktree = repo.root.join(&module.path);
    let admin_dir = read_tree_submodule_admin_dir_for_name(repo, &module.name)?;
    fs::create_dir_all(&admin_dir)?;
    connect_read_tree_submodule_failure_worktree(&admin_dir, &worktree)?;
    Err(read_tree_submodule_repository_missing_error(
        &module.name,
        super_prefix,
    ))
}

fn apply_stale_read_tree_submodule_transition(
    worktree: &Path,
    admin_dir: &Path,
    parent_path: &str,
    module_path: &str,
    super_prefix: Option<&str>,
) -> Result<()> {
    let embedded_marker = worktree.join(".git").is_file();
    let missing_path = read_tree_nested_submodule_missing_path(worktree, admin_dir, module_path);
    if !embedded_marker {
        fs::create_dir_all(admin_dir)?;
        connect_read_tree_submodule_failure_worktree(admin_dir, worktree)?;
    }
    Err(read_tree_nested_submodule_repository_missing_error(
        parent_path,
        &missing_path,
        !embedded_marker,
        super_prefix,
    ))
}

fn connect_read_tree_submodule_failure_worktree(admin_dir: &Path, worktree: &Path) -> Result<()> {
    if !path_exists(worktree) {
        fs::create_dir_all(worktree)?;
    }
    let git_dir =
        relative_path_between(worktree, admin_dir).unwrap_or_else(|| admin_dir.to_path_buf());
    fs::write(
        worktree.join(".git"),
        format!("gitdir: {}\n", git_dir.display()),
    )?;
    let worktree_path =
        relative_path_between(admin_dir, worktree).unwrap_or_else(|| worktree.to_path_buf());
    set_config_value_in_file(
        &admin_dir.join("config"),
        "core.worktree",
        &worktree_path.display().to_string(),
    )?;
    Ok(())
}

fn preserve_read_tree_submodule_failure_worktree_config(
    repo: &GitRepo,
    index: &GitIndex,
) -> Result<()> {
    let modules = read_gitmodules_from_index(repo, index)?;
    let path_index = ReadTreeSubmodulePathIndex::new(&modules);
    for entry in index
        .entries()
        .iter()
        .filter(|entry| entry.stage == 0 && entry.mode == IndexMode::Gitlink)
    {
        let Some(module) = path_index
            .module_index(&entry.path)
            .and_then(|index| modules.get(index))
        else {
            continue;
        };
        let worktree = repo.root.join(&module.path);
        let admin_dir = read_tree_submodule_admin_dir(repo, module)?;
        let config = admin_dir.join("config");
        if !worktree.join(".git").is_file() || !config.is_file() {
            continue;
        }
        if read_config_file(&config)?
            .iter()
            .any(|entry| entry.section == "core" && entry.key == "worktree")
        {
            continue;
        }
        let worktree_path =
            relative_path_between(&admin_dir, &worktree).unwrap_or_else(|| worktree.to_path_buf());
        set_config_value_in_file(
            &config,
            "core.worktree",
            &worktree_path.display().to_string(),
        )?;
    }
    Ok(())
}

fn restore_read_tree_submodule_transition_after_nested_failure(
    repo: &GitRepo,
    original_index: &GitIndex,
    original_head: &ObjectId,
) -> Result<()> {
    original_index.write_to_path(&repo.index_path)?;
    let algorithm = repo_hash_algorithm_from_config(repo)?;
    RefStore::new(&repo.git_dir, algorithm).write_head_direct(original_head)?;
    Ok(())
}

fn remove_empty_read_tree_submodule_worktree(worktree: &Path) -> Result<()> {
    if !worktree.is_dir() {
        return Ok(());
    }
    if fs::read_dir(worktree)?.next().is_none() {
        fs::remove_dir(worktree)?;
    }
    Ok(())
}

fn remove_empty_read_tree_submodule_worktrees(repo: &GitRepo, index: &GitIndex) -> Result<()> {
    for entry in index
        .entries()
        .iter()
        .filter(|entry| entry.stage == 0 && entry.mode == IndexMode::Gitlink)
    {
        let path = String::from_utf8_lossy(&entry.path);
        remove_empty_read_tree_submodule_worktree(&repo.root.join(path.as_ref()))?;
    }
    Ok(())
}

pub(crate) fn remove_read_tree_submodules(
    repo: &GitRepo,
    original_index: &GitIndex,
    result_index: &GitIndex,
    recurse: bool,
    root_contexts: Option<&ReadTreeSubmoduleRootContexts>,
    super_prefix: Option<&str>,
) -> Result<()> {
    if !recurse {
        return Ok(());
    }
    let frame = root_contexts
        .expect("read-tree recursive submodule contexts")
        .original()?;
    for entry in original_index
        .entries()
        .iter()
        .filter(|entry| entry.stage == 0 && entry.mode == IndexMode::Gitlink)
    {
        if find_index_entry(result_index, &entry.path)
            .is_some_and(|target| target.mode == IndexMode::Gitlink)
        {
            continue;
        }
        let Some(module) = frame.module_for_path(&entry.path) else {
            continue;
        };
        let worktree = repo.root.join(&module.path);
        if !frame.activation.is_active(module) {
            warn_if_read_tree_submodule_worktree_remains(&module.path, &worktree)?;
            continue;
        }
        let admin_dir = read_tree_submodule_admin_dir_for_name(repo, &module.name)?;
        if exact_repo_at(&worktree).is_none() {
            continue;
        }
        let display_path = read_tree_submodule_display_path(super_prefix, &module.path);
        remove_read_tree_submodule_worktree(repo, module, &worktree, &display_path)?;
        clear_read_tree_submodule_worktrees(&admin_dir)?;
    }
    Ok(())
}

fn clear_read_tree_submodule_worktrees(admin_dir: &Path) -> Result<()> {
    if !admin_dir.is_dir() {
        return Ok(());
    }
    let config = admin_dir.join("config");
    if config.is_file() {
        let _ = unset_config_value_in_file(&config, "core.worktree");
    }
    Ok(())
}

fn prepare_read_tree_submodule_worktree(
    repo: &GitRepo,
    original_index: &GitIndex,
    module: &ReadTreeSubmoduleModuleDescriptor,
) -> Result<()> {
    let worktree = repo.root.join(&module.path);
    let Ok(metadata) = fs::symlink_metadata(&worktree) else {
        return Ok(());
    };
    if metadata.is_dir() {
        if exact_repo_at(&worktree).is_some() {
            return Ok(());
        }
        return Ok(());
    }
    let tracked_non_gitlink = find_index_entry(original_index, module.path.as_bytes())
        .is_some_and(|entry| entry.mode != IndexMode::Gitlink);
    if tracked_non_gitlink {
        fs::remove_file(&worktree)?;
        return Ok(());
    }
    fs::remove_file(&worktree)?;
    Ok(())
}

fn validate_read_tree_submodule_target_worktree(
    repo: &GitRepo,
    original_index: &GitIndex,
    module: &ReadTreeSubmoduleModuleDescriptor,
    super_prefix: Option<&str>,
) -> Result<()> {
    let worktree = repo.root.join(&module.path);
    let Ok(metadata) = fs::symlink_metadata(&worktree) else {
        return Ok(());
    };
    if metadata.is_dir() {
        if exact_repo_at(&worktree).is_none()
            && directory_contains_untracked_nonignored(repo, &worktree, original_index)?
        {
            return Err(CliError::Stderr {
                code: 128,
                text: format!(
                    "error: Updating '{}' would lose untracked files in it\n",
                    read_tree_submodule_display_path(super_prefix, &module.path)
                ),
            });
        }
        return Ok(());
    }
    let tracked_non_gitlink = find_index_entry(original_index, module.path.as_bytes())
        .is_some_and(|entry| entry.mode != IndexMode::Gitlink);
    let ignore = standard_repo_ignore(repo)?;
    let ignored = ignore.is_ignored(module.path.as_bytes(), false)
        || ignore.is_ignored(module.path.as_bytes(), true);
    if tracked_non_gitlink || ignored {
        return Ok(());
    }
    Err(CliError::Fatal {
        code: 128,
        message: format!(
            "Untracked working tree file '{}' would be overwritten by merge.",
            read_tree_submodule_display_path(super_prefix, &module.path)
        ),
    })
}

fn read_tree_submodule_targets(
    repo: &GitRepo,
    index: &GitIndex,
    active_only: bool,
) -> Result<Vec<ReadTreeSubmoduleTarget>> {
    let frame = ReadTreeSubmoduleFrameContext::read(repo, index)?;
    Ok(read_tree_submodule_targets_from_frame(
        index,
        active_only,
        &frame,
    ))
}

fn read_tree_submodule_targets_from_frame(
    index: &GitIndex,
    active_only: bool,
    frame: &ReadTreeSubmoduleFrameContext,
) -> Vec<ReadTreeSubmoduleTarget> {
    let mut targets = Vec::new();
    for entry in index
        .entries()
        .iter()
        .filter(|entry| entry.stage == 0 && entry.mode == IndexMode::Gitlink)
    {
        let Some(module) = frame.module_for_path(&entry.path) else {
            continue;
        };
        if !active_only || frame.activation.is_active(module) {
            targets.push(ReadTreeSubmoduleTarget {
                module: ReadTreeSubmoduleModuleDescriptor::from_module(module),
                id: entry.id.clone(),
            });
        }
    }
    targets
}

fn read_tree_submodule_checkout_targets_from_frame(
    original_index: &GitIndex,
    index: &GitIndex,
    active_only: bool,
    frame: &ReadTreeSubmoduleFrameContext,
) -> Vec<ReadTreeSubmoduleCheckoutTarget> {
    read_tree_submodule_targets_from_frame(index, active_only, frame)
        .into_iter()
        .map(|target| {
            let previous = find_index_entry(original_index, target.module.path.as_bytes());
            let transition = previous.is_none_or(|current| {
                current.mode != IndexMode::Gitlink || current.id != target.id
            });
            ReadTreeSubmoduleCheckoutTarget {
                module: target.module,
                id: target.id,
                transition,
            }
        })
        .collect()
}

#[derive(Clone, Debug, Default)]
struct ReadTreeSubmoduleActivation {
    module_values: BTreeMap<String, bool>,
    active_specs: Option<Vec<Vec<u8>>>,
    configured_urls: BTreeSet<String>,
}

#[derive(Clone, Debug)]
struct ReadTreeSubmoduleValidationTarget {
    module_name: String,
    module_path: String,
    id: ObjectId,
    transition: bool,
    worktree_state: ReadTreeSubmoduleWorktreeState,
    admin_dir_exists: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReadTreeSubmoduleWorktreeState {
    Populated,
    Absent,
    AdminOnly,
    Stale,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReadTreeSubmoduleRootOperation {
    Merge,
    OneWayMerge,
    Reset,
    SingleTree,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReadTreeSubmoduleUnpopulatedPolicy {
    Preserve,
    Resolve,
}

impl ReadTreeSubmoduleRootOperation {
    fn nested_operation(self) -> Self {
        match self {
            Self::OneWayMerge | Self::SingleTree => Self::Merge,
            operation => operation,
        }
    }

    fn unpopulated_policy(self) -> ReadTreeSubmoduleUnpopulatedPolicy {
        match self {
            Self::Merge => ReadTreeSubmoduleUnpopulatedPolicy::Preserve,
            Self::OneWayMerge | Self::Reset | Self::SingleTree => {
                ReadTreeSubmoduleUnpopulatedPolicy::Resolve
            }
        }
    }

    fn preserves_unpopulated_worktrees(self) -> bool {
        matches!(self, Self::Merge)
    }

    fn skips_unpopulated_root_validation(self) -> bool {
        matches!(self, Self::OneWayMerge)
    }
}

struct ReadTreeSubmoduleFrameContext {
    modules: Vec<GitmodulesEntry>,
    activation: ReadTreeSubmoduleActivation,
    path_index: ReadTreeSubmodulePathIndex,
}

pub(crate) struct ReadTreeSubmoduleRootContexts {
    original: Option<ReadTreeSubmoduleFrameContext>,
    result: ReadTreeSubmoduleFrameContext,
}

impl ReadTreeSubmoduleRootContexts {
    pub(crate) fn read(
        repo: &GitRepo,
        original_index: &GitIndex,
        result_index: &GitIndex,
        include_original: bool,
    ) -> Result<Self> {
        Ok(Self {
            original: include_original
                .then(|| ReadTreeSubmoduleFrameContext::read(repo, original_index))
                .transpose()?,
            result: ReadTreeSubmoduleFrameContext::read(repo, result_index)?,
        })
    }

    fn original(&self) -> Result<&ReadTreeSubmoduleFrameContext> {
        self.original.as_ref().ok_or_else(|| {
            CliError::Message("read-tree original submodule context was not prepared".into())
        })
    }

    fn result(&self) -> &ReadTreeSubmoduleFrameContext {
        &self.result
    }
}

impl ReadTreeSubmoduleFrameContext {
    fn read(repo: &GitRepo, index: &GitIndex) -> Result<Self> {
        let modules = read_gitmodules_from_index(repo, index)?;
        Ok(Self {
            path_index: ReadTreeSubmodulePathIndex::new(&modules),
            modules,
            activation: read_tree_submodule_activation(repo)?,
        })
    }

    fn module_for_path(&self, path: &[u8]) -> Option<&GitmodulesEntry> {
        self.path_index
            .module_index(path)
            .and_then(|index| self.modules.get(index))
    }
}

#[derive(Clone, Debug, Default)]
struct ReadTreeSubmodulePathIndex {
    entries: Vec<ReadTreeSubmodulePathIndexEntry>,
}

#[derive(Clone, Debug)]
struct ReadTreeSubmodulePathIndexEntry {
    path: Vec<u8>,
    module_index: usize,
    original_index: usize,
}

impl ReadTreeSubmodulePathIndex {
    fn new(modules: &[GitmodulesEntry]) -> Self {
        let mut entries = modules
            .iter()
            .enumerate()
            .map(|(original_index, module)| ReadTreeSubmodulePathIndexEntry {
                path: module.path.as_bytes().to_vec(),
                module_index: original_index,
                original_index,
            })
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| {
            left.path
                .cmp(&right.path)
                .then(left.original_index.cmp(&right.original_index))
        });
        Self { entries }
    }

    fn module_index(&self, path: &[u8]) -> Option<usize> {
        let mut low = 0;
        let mut high = self.entries.len();
        while low < high {
            let middle = low + (high - low) / 2;
            if self.entries[middle].path.as_slice() < path {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        self.entries
            .get(low)
            .filter(|entry| entry.path.as_slice() == path)
            .map(|entry| entry.module_index)
    }
}

fn read_tree_submodule_worktree_state(
    worktree: &Path,
    admin_dir: &Path,
) -> Result<ReadTreeSubmoduleWorktreeState> {
    if exact_repo_at(worktree).is_some() {
        return Ok(ReadTreeSubmoduleWorktreeState::Populated);
    }
    if admin_dir.exists() {
        return Ok(ReadTreeSubmoduleWorktreeState::AdminOnly);
    }
    if !path_exists(worktree) {
        return Ok(ReadTreeSubmoduleWorktreeState::Absent);
    }
    if worktree.is_dir() && fs::read_dir(worktree)?.next().is_none() {
        return Ok(ReadTreeSubmoduleWorktreeState::Absent);
    }
    Ok(ReadTreeSubmoduleWorktreeState::Stale)
}

impl ReadTreeSubmoduleActivation {
    fn is_active(&self, module: &GitmodulesEntry) -> bool {
        if let Some(value) = self.module_values.get(&module.name) {
            return *value;
        }
        if let Some(specs) = &self.active_specs {
            return pathspec_matches(module.path.as_bytes(), specs);
        }
        self.configured_urls.contains(&module.name)
    }
}

fn read_tree_submodule_activation(repo: &GitRepo) -> Result<ReadTreeSubmoduleActivation> {
    let mut activation = ReadTreeSubmoduleActivation::default();
    let mut active_specs = Vec::new();
    let mut has_active_specs = false;
    for entry in read_config_entries(repo)? {
        if entry.section != "submodule" {
            continue;
        }
        if entry.subsection.is_empty() {
            if entry.key != "active" {
                continue;
            }
            validate_submodule_active_pathspec(&entry.value)?;
            has_active_specs = true;
            if entry.value != "." {
                active_specs.push(entry.value.into_bytes());
            }
            continue;
        }
        match entry.key.as_str() {
            "active" => {
                let value = entry.bool_value().ok_or_else(|| CliError::Fatal {
                    code: 128,
                    message: format!(
                        "bad boolean config value '{}' for 'submodule.{}.active'",
                        entry.value, entry.subsection
                    ),
                })?;
                activation
                    .module_values
                    .insert(entry.subsection.clone(), value);
            }
            "url" => {
                activation.configured_urls.insert(entry.subsection.clone());
            }
            _ => {}
        }
    }
    if has_active_specs {
        activation.active_specs = Some(active_specs);
    }
    Ok(activation)
}

fn validate_submodule_active_pathspec(value: &str) -> Result<()> {
    if value.is_empty() {
        return Err(CliError::Fatal {
            code: 128,
            message: "empty string is not a valid pathspec. please use . instead if you meant to match all paths".into(),
        });
    }
    let Some(magic) = value.strip_prefix(":(") else {
        return Ok(());
    };
    let Some(close) = magic.find(')') else {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("Invalid pathspec magic 'unterminated' in '{value}'"),
        });
    };
    for token in magic[..close].split(',') {
        if matches!(
            token,
            "top" | "literal" | "glob" | "icase" | "exclude" | "!" | "^"
        ) || token
            .strip_prefix("attr:")
            .is_some_and(|name| !name.is_empty())
        {
            continue;
        }
        return Err(CliError::Fatal {
            code: 128,
            message: format!("Invalid pathspec magic '{token}' in '{value}'"),
        });
    }
    Ok(())
}

fn read_tree_submodule_admin_dir_for_name(repo: &GitRepo, module_name: &str) -> Result<PathBuf> {
    Ok(read_common_git_dir(&repo.git_dir)?
        .join("modules")
        .join(module_name))
}

fn read_tree_submodule_admin_dir(repo: &GitRepo, module: &GitmodulesEntry) -> Result<PathBuf> {
    read_tree_submodule_admin_dir_for_name(repo, &module.name)
}

fn connect_read_tree_submodule_worktree(
    repo: &GitRepo,
    module: &ReadTreeSubmoduleModuleDescriptor,
    worktree: &Path,
) -> Result<()> {
    if exact_repo_at(worktree).is_some() {
        return Ok(());
    }
    let admin_dir = read_tree_submodule_admin_dir_for_name(repo, &module.name)?;
    if !admin_dir.join("objects").is_dir() {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("could not find submodule repository for '{}'", module.path),
        });
    }
    if !path_exists(worktree) {
        fs::create_dir_all(worktree)?;
    }

    let git_dir = relative_path_between(worktree, &admin_dir).unwrap_or(admin_dir.clone());
    fs::write(
        worktree.join(".git"),
        format!("gitdir: {}\n", git_dir.display()),
    )?;
    let worktree_path =
        relative_path_between(&admin_dir, worktree).unwrap_or_else(|| worktree.to_path_buf());
    set_config_value_in_file(
        &admin_dir.join("config"),
        "core.worktree",
        &worktree_path.display().to_string(),
    )?;
    Ok(())
}

fn directory_contains_untracked_nonignored(
    repo: &GitRepo,
    root: &Path,
    index: &GitIndex,
) -> Result<bool> {
    let ignore = standard_repo_ignore(repo)?;
    directory_contains_untracked_nonignored_with_ignore(repo, root, index, &ignore)
}

fn directory_contains_untracked_nonignored_with_ignore(
    repo: &GitRepo,
    root: &Path,
    index: &GitIndex,
    ignore: &GitIgnore,
) -> Result<bool> {
    let mut directories = vec![root.to_path_buf()];
    while let Some(directory) = directories.pop() {
        for entry in fs::read_dir(&directory)? {
            let entry = entry?;
            if entry.file_name() == ".git" {
                continue;
            }
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path)?;
            let relative = repo_relative_path(&repo.root, &path)?;
            if metadata.is_dir() {
                if !ignore.is_ignored(&relative, true) {
                    directories.push(path);
                }
                continue;
            }
            if index.entry(&relative, 0).is_some() || ignore.is_ignored(&relative, false) {
                continue;
            }
            return Ok(true);
        }
    }
    Ok(false)
}

fn remove_read_tree_submodule_worktree(
    parent_repo: &GitRepo,
    module: &GitmodulesEntry,
    worktree: &Path,
    display_path: &str,
) -> Result<()> {
    if let Some(submodule_repo) = exact_repo_at(worktree) {
        let index = read_repo_index(&submodule_repo)?;
        remove_read_tree_tracked_entries(&submodule_repo, &index, display_path)?;
    }

    let git_path = worktree.join(".git");
    if git_path.is_dir() {
        let admin_gitdir = parent_repo.git_dir.join("modules").join(&module.name);
        if !admin_gitdir.exists() {
            warn_read_tree_submodule_gitdir_migration(display_path, &git_path, &admin_gitdir);
        }
        absorb_submodule_gitdir(parent_repo, &module.path, &module.name)?;
    }
    if git_path.is_file() {
        fs::remove_file(&git_path)?;
    } else if git_path.is_dir() {
        fs::remove_dir(&git_path)?;
    }
    remove_worktree_path(parent_repo, module.path.as_bytes())?;
    warn_if_read_tree_submodule_worktree_remains(display_path, worktree)?;
    Ok(())
}

fn warn_if_read_tree_submodule_worktree_remains(path: &str, worktree: &Path) -> Result<()> {
    if worktree.is_dir() && fs::read_dir(worktree)?.next().is_some() {
        eprintln!("warning: unable to rmdir '{path}': Directory not empty");
    }
    Ok(())
}

fn remove_read_tree_tracked_entries(
    repo: &GitRepo,
    index: &GitIndex,
    display_prefix: &str,
) -> Result<()> {
    let modules = read_gitmodules_from_index(repo, index)?;
    let path_index = ReadTreeSubmodulePathIndex::new(&modules);
    let activation = read_tree_submodule_activation(repo)?;
    for entry in index
        .entries()
        .iter()
        .rev()
        .filter(|entry| entry.stage == 0)
    {
        if entry.mode == IndexMode::Gitlink
            && let Some(module) = path_index
                .module_index(&entry.path)
                .and_then(|index| modules.get(index))
            && activation.is_active(module)
        {
            let worktree = repo.root.join(&module.path);
            let display_path = if display_prefix.is_empty() {
                module.path.clone()
            } else if worktree.join(".git").is_dir() {
                format!("{display_prefix}/{}", module.path)
            } else {
                module.path.clone()
            };
            let was_populated = exact_repo_at(&worktree).is_some();
            remove_read_tree_submodule_worktree(repo, module, &worktree, &display_path)?;
            if was_populated {
                let admin_dir = read_tree_submodule_admin_dir(repo, module)?;
                clear_read_tree_submodule_worktrees(&admin_dir)?;
            }
        }
        remove_worktree_path(repo, &entry.path)?;
    }
    let empty_index = GitIndex::new_with_algorithm(repo_hash_algorithm_from_config(repo)?);
    empty_index.write_to_path(&repo.index_path)?;
    Ok(())
}

fn update_submodule_checkout(
    path: &std::path::Path,
    id: &ObjectId,
    strategy: SubmoduleUpdateStrategy,
) -> Result<()> {
    match strategy {
        SubmoduleUpdateStrategy::Checkout => checkout_submodule_gitlink(path, id),
        SubmoduleUpdateStrategy::Merge => merge_submodule_gitlink(path, id),
        SubmoduleUpdateStrategy::Rebase => rebase_submodule_gitlink(path, id),
    }
}

fn merge_submodule_gitlink(path: &std::path::Path, id: &ObjectId) -> Result<()> {
    with_submodule_current_dir(path, || {
        merge(MergeOptions {
            abort: false,
            continue_: false,
            quit: false,
            ff: true,
            ff_only: false,
            no_ff: false,
            show_diffstat: true,
            no_commit: false,
            log_limit: None,
            squash: false,
            cleanup: None,
            signoff: false,
            gpg_sign: None,
            no_gpg_sign: false,
            verify_signatures: false,
            quiet: false,
            allow_unrelated_histories: false,
            strategies: Vec::new(),
            strategy_options: Vec::new(),
            message: None,
            into_name: None,
            message_file: None,
            commits: vec![id.to_hex()],
            commit_label: None,
            commit_source: None,
        })
    })
}

fn rebase_submodule_gitlink(path: &std::path::Path, id: &ObjectId) -> Result<()> {
    with_submodule_current_dir(path, || {
        rebase(
            false,
            false,
            None,
            vec![id.to_hex()],
            false,
            false,
            false,
            false,
            false,
            false,
            false,
            false,
            false,
            Vec::new(),
            None,
            false,
            None,
            false,
            false,
            None,
        )
    })
}

fn with_submodule_current_dir<T>(
    path: &std::path::Path,
    run: impl FnOnce() -> Result<T>,
) -> Result<T> {
    let previous = std::env::current_dir()?;
    std::env::set_current_dir(path)?;
    let result = run();
    let restore = std::env::set_current_dir(previous);
    match (result, restore) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(error)) => Err(error.into()),
        (Err(error), Err(_)) => Err(error),
    }
}

fn update_submodule_remote_head(
    repo: &GitRepo,
    module: &GitmodulesEntry,
    path: &std::path::Path,
    parent_repository: &str,
    no_fetch: bool,
) -> Result<ObjectId> {
    let submodule_repo = find_repo_at(path)?;
    let submodule_algorithm = repo_hash_algorithm_from_config(&submodule_repo)?;
    let submodule_refs = RefStore::new(&submodule_repo.git_dir, submodule_algorithm);
    let mut source_refs = None;
    if !no_fetch {
        let remote_url = submodule_remote_url(repo, &submodule_repo, module, parent_repository)?;
        let Some(remote_path) =
            crate::runtime::local_repository_path_from_base(&repo.root, &remote_url)?
        else {
            return Err(CliError::Fatal {
                code: 128,
                message: format!(
                    "submodule update --remote cannot fetch non-local remote '{remote_url}' yet"
                ),
            });
        };
        let source = local_clone_source(&remote_path)?;
        let source_repo = local_clone_source_repo(&source);
        copy_dir_contents(
            &source.common_dir.join("objects"),
            &submodule_repo.objects_dir,
        )?;
        let source_algorithm = repo_hash_algorithm_from_config(&source_repo)?;
        source_refs = Some(RefStore::new(&source.git_dir, source_algorithm));
    }
    let branch = match configured_submodule_remote_branch(repo, module)? {
        Some(branch) => branch,
        None => match source_refs.as_ref() {
            Some(refs) => default_submodule_remote_branch(refs)?,
            None => default_submodule_remote_tracking_branch(&submodule_refs)?,
        },
    };
    if branch.is_empty() {
        return Err(CliError::Fatal {
            code: 128,
            message: format!(
                "Unable to find current remote branch for submodule path '{}'",
                module.path
            ),
        });
    }
    if let Some(source_refs) = source_refs.as_ref() {
        copy_remote_refs(source_refs, &submodule_refs, "origin", Some(&branch), true)?;
    }
    submodule_refs
        .resolve(&format!("refs/remotes/origin/{branch}"))
        .map_err(|_| CliError::Fatal {
            code: 128,
            message: format!(
                "Unable to find refs/remotes/origin/{branch} revision in submodule path '{}'",
                module.path
            ),
        })
}

fn fetch_submodule_target(
    repo: &GitRepo,
    module: &GitmodulesEntry,
    path: &std::path::Path,
    parent_repository: &str,
    target: &ObjectId,
    filter_options: &SubmoduleCloneFilterOptions,
) -> Result<()> {
    let submodule_repo = find_repo_at(path)?;
    let submodule_algorithm = repo_hash_algorithm_from_config(&submodule_repo)?;
    let submodule_store =
        LooseObjectStore::new(submodule_repo.objects_dir.clone(), submodule_algorithm);
    if submodule_store.read_object(target).is_ok() {
        return Ok(());
    }
    let remote_url = submodule_remote_url(repo, &submodule_repo, module, parent_repository)?;
    if let Some(remote_path) =
        crate::runtime::local_repository_path_from_base(&repo.root, &remote_url)?
    {
        fetch_local_submodule_target(
            repo,
            module,
            &submodule_repo,
            &submodule_store,
            &remote_path,
            target,
        )
    } else if is_http_transport_url(&remote_url)
        || is_git_daemon_transport_url(&remote_url)
        || is_ssh_transport_url(&remote_url)
    {
        fetch_network_submodule_target(
            module,
            submodule_repo,
            &submodule_store,
            target,
            filter_options,
        )
    } else {
        Err(CliError::Fatal {
            code: 128,
            message: format!(
                "fetch --recurse-submodules cannot fetch submodule remote '{remote_url}' yet"
            ),
        })
    }
}

fn fetch_local_submodule_target(
    repo: &GitRepo,
    module: &GitmodulesEntry,
    submodule_repo: &GitRepo,
    submodule_store: &LooseObjectStore,
    remote_path: &std::path::Path,
    target: &ObjectId,
) -> Result<()> {
    let source = local_clone_source(remote_path)?;
    let source_repo = local_clone_source_repo(&source);
    let submodule_algorithm = repo_hash_algorithm_from_config(submodule_repo)?;
    copy_dir_contents(
        &source.common_dir.join("objects"),
        &submodule_repo.objects_dir,
    )?;
    let source_algorithm = repo_hash_algorithm_from_config(&source_repo)?;
    let source_refs = RefStore::new(&source.git_dir, source_algorithm);
    let submodule_refs = RefStore::new(&submodule_repo.git_dir, submodule_algorithm);
    let branch = configured_submodule_remote_branch(repo, module)?
        .or_else(|| default_submodule_remote_branch(&source_refs).ok());
    copy_remote_refs(
        &source_refs,
        &submodule_refs,
        "origin",
        branch.as_deref(),
        true,
    )?;
    ensure_submodule_target_fetched(module, submodule_store, target)
}

fn fetch_network_submodule_target(
    module: &GitmodulesEntry,
    submodule_repo: GitRepo,
    submodule_store: &LooseObjectStore,
    target: &ObjectId,
    filter_options: &SubmoduleCloneFilterOptions,
) -> Result<()> {
    let options = submodule_target_fetch_options(&submodule_repo, filter_options)?;
    fetch_submodule_target_with_options(submodule_repo, "origin", target, options)?;
    ensure_submodule_target_fetched(module, submodule_store, target)
}

fn submodule_target_fetch_options(
    submodule_repo: &GitRepo,
    filter_options: &SubmoduleCloneFilterOptions,
) -> Result<SubmoduleTargetFetchOptions> {
    let hash = repo_hash_algorithm_from_config(submodule_repo)?;
    Ok(SubmoduleTargetFetchOptions {
        effective_filter: filter_options.filter.clone(),
        no_filter: filter_options.filter.is_none(),
        refetch: false,
        hash,
        ref_format: Some(submodule_ref_format(submodule_repo)?),
    })
}

fn ensure_submodule_target_fetched(
    module: &GitmodulesEntry,
    submodule_store: &LooseObjectStore,
    target: &ObjectId,
) -> Result<()> {
    submodule_store
        .read_object(target)
        .map(|_| ())
        .map_err(|_| CliError::Fatal {
            code: 128,
            message: format!(
                "Fetched in submodule path '{}', but it did not contain {}",
                module.path,
                target.to_hex()
            ),
        })
}

fn submodule_remote_url(
    repo: &GitRepo,
    submodule_repo: &GitRepo,
    module: &GitmodulesEntry,
    parent_repository: &str,
) -> Result<String> {
    let url = read_config_value(submodule_repo, "remote.origin.url")?
        .or(read_config_value(
            repo,
            &format!("submodule.{}.url", module.name),
        )?)
        .unwrap_or_else(|| module.url.clone());
    Ok(resolve_submodule_clone_url(parent_repository, &url))
}

fn configured_submodule_remote_branch(
    repo: &GitRepo,
    module: &GitmodulesEntry,
) -> Result<Option<String>> {
    let branch = read_config_value(repo, &format!("submodule.{}.branch", module.name))?
        .or_else(|| module.branch.clone());
    match branch.as_deref() {
        Some(".") => {
            let algorithm = repo_hash_algorithm_from_config(repo)?;
            let refs = RefStore::new(&repo.git_dir, algorithm);
            current_branch_ref(&refs)?
                .map(|name| branch_display_name(&name))
                .ok_or_else(|| CliError::Fatal {
                    code: 128,
                    message: format!(
                        "submodule '{}' uses branch '.' but the superproject HEAD is detached",
                        module.path
                    ),
                })
                .map(Some)
        }
        Some(value) => Ok(Some(value.to_owned())),
        None => Ok(None),
    }
}

fn default_submodule_remote_branch(source_refs: &RefStore) -> Result<String> {
    if let Some(head) = current_branch_ref(source_refs)? {
        return Ok(branch_display_name(&head));
    }
    let head_id = source_refs.resolve("HEAD").map_err(CliError::Io)?;
    let mut branch = None;
    source_refs.for_each_resolved_ref("refs/heads/", |ref_name, id| {
        if branch.is_none() && id == &head_id {
            branch = ref_name.strip_prefix("refs/heads/").map(str::to_owned);
        }
        Ok::<(), CliError>(())
    })?;
    branch.ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "remote HEAD does not point at a branch".into(),
    })
}

fn default_submodule_remote_tracking_branch(refs: &RefStore) -> Result<String> {
    match refs.read_ref("refs/remotes/origin/HEAD") {
        Ok(RefTarget::Symbolic(target)) => target
            .strip_prefix("refs/remotes/origin/")
            .map(str::to_owned)
            .ok_or_else(|| CliError::Fatal {
                code: 128,
                message: "origin/HEAD does not point at an origin branch".into(),
            }),
        Ok(RefTarget::Direct(id)) => {
            let mut branch = None;
            refs.for_each_resolved_ref("refs/remotes/origin/", |ref_name, ref_id| {
                if branch.is_none() && ref_name != "refs/remotes/origin/HEAD" && ref_id == &id {
                    branch = ref_name
                        .strip_prefix("refs/remotes/origin/")
                        .map(str::to_owned);
                }
                Ok::<(), CliError>(())
            })?;
            branch.ok_or_else(|| CliError::Fatal {
                code: 128,
                message: "origin/HEAD does not match an origin branch".into(),
            })
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Err(CliError::Fatal {
            code: 128,
            message: "origin/HEAD is not available; run without --no-fetch first".into(),
        }),
        Err(error) => Err(CliError::Io(error)),
    }
}

pub(crate) fn absorb_submodule_gitdir(repo: &GitRepo, path: &str, admin_name: &str) -> Result<()> {
    let worktree = repo.root.join(path);
    let git_path = worktree.join(".git");
    if !git_path.exists() || git_path.is_file() {
        return Ok(());
    }
    let target = repo.git_dir.join("modules").join(admin_name);
    if !target.exists() {
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(&git_path, &target)?;
    } else {
        fs::remove_dir_all(&git_path)?;
    }
    let git_dir = relative_path_between(&worktree, &target).unwrap_or(target.clone());
    fs::write(&git_path, format!("gitdir: {}\n", git_dir.display()))?;
    let worktree_path =
        relative_path_between(&target, &worktree).unwrap_or_else(|| worktree.to_path_buf());
    set_config_value_in_file(
        &target.join("config"),
        "core.worktree",
        &worktree_path.display().to_string(),
    )?;
    Ok(())
}

fn foreach_submodules_for_repo(
    repo: &GitRepo,
    command: &str,
    quiet: bool,
    recursive: bool,
    prefix: &str,
) -> Result<()> {
    let index = read_repo_index(repo)?;
    let modules = selected_gitmodules(repo, &[])?;
    for module in modules {
        let Some(entry) = submodule_gitlink_entry(&index, &module.path) else {
            continue;
        };
        let path = repo.root.join(&module.path);
        if exact_repo_at(&path).is_none() {
            continue;
        }
        let display_path = format!("{prefix}{}", module.path);
        if !quiet {
            println!("Entering '{}'", display_path);
        }
        let mut shell = ProcessCommand::new("sh");
        shell.arg("-c").arg(foreach_shell_command(
            command,
            &module.name,
            &module.path,
            &display_path,
            &entry.id.to_hex(),
            &repo.root.display().to_string(),
        ));
        #[cfg(not(windows))]
        shell
            .env("name", &module.name)
            .env("sm_path", &module.path)
            .env("path", &module.path)
            .env("displaypath", &display_path)
            .env("sha1", entry.id.to_hex())
            .env("toplevel", repo.root.display().to_string());
        let status = shell.current_dir(&path).status()?;
        if !status.success() {
            return Err(CliError::Exit(status.code().unwrap_or(1)));
        }
        if recursive {
            let submodule_repo = find_repo_at(&path)?;
            foreach_submodules_for_repo(
                &submodule_repo,
                command,
                quiet,
                true,
                &format!("{display_path}/"),
            )?;
        }
    }
    Ok(())
}

#[cfg(windows)]
fn foreach_shell_command(
    command: &str,
    name: &str,
    path: &str,
    display_path: &str,
    sha1: &str,
    toplevel: &str,
) -> String {
    format!(
        "name={}; sm_path={}; path={}; displaypath={}; sha1={}; toplevel={}; export name sm_path path displaypath sha1 toplevel; {}",
        shell_quote_single(name),
        shell_quote_single(path),
        shell_quote_single(path),
        shell_quote_single(display_path),
        shell_quote_single(sha1),
        shell_quote_single(toplevel),
        command
    )
}

#[cfg(not(windows))]
fn foreach_shell_command(
    command: &str,
    _name: &str,
    _path: &str,
    _display_path: &str,
    _sha1: &str,
    _toplevel: &str,
) -> String {
    command.to_owned()
}

#[cfg(windows)]
fn shell_quote_single(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub(crate) struct SubmoduleHeadState {
    pub(crate) prefix: char,
    pub(crate) id: ObjectId,
    pub(crate) display: String,
}

pub(crate) fn write_gitmodules_named_entry(
    repo: &GitRepo,
    name: &str,
    url: &str,
    path: &str,
    branch: Option<&str>,
) -> Result<()> {
    let gitmodules = repo.root.join(".gitmodules");
    set_config_value_in_file(&gitmodules, &format!("submodule.{name}.path"), path)?;
    set_config_value_in_file(&gitmodules, &format!("submodule.{name}.url"), url)?;
    if let Some(branch) = branch {
        set_config_value_in_file(&gitmodules, &format!("submodule.{name}.branch"), branch)?;
    }
    Ok(())
}

pub(crate) fn submodule_head_state(
    path: &std::path::Path,
    index_id: &ObjectId,
    cached: bool,
) -> Option<SubmoduleHeadState> {
    let repo = exact_repo_at(path)?;
    let algorithm = repo_hash_algorithm_from_config(&repo).ok()?;
    let refs = RefStore::new(&repo.git_dir, algorithm);
    let head_id = refs.resolve("HEAD").ok()?;
    let prefix = if head_id == *index_id { ' ' } else { '+' };
    let id = if cached {
        index_id.clone()
    } else {
        head_id.clone()
    };
    Some(SubmoduleHeadState {
        prefix,
        display: submodule_head_display(&refs, &id),
        id,
    })
}

fn submodule_head_display(refs: &RefStore, id: &ObjectId) -> String {
    if let Some(branch) = current_branch_ref(refs).ok().flatten()
        && refs
            .resolve(&branch)
            .is_ok_and(|branch_id| branch_id == *id)
    {
        return branch.strip_prefix("refs/").unwrap_or(&branch).to_owned();
    }
    let mut display = None;
    let _ = refs.for_each_resolved_ref("refs/heads/", |branch, branch_id| {
        if display.is_none() && branch_id == id {
            display = Some(branch.strip_prefix("refs/").unwrap_or(branch).to_owned());
        }
        Ok::<(), CliError>(())
    });
    if let Some(display) = display {
        return display;
    }
    let mut display = None;
    let _ = refs.for_each_resolved_ref("refs/remotes/", |remote, remote_id| {
        if display.is_none() && remote_id == id {
            display = Some(remote.strip_prefix("refs/").unwrap_or(remote).to_owned());
        }
        Ok::<(), CliError>(())
    });
    if let Some(display) = display {
        return display;
    }
    short_object_id(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filtered_recursive_child_keeps_combined_filter_and_marker() {
        let options = SubmoduleCloneFilterOptions {
            filter: Some("combine:blob:none+tree:0".to_owned()),
            also_filter_submodules: true,
        };
        let nested = vec![".".to_owned()];

        assert!(options.child_also_filter_submodules(&nested));
        assert_eq!(
            options.child_configs(&nested),
            vec![format!("{CLONE_SUBMODULE_FILTER_CONFIG_KEY}=true")]
        );
    }

    #[test]
    fn filtered_leaf_child_keeps_filter_but_not_recursive_marker() {
        let options = SubmoduleCloneFilterOptions {
            filter: Some("blob:none".to_owned()),
            also_filter_submodules: true,
        };

        assert!(!options.child_also_filter_submodules(&[]));
        assert!(options.child_configs(&[]).is_empty());
    }

    #[test]
    fn absent_filter_never_propagates_or_falls_back() {
        let options = SubmoduleCloneFilterOptions::default();
        let nested = vec![".".to_owned()];

        assert!(!options.child_also_filter_submodules(&nested));
        assert!(options.child_configs(&nested).is_empty());
        assert_eq!(options.filter, None);
    }

    #[test]
    fn recursive_update_filter_is_explicit_and_order_independent() {
        let options = SubmoduleCloneFilterOptions::for_update(
            Some("combine:tree:0+blob:none".to_owned()),
            true,
        );
        let nested = vec![".".to_owned()];

        assert_eq!(options.filter.as_deref(), Some("combine:tree:0+blob:none"));
        assert!(options.child_also_filter_submodules(&nested));
    }

    #[test]
    fn recursion_state_round_trips_bounded_cycle_keys() {
        let state = SubmoduleRecursionState {
            depth: 3,
            total_modules: 7,
            path_bytes: 12,
            input_bytes: 34,
            active_keys: vec![SubmoduleCycleKey {
                source: "/tmp/parent with spaces".to_owned(),
                path: "nested/child".to_owned(),
                gitlink: "0123456789abcdef".to_owned(),
            }],
            ref_format: Some("reftable".to_owned()),
        };
        let encoded = encode_submodule_cycle_keys(&state.active_keys);
        assert_eq!(
            decode_submodule_cycle_keys(&encoded).unwrap(),
            state.active_keys
        );
        assert_eq!(state.child_configs().len(), 6);
    }

    #[test]
    fn recursion_state_rejects_hostile_bounds() {
        let state = SubmoduleRecursionState {
            depth: SUBMODULE_MAX_RECURSION_DEPTH + 1,
            ..SubmoduleRecursionState::default()
        };
        assert!(state.validate_bounds().is_err());
        let state = SubmoduleRecursionState {
            input_bytes: SUBMODULE_MAX_INPUT_BYTES + 1,
            ..SubmoduleRecursionState::default()
        };
        assert!(state.validate_bounds().is_err());
        assert!(decode_submodule_text(&"0".repeat(SUBMODULE_MAX_STATE_VALUE_BYTES + 2)).is_err());
    }

    #[test]
    fn network_target_fetch_options_preserve_hash_filter_and_ref_format() {
        let sha1_temp = tempfile::tempdir().unwrap();
        let sha1_git_dir = sha1_temp.path().join(".git");
        fs::create_dir_all(&sha1_git_dir).unwrap();
        fs::write(
            sha1_git_dir.join("config"),
            "[core]\n\trepositoryformatversion = 0\n",
        )
        .unwrap();
        let sha1_repo = GitRepo {
            root: sha1_temp.path().to_owned(),
            git_dir: sha1_git_dir.clone(),
            objects_dir: sha1_git_dir.join("objects"),
            index_path: sha1_git_dir.join("index"),
        };
        let no_filter =
            submodule_target_fetch_options(&sha1_repo, &SubmoduleCloneFilterOptions::default())
                .unwrap();
        assert_eq!(no_filter.hash, GitHashAlgorithm::Sha1);
        assert_eq!(no_filter.ref_format.as_deref(), Some("files"));
        assert!(no_filter.effective_filter.is_none());
        assert!(no_filter.no_filter);
        assert!(!no_filter.refetch);

        let sha256_temp = tempfile::tempdir().unwrap();
        let sha256_git_dir = sha256_temp.path().join(".git");
        fs::create_dir_all(&sha256_git_dir).unwrap();
        fs::write(
            sha256_git_dir.join("config"),
            "[core]\n\trepositoryformatversion = 1\n[extensions]\n\tobjectFormat = sha256\n\trefStorage = reftable\n",
        )
        .unwrap();
        let sha256_repo = GitRepo {
            root: sha256_temp.path().to_owned(),
            git_dir: sha256_git_dir.clone(),
            objects_dir: sha256_git_dir.join("objects"),
            index_path: sha256_git_dir.join("index"),
        };
        let filtered = submodule_target_fetch_options(
            &sha256_repo,
            &SubmoduleCloneFilterOptions {
                filter: Some("combine:tree:0+blob:none".to_owned()),
                also_filter_submodules: true,
            },
        )
        .unwrap();
        assert_eq!(filtered.hash, GitHashAlgorithm::Sha256);
        assert_eq!(filtered.ref_format.as_deref(), Some("reftable"));
        assert_eq!(
            filtered.effective_filter.as_deref(),
            Some("combine:tree:0+blob:none")
        );
        assert!(!filtered.no_filter);
    }

    #[test]
    fn submodule_path_validation_rejects_traversal_and_git_aliases() {
        let root =
            std::env::temp_dir().join(format!("zmin-submodule-path-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let repo = GitRepo {
            root: root.clone(),
            git_dir: root.join(".git"),
            objects_dir: root.join(".git/objects"),
            index_path: root.join(".git/index"),
        };
        for path in [
            "",
            ".",
            "..",
            "a/../b",
            ".git/config",
            "GIT~1/config",
            "a\\b",
        ] {
            assert!(validate_submodule_path(&repo, path).is_err(), "{path:?}");
        }
        assert!(validate_submodule_path(&repo, "safe/nested").is_ok());
        let outside = root.with_file_name(format!(
            "zmin-submodule-path-outside-{}",
            std::process::id()
        ));
        fs::create_dir_all(&outside).unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&outside, root.join("escape")).unwrap();
            assert!(validate_submodule_path(&repo, "escape/child").is_err());
        }
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
    }
}
