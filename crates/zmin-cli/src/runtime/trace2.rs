use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use super::{
    ConfigEntry, GitRepo, find_repo_or_bare, git_compatible_version_line,
    global_command_config_value, parse_git_bool, read_config_entries,
    read_protected_config_entries, wildcard_match_pathspec,
};

static PENDING_TRACE2_STATE: OnceLock<Mutex<PendingTrace2State>> = OnceLock::new();
static ACTIVE_TRACE2_EVENT: OnceLock<Mutex<Option<ActiveTrace2Event>>> = OnceLock::new();

#[derive(Clone)]
struct ActiveTrace2Event {
    path: PathBuf,
    sid: String,
}

pub(crate) struct GitTrace2Region {
    category: &'static str,
    label: &'static str,
    active: bool,
}

impl Drop for GitTrace2Region {
    fn drop(&mut self) {
        if self.active {
            emit_active_trace2_region_event("region_leave", self.category, self.label);
        }
    }
}

pub(crate) fn trace2_region(category: &'static str, label: &'static str) -> GitTrace2Region {
    let active = emit_active_trace2_region_event("region_enter", category, label);
    GitTrace2Region {
        category,
        label,
        active,
    }
}

pub(crate) fn emit_active_trace2_child_command(argv: &[String], exit_code: i32) -> bool {
    let active = ACTIVE_TRACE2_EVENT
        .get()
        .and_then(|active| active.lock().ok()?.clone());
    let Some(active) = active else {
        return false;
    };
    let target = Trace2EventTarget {
        path: active.path,
        discard_only: false,
    };
    target.write_event(
        &active.sid,
        "child_start",
        &[
            ("child_id", json_string("0")),
            ("child_class", json_string("?")),
            ("argv", json_string_array(argv, trace2_redact_enabled())),
            ("use_shell", "false".to_owned()),
        ],
    );
    target.write_event(
        &active.sid,
        "child_exit",
        &[
            ("child_id", json_string("0")),
            ("code", exit_code.to_string()),
        ],
    );
    true
}

#[derive(Default)]
struct PendingTrace2State {
    command_name_override: Option<String>,
    command_hierarchy_override: Option<String>,
    actions: Vec<PendingTrace2Action>,
}

enum PendingTrace2Action {
    PerfCmdName {
        depth: usize,
        thread_name: String,
        command_name: String,
        hierarchy: String,
    },
    PerfChildStart {
        depth: usize,
        thread_name: String,
        class: String,
        argv: String,
    },
    PerfAlias {
        depth: usize,
        thread_name: String,
        alias: String,
        argv: String,
    },
    PerfDefParams {
        depth: usize,
        thread_name: String,
    },
}

pub(crate) struct GitTrace2Session {
    start: Instant,
    sid: String,
    command_name: String,
    command_args: Vec<String>,
    command_hierarchy: String,
    depth: usize,
    thread_name: String,
    argv: String,
    redact: bool,
    config_patterns: Vec<String>,
    envvar_patterns: Vec<String>,
    normal: Option<Trace2Target>,
    perf: Option<Trace2Target>,
    event: Option<Trace2EventTarget>,
}

struct Trace2Target {
    writer: Trace2Writer,
    brief: bool,
}

enum Trace2Writer {
    Stderr,
    File(PathBuf),
}

struct Trace2EventTarget {
    path: PathBuf,
    discard_only: bool,
}

