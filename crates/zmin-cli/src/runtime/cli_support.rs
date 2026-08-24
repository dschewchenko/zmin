use std::collections::BTreeSet;
#[cfg(not(unix))]
use std::sync::atomic::{AtomicBool, Ordering};

use clap::Parser;

use super::*;

include!(concat!(env!("OUT_DIR"), "/known_commands.rs"));

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LogPatchMode {
    Default,
    Patch,
    NoPatch,
}

impl LogPatchMode {
    pub(crate) fn from_flags(patch: bool, no_patch: bool) -> Self {
        match (patch, no_patch) {
            (true, false) => Self::Patch,
            (false, true) => Self::NoPatch,
            _ => Self::Default,
        }
    }

    pub(crate) fn from_raw_args(raw_args: &[String], patch: bool, no_patch: bool) -> Self {
        let mut mode = Self::from_flags(patch, no_patch);
        for argument in raw_args {
            if argument == "--" {
                break;
            }
            if argument == "--patch" {
                mode = Self::Patch;
            } else if argument == "--no-patch" {
                mode = Self::NoPatch;
            } else {
                mode.apply_short_patch_cluster(argument);
            }
        }
        mode
    }

    fn apply_short_patch_cluster(&mut self, argument: &str) -> bool {
        let bytes = argument.as_bytes();
        if bytes.len() < 2 || bytes[0] != b'-' || bytes[1] == b'-' {
            return false;
        }
        let mut candidate = *self;
        for &option in &bytes[1..] {
            match option {
                b'p' => candidate = Self::Patch,
                b's' => candidate = Self::NoPatch,
                _ => return false,
            }
        }
        *self = candidate;
        true
    }
}

pub(crate) const GIT_COMPAT_VERSION: &str = "2.47.1.zmin";
const ROOT_HELP_TEXT: &str = include_str!("../cli/help_fixtures/root_help.txt");
const BUILTINS_TEXT: &str = include_str!("../cli/help_fixtures/builtins.txt");
const EMPTY_FOR_EACH_REF_FORMAT: &str =
    "%(if:equals=__ZMIN_EMPTY_FOR_EACH_REF_FORMAT__)%(then)%(else)%(end)";
const ROOT_USAGE_TEXT: &str = concat!(
    "usage: git [-v | --version] [-h | --help] [-C <path>] [-c <name>=<value>]\n",
    "           [--exec-path[=<path>]] [--html-path] [--man-path] [--info-path]\n",
    "           [-p | --paginate | -P | --no-pager] [--no-replace-objects] [--no-lazy-fetch]\n",
    "           [--no-optional-locks] [--no-advice] [--bare] [--git-dir=<path>]\n",
    "           [--work-tree=<path>] [--namespace=<name>] [--config-env=<name>=<envvar>]\n",
    "           <command> [<args>]\n",
);
const REFS_LIST_USAGE: &str = concat!(
    "usage: git refs list [--count=<count>] [--shell|--perl|--python|--tcl]\n",
    "                                [(--sort=<key>)...] [--format=<format>]\n",
    "                                [--include-root-refs] [--points-at=<object>]\n",
    "                                [--merged[=<object>]] [--no-merged[=<object>]]\n",
    "                                [--contains[=<object>]] [--no-contains[=<object>]]\n",
    "                                [(--exclude=<pattern>)...] [--start-after=<marker>]\n",
    "                                [ --stdin | (<pattern>...)]\n",
    "\n",
    "    -s, --[no-]shell      quote placeholders suitably for shells\n",
    "    -p, --[no-]perl       quote placeholders suitably for perl\n",
    "    --[no-]python         quote placeholders suitably for python\n",
    "    --[no-]tcl            quote placeholders suitably for Tcl\n",
    "    --[no-]omit-empty     do not output a newline after empty formatted refs\n",
    "\n",
    "    --[no-]count <n>      show only <n> matched refs\n",
    "    --[no-]format <format>\n",
    "                          format to use for the output\n",
    "    --[no-]start-after <marker>\n",
    "                          start iteration after the provided marker\n",
    "    --[no-]color[=<when>] respect format colors\n",
    "    --[no-]exclude <pattern>\n",
    "                          exclude refs which match pattern\n",
    "    --[no-]sort <key>     field name to sort on\n",
    "    --[no-]points-at <object>\n",
    "                          print only refs which points at the given object\n",
    "    --merged <commit>     print only refs that are merged\n",
    "    --no-merged <commit>  print only refs that are not merged\n",
    "    --contains <commit>   print only refs which contain the commit\n",
    "    --no-contains <commit>\n",
    "                          print only refs which don't contain the commit\n",
    "    --[no-]ignore-case    sorting and filtering are case insensitive\n",
    "    --[no-]stdin          read reference patterns from stdin\n",
    "    --[no-]include-root-refs\n",
    "                          also include HEAD ref and pseudorefs\n",
    "\n",
);
const PENDING_FORMAT_PATCH_RELATIVE_ENV: &str = "ZMIN_PENDING_FORMAT_PATCH_RELATIVE";
const PENDING_FORMAT_PATCH_ATTACH_ENV: &str = "ZMIN_PENDING_FORMAT_PATCH_ATTACH";
const PENDING_FORMAT_PATCH_INLINE_ENV: &str = "ZMIN_PENDING_FORMAT_PATCH_INLINE";
const PENDING_FORMAT_PATCH_MBOXRD_ENV: &str = "ZMIN_PENDING_FORMAT_PATCH_MBOXRD";
const PENDING_UPDATE_INDEX_FORCE_REMOVE_PATHS_ENV: &str =
    "ZMIN_PENDING_UPDATE_INDEX_FORCE_REMOVE_PATHS";

#[cfg(not(unix))]
static BROKEN_PIPE_PANIC: AtomicBool = AtomicBool::new(false);

#[cfg(unix)]
pub(crate) fn restore_default_sigpipe() {
    // Git restores the default SIGPIPE disposition so large stdout writers die
    // with shell-conventional signal exits instead of surfacing EPIPE as a
    // regular application error.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

pub(crate) fn command_definition() -> clap::Command {
    top_level_command_definition()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HistoryArgValueArity {
    None,
    Required,
    Optional,
}

#[derive(Clone, Debug)]
struct HistoryArgSpec {
    long_names: Vec<String>,
    short_names: Vec<char>,
    value_arity: HistoryArgValueArity,
    require_equals: bool,
}

impl HistoryArgSpec {
    fn matches_long(&self, name: &str) -> bool {
        self.long_names.iter().any(|candidate| candidate == name)
    }

    fn matches_short(&self, name: char) -> bool {
        self.short_names.contains(&name)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HistoryOptionName<'a> {
    Long(&'a str),
    Short(char),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HistoryArgToken<'a> {
    Option {
        name: HistoryOptionName<'a>,
        value: Option<&'a str>,
        position: usize,
    },
    Revision {
        value: &'a str,
        position: usize,
    },
    EndOfOptions {
        position: usize,
    },
}

#[derive(Clone, Copy, Debug)]
struct HistoryShortCluster {
    argument_index: usize,
    byte_offset: usize,
}

pub(crate) struct HistoryArgCursor<'a> {
    args: &'a [String],
    index: usize,
    specs: Vec<HistoryArgSpec>,
    short_cluster: Option<HistoryShortCluster>,
    after_end_of_options: bool,
}

impl<'a> HistoryArgCursor<'a> {
    pub(crate) fn new(args: &'a [String]) -> Self {
        let specs = {
            let definition = command_definition();
            args.first()
                .and_then(|command| definition.find_subcommand(command))
                .map(|command| {
                    command
                        .get_arguments()
                        .filter(|argument| !argument.is_positional())
                        .map(|argument| {
                            let mut long_names = argument
                                .get_long()
                                .into_iter()
                                .map(str::to_owned)
                                .collect::<Vec<_>>();
                            long_names.extend(
                                argument
                                    .get_all_aliases()
                                    .into_iter()
                                    .flatten()
                                    .map(str::to_owned),
                            );
                            let mut short_names =
                                argument.get_short_and_visible_aliases().unwrap_or_default();
                            short_names
                                .extend(argument.get_all_short_aliases().unwrap_or_default());
                            short_names.sort_unstable();
                            short_names.dedup();
                            let value_arity = match argument.get_action() {
                                clap::ArgAction::Set | clap::ArgAction::Append => {
                                    let values = argument.get_num_args();
                                    if values.is_some_and(|range| range.min_values() == 0) {
                                        HistoryArgValueArity::Optional
                                    } else {
                                        HistoryArgValueArity::Required
                                    }
                                }
                                _ => HistoryArgValueArity::None,
                            };
                            HistoryArgSpec {
                                long_names,
                                short_names,
                                value_arity,
                                require_equals: argument.is_require_equals_set(),
                            }
                        })
                        .collect()
                })
                .unwrap_or_default()
        };
        Self {
            args,
            index: 1,
            specs,
            short_cluster: None,
            after_end_of_options: false,
        }
    }

    fn long_spec(&self, name: &str) -> Option<&HistoryArgSpec> {
        self.specs.iter().find(|spec| spec.matches_long(name))
    }

    fn short_spec(&self, name: char) -> Option<&HistoryArgSpec> {
        self.specs.iter().find(|spec| spec.matches_short(name))
    }

    fn option_with_value(&self, name: HistoryOptionName<'_>, value: &str) -> String {
        match name {
            HistoryOptionName::Long(name) => format!("--{name}={value}"),
            HistoryOptionName::Short(name) => self
                .short_spec(name)
                .and_then(|spec| spec.long_names.first())
                .map(|long_name| format!("--{long_name}={value}"))
                .unwrap_or_else(|| format!("-{name}{value}")),
        }
    }

    fn next_short_token(&mut self) -> Option<HistoryArgToken<'a>> {
        let cluster = self.short_cluster?;
        let argument = self.args.get(cluster.argument_index)?;
        let (relative_offset, short) = argument[cluster.byte_offset..].char_indices().next()?;
        let short_end = cluster.byte_offset + relative_offset + short.len_utf8();
        let spec = self.short_spec(short).cloned();
        let position = cluster.argument_index;
        let inline_value = spec
            .as_ref()
            .filter(|spec| spec.value_arity != HistoryArgValueArity::None)
            .and_then(|_| (short_end < argument.len()).then_some(short_end));
        if let Some(value_start) = inline_value {
            self.short_cluster = None;
            self.index = position + 1;
            return Some(HistoryArgToken::Option {
                name: HistoryOptionName::Short(short),
                value: Some(&argument[value_start..]),
                position,
            });
        }

        let Some(spec) = spec else {
            self.short_cluster = (short_end < argument.len()).then_some(HistoryShortCluster {
                argument_index: position,
                byte_offset: short_end,
            });
            if self.short_cluster.is_none() {
                self.index = position + 1;
            }
            return Some(HistoryArgToken::Option {
                name: HistoryOptionName::Short(short),
                value: None,
                position,
            });
        };
        if spec.value_arity == HistoryArgValueArity::Required {
            let value_index = position + 1;
            let value = self.args.get(value_index).map(String::as_str);
            self.short_cluster = None;
            self.index = (value.is_some())
                .then_some(value_index + 1)
                .unwrap_or(value_index);
            return Some(HistoryArgToken::Option {
                name: HistoryOptionName::Short(short),
                value,
                position,
            });
        }
        if spec.value_arity == HistoryArgValueArity::Optional {
            let value_index = position + 1;
            let value = self
                .args
                .get(value_index)
                .filter(|value| !spec.require_equals && !value.starts_with('-'))
                .map(String::as_str);
            self.short_cluster = None;
            self.index = (value.is_some())
                .then_some(value_index + 1)
                .unwrap_or(value_index);
            return Some(HistoryArgToken::Option {
                name: HistoryOptionName::Short(short),
                value,
                position,
            });
        }
        self.short_cluster = (short_end < argument.len()).then_some(HistoryShortCluster {
            argument_index: position,
            byte_offset: short_end,
        });
        if self.short_cluster.is_none() {
            self.index = position + 1;
        }
        Some(HistoryArgToken::Option {
            name: HistoryOptionName::Short(short),
            value: None,
            position,
        })
    }

    pub(crate) fn next(&mut self) -> Option<HistoryArgToken<'a>> {
        if self.short_cluster.is_some() {
            return self.next_short_token();
        }
        let position = self.index;
        let argument = self.args.get(position)?;
        self.index += 1;
        if self.after_end_of_options {
            return Some(HistoryArgToken::Revision {
                value: argument,
                position,
            });
        }
        if argument == "--" {
            self.after_end_of_options = true;
            return Some(HistoryArgToken::EndOfOptions { position });
        }
        if argument == "-" || !argument.starts_with('-') {
            return Some(HistoryArgToken::Revision {
                value: argument,
                position,
            });
        }
        if let Some(long_argument) = argument.strip_prefix("--") {
            let (name, inline_value) = long_argument
                .split_once('=')
                .map(|(name, value)| (name, Some(value)))
                .unwrap_or((long_argument, None));
            let Some(spec) = self.long_spec(name).cloned() else {
                return Some(HistoryArgToken::Option {
                    name: HistoryOptionName::Long(name),
                    value: inline_value,
                    position,
                });
            };
            if inline_value.is_some() || spec.value_arity == HistoryArgValueArity::None {
                return Some(HistoryArgToken::Option {
                    name: HistoryOptionName::Long(name),
                    value: inline_value,
                    position,
                });
            }
            if spec.require_equals && spec.value_arity == HistoryArgValueArity::Optional {
                return Some(HistoryArgToken::Option {
                    name: HistoryOptionName::Long(name),
                    value: None,
                    position,
                });
            }
            let value = self
                .args
                .get(self.index)
                .filter(|value| {
                    spec.value_arity == HistoryArgValueArity::Required || !value.starts_with('-')
                })
                .map(String::as_str);
            if value.is_some() {
                self.index += 1;
            }
            return Some(HistoryArgToken::Option {
                name: HistoryOptionName::Long(name),
                value,
                position,
            });
        }
        if argument.len() > 1 {
            self.short_cluster = Some(HistoryShortCluster {
                argument_index: position,
                byte_offset: 1,
            });
            return self.next_short_token();
        }
        Some(HistoryArgToken::Option {
            name: HistoryOptionName::Short('-'),
            value: None,
            position,
        })
    }
}

pub(crate) fn git_compatible_version_line() -> String {
    format!(
        "git version {} (zmin {})",
        GIT_COMPAT_VERSION,
        env!("CARGO_PKG_VERSION")
    )
}

fn normalize_history_required_option_values(mut command_args: Vec<String>) -> Vec<String> {
    let mut cursor = HistoryArgCursor::new(&command_args);
    let mut replacements = Vec::new();
    let mut removed_positions = BTreeSet::new();
    while let Some(token) = cursor.next() {
        let HistoryArgToken::Option {
            name,
            value: Some(value),
            position,
        } = token
        else {
            continue;
        };
        let Some(next) = command_args.get(position + 1) else {
            continue;
        };
        if next != value || !value.starts_with('-') || command_args[position].contains('=') {
            continue;
        }
        replacements.push((position, cursor.option_with_value(name, value)));
        removed_positions.insert(position + 1);
    }
    if replacements.is_empty() {
        return command_args;
    }
    drop(cursor);
    for (position, replacement) in replacements {
        command_args[position] = replacement;
    }
    command_args
        .into_iter()
        .enumerate()
        .filter_map(|(position, argument)| {
            (!removed_positions.contains(&position)).then_some(argument)
        })
        .collect()
}

pub(crate) fn write_git_compatible_version(
    mut writer: impl std::io::Write,
    build_options: bool,
) -> io::Result<()> {
    writeln!(writer, "{}", git_compatible_version_line())?;
    if build_options {
        writeln!(writer, "cpu: {}", std::env::consts::ARCH)?;
        writeln!(writer, "no commit associated with this build")?;
        writeln!(
            writer,
            "sizeof-long: {}",
            std::mem::size_of::<std::os::raw::c_long>()
        )?;
        writeln!(writer, "sizeof-size_t: {}", std::mem::size_of::<usize>())?;
        writeln!(writer, "shell-path: {}", git_shell_path())?;
        writeln!(writer, "default-hash: sha1")?;
        writeln!(writer, "default-ref-format: files")?;
        writeln!(writer, "zmin-version: {}", env!("CARGO_PKG_VERSION"))?;
        writeln!(writer, "zlib: miniz_oxide")?;
        writeln!(writer, "SHA-1: zmin-git-core")?;
        writeln!(writer, "SHA-256: zmin-git-core")?;
    }
    Ok(())
}

#[cfg(not(unix))]
pub(crate) fn install_broken_pipe_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if panic_info_is_broken_pipe(info) {
            BROKEN_PIPE_PANIC.store(true, Ordering::Relaxed);
            return;
        }
        default_hook(info);
    }));
}

#[cfg(not(unix))]
fn panic_info_is_broken_pipe(info: &std::panic::PanicHookInfo<'_>) -> bool {
    panic_payload_is_broken_pipe(info.payload()) || broken_pipe_message(&info.to_string())
}

#[cfg(not(unix))]
pub(crate) fn panic_payload_is_broken_pipe(payload: &(dyn std::any::Any + Send)) -> bool {
    let message = payload
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| payload.downcast_ref::<&str>().copied());
    message.is_some_and(broken_pipe_message)
}

#[cfg(not(unix))]
fn broken_pipe_message(message: &str) -> bool {
    (message.contains("failed printing to stdout")
        || message.contains("failed printing to stderr")
        || message.contains("Broken pipe"))
        && message.contains("Broken pipe")
}

fn clear_pending_format_patch_relative_arg() {
    // SAFETY: CLI startup is single-threaded before any worker threads are spawned.
    unsafe {
        std::env::remove_var(PENDING_FORMAT_PATCH_RELATIVE_ENV);
        std::env::remove_var(PENDING_FORMAT_PATCH_ATTACH_ENV);
        std::env::remove_var(PENDING_FORMAT_PATCH_INLINE_ENV);
        std::env::remove_var(PENDING_FORMAT_PATCH_MBOXRD_ENV);
        std::env::remove_var(PENDING_UPDATE_INDEX_FORCE_REMOVE_PATHS_ENV);
    }
}

fn set_pending_format_patch_relative_arg(value: &str) {
    // SAFETY: CLI startup is single-threaded before any worker threads are spawned.
    unsafe {
        std::env::set_var(PENDING_FORMAT_PATCH_RELATIVE_ENV, value);
    }
}

fn set_pending_format_patch_attach_arg(value: &str) {
    // SAFETY: CLI startup is single-threaded before any worker threads are spawned.
    unsafe {
        std::env::set_var(PENDING_FORMAT_PATCH_ATTACH_ENV, value);
    }
}

fn set_pending_format_patch_inline_arg(value: &str) {
    // SAFETY: CLI startup is single-threaded before any worker threads are spawned.
    unsafe {
        std::env::set_var(PENDING_FORMAT_PATCH_INLINE_ENV, value);
    }
}

fn set_pending_format_patch_mboxrd_arg(enabled: bool) {
    // SAFETY: CLI startup is single-threaded before any worker threads are spawned.
    unsafe {
        if enabled {
            std::env::set_var(PENDING_FORMAT_PATCH_MBOXRD_ENV, "1");
        } else {
            std::env::remove_var(PENDING_FORMAT_PATCH_MBOXRD_ENV);
        }
    }
}

fn normalize_format_patch_relative_value(args: Vec<String>) -> Vec<String> {
    if args.first().map(String::as_str) != Some("format-patch") {
        return args;
    }
    let mut normalized = Vec::with_capacity(args.len());
    let mut index = 0usize;
    while index < args.len() {
        let arg = &args[index];
        if let Some(value) = arg.strip_prefix("--relative=") {
            set_pending_format_patch_relative_arg(value);
            normalized.push(String::from("--relative"));
        } else if let Some(value) = arg.strip_prefix("--attach=") {
            set_pending_format_patch_attach_arg(value);
            normalized.push(String::from("--attach"));
        } else if let Some(value) = arg.strip_prefix("--inline=") {
            set_pending_format_patch_inline_arg(value);
            normalized.push(String::from("--inline"));
        } else if arg == "--pretty" && args.get(index + 1).map(String::as_str) == Some("mboxrd") {
            set_pending_format_patch_mboxrd_arg(true);
            index += 1;
        } else if arg == "--pretty=mboxrd" {
            set_pending_format_patch_mboxrd_arg(true);
        } else {
            normalized.push(arg.clone());
        }
        index += 1;
    }
    normalized
}

#[cfg(not(unix))]
pub(crate) fn broken_pipe_panic_triggered() -> bool {
    BROKEN_PIPE_PANIC.load(Ordering::Relaxed)
}

pub(crate) const EMPTY_TEMPLATE_SENTINEL: &str = "__ZMIN_EMPTY_TEMPLATE__";

pub(crate) fn pending_format_patch_relative_arg() -> Option<String> {
    std::env::var(PENDING_FORMAT_PATCH_RELATIVE_ENV).ok()
}

pub(crate) fn pending_format_patch_attach_arg() -> Option<String> {
    std::env::var(PENDING_FORMAT_PATCH_ATTACH_ENV).ok()
}

pub(crate) fn pending_format_patch_inline_arg() -> Option<String> {
    std::env::var(PENDING_FORMAT_PATCH_INLINE_ENV).ok()
}

pub(crate) fn pending_format_patch_mboxrd_arg() -> bool {
    std::env::var_os(PENDING_FORMAT_PATCH_MBOXRD_ENV).is_some()
}

fn set_pending_update_index_force_remove_paths(paths: &[String]) {
    // SAFETY: CLI startup is single-threaded before any worker threads are spawned.
    unsafe {
        if paths.is_empty() {
            std::env::remove_var(PENDING_UPDATE_INDEX_FORCE_REMOVE_PATHS_ENV);
        } else {
            std::env::set_var(
                PENDING_UPDATE_INDEX_FORCE_REMOVE_PATHS_ENV,
                paths.join("\n"),
            );
        }
    }
}

pub(crate) fn pending_update_index_force_remove_paths() -> Vec<String> {
    std::env::var(PENDING_UPDATE_INDEX_FORCE_REMOVE_PATHS_ENV)
        .ok()
        .map(|raw| raw.lines().map(str::to_owned).collect())
        .unwrap_or_default()
}

fn normalize_update_index_ordered_force_remove_paths(args: Vec<String>) -> Vec<String> {
    if args.first().map(String::as_str) != Some("update-index") {
        return args;
    }
    let mut force_remove_active = false;
    let mut stop_parsing = false;
    let mut force_remove_paths = Vec::new();
    for arg in args.iter().skip(1) {
        if stop_parsing {
            if force_remove_active {
                force_remove_paths.push(arg.clone());
            }
            continue;
        }
        if arg == "--" {
            stop_parsing = true;
            continue;
        }
        if arg == "--force-remove" {
            force_remove_active = true;
            continue;
        }
        if arg.starts_with('-') && arg != "-" {
            continue;
        }
        if force_remove_active {
            force_remove_paths.push(arg.clone());
        }
    }
    set_pending_update_index_force_remove_paths(&force_remove_paths);
    args
}

pub(crate) fn refs_list_help_error(raw_args: &[String]) -> Option<CliError> {
    if raw_args.first().map(String::as_str) != Some("refs")
        || raw_args.get(1).map(String::as_str) != Some("list")
    {
        return None;
    }
    if let Err(error) = validate_refs_list_invocation_before_clap(raw_args) {
        return Some(error);
    }
    if !refs_list_help_requested(raw_args) {
        return None;
    }
    use std::io::Write;

    let mut stdout = io::stdout().lock();
    if let Err(error) = stdout.write_all(REFS_LIST_USAGE.as_bytes()) {
        return Some(CliError::Io(error));
    }
    Some(CliError::Exit(129))
}

fn refs_list_help_requested(raw_args: &[String]) -> bool {
    refs_list_help_requested_from(raw_args, 2)
}

fn refs_list_help_requested_from(raw_args: &[String], start_index: usize) -> bool {
    let mut index = start_index;
    while index < raw_args.len() {
        let argument = &raw_args[index];
        if argument == "--" {
            return false;
        }
        if matches!(argument.as_str(), "-h" | "--help") {
            return true;
        }
        if refs_list_option_requires_value(argument) {
            index = index.saturating_add(2);
        } else {
            index = index.saturating_add(1);
        }
    }
    false
}

fn refs_list_option_requires_value(argument: &str) -> bool {
    matches!(
        argument,
        "--count" | "--format" | "--sort" | "--exclude" | "--points-at" | "--start-after"
    )
}

fn refs_list_unsupported_config_error(raw_args: &[String]) -> Option<CliError> {
    let refs_list_index = raw_args
        .windows(2)
        .position(|pair| pair[0] == "refs" && pair[1] == "list")?;
    let option = raw_args[..refs_list_index].iter().find_map(|argument| {
        (argument == "--config" || argument.starts_with("--config=")).then_some(argument.as_str())
    })?;
    Some(CliError::Stderr {
        code: 129,
        text: format!("unknown option: {option}\n{ROOT_USAGE_TEXT}"),
    })
}

pub(crate) fn parse_cli_invocation(
    program: String,
    raw_args: &[String],
) -> Result<(Args, Vec<String>)> {
    crate::runtime::clear_pending_trace2_state();
    clear_pending_format_patch_relative_arg();
    if raw_args.is_empty() {
        let _ = program;
        write_root_help(io::stdout().lock()).map_err(CliError::Io)?;
        return Err(CliError::Exit(1));
    }
    if matches!(raw_args, [arg] if arg == "-h" || arg == "--help") {
        let _ = program;
        write_root_help(io::stdout().lock()).map_err(CliError::Io)?;
        return Err(CliError::Exit(0));
    }
    if let Some(error) = refs_list_unsupported_config_error(raw_args) {
        return Err(error);
    }
    if try_handle_root_list_cmds_invocation(raw_args)? {
        return Err(CliError::Exit(0));
    }

    let (command_args, global_configs, global_repo_options, pathspec_options) =
        apply_leading_global_options(raw_args)?;
    set_global_config_entries(global_configs);
    propagate_reftable_lock_timeout_override();
    propagate_reftable_write_options_overrides();
    set_global_repo_options(global_repo_options);
    set_global_pathspec_options(pathspec_options);
    if command_args.is_empty() {
        write_root_help(io::stdout().lock()).map_err(CliError::Io)?;
        return Err(CliError::Exit(1));
    }
    if let Some(build_options) = root_version_invocation(&command_args) {
        write_git_compatible_version(io::stdout().lock(), build_options).map_err(CliError::Io)?;
        return Err(CliError::Exit(0));
    }
    let command_args = apply_command_alias(command_args)?;
    let command_args = apply_alias_leading_config_options(command_args)?;
    if let Some(error) = refs_list_help_error(&command_args) {
        return Err(error);
    }
    let original_command_args = command_args.clone();
    let refs_list_invocation = is_refs_list_invocation(&command_args);
    validate_refs_list_invocation_before_clap(&command_args)?;
    let command_args = normalize_refs_list_invocation(command_args);
    let command_args = if refs_list_invocation {
        normalize_for_each_ref_sort_options(command_args)
    } else {
        command_args
    };
    propagate_reftable_lock_timeout_override();
    propagate_reftable_write_options_overrides();
    if let Some(code) = maybe_exec_diff_output_redirection(raw_args, &command_args)? {
        return Err(CliError::Exit(code));
    }
    if let Some(build_options) = root_version_invocation(&command_args) {
        write_git_compatible_version(io::stdout().lock(), build_options).map_err(CliError::Io)?;
        return Err(CliError::Exit(0));
    }
    if let Some(args) = parse_common_command_without_clap(&command_args) {
        let dispatch_args = if refs_list_invocation {
            original_command_args
        } else {
            command_args
        };
        return Ok((args, dispatch_args));
    }
    let command_args = normalize_empty_init_template(command_args);
    let command_args = normalize_empty_clone_template(command_args);
    let command_args = normalize_history_count_shorthand(command_args);
    let command_args = normalize_history_no_walk_value(command_args);
    let command_args = normalize_history_required_option_values(command_args);
    let command_args = normalize_show_interspersed_options(command_args);
    let command_args = normalize_log_date_hyphen_value(command_args);
    let command_args = normalize_diff_dirstat_short_value(command_args);
    let command_args = preserve_consumed_pathspec_separator(command_args);
    let command_args = normalize_status_untracked_short_value(command_args);
    let command_args = normalize_format_patch_relative_value(command_args);
    let command_args = normalize_update_index_ordered_force_remove_paths(command_args);
    validate_version_invocation_before_clap(&command_args)?;
    validate_var_invocation_before_clap(&command_args)?;
    validate_check_mailmap_invocation_before_clap(&command_args)?;
    validate_check_attr_invocation_before_clap(&command_args)?;
    validate_help_invocation_before_clap(raw_args, &command_args)?;
    validate_web_browse_invocation_before_clap(&command_args)?;
    validate_unavailable_foreign_helper_invocation_before_clap(&command_args)?;
    validate_sh_helper_invocation_before_clap(&command_args)?;
    validate_update_ref_invocation_before_clap(&command_args)?;
    validate_whatchanged_invocation_before_clap(&command_args)?;
    validate_scalar_invocation_before_clap(&command_args)?;
    validate_add_invocation_before_clap(&command_args)?;
    validate_status_invocation_before_clap(&command_args)?;
    validate_ls_files_invocation_before_clap(&command_args)?;
    validate_show_invocation_before_clap(&command_args)?;
    validate_restore_invocation_before_clap(&command_args)?;
    validate_rm_invocation_before_clap(&command_args)?;
    validate_branch_invocation_before_clap(&command_args)?;
    validate_for_each_ref_invocation_before_clap(&command_args)?;
    validate_diff_invocation_before_clap(&command_args)?;
    validate_fetch_invocation_before_clap(&command_args)?;
    validate_pull_invocation_before_clap(&command_args)?;
    validate_fetch_pack_invocation_before_clap(&command_args)?;
    validate_maintenance_invocation_before_clap(&command_args)?;
    validate_hash_object_invocation_before_clap(&command_args)?;
    validate_fast_export_invocation_before_clap(&command_args)?;
    validate_range_diff_invocation_before_clap(&command_args)?;
    validate_request_pull_invocation_before_clap(&command_args)?;
    validate_credential_store_invocation_before_clap(&command_args)?;
    validate_cherry_invocation_before_clap(&command_args)?;
    validate_commit_tree_invocation_before_clap(&command_args)?;
    validate_write_tree_invocation_before_clap(&command_args)?;
    validate_show_index_invocation_before_clap(&command_args)?;
    validate_update_server_info_invocation_before_clap(&command_args)?;
    validate_prune_packed_invocation_before_clap(&command_args)?;
    validate_verify_commit_invocation_before_clap(&command_args)?;
    validate_verify_pack_invocation_before_clap(&command_args)?;
    validate_count_objects_invocation_before_clap(&command_args)?;
    validate_patch_id_invocation_before_clap(&command_args)?;
    validate_stripspace_invocation_before_clap(&command_args)?;
    validate_mailsplit_invocation_before_clap(&command_args)?;
    validate_mergetool_invocation_before_clap(&command_args)?;
    validate_merge_tree_invocation_before_clap(&command_args)?;
    validate_merge_file_invocation_before_clap(&command_args)?;
    validate_mktree_invocation_before_clap(&command_args)?;
    validate_tag_invocation_before_clap(&command_args)?;
    validate_config_invocation_before_clap(&command_args)?;
    validate_bugreport_invocation_before_clap(&command_args)?;
    validate_refs_invocation_before_clap(&command_args)?;
    validate_unknown_command_invocation_before_clap(&command_args)?;
    if let Some(args) = parse_rev_list_object_walk(&command_args) {
        let dispatch_args = if refs_list_invocation {
            original_command_args
        } else {
            command_args
        };
        return Ok((args, dispatch_args));
    }
    let clap_command_args = normalize_rev_parse_end_of_options_for_clap(&command_args);
    let args = parse_validated_command(program, clap_command_args.as_ref());
    let dispatch_args = if refs_list_invocation {
        original_command_args
    } else {
        command_args
    };
    Ok((args, dispatch_args))
}

fn parse_common_command_without_clap(command_args: &[String]) -> Option<Args> {
    match command_args.first().map(String::as_str) {
        Some("status") => parse_common_status_without_clap(command_args),
        Some("show") => parse_common_show_without_clap(command_args),
        Some("ls-files") => parse_common_ls_files_without_clap(command_args),
        Some("config") => parse_common_config_without_clap(command_args),
        Some("branch") => parse_common_branch_without_clap(command_args),
        Some("for-each-ref") => parse_common_for_each_ref_without_clap(command_args),
        Some("ls-tree") => parse_common_ls_tree_without_clap(command_args),
        Some("rev-parse") => parse_common_rev_parse_without_clap(command_args),
        _ => None,
    }
}

fn normalize_refs_list_invocation(mut command_args: Vec<String>) -> Vec<String> {
    if !is_refs_list_invocation(&command_args) {
        return command_args;
    }

    command_args.drain(..2);
    let mut normalized = Vec::with_capacity(command_args.len() + 1);
    normalized.push("for-each-ref".to_owned());
    normalized.extend(command_args);
    normalized
}

fn is_refs_list_invocation(command_args: &[String]) -> bool {
    command_args.first().map(String::as_str) == Some("refs")
        && command_args.get(1).map(String::as_str) == Some("list")
}

fn normalize_for_each_ref_sort_options(command_args: Vec<String>) -> Vec<String> {
    if command_args.first().map(String::as_str) != Some("for-each-ref") {
        return command_args;
    }

    let mut normalized = Vec::with_capacity(command_args.len());
    let mut index = 0;
    while index < command_args.len() {
        let argument = &command_args[index];
        if argument == "--" {
            normalized.extend(command_args[index..].iter().cloned());
            break;
        }
        if argument == "--sort" {
            if let Some(value) = command_args.get(index + 1) {
                if matches!(value.as_str(), "--no-sort" | "-h" | "--help") {
                    normalized.push(format!("--sort={value}"));
                } else {
                    normalized.push(argument.clone());
                    normalized.push(value.clone());
                }
                index = index.saturating_add(2);
            } else {
                normalized.push(argument.clone());
                index = index.saturating_add(1);
            }
            continue;
        }
        if argument == "--no-sort" {
            clear_for_each_ref_sort_options(&mut normalized);
            index = index.saturating_add(1);
            continue;
        }
        normalized.push(argument.clone());
        index = index.saturating_add(1);
    }
    normalized
}

fn clear_for_each_ref_sort_options(command_args: &mut Vec<String>) {
    let mut without_sorts = Vec::with_capacity(command_args.len());
    let mut index = 0;
    while index < command_args.len() {
        if command_args[index] == "--sort" {
            index = index.saturating_add(2).min(command_args.len());
        } else if command_args[index].starts_with("--sort=") {
            index = index.saturating_add(1);
        } else {
            without_sorts.push(command_args[index].clone());
            index = index.saturating_add(1);
        }
    }
    *command_args = without_sorts;
}

fn parse_common_config_without_clap(command_args: &[String]) -> Option<Args> {
    let mut options = ConfigCommandArgs::default();
    for argument in command_args.iter().skip(1) {
        match argument.as_str() {
            "-z" | "--null" => options.null = true,
            "-l" | "--list" => options.list = true,
            "--show-origin" => options.show_origin = true,
            "--show-scope" => options.show_scope = true,
            "--includes" => options.includes = true,
            "--no-includes" => options.no_includes = true,
            _ => return None,
        }
    }
    Some(Args {
        command: Command::Config { options },
    })
}

fn parse_common_branch_without_clap(command_args: &[String]) -> Option<Args> {
    let mut options = BranchCommandArgs::default();
    for argument in command_args.iter().skip(1) {
        match argument.as_str() {
            "--show-current" => options.show_current = options.show_current.saturating_add(1),
            "--no-show-current" => {
                options.no_show_current = options.no_show_current.saturating_add(1)
            }
            "-q" | "--quiet" => options.quiet = options.quiet.saturating_add(1),
            _ => return None,
        }
    }
    Some(Args {
        command: Command::Branch { options },
    })
}

fn parse_common_for_each_ref_without_clap(command_args: &[String]) -> Option<Args> {
    let mut options = ForEachRefArgs::default();
    let mut arguments = command_args.iter().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--format" => options.format = Some(arguments.next()?.clone()),
            "--format=" => options.format = Some(EMPTY_FOR_EACH_REF_FORMAT.to_owned()),
            "--omit-empty" => options.omit_empty = true,
            "--" => {
                options.patterns.extend(arguments.cloned());
                break;
            }
            value if value.starts_with("--format=") => {
                options.format = Some(value["--format=".len()..].to_owned());
            }
            value if !value.starts_with('-') => options.patterns.push(value.to_owned()),
            _ => return None,
        }
    }
    Some(Args {
        command: Command::ForEachRef { options },
    })
}

fn parse_common_ls_tree_without_clap(command_args: &[String]) -> Option<Args> {
    let mut options = LsTreeCommandArgs::default();
    let mut arguments = command_args.iter().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "-d" => options.directory_only = true,
            "-r" => options.recursive = true,
            "-t" => options.show_trees = true,
            "-l" | "--long" => options.long = true,
            "-z" => options.nul_terminated = true,
            "--name-only" => options.name_only = true,
            "--name-status" => options.name_status = true,
            "--object-only" => options.object_only = true,
            "--full-tree" => options.full_tree = true,
            "--no-abbrev" => options.no_abbrev = true,
            "--" => {
                options.paths.extend(arguments.cloned());
                break;
            }
            value if !value.starts_with('-') && options.treeish.is_empty() => {
                options.treeish = value.to_owned();
            }
            value if !value.starts_with('-') => options.paths.push(value.to_owned()),
            _ => return None,
        }
    }
    if options.treeish.is_empty() {
        return None;
    }
    Some(Args {
        command: Command::LsTree { options },
    })
}

fn parse_common_rev_parse_without_clap(command_args: &[String]) -> Option<Args> {
    let mut options = RevParseCommandArgs::default();
    for argument in command_args.iter().skip(1) {
        match argument.as_str() {
            "--show-toplevel" => options.show_toplevel = true,
            "--show-prefix" => options.show_prefix = true,
            "--show-cdup" => options.show_cdup = true,
            "--git-dir" => options.git_dir = true,
            "--absolute-git-dir" => options.absolute_git_dir = true,
            "--git-common-dir" => options.git_common_dir = true,
            "--is-inside-git-dir" => options.is_inside_git_dir = true,
            "--is-inside-work-tree" => options.is_inside_work_tree = true,
            "--is-bare-repository" => options.is_bare_repository = true,
            "--is-shallow-repository" => options.is_shallow_repository = true,
            value if !value.starts_with('-') => options.revs.push(value.to_owned()),
            _ => return None,
        }
    }
    Some(Args {
        command: Command::RevParse { options },
    })
}

fn parse_common_ls_files_without_clap(command_args: &[String]) -> Option<Args> {
    let mut options = LsFilesCommandArgs::default();
    let mut arguments = command_args.iter().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "-c" | "--cached" => options.cached = true,
            "-z" => options.zero = true,
            "--full-name" => options.full_name = true,
            "--error-unmatch" => options.error_unmatch = true,
            "-t" => options.tagged = true,
            "-v" => options.lowercase_assume_valid = true,
            "-f" => options.fsmonitor_clean = true,
            "--deduplicate" => options.deduplicate = true,
            "-s" | "--stage" => options.stage = true,
            "-u" | "--unmerged" => options.unmerged = true,
            "-d" | "--deleted" => options.deleted = true,
            "-m" | "--modified" => options.modified = true,
            "-o" | "--others" => options.others = true,
            "--exclude-standard" => options.exclude_standard = true,
            "--" => {
                options.paths.extend(arguments.map(PathBuf::from));
                break;
            }
            value if !value.starts_with('-') || value == "-" => {
                options.paths.push(PathBuf::from(value));
            }
            _ => return None,
        }
    }
    Some(Args {
        command: Command::LsFiles(options),
    })
}

fn parse_common_show_without_clap(command_args: &[String]) -> Option<Args> {
    let mut options = ShowCommandArgs::default();
    let mut patch_mode = LogPatchMode::Default;
    let mut arguments = command_args.iter().skip(1).peekable();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--no-patch" => patch_mode = LogPatchMode::NoPatch,
            "--patch" => patch_mode = LogPatchMode::Patch,
            "--oneline" => options.oneline = true,
            "-z" => options.zero = true,
            "--stat" => options.stat = true,
            "--numstat" => options.numstat = true,
            "--shortstat" => options.shortstat = true,
            "--raw" => options.raw = true,
            "--summary" => options.summary = true,
            "--name-only" => options.name_only = true,
            "--name-status" => options.name_status = true,
            "--show-signature" => options.show_signature = true,
            "--parents" => options.parents = true,
            "--root" => options.root = true,
            "--first-parent" => options.first_parent = true,
            "--no-diff-merges" => options.no_diff_merges = true,
            "--do-walk" => options.do_walk = true,
            "--stdin" => options.stdin = true,
            "--format" => {
                if arguments.peek()?.starts_with('-') {
                    return None;
                }
                options.format = Some(arguments.next()?.clone());
            }
            "--pretty" => {
                options.pretty = Some("medium".to_owned());
            }
            "--encoding" => {
                if arguments.peek()?.starts_with('-') {
                    return None;
                }
                options.encoding = Some(arguments.next()?.clone());
            }
            "-n" | "--max-count" => {
                if arguments.peek()?.starts_with('-') {
                    return None;
                }
                options.max_count = Some(arguments.next()?.clone());
            }
            value if patch_mode.apply_short_patch_cluster(value) => {}
            "--" => {
                options.args.push("--".to_owned());
                options.args.extend(arguments.cloned());
                break;
            }
            value if value.starts_with("--format=") => {
                options.format = Some(value["--format=".len()..].to_owned());
            }
            value if value.starts_with("--pretty=") => {
                options.pretty = Some(value["--pretty=".len()..].to_owned());
            }
            value if value.starts_with("--encoding=") => {
                options.encoding = Some(value["--encoding=".len()..].to_owned());
            }
            value if value.starts_with("--max-count=") => {
                options.max_count = Some(value["--max-count=".len()..].to_owned());
            }
            value if value.starts_with("--diff-merges=") => {
                options.diff_merges = Some(value["--diff-merges=".len()..].to_owned());
            }
            value if !value.starts_with('-') || value == "-" => {
                options.args.push(value.to_owned());
            }
            _ => return None,
        }
    }
    options.patch = matches!(patch_mode, LogPatchMode::Patch);
    options.no_patch = matches!(patch_mode, LogPatchMode::NoPatch);
    Some(Args {
        command: Command::Show(options),
    })
}

fn parse_common_status_without_clap(command_args: &[String]) -> Option<Args> {
    let mut options = StatusCommandArgs::default();
    for argument in command_args.iter().skip(1) {
        match argument.as_str() {
            "--porcelain" => {
                options.porcelain = Some("v1".to_owned());
                options.no_porcelain = false;
            }
            "--no-porcelain" => {
                options.porcelain = None;
                options.no_porcelain = true;
            }
            "-b" | "--branch" => {
                options.branch = true;
                options.no_branch = false;
            }
            "--no-branch" => {
                options.branch = false;
                options.no_branch = true;
            }
            "--ahead-behind" => {
                options.ahead_behind = true;
                options.no_ahead_behind = false;
            }
            "--no-ahead-behind" => {
                options.ahead_behind = false;
                options.no_ahead_behind = true;
            }
            "-s" | "--short" => {
                options.short = true;
                options.no_short = false;
                options.long = false;
                options.no_long = false;
            }
            "--no-short" => {
                options.short = false;
                options.no_short = true;
            }
            "-z" | "--null" => {
                options.null = true;
                options.no_null = false;
            }
            "--no-null" => {
                options.null = false;
                options.no_null = true;
            }
            "--renames" => {
                options.renames = true;
                options.no_renames = false;
            }
            "--no-renames" => {
                options.renames = false;
                options.no_renames = true;
            }
            "--ignored" => {
                options.ignored = Some("traditional".to_owned());
                options.no_ignored = false;
            }
            "--no-ignored" => {
                options.ignored = None;
                options.no_ignored = true;
            }
            "-u" | "--untracked-files" => {
                options.untracked_files = Some("all".to_owned());
                options.no_untracked_files = false;
            }
            "--no-untracked-files" => {
                options.untracked_files = None;
                options.no_untracked_files = true;
            }
            value if value.starts_with("--porcelain=") => {
                options.porcelain = Some(value["--porcelain=".len()..].to_owned());
                options.no_porcelain = false;
            }
            value if value.starts_with("--ignored=") => {
                options.ignored = Some(value["--ignored=".len()..].to_owned());
                options.no_ignored = false;
            }
            value if value.starts_with("--untracked-files=") => {
                options.untracked_files = Some(value["--untracked-files=".len()..].to_owned());
                options.no_untracked_files = false;
            }
            value if value.starts_with("--ignore-submodules=") => {
                options.ignore_submodules = Some(value["--ignore-submodules=".len()..].to_owned());
                options.no_ignore_submodules = false;
            }
            _ => return None,
        }
    }
    Some(Args {
        command: Command::Status(options),
    })
}

fn normalize_status_untracked_short_value(args: Vec<String>) -> Vec<String> {
    if args.first().map(String::as_str) != Some("status") {
        return args;
    }
    let mut normalized = Vec::with_capacity(args.len() + 1);
    normalized.push(args[0].clone());
    let mut options = true;
    for arg in args.into_iter().skip(1) {
        if !options || arg == "--" {
            options = false;
            normalized.push(arg);
            continue;
        }
        let Some(shorts) = arg
            .strip_prefix('-')
            .filter(|value| !value.starts_with('-'))
        else {
            normalized.push(arg);
            continue;
        };
        let Some(untracked_index) = shorts.find('u') else {
            normalized.push(arg);
            continue;
        };
        let prefix = &shorts[..untracked_index];
        if !prefix
            .chars()
            .all(|option| matches!(option, 'b' | 's' | 'v' | 'z'))
        {
            normalized.push(arg);
            continue;
        }
        if !prefix.is_empty() {
            normalized.push(format!("-{prefix}"));
        }
        let value = &shorts[untracked_index + 1..];
        if value.is_empty() {
            normalized.push("-u".to_owned());
        } else {
            normalized.push(format!("--untracked-files={value}"));
        }
    }
    normalized
}

fn normalize_rev_parse_end_of_options_for_clap(
    command_args: &[String],
) -> std::borrow::Cow<'_, [String]> {
    if command_args.first().map(String::as_str) != Some("rev-parse")
        || !command_args
            .iter()
            .skip(1)
            .any(|arg| arg == "--end-of-options")
    {
        return std::borrow::Cow::Borrowed(command_args);
    }

    let mut normalized = command_args.to_vec();
    if let Some(marker) = normalized
        .iter_mut()
        .skip(1)
        .find(|arg| arg.as_str() == "--end-of-options")
    {
        *marker = "--".to_owned();
    }
    std::borrow::Cow::Owned(normalized)
}

fn parse_rev_list_object_walk(command_args: &[String]) -> Option<Args> {
    if command_args.first().map(String::as_str) != Some("rev-list") {
        return None;
    }
    let mut objects = false;
    let mut all = false;
    let mut object_names = false;
    let mut no_object_names = false;
    for arg in command_args.iter().skip(1) {
        match arg.as_str() {
            "--objects" => objects = true,
            "--all" => all = true,
            "--object-names" => object_names = true,
            "--no-object-names" => no_object_names = true,
            _ => return None,
        }
    }
    if !objects || !all {
        return None;
    }
    Some(Args {
        command: Command::RevList {
            oneline: false,
            header: false,
            graph: false,
            all,
            not: 0,
            exclude: Vec::new(),
            exclude_first_parent_only: false,
            exclude_hidden: None,
            exclude_promisor_objects: false,
            author: None,
            committer: None,
            alternate_refs: false,
            encoding: None,
            expand_tabs: false,
            no_expand_tabs: false,
            notes: false,
            no_notes: false,
            show_notes: false,
            show_notes_by_default: false,
            standard_notes: false,
            no_standard_notes: false,
            abbrev_commit: false,
            no_abbrev_commit: false,
            grep: Vec::new(),
            invert_grep: false,
            all_match: false,
            regexp_ignore_case: false,
            basic_regexp: false,
            extended_regexp: false,
            fixed_strings: false,
            perl_regexp: false,
            bisect: false,
            bisect_all: false,
            bisect_vars: false,
            cherry: false,
            count: false,
            glob: Vec::new(),
            skip: None,
            branches: Vec::new(),
            tags: Vec::new(),
            remotes: Vec::new(),
            max_parents: None,
            max_age: None,
            no_max_parents: false,
            merges: false,
            merge: false,
            min_parents: None,
            min_age: None,
            no_min_parents: false,
            no_merges: false,
            objects,
            objects_edge: false,
            objects_edge_aggressive: false,
            indexed_objects: false,
            unpacked: false,
            remove_empty: false,
            ignore_missing: false,
            object_names,
            no_object_names,
            filter: None,
            filter_print_omitted: false,
            filter_provided_objects: false,
            parents: false,
            first_parent: false,
            children: false,
            walk_reflogs: false,
            reflog: false,
            do_walk: false,
            no_walk: false,
            stdin: false,
            grep_reflog: Vec::new(),
            reverse: false,
            full_history: false,
            in_commit_order: false,
            ancestry_path: false,
            dense: false,
            sparse: false,
            show_pulls: false,
            show_linear_break: false,
            simplify_merges: false,
            simplify_by_decoration: false,
            topo_order: false,
            date_order: false,
            author_date_order: false,
            left_right: false,
            left_only: false,
            right_only: false,
            cherry_pick: false,
            cherry_mark: false,
            boundary: false,
            max_count: None,
            since: None,
            since_as_filter: None,
            until: None,
            relative_date: false,
            timestamp: false,
            date: None,
            show_signature: false,
            single_worktree: false,
            commit_header: false,
            no_commit_header: false,
            disk_usage: None,
            progress: false,
            no_filter: false,
            missing: None,
            use_bitmap_index: false,
            quiet: false,
            format: None,
            pretty: None,
            revs: Vec::new(),
        },
    })
}

fn parse_validated_command(program: String, command_args: &[String]) -> Args {
    match command_args.first().map(String::as_str) {
        Some("status") => {
            let status = StatusOnlyArgs::try_parse_from(
                std::iter::once(format!("{program} status"))
                    .chain(command_args.iter().skip(1).cloned()),
            )
            .unwrap_or_else(|error| error.exit());
            return Args {
                command: Command::Status(status.options),
            };
        }
        Some("config") => {
            let config = ConfigOnlyArgs::try_parse_from(
                std::iter::once(format!("{program} config"))
                    .chain(command_args.iter().skip(1).cloned()),
            )
            .unwrap_or_else(|error| error.exit());
            return Args {
                command: Command::Config {
                    options: config.options,
                },
            };
        }
        Some("ls-files") => {
            let ls_files = LsFilesOnlyArgs::try_parse_from(
                std::iter::once(format!("{program} ls-files"))
                    .chain(command_args.iter().skip(1).cloned()),
            )
            .unwrap_or_else(|error| error.exit());
            return Args {
                command: Command::LsFiles(ls_files.options),
            };
        }
        Some("show") => {
            let show = ShowOnlyArgs::try_parse_from(
                std::iter::once(format!("{program} show"))
                    .chain(command_args.iter().skip(1).cloned()),
            )
            .unwrap_or_else(|error| error.exit());
            return Args {
                command: Command::Show(show.options),
            };
        }
        Some("check-attr") => {
            let check_attr = CheckAttrOnlyArgs::try_parse_from(
                std::iter::once(format!("{program} check-attr"))
                    .chain(command_args.iter().skip(1).cloned()),
            )
            .unwrap_or_else(|error| error.exit());
            return Args {
                command: Command::CheckAttr(check_attr.options),
            };
        }
        Some("for-each-ref") => {
            let for_each_ref = ForEachRefOnlyArgs::try_parse_from(
                std::iter::once(format!("{program} for-each-ref"))
                    .chain(command_args.iter().skip(1).cloned()),
            )
            .unwrap_or_else(|error| error.exit());
            return Args {
                command: Command::ForEachRef {
                    options: for_each_ref.options,
                },
            };
        }
        Some("show-ref") => {
            let show_ref = ShowRefOnlyArgs::try_parse_from(
                std::iter::once(format!("{program} show-ref"))
                    .chain(command_args.iter().skip(1).cloned()),
            )
            .unwrap_or_else(|error| error.exit());
            return Args {
                command: Command::ShowRef {
                    options: show_ref.options,
                },
            };
        }
        Some("ls-tree") => {
            let ls_tree = LsTreeOnlyArgs::try_parse_from(
                std::iter::once(format!("{program} ls-tree"))
                    .chain(command_args.iter().skip(1).cloned()),
            )
            .unwrap_or_else(|error| error.exit());
            return Args {
                command: Command::LsTree {
                    options: ls_tree.options,
                },
            };
        }
        Some("rev-parse") => {
            let rev_parse = RevParseOnlyArgs::try_parse_from(
                std::iter::once(format!("{program} rev-parse"))
                    .chain(command_args.iter().skip(1).cloned()),
            )
            .unwrap_or_else(|error| error.exit());
            return Args {
                command: Command::RevParse {
                    options: rev_parse.options,
                },
            };
        }
        Some("branch") => {
            let branch = BranchOnlyArgs::try_parse_from(
                std::iter::once(format!("{program} branch"))
                    .chain(command_args.iter().skip(1).cloned()),
            )
            .unwrap_or_else(|error| error.exit());
            return Args {
                command: Command::Branch {
                    options: branch.options,
                },
            };
        }
        Some("log") => {
            let log = LogOnlyArgs::try_parse_from(
                std::iter::once(format!("{program} log"))
                    .chain(command_args.iter().skip(1).cloned()),
            )
            .unwrap_or_else(|error| error.exit());
            return Args {
                command: Command::Log {
                    options: log.options,
                },
            };
        }
        Some("daemon") => {
            let daemon = DaemonOnlyArgs::try_parse_from(
                std::iter::once(format!("{program} daemon"))
                    .chain(command_args.iter().skip(1).cloned()),
            )
            .unwrap_or_else(|error| error.exit());
            return Args {
                command: Command::Daemon {
                    options: daemon.options,
                },
            };
        }
        _ => {
            if let Some(args) = parse_top_level_command_only(program.clone(), command_args) {
                return args;
            }
        }
    }
    unreachable!("validated top-level command has no command-only parser")
}

fn write_root_help(mut writer: impl std::io::Write) -> io::Result<()> {
    writer.write_all(ROOT_HELP_TEXT.as_bytes())
}

fn try_handle_root_list_cmds_invocation(raw_args: &[String]) -> Result<bool> {
    let mut handled = false;
    for arg in raw_args {
        if let Some(value) = arg.strip_prefix("--list-cmds=") {
            let wants_builtins = value.split(',').any(|item| item == "builtins");
            if wants_builtins {
                io::stdout()
                    .lock()
                    .write_all(BUILTINS_TEXT.as_bytes())
                    .map_err(CliError::Io)?;
                handled = true;
            }
        }
    }
    Ok(handled)
}

fn normalize_diff_dirstat_short_value(command_args: Vec<String>) -> Vec<String> {
    let Some(command) = command_args.first().map(String::as_str) else {
        return command_args;
    };
    if !matches!(command, "diff" | "diff-files" | "diff-index" | "diff-tree") {
        return command_args;
    }
    let mut normalized = Vec::with_capacity(command_args.len());
    let mut stop_normalizing = false;
    for arg in command_args {
        if stop_normalizing {
            normalized.push(arg);
            continue;
        }
        if arg == "--" {
            stop_normalizing = true;
            normalized.push(arg);
            continue;
        }
        if arg == "-X" {
            normalized.push("--dirstat".to_owned());
            continue;
        }
        if let Some(value) = arg.strip_prefix("-X")
            && !value.is_empty()
        {
            normalized.push(format!("--dirstat={value}"));
            continue;
        }
        normalized.push(arg);
    }
    normalized
}

fn preserve_consumed_pathspec_separator(mut command_args: Vec<String>) -> Vec<String> {
    if !matches!(
        command_args.first().map(String::as_str),
        Some("diff" | "log" | "rev-list")
    ) {
        return command_args;
    }
    let Some(separator) = command_args.iter().skip(1).position(|arg| arg == "--") else {
        return command_args;
    };
    let pathspec_start = separator + 2;
    if pathspec_start < command_args.len() {
        command_args.insert(pathspec_start, "--".to_owned());
    }
    command_args
}

fn maybe_exec_diff_output_redirection(
    raw_args: &[String],
    command_args: &[String],
) -> Result<Option<i32>> {
    let Some(command) = command_args.first().map(String::as_str) else {
        return Ok(None);
    };
    if !matches!(command, "diff" | "diff-files" | "diff-index" | "diff-tree") {
        return Ok(None);
    }
    let Some(output_path) = find_diff_output_path(raw_args, command)? else {
        return Ok(None);
    };
    let child_args = strip_diff_output_args(raw_args, command);
    let output_file = fs::File::create(output_path).map_err(CliError::Io)?;
    let status = std::process::Command::new(std::env::current_exe().map_err(CliError::Io)?)
        .args(child_args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::from(output_file))
        .stderr(Stdio::inherit())
        .status()
        .map_err(CliError::Io)?;
    Ok(Some(status.code().unwrap_or(1)))
}

fn find_diff_output_path(raw_args: &[String], command: &str) -> Result<Option<std::path::PathBuf>> {
    let Some(command_index) = raw_args.iter().position(|arg| arg == command) else {
        return Ok(None);
    };
    let mut output_path = None;
    let mut index = command_index + 1;
    while index < raw_args.len() {
        let arg = &raw_args[index];
        if arg == "--" {
            break;
        }
        if arg == "--output" {
            let Some(path) = raw_args.get(index + 1) else {
                return Ok(None);
            };
            output_path = Some(std::path::PathBuf::from(path));
            index += 2;
            continue;
        }
        if let Some(path) = arg.strip_prefix("--output=") {
            output_path = Some(std::path::PathBuf::from(path));
        }
        index += 1;
    }
    Ok(output_path)
}

fn strip_diff_output_args(raw_args: &[String], command: &str) -> Vec<String> {
    let Some(command_index) = raw_args.iter().position(|arg| arg == command) else {
        return raw_args.to_vec();
    };
    let mut stripped = raw_args[..=command_index].to_vec();
    let mut index = command_index + 1;
    while index < raw_args.len() {
        let arg = &raw_args[index];
        if arg == "--" {
            stripped.extend_from_slice(&raw_args[index..]);
            break;
        }
        if arg == "--output" {
            index += 2;
            continue;
        }
        if arg.starts_with("--output=") {
            index += 1;
            continue;
        }
        stripped.push(arg.clone());
        index += 1;
    }
    stripped
}

fn validate_add_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if !matches!(
        command_args.first().map(String::as_str),
        Some("add" | "stage")
    ) {
        return Ok(());
    }
    for arg in command_args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        let switch = match arg.as_str() {
            "-a" => "a",
            "-1" => "1",
            "-2" => "2",
            _ => continue,
        };
        return Err(CliError::Stderr {
            code: 129,
            text: format!("error: unknown switch `{switch}'\n{ADD_USAGE}"),
        });
    }
    Ok(())
}

const ADD_USAGE: &str = "usage: git add [<options>] [--] <pathspec>...\n\n    -n, --[no-]dry-run    dry run\n    -v, --[no-]verbose    be verbose\n\n    -i, --[no-]interactive\n                          interactive picking\n    -p, --[no-]patch      select hunks interactively\n    -e, --[no-]edit       edit current diff and apply\n    -f, --[no-]force      allow adding otherwise ignored files\n    -u, --[no-]update     update tracked files\n    --[no-]renormalize    renormalize EOL of tracked files (implies -u)\n    -N, --[no-]intent-to-add\n                          record only the fact that the path will be added later\n    -A, --[no-]all        add changes from all tracked and untracked files\n    --[no-]ignore-removal ignore paths removed in the working tree (same as --no-all)\n    --[no-]refresh        don't add, only refresh the index\n    --[no-]ignore-errors  just skip files which cannot be added because of errors\n    --[no-]ignore-missing check if - even missing - files are ignored in dry run\n    --[no-]sparse         allow updating entries outside of the sparse-checkout cone\n    --[no-]chmod (+|-)x   override the executable bit of the listed files\n    --[no-]pathspec-from-file <file>\n                          read pathspec from file\n    --[no-]pathspec-file-nul\n                          with --pathspec-from-file, pathspec elements are separated with NUL character\n\n";

fn validate_status_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("status") {
        return Ok(());
    }
    for arg in command_args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        if arg == "-1" {
            return Err(CliError::Stderr {
                code: 129,
                text: format!("error: unknown switch `1'\n{STATUS_USAGE}"),
            });
        }
        if let Some(switch) = status_unknown_short_switch(arg) {
            return Err(CliError::Stderr {
                code: 129,
                text: format!("error: unknown switch `{switch}'\n{STATUS_USAGE}"),
            });
        }
        if let Some(option) = arg.strip_prefix("--")
            && !status_long_option_is_known(arg)
        {
            return Err(CliError::Stderr {
                code: 129,
                text: format!("error: unknown option `{option}'\n{STATUS_USAGE}"),
            });
        }
    }
    Ok(())
}

fn validate_ls_files_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("ls-files") {
        return Ok(());
    }
    let mut index = 1;
    while index < command_args.len() {
        let arg = &command_args[index];
        if arg == "--" {
            break;
        }
        if let Some(switch) = ls_files_unknown_short_switch(arg) {
            return Err(CliError::Stderr {
                code: 129,
                text: format!("error: unknown switch `{switch}'\n{LS_FILES_USAGE}"),
            });
        }
        if let Some(option) = arg.strip_prefix("--")
            && !ls_files_long_option_is_known(option)
        {
            return Err(CliError::Stderr {
                code: 129,
                text: format!("error: unknown option `{option}'\n{LS_FILES_USAGE}"),
            });
        }
        if matches!(
            arg.as_str(),
            "-x" | "-X"
                | "--exclude"
                | "--exclude-from"
                | "--exclude-per-directory"
                | "--format"
                | "--with-tree"
        ) {
            index += 1;
        }
        index += 1;
    }
    Ok(())
}

fn ls_files_unknown_short_switch(arg: &str) -> Option<char> {
    let options = arg.strip_prefix('-')?;
    if options.is_empty() || options.starts_with('-') {
        return None;
    }
    for option in options.chars() {
        match option {
            'z' | 't' | 'v' | 'f' | 'c' | 'd' | 'm' | 'o' | 'i' | 's' | 'k' | 'u' => {}
            'x' | 'X' => return None,
            unknown => return Some(unknown),
        }
    }
    None
}

fn ls_files_long_option_is_known(option: &str) -> bool {
    matches!(
        option.split_once('=').map_or(option, |(name, _)| name),
        "abbrev"
            | "cached"
            | "debug"
            | "deduplicate"
            | "deleted"
            | "directory"
            | "empty-directory"
            | "eol"
            | "error-unmatch"
            | "exclude"
            | "exclude-from"
            | "exclude-per-directory"
            | "exclude-standard"
            | "format"
            | "full-name"
            | "ignored"
            | "killed"
            | "modified"
            | "no-abbrev"
            | "no-cached"
            | "no-debug"
            | "no-deduplicate"
            | "no-deleted"
            | "no-directory"
            | "no-empty-directory"
            | "no-eol"
            | "no-error-unmatch"
            | "no-ignored"
            | "no-killed"
            | "no-modified"
            | "no-others"
            | "no-recurse-submodules"
            | "no-resolve-undo"
            | "no-sparse"
            | "no-stage"
            | "no-unmerged"
            | "no-with-tree"
            | "others"
            | "recurse-submodules"
            | "resolve-undo"
            | "sparse"
            | "stage"
            | "unmerged"
            | "with-tree"
    )
}

fn validate_show_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("show") {
        return Ok(());
    }
    for (index, arg) in command_args.iter().enumerate().skip(1) {
        if arg == "--" {
            break;
        }
        if matches!(
            arg.as_str(),
            "--diff-merges" | "--encoding" | "--format" | "--max-count" | "--pretty" | "-n"
        ) && command_args
            .get(index + 1)
            .is_none_or(|value| value.starts_with('-'))
        {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("unrecognized argument: {arg}"),
            });
        }
    }
    Ok(())
}

fn status_unknown_short_switch(arg: &str) -> Option<char> {
    let options = arg.strip_prefix('-')?;
    if options.is_empty() || options.starts_with('-') {
        return None;
    }
    for option in options.chars() {
        match option {
            'b' | 's' | 'v' | 'z' => {}
            'M' | 'u' => return None,
            unknown => return Some(unknown),
        }
    }
    None
}

fn status_long_option_is_known(arg: &str) -> bool {
    matches!(
        arg,
        "--ahead-behind"
            | "--branch"
            | "--column"
            | "--find-renames"
            | "--ignore-submodules"
            | "--ignored"
            | "--long"
            | "--no-ahead-behind"
            | "--no-branch"
            | "--no-column"
            | "--no-ignore-submodules"
            | "--no-ignored"
            | "--no-long"
            | "--no-null"
            | "--no-porcelain"
            | "--no-renames"
            | "--no-short"
            | "--no-show-stash"
            | "--no-untracked-files"
            | "--no-verbose"
            | "--null"
            | "--porcelain"
            | "--renames"
            | "--short"
            | "--show-stash"
            | "--untracked-files"
            | "--verbose"
    ) || [
        "--column=",
        "--find-renames=",
        "--ignore-submodules=",
        "--ignored=",
        "--porcelain=",
        "--untracked-files=",
    ]
    .iter()
    .any(|prefix| arg.starts_with(prefix))
}

const STATUS_USAGE: &str = "usage: git status [<options>] [--] [<pathspec>...]\n\n    -v, --[no-]verbose    be verbose\n    -s, --[no-]short      show status concisely\n    -b, --[no-]branch     show branch information\n    --[no-]show-stash     show stash information\n    --[no-]ahead-behind   compute full ahead/behind values\n    --[no-]porcelain[=<version>]\n                          machine-readable output\n    --[no-]long           show status in long format (default)\n    -z, --[no-]null       terminate entries with NUL\n    -u, --[no-]untracked-files[=<mode>]\n                          show untracked files, optional modes: all, normal, no. (Default: all)\n    --[no-]ignored[=<mode>]\n                          show ignored files, optional modes: traditional, matching, no. (Default: traditional)\n    --[no-]ignore-submodules[=<when>]\n                          ignore changes to submodules, optional when: all, dirty, untracked. (Default: all)\n    --[no-]column[=<style>]\n                          list untracked files in columns\n    --no-renames          do not detect renames\n    --renames             opposite of --no-renames\n    -M, --find-renames[=<n>]\n                          detect renames, optionally set similarity index\n\n";

const WORKTREE_USAGE: &str = concat!(
    "usage: git worktree add [-f] [--detach] [--checkout] [--lock [--reason <string>]]\n",
    "                        [--orphan] [(-b | -B) <new-branch>] <path> [<commit-ish>]\n",
    "   or: git worktree list [-v | --porcelain [-z]]\n",
    "   or: git worktree lock [--reason <string>] <worktree>\n",
    "   or: git worktree move <worktree> <new-path>\n",
    "   or: git worktree prune [-n] [-v] [--expire <expire>]\n",
    "   or: git worktree remove [-f] <worktree>\n",
    "   or: git worktree repair [<path>...]\n",
    "   or: git worktree unlock <worktree>\n\n"
);

const WRITE_TREE_USAGE: &str = "usage: git write-tree [--missing-ok] [--prefix=<prefix>/]\n\n    --[no-]missing-ok     allow missing objects\n    --[no-]prefix <prefix>/\n                          write tree object for a subdirectory <prefix>\n\n";

const SUBMODULE_USAGE: &str = "usage: git submodule [--quiet] [--cached]\n   or: git submodule [--quiet] add [-b <branch>] [-f|--force] [--name <name>] [--reference <repository>] [--] <repository> [<path>]\n   or: git submodule [--quiet] status [--cached] [--recursive] [--] [<path>...]\n   or: git submodule [--quiet] init [--] [<path>...]\n   or: git submodule [--quiet] deinit [-f|--force] (--all| [--] <path>...)\n   or: git submodule [--quiet] update [--init [--filter=<filter-spec>]] [--remote] [-N|--no-fetch] [-f|--force] [--checkout|--merge|--rebase] [--[no-]recommend-shallow] [--reference <repository>] [--recursive] [--[no-]single-branch] [--] [<path>...]\n   or: git submodule [--quiet] set-branch (--default|--branch <branch>) [--] <path>\n   or: git submodule [--quiet] set-url [--] <path> <newurl>\n   or: git submodule [--quiet] summary [--cached|--files] [--summary-limit <n>] [commit] [--] [<path>...]\n   or: git submodule [--quiet] foreach [--recursive] <command>\n   or: git submodule [--quiet] sync [--recursive] [--] [<path>...]\n   or: git submodule [--quiet] absorbgitdirs [--] [<path>...]\n";

const STASH_TOP_LEVEL_USAGE: &str = "usage: git stash list [<log-options>]\n   or: git stash show [-u | --include-untracked | --only-untracked] [<diff-options>] [<stash>]\n   or: git stash drop [-q | --quiet] [<stash>]\n   or: git stash pop [--index] [-q | --quiet] [<stash>]\n   or: git stash apply [--index] [-q | --quiet] [<stash>]\n   or: git stash branch <branchname> [<stash>]\n   or: git stash [push [-p | --patch] [-S | --staged] [-k | --[no-]keep-index] [-q | --quiet]\n                 [-u | --include-untracked] [-a | --all] [(-m | --message) <message>]\n                 [--pathspec-from-file=<file> [--pathspec-file-nul]]\n                 [--] [<pathspec>...]]\n   or: git stash save [-p | --patch] [-S | --staged] [-k | --[no-]keep-index] [-q | --quiet]\n                 [-u | --include-untracked] [-a | --all] [<message>]\n   or: git stash clear\n   or: git stash create [<message>]\n   or: git stash store [(-m | --message) <message>] [-q | --quiet] <commit>\n\n";
const STASH_LIST_USAGE: &str = "usage: git stash list [<log-options>]\n\n";
const STASH_PUSH_USAGE: &str = "usage: git stash [push [-p | --patch] [-S | --staged] [-k | --[no-]keep-index] [-q | --quiet]\n                 [-u | --include-untracked] [-a | --all] [(-m | --message) <message>]\n                 [--pathspec-from-file=<file> [--pathspec-file-nul]]\n                 [--] [<pathspec>...]]\n\n    -k, --[no-]keep-index keep index\n    -S, --[no-]staged     stash staged changes only\n    -p, --[no-]patch      stash in patch mode\n    -q, --[no-]quiet      quiet mode\n    -u, --[no-]include-untracked\n                          include untracked files in stash\n    -a, --[no-]all        include ignore files\n    -m, --[no-]message <message>\n                          stash message\n    --[no-]pathspec-from-file <file>\n                          read pathspec from file\n    --[no-]pathspec-file-nul\n                          with --pathspec-from-file, pathspec elements are separated with NUL character\n\n";
const STASH_APPLY_USAGE: &str = "usage: git stash apply [--index] [-q | --quiet] [<stash>]\n\n    -q, --[no-]quiet      be quiet, only report errors\n    --[no-]index          attempt to recreate the index\n\n";
const STASH_DROP_USAGE: &str = "usage: git stash drop [-q | --quiet] [<stash>]\n\n    -q, --[no-]quiet      be quiet, only report errors\n\n";
const STASH_POP_USAGE: &str = "usage: git stash pop [--index] [-q | --quiet] [<stash>]\n\n    -q, --[no-]quiet      be quiet, only report errors\n    --[no-]index          attempt to recreate the index\n\n";

fn validate_restore_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("restore") {
        return Ok(());
    }
    for arg in command_args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        let no_value_option = match arg.as_str() {
            _ if arg.starts_with("--staged=") => Some("staged"),
            _ if arg.starts_with("--worktree=") => Some("worktree"),
            _ if arg.starts_with("--no-staged=") => Some("no-staged"),
            _ if arg.starts_with("--no-worktree=") => Some("no-worktree"),
            _ => None,
        };
        if let Some(option) = no_value_option {
            return Err(CliError::Stderr {
                code: 129,
                text: format!("error: option `{option}' takes no value\n"),
            });
        }
        if arg.starts_with("-S=") || arg.starts_with("-W=") {
            return Err(CliError::Stderr {
                code: 129,
                text: format!("error: unknown switch `='\n{RESTORE_USAGE}"),
            });
        }
    }
    Ok(())
}

const RESTORE_USAGE: &str = "usage: git restore [<options>] [--source=<branch>] <file>...\n\n    -s, --[no-]source <tree-ish>\n                          which tree-ish to checkout from\n    -S, --[no-]staged     restore the index\n    -W, --[no-]worktree   restore the working tree (default)\n    --[no-]ignore-unmerged\n                          ignore unmerged entries\n    --[no-]overlay        use overlay mode\n    -q, --[no-]quiet      suppress progress reporting\n    --[no-]recurse-submodules[=<checkout>]\n                          control recursive updating of submodules\n    --[no-]progress       force progress reporting\n    -m, --[no-]merge      perform a 3-way merge with the new branch\n    --[no-]conflict <style>\n                          conflict style (merge, diff3, or zdiff3)\n    -2, --ours            checkout our version for unmerged files\n    -3, --theirs          checkout their version for unmerged files\n    -p, --[no-]patch      select hunks interactively\n    --[no-]ignore-skip-worktree-bits\n                          do not limit pathspecs to sparse entries only\n    --[no-]pathspec-from-file <file>\n                          read pathspec from file\n    --[no-]pathspec-file-nul\n                          with --pathspec-from-file, pathspec elements are separated with NUL character\n\n";