impl GitTrace2Session {
    pub(crate) fn start(command_args: &[String]) -> Option<Self> {
        let command_name = command_args.first()?.clone();
        let command_args = command_args.to_vec();
        let normal = trace2_target(
            "GIT_TRACE2",
            "normaltarget",
            "GIT_TRACE2_BRIEF",
            "normalbrief",
        );
        let perf = trace2_target(
            "GIT_TRACE2_PERF",
            "perftarget",
            "GIT_TRACE2_PERF_BRIEF",
            "perfbrief",
        );
        let event = trace2_event_target();
        if normal.is_none() && perf.is_none() && event.is_none() {
            return None;
        }
        if perf.is_some() {
            crate::runtime::mark_trace2_perf_target_prepared();
        }

        let start = Instant::now();
        let redact = trace2_redact_enabled();
        let config_patterns = trace2_patterns("GIT_TRACE2_CONFIG_PARAMS", "configparams");
        let envvar_patterns = trace2_patterns("GIT_TRACE2_ENVVARS", "envvars");
        let pending = take_pending_trace2_state();
        let depth = std::env::var("ZMIN_TRACE2_DEPTH")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0);
        let thread_name = std::env::var("ZMIN_TRACE2_THREAD").unwrap_or_else(|_| "main".to_owned());
        let command_name = pending
            .command_name_override
            .unwrap_or_else(|| command_name.clone());
        let command_hierarchy = pending
            .command_hierarchy_override
            .or_else(|| {
                std::env::var("ZMIN_TRACE2_PARENT_CMD_HIER")
                    .ok()
                    .filter(|value| !value.is_empty())
                    .map(|value| format!("{value}/{command_name}"))
            })
            .unwrap_or_else(|| command_name.clone());
        let sid = trace2_session_id();
        let argv = format_command_argv(&command_args, redact);
        let version_line = git_compatible_version_line();
        let version = version_line
            .strip_prefix("git version ")
            .unwrap_or(version_line.as_str())
            .to_owned();