fn validate_rm_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("rm") {
        return Ok(());
    }
    for arg in command_args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        let message = match arg.as_str() {
            "-0" => Some("error: unknown switch `0'\n"),
            "-A" => Some("error: unknown switch `A'\n"),
            "-a" => Some("error: unknown switch `a'\n"),
            "-u" => Some("error: unknown switch `u'\n"),
            "-z" => Some("error: unknown switch `z'\n"),
            "--name-only" => Some("error: unknown option `name-only'\n"),
            "--literal-pathspecs" => Some("error: unknown option `literal-pathspecs'\n"),
            _ if arg.starts_with("--diff-filter") => {
                Some("error: unknown option `diff-filter=A'\n")
            }
            _ => None,
        };
        if let Some(message) = message {
            return Err(CliError::Stderr {
                code: 129,
                text: format!("{message}{RM_USAGE}"),
            });
        }
    }
    Ok(())
}

const RM_USAGE: &str = "usage: git rm [-f | --force] [-n] [-r] [--cached] [--ignore-unmatch]\n              [--quiet] [--pathspec-from-file=<file> [--pathspec-file-nul]]\n              [--] [<pathspec>...]\n\n    -n, --[no-]dry-run    dry run\n    -q, --[no-]quiet      do not list removed files\n    --[no-]cached         only remove from the index\n    -f, --[no-]force      override the up-to-date check\n    -r                    allow recursive removal\n    --[no-]ignore-unmatch exit with a zero status even if nothing matched\n    --[no-]sparse         allow updating entries outside of the sparse-checkout cone\n    --[no-]pathspec-from-file <file>\n                          read pathspec from file\n    --[no-]pathspec-file-nul\n                          with --pathspec-from-file, pathspec elements are separated with NUL character\n\n";

fn validate_branch_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("branch") {
        return Ok(());
    }
    for arg in command_args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        let message = match arg.as_str() {
            "-b" => Some("error: unknown switch `b'\n"),
            "--rebase-merges" => Some("error: unknown option `rebase-merges'\n"),
            _ => None,
        };
        if let Some(message) = message {
            return Err(CliError::Stderr {
                code: 129,
                text: format!("{message}{BRANCH_USAGE}"),
            });
        }
    }
    validate_known_long_options_before_clap(
        command_args,
        "branch",
        BRANCH_USAGE,
        &[
            "--help",
            "--remotes",
            "--all",
            "--list",
            "--no-list",
            "--force",
            "--quiet",
            "--verbose",
            "--no-verbose",
            "--abbrev",
            "--no-abbrev",
            "--column",
            "--no-column",
            "--ignore-case",
            "--no-color",
            "--create-reflog",
            "--no-create-reflog",
            "--show-current",
            "--no-show-current",
            "--edit-description",
            "--delete",
            "--move",
            "--copy",
            "--set-upstream",
            "--unset-upstream",
            "--no-track",
            "--omit-empty",
            "--no-sort",
            "--recurse-submodules",
            "--no-recurse-submodules",
        ],
        &[
            "--abbrev=",
            "--column=",
            "--color=",
            "--track",
            "--track=",
            "--sort",
            "--sort=",
            "--format",
            "--format=",
            "--contains",
            "--contains=",
            "--no-contains",
            "--no-contains=",
            "--merged",
            "--merged=",
            "--no-merged",
            "--no-merged=",
            "--points-at",
            "--points-at=",
            "--set-upstream-to",
            "--set-upstream-to=",
        ],
    )?;
    Ok(())
}

fn validate_for_each_ref_invocation_before_clap(command_args: &[String]) -> Result<()> {
    validate_known_long_options_before_clap(
        command_args,
        "for-each-ref",
        FOR_EACH_REF_USAGE,
        &[
            "--help",
            "--shell",
            "--perl",
            "--python",
            "--tcl",
            "--no-color",
            "--omit-empty",
            "--ignore-case",
            "--stdin",
            "--include-root-refs",
        ],
        &[
            "--count",
            "--count=",
            "--format",
            "--format=",
            "--color",
            "--color=",
            "--exclude",
            "--exclude=",
            "--sort",
            "--sort=",
            "--points-at",
            "--points-at=",
            "--merged",
            "--merged=",
            "--no-merged",
            "--no-merged=",
            "--contains",
            "--contains=",
            "--no-contains",
            "--no-contains=",
            "--start-after",
            "--start-after=",
        ],
    )
}

const BRANCH_USAGE: &str = "usage: git branch [<options>] [-r | -a] [--merged] [--no-merged]\n   or: git branch [<options>] [-f] [--recurse-submodules] <branch-name> [<start-point>]\n   or: git branch [<options>] [-l] [<pattern>...]\n   or: git branch [<options>] [-r] (-d | -D) <branch-name>...\n   or: git branch [<options>] (-m | -M) [<old-branch>] <new-branch>\n   or: git branch [<options>] (-c | -C) [<old-branch>] <new-branch>\n   or: git branch [<options>] [-r | -a] [--points-at]\n   or: git branch [<options>] [-r | -a] [--format]\n\nGeneric options\n    -v, --[no-]verbose    show hash and subject, give twice for upstream branch\n    -q, --[no-]quiet      suppress informational messages\n    -t, --[no-]track[=(direct|inherit)]\n                          set branch tracking configuration\n    -u, --[no-]set-upstream-to <upstream>\n                          change the upstream info\n    --[no-]unset-upstream unset the upstream info\n    --[no-]color[=<when>] use colored output\n    -r, --remotes         act on remote-tracking branches\n    --contains <commit>   print only branches that contain the commit\n    --no-contains <commit>\n                          print only branches that don't contain the commit\n    --[no-]abbrev[=<n>]   use <n> digits to display object names\n\nSpecific git-branch actions:\n    -a, --all             list both remote-tracking and local branches\n    -d, --[no-]delete     delete fully merged branch\n    -D                    delete branch (even if not merged)\n    -m, --[no-]move       move/rename a branch and its reflog\n    -M                    move/rename a branch, even if target exists\n    --[no-]omit-empty     do not output a newline after empty formatted refs\n    -c, --[no-]copy       copy a branch and its reflog\n    -C                    copy a branch, even if target exists\n    -l, --[no-]list       list branch names\n    --[no-]show-current   show current branch name\n    --[no-]create-reflog  create the branch's reflog\n    --[no-]edit-description\n                          edit the description for the branch\n    -f, --[no-]force      force creation, move/rename, deletion\n    --merged <commit>     print only branches that are merged\n    --no-merged <commit>  print only branches that are not merged\n    --[no-]column[=<style>]\n                          list branches in columns\n    --[no-]sort <key>     field name to sort on\n    --[no-]points-at <object>\n                          print only branches of the object\n    -i, --[no-]ignore-case\n                          sorting and filtering are case insensitive\n    --[no-]recurse-submodules\n                          recurse through submodules\n    --[no-]format <format>\n                          format to use for the output\n\n";

fn validate_refs_list_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if !is_refs_list_invocation(command_args) {
        return Ok(());
    }
    let validation_args = std::iter::once("refs list".to_owned())
        .chain(command_args.iter().skip(2).cloned())
        .collect::<Vec<_>>();
    validate_known_long_options_before_clap(
        &validation_args,
        "refs list",
        REFS_LIST_USAGE,
        &[
            "--help",
            "--shell",
            "--perl",
            "--python",
            "--tcl",
            "--no-color",
            "--omit-empty",
            "--ignore-case",
            "--stdin",
            "--include-root-refs",
            "--no-sort",
        ],
        &[
            "--count",
            "--count=",
            "--format",
            "--format=",
            "--color",
            "--color=",
            "--exclude",
            "--exclude=",
            "--sort",
            "--sort=",
            "--points-at",
            "--points-at=",
            "--merged",
            "--merged=",
            "--no-merged",
            "--no-merged=",
            "--contains",
            "--contains=",
            "--no-contains",
            "--no-contains=",
            "--start-after",
            "--start-after=",
        ],
    )?;
    validate_for_each_ref_option_values(&validation_args)
}

fn validate_for_each_ref_option_values(command_args: &[String]) -> Result<()> {
    let mut index = 1;
    while index < command_args.len() {
        let argument = &command_args[index];
        if argument == "--" {
            break;
        }
        if argument == "--sort" {
            let Some(_value) = command_args.get(index + 1) else {
                return Err(CliError::Stderr {
                    code: 129,
                    text: "error: option `sort' requires a value\n".to_owned(),
                });
            };
            index = index.saturating_add(2);
            continue;
        }
        if argument == "--count" {
            if let Some(value) = command_args.get(index + 1) {
                validate_refs_count_value(value)?;
                index = index.saturating_add(2);
                continue;
            }
        }
        if let Some(value) = argument.strip_prefix("--color=")
            && !matches!(value, "always" | "auto" | "never")
        {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: option `color' expects \"always\", \"auto\", or \"never\"\n"
                    .to_owned(),
            });
        }
        if let Some(value) = argument.strip_prefix("--count=") {
            validate_refs_count_value(value)?;
        }
        index = index.saturating_add(1);
    }
    Ok(())
}

fn validate_refs_count_value(value: &str) -> Result<()> {
    if let Some(parsed) = parse_signed_refs_count(value) {
        if !(-2_147_483_648..=2_147_483_647).contains(&parsed) {
            return Err(CliError::Stderr {
                code: 129,
                text: format!(
                    "error: value {value} for option `count' not in range [-2147483648,2147483647]\n"
                ),
            });
        }
        if parsed >= 0 {
            return Ok(());
        }
        return Err(CliError::Stderr {
            code: 129,
            text: format!("error: invalid --count argument: `{parsed}'\n{REFS_LIST_USAGE}"),
        });
    }
    Err(CliError::Stderr {
        code: 129,
        text: "error: option `count' expects an integer value with an optional k/m/g suffix\n"
            .to_owned(),
    })
}

fn parse_signed_refs_count(value: &str) -> Option<i128> {
    let (negative, unsigned) = match value.as_bytes().first().copied() {
        Some(b'-') => (true, &value[1..]),
        Some(b'+') => (false, &value[1..]),
        _ => (false, value),
    };
    let (digits, multiplier) = match unsigned.as_bytes().last().copied() {
        Some(b'k') => (&unsigned[..unsigned.len().saturating_sub(1)], 1024_i128),
        Some(b'm') => (
            &unsigned[..unsigned.len().saturating_sub(1)],
            1024_i128.pow(2),
        ),
        Some(b'g') => (
            &unsigned[..unsigned.len().saturating_sub(1)],
            1024_i128.pow(3),
        ),
        _ => (unsigned, 1_i128),
    };
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let magnitude = digits.parse::<i128>().ok()?.checked_mul(multiplier)?;
    Some(if negative { -magnitude } else { magnitude })
}

fn normalize_empty_init_template(args: Vec<String>) -> Vec<String> {
    if args.first().map(String::as_str) != Some("init") {
        return args;
    }
    args.into_iter()
        .map(|arg| {
            if arg == "--template=" {
                format!("--template={EMPTY_TEMPLATE_SENTINEL}")
            } else {
                arg
            }
        })
        .collect()
}

fn normalize_empty_clone_template(args: Vec<String>) -> Vec<String> {
    if args.first().map(String::as_str) != Some("clone") {
        return args;
    }
    args.into_iter()
        .map(|arg| {
            if arg == "--template=" {
                format!("--template={EMPTY_TEMPLATE_SENTINEL}")
            } else {
                arg
            }
        })
        .collect()
}

fn normalize_history_count_shorthand(args: Vec<String>) -> Vec<String> {
    let Some(command) = args.first().map(String::as_str) else {
        return args;
    };
    if !matches!(command, "log" | "whatchanged" | "rev-list" | "format-patch") {
        return args;
    }
    let mut normalized = Vec::with_capacity(args.len());
    let mut after_separator = false;
    for arg in args {
        if arg == "--" {
            after_separator = true;
            normalized.push(arg);
            continue;
        }
        if !after_separator {
            if let Some(value) = arg.strip_prefix('-').filter(|value| {
                !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
            }) {
                normalized.push(format!("--max-count={value}"));
                continue;
            }
        }
        normalized.push(arg);
    }
    normalized
}

fn normalize_history_no_walk_value(args: Vec<String>) -> Vec<String> {
    let Some(command) = args.first().map(String::as_str) else {
        return args;
    };
    if !matches!(command, "log" | "whatchanged" | "rev-list" | "shortlog") {
        return args;
    }
    args.into_iter()
        .map(|arg| match arg.as_str() {
            "--no-walk=sorted" | "--no-walk=unsorted" => "--no-walk".to_owned(),
            _ => arg,
        })
        .collect()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ShowInterspersedOptionKind {
    NoValue,
    OptionalValue,
    RequiredValue,
}

fn normalize_show_interspersed_options(args: Vec<String>) -> Vec<String> {
    if args.first().map(String::as_str) != Some("show") {
        return args;
    }

    let mut normalized = Vec::with_capacity(args.len());
    normalized.push(args[0].clone());

    let mut positionals = Vec::new();
    let mut after_separator = false;
    let mut index = 1;
    while index < args.len() {
        let arg = &args[index];
        if after_separator {
            positionals.push(arg.clone());
            index += 1;
            continue;
        }
        if arg == "--" {
            after_separator = true;
            positionals.push(arg.clone());
            index += 1;
            continue;
        }
        let Some(kind) = show_interspersed_option_kind(arg) else {
            positionals.push(arg.clone());
            index += 1;
            continue;
        };
        normalized.push(arg.clone());
        if show_option_consumes_next_value(kind, args.get(index + 1).map(String::as_str)) {
            normalized.push(args[index + 1].clone());
            index += 2;
        } else {
            index += 1;
        }
    }

    normalized.extend(positionals);
    normalized
}

fn show_interspersed_option_kind(arg: &str) -> Option<ShowInterspersedOptionKind> {
    if let Some(flag) = arg.strip_prefix("--") {
        let (flag, has_inline_value) = flag
            .split_once('=')
            .map(|(name, _)| (name, true))
            .unwrap_or((flag, false));
        return match flag {
            "format" | "pretty" | "max-count" | "encoding" | "diff-merges" => {
                Some(if has_inline_value {
                    ShowInterspersedOptionKind::NoValue
                } else {
                    ShowInterspersedOptionKind::RequiredValue
                })
            }
            "abbrev" | "find-renames" | "find-copies" | "no-walk" => Some(if has_inline_value {
                ShowInterspersedOptionKind::NoValue
            } else {
                ShowInterspersedOptionKind::OptionalValue
            }),
            "no-patch"
            | "patch"
            | "oneline"
            | "stat"
            | "patch-with-raw"
            | "patch-with-stat"
            | "numstat"
            | "shortstat"
            | "raw"
            | "summary"
            | "name-only"
            | "name-status"
            | "find-copies-harder"
            | "expand-tabs"
            | "no-expand-tabs"
            | "notes"
            | "no-notes"
            | "show-notes"
            | "show-notes-by-default"
            | "standard-notes"
            | "no-standard-notes"
            | "show-signature"
            | "no-abbrev"
            | "abbrev-commit"
            | "no-abbrev-commit"
            | "root"
            | "first-parent"
            | "no-diff-merges"
            | "do-walk"
            | "stdin" => Some(ShowInterspersedOptionKind::NoValue),
            _ => None,
        };
    }

    match arg {
        "-s" | "-p" | "-z" | "-c" | "-m" => Some(ShowInterspersedOptionKind::NoValue),
        "-n" => Some(ShowInterspersedOptionKind::RequiredValue),
        _ if arg.starts_with("-n") => Some(ShowInterspersedOptionKind::NoValue),
        "-M" | "-C" => Some(ShowInterspersedOptionKind::OptionalValue),
        _ if arg.starts_with("-M") || arg.starts_with("-C") => {
            Some(ShowInterspersedOptionKind::NoValue)
        }
        _ => None,
    }
}

fn show_option_consumes_next_value(kind: ShowInterspersedOptionKind, next: Option<&str>) -> bool {
    match kind {
        ShowInterspersedOptionKind::NoValue => false,
        ShowInterspersedOptionKind::RequiredValue => next.is_some(),
        ShowInterspersedOptionKind::OptionalValue => {
            next.is_some_and(|value| !value.starts_with('-'))
        }
    }
}

fn normalize_log_date_hyphen_value(args: Vec<String>) -> Vec<String> {
    if args.first().map(String::as_str) != Some("log") {
        return args;
    }
    let mut normalized = Vec::with_capacity(args.len());
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if arg == "--date"
            && let Some(value) = args.get(index + 1)
            && value.starts_with('-')
        {
            normalized.push(format!("--date={value}"));
            index += 2;
            continue;
        }
        normalized.push(arg.clone());
        index += 1;
    }
    normalized
}

fn apply_command_alias(command_args: Vec<String>) -> Result<Vec<String>> {
    apply_command_alias_inner(
        command_args,
        &mut Vec::new(),
        &mut Trace2AliasContext::default(),
    )
}

fn apply_alias_leading_config_options(command_args: Vec<String>) -> Result<Vec<String>> {
    let mut entries = Vec::new();
    let mut index = 0usize;
    while let Some(arg) = command_args.get(index) {
        if arg == "-c" {
            let Some(config) = command_args.get(index + 1) else {
                return Err(CliError::Stderr {
                    code: 129,
                    text: "-c expects a configuration string\n".into(),
                });
            };
            entries.push(parse_global_config_entry(config)?);
            index += 2;
        } else if let Some(config) = arg.strip_prefix("--config-env=") {
            entries.push(parse_global_config_env_entry(config)?);
            index += 1;
        } else if arg == "--config-env" {
            let Some(config) = command_args.get(index + 1) else {
                return Err(CliError::Stderr {
                    code: 129,
                    text: "no config key given for --config-env\n".into(),
                });
            };
            entries.push(parse_global_config_env_entry(config)?);
            index += 2;
        } else {
            break;
        }
    }
    if entries.is_empty() {
        return Ok(command_args);
    }
    set_global_config_entries(entries);
    Ok(command_args.into_iter().skip(index).collect())
}

#[derive(Default)]
struct Trace2AliasContext {
    dashed_hierarchy: Vec<String>,
}

fn apply_command_alias_inner(
    command_args: Vec<String>,
    seen: &mut Vec<String>,
    trace2_context: &mut Trace2AliasContext,
) -> Result<Vec<String>> {
    let Some(command) = command_args.first().map(String::as_str) else {
        return Ok(command_args);
    };
    if command_uses_dashed_trace2(command) || !is_known_command(command) {
        trace_dashed_command_lookup_if_needed(command, &command_args[1..])?;
        trace2_context
            .dashed_hierarchy
            .push("_run_dashed_".to_owned());
        queue_trace2_dashed_lookup(command, &trace2_context.dashed_hierarchy);
    }
    // Built-in commands take precedence over aliases, so skip config alias
    // resolution for known commands and avoid parse-time config I/O on the
    // common path.
    if is_known_command(command) && !is_deprecated_alias_shadowable_command(command) {
        return Ok(command_args);
    }
    let Some(alias) = read_alias_value(command)? else {
        return Ok(command_args);
    };
    if let Some(position) = seen.iter().position(|item| item == command) {
        let cycle = seen[position..]
            .iter()
            .cloned()
            .chain(std::iter::once(command.to_owned()))
            .collect::<Vec<_>>();
        let mut text = String::new();
        for pair in cycle.windows(2) {
            text.push_str(&format!("'{}' is aliased to '{}'\n", pair[0], pair[1]));
        }
        text.push_str(&format!(
            "fatal: alias loop detected: expansion of '{command}' does not terminate:\n"
        ));
        for (index, name) in cycle[..cycle.len().saturating_sub(1)].iter().enumerate() {
            let marker = if index == 0 {
                " <=="
            } else if index + 1 == cycle.len() - 1 {
                " ==>"
            } else {
                ""
            };
            text.push_str(&format!("  {name}{marker}\n"));
        }
        return Err(CliError::Stderr { code: 128, text });
    }
    seen.push(command.to_owned());
    if let Some(shell_command) = alias.strip_prefix('!') {
        let mut process = std::process::Command::new(git_shell_command_path());
        let prepared = shell_alias_prepared_command(shell_command);
        trace_shell_alias_if_needed(shell_command, &command_args[1..], &prepared)?;
        queue_trace2_shell_alias(command, shell_command, trace2_context);
        process.arg("-c").arg(prepared).arg(shell_command);
        process.args(command_args.iter().skip(1));
        if let Ok(repo) = find_repo_or_bare() {
            process.current_dir(&repo.root);
            if let Ok(prefix) = shell_alias_git_prefix(&repo) {
                process.env("GIT_PREFIX", prefix);
            }
        }
        propagate_command_config_to_alias(&mut process);
        let status = process.status().map_err(CliError::Io)?;
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;

            if let Some(signal) = status.signal() {
                let result = Err(CliError::Stderr {
                    code: 128 + signal,
                    text: format!("error: {shell_command} died of signal {signal}\n"),
                });
                crate::runtime::run_parse_time_trace2_session(&command_args, &result);
                return Err(match result {
                    Err(error) => error,
                    Ok(()) => unreachable!(),
                });
            }
        }
        let result = Err(CliError::Exit(status.code().unwrap_or(1)));
        crate::runtime::run_parse_time_trace2_session(&command_args, &result);
        return Err(match result {
            Err(error) => error,
            Ok(()) => unreachable!(),
        });
    }
    let mut expanded = split_alias_words(&alias)?;
    if expanded.is_empty() {
        return Ok(command_args);
    }
    queue_trace2_git_alias(command, &expanded, trace2_context);
    expanded.extend(command_args.into_iter().skip(1));
    apply_command_alias_inner(expanded, seen, trace2_context)
}

fn propagate_command_config_to_alias(process: &mut std::process::Command) {
    let entries = global_config_entries();
    process.env_remove("GIT_CONFIG_PARAMETERS");
    let inherited_count = std::env::var("GIT_CONFIG_COUNT")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or_default();
    process.env_remove("GIT_CONFIG_COUNT");
    for index in 0..inherited_count {
        process.env_remove(format!("GIT_CONFIG_KEY_{index}"));
        process.env_remove(format!("GIT_CONFIG_VALUE_{index}"));
    }
    if entries.is_empty() {
        return;
    }
    process.env("GIT_CONFIG_COUNT", entries.len().to_string());
    for (index, entry) in entries.into_iter().enumerate() {
        process.env(format!("GIT_CONFIG_KEY_{index}"), entry.name());
        process.env(format!("GIT_CONFIG_VALUE_{index}"), entry.value);
    }
}

fn root_version_invocation(args: &[String]) -> Option<bool> {
    match args {
        [arg] if matches!(arg.as_str(), "--version" | "-v") => Some(false),
        [arg, build_options]
            if matches!(arg.as_str(), "--version" | "-v") && build_options == "--build-options" =>
        {
            Some(true)
        }
        _ => None,
    }
}

fn validate_version_invocation_before_clap(args: &[String]) -> Result<()> {
    if matches!(args, [command, option] if command == "version" && option == "--version") {
        return Err(CliError::Stderr {
            code: 129,
            text: "error: unknown option `version'\nusage: git version [--build-options]\n\n    --[no-]build-options  also print build options\n\n".into(),
        });
    }
    Ok(())
}

fn validate_var_invocation_before_clap(args: &[String]) -> Result<()> {
    if args.first().map(String::as_str) != Some("var") {
        return Ok(());
    }
    let invalid_list_usage = matches!(args, [_, option] if matches!(option.as_str(), "--list" | "--no-list" | "--nofork" | "-i"))
        || matches!(args, [_, option] if option == "-l=true" || option == "-l=" || option == "-ll")
        || matches!(args, [_, first, second] if first == "-l" && second == "-l");
    if invalid_list_usage {
        return Err(CliError::Stderr {
            code: 129,
            text: "usage: git var (-l | <variable>)\n".into(),
        });
    }
    Ok(())
}

fn validate_refs_invocation_before_clap(args: &[String]) -> Result<()> {
    let [command, subcommand, ..] = args else {
        return Ok(());
    };
    if command != "refs" || matches!(subcommand.as_str(), "migrate" | "verify") {
        return Ok(());
    }
    Err(CliError::Stderr {
        code: 129,
        text: format!("error: unknown subcommand: `{subcommand}'\n{REFS_USAGE}"),
    })
}

fn validate_check_attr_invocation_before_clap(args: &[String]) -> Result<()> {
    if args.first().map(String::as_str) != Some("check-attr") {
        return Ok(());
    }
    let mut index = 1usize;
    while index < args.len() {
        let arg = &args[index];
        if arg == "--" {
            break;
        }
        if arg == "--source" && args.get(index + 1).is_none() {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: option `source' requires a value\n".into(),
            });
        }
        index += 1;
    }
    Ok(())
}

fn validate_check_mailmap_invocation_before_clap(args: &[String]) -> Result<()> {
    if args.first().map(String::as_str) != Some("check-mailmap") {
        return Ok(());
    }
    for arg in args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        if arg.starts_with("--stdin=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: option `stdin' takes no value\n".into(),
            });
        }
    }
    if matches!(args, [_, option] if option == "--no-stdin") {
        return Err(CliError::Fatal {
            code: 128,
            message: "no contacts specified".into(),
        });
    }
    Ok(())
}

fn validate_help_invocation_before_clap(
    raw_args: &[String],
    command_args: &[String],
) -> Result<()> {
    if let Some(code) = try_emit_builtin_help_before_clap(command_args)? {
        return Err(CliError::Exit(code));
    }
    if matches!(command_args, [command, topic] if command == "help" && topic == "unknown") {
        return Err(CliError::Stderr {
            code: 1,
            text: "No manual entry for gitunknown\n".into(),
        });
    }
    let _ = raw_args;
    Ok(())
}

fn try_emit_builtin_help_before_clap(command_args: &[String]) -> Result<Option<i32>> {
    let Some(command) = command_args.first().map(String::as_str) else {
        return Ok(None);
    };
    if matches!(command_args, [command, help] if command == "ls-tree" && is_help_flag(help)) {
        crate::cli::commands::reference_commands::validate_ls_tree_config_before_help()?;
    }
    if let Some((usage, code)) = specialized_builtin_help_surface(command_args) {
        if command == "diff" {
            io::stderr()
                .lock()
                .write_all(usage.as_bytes())
                .map_err(CliError::Io)?;
        } else {
            io::stdout()
                .lock()
                .write_all(usage.as_bytes())
                .map_err(CliError::Io)?;
        }
        return Ok(Some(code));
    }
    if !command_args
        .iter()
        .skip(1)
        .take_while(|arg| arg.as_str() != "--")
        .any(|arg| arg == "-h" || arg == "--help")
    {
        return Ok(None);
    }
    let usage = match command {
        "add" => Some(ADD_USAGE),
        "restore" => Some(RESTORE_USAGE),
        "rm" => Some(RM_USAGE),
        "branch" if command_args.iter().skip(1).any(|arg| arg == "--help") => None,
        "branch" => Some(BRANCH_USAGE),
        "status" => Some(STATUS_USAGE),
        "worktree" => Some(WORKTREE_USAGE),
        "write-tree" => Some(WRITE_TREE_USAGE),
        _ => None,
    };
    let Some(usage) = usage else {
        return Ok(None);
    };
    io::stdout()
        .lock()
        .write_all(usage.as_bytes())
        .map_err(CliError::Io)?;
    Ok(Some(129))
}

fn specialized_builtin_help_surface(command_args: &[String]) -> Option<(&'static str, i32)> {
    match command_args {
        [command, help] if command == "sparse-checkout" && is_help_flag(help) => {
            Some((SPARSE_CHECKOUT_USAGE, 129))
        }
        [command, help] if command == "stage" && is_help_flag(help) => Some((ADD_USAGE, 129)),
        [command, help] if command == "stripspace" && is_help_flag(help) => {
            Some((STRIPSPACE_USAGE, 129))
        }
        [command, help] if command == "submodule--helper" && is_help_flag(help) => {
            Some((SUBMODULE_HELPER_USAGE, 129))
        }
        [command, help] if command == "switch" && is_help_flag(help) => Some((SWITCH_USAGE, 129)),
        [command, help] if command == "symbolic-ref" && is_help_flag(help) => {
            Some((SYMBOLIC_REF_USAGE, 129))
        }
        [command, help] if command == "show-branch" && is_help_flag(help) => {
            Some((SHOW_BRANCH_USAGE, 129))
        }
        [command, help] if command == "show-index" && is_help_flag(help) => {
            Some((SHOW_INDEX_USAGE, 129))
        }
        [command, help] if command == "show-ref" && is_help_flag(help) => {
            Some((SHOW_REF_USAGE, 129))
        }
        [command, help] if command == "send-pack" && is_help_flag(help) => {
            Some((SEND_PACK_USAGE, 129))
        }
        [command, help] if command == "rev-list" && is_help_flag(help) => {
            Some((REV_LIST_USAGE, 129))
        }
        [command, help] if command == "rev-parse" && is_help_flag(help) => {
            Some((REV_PARSE_USAGE, 129))
        }
        [command, help] if command == "revert" && is_help_flag(help) => Some((REVERT_USAGE, 129)),
        [command, help] if command == "replay" && is_help_flag(help) => Some((REPLAY_USAGE, 129)),
        [command, help] if command == "rerere" && is_help_flag(help) => Some((RERERE_USAGE, 129)),
        [command, help] if command == "reset" && is_help_flag(help) => Some((RESET_USAGE, 129)),
        [command, help] if command == "remote-fd" && is_help_flag(help) => {
            Some((REMOTE_FD_USAGE, 129))
        }
        [command, help] if command == "refs" && is_help_flag(help) => Some((REFS_USAGE, 129)),
        [command, help] if command == "remote" && is_help_flag(help) => Some((REMOTE_USAGE, 129)),
        [command, help] if command == "remote-ext" && is_help_flag(help) => {
            Some((REMOTE_EXT_USAGE, 129))
        }
        [command, help] if command == "rebase" && is_help_flag(help) => Some((REBASE_USAGE, 129)),
        [command, help] if command == "receive-pack" && is_help_flag(help) => {
            Some((RECEIVE_PACK_USAGE, 129))
        }
        [command, help] if command == "reflog" && is_help_flag(help) => Some((REFLOG_USAGE, 129)),
        [command, help] if command == "push" && is_help_flag(help) => Some((PUSH_USAGE, 129)),
        [command, help] if command == "multi-pack-index" && is_help_flag(help) => {
            Some((MULTI_PACK_INDEX_USAGE, 129))
        }
        [command, help] if command == "merge-tree" && is_help_flag(help) => {
            Some((MERGE_TREE_USAGE, 129))
        }
        [command, help] if command == "merge-recursive-ours" && is_help_flag(help) => {
            Some((MERGE_RECURSIVE_OURS_USAGE, 129))
        }
        [command, help] if command == "merge-recursive-theirs" && is_help_flag(help) => {
            Some((MERGE_RECURSIVE_THEIRS_USAGE, 129))
        }
        [command, help] if command == "merge-index" && is_help_flag(help) => {
            Some((MERGE_INDEX_USAGE, 129))
        }
        [command, help] if command == "merge-ours" && is_help_flag(help) => {
            Some((MERGE_OURS_USAGE, 129))
        }
        [command, help] if command == "merge-recursive" && is_help_flag(help) => {
            Some((MERGE_RECURSIVE_USAGE, 129))
        }
        [command, help] if command == "merge" && is_help_flag(help) => Some((MERGE_USAGE, 129)),
        [command, help] if command == "merge-base" && is_help_flag(help) => {
            Some((MERGE_BASE_USAGE, 129))
        }
        [command, help] if command == "merge-file" && is_help_flag(help) => {
            Some((MERGE_FILE_USAGE, 129))
        }
        [command, help] if command == "mailinfo" && is_help_flag(help) => {
            Some((MAILINFO_USAGE, 129))
        }
        [command, help] if command == "mailsplit" && is_help_flag(help) => {
            Some((MAILSPLIT_USAGE, 129))
        }
        [command, help] if command == "maintenance" && is_help_flag(help) => {
            Some((MAINTENANCE_USAGE, 129))
        }
        [command, help] if command == "ls-files" && is_help_flag(help) => {
            Some((LS_FILES_USAGE, 129))
        }
        [command, help] if command == "ls-remote" && is_help_flag(help) => {
            Some((LS_REMOTE_USAGE, 129))
        }
        [command, help] if command == "ls-tree" && is_help_flag(help) => Some((LS_TREE_USAGE, 129)),
        [command, help] if command == "init-db" && is_help_flag(help) => Some((INIT_DB_USAGE, 129)),
        [command, help] if command == "hook" && is_help_flag(help) => Some((HOOK_USAGE, 129)),
        [command, help] if command == "index-pack" && is_help_flag(help) => {
            Some((INDEX_PACK_USAGE, 129))
        }
        [command, help] if command == "init" && is_help_flag(help) => Some((INIT_DB_USAGE, 129)),
        [command, help] if command == "interpret-trailers" && is_help_flag(help) => {
            Some((INTERPRET_TRAILERS_USAGE, 129))
        }
        [command, help] if command == "grep" && is_help_flag(help) => Some((GREP_USAGE, 129)),
        [command, help] if command == "hash-object" && is_help_flag(help) => {
            Some((HASH_OBJECT_USAGE, 129))
        }
        [command, help] if command == "help" && is_help_flag(help) => Some((HELP_USAGE, 129)),
        [command, help] if command == "clean" && is_help_flag(help) => Some((CLEAN_USAGE, 129)),
        [command, help] if command == "am" && is_help_flag(help) => Some((AM_USAGE, 129)),
        [command, help] if command == "apply" && is_help_flag(help) => Some((APPLY_USAGE, 129)),
        [command, help] if command == "archive" && is_help_flag(help) => Some((ARCHIVE_USAGE, 129)),
        [command, help] if command == "backfill" && is_help_flag(help) => {
            Some((BACKFILL_USAGE, 129))
        }
        [command, help] if command == "bugreport" && is_help_flag(help) => {
            Some((BUGREPORT_USAGE, 129))
        }
        [command, help] if command == "bundle" && is_help_flag(help) => Some((BUNDLE_USAGE, 129)),
        [command, help] if command == "cat-file" && is_help_flag(help) => {
            Some((CAT_FILE_USAGE, 129))
        }
        [command, help] if command == "check-ignore" && is_help_flag(help) => {
            Some((CHECK_IGNORE_USAGE, 129))
        }
        [command, help] if command == "check-mailmap" && is_help_flag(help) => {
            Some((CHECK_MAILMAP_USAGE, 129))
        }
        [command, help] if command == "check-attr" && is_help_flag(help) => {
            Some((CHECK_ATTR_USAGE, 129))
        }
        [command, help] if command == "check-ref-format" && is_help_flag(help) => {
            Some((CHECK_REF_FORMAT_USAGE, 129))
        }
        [command, help] if command == "checkout" && is_help_flag(help) => {
            Some((CHECKOUT_USAGE, 129))
        }
        [command, help] if command == "checkout--worker" && is_help_flag(help) => {
            Some((CHECKOUT_WORKER_USAGE, 129))
        }
        [command, help] if command == "checkout-index" && is_help_flag(help) => {
            Some((CHECKOUT_INDEX_USAGE, 129))
        }
        [command, help] if command == "cherry" && is_help_flag(help) => Some((CHERRY_USAGE, 129)),
        [command, help] if command == "cherry-pick" && is_help_flag(help) => {
            Some((CHERRY_PICK_USAGE, 129))
        }
        [command, help] if command == "clone" && is_help_flag(help) => Some((CLONE_USAGE, 129)),
        [command, help] if command == "column" && is_help_flag(help) => Some((COLUMN_USAGE, 129)),
        [command, help] if command == "commit" && is_help_flag(help) => Some((COMMIT_USAGE, 129)),
        [command, help] if command == "commit-graph" && is_help_flag(help) => {
            Some((COMMIT_GRAPH_USAGE, 129))
        }
        [command, help] if command == "commit-tree" && is_help_flag(help) => {
            Some((COMMIT_TREE_USAGE, 129))
        }
        [command, help] if command == "config" && is_help_flag(help) => Some((CONFIG_USAGE, 129)),
        [command, help] if command == "count-objects" && is_help_flag(help) => {
            Some((COUNT_OBJECTS_USAGE, 129))
        }
        [command, help] if command == "credential" && is_help_flag(help) => {
            Some((CREDENTIAL_USAGE, 129))
        }
        [command, help] if command == "credential-cache" && is_help_flag(help) => {
            Some((CREDENTIAL_CACHE_USAGE, 129))
        }
        [command, help] if command == "credential-cache--daemon" && is_help_flag(help) => {
            Some((CREDENTIAL_CACHE_DAEMON_USAGE, 129))
        }
        [command, help] if command == "credential-store" && is_help_flag(help) => {
            Some((CREDENTIAL_STORE_USAGE, 129))
        }
        [command, help] if command == "describe" && is_help_flag(help) => {
            Some((DESCRIBE_USAGE, 129))
        }
        [command, help] if command == "diagnose" && is_help_flag(help) => {
            Some((DIAGNOSE_USAGE, 129))
        }
        [command, help] if command == "diff" && is_help_flag(help) => Some((DIFF_USAGE, 129)),
        [command, help] if command == "diff-files" && is_help_flag(help) => {
            Some((DIFF_FILES_USAGE, 129))
        }
        [command, help] if command == "diff-index" && is_help_flag(help) => {
            Some((DIFF_INDEX_USAGE, 129))
        }
        [command, help] if command == "diff-pairs" && is_help_flag(help) => {
            Some((DIFF_PAIRS_USAGE, 129))
        }
        [command, help] if command == "diff-tree" && is_help_flag(help) => {
            Some((DIFF_TREE_USAGE, 129))
        }
        [command, help] if command == "difftool" && is_help_flag(help) => {
            Some((DIFFTOOL_USAGE, 129))
        }
        [command, help] if command == "fast-import" && is_help_flag(help) => {
            Some((FAST_IMPORT_USAGE, 129))
        }
        [command, help] if command == "fast-export" && is_help_flag(help) => {
            Some((FAST_EXPORT_USAGE, 129))
        }
        [command, help] if command == "fetch" && is_help_flag(help) => Some((FETCH_USAGE, 129)),
        [command, help] if command == "fetch-pack" && is_help_flag(help) => {
            Some((FETCH_PACK_USAGE, 129))
        }
        [command, help] if command == "fsmonitor--daemon" && is_help_flag(help) => {
            Some((FSMONITOR_DAEMON_USAGE, 129))
        }
        [command, help] if command == "format-patch" && is_help_flag(help) => {
            Some((FORMAT_PATCH_USAGE, 129))
        }
        [command, help] if command == "fmt-merge-msg" && is_help_flag(help) => {
            Some((FMT_MERGE_MSG_USAGE, 129))
        }
        [command, help] if command == "for-each-ref" && is_help_flag(help) => {
            Some((FOR_EACH_REF_USAGE, 129))
        }
        [command, help] if command == "for-each-repo" && is_help_flag(help) => {
            Some((FOR_EACH_REPO_USAGE, 129))
        }
        [command, help] if command == "gc" && is_help_flag(help) => Some((GC_USAGE, 129)),
        [command, help] if command == "get-tar-commit-id" && is_help_flag(help) => {
            Some((GET_TAR_COMMIT_ID_USAGE, 129))
        }
        [command, help] if command == "fsck" && is_help_flag(help) => Some((FSCK_USAGE, 129)),
        [command, help] if command == "fsck-objects" && is_help_flag(help) => {
            Some((FSCK_USAGE, 129))
        }
        [command, help] if command == "log" && is_help_flag(help) => Some((LOG_USAGE, 129)),
        [command, help] if command == "merge-subtree" && is_help_flag(help) => {
            Some((MERGE_SUBTREE_USAGE, 129))
        }
        [command, help] if command == "mktag" && is_help_flag(help) => Some((MKTAG_USAGE, 129)),
        [command, help] if command == "mktree" && is_help_flag(help) => Some((MKTREE_USAGE, 129)),
        [command, help] if command == "mv" && is_help_flag(help) => Some((MV_USAGE, 129)),
        [command, help] if command == "name-rev" && is_help_flag(help) => {
            Some((NAME_REV_USAGE, 129))
        }
        [command, help] if command == "prune" && is_help_flag(help) => Some((PRUNE_USAGE, 129)),
        [command, help] if command == "prune-packed" && is_help_flag(help) => {
            Some((PRUNE_PACKED_USAGE, 129))
        }
        [command, help] if command == "pull" && is_help_flag(help) => Some((PULL_USAGE, 129)),
        [command, help] if command == "notes" && is_help_flag(help) => Some((NOTES_USAGE, 129)),
        [command, help] if command == "pack-refs" && is_help_flag(help) => {
            Some((PACK_REFS_USAGE, 129))
        }
        [command, help] if command == "pack-objects" && is_help_flag(help) => {
            Some((PACK_OBJECTS_USAGE, 129))
        }
        [command, help] if command == "pack-redundant" && is_help_flag(help) => {
            Some((PACK_REDUNDANT_USAGE, 129))
        }
        [command, help] if command == "patch-id" && is_help_flag(help) => {
            Some((PATCH_ID_USAGE, 129))
        }
        [command, help] if command == "pickaxe" && is_help_flag(help) => Some((PICKAXE_USAGE, 129)),
        [command, help] if command == "range-diff" && is_help_flag(help) => {
            Some((RANGE_DIFF_USAGE, 129))
        }
        [command, help] if command == "read-tree" && is_help_flag(help) => {
            Some((READ_TREE_USAGE, 129))
        }
        [command, help] if command == "repack" && is_help_flag(help) => Some((REPACK_USAGE, 129)),
        [command, help] if command == "replace" && is_help_flag(help) => Some((REPLACE_USAGE, 129)),
        [command, help] if command == "shortlog" && is_help_flag(help) => {
            Some((SHORTLOG_USAGE, 129))
        }
        [command, help] if command == "show" && is_help_flag(help) => {
            Some((WHATCHANGED_USAGE, 129))
        }
        [command, help] if command == "tag" && is_help_flag(help) => Some((TAG_USAGE, 129)),
        [command, help] if command == "unpack-file" && is_help_flag(help) => {
            Some((UNPACK_FILE_USAGE, 129))
        }
        [command, help] if command == "unpack-objects" && is_help_flag(help) => {
            Some((UNPACK_OBJECTS_USAGE, 129))
        }
        [command, help] if command == "update-index" && is_help_flag(help) => {
            Some((UPDATE_INDEX_USAGE, 129))
        }
        [command, help] if command == "update-ref" && is_help_flag(help) => {
            Some((UPDATE_REF_USAGE, 129))
        }
        [command, help] if command == "update-server-info" && is_help_flag(help) => {
            Some((UPDATE_SERVER_INFO_USAGE, 129))
        }
        [command, help] if command == "upload-archive" && is_help_flag(help) => {
            Some((UPLOAD_ARCHIVE_USAGE, 129))
        }
        [command, help] if command == "upload-archive--writer" && is_help_flag(help) => {
            Some((UPLOAD_ARCHIVE_USAGE, 129))
        }
        [command, help] if command == "upload-pack" && is_help_flag(help) => {
            Some((UPLOAD_PACK_USAGE, 129))
        }
        [command, help] if command == "var" && is_help_flag(help) => Some((VAR_USAGE, 129)),
        [command, help] if command == "version" && is_help_flag(help) => Some((VERSION_USAGE, 129)),
        [command, help] if command == "verify-commit" && is_help_flag(help) => {
            Some((VERIFY_COMMIT_USAGE, 129))
        }
        [command, help] if command == "verify-pack" && is_help_flag(help) => {
            Some((VERIFY_PACK_USAGE, 129))
        }
        [command, help] if command == "verify-tag" && is_help_flag(help) => {
            Some((VERIFY_TAG_USAGE, 129))
        }
        [command, help] if command == "whatchanged" && is_help_flag(help) => {
            Some((WHATCHANGED_USAGE, 129))
        }
        [command, help] if command == "submodule" && is_help_flag(help) => {
            Some((SUBMODULE_USAGE, 0))
        }
        [command, help] if command == "stash" && is_help_flag(help) => {
            Some((STASH_TOP_LEVEL_USAGE, 129))
        }
        [command, subcommand, help]
            if command == "stash" && subcommand == "list" && is_help_flag(help) =>
        {
            Some((STASH_LIST_USAGE, 129))
        }
        [command, subcommand, help]
            if command == "stash" && subcommand == "push" && is_help_flag(help) =>
        {
            Some((STASH_PUSH_USAGE, 129))
        }
        [command, subcommand, help]
            if command == "stash" && subcommand == "apply" && is_help_flag(help) =>
        {
            Some((STASH_APPLY_USAGE, 129))
        }
        [command, subcommand, help]
            if command == "stash" && subcommand == "pop" && is_help_flag(help) =>
        {
            Some((STASH_POP_USAGE, 129))
        }
        [command, subcommand, help]
            if command == "stash" && subcommand == "drop" && is_help_flag(help) =>
        {
            Some((STASH_DROP_USAGE, 129))
        }
        _ => None,
    }
}

fn is_help_flag(arg: &str) -> bool {
    arg == "-h" || arg == "--help"
}

const SPARSE_CHECKOUT_USAGE: &str = "usage: git sparse-checkout (init | list | set | add | reapply | disable | check-rules | clean) [<options>]\n";

const STRIPSPACE_USAGE: &str = "usage: git stripspace [-s | --strip-comments]\n   or: git stripspace [-c | --comment-lines]\n\n    -s, --strip-comments  skip and remove all lines starting with comment character\n    -c, --comment-lines   prepend comment character and space to each line\n\n";

const SUBMODULE_HELPER_USAGE: &str = "usage: git submodule--helper <command>\n";

const SWITCH_USAGE: &str = "usage: git switch [<options>] [<branch>]\n\n    -c, --[no-]create <branch>\n                          create and switch to a new branch\n    -C, --[no-]force-create <branch>\n                          create/reset and switch to a branch\n    --[no-]guess          second guess 'git switch <no-such-branch>'\n    --[no-]discard-changes\n                          throw away local modifications\n    -q, --[no-]quiet      suppress progress reporting\n    --[no-]recurse-submodules[=<checkout>]\n                          control recursive updating of submodules\n    --[no-]progress       force progress reporting\n    -m, --[no-]merge      perform a 3-way merge with the new branch\n    --[no-]conflict <style>\n                          conflict style (merge, diff3, or zdiff3)\n    -d, --[no-]detach     detach HEAD at named commit\n    -t, --[no-]track[=(direct|inherit)]\n                          set branch tracking configuration\n    -f, --[no-]force      force checkout (throw away local modifications)\n    --[no-]orphan <new-branch>\n                          new unborn branch\n    --[no-]overwrite-ignore\n                          update ignored files (default)\n    --[no-]ignore-other-worktrees\n                          do not check if another worktree is using this branch\n\n";

const SYMBOLIC_REF_USAGE: &str = "usage: git symbolic-ref [-m <reason>] <name> <ref>\n   or: git symbolic-ref [-q] [--short] [--no-recurse] <name>\n   or: git symbolic-ref --delete [-q] <name>\n\n    -q, --[no-]quiet      suppress error message for non-symbolic (detached) refs\n    -d, --[no-]delete     delete symbolic ref\n    --[no-]short          shorten ref output\n    --[no-]recurse        recursively dereference (default)\n    -m <reason>           reason of the update\n\n";

const SHOW_BRANCH_USAGE: &str = "usage: git show-branch [-a | --all] [-r | --remotes] [--topo-order | --date-order]\n                       [--current] [--color[=<when>] | --no-color] [--sparse]\n                       [--more=<n> | --list | --independent | --merge-base]\n                       [--no-name | --sha1-name] [--topics]\n                       [(<rev> | <glob>)...]\n   or: git show-branch (-g | --reflog)[=<n>[,<base>]] [--list] [<ref>]\n\n    -a, --[no-]all        show remote-tracking and local branches\n    -r, --[no-]remotes    show remote-tracking branches\n    --[no-]color[=<when>] color '*!+-' corresponding to the branch\n    --[no-]more[=<n>]     show <n> more commits after the common ancestor\n    --[no-]list           synonym to more=-1\n    --no-name             suppress naming strings\n    --name                opposite of --no-name\n    --[no-]current        include the current branch\n    --[no-]sha1-name      name commits with their object names\n    --[no-]merge-base     show possible merge bases\n    --[no-]independent    show refs unreachable from any other ref\n    --topo-order          show commits in topological order\n    --[no-]topics         show only commits not on the first branch\n    --[no-]sparse         show merges reachable from only one tip\n    --date-order          topologically sort, maintaining date order where possible\n    -g, --reflog[=<n>[,<base>]]\n                          show <n> most recent ref-log entries starting at base\n\n";

const SHOW_INDEX_USAGE: &str = "usage: git show-index [--object-format=<hash-algorithm>] < <pack-idx-file>\n\n    --[no-]object-format <hash-algorithm>\n                          specify the hash algorithm to use\n\n";

const SHOW_REF_USAGE: &str = "usage: git show-ref [--head] [-d | --dereference]\n                    [-s | --hash[=<n>]] [--abbrev[=<n>]] [--branches] [--tags]\n                    [--] [<pattern>...]\n   or: git show-ref --verify [-q | --quiet] [-d | --dereference]\n                    [-s | --hash[=<n>]] [--abbrev[=<n>]]\n                    [--] [<ref>...]\n   or: git show-ref --exclude-existing[=<pattern>]\n   or: git show-ref --exists <ref>\n\n    --[no-]tags           only show tags (can be combined with --branches)\n    --[no-]branches       only show branches (can be combined with --tags)\n    --[no-]exists         check for reference existence without resolving\n    --[no-]verify         stricter reference checking, requires exact ref path\n    --[no-]head           show the HEAD reference, even if it would be filtered out\n    -d, --[no-]dereference\n                          dereference tags into object IDs\n    -s, --[no-]hash[=<n>] only show SHA1 hash using <n> digits\n    --[no-]abbrev[=<n>]   use <n> digits to display object names\n    -q, --[no-]quiet      do not print results to stdout (useful with --verify)\n    --exclude-existing[=<pattern>]\n                          show refs from stdin that aren't in local repository\n\n";

const SEND_PACK_USAGE: &str = "usage: git send-pack [--mirror] [--dry-run] [--force]\n                     [--receive-pack=<git-receive-pack>]\n                     [--verbose] [--thin] [--atomic]\n                     [--[no-]signed | --signed=(true|false|if-asked)]\n                     [<host>:]<directory> (--all | <ref>...)\n\n    -v, --[no-]verbose    be more verbose\n    -q, --[no-]quiet      be more quiet\n    --[no-]receive-pack <receive-pack>\n                          receive pack program\n    --[no-]exec <receive-pack>\n                          receive pack program\n    --[no-]remote <remote>\n                          remote name\n    --[no-]all            push all refs\n    -n, --[no-]dry-run    dry run\n    --[no-]mirror         mirror all refs\n    -f, --[no-]force      force updates\n    --[no-]signed[=(yes|no|if-asked)]\n                          GPG sign the push\n    --[no-]push-option <server-specific>\n                          option to transmit\n    --[no-]progress       force progress reporting\n    --[no-]thin           use thin pack\n    --[no-]atomic         request atomic transaction on remote side\n    --[no-]stateless-rpc  use stateless RPC protocol\n    --[no-]stdin          read refs from stdin\n    --[no-]helper-status  print status from remote helper\n    --[no-]force-with-lease[=<refname>:<expect>]\n                          require old value of ref to be at this value\n    --[no-]force-if-includes\n                          require remote updates to be integrated locally\n\n";

const REV_LIST_USAGE: &str = "usage: git rev-list [<options>] <commit>... [--] [<path>...]\n\n  limiting output:\n    --max-count=<n>\n    --max-age=<epoch>\n    --min-age=<epoch>\n    --sparse\n    --no-merges\n    --min-parents=<n>\n    --no-min-parents\n    --max-parents=<n>\n    --no-max-parents\n    --remove-empty\n    --all\n    --branches\n    --tags\n    --remotes\n    --stdin\n    --exclude-hidden=[fetch|receive|uploadpack]\n    --quiet\n  ordering output:\n    --topo-order\n    --date-order\n    --reverse\n  formatting output:\n    --parents\n    --children\n    --objects | --objects-edge\n    --disk-usage[=human]\n    --unpacked\n    --header | --pretty\n    --[no-]object-names\n    --abbrev=<n> | --no-abbrev\n    --abbrev-commit\n    --left-right\n    --count\n    -z\n  special purpose:\n    --bisect\n    --bisect-vars\n    --bisect-all\n";

const REV_PARSE_USAGE: &str = "usage: git rev-parse --parseopt [<options>] -- [<args>...]\n   or: git rev-parse --sq-quote [<arg>...]\n   or: git rev-parse [<options>] [<arg>...]\n\nRun \"git rev-parse --parseopt -h\" for more information on the first usage.\n";

const REVERT_USAGE: &str = "usage: git revert [--[no-]edit] [-n] [-m <parent-number>] [-s] [-S[<keyid>]] <commit>...\n   or: git revert (--continue | --skip | --abort | --quit)\n\n    --quit                end revert or cherry-pick sequence\n    --continue            resume revert or cherry-pick sequence\n    --abort               cancel revert or cherry-pick sequence\n    --skip                skip current commit and continue\n    --[no-]cleanup <mode> how to strip spaces and #comments from message\n    -n, --no-commit       don't automatically commit\n    --commit              opposite of --no-commit\n    -e, --[no-]edit       edit the commit message\n    -s, --[no-]signoff    add a Signed-off-by trailer\n    -m, --[no-]mainline <parent-number>\n                          select mainline parent\n    --[no-]rerere-autoupdate\n                          update the index with reused conflict resolution if possible\n    --[no-]strategy <strategy>\n                          merge strategy\n    -X, --[no-]strategy-option <option>\n                          option for merge strategy\n    -S, --[no-]gpg-sign[=<key-id>]\n                          GPG sign commit\n    --[no-]reference      use the 'reference' format to refer to commits\n\n";

const REPLAY_USAGE: &str = "usage: (EXPERIMENTAL!) git replay ([--contained] --onto <newbase> | --advance <branch>) <revision-range>...\n\n    --[no-]advance <branch>\n                          make replay advance given branch\n    --[no-]onto <revision>\n                          replay onto given commit\n    --[no-]contained      advance all branches contained in revision-range\n\n";

const RERERE_USAGE: &str = "usage: git rerere [clear | forget <pathspec>... | diff | status | remaining | gc]\n\n    --[no-]rerere-autoupdate\n                          register clean resolutions in index\n\n";

const RESET_USAGE: &str = "usage: git reset [--mixed | --soft | --hard | --merge | --keep] [-q] [<commit>]\n   or: git reset [-q] [<tree-ish>] [--] <pathspec>...\n   or: git reset [-q] [--pathspec-from-file [--pathspec-file-nul]] [<tree-ish>]\n   or: git reset --patch [<tree-ish>] [--] [<pathspec>...]\n\n    -q, --[no-]quiet      be quiet, only report errors\n    --no-refresh          skip refreshing the index after reset\n    --refresh             opposite of --no-refresh\n    --mixed               reset HEAD and index\n    --soft                reset only HEAD\n    --hard                reset HEAD, index and working tree\n    --merge               reset HEAD, index and working tree\n    --keep                reset HEAD but keep local changes\n    --[no-]recurse-submodules[=<reset>]\n                          control recursive updating of submodules\n    -p, --[no-]patch      select hunks interactively\n    -N, --[no-]intent-to-add\n                          record only the fact that removed paths will be added later\n    --[no-]pathspec-from-file <file>\n                          read pathspec from file\n    --[no-]pathspec-file-nul\n                          with --pathspec-from-file, pathspec elements are separated with NUL character\n\n";

const REMOTE_FD_USAGE: &str = "usage: git remote-fd <remote> <url>\n";

const REFS_USAGE: &str = "usage: git refs migrate --ref-format=<format> [--dry-run]\n   or: git refs verify [--strict] [--verbose]\n";

const REMOTE_USAGE: &str = "usage: git remote [-v | --verbose]\n   or: git remote add [-t <branch>] [-m <master>] [-f] [--tags | --no-tags] [--mirror=<fetch|push>] <name> <url>\n   or: git remote rename [--[no-]progress] <old> <new>\n   or: git remote remove <name>\n   or: git remote set-head <name> (-a | --auto | -d | --delete | <branch>)\n   or: git remote [-v | --verbose] show [-n] <name>\n   or: git remote prune [-n | --dry-run] <name>\n   or: git remote [-v | --verbose] update [-p | --prune] [(<group> | <remote>)...]\n   or: git remote set-branches [--add] <name> <branch>...\n   or: git remote get-url [--push] [--all] <name>\n   or: git remote set-url [--push] <name> <newurl> [<oldurl>]\n   or: git remote set-url --add <name> <newurl>\n   or: git remote set-url --delete <name> <url>\n\n    -v, --[no-]verbose    be verbose; must be placed before a subcommand\n\n";

const REMOTE_EXT_USAGE: &str = "usage: git remote-ext <remote> <url>\n";

const REBASE_USAGE: &str = "usage: git rebase [-i] [options] [--exec <cmd>] [--onto <newbase> | --keep-base] [<upstream> [<branch>]]\n   or: git rebase [-i] [options] [--exec <cmd>] [--onto <newbase>] --root [<branch>]\n   or: git rebase --continue | --abort | --skip | --edit-todo\n\n    --[no-]onto <revision>\n                          rebase onto given branch instead of upstream\n    --[no-]keep-base      use the merge-base of upstream and branch as the current base\n    --no-verify           allow pre-rebase hook to run\n    --verify              opposite of --no-verify\n    -q, --[no-]quiet      be quiet. implies --no-stat\n    -v, --[no-]verbose    display a diffstat of what changed upstream\n    -n, --no-stat         do not show diffstat of what changed upstream\n    --stat                opposite of --no-stat\n    --[no-]signoff        add a Signed-off-by trailer to each commit\n    --[no-]committer-date-is-author-date\n                          make committer date match author date\n    --[no-]reset-author-date\n                          ignore author date and use current date\n    -C <n>                passed to 'git apply'\n    --[no-]ignore-whitespace\n                          ignore changes in whitespace\n    --[no-]whitespace <action>\n                          passed to 'git apply'\n    -f, --[no-]force-rebase\n                          cherry-pick all commits, even if unchanged\n    --no-ff               cherry-pick all commits, even if unchanged\n    --ff                  opposite of --no-ff\n    --continue            continue\n    --skip                skip current patch and continue\n    --abort               abort and check out the original branch\n    --quit                abort but keep HEAD where it is\n    --edit-todo           edit the todo list during an interactive rebase\n    --show-current-patch  show the patch file being applied or merged\n    --apply               use apply strategies to rebase\n    -m, --merge           use merging strategies to rebase\n    -i, --interactive     let the user edit the list of commits to rebase\n    --[no-]rerere-autoupdate\n                          update the index with reused conflict resolution if possible\n    --empty (drop|keep|stop)\n                          how to handle commits that become empty\n    --[no-]autosquash     move commits that begin with squash!/fixup! under -i\n    --[no-]update-refs    update branches that point to commits that are being rebased\n    -S, --[no-]gpg-sign[=<key-id>]\n                          GPG-sign commits\n    --[no-]autostash      automatically stash/stash pop before and after\n    -x, --[no-]exec <exec>\n                          add exec lines after each commit of the editable list\n    -r, --[no-]rebase-merges[=<mode>]\n                          try to rebase merges instead of skipping them\n    --[no-]fork-point     use 'merge-base --fork-point' to refine upstream\n    -s, --[no-]strategy <strategy>\n                          use the given merge strategy\n    -X, --[no-]strategy-option <option>\n                          pass the argument through to the merge strategy\n    --[no-]root           rebase all reachable commits up to the root(s)\n    --[no-]reschedule-failed-exec\n                          automatically re-schedule any `exec` that fails\n    --[no-]reapply-cherry-picks\n                          apply all changes, even those already present upstream\n\n";

const RECEIVE_PACK_USAGE: &str =
    "usage: git receive-pack <git-dir>\n\n    -q, --[no-]quiet      quiet\n\n";

const REFLOG_USAGE: &str = "usage: git reflog [show] [<log-options>] [<ref>]\n   or: git reflog list\n   or: git reflog exists <ref>\n   or: git reflog write <ref> <old-oid> <new-oid> <message>\n   or: git reflog delete [--rewrite] [--updateref]\n\t[--dry-run | -n] [--verbose] <ref>@{<specifier>}...\n   or: git reflog drop [--all [--single-worktree] | <refs>...]\n   or: git reflog expire [--expire=<time>] [--expire-unreachable=<time>]\n\t[--rewrite] [--updateref] [--stale-fix]\n\t[--dry-run | -n] [--verbose] [--all [--single-worktree] | <refs>...]\n\n";

const PUSH_USAGE: &str = "usage: git push [<options>] [<repository> [<refspec>...]]\n\n    -v, --[no-]verbose    be more verbose\n    -q, --[no-]quiet      be more quiet\n    --[no-]repo <repository>\n                          repository\n    --[no-]all            push all branches\n    --[no-]branches       alias of --all\n    --[no-]mirror         mirror all refs\n    -d, --[no-]delete     delete refs\n    --[no-]tags           push tags (can't be used with --all or --branches or --mirror)\n    -n, --[no-]dry-run    dry run\n    --[no-]porcelain      machine-readable output\n    -f, --[no-]force      force updates\n    --[no-]force-with-lease[=<refname>:<expect>]\n                          require old value of ref to be at this value\n    --[no-]force-if-includes\n                          require remote updates to be integrated locally\n    --[no-]recurse-submodules (check|on-demand|no)\n                          control recursive pushing of submodules\n    --[no-]thin           use thin pack\n    --[no-]receive-pack <receive-pack>\n                          receive pack program\n    --[no-]exec <receive-pack>\n                          receive pack program\n    -u, --[no-]set-upstream\n                          set upstream for git pull/status\n    --[no-]progress       force progress reporting\n    --[no-]prune          prune locally removed refs\n    --no-verify           bypass pre-push hook\n    --verify              opposite of --no-verify\n    --[no-]follow-tags    push missing but relevant tags\n    --[no-]signed[=(yes|no|if-asked)]\n                          GPG sign the push\n    --[no-]atomic         request atomic transaction on remote side\n    -o, --[no-]push-option <server-specific>\n                          option to transmit\n    -4, --ipv4            use IPv4 addresses only\n    -6, --ipv6            use IPv6 addresses only\n\n";

const MULTI_PACK_INDEX_USAGE: &str = "usage: git multi-pack-index [<options>] write [--preferred-pack=<pack>][--refs-snapshot=<path>]\n   or: git multi-pack-index [<options>] verify\n   or: git multi-pack-index [<options>] expire\n   or: git multi-pack-index [<options>] repack [--batch-size=<size>]\n\n    --[no-]object-dir <directory>\n                          object directory containing set of packfile and pack-index pairs\n\n";

const MERGE_TREE_USAGE: &str = "usage: git merge-tree [--write-tree] [<options>] <branch1> <branch2>\n   or: git merge-tree [--trivial-merge] <base-tree> <branch1> <branch2>\n\n    --write-tree          do a real merge instead of a trivial merge\n    --trivial-merge       do a trivial merge only\n    --[no-]messages       also show informational/conflict messages\n    --quiet               suppress all output; only exit status wanted\n    -z                    separate paths with the NUL character\n    --name-only           list filenames without modes/oids/stages\n    --allow-unrelated-histories\n                          allow merging unrelated histories\n    --stdin               perform multiple merges, one per line of input\n    --[no-]merge-base <tree-ish>\n                          specify a merge-base for the merge\n    -X, --[no-]strategy-option <option=value>\n                          option for selected merge strategy\n\n";

const MERGE_RECURSIVE_OURS_USAGE: &str =
    "usage: git merge-recursive-ours <base>... -- <head> <remote> ...\n";

const MERGE_RECURSIVE_THEIRS_USAGE: &str =
    "usage: git merge-recursive-theirs <base>... -- <head> <remote> ...\n";

const MERGE_INDEX_USAGE: &str =
    "usage: git merge-index [-o] [-q] <merge-program> (-a | [--] [<filename>...])\n";

const MERGE_OURS_USAGE: &str = "usage: git merge-ours <base>... -- HEAD <remote>...\n";

const MERGE_RECURSIVE_USAGE: &str = "usage: git merge-recursive <base>... -- <head> <remote> ...\n";

const MERGE_USAGE: &str = "usage: git merge [<options>] [<commit>...]\n   or: git merge --abort\n   or: git merge --continue\n\n    -n                    do not show a diffstat at the end of the merge\n    --[no-]stat           show a diffstat at the end of the merge\n    --[no-]summary        (synonym to --stat)\n    --[no-]log[=<n>]      add (at most <n>) entries from shortlog to merge commit message\n    --[no-]squash         create a single commit instead of doing a merge\n    --[no-]commit         perform a commit if the merge succeeds (default)\n    -e, --[no-]edit       edit message before committing\n    --[no-]cleanup <mode> how to strip spaces and #comments from message\n    --[no-]ff             allow fast-forward (default)\n    --ff-only             abort if fast-forward is not possible\n    --[no-]rerere-autoupdate\n                          update the index with reused conflict resolution if possible\n    --[no-]verify-signatures\n                          verify that the named commit has a valid GPG signature\n    -s, --[no-]strategy <strategy>\n                          merge strategy to use\n    -X, --[no-]strategy-option <option=value>\n                          option for selected merge strategy\n    -m, --[no-]message <message>\n                          merge commit message (for a non-fast-forward merge)\n    -F, --file <path>     read message from file\n    --[no-]into-name <name>\n                          use <name> instead of the real target\n    -v, --[no-]verbose    be more verbose\n    -q, --[no-]quiet      be more quiet\n    --[no-]abort          abort the current in-progress merge\n    --[no-]quit           --abort but leave index and working tree alone\n    --[no-]continue       continue the current in-progress merge\n    --[no-]allow-unrelated-histories\n                          allow merging unrelated histories\n    --[no-]progress       force progress reporting\n    -S, --[no-]gpg-sign[=<key-id>]\n                          GPG sign commit\n    --[no-]autostash      automatically stash/stash pop before and after\n    --[no-]overwrite-ignore\n                          update ignored files (default)\n    --[no-]signoff        add a Signed-off-by trailer\n    --no-verify           bypass pre-merge-commit and commit-msg hooks\n    --verify              opposite of --no-verify\n\n";

const MERGE_BASE_USAGE: &str = "usage: git merge-base [-a | --all] <commit> <commit>...\n   or: git merge-base [-a | --all] --octopus <commit>...\n   or: git merge-base --is-ancestor <commit> <commit>\n   or: git merge-base --independent <commit>...\n   or: git merge-base --fork-point <ref> [<commit>]\n\n    -a, --[no-]all        output all common ancestors\n    --octopus             find ancestors for a single n-way merge\n    --independent         list revs not reachable from others\n    --is-ancestor         is the first one ancestor of the other?\n    --fork-point          find where <commit> forked from reflog of <ref>\n\n";

const MERGE_FILE_USAGE: &str = "usage: git merge-file [<options>] [-L <name1> [-L <orig> [-L <name2>]]] <file1> <orig-file> <file2>\n\n    -p, --[no-]stdout     send results to standard output\n    --[no-]object-id      use object IDs instead of filenames\n    --[no-]diff3          use a diff3 based merge\n    --[no-]zdiff3         use a zealous diff3 based merge\n    --[no-]ours           for conflicts, use our version\n    --[no-]theirs         for conflicts, use their version\n    --[no-]union          for conflicts, use a union version\n    --diff-algorithm <algorithm>\n                          choose a diff algorithm\n    --[no-]marker-size <n>\n                          for conflicts, use this marker size\n    -q, --[no-]quiet      do not warn about conflicts\n    -L <name>             set labels for file1/orig-file/file2\n\n";

const MAILINFO_USAGE: &str = "usage: git mailinfo [<options>] <msg> <patch> < mail >info\n\n    -k                    keep subject\n    -b                    keep non patch brackets in subject\n    -m, --[no-]message-id copy Message-ID to the end of commit message\n    -u                    re-code metadata to i18n.commitEncoding\n    -n                    disable charset re-coding of metadata\n    --encoding <encoding> re-code metadata to this encoding\n    --[no-]scissors       use scissors\n    --quoted-cr <action>  action when quoted CR is found\n\n";

const MAILSPLIT_USAGE: &str = "usage: git mailsplit [-d<prec>] [-f<n>] [-b] [--keep-cr] -o<directory> [(<mbox>|<Maildir>)...]\n";

const MAINTENANCE_USAGE: &str = "usage: git maintenance <subcommand> [<options>]\n";

const LS_FILES_USAGE: &str = "usage: git ls-files [<options>] [<file>...]\n\n    -z                    separate paths with the NUL character\n    -t                    identify the file status with tags\n    -v                    use lowercase letters for 'assume unchanged' files\n    -f                    use lowercase letters for 'fsmonitor clean' files\n    -c, --[no-]cached     show cached files in the output (default)\n    -d, --[no-]deleted    show deleted files in the output\n    -m, --[no-]modified   show modified files in the output\n    -o, --[no-]others     show other files in the output\n    -i, --[no-]ignored    show ignored files in the output\n    -s, --[no-]stage      show staged contents' object name in the output\n    -k, --[no-]killed     show files on the filesystem that need to be removed\n    --[no-]directory      show 'other' directories' names only\n    --[no-]eol            show line endings of files\n    --[no-]empty-directory\n                          don't show empty directories\n    -u, --[no-]unmerged   show unmerged files in the output\n    --[no-]resolve-undo   show resolve-undo information\n    -x, --exclude <pattern>\n                          skip files matching pattern\n    -X, --exclude-from <file>\n                          read exclude patterns from <file>\n    --[no-]exclude-per-directory <file>\n                          read additional per-directory exclude patterns in <file>\n    --exclude-standard    add the standard git exclusions\n    --full-name           make the output relative to the project top directory\n    --[no-]recurse-submodules\n                          recurse through submodules\n    --[no-]error-unmatch  if any <file> is not in the index, treat this as an error\n    --[no-]with-tree <tree-ish>\n                          pretend that paths removed since <tree-ish> are still present\n    --[no-]abbrev[=<n>]   use <n> digits to display object names\n    --[no-]debug          show debugging data\n    --[no-]deduplicate    suppress duplicate entries\n    --[no-]sparse         show sparse directories in the presence of a sparse index\n    --format <format>     format to use for the output\n\n";

const LS_REMOTE_USAGE: &str = "usage: git ls-remote [--branches] [--tags] [--refs] [--upload-pack=<exec>]\n                     [-q | --quiet] [--exit-code] [--get-url] [--sort=<key>]\n                     [--symref] [<repository> [<patterns>...]]\n\n    -q, --[no-]quiet      do not print remote URL\n    --[no-]upload-pack <exec>\n                          path of git-upload-pack on the remote host\n    -t, --[no-]tags       limit to tags\n    -b, --[no-]branches   limit to branches\n    --[no-]refs           do not show peeled tags\n    --[no-]get-url        take url.<base>.insteadOf into account\n    --[no-]sort <key>     field name to sort on\n    --[no-]exit-code      exit with exit code 2 if no matching refs are found\n    --[no-]symref         show underlying ref in addition to the object pointed by it\n    -o, --[no-]server-option <server-specific>\n                          option to transmit\n\n";

const LS_TREE_USAGE: &str = "usage: git ls-tree [<options>] <tree-ish> [<path>...]\n\n    -d                    only show trees\n    -r                    recurse into subtrees\n    -t                    show trees when recursing\n    -z                    terminate entries with NUL byte\n    -l, --long            include object size\n    --name-only           list only filenames\n    --name-status         list only filenames\n    --object-only         list only objects\n    --[no-]full-name      use full path names\n    --[no-]full-tree      list entire tree; not just current directory (implies --full-name)\n    --format <format>     format to use for the output\n    --[no-]abbrev[=<n>]   use <n> digits to display object names\n\n";

const INIT_DB_USAGE: &str = "usage: git init [-q | --quiet] [--bare] [--template=<template-directory>]\n                [--separate-git-dir <git-dir>] [--object-format=<format>]\n                [--ref-format=<format>]\n                [-b <branch-name> | --initial-branch=<branch-name>]\n                [--shared[=<permissions>]] [<directory>]\n\n    --[no-]template <template-directory>\n                          directory from which templates will be used\n    --[no-]bare           create a bare repository\n    --shared[=<permissions>]\n                          specify that the git repository is to be shared amongst several users\n    -q, --[no-]quiet      be quiet\n    --[no-]separate-git-dir <gitdir>\n                          separate git dir from working tree\n    -b, --[no-]initial-branch <name>\n                          override the name of the initial branch\n    --[no-]object-format <hash>\n                          specify the hash algorithm to use\n    --[no-]ref-format <format>\n                          specify the reference format to use\n\n";

const HOOK_USAGE: &str =
    "usage: git hook run [--ignore-missing] [--to-stdin=<path>] <hook-name> [-- <hook-args>]\n\n";

const INDEX_PACK_USAGE: &str = "usage: git index-pack [-v] [-o <index-file>] [--keep | --keep=<msg>] [--[no-]rev-index] [--verify] [--strict[=<msg-id>=<severity>...]] [--fsck-objects[=<msg-id>=<severity>...]] (<pack-file> | --stdin [--fix-thin] [<pack-file>])\n";