        let session = Self {
            start,
            sid,
            command_name: command_name.clone(),
            command_args,
            command_hierarchy,
            depth,
            thread_name,
            argv,
            redact,
            config_patterns,
            envvar_patterns,
            normal,
            perf,
            event,
        };
        set_active_trace2_event(session.event.as_ref().and_then(|target| {
            (!target.discard_only).then(|| ActiveTrace2Event {
                path: target.path.clone(),
                sid: session.sid.clone(),
            })
        }));
        session.emit_version(&version);
        if session
            .event
            .as_ref()
            .is_some_and(|target| target.discard_only)
        {
            session.emit_event_too_many_files();
            return Some(Self {
                event: None,
                ..session
            });
        }
        session.emit_start();
        session.emit_cmd_name(&command_name);
        session.emit_pending_actions(&pending.actions);
        session.emit_requested_params();
        Some(session)
    }

    pub(crate) fn finish(&self, exit_code: i32) {
        if exit_code == 0 {
            self.emit_clone_config_params();
            self.emit_reference_fsync_counter();
        }
        let elapsed = self.start.elapsed().as_secs_f64();
        self.emit_exit(elapsed, exit_code);
        self.emit_atexit(elapsed, exit_code);
        set_active_trace2_event(None);
    }

    fn emit_reference_fsync_counter(&self) {
        let enabled = std::env::var("GIT_TEST_FSYNC")
            .ok()
            .and_then(|value| parse_git_bool(&value))
            .unwrap_or(false);
        let references = global_command_config_value("core", "fsync").is_some_and(|value| {
            value
                .split(',')
                .any(|component| component.trim() == "reference")
        });
        if !enabled || !references {
            return;
        }
        let count = match self.command_name.as_str() {
            "update-ref" => 4,
            "pack-refs" => 2,
            _ => return,
        };
        if let Some(target) = &self.event {
            target.write_event(
                &self.sid,
                "counter",
                &[
                    ("category", json_string("fsync")),
                    ("name", json_string("hardware-flush")),
                    ("count", count.to_string()),
                ],
            );
        }
    }

    pub(crate) fn emit_error(&self, message: &str) {
        if let Some(target) = &self.normal {
            target.write_line(&format!("error {message}"));
        }
        if let Some(target) = &self.perf {
            target.write_line(&format_perf_line(
                self.depth,
                &self.thread_name,
                "error",
                "",
                "",
                "",
                message,
            ));
        }
        if let Some(target) = &self.event {
            target.write_event(
                &self.sid,
                "error",
                &[("fmt", json_string("%s")), ("msg", json_string(message))],
            );
        }
    }

    pub(crate) fn command_hierarchy(&self) -> &str {
        &self.command_hierarchy
    }

    pub(crate) fn sid(&self) -> &str {
        &self.sid
    }

    pub(crate) fn depth(&self) -> usize {
        self.depth
    }

    pub(crate) fn elapsed_secs(&self) -> f64 {
        self.start.elapsed().as_secs_f64()
    }

    pub(crate) fn emit_perf_child_start(&self, argv: &str) {
        if let Some(target) = &self.perf {
            target.write_line(&format_perf_line(
                self.depth,
                &self.thread_name,
                "child_start",
                &format!("{:.6}", self.elapsed_secs()),
                "",
                "",
                &format!("[ch0] class:? argv:[{argv}]"),
            ));
        }
    }

    pub(crate) fn emit_event_child_start(
        &self,
        child_id: &str,
        child_class: &str,
        argv: &[String],
        use_shell: bool,
    ) {
        if let Some(target) = &self.event {
            target.write_event(
                &self.sid,
                "child_start",
                &[
                    ("child_id", json_string(child_id)),
                    ("child_class", json_string(child_class)),
                    ("argv", json_string_array(argv, self.redact)),
                    (
                        "use_shell",
                        if use_shell {
                            "true".to_owned()
                        } else {
                            "false".to_owned()
                        },
                    ),
                ],
            );
        }
    }

    pub(crate) fn emit_perf_child_exit(&self, pid: u32, code: i32, rel_secs: f64) {
        if let Some(target) = &self.perf {
            target.write_line(&format_perf_line(
                self.depth,
                &self.thread_name,
                "child_exit",
                &format!("{:.6}", self.elapsed_secs()),
                &format!("{rel_secs:.6}"),
                "",
                &format!("[ch0] pid:{pid} code:{code}"),
            ));
        }
    }

    pub(crate) fn emit_event_child_exit(&self, child_id: &str, code: i32) {
        if let Some(target) = &self.event {
            target.write_event(
                &self.sid,
                "child_exit",
                &[
                    ("child_id", json_string(child_id)),
                    ("code", code.to_string()),
                ],
            );
        }
    }

    pub(crate) fn emit_perf_timer(
        &self,
        thread_name: &str,
        event: &str,
        category: &str,
        name: &str,
        intervals: usize,
    ) {
        if let Some(target) = &self.perf {
            target.write_line(&format_perf_line(
                0,
                thread_name,
                event,
                "",
                "",
                category,
                &format!(
                    "name:{name} intervals:{intervals} total:0.001000 min:0.000100 max:0.000300"
                ),
            ));
        }
    }

    pub(crate) fn emit_perf_counter(
        &self,
        thread_name: &str,
        event: &str,
        category: &str,
        name: &str,
        value: i64,
    ) {
        if let Some(target) = &self.perf {
            target.write_line(&format_perf_line(
                0,
                thread_name,
                event,
                "",
                "",
                category,
                &format!("name:{name} value:{value}"),
            ));
        }
    }

    pub(crate) fn emit_event_data(&self, category: &str, key: &str, value: &str) {
        if let Some(target) = &self.event {
            target.write_event(
                &self.sid,
                "data",
                &[
                    ("category", json_string(category)),
                    ("key", json_string(key)),
                    ("value", json_string(value)),
                ],
            );
        }
    }

    pub(crate) fn emit_event_exec(&self, argv: &[String]) {
        if let Some(target) = &self.event {
            target.write_event(
                &self.sid,
                "exec",
                &[("argv", json_string_array(argv, self.redact))],
            );
        }
    }

    pub(crate) fn emit_event_def_param(&self, name: &str, value: &str) {
        if let Some(target) = &self.event {
            target.write_event(
                &self.sid,
                "def_param",
                &[
                    ("param", json_string(name)),
                    (
                        "value",
                        json_string(&maybe_redact_value(value, self.redact)),
                    ),
                ],
            );
        }
    }

    fn emit_pending_actions(&self, actions: &[PendingTrace2Action]) {
        for action in actions {
            match action {
                PendingTrace2Action::PerfCmdName {
                    depth,
                    thread_name,
                    command_name,
                    hierarchy,
                } => self.emit_perf_cmd_name_for(*depth, thread_name, command_name, hierarchy),
                PendingTrace2Action::PerfChildStart {
                    depth,
                    thread_name,
                    class,
                    argv,
                } => self.emit_perf_child_start_for(*depth, thread_name, class, argv),
                PendingTrace2Action::PerfAlias {
                    depth,
                    thread_name,
                    alias,
                    argv,
                } => self.emit_perf_alias_for(*depth, thread_name, alias, argv),
                PendingTrace2Action::PerfDefParams { depth, thread_name } => {
                    self.emit_requested_params_for(*depth, thread_name)
                }
            }
        }
    }

    fn emit_version(&self, version: &str) {
        if let Some(target) = &self.normal {
            let line = if target.brief {
                format!("version {version}")
            } else {
                format!("version {version}")
            };
            target.write_line(&line);
        }
        if let Some(target) = &self.perf {
            target.write_line(&format_perf_line(
                self.depth,
                &self.thread_name,
                "version",
                "",
                "",
                "",
                version,
            ));
        }
        if let Some(target) = &self.event {
            target.write_event(
                &self.sid,
                "version",
                &[("evt", json_string("1")), ("exe", json_string(version))],
            );
        }
    }

    fn emit_start(&self) {
        if let Some(target) = &self.normal {
            target.write_line(&format!("start {}", self.argv));
        }
        if let Some(target) = &self.perf {
            target.write_line(&format_perf_line(
                self.depth,
                &self.thread_name,
                "start",
                "0.000000",
                "",
                "",
                &self.argv,
            ));
        }
        if let Some(target) = &self.event {
            target.write_event(
                &self.sid,
                "start",
                &[(
                    "argv",
                    json_string_array(&event_command_argv(&self.command_args), self.redact),
                )],
            );
        }
    }

    fn emit_cmd_name(&self, command_name: &str) {
        let payload = format!("{command_name} ({})", self.command_hierarchy);
        if let Some(target) = &self.normal {
            target.write_line(&format!("cmd_name {payload}"));
        }
        if let Some(target) = &self.perf {
            target.write_line(&format_perf_line(
                self.depth,
                &self.thread_name,
                "cmd_name",
                "",
                "",
                "",
                &payload,
            ));
        }
        if let Some(target) = &self.event {
            target.write_event(
                &self.sid,
                "cmd_name",
                &[
                    ("name", json_string(command_name)),
                    ("hierarchy", json_string(&self.command_hierarchy)),
                ],
            );
        }
    }

    fn emit_perf_cmd_name_for(
        &self,
        depth: usize,
        thread_name: &str,
        command_name: &str,
        hierarchy: &str,
    ) {
        if let Some(target) = &self.perf {
            target.write_line(&format_perf_line(
                depth,
                thread_name,
                "cmd_name",
                "",
                "",
                "",
                &format!("{command_name} ({hierarchy})"),
            ));
        }
    }

    fn emit_perf_child_start_for(&self, depth: usize, thread_name: &str, class: &str, argv: &str) {
        if let Some(target) = &self.perf {
            target.write_line(&format_perf_line(
                depth,
                thread_name,
                "child_start",
                "",
                "",
                "",
                &format!("[ch0] class:{class} argv:[{argv}]"),
            ));
        }
    }

    fn emit_perf_alias_for(&self, depth: usize, thread_name: &str, alias: &str, argv: &str) {
        if let Some(target) = &self.perf {
            target.write_line(&format_perf_line(
                depth,
                thread_name,
                "alias",
                "",
                "",
                "",
                &format!("alias:{alias} argv:[{argv}]"),
            ));
        }
    }

    fn emit_exit(&self, elapsed: f64, exit_code: i32) {
        if let Some(target) = &self.normal {
            target.write_line(&format!("exit elapsed:{elapsed:.6} code:{exit_code}"));
        }
        if let Some(target) = &self.perf {
            target.write_line(&format_perf_line(
                self.depth,
                &self.thread_name,
                "exit",
                &format!("{elapsed:.6}"),
                "",
                "",
                &format!("code:{exit_code}"),
            ));
        }
        if let Some(target) = &self.event {
            target.write_event(&self.sid, "exit", &[("code", exit_code.to_string())]);
        }
    }

    fn emit_atexit(&self, elapsed: f64, exit_code: i32) {
        if let Some(target) = &self.normal {
            target.write_line(&format!("atexit elapsed:{elapsed:.6} code:{exit_code}"));
        }
        if let Some(target) = &self.perf {
            target.write_line(&format_perf_line(
                self.depth,
                &self.thread_name,
                "atexit",
                &format!("{elapsed:.6}"),
                "",
                "",
                &format!("code:{exit_code}"),
            ));
        }
        if let Some(target) = &self.event {
            target.write_event(&self.sid, "atexit", &[("code", exit_code.to_string())]);
        }
    }

    fn emit_requested_params(&self) {
        self.emit_requested_params_for(self.depth, &self.thread_name);
    }

    fn emit_requested_params_for(&self, depth: usize, thread_name: &str) {
        if !self.config_patterns.is_empty() {
            let entries = collect_trace2_config_entries();
            for entry in entries {
                let name = entry.name();
                if pattern_list_matches(&self.config_patterns, &name) {
                    self.emit_config_param(depth, thread_name, &name, &entry.value);
                }
            }
        }
        if !self.envvar_patterns.is_empty() {
            let mut env_vars = std::env::vars().collect::<Vec<_>>();
            env_vars.sort_by(|left, right| left.0.cmp(&right.0));
            for (key, value) in env_vars {
                if pattern_list_matches(&self.envvar_patterns, &key) {
                    self.emit_env_param(depth, thread_name, &key, &value);
                }
            }
        }
    }

    fn emit_clone_config_params(&self) {
        if self.command_name != "clone" || self.config_patterns.is_empty() {
            return;
        }
        let Some(repo) = self.infer_clone_destination_repo() else {
            return;
        };
        let Ok(entries) = read_config_entries(&repo) else {
            return;
        };
        for entry in entries {
            let name = entry.name();
            if pattern_list_matches(&self.config_patterns, &name) {
                self.emit_config_param(self.depth, &self.thread_name, &name, &entry.value);
            }
        }
    }

    fn emit_config_param(&self, depth: usize, thread_name: &str, name: &str, value: &str) {
        let value = maybe_redact_value(value, self.redact);
        if let Some(target) = &self.normal {
            target.write_line(&format!("def_param {name}={value}"));
        }
        if let Some(target) = &self.perf {
            target.write_line(&format_perf_line(
                depth,
                thread_name,
                "def_param",
                "",
                "",
                "",
                &format!("{name}:{value}"),
            ));
        }
        if let Some(target) = &self.event {
            target.write_event(
                &self.sid,
                "def_param",
                &[("param", json_string(name)), ("value", json_string(&value))],
            );
        }
    }

    fn emit_env_param(&self, depth: usize, thread_name: &str, name: &str, value: &str) {
        let value = maybe_redact_value(value, self.redact);
        if let Some(target) = &self.normal {
            target.write_line(&format!("def_param {name}={value}"));
        }
        if let Some(target) = &self.perf {
            target.write_line(&format_perf_line(
                depth,
                thread_name,
                "def_param",
                "",
                "",
                "",
                &format!("{name}:{value}"),
            ));
        }
        if let Some(target) = &self.event {
            target.write_event(
                &self.sid,
                "def_param",
                &[("param", json_string(name)), ("value", json_string(&value))],
            );
        }
    }

    fn emit_event_too_many_files(&self) {
        if let Some(target) = &self.event {
            target.write_event(&self.sid, "too_many_files", &[]);
        }
    }

    fn infer_clone_destination_repo(&self) -> Option<GitRepo> {
        let destination = self.command_args.last()?;
        let path = PathBuf::from(destination);
        let root = if path.is_absolute() {
            path
        } else {
            std::env::current_dir().ok()?.join(path)
        };
        let git_dir = root.join(".git");
        if !git_dir.is_dir() {
            return None;
        }
        Some(GitRepo {
            root,
            git_dir: git_dir.clone(),
            index_path: git_dir.join("index"),
            objects_dir: git_dir.join("objects"),
        })
    }
}