const INTERPRET_TRAILERS_USAGE: &str = "usage: git interpret-trailers [--in-place] [--trim-empty]\n                              [(--trailer (<key>|<key-alias>)[(=|:)<value>])...]\n                              [--parse] [<file>...]\n\n    --[no-]in-place       edit files in place\n    --[no-]trim-empty     trim empty trailers\n    --[no-]where <placement>\n                          where to place the new trailer\n    --[no-]if-exists <action>\n                          action if trailer already exists\n    --[no-]if-missing <action>\n                          action if trailer is missing\n    --[no-]only-trailers  output only the trailers\n    --[no-]only-input     do not apply trailer.* configuration variables\n    --[no-]unfold         reformat multiline trailer values as single-line values\n    --parse               alias for --only-trailers --only-input --unfold\n    --no-divider          do not treat \"---\" as the end of input\n    --divider             opposite of --no-divider\n    --[no-]trailer <trailer>\n                          trailer(s) to add\n\n";

const GREP_USAGE: &str = "usage: git grep [<options>] [-e] <pattern> [<rev>...] [[--] <path>...]\n\n    --[no-]cached         search in index instead of in the work tree\n    --no-index            find in contents not managed by git\n    --index               opposite of --no-index\n    --[no-]untracked      search in both tracked and untracked files\n    --[no-]exclude-standard\n                          ignore files specified via '.gitignore'\n    --[no-]recurse-submodules\n                          recursively search in each submodule\n\n    -v, --[no-]invert-match\n                          show non-matching lines\n    -i, --[no-]ignore-case\n                          case insensitive matching\n    -w, --[no-]word-regexp\n                          match patterns only at word boundaries\n    -a, --[no-]text       process binary files as text\n    -I                    don't match patterns in binary files\n    --[no-]textconv       process binary files with textconv filters\n    -r, --[no-]recursive  search in subdirectories (default)\n    --max-depth <n>       descend at most <n> levels\n\n    -E, --[no-]extended-regexp\n                          use extended POSIX regular expressions\n    -G, --[no-]basic-regexp\n                          use basic POSIX regular expressions (default)\n    -F, --[no-]fixed-strings\n                          interpret patterns as fixed strings\n    -P, --[no-]perl-regexp\n                          use Perl-compatible regular expressions\n\n    -n, --[no-]line-number\n                          show line numbers\n    --[no-]column         show column number of first match\n    -h                    don't show filenames\n    -H                    show filenames\n    --[no-]full-name      show filenames relative to top directory\n    -l, --[no-]files-with-matches\n                          show only filenames instead of matching lines\n    --[no-]name-only      synonym for --files-with-matches\n    -L, --[no-]files-without-match\n                          show only the names of files without match\n    -z, --[no-]null       print NUL after filenames\n    -o, --[no-]only-matching\n                          show only matching parts of a line\n    -c, --[no-]count      show the number of matches instead of matching lines\n    --[no-]color[=<when>] highlight matches\n    --[no-]break          print empty line between matches from different files\n    --[no-]heading        show filename only once above matches from same file\n\n    -C, --[no-]context <n>\n                          show <n> context lines before and after matches\n    -B, --before-context <n>\n                          show <n> context lines before matches\n    -A, --after-context <n>\n                          show <n> context lines after matches\n    --[no-]threads <n>    use <n> worker threads\n    -NUM                  shortcut for -C NUM\n    -p, --[no-]show-function\n                          show a line with the function name before matches\n    -W, --[no-]function-context\n                          show the surrounding function\n\n    -f <file>             read patterns from file\n    -e <pattern>          match <pattern>\n    --and                 combine patterns specified with -e\n    --or\n    --not\n    (\n    )\n    -q, --[no-]quiet      indicate hit with exit status without output\n    --[no-]all-match      show only matches from files that match all patterns\n\n    -O, --[no-]open-files-in-pager[=<pager>]\n                          show matching files in the pager\n    --[no-]ext-grep       allow calling of grep(1) (ignored by this build)\n    -m, --[no-]max-count <n>\n                          maximum number of results per file\n\n";

const HASH_OBJECT_USAGE: &str = "usage: git hash-object [-t <type>] [-w] [--path=<file> | --no-filters]\n                       [--stdin [--literally]] [--] <file>...\n   or: git hash-object [-t <type>] [-w] --stdin-paths [--no-filters]\n\n    -t <type>             object type\n    -w                    write the object into the object database\n    --[no-]stdin          read the object from stdin\n    --[no-]stdin-paths    read file names from stdin\n    --no-filters          store file as is without filters\n    --filters             opposite of --no-filters\n    --[no-]literally      just hash any random garbage to create corrupt objects for debugging Git\n    --[no-]path <file>    process file as it were from this path\n\n";

const HELP_USAGE: &str = "usage: git help [-a|--all] [--[no-]verbose] [--[no-]external-commands] [--[no-]aliases]\n   or: git help [[-i|--info] [-m|--man] [-w|--web]] [<command>|<doc>]\n   or: git help [-g|--guides]\n   or: git help [-c|--config]\n   or: git help [--user-interfaces]\n   or: git help [--developer-interfaces]\n\n    -a, --all             print all available commands\n    --[no-]external-commands\n                          show external commands in --all\n    --[no-]aliases        show aliases in --all\n    -m, --[no-]man        show man page\n    -w, --[no-]web        show manual in web browser\n    -i, --[no-]info       show info page\n    -v, --[no-]verbose    print command description\n    -g, --guides          print list of useful guides\n    --user-interfaces     print list of user-facing repository, command and file interfaces\n    --developer-interfaces\n                          print list of file formats, protocols and other developer interfaces\n    -c, --config          print all configuration variable names\n\n";

const CLEAN_USAGE: &str = "usage: git clean [-d] [-f] [-i] [-n] [-q] [-e <pattern>] [-x | -X] [--] [<pathspec>...]\n\n    -q, --[no-]quiet      do not print names of files removed\n    -n, --[no-]dry-run    dry run\n    -f, --[no-]force      force\n    -i, --[no-]interactive\n                          interactive cleaning\n    -d                    remove whole directories\n    -e, --exclude <pattern>\n                          add <pattern> to ignore rules\n    -x                    remove ignored files, too\n    -X                    remove only ignored files\n\n";

const AM_USAGE: &str = "usage: git am [<options>] [(<mbox> | <Maildir>)...]\n   or: git am [<options>] (--continue | --skip | --abort)\n\n    -i, --[no-]interactive\n                          run interactively\n    -n, --no-verify       bypass pre-applypatch and applypatch-msg hooks\n    --verify              opposite of --no-verify\n    -3, --[no-]3way       allow fall back on 3way merging if needed\n    -q, --[no-]quiet      be quiet\n    -s, --[no-]signoff    add a Signed-off-by trailer to the commit message\n    -u, --[no-]utf8       recode into utf8 (default)\n    -k, --[no-]keep       pass -k flag to git-mailinfo\n    --[no-]keep-non-patch pass -b flag to git-mailinfo\n    -m, --[no-]message-id pass -m flag to git-mailinfo\n    --[no-]keep-cr        pass --keep-cr flag to git-mailsplit for mbox format\n    -c, --[no-]scissors   strip everything before a scissors line\n    --quoted-cr <action>  pass it through git-mailinfo\n    --[no-]whitespace <action>\n                          pass it through git-apply\n    --[no-]ignore-space-change\n                          pass it through git-apply\n    --[no-]ignore-whitespace\n                          pass it through git-apply\n    --[no-]directory <root>\n                          pass it through git-apply\n    --[no-]exclude <path> pass it through git-apply\n    --[no-]include <path> pass it through git-apply\n    -C <n>                pass it through git-apply\n    -p <num>              pass it through git-apply\n    --[no-]patch-format <format>\n                          format the patch(es) are in\n    --[no-]reject         pass it through git-apply\n    --[no-]resolvemsg ... override error message when patch failure occurs\n    --continue            continue applying patches after resolving a conflict\n    -r, --resolved        synonyms for --continue\n    --skip                skip the current patch\n    --abort               restore the original branch and abort the patching operation\n    --quit                abort the patching operation but keep HEAD where it is\n    --show-current-patch[=(diff|raw)]\n                          show the patch being applied\n    --retry               try to apply current patch again\n    --allow-empty         record the empty patch as an empty commit\n    --[no-]committer-date-is-author-date\n                          lie about committer date\n    --[no-]ignore-date    use current timestamp for author date\n    --[no-]rerere-autoupdate\n                          update the index with reused conflict resolution if possible\n    -S, --[no-]gpg-sign[=<key-id>]\n                          GPG-sign commits\n    --empty (stop|drop|keep)\n                          how to handle empty patches\n\n";

const APPLY_USAGE: &str = "usage: git apply [<options>] [<patch>...]\n\n    --exclude <path>      don't apply changes matching the given path\n    --include <path>      apply changes matching the given path\n    -p <num>              remove <num> leading slashes from traditional diff paths\n    --no-add              ignore additions made by the patch\n    --add                 opposite of --no-add\n    --[no-]stat           instead of applying the patch, output diffstat for the input\n    --[no-]numstat        show number of added and deleted lines in decimal notation\n    --[no-]summary        instead of applying the patch, output a summary for the input\n    --[no-]check          instead of applying the patch, see if the patch is applicable\n    --[no-]index          make sure the patch is applicable to the current index\n    -N, --[no-]intent-to-add\n                          mark new files with `git add --intent-to-add`\n    --[no-]cached         apply a patch without touching the working tree\n    --[no-]unsafe-paths   accept a patch that touches outside the working area\n    --[no-]apply          also apply the patch (use with --stat/--summary/--check)\n    -3, --[no-]3way       attempt three-way merge, fall back on normal patch if that fails\n    --ours                for conflicts, use our version\n    --theirs              for conflicts, use their version\n    --union               for conflicts, use a union version\n    --[no-]build-fake-ancestor <file>\n                          build a temporary index based on embedded index information\n    -z                    paths are separated with NUL character\n    -C <n>                ensure at least <n> lines of context match\n    --[no-]whitespace <action>\n                          detect new or modified lines that have whitespace errors\n    --[no-]ignore-space-change\n                          ignore changes in whitespace when finding context\n    --[no-]ignore-whitespace\n                          ignore changes in whitespace when finding context\n    -R, --[no-]reverse    apply the patch in reverse\n    --[no-]unidiff-zero   don't expect at least one line of context\n    --[no-]reject         leave the rejected hunks in corresponding *.rej files\n    --[no-]allow-overlap  allow overlapping hunks\n    -v, --[no-]verbose    be more verbose\n    -q, --[no-]quiet      be more quiet\n    --[no-]inaccurate-eof tolerate incorrectly detected missing new-line at the end of file\n    --[no-]recount        do not trust the line counts in the hunk headers\n    --[no-]directory <root>\n                          prepend <root> to all filenames\n    --[no-]allow-empty    don't return error for empty patches\n\n";

const ARCHIVE_USAGE: &str = "usage: git archive [<options>] <tree-ish> [<path>...]\n   or: git archive --list\n   or: git archive --remote <repo> [--exec <cmd>] [<options>] <tree-ish> [<path>...]\n   or: git archive --remote <repo> [--exec <cmd>] --list\n\n    --[no-]format <fmt>   archive format\n    --[no-]prefix <prefix>\n                          prepend prefix to each pathname in the archive\n    --[no-]add-file <file>\n                          add untracked file to archive\n    --[no-]add-virtual-file <path:content>\n                          add untracked file to archive\n    -o, --[no-]output <file>\n                          write the archive to this file\n    --[no-]worktree-attributes\n                          read .gitattributes in working directory\n    -v, --[no-]verbose    report archived files on stderr\n    --mtime <time>        set modification time of archive entries\n    -NUM                  set compression level\n\n    -l, --[no-]list       list supported archive formats\n\n    --[no-]remote <repo>  retrieve the archive from remote repository <repo>\n    --[no-]exec <command> path to the remote git-upload-archive command\n\n";

const BACKFILL_USAGE: &str = "usage: git backfill [--min-batch-size=<n>] [--[no-]sparse]\n\n    --min-batch-size <n>  Minimum number of objects to request at a time\n    --[no-]sparse         Restrict the missing objects to the current sparse-checkout\n\n";

const BUGREPORT_USAGE: &str = "usage: git bugreport [(-o | --output-directory) <path>]\n                     [(-s | --suffix) <format> | --no-suffix]\n                     [--diagnose[=<mode>]]\n\n    --[no-]diagnose[=<mode>]\n                          create an additional zip archive of detailed diagnostics (default 'stats')\n    -o, --[no-]output-directory <path>\n                          specify a destination for the bugreport file(s)\n    -s, --[no-]suffix <format>\n                          specify a strftime format suffix for the filename(s)\n\n";
const BUGREPORT_SHORT_USAGE: &str = "usage: git bugreport [(-o | --output-directory) <path>]\n              [(-s | --suffix) <format> | --no-suffix]\n              [--diagnose[=<mode>]]\n";

const BUNDLE_USAGE: &str = "usage: git bundle create [-q | --quiet | --progress]\n                         [--version=<version>] <file> <git-rev-list-args>\n   or: git bundle verify [-q | --quiet] <file>\n   or: git bundle list-heads <file> [<refname>...]\n   or: git bundle unbundle [--progress] <file> [<refname>...]\n";

const CAT_FILE_USAGE: &str = "usage: git cat-file <type> <object>\n   or: git cat-file (-e | -p) <object>\n   or: git cat-file (-t | -s) [--allow-unknown-type] <object>\n   or: git cat-file (--textconv | --filters)\n                    [<rev>:<path|tree-ish> | --path=<path|tree-ish> <rev>]\n   or: git cat-file (--batch | --batch-check | --batch-command) [--batch-all-objects]\n                    [--buffer] [--follow-symlinks] [--unordered]\n                    [--textconv | --filters] [-Z]\n\nCheck object existence or emit object contents\n    -e                    check if <object> exists\n    -p                    pretty-print <object> content\n\nEmit [broken] object attributes\n    -t                    show object type (one of 'blob', 'tree', 'commit', 'tag', ...)\n    -s                    show object size\n    --[no-]use-mailmap    use mail map file\n    --[no-]mailmap        alias of --use-mailmap\n\nBatch objects requested on stdin (or --batch-all-objects)\n    --batch[=<format>]    show full <object> or <rev> contents\n    --batch-check[=<format>]\n                          like --batch, but don't emit <contents>\n    -Z                    stdin and stdout is NUL-terminated\n    --batch-command[=<format>]\n                          read commands from stdin\n    --batch-all-objects   with --batch[-check]: ignores stdin, batches all known objects\n\nChange or optimize batch output\n    --[no-]buffer         buffer --batch output\n    --[no-]follow-symlinks\n                          follow in-tree symlinks\n    --[no-]unordered      do not order objects before emitting them\n\nEmit object (blob or tree) with conversion or filter (stand-alone, or with batch)\n    --textconv            run textconv on object's content\n    --filters             run filters on object's content\n    --[no-]path blob|tree use a <path> for (--textconv | --filters); Not with 'batch'\n    --[no-]filter <args>  object filtering\n\nZmin extensions\n    --type                alias of -t\n    --size                alias of -s\n    --exists              alias of -e\n    --pretty              alias of -p\n\n";

const CHECK_ATTR_USAGE: &str = "usage: git check-attr [--source <tree-ish>] [-a | --all | <attr>...] [--] <pathname>...\n   or: git check-attr --stdin [-z] [--source <tree-ish>] [-a | --all | <attr>...]\n\n    -a, --[no-]all        report all attributes set on file\n    --[no-]cached         use .gitattributes only from the index\n    --[no-]stdin          read file names from stdin\n    -z                    terminate input and output records by a NUL character\n    --[no-]source <tree-ish>\n                          which tree-ish to check attributes at\n\n";

const CHECK_IGNORE_USAGE: &str = "usage: git check-ignore [<options>] <pathname>...\n   or: git check-ignore [<options>] --stdin\n\n    -q, --[no-]quiet      suppress progress reporting\n    -v, --[no-]verbose    be verbose\n\n    --[no-]stdin          read file names from stdin\n    -z                    terminate input and output records by a NUL character\n    -n, --[no-]non-matching\n                          show non-matching input paths\n    --no-index            ignore index when checking\n    --index               opposite of --no-index\n\n";

const CHECK_MAILMAP_USAGE: &str = "usage: git check-mailmap [<options>] <contact>...\n\n    --[no-]stdin          also read contacts from stdin\n    --[no-]mailmap-file <file>\n                          read additional mailmap entries from file\n    --[no-]mailmap-blob <blob>\n                          read additional mailmap entries from blob\n\n";

const CHECK_REF_FORMAT_USAGE: &str = "usage: git check-ref-format [--normalize] [<options>] <refname>\n   or: git check-ref-format --branch <branchname-shorthand>\n";

const CHECKOUT_USAGE: &str = "usage: git checkout [<options>] <branch>\n   or: git checkout [<options>] [<branch>] -- <file>...\n\n    -b <branch>           create and checkout a new branch\n    -B <branch>           create/reset and checkout a branch\n    -l                    create reflog for new branch\n    --[no-]guess          second guess 'git checkout <no-such-branch>' (default)\n    --[no-]overlay        use overlay mode (default)\n    -q, --[no-]quiet      suppress progress reporting\n    --[no-]recurse-submodules[=<checkout>]\n                          control recursive updating of submodules\n    --[no-]progress       force progress reporting\n    -m, --[no-]merge      perform a 3-way merge with the new branch\n    --[no-]conflict <style>\n                          conflict style (merge, diff3, or zdiff3)\n    -d, --[no-]detach     detach HEAD at named commit\n    -t, --[no-]track[=(direct|inherit)]\n                          set branch tracking configuration\n    -f, --[no-]force      force checkout (throw away local modifications)\n    --[no-]orphan <new-branch>\n                          new unborn branch\n    --[no-]overwrite-ignore\n                          update ignored files (default)\n    --[no-]ignore-other-worktrees\n                          do not check if another worktree is using this branch\n    -2, --ours            checkout our version for unmerged files\n    -3, --theirs          checkout their version for unmerged files\n    -p, --[no-]patch      select hunks interactively\n    --[no-]ignore-skip-worktree-bits\n                          do not limit pathspecs to sparse entries only\n    --[no-]pathspec-from-file <file>\n                          read pathspec from file\n    --[no-]pathspec-file-nul\n                          with --pathspec-from-file, pathspec elements are separated with NUL character\n\n";

const CHECKOUT_WORKER_USAGE: &str = "usage: git checkout--worker [<options>]\n\n    --[no-]prefix <string>\n                          when creating files, prepend <string>\n\n";

const CHECKOUT_INDEX_USAGE: &str = "usage: git checkout-index [<options>] [--] [<file>...]\n\n    -a, --[no-]all        check out all files in the index\n    --[no-]ignore-skip-worktree-bits\n                          do not skip files with skip-worktree set\n    -f, --[no-]force      force overwrite of existing files\n    -q, --[no-]quiet      no warning for existing files and files not in index\n    -n, --no-create       don't checkout new files\n    --create              opposite of --no-create\n    -u, --[no-]index      update stat information in the index file\n    -z                    paths are separated with NUL character\n    --[no-]stdin          read list of paths from the standard input\n    --[no-]temp           write the content to temporary files\n    --[no-]prefix <string>\n                          when creating files, prepend <string>\n    --stage (1|2|3|all)   copy out the files from named stage\n\n";

const CHERRY_USAGE: &str = "usage: git cherry [-v] [<upstream> [<head> [<limit>]]]\n\n    --[no-]abbrev[=<n>]   use <n> digits to display object names\n    -v, --[no-]verbose    be verbose\n\n";

const CHERRY_PICK_USAGE: &str = "usage: git cherry-pick [--edit] [-n] [-m <parent-number>] [-s] [-x] [--ff]\n                       [-S[<keyid>]] <commit>...\n   or: git cherry-pick (--continue | --skip | --abort | --quit)\n\n    --quit                end revert or cherry-pick sequence\n    --continue            resume revert or cherry-pick sequence\n    --abort               cancel revert or cherry-pick sequence\n    --skip                skip current commit and continue\n    --[no-]cleanup <mode> how to strip spaces and #comments from message\n    -n, --no-commit       don't automatically commit\n    --commit              opposite of --no-commit\n    -e, --[no-]edit       edit the commit message\n    -s, --[no-]signoff    add a Signed-off-by trailer\n    -m, --[no-]mainline <parent-number>\n                          select mainline parent\n    --[no-]rerere-autoupdate\n                          update the index with reused conflict resolution if possible\n    --[no-]strategy <strategy>\n                          merge strategy\n    -X, --[no-]strategy-option <option>\n                          option for merge strategy\n    -S, --[no-]gpg-sign[=<key-id>]\n                          GPG sign commit\n    -x                    append commit name\n    --[no-]ff             allow fast-forward\n    --[no-]allow-empty    preserve initially empty commits\n    --[no-]allow-empty-message\n                          allow commits with empty messages\n    --[no-]keep-redundant-commits\n                          deprecated: use --empty=keep instead\n    --empty (stop|drop|keep)\n                          how to handle commits that become empty\n\n";

const CLONE_USAGE: &str = "usage: git clone [<options>] [--] <repo> [<dir>]\n\n    -v, --[no-]verbose    be more verbose\n    -q, --[no-]quiet      be more quiet\n    --[no-]progress       force progress reporting\n    --[no-]reject-shallow don't clone shallow repository\n    -n, --no-checkout     don't create a checkout\n    --checkout            opposite of --no-checkout\n    --[no-]bare           create a bare repository\n    --[no-]mirror         create a mirror repository (implies --bare)\n    -l, --[no-]local      to clone from a local repository\n    --no-hardlinks        don't use local hardlinks, always copy\n    --hardlinks           opposite of --no-hardlinks\n    -s, --[no-]shared     setup as shared repository\n    --[no-]recurse-submodules[=<pathspec>]\n                          initialize submodules in the clone\n    --[no-]recursive[=<pathspec>]\n                          alias of --recurse-submodules\n    -j, --[no-]jobs <n>   number of submodules cloned in parallel\n    --[no-]template <template-directory>\n                          directory from which templates will be used\n    --[no-]reference <repo>\n                          reference repository\n    --[no-]reference-if-able <repo>\n                          reference repository\n    --[no-]dissociate     use --reference only while cloning\n    -o, --[no-]origin <name>\n                          use <name> instead of 'origin' to track upstream\n    -b, --[no-]branch <branch>\n                          checkout <branch> instead of the remote's HEAD\n    --[no-]revision <rev> clone single revision <rev> and check out\n    -u, --[no-]upload-pack <path>\n                          path to git-upload-pack on the remote\n    --[no-]depth <depth>  create a shallow clone of that depth\n    --[no-]shallow-since <time>\n                          create a shallow clone since a specific time\n    --[no-]shallow-exclude <ref>\n                          deepen history of shallow clone, excluding ref\n    --[no-]single-branch  clone only one branch, HEAD or --branch\n    --[no-]tags           clone tags, and make later fetches not to follow them\n    --[no-]shallow-submodules\n                          any cloned submodules will be shallow\n    --[no-]separate-git-dir <gitdir>\n                          separate git dir from working tree\n    --[no-]ref-format <format>\n                          specify the reference format to use\n    -c, --[no-]config <key=value>\n                          set config inside the new repository\n    --[no-]server-option <server-specific>\n                          option to transmit\n    -4, --ipv4            use IPv4 addresses only\n    -6, --ipv6            use IPv6 addresses only\n    --[no-]filter <args>  object filtering\n    --[no-]also-filter-submodules\n                          apply partial clone filters to submodules\n    --[no-]remote-submodules\n                          any cloned submodules will use their remote-tracking branch\n    --[no-]sparse         initialize sparse-checkout file to include only files at root\n    --[no-]bundle-uri <uri>\n                          a URI for downloading bundles before fetching from origin remote\n\n";

const COLUMN_USAGE: &str = "usage: git column [<options>]\n\n    --[no-]command <name> lookup config vars\n    --[no-]mode[=<style>] layout to use\n    --raw-mode <n>        layout to use\n    --[no-]width <n>      maximum width\n    --[no-]indent <string>\n                          padding space on left border\n    --[no-]nl <string>    padding space on right border\n    --[no-]padding <n>    padding space between columns\n\n";

const COMMIT_USAGE: &str = "usage: git commit [-a | --interactive | --patch] [-s] [-v] [-u[<mode>]] [--amend]\n                  [--dry-run] [(-c | -C | --squash) <commit> | --fixup [(amend|reword):]<commit>]\n                  [-F <file> | -m <msg>] [--reset-author] [--allow-empty]\n                  [--allow-empty-message] [--no-verify] [-e] [--author=<author>]\n                  [--date=<date>] [--cleanup=<mode>] [--[no-]status]\n                  [-i | -o] [--pathspec-from-file=<file> [--pathspec-file-nul]]\n                  [(--trailer <token>[(=|:)<value>])...] [-S[<keyid>]]\n                  [--] [<pathspec>...]\n\n    -q, --[no-]quiet      suppress summary after successful commit\n    -v, --[no-]verbose    show diff in commit message template\n\nCommit message options\n    -F, --[no-]file <file>\n                          read message from file\n    --[no-]author <author>\n                          override author for commit\n    --[no-]date <date>    override date for commit\n    -m, --[no-]message <message>\n                          commit message\n    -c, --[no-]reedit-message <commit>\n                          reuse and edit message from specified commit\n    -C, --[no-]reuse-message <commit>\n                          reuse message from specified commit\n    --[no-]fixup [(amend|reword):]commit\n                          use autosquash formatted message to fixup or amend/reword specified commit\n    --[no-]squash <commit>\n                          use autosquash formatted message to squash specified commit\n    --[no-]reset-author   the commit is authored by me now (used with -C/-c/--amend)\n    --trailer <trailer>   add custom trailer(s)\n    -s, --[no-]signoff    add a Signed-off-by trailer\n    -t, --[no-]template <file>\n                          use specified template file\n    -e, --[no-]edit       force edit of commit\n    --[no-]cleanup <mode> how to strip spaces and #comments from message\n    --[no-]status         include status in commit message template\n    -S, --[no-]gpg-sign[=<key-id>]\n                          GPG sign commit\n\nCommit contents options\n    -a, --[no-]all        commit all changed files\n    -i, --[no-]include    add specified files to index for commit\n    --[no-]interactive    interactively add files\n    -p, --[no-]patch      interactively add changes\n    -o, --[no-]only       commit only specified files\n    -n, --no-verify       bypass pre-commit and commit-msg hooks\n    --verify              opposite of --no-verify\n    --[no-]dry-run        show what would be committed\n    --[no-]short          show status concisely\n    --[no-]branch         show branch information\n    --[no-]ahead-behind   compute full ahead/behind values\n    --[no-]porcelain      machine-readable output\n    --[no-]long           show status in long format (default)\n    -z, --[no-]null       terminate entries with NUL\n    --[no-]amend          amend previous commit\n    --no-post-rewrite     bypass post-rewrite hook\n    --post-rewrite        opposite of --no-post-rewrite\n    -u, --[no-]untracked-files[=<mode>]\n                          show untracked files, optional modes: all, normal, no. (Default: all)\n    --[no-]pathspec-from-file <file>\n                          read pathspec from file\n    --[no-]pathspec-file-nul\n                          with --pathspec-from-file, pathspec elements are separated with NUL character\n\n";

const COMMIT_GRAPH_USAGE: &str = "usage: git commit-graph verify [--object-dir <dir>] [--shallow] [--[no-]progress]\n   or: git commit-graph write [--object-dir <dir>] [--append]\n                              [--split[=<strategy>]] [--reachable | --stdin-packs | --stdin-commits]\n                              [--changed-paths] [--[no-]max-new-filters <n>] [--[no-]progress]\n                              <split-options>\n\n    --[no-]object-dir <dir>\n                          the object directory to store the graph\n\n";

const COMMIT_TREE_USAGE: &str = "usage: git commit-tree <tree> [(-p <parent>)...]\n   or: git commit-tree [(-p <parent>)...] [-S[<keyid>]] [(-m <message>)...]\n                       [(-F <file>)...] <tree>\n\n    -p <parent>           id of a parent commit object\n    -m <message>          commit message\n    -F <file>             read commit log message from file\n    -S, --[no-]gpg-sign[=<key-id>]\n                          GPG sign commit\n\n";

const CONFIG_USAGE: &str = "usage: git config list [<file-option>] [<display-option>] [--includes]\n   or: git config get [<file-option>] [<display-option>] [--includes] [--all] [--regexp] [--value=<pattern>] [--fixed-value] [--default=<default>] [--url=<url>] <name>\n   or: git config set [<file-option>] [--type=<type>] [--all] [--value=<pattern>] [--fixed-value] <name> <value>\n   or: git config unset [<file-option>] [--all] [--value=<pattern>] [--fixed-value] <name>\n   or: git config rename-section [<file-option>] <old-name> <new-name>\n   or: git config remove-section [<file-option>] <name>\n   or: git config edit [<file-option>]\n   or: git config [<file-option>] --get-colorbool <name> [<stdout-is-tty>]\n";

const COUNT_OBJECTS_USAGE: &str = "usage: git count-objects [-v] [-H | --human-readable]\n\n    -v, --[no-]verbose    be verbose\n    -H, --[no-]human-readable\n                          print sizes in human readable format\n\n";

const CREDENTIAL_USAGE: &str = "usage: git credential (fill|approve|reject)\n";

const CREDENTIAL_CACHE_USAGE: &str = "usage: git credential-cache [<options>] <action>\n\n    --[no-]timeout <n>    number of seconds to cache credentials\n    --[no-]socket <path>  path of cache-daemon socket\n\n";

const CREDENTIAL_CACHE_DAEMON_USAGE: &str = "usage: git credential-cache--daemon [--debug] <socket-path>\n\n    --[no-]debug          print debugging messages to stderr\n\n";

const CREDENTIAL_STORE_USAGE: &str = "usage: git credential-store [<options>] <action>\n\n    --[no-]file <path>    fetch and store credentials in <path>\n\n";

const DESCRIBE_USAGE: &str = "usage: git describe [--all] [--tags] [--contains] [--abbrev=<n>] [<commit-ish>...]\n   or: git describe [--all] [--tags] [--contains] [--abbrev=<n>] --dirty[=<mark>]\n   or: git describe <blob>\n\n    --[no-]contains       find the tag that comes after the commit\n    --[no-]debug          debug search strategy on stderr\n    --[no-]all            use any ref\n    --[no-]tags           use any tag, even unannotated\n    --[no-]long           always use long format\n    --[no-]first-parent   only follow first parent\n    --[no-]abbrev[=<n>]   use <n> digits to display object names\n    --[no-]exact-match    only output exact matches\n    --[no-]candidates <n> consider <n> most recent tags (default: 10)\n    --[no-]match <pattern>\n                          only consider tags matching <pattern>\n    --[no-]exclude <pattern>\n                          do not consider tags matching <pattern>\n    --[no-]always         show abbreviated commit object as fallback\n    --[no-]dirty[=<mark>] append <mark> on dirty working tree (default: \"-dirty\")\n    --[no-]broken[=<mark>]\n                          append <mark> on broken working tree (default: \"-broken\")\n\n";

const DIAGNOSE_USAGE: &str = "usage: git diagnose [(-o | --output-directory) <path>] [(-s | --suffix) <format>]\n                    [--mode=<mode>]\n\n    -o, --[no-]output-directory <path>\n                          specify a destination for the diagnostics archive\n    -s, --[no-]suffix <format>\n                          specify a strftime format suffix for the filename\n    --mode (stats|all)    specify the content of the diagnostic archive\n\n";

const DIFF_USAGE: &str = "usage: git diff [<options>] [<commit>] [--] [<path>...]\n   or: git diff [<options>] --cached [--merge-base] [<commit>] [--] [<path>...]\n   or: git diff [<options>] [--merge-base] <commit> [<commit>...] <commit> [--] [<path>...]\n   or: git diff [<options>] <commit>...<commit> [--] [<path>...]\n   or: git diff [<options>] <blob> <blob>\n   or: git diff [<options>] --no-index [--] <path> <path> [<pathspec>...]\n\ncommon diff options:\n  -z            output diff-raw with lines terminated with NUL.\n  -p            output patch format.\n  -u            synonym for -p.\n  --patch-with-raw\n                output both a patch and the diff-raw format.\n  --stat        show diffstat instead of patch.\n  --numstat     show numeric diffstat instead of patch.\n  --patch-with-stat\n                output a patch and prepend its diffstat.\n  --name-only   show only names of changed files.\n  --name-status show names and status of changed files.\n  --full-index  show full object name on index lines.\n  --abbrev=<n>  abbreviate object names in diff-tree header and diff-raw.\n  -R            swap input file pairs.\n  -B            detect complete rewrites.\n  -M            detect renames.\n  -C            detect copies.\n  --find-copies-harder\n                try unchanged files as candidate for copy detection.\n  -l<n>         limit rename attempts up to <n> paths.\n  -O<file>      reorder diffs according to the <file>.\n  -S<string>    find filepair whose only one side contains the string.\n  --pickaxe-all\n                show all files diff when -S is used and hit is found.\n  -a  --text    treat all files as text.\n";

const DIFF_FILES_USAGE: &str = "usage: git diff-files [-q] [-0 | -1 | -2 | -3 | -c | --cc] [<common-diff-options>] [<path>...]\n\ncommon diff options:\n  -z            output diff-raw with lines terminated with NUL.\n  -p            output patch format.\n  -u            synonym for -p.\n  --patch-with-raw\n                output both a patch and the diff-raw format.\n  --stat        show diffstat instead of patch.\n  --numstat     show numeric diffstat instead of patch.\n  --patch-with-stat\n                output a patch and prepend its diffstat.\n  --name-only   show only names of changed files.\n  --name-status show names and status of changed files.\n  --full-index  show full object name on index lines.\n  --abbrev=<n>  abbreviate object names in diff-tree header and diff-raw.\n  -R            swap input file pairs.\n  -B            detect complete rewrites.\n  -M            detect renames.\n  -C            detect copies.\n  --find-copies-harder\n                try unchanged files as candidate for copy detection.\n  -l<n>         limit rename attempts up to <n> paths.\n  -O<file>      reorder diffs according to the <file>.\n  -S<string>    find filepair whose only one side contains the string.\n  --pickaxe-all\n                show all files diff when -S is used and hit is found.\n  -a  --text    treat all files as text.\n\n";

const DIFF_INDEX_USAGE: &str = "usage: git diff-index [-m] [--cached] [--merge-base] [<common-diff-options>] <tree-ish> [<path>...]\n\ncommon diff options:\n  -z            output diff-raw with lines terminated with NUL.\n  -p            output patch format.\n  -u            synonym for -p.\n  --patch-with-raw\n                output both a patch and the diff-raw format.\n  --stat        show diffstat instead of patch.\n  --numstat     show numeric diffstat instead of patch.\n  --patch-with-stat\n                output a patch and prepend its diffstat.\n  --name-only   show only names of changed files.\n  --name-status show names and status of changed files.\n  --full-index  show full object name on index lines.\n  --abbrev=<n>  abbreviate object names in diff-tree header and diff-raw.\n  -R            swap input file pairs.\n  -B            detect complete rewrites.\n  -M            detect renames.\n  -C            detect copies.\n  --find-copies-harder\n                try unchanged files as candidate for copy detection.\n  -l<n>         limit rename attempts up to <n> paths.\n  -O<file>      reorder diffs according to the <file>.\n  -S<string>    find filepair whose only one side contains the string.\n  --pickaxe-all\n                show all files diff when -S is used and hit is found.\n  -a  --text    treat all files as text.\n\n";

const DIFF_PAIRS_USAGE: &str = "usage: git diff-pairs -z [<diff-options>]\n\nDiff output format options\n    -p, --patch           generate patch\n    -s, --no-patch        suppress diff output\n    -u                    generate patch\n    -U, --unified[=<n>]   generate diffs with <n> lines context\n    -W, --[no-]function-context\n                          generate diffs with <n> lines context\n    --raw                 generate the diff in raw format\n    --patch-with-raw      synonym for '-p --raw'\n    --patch-with-stat     synonym for '-p --stat'\n    --numstat             machine friendly --stat\n    --shortstat           output only the last line of --stat\n    -X, --dirstat[=<param1>,<param2>...]\n                          output the distribution of relative amount of changes for each sub-directory\n    --cumulative          synonym for --dirstat=cumulative\n    --dirstat-by-file[=<param1>,<param2>...]\n                          synonym for --dirstat=files,<param1>,<param2>...\n    --check               warn if changes introduce conflict markers or whitespace errors\n    --summary             condensed summary such as creations, renames and mode changes\n    --name-only           show only names of changed files\n    --name-status         show only names and status of changed files\n    --stat[=<width>[,<name-width>[,<count>]]]\n                          generate diffstat\n    --stat-width <width>  generate diffstat with a given width\n    --stat-name-width <width>\n                          generate diffstat with a given name width\n    --stat-graph-width <width>\n                          generate diffstat with a given graph width\n    --stat-count <count>  generate diffstat with limited lines\n    --[no-]compact-summary\n                          generate compact summary in diffstat\n    --binary              output a binary diff that can be applied\n    --[no-]full-index     show full pre- and post-image object names on the \"index\" lines\n    --[no-]color[=<when>] show colored diff\n    --ws-error-highlight <kind>\n                          highlight whitespace errors in the 'context', 'old' or 'new' lines in the diff\n    -z                    do not munge pathnames and use NULs as output field terminators in --raw or --numstat\n    --[no-]abbrev[=<n>]   use <n> digits to display object names\n    --src-prefix <prefix> show the given source prefix instead of \"a/\"\n    --dst-prefix <prefix> show the given destination prefix instead of \"b/\"\n    --line-prefix <prefix>\n                          prepend an additional prefix to every line of output\n    --no-prefix           do not show any source or destination prefix\n    --default-prefix      use default prefixes a/ and b/\n    --inter-hunk-context <n>\n                          show context between diff hunks up to the specified number of lines\n    --output-indicator-new <char>\n                          specify the character to indicate a new line instead of '+'\n    --output-indicator-old <char>\n                          specify the character to indicate an old line instead of '-'\n    --output-indicator-context <char>\n                          specify the character to indicate a context instead of ' '\n\nDiff rename options\n    -B, --break-rewrites[=<n>[/<m>]]\n                          break complete rewrite changes into pairs of delete and create\n    -M, --find-renames[=<n>]\n                          detect renames\n    -D, --irreversible-delete\n                          omit the preimage for deletes\n    -C, --find-copies[=<n>]\n                          detect copies\n    --[no-]find-copies-harder\n                          use unmodified files as source to find copies\n    --no-renames          disable rename detection\n    --[no-]rename-empty   use empty blobs as rename source\n    --[no-]follow         continue listing the history of a file beyond renames\n    -l <n>                prevent rename/copy detection if the number of rename/copy targets exceeds given limit\n\nDiff algorithm options\n    --minimal             produce the smallest possible diff\n    -w, --ignore-all-space\n                          ignore whitespace when comparing lines\n    -b, --ignore-space-change\n                          ignore changes in amount of whitespace\n    --ignore-space-at-eol ignore changes in whitespace at EOL\n    --ignore-cr-at-eol    ignore carrier-return at the end of line\n    --ignore-blank-lines  ignore changes whose lines are all blank\n    -I, --[no-]ignore-matching-lines <regex>\n                          ignore changes whose all lines match <regex>\n    --[no-]indent-heuristic\n                          heuristic to shift diff hunk boundaries for easy reading\n    --patience            generate diff using the \"patience diff\" algorithm\n    --histogram           generate diff using the \"histogram diff\" algorithm\n    --diff-algorithm <algorithm>\n                          choose a diff algorithm\n    --anchored <text>     generate diff using the \"anchored diff\" algorithm\n    --word-diff[=<mode>]  show word diff, using <mode> to delimit changed words\n    --word-diff-regex <regex>\n                          use <regex> to decide what a word is\n    --color-words[=<regex>]\n                          equivalent to --word-diff=color --word-diff-regex=<regex>\n    --[no-]color-moved[=<mode>]\n                          moved lines of code are colored differently\n    --[no-]color-moved-ws <mode>\n                          how white spaces are ignored in --color-moved\n\nOther diff options\n    --[no-]relative[=<prefix>]\n                          when run from subdir, exclude changes outside and show relative paths\n    -a, --[no-]text       treat all files as text\n    -R                    swap two inputs, reverse the diff\n    --[no-]exit-code      exit with 1 if there were differences, 0 otherwise\n    --[no-]quiet          disable all output of the program\n    --[no-]ext-diff       allow an external diff helper to be executed\n    --[no-]textconv       run external text conversion filters when comparing binary files\n    --ignore-submodules[=<when>]\n                          ignore changes to submodules in the diff generation\n    --submodule[=<format>]\n                          specify how differences in submodules are shown\n    --ita-invisible-in-index\n                          hide 'git add -N' entries from the index\n    --ita-visible-in-index\n                          treat 'git add -N' entries as real in the index\n    -S <string>           look for differences that change the number of occurrences of the specified string\n    -G <regex>            look for differences that change the number of occurrences of the specified regex\n    --pickaxe-all         show all changes in the changeset with -S or -G\n    --pickaxe-regex       treat <string> in -S as extended POSIX regular expression\n    -O <file>             control the order in which files appear in the output\n    --rotate-to <path>    show the change in the specified path first\n    --skip-to <path>      skip the output to the specified path\n    --find-object <object-id>\n                          look for differences that change the number of occurrences of the specified object\n    --diff-filter [(A|C|D|M|R|T|U|X|B)...[*]]\n                          select files by diff type\n    --output <file>       output to a specific file\n\n";

const DIFF_TREE_USAGE: &str = "usage: git diff-tree [--stdin] [-m] [-s] [-v] [--no-commit-id] [--pretty]\n              [-t] [-r] [-c | --cc] [--combined-all-paths] [--root] [--merge-base]\n              [<common-diff-options>] <tree-ish> [<tree-ish>] [<path>...]\n\n  -r            diff recursively\n  -c            show combined diff for merge commits\n  --cc          show combined diff for merge commits removing uninteresting hunks\n  --combined-all-paths\n                show name of file in all parents for combined diffs\n  --root        include the initial commit as diff against /dev/null\n\ncommon diff options:\n  -z            output diff-raw with lines terminated with NUL.\n  -p            output patch format.\n  -u            synonym for -p.\n  --patch-with-raw\n                output both a patch and the diff-raw format.\n  --stat        show diffstat instead of patch.\n  --numstat     show numeric diffstat instead of patch.\n  --patch-with-stat\n                output a patch and prepend its diffstat.\n  --name-only   show only names of changed files.\n  --name-status show names and status of changed files.\n  --full-index  show full object name on index lines.\n  --abbrev=<n>  abbreviate object names in diff-tree header and diff-raw.\n  -R            swap input file pairs.\n  -B            detect complete rewrites.\n  -M            detect renames.\n  -C            detect copies.\n  --find-copies-harder\n                try unchanged files as candidate for copy detection.\n  -l<n>         limit rename attempts up to <n> paths.\n  -O<file>      reorder diffs according to the <file>.\n  -S<string>    find filepair whose only one side contains the string.\n  --pickaxe-all\n                show all files diff when -S is used and hit is found.\n  -a  --text    treat all files as text.\n\n";

const DIFFTOOL_USAGE: &str = "usage: git difftool [<options>] [<commit> [<commit>]] [--] [<path>...]\n\n    -g, --[no-]gui        use `diff.guitool` instead of `diff.tool`\n    -d, --[no-]dir-diff   perform a full-directory diff\n    -y, --no-prompt       do not prompt before launching a diff tool\n    --[no-]symlinks       use symlinks in dir-diff mode\n    -t, --[no-]tool <tool>\n                          use the specified diff tool\n    --[no-]tool-help      print a list of diff tools that may be used with `--tool`\n    --[no-]trust-exit-code\n                          make 'git-difftool' exit when an invoked diff tool returns a non-zero exit code\n    -x, --[no-]extcmd <command>\n                          specify a custom command for viewing diffs\n    --no-index            passed to `diff`\n    --index               opposite of --no-index\n\n";

const FAST_IMPORT_USAGE: &str = "usage: git fast-import [--date-format=<f>] [--max-pack-size=<n>] [--big-file-threshold=<n>] [--depth=<n>] [--active-branches=<n>] [--export-marks=<marks.file>]\n";

const FAST_EXPORT_USAGE: &str = "usage: git fast-export [<rev-list-opts>]\n\n    --[no-]progress <n>   show progress after <n> objects\n    --[no-]signed-tags <mode>\n                          select handling of signed tags\n    --[no-]signed-commits <mode>\n                          select handling of signed commits\n    --[no-]tag-of-filtered-object <mode>\n                          select handling of tags that tag filtered objects\n    --[no-]reencode <mode>\n                          select handling of commit messages in an alternate encoding\n    --[no-]export-marks <file>\n                          dump marks to this file\n    --[no-]import-marks <file>\n                          import marks from this file\n    --[no-]import-marks-if-exists <file>\n                          import marks from this file if it exists\n    --[no-]fake-missing-tagger\n                          fake a tagger when tags lack one\n    --[no-]full-tree      output full tree for each commit\n    --[no-]use-done-feature\n                          use the done feature to terminate the stream\n    --no-data             skip output of blob data\n    --data                opposite of --no-data\n    --[no-]refspec <refspec>\n                          apply refspec to exported refs\n    --[no-]anonymize      anonymize output\n    --anonymize-map <from:to>\n                          convert <from> to <to> in anonymized output\n    --[no-]reference-excluded-parents\n                          reference parents which are not in fast-export stream by object id\n    --[no-]show-original-ids\n                          show original object ids of blobs/commits\n    --[no-]mark-tags      label tags with mark ids\n\n";

const FETCH_USAGE: &str = "usage: git fetch [<options>] [<repository> [<refspec>...]]\n   or: git fetch [<options>] <group>\n   or: git fetch --multiple [<options>] [(<repository>|<group>)...]\n   or: git fetch --all [<options>]\n\n    -v, --[no-]verbose    be more verbose\n    -q, --[no-]quiet      be more quiet\n    --[no-]all            fetch from all remotes\n    --[no-]set-upstream   set upstream for git pull/fetch\n    -a, --[no-]append     append to .git/FETCH_HEAD instead of overwriting\n    --[no-]atomic         use atomic transaction to update references\n    --[no-]upload-pack <path>\n                          path to upload pack on remote end\n    -f, --[no-]force      force overwrite of local reference\n    -m, --[no-]multiple   fetch from multiple remotes\n    -t, --[no-]tags       fetch all tags and associated objects\n    -n                    do not fetch all tags (--no-tags)\n    -j, --[no-]jobs <n>   number of submodules fetched in parallel\n    --[no-]prefetch       modify the refspec to place all refs within refs/prefetch/\n    -p, --[no-]prune      prune remote-tracking branches no longer on remote\n    -P, --[no-]prune-tags prune local tags no longer on remote and clobber changed tags\n    --[no-]recurse-submodules[=<on-demand>]\n                          control recursive fetching of submodules\n    --[no-]dry-run        dry run\n    --[no-]porcelain      machine-readable output\n    --[no-]write-fetch-head\n                          write fetched references to the FETCH_HEAD file\n    -k, --[no-]keep       keep downloaded pack\n    -u, --[no-]update-head-ok\n                          allow updating of HEAD ref\n    --[no-]progress       force progress reporting\n    --[no-]depth <depth>  deepen history of shallow clone\n    --[no-]shallow-since <time>\n                          deepen history of shallow repository based on time\n    --[no-]shallow-exclude <ref>\n                          deepen history of shallow clone, excluding ref\n    --[no-]deepen <n>     deepen history of shallow clone\n    --unshallow           convert to a complete repository\n    --refetch             re-fetch without negotiating common commits\n    --[no-]update-shallow accept refs that update .git/shallow\n    --refmap <refmap>     specify fetch refmap\n    -o, --[no-]server-option <server-specific>\n                          option to transmit\n    -4, --ipv4            use IPv4 addresses only\n    -6, --ipv6            use IPv6 addresses only\n    --[no-]negotiation-tip <revision>\n                          report that we have only objects reachable from this object\n    --[no-]negotiate-only do not fetch a packfile; instead, print ancestors of negotiation tips\n    --[no-]filter <args>  object filtering\n    --[no-]auto-maintenance\n                          run 'maintenance --auto' after fetching\n    --[no-]auto-gc        run 'maintenance --auto' after fetching\n    --[no-]show-forced-updates\n                          check for forced-updates on all updated branches\n    --[no-]write-commit-graph\n                          write the commit-graph after fetching\n    --[no-]stdin          accept refspecs from stdin\n\n";

const FETCH_PACK_USAGE: &str = "usage: git fetch-pack [--all] [--stdin] [--quiet | -q] [--keep | -k] [--thin] [--include-tag] [--upload-pack=<git-upload-pack>] [--depth=<n>] [--no-progress] [--diag-url] [-v] [<host>:]<directory> [<refs>...]\n";

const FSMONITOR_DAEMON_USAGE: &str = "usage: git fsmonitor--daemon start [<options>]\n   or: git fsmonitor--daemon run [<options>]\n   or: git fsmonitor--daemon stop\n   or: git fsmonitor--daemon status\n\n    --[no-]detach         detach from console\n    --[no-]ipc-threads <n>\n                          use <n> ipc worker threads\n    --[no-]start-timeout <n>\n                          max seconds to wait for background daemon startup\n\n";

const FORMAT_PATCH_USAGE: &str = "usage: git format-patch [<options>] [<since> | <revision-range>]\n\n    -n, --[no-]numbered   use [PATCH n/m] even with a single patch\n    -N, --no-numbered     use [PATCH] even with multiple patches\n    -s, --[no-]signoff    add a Signed-off-by trailer\n    --[no-]stdout         print patches to standard out\n    --[no-]cover-letter   generate a cover letter\n    --[no-]numbered-files use simple number sequence for output file names\n    --[no-]suffix <sfx>   use <sfx> instead of '.patch'\n    --[no-]start-number <n>\n                          start numbering patches at <n> instead of 1\n    -v, --[no-]reroll-count <reroll-count>\n                          mark the series as Nth re-roll\n    --[no-]filename-max-length <n>\n                          max length of output filename\n    --[no-]rfc[=<rfc>]    add <rfc> (default 'RFC') before 'PATCH'\n    --[no-]cover-from-description <cover-from-description-mode>\n                          generate parts of a cover letter based on a branch's description\n    --[no-]description-file <file>\n                          use branch description from file\n    --subject-prefix <prefix>\n                          use [<prefix>] instead of [PATCH]\n    -o, --output-directory <dir>\n                          store resulting files in <dir>\n    -k, --keep-subject    don't strip/add [PATCH]\n    --no-binary           don't output binary diffs\n    --binary              opposite of --no-binary\n    --[no-]zero-commit    output all-zero hash in From header\n    --[no-]ignore-if-in-upstream\n                          don't include a patch matching a commit upstream\n    -p, --no-stat         show patch format instead of default (patch + stat)\n\nMessaging\n    --[no-]add-header <header>\n                          add email header\n    --[no-]to <email>     add To: header\n    --[no-]cc <email>     add Cc: header\n    --[no-]from[=<ident>] set From address to <ident> (or committer ident if absent)\n    --[no-]in-reply-to <message-id>\n                          make first mail a reply to <message-id>\n    --[no-]attach[=<boundary>]\n                          attach the patch\n    --inline[=<boundary>] inline the patch\n    --[no-]thread[=<style>]\n                          enable message threading, styles: shallow, deep\n    --[no-]signature <signature>\n                          add a signature\n    --[no-]base <base-commit>\n                          add prerequisite tree info to the patch series\n    --[no-]signature-file <file>\n                          add a signature from a file\n    -q, --[no-]quiet      don't print the patch filenames\n    --[no-]progress       show progress while generating patches\n    --[no-]interdiff <rev>\n                          show changes against <rev> in cover letter or single patch\n    --[no-]range-diff <refspec>\n                          show changes against <refspec> in cover letter or single patch\n    --[no-]creation-factor <n>\n                          percentage by which creation is weighted\n    --[no-]force-in-body-from\n                          show in-body From: even if identical to the e-mail header\n\n";

const FMT_MERGE_MSG_USAGE: &str = "usage: git fmt-merge-msg [-m <message>] [--log[=<n>] | --no-log] [--file <file>]\n\n    --[no-]log[=<n>]      populate log with at most <n> entries from shortlog\n    -m, --[no-]message <text>\n                          use <text> as start of message\n    --[no-]into-name <name>\n                          use <name> instead of the real target branch\n    -F, --[no-]file <file>\n                          file to read from\n\n";

const FOR_EACH_REF_USAGE: &str = "usage: git for-each-ref [--count=<count>] [--shell|--perl|--python|--tcl]\n\t[(--sort=<key>)...] [--format=<format>]\n\t[--include-root-refs] [--points-at=<object>]\n\t[--merged[=<object>]] [--no-merged[=<object>]]\n\t[--contains[=<object>]] [--no-contains[=<object>]]\n\t[(--exclude=<pattern>)...] [--start-after=<marker>]\n\t[ --stdin | (<pattern>...)]\n\n    -s, --[no-]shell      quote placeholders suitably for shells\n    -p, --[no-]perl       quote placeholders suitably for perl\n    --[no-]python         quote placeholders suitably for python\n    --[no-]tcl            quote placeholders suitably for Tcl\n    --[no-]omit-empty     do not output a newline after empty formatted refs\n\n    --[no-]count <n>      show only <n> matched refs\n    --[no-]format <format>\n                          format to use for the output\n    --[no-]color[=<when>] respect format colors\n    --[no-]exclude <pattern>\n                          exclude refs which match pattern\n    --[no-]sort <key>     field name to sort on\n    --[no-]points-at <object>\n                          print only refs which points at the given object\n    --merged <commit>     print only refs that are merged\n    --no-merged <commit>  print only refs that are not merged\n    --contains <commit>   print only refs which contain the commit\n    --no-contains <commit>\n                          print only refs which don't contain the commit\n    --[no-]ignore-case    sorting and filtering are case insensitive\n    --[no-]stdin          read reference patterns from stdin\n    --[no-]include-root-refs\n                          also include HEAD ref and pseudorefs\n\n";

const FOR_EACH_REPO_USAGE: &str = "usage: git for-each-repo --config=<config> [--] <arguments>\n\n    --[no-]config <config>\n                          config key storing a list of repository paths\n    --[no-]keep-going     keep going even if command fails in a repository\n\n";

const FSCK_USAGE: &str = "usage: git fsck [--tags] [--root] [--unreachable] [--cache] [--no-reflogs]\n                [--[no-]full] [--strict] [--verbose] [--lost-found]\n                [--[no-]dangling] [--[no-]progress] [--connectivity-only]\n                [--[no-]name-objects] [--[no-]references] [<object>...]\n\n    -v, --[no-]verbose    be verbose\n    --[no-]unreachable    show unreachable objects\n    --[no-]dangling       show dangling objects\n    --[no-]tags           report tags\n    --[no-]root           report root nodes\n    --[no-]cache          make index objects head nodes\n    --[no-]reflogs        make reflogs head nodes (default)\n    --[no-]full           also consider packs and alternate objects\n    --[no-]connectivity-only\n                          check only connectivity\n    --[no-]strict         enable more strict checking\n    --[no-]lost-found     write dangling objects in .git/lost-found\n    --[no-]progress       show progress\n    --[no-]name-objects   show verbose names for reachable objects\n    --[no-]references     check reference database consistency\n\n";

const GC_USAGE: &str = "usage: git gc [<options>]\n\n    -q, --[no-]quiet      suppress progress reporting\n    --[no-]prune[=<date>] prune unreferenced objects\n    --[no-]cruft          pack unreferenced objects separately\n    --max-cruft-size <n>  with --cruft, limit the size of new cruft packs\n    --[no-]aggressive     be more thorough (increased runtime)\n    --[no-]auto           enable auto-gc mode\n    --[no-]detach         perform garbage collection in the background\n    --[no-]force          force running gc even if there may be another gc running\n    --[no-]keep-largest-pack\n                          repack all other packs except the largest pack\n    --[no-]expire-to <dir>\n                          pack prefix to store a pack containing pruned objects\n\n";

const GET_TAR_COMMIT_ID_USAGE: &str = "usage: git get-tar-commit-id\n";

const LOG_USAGE: &str = "usage: git log [<options>] [<revision-range>] [[--] <path>...]\n   or: git show [<options>] <object>...\n\n    -q, --[no-]quiet      suppress diff output\n    --[no-]source         show source\n    --[no-]use-mailmap    use mail map file\n    --[no-]mailmap        alias of --use-mailmap\n    --clear-decorations   clear all previously-defined decoration filters\n    --[no-]decorate-refs <pattern>\n                          only decorate refs that match <pattern>\n    --[no-]decorate-refs-exclude <pattern>\n                          do not decorate refs that match <pattern>\n    --[no-]decorate[=...] decorate options\n    -L <range:file>       trace the evolution of line range <start>,<end> or function :<funcname> in <file>\n\n";

const MERGE_SUBTREE_USAGE: &str = "usage: git merge-subtree <base>... -- <head> <remote> ...\n";

const MKTAG_USAGE: &str =
    "usage: git mktag\n\n    --[no-]strict         enable more strict checking\n\n";

const MKTREE_USAGE: &str = "usage: git mktree [-z] [--missing] [--batch]\n\n    -z                    input is NUL terminated\n    --[no-]missing        allow missing objects\n    --[no-]batch          allow creation of more than one tree\n\n";

const MV_USAGE: &str = "usage: git mv [-v] [-f] [-n] [-k] <source> <destination>\n   or: git mv [-v] [-f] [-n] [-k] <source>... <destination-directory>\n\n    -v, --[no-]verbose    be verbose\n    -n, --[no-]dry-run    dry run\n    -f, --[no-]force      force move/rename even if target exists\n    -k                    skip move/rename errors\n    --[no-]sparse         allow updating entries outside of the sparse-checkout cone\n\n";

const NAME_REV_USAGE: &str = "usage: git name-rev [<options>] <commit>...\n   or: git name-rev [<options>] --all\n   or: git name-rev [<options>] --annotate-stdin\n\n    --[no-]name-only      print only ref-based names (no object names)\n    --[no-]tags           only use tags to name the commits\n    --[no-]refs <pattern> only use refs matching <pattern>\n    --[no-]exclude <pattern>\n                          ignore refs matching <pattern>\n\n    --[no-]all            list all commits reachable from all refs\n    --[no-]annotate-stdin annotate text from stdin\n    --[no-]undefined      allow to print `undefined` names (default)\n    --[no-]always         show abbreviated commit object as fallback\n\n";

const PRUNE_USAGE: &str = "usage: git prune [-n] [-v] [--progress] [--expire <time>] [--] [<head>...]\n\n    -n, --[no-]dry-run    do not remove, show only\n    -v, --[no-]verbose    report pruned objects\n    --[no-]progress       show progress\n    --[no-]expire <expiry-date>\n                          expire objects older than <time>\n    --[no-]exclude-promisor-objects\n                          limit traversal to objects outside promisor packfiles\n\n";

const PRUNE_PACKED_USAGE: &str = "usage: git prune-packed [-n | --dry-run] [-q | --quiet]\n\n    -n, --[no-]dry-run    dry run\n    -q, --[no-]quiet      be quiet\n\n";

const PULL_USAGE: &str = "usage: git pull [<options>] [<repository> [<refspec>...]]\n\n    -v, --[no-]verbose    be more verbose\n    -q, --[no-]quiet      be more quiet\n    --[no-]progress       force progress reporting\n    --[no-]recurse-submodules[=<on-demand>]\n                          control for recursive fetching of submodules\n\nOptions related to merging\n    -r, --[no-]rebase[=(false|true|merges|interactive)]\n                          incorporate changes by rebasing rather than merging\n    -n                    do not show a diffstat at the end of the merge\n    --[no-]stat           show a diffstat at the end of the merge\n    --[no-]log[=<n>]      add (at most <n>) entries from shortlog to merge commit message\n    --[no-]signoff[=...]  add a Signed-off-by trailer\n    --[no-]squash         create a single commit instead of doing a merge\n    --[no-]commit         perform a commit if the merge succeeds (default)\n    --[no-]edit           edit message before committing\n    --[no-]cleanup <mode> how to strip spaces and #comments from message\n    --[no-]ff             allow fast-forward\n    --ff-only             abort if fast-forward is not possible\n    --[no-]verify         control use of pre-merge-commit and commit-msg hooks\n    --[no-]verify-signatures\n                          verify that the named commit has a valid GPG signature\n    --[no-]autostash      automatically stash/stash pop before and after\n    -s, --[no-]strategy <strategy>\n                          merge strategy to use\n    -X, --[no-]strategy-option <option=value>\n                          option for selected merge strategy\n    -S, --[no-]gpg-sign[=<key-id>]\n                          GPG sign commit\n    --[no-]allow-unrelated-histories\n                          allow merging unrelated histories\n\nOptions related to fetching\n    --[no-]all            fetch from all remotes\n    -a, --[no-]append     append to .git/FETCH_HEAD instead of overwriting\n    --[no-]upload-pack <path>\n                          path to upload pack on remote end\n    -f, --[no-]force      force overwrite of local branch\n    -t, --[no-]tags       fetch all tags and associated objects\n    -p, --[no-]prune      prune remote-tracking branches no longer on remote\n    -j, --[no-]jobs[=<n>] number of submodules pulled in parallel\n    --[no-]dry-run        dry run\n    -k, --[no-]keep       keep downloaded pack\n    --[no-]depth <depth>  deepen history of shallow clone\n    --[no-]shallow-since <time>\n                          deepen history of shallow repository based on time\n    --[no-]shallow-exclude <ref>\n                          deepen history of shallow clone, excluding ref\n    --[no-]deepen <n>     deepen history of shallow clone\n    --unshallow           convert to a complete repository\n    --[no-]update-shallow accept refs that update .git/shallow\n    --refmap <refmap>     specify fetch refmap\n    -o, --[no-]server-option <server-specific>\n                          option to transmit\n    -4, --[no-]ipv4       use IPv4 addresses only\n    -6, --[no-]ipv6       use IPv6 addresses only\n    --[no-]negotiation-tip <revision>\n                          report that we have only objects reachable from this object\n    --[no-]show-forced-updates\n                          check for forced-updates on all updated branches\n    --[no-]set-upstream   set upstream for git pull/fetch\n\n";

const NOTES_USAGE: &str = "usage: git notes [--ref <notes-ref>] [list [<object>]]\n   or: git notes [--ref <notes-ref>] add [-f] [--allow-empty] [--[no-]separator|--separator=<paragraph-break>] [--[no-]stripspace] [-m <msg> | -F <file> | (-c | -C) <object>] [<object>] [-e]\n   or: git notes [--ref <notes-ref>] copy [-f] <from-object> <to-object>\n   or: git notes [--ref <notes-ref>] append [--allow-empty] [--[no-]separator|--separator=<paragraph-break>] [--[no-]stripspace] [-m <msg> | -F <file> | (-c | -C) <object>] [<object>] [-e]\n   or: git notes [--ref <notes-ref>] edit [--allow-empty] [<object>]\n   or: git notes [--ref <notes-ref>] show [<object>]\n   or: git notes [--ref <notes-ref>] merge [-v | -q] [-s <strategy>] <notes-ref>\n   or: git notes merge --commit [-v | -q]\n   or: git notes merge --abort [-v | -q]\n   or: git notes [--ref <notes-ref>] remove [<object>...]\n   or: git notes [--ref <notes-ref>] prune [-n] [-v]\n   or: git notes [--ref <notes-ref>] get-ref\n\n    --[no-]ref <notes-ref>\n                          use notes from <notes-ref>\n\n";

const PACK_REFS_USAGE: &str = "usage: git pack-refs [--all] [--no-prune] [--auto] [--include <pattern>] [--exclude <pattern>]\n\n    --[no-]all            pack everything\n    --[no-]prune          prune loose refs (default)\n    --[no-]auto           auto-pack refs as needed\n    --[no-]include <pattern>\n                          references to include\n    --[no-]exclude <pattern>\n                          references to exclude\n\n";

const PACK_OBJECTS_USAGE: &str = "usage: git pack-objects [-q | --progress | --all-progress] [--all-progress-implied]\n\t   [--no-reuse-delta] [--delta-base-offset] [--non-empty]\n\t   [--local] [--incremental] [--window=<n>] [--depth=<n>]\n\t   [--revs [--unpacked | --all]] [--keep-pack=<pack-name>]\n\t   [--cruft] [--cruft-expiration=<time>]\n\t   [--stdout [--filter=<filter-spec>] | <base-name>]\n\t   [--shallow] [--keep-true-parents] [--[no-]sparse]\n\t   [--name-hash-version=<n>] [--path-walk] < <object-list>\n\n    -q, --[no-]quiet      do not show progress meter\n    --[no-]progress       show progress meter\n    --[no-]all-progress   show progress meter during object writing phase\n    --[no-]all-progress-implied\n                          similar to --all-progress when progress meter is shown\n    --index-version <version>[,<offset>]\n                          write the pack index file in the specified idx format version\n    --max-pack-size <n>   maximum size of each output pack file\n    --[no-]local          ignore borrowed objects from alternate object store\n    --[no-]incremental    ignore packed objects\n    --[no-]window <n>     limit pack window by objects\n    --window-memory <n>   limit pack window by memory in addition to object limit\n    --[no-]depth <n>      maximum length of delta chain allowed in the resulting pack\n    --[no-]reuse-delta    reuse existing deltas\n    --[no-]reuse-object   reuse existing objects\n    --[no-]delta-base-offset\n                          use OFS_DELTA objects\n    --[no-]threads <n>    use threads when searching for best delta matches\n    --[no-]non-empty      do not create an empty pack output\n    --[no-]revs           read revision arguments from standard input\n    --unpacked            limit the objects to those that are not yet packed\n    --all                 include objects reachable from any reference\n    --reflog              include objects referred by reflog entries\n    --indexed-objects     include objects referred to by the index\n    --[no-]stdin-packs    read packs from stdin\n    --[no-]stdout         output pack to stdout\n    --[no-]include-tag    include tag objects that refer to objects to be packed\n    --[no-]keep-unreachable\n                          keep unreachable objects\n    --[no-]pack-loose-unreachable\n                          pack loose unreachable objects\n    --[no-]unpack-unreachable[=<time>]\n                          unpack unreachable objects newer than <time>\n    --[no-]cruft          create a cruft pack\n    --[no-]cruft-expiration[=<time>]\n                          expire cruft objects older than <time>\n    --[no-]sparse         use the sparse reachability algorithm\n    --[no-]thin           create thin packs\n    --[no-]shallow        create packs suitable for shallow fetches\n    --[no-]honor-pack-keep\n                          ignore packs that have companion .keep file\n    --[no-]keep-pack <name>\n                          ignore this pack\n    --[no-]compression <n>\n                          pack compression level\n    --[no-]keep-true-parents\n                          do not hide commits by grafts\n    --[no-]use-bitmap-index\n                          use a bitmap index if available to speed up counting objects\n    --[no-]write-bitmap-index\n                          write a bitmap index together with the pack index\n    --[no-]filter <args>  object filtering\n    --missing <action>    handling for missing objects\n    --[no-]exclude-promisor-objects\n                          do not pack objects in promisor packfiles\n    --[no-]exclude-promisor-objects-best-effort\n                          implies --missing=allow-any\n    --[no-]delta-islands  respect islands during delta compression\n    --[no-]uri-protocol <protocol>\n                          exclude any configured uploadpack.blobpackfileuri with this protocol\n    --[no-]name-hash-version <n>\n                          use the specified name-hash function to group similar objects\n\n";

const PACK_REDUNDANT_USAGE: &str =
    "usage: git pack-redundant [--verbose] [--alt-odb] (--all | <pack-filename>...)\n";

const PATCH_ID_USAGE: &str = "usage: git patch-id [--stable | --unstable | --verbatim]\n\n    --unstable            use the unstable patch-id algorithm\n    --stable              use the stable patch-id algorithm\n    --verbatim            don't strip whitespace from the patch\n\n";

const PICKAXE_USAGE: &str = "usage: git blame [<options>] [<rev-opts>] [<rev>] [--] <file>\n\n    <rev-opts> are documented in git-rev-list(1)\n\n    --[no-]incremental    show blame entries as we find them, incrementally\n    -b                    do not show object names of boundary commits (Default: off)\n    --[no-]root           do not treat root commits as boundaries (Default: off)\n    --[no-]show-stats     show work cost statistics\n    --[no-]progress       force progress reporting\n    --[no-]score-debug    show output score for blame entries\n    -f, --[no-]show-name  show original filename (Default: auto)\n    -n, --[no-]show-number\n                          show original linenumber (Default: off)\n    -p, --[no-]porcelain  show in a format designed for machine consumption\n    --[no-]line-porcelain show porcelain format with per-line commit information\n    -c                    use the same output mode as git-annotate (Default: off)\n    -t                    show raw timestamp (Default: off)\n    -l                    show long commit SHA1 (Default: off)\n    -s                    suppress author name and timestamp (Default: off)\n    -e, --[no-]show-email show author email instead of name (Default: off)\n    -w                    ignore whitespace differences\n    --[no-]ignore-rev <rev>\n                          ignore <rev> when blaming\n    --[no-]ignore-revs-file <file>\n                          ignore revisions from <file>\n    --[no-]color-lines    color redundant metadata from previous line differently\n    --[no-]color-by-age   color lines by age\n    --[no-]minimal        spend extra cycles to find better match\n    -S <file>             use revisions from <file> instead of calling git-rev-list\n    --[no-]contents <file>\n                          use <file>'s contents as the final image\n    -C[<score>]           find line copies within and across files\n    -M[<score>]           find line movements within and across files\n    -L <range>            process only line range <start>,<end> or function :<funcname>\n    --[no-]abbrev[=<n>]   use <n> digits to display object names\n\n";

const RANGE_DIFF_USAGE: &str = "usage: git range-diff [<options>] <old-base>..<old-tip> <new-base>..<new-tip>\n   or: git range-diff [<options>] <old-tip>...<new-tip>\n   or: git range-diff [<options>] <base> <old-tip> <new-tip>\n\n    --[no-]creation-factor <n>\n                          percentage by which creation is weighted\n    --no-dual-color       use simple diff colors\n    --dual-color          opposite of --no-dual-color\n    --[no-]notes[=<notes>]\n                          passed to 'git log'\n    --[no-]diff-merges <style>\n                          passed to 'git log'\n    --[no-]remerge-diff   passed to 'git log'\n    --[no-]left-only      only emit output related to the first range\n    --[no-]right-only     only emit output related to the second range\n\nDiff output format options\n    -p, --patch           generate patch\n    -s, --no-patch        suppress diff output\n    -u                    generate patch\n    -U, --unified[=<n>]   generate diffs with <n> lines context\n    -W, --[no-]function-context\n                          generate diffs with <n> lines context\n    --raw                 generate the diff in raw format\n    --patch-with-raw      synonym for '-p --raw'\n    --patch-with-stat     synonym for '-p --stat'\n    --numstat             machine friendly --stat\n    --shortstat           output only the last line of --stat\n    -X, --dirstat[=<param1>,<param2>...]\n                          output the distribution of relative amount of changes for each sub-directory\n    --cumulative          synonym for --dirstat=cumulative\n    --dirstat-by-file[=<param1>,<param2>...]\n                          synonym for --dirstat=files,<param1>,<param2>...\n    --check               warn if changes introduce conflict markers or whitespace errors\n    --summary             condensed summary such as creations, renames and mode changes\n    --name-only           show only names of changed files\n    --name-status         show only names and status of changed files\n    --stat[=<width>[,<name-width>[,<count>]]]\n                          generate diffstat\n    --stat-width <width>  generate diffstat with a given width\n    --stat-name-width <width>\n                          generate diffstat with a given name width\n    --stat-graph-width <width>\n                          generate diffstat with a given graph width\n    --stat-count <count>  generate diffstat with limited lines\n    --[no-]compact-summary\n                          generate compact summary in diffstat\n    --binary              output a binary diff that can be applied\n    --[no-]full-index     show full pre- and post-image object names on the \"index\" lines\n    --[no-]color[=<when>] show colored diff\n    --ws-error-highlight <kind>\n                          highlight whitespace errors in the 'context', 'old' or 'new' lines in the diff\n    -z                    do not munge pathnames and use NULs as output field terminators in --raw or --numstat\n    --[no-]abbrev[=<n>]   use <n> digits to display object names\n    --src-prefix <prefix> show the given source prefix instead of \"a/\"\n    --dst-prefix <prefix> show the given destination prefix instead of \"b/\"\n    --line-prefix <prefix>\n                          prepend an additional prefix to every line of output\n    --no-prefix           do not show any source or destination prefix\n    --default-prefix      use default prefixes a/ and b/\n    --inter-hunk-context <n>\n                          show context between diff hunks up to the specified number of lines\n    --output-indicator-new <char>\n                          specify the character to indicate a new line instead of '+'\n    --output-indicator-old <char>\n                          specify the character to indicate an old line instead of '-'\n    --output-indicator-context <char>\n                          specify the character to indicate a context instead of ' '\n\nDiff rename options\n    -B, --break-rewrites[=<n>[/<m>]]\n                          break complete rewrite changes into pairs of delete and create\n    -M, --find-renames[=<n>]\n                          detect renames\n    -D, --irreversible-delete\n                          omit the preimage for deletes\n    -C, --find-copies[=<n>]\n                          detect copies\n    --[no-]find-copies-harder\n                          use unmodified files as source to find copies\n    --no-renames          disable rename detection\n    --[no-]rename-empty   use empty blobs as rename source\n    --[no-]follow         continue listing the history of a file beyond renames\n    -l <n>                prevent rename/copy detection if the number of rename/copy targets exceeds given limit\n\nDiff algorithm options\n    --minimal             produce the smallest possible diff\n    -w, --ignore-all-space\n                          ignore whitespace when comparing lines\n    -b, --ignore-space-change\n                          ignore changes in amount of whitespace\n    --ignore-space-at-eol ignore changes in whitespace at EOL\n    --ignore-cr-at-eol    ignore carrier-return at the end of line\n    --ignore-blank-lines  ignore changes whose lines are all blank\n    -I, --[no-]ignore-matching-lines <regex>\n                          ignore changes whose all lines match <regex>\n    --[no-]indent-heuristic\n                          heuristic to shift diff hunk boundaries for easy reading\n    --patience            generate diff using the \"patience diff\" algorithm\n    --histogram           generate diff using the \"histogram diff\" algorithm\n    --diff-algorithm <algorithm>\n                          choose a diff algorithm\n    --anchored <text>     generate diff using the \"anchored diff\" algorithm\n    --word-diff[=<mode>]  show word diff, using <mode> to delimit changed words\n    --word-diff-regex <regex>\n                          use <regex> to decide what a word is\n    --color-words[=<regex>]\n                          equivalent to --word-diff=color --word-diff-regex=<regex>\n    --[no-]color-moved[=<mode>]\n                          moved lines of code are colored differently\n    --[no-]color-moved-ws <mode>\n                          how white spaces are ignored in --color-moved\n\nOther diff options\n    --[no-]relative[=<prefix>]\n                          when run from subdir, exclude changes outside and show relative paths\n    -a, --[no-]text       treat all files as text\n    -R                    swap two inputs, reverse the diff\n    --[no-]exit-code      exit with 1 if there were differences, 0 otherwise\n    --[no-]quiet          disable all output of the program\n    --[no-]ext-diff       allow an external diff helper to be executed\n    --[no-]textconv       run external text conversion filters when comparing binary files\n    --ignore-submodules[=<when>]\n                          ignore changes to submodules in the diff generation\n    --submodule[=<format>]\n                          specify how differences in submodules are shown\n    --ita-invisible-in-index\n                          hide 'git add -N' entries from the index\n    --ita-visible-in-index\n                          treat 'git add -N' entries as real in the index\n    -S <string>           look for differences that change the number of occurrences of the specified string\n    -G <regex>            look for differences that change the number of occurrences of the specified regex\n    --pickaxe-all         show all changes in the changeset with -S or -G\n    --pickaxe-regex       treat <string> in -S as extended POSIX regular expression\n    -O <file>             control the order in which files appear in the output\n    --rotate-to <path>    show the change in the specified path first\n    --skip-to <path>      skip the output to the specified path\n    --find-object <object-id>\n                          look for differences that change the number of occurrences of the specified object\n    --diff-filter [(A|C|D|M|R|T|U|X|B)...[*]]\n                          select files by diff type\n    --output <file>       output to a specific file\n\n";