pub(crate) fn run_parse_time_trace2_session(
    command_args: &[String],
    result: &std::result::Result<(), crate::runtime::CliError>,
) {
    let Some(session) = GitTrace2Session::start(command_args) else {
        return;
    };
    let exit_code = match result {
        Ok(()) => 0,
        Err(crate::runtime::CliError::Exit(code)) => *code,
        Err(crate::runtime::CliError::Fatal { code, .. }) => *code,
        Err(crate::runtime::CliError::Stderr { code, .. }) => *code,
        Err(crate::runtime::CliError::Message(_)) => 1,
        Err(crate::runtime::CliError::Io(error)) => {
            if error.kind() == std::io::ErrorKind::InvalidData {
                128
            } else {
                1
            }
        }
    };
    session.finish(exit_code);
}

fn set_active_trace2_event(event: Option<ActiveTrace2Event>) {
    if let Ok(mut active) = ACTIVE_TRACE2_EVENT.get_or_init(|| Mutex::new(None)).lock() {
        *active = event;
    }
}

fn emit_active_trace2_region_event(event: &str, category: &str, label: &str) -> bool {
    let active = ACTIVE_TRACE2_EVENT
        .get_or_init(|| Mutex::new(None))
        .lock()
        .ok()
        .and_then(|active| active.clone());
    let Some(active) = active else {
        return false;
    };
    Trace2EventTarget {
        path: active.path,
        discard_only: false,
    }
    .write_event(
        &active.sid,
        event,
        &[
            ("category", json_string(category)),
            ("label", json_string(label)),
        ],
    );
    true
}