const READ_TREE_USAGE: &str = "usage: git read-tree [(-m [--trivial] [--aggressive] | --reset | --prefix=<prefix>)\n                     [-u | -i]] [--index-output=<file>] [--no-sparse-checkout]\n                     (--empty | <tree-ish1> [<tree-ish2> [<tree-ish3>]])\n\n    --index-output <file> write resulting index to <file>\n    --[no-]empty          only empty the index\n    -v, --[no-]verbose    be verbose\n\nMerging\n    -m                    perform a merge in addition to a read\n    --[no-]trivial        3-way merge if no file level merging required\n    --[no-]aggressive     3-way merge in presence of adds and removes\n    --[no-]reset          same as -m, but discard unmerged entries\n    --prefix <subdirectory>/\n                          read the tree into the index under <subdirectory>/\n    -u                    update working tree with merge result\n    --exclude-per-directory <gitignore>\n                          allow explicitly ignored files to be overwritten\n    -i                    don't check the working tree after merging\n    -n, --[no-]dry-run    don't update the index or the work tree\n    --no-sparse-checkout  skip applying sparse checkout filter\n    --sparse-checkout     opposite of --no-sparse-checkout\n    --[no-]debug-unpack   debug unpack-trees\n    --[no-]recurse-submodules[=<checkout>]\n                          control recursive updating of submodules\n    -q, --[no-]quiet      suppress feedback messages\n\n";

const REPACK_USAGE: &str = "usage: git repack [-a] [-A] [-d] [-f] [-F] [-l] [-n] [-q] [-b] [-m]\n       [--window=<n>] [--depth=<n>] [--threads=<n>] [--keep-pack=<pack-name>]\n       [--write-midx] [--name-hash-version=<n>]\n\n    -a                    pack everything in a single pack\n    -A                    same as -a, and turn unreachable objects loose\n    --[no-]cruft          same as -a, pack unreachable cruft objects separately\n    --[no-]cruft-expiration <approxidate>\n                          with --cruft, expire objects older than this\n    --combine-cruft-below-size <n>\n                          with --cruft, only repack cruft packs smaller than this\n    --max-cruft-size <n>  with --cruft, limit the size of new cruft packs\n    -d                    remove redundant packs, and run git-prune-packed\n    -f                    pass --no-reuse-delta to git-pack-objects\n    -F                    pass --no-reuse-object to git-pack-objects\n    --[no-]name-hash-version <n>\n                          specify the name hash version to use for grouping similar objects by path\n    -n                    do not run git-update-server-info\n    -q, --[no-]quiet      be quiet\n    -l, --[no-]local      pass --local to git-pack-objects\n    -b, --[no-]write-bitmap-index\n                          write bitmap index\n    -i, --[no-]delta-islands\n                          pass --delta-islands to git-pack-objects\n    --[no-]unpack-unreachable <approxidate>\n                          with -A, do not loosen objects older than this\n    -k, --[no-]keep-unreachable\n                          with -a, repack unreachable objects\n    --[no-]window <n>     size of the window used for delta compression\n    --[no-]window-memory <bytes>\n                          same as the above, but limit memory size instead of entries count\n    --[no-]depth <n>      limits the maximum delta depth\n    --[no-]threads <n>    limits the maximum number of threads\n    --max-pack-size <n>   maximum size of each packfile\n    --[no-]filter <args>  object filtering\n    --[no-]pack-kept-objects\n                          repack objects in packs marked with .keep\n    --[no-]keep-pack <name>\n                          do not repack this pack\n    -g, --[no-]geometric <n>\n                          find a geometric progression with factor <N>\n    -m, --[no-]write-midx write a multi-pack index of the resulting packs\n    --[no-]expire-to <dir>\n                          pack prefix to store a pack containing pruned objects\n    --[no-]filter-to <dir>\n                          pack prefix to store a pack containing filtered out objects\n\n";

const REPLACE_USAGE: &str = "usage: git replace [-f] <object> <replacement>\n   or: git replace [-f] --edit <object>\n   or: git replace [-f] --graft <commit> [<parent>...]\n   or: git replace [-f] --convert-graft-file\n   or: git replace -d <object>...\n   or: git replace [--format=<format>] [-l [<pattern>]]\n\n    -l, --list            list replace refs\n    -d, --delete          delete replace refs\n    -e, --edit            edit existing object\n    -g, --graft           change a commit's parents\n    --convert-graft-file  convert existing graft file\n    -f, --[no-]force      replace the ref if it exists\n    --[no-]raw            do not pretty-print contents for --edit\n    --[no-]format <format>\n                          use this format\n\n";

const SHORTLOG_USAGE: &str = "usage: git shortlog [<options>] [<revision-range>] [[--] <path>...]\n   or: git log --pretty=short | git shortlog [<options>]\n\n    -c, --[no-]committer  group by committer rather than author\n    -n, --[no-]numbered   sort output according to the number of commits per author\n    -s, --[no-]summary    suppress commit descriptions, only provides commit count\n    -e, --[no-]email      show the email address of each author\n    -w[<w>[,<i1>[,<i2>]]] linewrap output\n    --[no-]group <field>  group by field\n\n";

const TAG_USAGE: &str = "usage: git tag [-a | -s | -u <key-id>] [-f] [-m <msg> | -F <file>] [-e]\n               [(--trailer <token>[(=|:)<value>])...]\n               <tagname> [<commit> | <object>]\n   or: git tag -d <tagname>...\n   or: git tag [-n[<num>]] -l [--contains <commit>] [--no-contains <commit>]\n               [--points-at <object>] [--column[=<options>] | --no-column]\n               [--create-reflog] [--sort=<key>] [--format=<format>]\n               [--merged <commit>] [--no-merged <commit>] [<pattern>...]\n   or: git tag -v [--format=<format>] <tagname>...\n\n    -l, --list            list tag names\n    -n[<n>]               print <n> lines of each tag message\n    -d, --delete          delete tags\n    -v, --verify          verify tags\n\nTag creation options\n    -a, --[no-]annotate   annotated tag, needs a message\n    -m, --message <message>\n                          tag message\n    -F, --[no-]file <file>\n                          read message from file\n    --trailer <trailer>   add custom trailer(s)\n    -e, --[no-]edit       force edit of tag message\n    -s, --[no-]sign       annotated and GPG-signed tag\n    --[no-]cleanup <mode> how to strip spaces and #comments from message\n    -u, --[no-]local-user <key-id>\n                          use another key to sign the tag\n    -f, --[no-]force      replace the tag if exists\n    --[no-]create-reflog  create a reflog\n\nTag listing options\n    --[no-]column[=<style>]\n                          show tag list in columns\n    --contains <commit>   print only tags that contain the commit\n    --no-contains <commit>\n                          print only tags that don't contain the commit\n    --merged <commit>     print only tags that are merged\n    --no-merged <commit>  print only tags that are not merged\n    --[no-]omit-empty     do not output a newline after empty formatted refs\n    --[no-]sort <key>     field name to sort on\n    --[no-]points-at <object>\n                          print only tags of the object\n    --[no-]format <format>\n                          format to use for the output\n    --[no-]color[=<when>] respect format colors\n    -i, --[no-]ignore-case\n                          sorting and filtering are case insensitive\n\n";

fn validate_tag_invocation_before_clap(command_args: &[String]) -> Result<()> {
    validate_known_long_options_before_clap(
        command_args,
        "tag",
        TAG_USAGE,
        &[
            "--help",
            "--delete",
            "--verify",
            "--list",
            "--no-column",
            "--force",
            "--annotate",
            "--edit",
            "--sign",
            "--no-sign",
            "--create-reflog",
            "--omit-empty",
            "--ignore-case",
        ],
        &[
            "--column",
            "--column=",
            "--color",
            "--color=",
            "--local-user",
            "--local-user=",
            "--cleanup",
            "--cleanup=",
            "--message",
            "--message=",
            "--file",
            "--file=",
            "--trailer",
            "--trailer=",
            "--contains",
            "--contains=",
            "--no-contains",
            "--no-contains=",
            "--merged",
            "--merged=",
            "--no-merged",
            "--no-merged=",
            "--sort",
            "--sort=",
            "--points-at",
            "--points-at=",
            "--format",
            "--format=",
        ],
    )
}

fn validate_known_long_options_before_clap(
    command_args: &[String],
    command_name: &str,
    usage: &str,
    allowed_exact: &[&str],
    allowed_prefixes: &[&str],
) -> Result<()> {
    if command_args.first().map(String::as_str) != Some(command_name) {
        return Ok(());
    }
    for arg in command_args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        if !arg.starts_with("--") {
            continue;
        }
        if allowed_exact.iter().any(|allowed| arg == allowed)
            || allowed_prefixes
                .iter()
                .any(|prefix| arg == prefix || arg.starts_with(prefix))
        {
            continue;
        }
        return Err(CliError::Stderr {
            code: 129,
            text: format!("error: unknown option `{}'\n{usage}", &arg[2..]),
        });
    }
    Ok(())
}

const UNPACK_FILE_USAGE: &str = "usage: git unpack-file <blob>\n";

const UNPACK_OBJECTS_USAGE: &str = "usage: git unpack-objects [-n] [-q] [-r] [--strict]\n";

const UPDATE_INDEX_USAGE: &str = "usage: git update-index [<options>] [--] [<file>...]\n\n    -q                    continue refresh even when index needs update\n    --[no-]ignore-submodules\n                          refresh: ignore submodules\n    --[no-]add            do not ignore new files\n    --[no-]replace        let files replace directories and vice-versa\n    --[no-]remove         notice files missing from worktree\n    --[no-]unmerged       refresh even if index contains unmerged entries\n    --refresh             refresh stat information\n    --really-refresh      like --refresh, but ignore assume-unchanged setting\n    --cacheinfo <mode>,<object>,<path>\n                          add the specified entry to the index\n    --chmod (+|-)x        override the executable bit of the listed files\n    --assume-unchanged    mark files as \"not changing\"\n    --no-assume-unchanged clear assumed-unchanged bit\n    --skip-worktree       mark files as \"index-only\"\n    --no-skip-worktree    clear skip-worktree bit\n    --[no-]ignore-skip-worktree-entries\n                          do not touch index-only entries\n    --[no-]info-only      add to index only; do not add content to object database\n    --[no-]force-remove   remove named paths even if present in worktree\n    -z                    with --stdin: input lines are terminated by null bytes\n    --stdin               read list of paths to be updated from standard input\n    --index-info          add entries from standard input to the index\n    --unresolve           repopulate stages #2 and #3 for the listed paths\n    -g, --again           only update entries that differ from HEAD\n    --[no-]ignore-missing ignore files missing from worktree\n    --[no-]verbose        report actions to standard output\n    --clear-resolve-undo  (for porcelains) forget saved unresolved conflicts\n    --[no-]index-version <n>\n                          write index in this format\n    --[no-]show-index-version\n                          report on-disk index format version\n    --[no-]split-index    enable or disable split index\n    --[no-]untracked-cache\n                          enable/disable untracked cache\n    --[no-]test-untracked-cache\n                          test if the filesystem supports untracked cache\n    --[no-]force-untracked-cache\n                          enable untracked cache without testing the filesystem\n    --[no-]force-write-index\n                          write out the index even if is not flagged as changed\n    --[no-]fsmonitor      enable or disable file system monitor\n    --fsmonitor-valid     mark files as fsmonitor valid\n    --no-fsmonitor-valid  clear fsmonitor valid bit\n\n";

const UPDATE_REF_USAGE: &str = "usage: git update-ref [<options>] -d <refname> [<old-oid>]\n   or: git update-ref [<options>]    <refname> <new-oid> [<old-oid>]\n   or: git update-ref [<options>] --stdin [-z] [--batch-updates]\n\n    -m <reason>           reason of the update\n    -d                    delete the reference\n    --no-deref            update <refname> not the one it points to\n    --deref               opposite of --no-deref\n    -z                    stdin has NUL-terminated arguments\n    --[no-]stdin          read updates from stdin\n    --[no-]create-reflog  create a reflog\n    -0, --[no-]batch-updates\n                          batch reference updates\n\n";

const UPDATE_SERVER_INFO_USAGE: &str = "usage: git update-server-info [-f | --force]\n\n    -f, --[no-]force      update the info files from scratch\n\n";

const UPLOAD_ARCHIVE_USAGE: &str = "usage: git upload-archive <repository>\n";

const UPLOAD_PACK_USAGE: &str = "usage: git-upload-pack [--[no-]strict] [--timeout=<n>] [--stateless-rpc]\n                       [--advertise-refs] <directory>\n\n    --[no-]stateless-rpc  quit after a single request/response exchange\n    --[no-]strict         do not try <directory>/.git/ if <directory> is no Git directory\n    --[no-]timeout <n>    interrupt transfer after <n> seconds of inactivity\n\n";

const VAR_USAGE: &str = "usage: git var (-l | <variable>)\n";

const VERSION_USAGE: &str = "usage: git version [--build-options]\n\n    --[no-]build-options  also print build options\n\n";

const VERIFY_COMMIT_USAGE: &str = "usage: git verify-commit [-v | --verbose] [--raw] <commit>...\n\n    -v, --[no-]verbose    print commit contents\n    --[no-]raw            print raw gpg status output\n\n";

const VERIFY_PACK_USAGE: &str = "usage: git verify-pack [-v | --verbose] [-s | --stat-only] [--] <pack>.idx...\n\n    -v, --[no-]verbose    verbose\n    -s, --[no-]stat-only  show statistics only\n    --[no-]object-format <hash>\n                          specify the hash algorithm to use\n\n";

const VERIFY_TAG_USAGE: &str = "usage: git verify-tag [-v | --verbose] [--format=<format>] [--raw] <tag>...\n\n    -v, --[no-]verbose    print tag contents\n    --[no-]raw            print raw gpg status output\n    --[no-]format <format>\n                          format to use for the output\n\n";

const WHATCHANGED_USAGE: &str = "usage: git log [<options>] [<revision-range>] [[--] <path>...]\n   or: git show [<options>] <object>...\n\n    -q, --[no-]quiet      suppress diff output\n    --[no-]source         show source\n    --[no-]use-mailmap    use mail map file\n    --[no-]mailmap        alias of --use-mailmap\n    --clear-decorations   clear all previously-defined decoration filters\n    --[no-]decorate-refs <pattern>\n                          only decorate refs that match <pattern>\n    --[no-]decorate-refs-exclude <pattern>\n                          do not decorate refs that match <pattern>\n    --[no-]decorate[=...] decorate options\n    -L <range:file>       trace the evolution of line range <start>,<end> or function :<funcname> in <file>\n\n";

const WEB_BROWSE_USAGE: &str =
    "usage: git web--browse [--browser=browser|--tool=browser] [--config=conf.var] url/file ...\n";

fn validate_web_browse_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("web--browse") {
        return Ok(());
    }

    let mut browser = None;
    let mut config_name = None;
    let mut targets = Vec::new();
    let mut index = 1;
    while index < command_args.len() {
        let arg = command_args[index].as_str();
        if is_help_flag(arg) {
            io::stdout()
                .lock()
                .write_all(WEB_BROWSE_USAGE.as_bytes())
                .map_err(CliError::Io)?;
            return Err(CliError::Exit(0));
        }
        if let Some(value) = arg.strip_prefix("--browser=") {
            browser = Some(value.to_owned());
        } else if let Some(value) = arg.strip_prefix("--tool=") {
            browser = Some(value.to_owned());
        } else if let Some(value) = arg.strip_prefix("--config=") {
            config_name = Some(value.to_owned());
        } else if arg == "--browser" || arg == "--tool" {
            index += 1;
            let Some(value) = command_args.get(index) else {
                return Err(CliError::Stderr {
                    code: 1,
                    text: "No known browser available.\n".into(),
                });
            };
            browser = Some(value.clone());
        } else if arg == "--config" {
            index += 1;
            let Some(value) = command_args.get(index) else {
                return Err(CliError::Stderr {
                    code: 1,
                    text: "No known browser available.\n".into(),
                });
            };
            config_name = Some(value.clone());
        } else if arg.starts_with('-') {
            targets.push(arg.to_owned());
        } else {
            targets.push(arg.to_owned());
        }
        index += 1;
    }

    if targets.is_empty() {
        return Err(CliError::Stderr {
            code: 1,
            text: WEB_BROWSE_USAGE.trim_end_matches('\n').to_owned(),
        });
    }

    if let Some(browser_name) = browser {
        return Err(CliError::Stderr {
            code: 1,
            text: web_browse_browser_error(&browser_name),
        });
    }

    let config_name = config_name.unwrap_or_else(|| "web.browser".to_owned());
    if let Some(configured) = read_multi_config_values(&config_name)?
        .into_iter()
        .last()
        .filter(|value| !value.trim().is_empty())
    {
        return Err(CliError::Stderr {
            code: 1,
            text: format!(
                "git config option {config_name} set to unknown browser: {configured}\nResetting to default...\nNo known browser available.\n"
            ),
        });
    }
    Err(CliError::Stderr {
        code: 1,
        text: "No known browser available.\n".into(),
    })
}

fn web_browse_browser_error(browser: &str) -> String {
    match browser {
        "firefox" => "The browser firefox is not available as 'firefox'.\n".into(),
        _ => format!("Unknown browser '{browser}'.\n"),
    }
}

fn validate_unavailable_foreign_helper_invocation_before_clap(args: &[String]) -> Result<()> {
    let text = match args {
        [command, subcommand] if command == "gui" && subcommand == "unknown" => {
            "git: 'gui' is not a git command. See 'git --help'.\n\nThe most similar commands are\n\tgc\n\tgrep\n\tinit\n\tpull\n\tpush\n"
        }
        [command, subcommand] if command == "svn" && subcommand == "unknown" => {
            "git: 'svn' is not a git command. See 'git --help'.\n\nThe most similar commands are\n\tfsck\n\tmv\n\tshow\n"
        }
        [command, subcommand] if command == "cvsexportcommit" && subcommand == "unknown" => {
            "git: 'cvsexportcommit' is not a git command. See 'git --help'.\n"
        }
        [command, subcommand] if command == "cvsimport" && subcommand == "unknown" => {
            "git: 'cvsimport' is not a git command. See 'git --help'.\n"
        }
        _ => return Ok(()),
    };
    Err(CliError::Stderr {
        code: 1,
        text: text.into(),
    })
}

fn validate_sh_helper_invocation_before_clap(args: &[String]) -> Result<()> {
    if args.first().map(String::as_str) == Some("shell")
        && (matches!(args, [_, option] if option == "-c" || option == "--no-c")
            || matches!(args, [_, first, second, ..] if first == "-c" && second == "-c"))
    {
        return Err(CliError::Fatal {
            code: 128,
            message: "Run with no arguments or with -c cmd".into(),
        });
    }
    let Some(command @ ("sh-i18n" | "sh-setup")) = args.first().map(String::as_str) else {
        return Ok(());
    };
    Err(CliError::Stderr {
        code: 1,
        text: format!("git: '{command}' is not a git command. See 'git --help'.\n"),
    })
}

fn validate_update_ref_invocation_before_clap(args: &[String]) -> Result<()> {
    if args.first().map(String::as_str) == Some("update-ref") {
        validate_reftable_lock_timeout_override()?;
    }
    if matches!(args, [command, option, ..] if command == "update-ref" && option == "--delete") {
        return Err(CliError::Stderr {
            code: 129,
            text: "error: unknown option `delete'\nusage: git update-ref [<options>] -d <refname> [<old-oid>]\n   or: git update-ref [<options>]    <refname> <new-oid> [<old-oid>]\n   or: git update-ref [<options>] --stdin [-z] [--batch-updates]\n\n    -m <reason>           reason of the update\n    -d                    delete the reference\n    --no-deref            update <refname> not the one it points to\n    --deref               opposite of --no-deref\n    -z                    stdin has NUL-terminated arguments\n    --[no-]stdin          read updates from stdin\n    --[no-]create-reflog  create a reflog\n    -0, --[no-]batch-updates\n                          batch reference updates\n\n".into(),
        });
    }
    Ok(())
}

fn is_known_command(command: &str) -> bool {
    is_known_top_level_command_name(command)
}

fn validate_whatchanged_invocation_before_clap(args: &[String]) -> Result<()> {
    if args.first().map(String::as_str) != Some("whatchanged") {
        return Ok(());
    }
    Ok(())
}

fn is_deprecated_alias_shadowable_command(command: &str) -> bool {
    matches!(command, "whatchanged" | "pack-redundant")
}

fn read_alias_value(name: &str) -> Result<Option<String>> {
    let entries = read_all_alias_entries()?;
    for entry in entries.iter().rev() {
        if entry.section != "alias" {
            continue;
        }
        let simple_match = entry.subsection.is_empty() && entry.key.eq_ignore_ascii_case(name);
        let subsection_match = entry.key == "command" && entry.subsection == name;
        let dotted_match = !entry.subsection.is_empty()
            && entry.key != "command"
            && format!("{}.{}", entry.subsection, entry.key).eq_ignore_ascii_case(name);
        if !simple_match && !subsection_match && !dotted_match {
            continue;
        }
        if entry.implicit_bool {
            let config_name = if subsection_match {
                format!("alias.{name}.command")
            } else {
                format!("alias.{name}")
            };
            let source = entry
                .origin
                .strip_prefix("file:")
                .filter(|value| !value.is_empty())
                .unwrap_or(entry.origin.as_str());
            return Err(CliError::Stderr {
                code: 128,
                text: format!(
                    "error: missing value for '{config_name}'\nfatal: bad config line {} in file {source}\n",
                    entry.line.unwrap_or(0)
                ),
            });
        }
        return Ok(Some(entry.value.clone()));
    }
    Ok(None)
}

fn read_all_alias_entries() -> Result<Vec<ConfigEntry>> {
    if let Ok(repo) = find_repo_or_bare() {
        let mut entries = read_config_entries(&repo).map_err(CliError::Io)?;
        entries.extend(read_bare_ancestor_alias_config()?);
        return Ok(entries);
    }

    let mut entries = Vec::new();
    for path in system_config_paths() {
        entries.extend(read_config_file(&path)?);
    }
    for home in global_config_homes() {
        entries.extend(read_config_file(&home.join(".gitconfig"))?);
        entries.extend(read_config_file(
            &xdg_config_home(&home).join("git/config"),
        )?);
    }
    entries.extend(global_config_entries());
    entries.extend(read_bare_ancestor_alias_config()?);
    Ok(entries)
}

fn read_alias_names() -> Result<Vec<String>> {
    let mut names = BTreeSet::new();
    for entry in read_all_alias_entries()? {
        if entry.section != "alias" {
            continue;
        }
        if entry.subsection.is_empty() && entry.key != "command" {
            names.insert(entry.key);
            continue;
        }
        if entry.key == "command" && !entry.subsection.is_empty() {
            names.insert(entry.subsection);
            continue;
        }
        if !entry.subsection.is_empty() {
            names.insert(format!("{}.{}", entry.subsection, entry.key));
        }
    }
    Ok(names.into_iter().collect())
}

pub(crate) fn write_help_aliases(mut writer: impl std::io::Write) -> Result<()> {
    let names = read_alias_names()?;
    if names.is_empty() {
        return Ok(());
    }
    writeln!(writer).map_err(CliError::Io)?;
    writeln!(writer, "Command aliases").map_err(CliError::Io)?;
    for name in names {
        writeln!(writer, "   {name}").map_err(CliError::Io)?;
    }
    Ok(())
}

fn validate_unknown_command_invocation_before_clap(args: &[String]) -> Result<()> {
    let Some(command) = args.first().map(String::as_str) else {
        return Ok(());
    };
    if command == "--" || command.starts_with('-') || is_known_command(command) {
        return Ok(());
    }
    if command == "remote-http" && args.len() == 1 {
        return Err(CliError::Stderr {
            code: 1,
            text: "error: remote-curl\n".into(),
        });
    }
    let result = Err(CliError::Stderr {
        code: 1,
        text: format!("git: '{command}' is not a git command. See 'git --help'.\n"),
    });
    crate::runtime::run_parse_time_trace2_session(args, &result);
    result
}

fn read_bare_ancestor_alias_config() -> Result<Vec<ConfigEntry>> {
    let mut dir = std::env::current_dir()?;
    let mut entries = Vec::new();
    while dir.pop() {
        if is_bare_git_dir(&dir) {
            entries.extend(read_config_file(&dir.join("config"))?);
            break;
        }
    }
    Ok(entries)
}

fn split_alias_words(value: &str) -> Result<Vec<String>> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut token_started = false;
    let mut quote = None;
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        match (quote, ch) {
            (None, '\'') => {
                quote = Some('\'');
                token_started = true;
            }
            (None, '"') => {
                quote = Some('"');
                token_started = true;
            }
            (None, '\\') | (Some('"'), '\\') => {
                token_started = true;
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            (None, ch) if ch.is_whitespace() => {
                if token_started {
                    words.push(std::mem::take(&mut current));
                    token_started = false;
                }
            }
            (Some('\''), '\'') | (Some('"'), '"') => quote = None,
            (_, ch) => {
                token_started = true;
                current.push(ch);
            }
        }
    }
    if quote.is_some() {
        return Err(CliError::Fatal {
            code: 128,
            message: "bad alias string: unclosed quote".into(),
        });
    }
    if token_started {
        words.push(current);
    }
    Ok(words)
}

fn shell_alias_git_prefix(repo: &GitRepo) -> Result<String> {
    let cwd = std::env::current_dir()?;
    let relative = cwd.strip_prefix(&repo.root).map_err(|_| CliError::Fatal {
        code: 128,
        message: "current directory is outside work tree".into(),
    })?;
    if relative.as_os_str().is_empty() {
        return Ok(String::new());
    }
    Ok(format!(
        "{}/",
        relative
            .components()
            .map(|component| component.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/")
    ))
}

fn shell_alias_prepared_command(command: &str) -> String {
    format!("{command} \"$@\"")
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn trace_dashed_command_lookup_if_needed(command: &str, args: &[String]) -> Result<()> {
    let helper = format!("git-{command}");
    let rendered = render_trace_command(&helper, args);
    write_git_trace_line_if_needed("exec", &rendered)?;
    write_git_trace_line_if_needed("run_command", &rendered)
}

fn queue_trace2_dashed_lookup(command: &str, hierarchy: &[String]) {
    let hierarchy_value = hierarchy.join("/");
    crate::runtime::override_pending_trace2_command(
        "_run_dashed_".to_owned(),
        hierarchy_value.clone(),
    );
    crate::runtime::push_pending_trace2_perf_cmd_name(
        0,
        "main".to_owned(),
        "_run_dashed_".to_owned(),
        hierarchy_value,
    );
    crate::runtime::push_pending_trace2_perf_child_start(
        0,
        "main".to_owned(),
        "dashed".to_owned(),
        format!("git-{command}"),
    );
    queue_trace2_child_command(command, &hierarchy.join("/"));
}

fn queue_trace2_git_alias(
    alias_name: &str,
    expanded: &[String],
    trace2_context: &Trace2AliasContext,
) {
    let Some(final_command) = expanded.first() else {
        return;
    };
    let mut hierarchy = trace2_context.dashed_hierarchy.clone();
    hierarchy.push("_run_git_alias_".to_owned());
    crate::runtime::push_pending_trace2_perf_alias(
        0,
        "main".to_owned(),
        alias_name.to_owned(),
        expanded.join(" "),
    );
    crate::runtime::override_pending_trace2_command(
        "_run_git_alias_".to_owned(),
        hierarchy.join("/"),
    );
    queue_trace2_child_command(final_command, &hierarchy.join("/"));
}

fn queue_trace2_shell_alias(
    alias_name: &str,
    shell_command: &str,
    trace2_context: &Trace2AliasContext,
) {
    let mut hierarchy = trace2_context.dashed_hierarchy.clone();
    hierarchy.push("_run_shell_alias_".to_owned());
    crate::runtime::push_pending_trace2_perf_alias(
        0,
        "main".to_owned(),
        alias_name.to_owned(),
        shell_command.to_owned(),
    );
    crate::runtime::override_pending_trace2_command(
        "_run_shell_alias_".to_owned(),
        hierarchy.join("/"),
    );
    if let Some(stripped) = shell_command.strip_prefix("git ") {
        let child = stripped.split_whitespace().next().unwrap_or("version");
        queue_trace2_child_command(child, &hierarchy.join("/"));
    }
}

fn queue_trace2_child_command(command_name: &str, parent_hierarchy: &str) {
    let child_name = if command_name == "remote-http" {
        "remote-curl"
    } else {
        command_name
    };
    crate::runtime::push_pending_trace2_perf_cmd_name(
        1,
        "main".to_owned(),
        child_name.to_owned(),
        format!("{parent_hierarchy}/{child_name}"),
    );
    crate::runtime::push_pending_trace2_perf_def_params(1, "main".to_owned());
}

fn command_uses_dashed_trace2(command: &str) -> bool {
    matches!(command, "http-fetch")
}

fn trace_shell_alias_if_needed(command: &str, args: &[String], prepared: &str) -> Result<()> {
    let run_command = std::iter::once(shell_quote(command))
        .chain(args.iter().map(|arg| trace_quote(arg)))
        .collect::<Vec<_>>()
        .join(" ");
    write_git_trace_line_if_needed("run_command", &run_command)?;
    let shell = git_shell_command_path().display().to_string();
    let start = std::iter::once(shell)
        .chain(["-c".to_owned(), shell_quote(prepared), shell_quote(command)])
        .chain(args.iter().map(|arg| trace_quote(arg)))
        .collect::<Vec<_>>()
        .join(" ");
    write_git_trace_line_if_needed("start_command", &start)
}

fn render_trace_command(command: &str, args: &[String]) -> String {
    std::iter::once(command.to_owned())
        .chain(args.iter().map(|arg| trace_quote(arg)))
        .collect::<Vec<_>>()
        .join(" ")
}

fn trace_quote(value: &str) -> String {
    if !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '/' | ':'))
    {
        value.to_owned()
    } else {
        shell_quote(value)
    }
}

fn write_git_trace_line_if_needed(label: &str, message: &str) -> Result<()> {
    let Some(destination) = git_trace_destination() else {
        return Ok(());
    };
    let line = format!("trace: {label}: {message}\n");
    match destination {
        GitTraceDestination::Stderr => {
            std::io::stderr()
                .lock()
                .write_all(line.as_bytes())
                .map_err(CliError::Io)?;
        }
        GitTraceDestination::File(path) => {
            let mut trace = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .map_err(CliError::Io)?;
            trace.write_all(line.as_bytes()).map_err(CliError::Io)?;
        }
    }
    Ok(())
}

enum GitTraceDestination {
    Stderr,
    File(std::path::PathBuf),
}

fn git_trace_destination() -> Option<GitTraceDestination> {
    let value = std::env::var_os("GIT_TRACE")?;
    let rendered = value.to_string_lossy();
    if rendered.is_empty() || rendered == "0" || rendered.eq_ignore_ascii_case("false") {
        return None;
    }
    if rendered == "1" || rendered == "2" || rendered.eq_ignore_ascii_case("true") {
        return Some(GitTraceDestination::Stderr);
    }
    Some(GitTraceDestination::File(std::path::PathBuf::from(value)))
}

fn validate_scalar_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) == Some("scalar")
        && command_args.get(1).map(String::as_str) == Some("-C")
        && command_args.get(2).is_none()
    {
        return Err(CliError::Fatal {
            code: 128,
            message: "-C requires a <directory>".into(),
        });
    }
    Ok(())
}

fn validate_diff_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("diff") {
        return Ok(());
    }
    if command_args.iter().skip(1).any(|arg| arg == "--no-rename") {
        return Err(CliError::Stderr {
            code: 129,
            text: "error: invalid option: --no-rename\n".into(),
        });
    }
    Ok(())
}

fn validate_fetch_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("fetch") {
        return Ok(());
    }
    let mut args = command_args.iter().skip(1).peekable();
    while let Some(arg) = args.next() {
        if arg == "--" {
            break;
        }
        if arg == "--server-option" && args.peek().is_none() {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: option `server-option' requires a value\n".into(),
            });
        }
    }
    Ok(())
}

fn validate_fetch_pack_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("fetch-pack") {
        return Ok(());
    }
    if command_args
        .iter()
        .skip(1)
        .take_while(|arg| arg.as_str() != "--")
        .any(|arg| arg == "--upload-pack")
    {
        return Err(CliError::Stderr {
            code: 129,
            text: "usage: git fetch-pack [--all] [--stdin] [--quiet | -q] [--keep | -k] [--thin] [--include-tag] [--upload-pack=<git-upload-pack>] [--depth=<n>] [--no-progress] [--diag-url] [-v] [<host>:]<directory> [<refs>...]\n".into(),
        });
    }
    if command_args
        .iter()
        .skip(1)
        .take_while(|arg| arg.as_str() != "--")
        .any(|arg| arg == "--verbose")
    {
        return Err(CliError::Stderr {
            code: 129,
            text: "usage: git fetch-pack [--all] [--stdin] [--quiet | -q] [--keep | -k] [--thin] [--include-tag] [--upload-pack=<git-upload-pack>] [--depth=<n>] [--no-progress] [--diag-url] [-v] [<host>:]<directory> [<refs>...]\n".into(),
        });
    }
    Ok(())
}

fn validate_maintenance_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("maintenance")
        || command_args.get(1).map(String::as_str) != Some("run")
    {
        return Ok(());
    }
    for arg in command_args.iter().skip(2) {
        if arg == "--" {
            break;
        }
        if arg == "-q" {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: unknown switch `q'\nusage: git maintenance run [--auto] [--[no-]quiet] [--task=<task>] [--schedule]\n\n    --[no-]auto           run tasks based on the state of the repository\n    --[no-]detach         perform maintenance in the background\n    --[no-]schedule <frequency>\n                          run tasks based on frequency\n    --[no-]quiet          do not report progress or other information over stderr\n    --task <task>         run a specific task\n\n".into(),
            });
        }
    }
    Ok(())
}

fn validate_hash_object_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("hash-object") {
        return Ok(());
    }
    for arg in command_args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        if let Some(option) = arg
            .strip_prefix("--type")
            .filter(|value| value.is_empty() || value.starts_with('='))
        {
            let name = if option.is_empty() {
                "type".to_owned()
            } else {
                format!("type{option}")
            };
            return Err(CliError::Stderr {
                code: 129,
                text: format!(
                    "error: unknown option `{name}'\nusage: git hash-object [-t <type>] [-w] [--path=<file> | --no-filters]\n                       [--stdin [--literally]] [--] <file>...\n   or: git hash-object [-t <type>] [-w] --stdin-paths [--no-filters]\n\n    -t <type>             object type\n    -w                    write the object into the object database\n    --[no-]stdin          read the object from stdin\n    --[no-]stdin-paths    read file names from stdin\n    --no-filters          store file as is without filters\n    --filters             opposite of --no-filters\n    --[no-]literally      just hash any random garbage to create corrupt objects for debugging Git\n    --[no-]path <file>    process file as it were from this path\n\n"
                ),
            });
        }
        if arg == "--write" {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: unknown option `write'\nusage: git hash-object [-t <type>] [-w] [--path=<file> | --no-filters]\n                       [--stdin [--literally]] [--] <file>...\n   or: git hash-object [-t <type>] [-w] --stdin-paths [--no-filters]\n\n    -t <type>             object type\n    -w                    write the object into the object database\n    --[no-]stdin          read the object from stdin\n    --[no-]stdin-paths    read file names from stdin\n    --no-filters          store file as is without filters\n    --filters             opposite of --no-filters\n    --[no-]literally      just hash any random garbage to create corrupt objects for debugging Git\n    --[no-]path <file>    process file as it were from this path\n\n".into(),
            });
        }
    }
    Ok(())
}

fn validate_fast_export_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("fast-export") {
        return Ok(());
    }
    for arg in command_args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        if arg == "--no-all" || arg.starts_with("--all=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "usage: git fast-export [<rev-list-opts>]\n\n    --[no-]progress <n>   show progress after <n> objects\n    --[no-]signed-tags <mode>\n                          select handling of signed tags\n    --[no-]signed-commits <mode>\n                          select handling of signed commits\n    --[no-]tag-of-filtered-object <mode>\n                          select handling of tags that tag filtered objects\n    --[no-]reencode <mode>\n                          select handling of commit messages in an alternate encoding\n    --[no-]export-marks <file>\n                          dump marks to this file\n    --[no-]import-marks <file>\n                          import marks from this file\n    --[no-]import-marks-if-exists <file>\n                          import marks from this file if it exists\n    --[no-]fake-missing-tagger\n                          fake a tagger when tags lack one\n    --[no-]full-tree      output full tree for each commit\n    --[no-]use-done-feature\n                          use the done feature to terminate the stream\n    --no-data             skip output of blob data\n    --data                opposite of --no-data\n    --[no-]refspec <refspec>\n                          apply refspec to exported refs\n    --[no-]anonymize      anonymize output\n    --anonymize-map <from:to>\n                          convert <from> to <to> in anonymized output\n    --[no-]reference-excluded-parents\n                          reference parents which are not in fast-export stream by object id\n    --[no-]show-original-ids\n                          show original object ids of blobs/commits\n    --[no-]mark-tags      label tags with mark ids\n\n".into(),
            });
        }
    }
    Ok(())
}

fn validate_range_diff_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("range-diff") {
        return Ok(());
    }
    for arg in command_args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        if arg.starts_with("--no-dual-color=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: option `no-dual-color' takes no value\n".into(),
            });
        }
    }
    Ok(())
}

fn validate_request_pull_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("request-pull") {
        return Ok(());
    }
    for arg in command_args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        if arg.starts_with("-p=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: unknown switch `='\nusage: git request-pull [options] start url [end]\n\n    -p                    show patch text as well\n\n".into(),
            });
        }
    }
    Ok(())
}

fn validate_pull_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("pull") {
        return Ok(());
    }
    const USAGE: &str = "usage: git pull [<options>] [<repository> [<refspec>...]]\n\n    -v, --[no-]verbose    be more verbose\n    -q, --[no-]quiet      be more quiet\n    --[no-]progress       force progress reporting\n    --[no-]recurse-submodules[=<on-demand>]\n                          control for recursive fetching of submodules\n\nOptions related to merging\n    -r, --[no-]rebase[=(false|true|merges|interactive)]\n                          incorporate changes by rebasing rather than merging\n    -n                    do not show a diffstat at the end of the merge\n    --[no-]stat           show a diffstat at the end of the merge\n    --[no-]log[=<n>]      add (at most <n>) entries from shortlog to merge commit message\n    --[no-]signoff[=...]  add a Signed-off-by trailer\n    --[no-]squash         create a single commit instead of doing a merge\n    --[no-]commit         perform a commit if the merge succeeds (default)\n    --[no-]edit           edit message before committing\n    --[no-]cleanup <mode> how to strip spaces and #comments from message\n    --[no-]ff             allow fast-forward\n    --ff-only             abort if fast-forward is not possible\n    --[no-]verify         control use of pre-merge-commit and commit-msg hooks\n    --[no-]verify-signatures\n                          verify that the named commit has a valid GPG signature\n    --[no-]autostash      automatically stash/stash pop before and after\n    -s, --[no-]strategy <strategy>\n                          merge strategy to use\n    -X, --[no-]strategy-option <option=value>\n                          option for selected merge strategy\n    -S, --[no-]gpg-sign[=<key-id>]\n                          GPG sign commit\n    --[no-]allow-unrelated-histories\n                          allow merging unrelated histories\n\nOptions related to fetching\n    --[no-]all            fetch from all remotes\n    -a, --[no-]append     append to .git/FETCH_HEAD instead of overwriting\n    --[no-]upload-pack <path>\n                          path to upload pack on remote end\n    -f, --[no-]force      force overwrite of local branch\n    -t, --[no-]tags       fetch all tags and associated objects\n    -p, --[no-]prune      prune remote-tracking branches no longer on remote\n    -j, --[no-]jobs[=<n>] number of submodules pulled in parallel\n    --[no-]dry-run        dry run\n    -k, --[no-]keep       keep downloaded pack\n    --[no-]depth <depth>  deepen history of shallow clone\n    --[no-]shallow-since <time>\n                          deepen history of shallow repository based on time\n    --[no-]shallow-exclude <ref>\n                          deepen history of shallow clone, excluding ref\n    --[no-]deepen <n>     deepen history of shallow clone\n    --unshallow           convert to a complete repository\n    --[no-]update-shallow accept refs that update .git/shallow\n    --refmap <refmap>     specify fetch refmap\n    -o, --[no-]server-option <server-specific>\n                          option to transmit\n    -4, --[no-]ipv4       use IPv4 addresses only\n    -6, --[no-]ipv6       use IPv6 addresses only\n    --[no-]negotiation-tip <revision>\n                          report that we have only objects reachable from this object\n    --[no-]show-forced-updates\n                          check for forced-updates on all updated branches\n    --[no-]set-upstream   set upstream for git pull/fetch\n\n";
    const UNKNOWN_LONG_OPTIONS: &[&str] = &[
        "--atomic",
        "--auto-gc",
        "--auto-maintenance",
        "--multiple",
        "--negotiate-only",
        "--no-auto-gc",
        "--no-auto-maintenance",
        "--no-write-commit-graph",
        "--no-write-fetch-head",
        "--porcelain",
        "--prefetch",
        "--prune-tags",
        "--refetch",
        "--update-head-ok",
        "--write-commit-graph",
        "--write-fetch-head",
    ];
    for arg in command_args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        if let Some(option) = UNKNOWN_LONG_OPTIONS
            .iter()
            .find(|option| arg.as_str() == **option)
        {
            return Err(CliError::Stderr {
                code: 129,
                text: format!(
                    "error: unknown option `{}'\n{USAGE}",
                    option.trim_start_matches("--")
                ),
            });
        }
        if arg.starts_with("--recurse-submodules-default") {
            return Err(CliError::Stderr {
                code: 129,
                text: format!(
                    "error: unknown option `{}'\n{USAGE}",
                    arg.trim_start_matches("--")
                ),
            });
        }
        if arg.starts_with("--submodule-prefix") {
            return Err(CliError::Stderr {
                code: 129,
                text: format!(
                    "error: unknown option `{}'\n{USAGE}",
                    arg.trim_start_matches("--")
                ),
            });
        }
        if matches!(arg.as_str(), "-P" | "-e" | "-u") {
            let switch = arg.trim_start_matches('-');
            return Err(CliError::Stderr {
                code: 129,
                text: format!("error: unknown switch `{switch}'\n{USAGE}"),
            });
        }
    }
    Ok(())
}

fn validate_credential_store_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("credential-store") {
        return Ok(());
    }
    let mut index = 1usize;
    while index < command_args.len() {
        let arg = command_args[index].as_str();
        if arg == "--file" {
            if index + 1 >= command_args.len() {
                return Err(CliError::Stderr {
                    code: 129,
                    text: "error: option `file' requires a value\n".into(),
                });
            }
            index += 2;
            continue;
        }
        if arg.starts_with("--no-file=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: option `no-file' takes no value\n".into(),
            });
        }
        if arg == "--no-file" {
            index += 1;
            continue;
        }
        if arg.starts_with("--file=") {
            index += 1;
            continue;
        }
        if matches!(arg, "get" | "store" | "erase") {
            return Ok(());
        }
        return Ok(());
    }
    Err(CliError::Stderr {
        code: 129,
        text: "usage: git credential-store [<options>] <action>\n\n    --[no-]file <path>    fetch and store credentials in <path>\n\n".into(),
    })
}

fn validate_cherry_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("cherry") {
        return Ok(());
    }
    for arg in command_args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        if arg.starts_with("--verbose=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: option `verbose' takes no value\n".into(),
            });
        }
        if arg.starts_with("-v=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: unknown switch `='\nusage: git cherry [-v] [<upstream> [<head> [<limit>]]]\n\n    --[no-]abbrev[=<n>]   use <n> digits to display object names\n    -v, --[no-]verbose    be verbose\n\n".into(),
            });
        }
    }
    Ok(())
}

fn validate_commit_tree_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("commit-tree") {
        return Ok(());
    }
    let mut tree_args = 0usize;
    let mut index = 1usize;
    while index < command_args.len() {
        let arg = command_args[index].as_str();
        if arg == "--" {
            tree_args += command_args.len().saturating_sub(index + 1);
            break;
        }
        if matches!(arg, "-m" | "-F" | "-p") {
            if index + 1 >= command_args.len() {
                let option = arg.trim_start_matches('-');
                return Err(CliError::Stderr {
                    code: 129,
                    text: format!("error: switch `{option}' requires a value\n"),
                });
            }
            index += 2;
            continue;
        }
        if arg.starts_with("--no-gpg-sign=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: option `no-gpg-sign' takes no value\n".into(),
            });
        }
        if let Some(parent) = arg.strip_prefix("-p=") {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("not a valid object name ={parent}"),
            });
        }
        if arg.starts_with("-m") && arg.len() > 2
            || arg.starts_with("-F") && arg.len() > 2
            || arg.starts_with("-p") && arg.len() > 2
            || arg == "--no-gpg-sign"
        {
            index += 1;
            continue;
        }
        if arg.starts_with('-') {
            if arg == "--date" || arg.starts_with("--date=") {
                let name = arg.strip_prefix("--").unwrap_or(arg);
                return Err(CliError::Stderr {
                    code: 129,
                    text: format!(
                        "error: unknown option `{name}'\nusage: git commit-tree <tree> [(-p <parent>)...]\n   or: git commit-tree [(-p <parent>)...] [-S[<keyid>]] [(-m <message>)...]\n                       [(-F <file>)...] <tree>\n\n    -p <parent>           id of a parent commit object\n    -m <message>          commit message\n    -F <file>             read commit log message from file\n    -S, --[no-]gpg-sign[=<key-id>]\n                          GPG sign commit\n\n"
                    ),
                });
            }
            return Ok(());
        }
        tree_args += 1;
        index += 1;
    }
    if tree_args != 1 {
        return Err(CliError::Fatal {
            code: 128,
            message: "must give exactly one tree".into(),
        });
    }
    Ok(())
}

fn validate_write_tree_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("write-tree") {
        return Ok(());
    }
    for (index, arg) in command_args.iter().enumerate().skip(1) {
        if matches!(
            arg.as_str(),
            "--missing-ok" | "--no-missing-ok" | "--no-prefix"
        ) {
            continue;
        }
        if let Some(option) = arg.strip_prefix("--").and_then(|value| {
            value
                .strip_suffix("=true")
                .or_else(|| value.strip_suffix("=false"))
                .or_else(|| value.strip_suffix('='))
        }) {
            if matches!(option, "missing-ok" | "no-missing-ok" | "no-prefix") {
                return Err(CliError::Stderr {
                    code: 129,
                    text: format!("error: option `{option}' takes no value\n"),
                });
            }
        }
        if arg == "--prefix" && index + 1 >= command_args.len() {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: option `prefix' requires a value\n".into(),
            });
        }
    }
    Ok(())
}

fn validate_show_index_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("show-index") {
        return Ok(());
    }
    for (index, arg) in command_args.iter().enumerate().skip(1) {
        if arg == "--" {
            break;
        }
        if arg == "--object-format" && index + 1 >= command_args.len() {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: option `object-format' requires a value\n".into(),
            });
        }
    }
    Ok(())
}

fn validate_update_server_info_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("update-server-info") {
        return Ok(());
    }
    for arg in command_args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        if arg.starts_with("--force=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: option `force' takes no value\n".into(),
            });
        }
        if arg.starts_with("--no-force=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: option `no-force' takes no value\n".into(),
            });
        }
        if arg.starts_with("-f=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: unknown switch `='\nusage: git update-server-info [-f | --force]\n\n    -f, --[no-]force      update the info files from scratch\n\n".into(),
            });
        }
    }
    Ok(())
}

fn validate_prune_packed_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("prune-packed") {
        return Ok(());
    }
    for arg in command_args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        if arg.starts_with("--dry-run=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: option `dry-run' takes no value\n".into(),
            });
        }
        if arg.starts_with("--quiet=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: option `quiet' takes no value\n".into(),
            });
        }
        if arg.starts_with("-n=") || arg.starts_with("-q=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: unknown switch `='\nusage: git prune-packed [-n | --dry-run] [-q | --quiet]\n\n    -n, --[no-]dry-run    dry run\n    -q, --[no-]quiet      be quiet\n\n".into(),
            });
        }
    }
    Ok(())
}

fn validate_verify_commit_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("verify-commit") {
        return Ok(());
    }
    const USAGE: &str = "usage: git verify-commit [-v | --verbose] [--raw] <commit>...\n\n    -v, --[no-]verbose    print commit contents\n    --[no-]raw            print raw gpg status output\n\n";
    for arg in command_args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        if arg == "-S" || arg.starts_with("-S") {
            return Err(CliError::Stderr {
                code: 129,
                text: format!("error: unknown switch `S'\n{USAGE}"),
            });
        }
    }
    Ok(())
}

fn validate_verify_pack_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("verify-pack") {
        return Ok(());
    }
    for arg in command_args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        if arg.starts_with("--verbose=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: option `verbose' takes no value\n".into(),
            });
        }
        if arg.starts_with("--stat-only=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: option `stat-only' takes no value\n".into(),
            });
        }
        if arg.starts_with("-v=") || arg.starts_with("-s=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: unknown switch `='\nusage: git verify-pack [-v | --verbose] [-s | --stat-only] [--] <pack>.idx...\n\n    -v, --[no-]verbose    verbose\n    -s, --[no-]stat-only  show statistics only\n    --[no-]object-format <hash>\n                          specify the hash algorithm to use\n\n".into(),
            });
        }
    }
    Ok(())
}

fn validate_count_objects_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("count-objects") {
        return Ok(());
    }
    for arg in command_args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        if arg.starts_with("--verbose=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: option `verbose' takes no value\n".into(),
            });
        }
        if arg.starts_with("--human-readable=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: option `human-readable' takes no value\n".into(),
            });
        }
        if arg.starts_with("--no-verbose=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: option `no-verbose' takes no value\n".into(),
            });
        }
        if arg.starts_with("--no-human-readable=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: option `no-human-readable' takes no value\n".into(),
            });
        }
        if arg.starts_with("-v=") || arg.starts_with("-H=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: unknown switch `='\nusage: git count-objects [-v] [-H | --human-readable]\n\n    -v, --[no-]verbose    be verbose\n    -H, --[no-]human-readable\n                          print sizes in human readable format\n\n".into(),
            });
        }
    }
    Ok(())
}

fn validate_patch_id_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("patch-id") {
        return Ok(());
    }
    for arg in command_args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        if arg == "-O" || arg.starts_with("-O") {
            return Err(CliError::Stderr {
                code: 129,
                text: format!("error: unknown switch `O'\n{PATCH_ID_USAGE}"),
            });
        }
        if let Some(option) = arg.strip_prefix("--").and_then(|value| {
            value
                .strip_suffix("=true")
                .or_else(|| value.strip_suffix("=false"))
                .or_else(|| value.strip_suffix('='))
        }) {
            if matches!(option, "stable" | "unstable" | "verbatim") {
                return Err(CliError::Stderr {
                    code: 129,
                    text: format!("error: option `{option}' takes no value\n"),
                });
            }
        }
        if matches!(
            arg.as_str(),
            "--no-stable" | "--no-unstable" | "--no-verbatim"
        ) {
            let option = arg.trim_start_matches("--");
            return Err(CliError::Stderr {
                code: 129,
                text: format!("error: unknown option `{option}'\n{PATCH_ID_USAGE}"),
            });
        }
    }
    Ok(())
}

fn validate_stripspace_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("stripspace") {
        return Ok(());
    }
    const USAGE: &str = "usage: git stripspace [-s | --strip-comments]\n   or: git stripspace [-c | --comment-lines]\n\n    -s, --strip-comments  skip and remove all lines starting with comment character\n    -c, --comment-lines   prepend comment character and space to each line\n\n";
    for arg in command_args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        if arg.starts_with("--strip-comments=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: option `strip-comments' takes no value\n".into(),
            });
        }
        if arg.starts_with("--comment-lines=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: option `comment-lines' takes no value\n".into(),
            });
        }
        if arg.starts_with("-s=") || arg.starts_with("-c=") {
            return Err(CliError::Stderr {
                code: 129,
                text: format!("error: unknown switch `='\n{USAGE}"),
            });
        }
        if arg == "--whitespace" {
            return Err(CliError::Stderr {
                code: 129,
                text: format!("error: unknown option `whitespace'\n{USAGE}"),
            });
        }
    }
    Ok(())
}

fn validate_mailsplit_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("mailsplit") {
        return Ok(());
    }
    for arg in command_args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        if arg.starts_with("-b=") || arg.starts_with("--keep-cr=") {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("unknown option: {arg}"),
            });
        }
    }
    Ok(())
}

fn validate_merge_file_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("merge-file") {
        return Ok(());
    }
    const USAGE: &str = "usage: git merge-file [<options>] [-L <name1> [-L <orig> [-L <name2>]]] <file1> <orig-file> <file2>\n\n    -p, --[no-]stdout     send results to standard output\n    --[no-]object-id      use object IDs instead of filenames\n    --[no-]diff3          use a diff3 based merge\n    --[no-]zdiff3         use a zealous diff3 based merge\n    --[no-]ours           for conflicts, use our version\n    --[no-]theirs         for conflicts, use their version\n    --[no-]union          for conflicts, use a union version\n    --diff-algorithm <algorithm>\n                          choose a diff algorithm\n    --[no-]marker-size <n>\n                          for conflicts, use this marker size\n    -q, --[no-]quiet      do not warn about conflicts\n    -L <name>             set labels for file1/orig-file/file2\n\n";
    for arg in command_args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        if let Some(value) = arg.strip_prefix("--marker-size=")
            && !is_merge_file_marker_size_value(value)
        {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: option `marker-size' expects an integer value with an optional k/m/g suffix\n"
                .into(),
            });
        }
        if let Some(value) = arg.strip_prefix("--diff-algorithm=")
            && !is_merge_file_diff_algorithm_value(value)
        {
            return Err(merge_file_diff_algorithm_error());
        }
        if arg.starts_with("-q=") {
            return Err(CliError::Stderr {
                code: 129,
                text: format!("error: unknown switch `='\n{USAGE}"),
            });
        }
        if arg.starts_with("--quiet=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: option `quiet' takes no value\n".into(),
            });
        }
        if arg.starts_with("--object-id=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: option `object-id' takes no value\n".into(),
            });
        }
        if arg.starts_with("--no-object-id=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: option `no-object-id' takes no value\n".into(),
            });
        }
        for option in ["ours", "theirs", "union", "diff3", "zdiff3"] {
            if arg.starts_with(&format!("--{option}=")) {
                return Err(CliError::Stderr {
                    code: 129,
                    text: format!("error: option `{option}' takes no value\n"),
                });
            }
        }
    }
    for window in command_args.windows(2) {
        if window[0] == "--marker-size" && !is_merge_file_marker_size_value(&window[1]) {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: option `marker-size' expects an integer value with an optional k/m/g suffix\n"
                    .into(),
            });
        }
        if window[0] == "--diff-algorithm" && !is_merge_file_diff_algorithm_value(&window[1]) {
            return Err(merge_file_diff_algorithm_error());
        }
    }
    Ok(())
}

fn merge_file_diff_algorithm_error() -> CliError {
    CliError::Stderr {
        code: 129,
        text: "error: option diff-algorithm accepts \"myers\", \"minimal\", \"patience\" and \"histogram\"\n"
            .into(),
    }
}

fn is_merge_file_diff_algorithm_value(value: &str) -> bool {
    matches!(value, "myers" | "minimal" | "patience" | "histogram")
}

fn is_merge_file_marker_size_value(value: &str) -> bool {
    let digits = match value.as_bytes().last().copied() {
        Some(b'k' | b'K' | b'm' | b'M' | b'g' | b'G') => &value[..value.len() - 1],
        _ => value,
    };
    !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
}

fn validate_merge_tree_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("merge-tree") {
        return Ok(());
    }
    const USAGE: &str = "usage: git merge-tree [--write-tree] [<options>] <branch1> <branch2>\n   or: git merge-tree [--trivial-merge] <base-tree> <branch1> <branch2>\n\n    --write-tree          do a real merge instead of a trivial merge\n    --trivial-merge       do a trivial merge only\n    --[no-]messages       also show informational/conflict messages\n    --quiet               suppress all output; only exit status wanted\n    -z                    separate paths with the NUL character\n    --name-only           list filenames without modes/oids/stages\n    --allow-unrelated-histories\n                          allow merging unrelated histories\n    --stdin               perform multiple merges, one per line of input\n    --[no-]merge-base <tree-ish>\n                          specify a merge-base for the merge\n    -X, --[no-]strategy-option <option=value>\n                          option for selected merge strategy\n\n";
    for arg in command_args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        if arg.starts_with("-X=") {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("unknown strategy option: {arg}"),
            });
        }
        if let Some(option) = arg
            .strip_prefix('-')
            .and_then(|value| value.as_bytes().first().copied())
            .map(char::from)
            .filter(|option| matches!(option, 'm' | 'p' | 'F'))
        {
            return Err(CliError::Stderr {
                code: 129,
                text: format!("error: unknown switch `{option}'\n{USAGE}"),
            });
        }
    }
    Ok(())
}

fn validate_mergetool_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("mergetool") {
        return Ok(());
    }
    const USAGE: &str = "usage: git mergetool [--tool=tool] [--tool-help] [-y|--no-prompt|--prompt] [-g|--gui|--no-gui] [-O<orderfile>] [file to merge] ...\n";
    if command_args
        .iter()
        .skip(1)
        .take_while(|arg| arg.as_str() != "--")
        .any(|arg| arg == "--output" || arg.starts_with("--output=") || arg == "--auto-merge")
    {
        return Err(CliError::Stderr {
            code: 1,
            text: USAGE.into(),
        });
    }
    Ok(())
}

fn validate_mktree_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("mktree") {
        return Ok(());
    }
    const USAGE: &str = "usage: git mktree [-z] [--missing] [--batch]\n\n    -z                    input is NUL terminated\n    --[no-]missing        allow missing objects\n    --[no-]batch          allow creation of more than one tree\n\n";
    for arg in command_args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        if arg == "--no-z" {
            return Err(CliError::Stderr {
                code: 129,
                text: format!("error: unknown option `no-z'\n{USAGE}"),
            });
        }
        if arg == "--batch-command"
            || arg == "--no-batch-command"
            || arg.starts_with("--batch-command=")
            || arg.starts_with("--no-batch-command=")
        {
            return Err(CliError::Stderr {
                code: 129,
                text: format!(
                    "error: unknown option `{}'\n{USAGE}",
                    arg.trim_start_matches('-')
                ),
            });
        }
        if arg.starts_with("-z=") {
            return Err(CliError::Stderr {
                code: 129,
                text: format!("error: unknown switch `='\n{USAGE}"),
            });
        }
        if arg.starts_with("--batch=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: option `batch' takes no value\n".into(),
            });
        }
        if arg.starts_with("--missing=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: option `missing' takes no value\n".into(),
            });
        }
    }
    Ok(())
}

fn validate_config_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("config") {
        return Ok(());
    }
    const USAGE: &str = "usage: git config list [<file-option>] [<display-option>] [--includes]\n   or: git config get [<file-option>] [<display-option>] [--includes] [--all] [--regexp] [--value=<value>] [--fixed-value] [--default=<default>] <name>\n   or: git config set [<file-option>] [--type=<type>] [--all] [--value=<value>] [--fixed-value] <name> <value>\n   or: git config unset [<file-option>] [--all] [--value=<value>] [--fixed-value] <name>\n   or: git config rename-section [<file-option>] <old-name> <new-name>\n   or: git config remove-section [<file-option>] <name>\n   or: git config edit [<file-option>]\n   or: git config [<file-option>] --get-colorbool <name> [<stdout-is-tty>]\n\nConfig file location\n    --[no-]global         use global config file\n    --[no-]system         use system config file\n    --[no-]local          use repository config file\n    --[no-]worktree       use per-worktree config file\n    -f, --[no-]file <file>\n                          use given config file\n    --[no-]blob <blob-id> read config from given blob object\n\nAction\n    --get                 get value: name [<value-pattern>]\n    --get-all             get all values: key [<value-pattern>]\n    --get-regexp          get values for regexp: name-regex [<value-pattern>]\n    --get-urlmatch        get value specific for the URL: section[.var] URL\n    --replace-all         replace all matching variables: name value [<value-pattern>]\n    --add                 add a new variable: name value\n    --unset               remove a variable: name [<value-pattern>]\n    --unset-all           remove all matches: name [<value-pattern>]\n    --rename-section      rename section: old-name new-name\n    --remove-section      remove a section: name\n    -l, --list            list all\n    -e, --edit            open an editor\n    --get-color           find the color configured: slot [<default>]\n    --get-colorbool       find the color setting: slot [<stdout-is-tty>]\n\nDisplay options\n    -z, --[no-]null       terminate values with NUL byte\n    --[no-]name-only      show variable names only\n    --[no-]show-origin    show origin of config (file, standard input, blob, command line)\n    --[no-]show-scope     show scope of config (worktree, local, global, system, command)\n    --[no-]show-names     show config keys in addition to their values\n\nType\n    -t, --[no-]type <type>\n                          value is given this type\n    --bool                value is \"true\" or \"false\"\n    --int                 value is decimal number\n    --bool-or-int         value is --bool or --int\n    --bool-or-str         value is --bool or string\n    --path                value is a path (file or directory name)\n    --expiry-date         value is an expiry date\n\nOther\n    --[no-]default <value>\n                          with --get, use default value when missing entry\n    --[no-]comment <value>\n                          human-readable comment string (# will be prepended as needed)\n    --[no-]fixed-value    use string equality when comparing values to value pattern\n    --[no-]includes       respect include directives on lookup\n";
    const NEGATED_ACTIONS: &[&str] = &[
        "--no-get",
        "--no-get-all",
        "--no-get-regexp",
        "--no-get-urlmatch",
        "--no-replace-all",
        "--no-add",
        "--no-unset",
        "--no-unset-all",
        "--no-rename-section",
        "--no-remove-section",
        "--no-list",
        "--no-edit",
        "--no-get-color",
        "--no-get-colorbool",
    ];
    let mut saw_get = false;
    let mut saw_get_all = false;
    for arg in command_args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        if NEGATED_ACTIONS.contains(&arg.as_str()) {
            return Err(CliError::Stderr {
                code: 129,
                text: format!("error: unknown option `{}'\n{USAGE}", &arg[2..]),
            });
        }
        if arg == "--get" {
            saw_get = true;
        } else if arg == "--get-all" {
            saw_get_all = true;
        }
    }
    if saw_get && saw_get_all {
        return Err(CliError::Stderr {
            code: 129,
            text: "error: options '--get-all' and '--get' cannot be used together\n".into(),
        });
    }
    Ok(())
}

fn apply_leading_global_options(
    args: &[String],
) -> Result<(
    Vec<String>,
    Vec<ConfigEntry>,
    GlobalRepoOptions,
    PathspecOptions,
)> {
    let mut command_args = Vec::new();
    let mut global_configs = read_global_config_env_entries()?;
    let mut repo_options = GlobalRepoOptions::default();
    let mut pathspec_options = PathspecOptions::default();
    let mut pending_git_dir_display = None;
    let mut pending_git_dir_raw: Option<String> = None;
    let mut pending_work_tree_raw: Option<String> = None;
    let mut bare_without_explicit_git_dir = false;
    let mut bare_git_dir_cwd: Option<std::path::PathBuf> = None;
    let mut effective_cwd = std::env::current_dir()?;
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if arg == "-C" {
            let Some(path) = args.get(index + 1) else {
                return Err(CliError::Stderr {
                    code: 129,
                    text: "error: switch `C' requires a value\n".into(),
                });
            };
            let next_cwd = absolute_path_from_base(&effective_cwd, std::path::Path::new(path))?;
            std::env::set_current_dir(&next_cwd).map_err(|error| CliError::Fatal {
                code: 128,
                message: format!("cannot change to '{path}': {error}"),
            })?;
            effective_cwd = std::env::current_dir()?;
            index += 2;
        } else if arg == "-c" {
            let Some(config) = args.get(index + 1) else {
                return Err(CliError::Stderr {
                    code: 129,
                    text: "-c expects a configuration string\n".into(),
                });
            };
            global_configs.push(parse_global_config_entry(config)?);
            index += 2;
        } else if let Some(config) = arg.strip_prefix("--config-env=") {
            global_configs.push(parse_global_config_env_entry(config)?);
            index += 1;
        } else if arg == "--config-env" {
            let Some(config) = args.get(index + 1) else {
                return Err(CliError::Stderr {
                    code: 129,
                    text: "no config key given for --config-env\n".into(),
                });
            };
            if !config.contains('=') {
                return Err(CliError::Stderr {
                    code: 129,
                    text: format!("invalid config format: {config}\n"),
                });
            }
            global_configs.push(parse_global_config_env_entry(config)?);
            index += 2;
        } else if arg == "--exec-path" {
            println!("{}", git_exec_path_output());
            return Err(CliError::Exit(0));
        } else if let Some(path) = arg.strip_prefix("--exec-path=") {
            // SAFETY: CLI startup is single-threaded before any worker threads are spawned.
            unsafe {
                std::env::set_var("GIT_EXEC_PATH", path);
            }
            index += 1;
        } else if arg == "--no-lazy-fetch" {
            // SAFETY: CLI startup is single-threaded before commands can spawn workers.
            unsafe {
                std::env::set_var("GIT_NO_LAZY_FETCH", "1");
            }
            index += 1;
        } else if matches!(
            arg.as_str(),
            "-P" | "--no-pager"
                | "-p"
                | "--paginate"
                | "--no-replace-objects"
                | "--no-optional-locks"
        ) {
            index += 1;
        } else if arg == "--no-advice" {
            // SAFETY: CLI startup is single-threaded before any worker threads are spawned.
            unsafe {
                std::env::set_var("GIT_ADVICE", "false");
            }
            index += 1;
        } else if arg == "--literal-pathspecs" {
            if pathspec_options.icase || pathspec_options.glob_explicit {
                return Err(literal_pathspec_incompatible_error());
            }
            pathspec_options.literal = true;
            pathspec_options.glob = false;
            index += 1;
        } else if arg == "--noglob-pathspecs" {
            pathspec_options.glob = false;
            index += 1;
        } else if arg == "--glob-pathspecs" {
            if pathspec_options.literal {
                return Err(literal_pathspec_incompatible_error());
            }
            pathspec_options.glob = true;
            pathspec_options.glob_explicit = true;
            index += 1;
        } else if arg == "--icase-pathspecs" {
            if pathspec_options.literal {
                return Err(literal_pathspec_incompatible_error());
            }
            pathspec_options.icase = true;
            index += 1;
        } else if arg == "--bare" {
            repo_options.bare = true;
            bare_without_explicit_git_dir = pending_git_dir_raw.is_none();
            bare_git_dir_cwd = bare_without_explicit_git_dir.then(|| effective_cwd.clone());
            index += 1;
        } else if let Some(path) = arg.strip_prefix("--git-dir=") {
            pending_git_dir_display = Some(path.to_owned());
            pending_git_dir_raw = Some(path.to_owned());
            bare_without_explicit_git_dir = false;
            bare_git_dir_cwd = None;
            index += 1;
        } else if arg == "--git-dir" {
            let Some(path) = args.get(index + 1) else {
                return Err(CliError::Stderr {
                    code: 129,
                    text: "error: option `git-dir' requires a value\n".into(),
                });
            };
            pending_git_dir_display = Some(path.clone());
            pending_git_dir_raw = Some(path.clone());
            bare_without_explicit_git_dir = false;
            bare_git_dir_cwd = None;
            index += 2;
        } else if let Some(path) = arg.strip_prefix("--work-tree=") {
            pending_work_tree_raw = Some(path.to_owned());
            index += 1;
        } else if arg == "--work-tree" {
            let Some(path) = args.get(index + 1) else {
                return Err(CliError::Stderr {
                    code: 129,
                    text: "error: option `work-tree' requires a value\n".into(),
                });
            };
            pending_work_tree_raw = Some(path.clone());
            index += 2;
        } else if let Some(source) = arg.strip_prefix("--attr-source=") {
            repo_options.attr_source = Some(source.to_owned());
            index += 1;
        } else if arg == "--attr-source" {
            let Some(source) = args.get(index + 1) else {
                return Err(CliError::Stderr {
                    code: 129,
                    text: "error: option `attr-source' requires a value\n".into(),
                });
            };
            repo_options.attr_source = Some(source.clone());
            index += 2;
        } else {
            command_args.extend_from_slice(&args[index..]);
            break;
        }
    }
    repo_options.git_dir_display = pending_git_dir_display;
    repo_options.git_dir = pending_git_dir_raw
        .map(|path| {
            absolute_path_from_base(&effective_cwd, std::path::Path::new(&path))
                .map(canonical_or_absolute)
        })
        .transpose()?;
    repo_options.work_tree = pending_work_tree_raw
        .map(|path| {
            absolute_path_from_base(&effective_cwd, std::path::Path::new(&path))
                .map(canonical_or_absolute)
        })
        .transpose()?;
    if bare_without_explicit_git_dir && repo_options.git_dir.is_none() {
        let git_dir =
            canonical_or_absolute(bare_git_dir_cwd.unwrap_or_else(|| effective_cwd.clone()));
        repo_options.git_dir_display = Some(git_dir.display().to_string());
        repo_options.git_dir = Some(git_dir);
    }
    Ok((command_args, global_configs, repo_options, pathspec_options))
}

fn git_exec_path_output() -> String {
    if let Some(path) = std::env::var_os("GIT_EXEC_PATH") {
        return git_var_path_output(std::path::Path::new(&path));
    }
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(git_var_path_output))
        .unwrap_or_default()
}

fn literal_pathspec_incompatible_error() -> CliError {
    CliError::Fatal {
        code: 128,
        message: "global 'literal' pathspec setting is incompatible with all other global pathspec settings".into(),
    }
}

fn validate_bugreport_invocation_before_clap(command_args: &[String]) -> Result<()> {
    let Some(command) = command_args.first().map(String::as_str) else {
        return Ok(());
    };
    if command != "bugreport" {
        return Ok(());
    }
    let mut index = 1;
    while index < command_args.len() {
        let arg = command_args[index].as_str();
        if arg == "--" {
            let value = command_args
                .get(index + 1)
                .map(String::as_str)
                .unwrap_or_default();
            return Err(CliError::Stderr {
                code: 129,
                text: format!("error: unknown argument `{value}'\n{BUGREPORT_SHORT_USAGE}"),
            });
        }
        if !arg.starts_with('-') || arg == "-" {
            return Err(CliError::Stderr {
                code: 129,
                text: format!("error: unknown argument `{arg}'\n{BUGREPORT_SHORT_USAGE}"),
            });
        }
        if arg == "-o" || arg == "--output-directory" || arg == "-s" || arg == "--suffix" {
            index += 2;
            continue;
        }
        if arg.starts_with("--output-directory=")
            || arg.starts_with("--suffix=")
            || arg.starts_with("--diagnose=")
        {
            index += 1;
            continue;
        }
        if matches!(arg, "--no-suffix" | "--diagnose" | "--no-diagnose") {
            index += 1;
            continue;
        }
        if let Some(name) = arg.strip_prefix("--") {
            return Err(CliError::Stderr {
                code: 129,
                text: format!("error: unknown option `{name}'\n{BUGREPORT_USAGE}"),
            });
        }
        index += 1;
    }
    Ok(())
}