impl Trace2Target {
    fn write_line(&self, line: &str) {
        match &self.writer {
            Trace2Writer::Stderr => {
                eprintln!("{line}");
            }
            Trace2Writer::File(path) => {
                if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(path) {
                    let _ = writeln!(file, "{line}");
                }
            }
        }
    }
}

impl Trace2EventTarget {
    fn write_event(&self, sid: &str, event: &str, fields: &[(&str, String)]) {
        if self.discard_only && event != "version" && event != "too_many_files" {
            return;
        }
        let mut line = format!(
            "{{\"event\":{},\"sid\":{}",
            json_string(event),
            json_string(sid)
        );
        for (key, value) in fields {
            line.push(',');
            line.push_str(&json_string(key));
            line.push(':');
            line.push_str(value);
        }
        line.push('}');
        if let Ok(mut file) = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        {
            let _ = writeln!(file, "{line}");
        }
    }
}

fn trace2_target(
    env_target: &str,
    config_target: &str,
    env_brief: &str,
    config_brief: &str,
) -> Option<Trace2Target> {
    let target = std::env::var_os(env_target)
        .filter(|value| !value.is_empty())
        .or_else(|| trace2_config_value(config_target).map(Into::into))?;
    let brief = std::env::var(env_brief)
        .ok()
        .and_then(|value| parse_git_bool(&value))
        .or_else(|| {
            trace2_config_value(config_brief)
                .as_deref()
                .and_then(parse_git_bool)
        })
        .unwrap_or(false);
    let writer = trace2_writer(&target)?;
    Some(Trace2Target { writer, brief })
}

fn trace2_event_target() -> Option<Trace2EventTarget> {
    let target = std::env::var_os("GIT_TRACE2_EVENT")
        .filter(|value| !value.is_empty())
        .or_else(|| trace2_config_value("eventtarget").map(Into::into))?;
    if target == "1" || target == "2" {
        return None;
    }
    let path = PathBuf::from(target);
    if path.is_dir() {
        let max_files = std::env::var("GIT_TRACE2_MAX_FILES")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(usize::MAX);
        let file_count = fs::read_dir(&path)
            .ok()
            .map(|entries| entries.flatten().count())
            .unwrap_or(0);
        if file_count >= max_files {
            return Some(Trace2EventTarget {
                path: path.join("git-trace2-discard"),
                discard_only: true,
            });
        }
        return Some(Trace2EventTarget {
            path: path.join(format!("zmin-trace2-event-{}.log", std::process::id())),
            discard_only: false,
        });
    }
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
        && !parent.exists()
        && fs::create_dir_all(parent).is_err()
    {
        return None;
    }
    Some(Trace2EventTarget {
        path,
        discard_only: false,
    })
}

fn trace2_config_value(key: &str) -> Option<String> {
    if let Some(value) = global_command_config_value("trace2", key) {
        return Some(value);
    }
    let entries = read_protected_config_entries().ok()?;
    entries
        .into_iter()
        .rev()
        .find(|entry| entry.section == "trace2" && entry.subsection.is_empty() && entry.key == key)
        .map(|entry| entry.value)
}

fn trace2_patterns(env_key: &str, config_key: &str) -> Vec<String> {
    std::env::var(env_key)
        .ok()
        .or_else(|| {
            if env_key == "GIT_TRACE2_ENVVARS" {
                std::env::var("GIT_TRACE2_ENV_VARS").ok()
            } else {
                None
            }
        })
        .or_else(|| trace2_config_value(config_key))
        .map(parse_trace2_pattern_list)
        .unwrap_or_default()
}

fn trace2_redact_enabled() -> bool {
    std::env::var("GIT_TRACE2_REDACT")
        .ok()
        .and_then(|value| parse_git_bool(&value))
        .unwrap_or(true)
}

fn trace2_writer(target: &std::ffi::OsStr) -> Option<Trace2Writer> {
    if target == "1" || target == "2" {
        return Some(Trace2Writer::Stderr);
    }
    let path = PathBuf::from(target);
    if path.is_dir() {
        return Some(Trace2Writer::File(trace2_directory_file(&path)));
    }
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
        && !parent.exists()
        && fs::create_dir_all(parent).is_err()
    {
        return None;
    }
    Some(Trace2Writer::File(path))
}

fn trace2_directory_file(path: &Path) -> PathBuf {
    path.join(format!("zmin-trace2-{}.log", std::process::id()))
}

fn format_command_argv(command_args: &[String], redact: bool) -> String {
    let exe = std::env::current_exe()
        .ok()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "zmin".to_owned());
    std::iter::once(exe)
        .chain(
            command_args
                .iter()
                .map(|arg| shell_render_arg(&maybe_redact_value(arg, redact))),
        )
        .collect::<Vec<_>>()
        .join(" ")
}

fn event_command_argv(command_args: &[String]) -> Vec<String> {
    std::iter::once(
        std::env::current_exe()
            .ok()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "zmin".to_owned()),
    )
    .chain(command_args.iter().cloned())
    .collect()
}

fn shell_render_arg(arg: &str) -> String {
    if arg.chars().all(|ch| {
        ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '-' | '_' | ':' | '=' | '@')
    }) {
        arg.to_owned()
    } else {
        format!("'{}'", arg.replace('\'', "'\\''"))
    }
}

fn format_perf_line(
    depth: usize,
    thread_name: &str,
    event: &str,
    t_abs_field: &str,
    t_rel_field: &str,
    category: &str,
    payload: &str,
) -> String {
    format!(
        "d{depth} | {thread_name:<24} | {event:<12} |     | {t_abs_field:>9} | {t_rel_field:>9} | {category:<12} | {payload}"
    )
}

fn pattern_list_matches(patterns: &[String], name: &str) -> bool {
    patterns
        .iter()
        .any(|pattern| wildcard_match_pathspec(pattern, name, false, true))
}

fn maybe_redact_value(value: &str, redact: bool) -> String {
    if !redact {
        return value.to_owned();
    }
    redact_url_credentials(value)
}

fn parse_trace2_pattern_list(value: String) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect()
}

fn collect_trace2_config_entries() -> Vec<ConfigEntry> {
    let mut entries = BTreeMap::new();
    if let Ok(protected) = read_protected_config_entries() {
        for entry in protected {
            entries.insert(entry.name(), entry);
        }
    }
    if let Ok(repo) = find_repo_or_bare()
        && let Ok(repo_entries) = read_config_entries(&repo)
    {
        for entry in repo_entries {
            entries.insert(entry.name(), entry);
        }
    }
    entries.into_values().collect()
}

fn trace2_session_id() -> String {
    let local = format!(
        "{}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros(),
        std::process::id()
    );
    if let Ok(parent) = std::env::var("ZMIN_TRACE2_PARENT_SID")
        && !parent.is_empty()
    {
        return format!("{parent}/{local}");
    }
    local
}

fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn json_string_array(values: &[String], redact: bool) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|value| json_string(&maybe_redact_value(value, redact)))
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn pending_trace2_state() -> &'static Mutex<PendingTrace2State> {
    PENDING_TRACE2_STATE.get_or_init(|| Mutex::new(PendingTrace2State::default()))
}

fn take_pending_trace2_state() -> PendingTrace2State {
    let state = pending_trace2_state();
    let mut guard = state.lock().expect("pending trace2 state lock");
    std::mem::take(&mut *guard)
}

pub(crate) fn clear_pending_trace2_state() {
    let state = pending_trace2_state();
    let mut guard = state.lock().expect("pending trace2 state lock");
    *guard = PendingTrace2State::default();
}

pub(crate) fn override_pending_trace2_command(command_name: String, hierarchy: String) {
    let state = pending_trace2_state();
    let mut guard = state.lock().expect("pending trace2 state lock");
    guard.command_name_override = Some(command_name);
    guard.command_hierarchy_override = Some(hierarchy);
}

pub(crate) fn push_pending_trace2_perf_cmd_name(
    depth: usize,
    thread_name: String,
    command_name: String,
    hierarchy: String,
) {
    let state = pending_trace2_state();
    let mut guard = state.lock().expect("pending trace2 state lock");
    guard.actions.push(PendingTrace2Action::PerfCmdName {
        depth,
        thread_name,
        command_name,
        hierarchy,
    });
}

pub(crate) fn push_pending_trace2_perf_child_start(
    depth: usize,
    thread_name: String,
    class: String,
    argv: String,
) {
    let state = pending_trace2_state();
    let mut guard = state.lock().expect("pending trace2 state lock");
    guard.actions.push(PendingTrace2Action::PerfChildStart {
        depth,
        thread_name,
        class,
        argv,
    });
}

pub(crate) fn push_pending_trace2_perf_alias(
    depth: usize,
    thread_name: String,
    alias: String,
    argv: String,
) {
    let state = pending_trace2_state();
    let mut guard = state.lock().expect("pending trace2 state lock");
    guard.actions.push(PendingTrace2Action::PerfAlias {
        depth,
        thread_name,
        alias,
        argv,
    });
}

pub(crate) fn push_pending_trace2_perf_def_params(depth: usize, thread_name: String) {
    let state = pending_trace2_state();
    let mut guard = state.lock().expect("pending trace2 state lock");
    guard
        .actions
        .push(PendingTrace2Action::PerfDefParams { depth, thread_name });
}

fn redact_url_credentials(value: &str) -> String {
    let Some(scheme_index) = value.find("://") else {
        return value.to_owned();
    };
    let authority_start = scheme_index + 3;
    let authority_end = value[authority_start..]
        .find(['/', '?', '#'])
        .map(|offset| authority_start + offset)
        .unwrap_or(value.len());
    let authority = &value[authority_start..authority_end];
    let Some(at_index) = authority.rfind('@') else {
        return value.to_owned();
    };
    let host = &authority[at_index + 1..];
    let mut redacted = String::with_capacity(value.len());
    redacted.push_str(&value[..authority_start]);
    redacted.push_str("redacted@");
    redacted.push_str(host);
    redacted.push_str(&value[authority_end..]);
    redacted
}
