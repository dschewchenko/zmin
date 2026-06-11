use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Write as _};

use clap::builder::ValueRange;

use crate::runtime::CliError;

const V2_32_COMMANDS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/src/compat/v2_32_commands.txt"
));
const V2_47_COMMANDS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/src/compat/v2_47_commands.txt"
));

#[derive(clap::ValueEnum, Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompatProfile {
    #[value(name = "v2-32")]
    V2_32,
    #[value(name = "v2-47")]
    V2_47,
    #[value(name = "modern")]
    Modern,
}

#[derive(clap::ValueEnum, Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompatFormat {
    #[value(name = "text")]
    Text,
    #[value(name = "json")]
    Json,
}

#[derive(Clone, Eq, PartialEq, Ord, PartialOrd)]
struct CommandSpec {
    name: String,
    args: Vec<ArgSpec>,
    groups: Vec<GroupSpec>,
    positional_order: Vec<String>,
    aliases: Vec<String>,
}

#[derive(Clone, Eq, PartialEq, Ord, PartialOrd)]
struct ArgSpec {
    id: String,
    long: Option<String>,
    short: Option<String>,
    num_args: String,
    required: bool,
    default_values: Vec<String>,
    conflicts_with: Vec<String>,
    positional: bool,
    action: String,
}

#[derive(Clone, Eq, PartialEq, Ord, PartialOrd)]
struct GroupSpec {
    id: String,
    required: bool,
    args: Vec<String>,
}

#[derive(Clone)]
struct CompatibilityCommand {
    name: String,
    ready: bool,
    readiness_note: Option<String>,
    spec: CommandSpec,
}

struct CompatibilityReport {
    profile: String,
    expected_count: Option<usize>,
    implemented_count: usize,
    baseline_match_count: usize,
    missing_count: usize,
    extra_count: usize,
    missing: Vec<String>,
    extra: Vec<String>,
    entries: Vec<CompatibilityCommand>,
    not_ready_count: usize,
}

pub fn run(profile: CompatProfile, format: CompatFormat) -> Result<(), CliError> {
    let report = collect_report(profile);
    match format {
        CompatFormat::Text => print!("{}", render_text_report(&report)),
        CompatFormat::Json => print!("{}", render_json_report(&report)),
    }
    Ok(())
}

fn collect_report(profile: CompatProfile) -> CompatibilityReport {
    let expected = expected_commands(profile);
    let current = collect_command_specs();
    let current_names: BTreeSet<_> = current.keys().cloned().collect();

    let (baseline_match_count, missing, extra) = if let Some(expected) = expected.as_ref() {
        (
            current_names.intersection(expected).count(),
            expected
                .difference(&current_names)
                .cloned()
                .collect::<Vec<_>>(),
            current_names
                .difference(expected)
                .cloned()
                .collect::<Vec<_>>(),
        )
    } else {
        (current_names.len(), Vec::new(), Vec::new())
    };

    let not_ready = explicit_not_ready();
    let mut entries = Vec::with_capacity(current.len());
    let mut not_ready_count = 0usize;
    for (name, spec) in current {
        let readiness_note = not_ready.get(name.as_str()).map(|note| (*note).to_owned());
        let ready = readiness_note.is_none();
        if !ready {
            not_ready_count += 1;
        }
        entries.push(CompatibilityCommand {
            name,
            ready,
            readiness_note,
            spec,
        });
    }

    CompatibilityReport {
        profile: profile.to_string(),
        expected_count: expected.as_ref().map(BTreeSet::len),
        implemented_count: entries.len(),
        baseline_match_count,
        missing_count: missing.len(),
        extra_count: extra.len(),
        missing,
        extra,
        entries,
        not_ready_count,
    }
}

fn explicit_not_ready() -> BTreeMap<&'static str, &'static str> {
    BTreeMap::new()
}

fn expected_commands(profile: CompatProfile) -> Option<BTreeSet<String>> {
    match profile {
        CompatProfile::V2_32 => Some(parse_command_list(V2_32_COMMANDS)),
        CompatProfile::V2_47 => Some(parse_command_list(V2_47_COMMANDS)),
        CompatProfile::Modern => None,
    }
}

fn parse_command_list(contents: &'static str) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let name = line.split_whitespace().next().unwrap_or_default();
        if name.starts_with("git-") {
            set.insert(name.to_owned());
        } else if !name.is_empty() {
            set.insert(format!("git-{name}"));
        }
    }
    set
}

fn collect_command_specs() -> BTreeMap<String, CommandSpec> {
    let mut out = BTreeMap::new();
    let root = crate::cli::command_definition();
    for command in root.get_subcommands() {
        if command.get_name() == "compatibility" {
            continue;
        }
        visit_command(command, &[], &mut out);
    }
    out.insert("git-help".to_owned(), synthetic_help_spec());
    out
}

fn synthetic_help_spec() -> CommandSpec {
    CommandSpec {
        name: "help".to_owned(),
        args: vec![ArgSpec {
            id: "args".to_owned(),
            long: None,
            short: None,
            num_args: "1..".to_owned(),
            required: false,
            default_values: Vec::new(),
            conflicts_with: Vec::new(),
            positional: true,
            action: "Append".to_owned(),
        }],
        groups: Vec::new(),
        positional_order: vec!["args".to_owned()],
        aliases: Vec::new(),
    }
}

fn visit_command(
    command: &clap::Command,
    prefix: &[String],
    out: &mut BTreeMap<String, CommandSpec>,
) {
    let mut path = prefix.to_vec();
    path.push(command.get_name().to_owned());
    let command_name = format!("git-{}", path.join("-"));
    out.insert(
        command_name,
        CommandSpec {
            name: command.get_name().to_owned(),
            args: collect_argument_specs(command),
            groups: collect_group_specs(command),
            positional_order: positional_order(command),
            aliases: collect_aliases(command),
        },
    );

    for child in command.get_subcommands() {
        visit_command(child, &path, out);
    }
}

fn collect_argument_specs(command: &clap::Command) -> Vec<ArgSpec> {
    let mut specs = Vec::new();
    for arg in command.get_arguments() {
        let conflicts_with = command
            .get_arg_conflicts_with(arg)
            .into_iter()
            .map(|conflict| conflict.get_id().to_string())
            .collect::<Vec<_>>();

        specs.push(ArgSpec {
            id: arg.get_id().to_string(),
            long: arg.get_long().map(|value| format!("--{value}")),
            short: arg.get_short().map(|value| format!("-{value}")),
            num_args: num_args_to_string(arg.get_num_args()),
            required: arg.is_required_set(),
            default_values: default_values_to_string(arg.get_default_values()),
            conflicts_with,
            positional: arg.is_positional(),
            action: format!("{:?}", arg.get_action()),
        });
    }
    specs.sort();
    specs
}

fn collect_group_specs(command: &clap::Command) -> Vec<GroupSpec> {
    let mut groups = Vec::new();
    for group in command.get_groups() {
        let mut args = group
            .get_args()
            .map(|id| id.as_str().to_owned())
            .collect::<Vec<_>>();
        args.sort();
        groups.push(GroupSpec {
            id: group.get_id().as_str().to_owned(),
            required: group.is_required_set(),
            args,
        });
    }
    groups.sort();
    groups
}

fn positional_order(command: &clap::Command) -> Vec<String> {
    command
        .get_positionals()
        .map(|arg| arg.get_id().to_string())
        .collect()
}

fn collect_aliases(command: &clap::Command) -> Vec<String> {
    let mut aliases = command
        .get_visible_aliases()
        .map(|alias| alias.to_owned())
        .collect::<Vec<_>>();
    aliases.sort();
    aliases
}

fn num_args_to_string(num_args: Option<ValueRange>) -> String {
    num_args
        .map(|value| value.to_string())
        .unwrap_or_else(|| "1".to_owned())
}

fn default_values_to_string(defaults: &[clap::builder::OsStr]) -> Vec<String> {
    defaults
        .iter()
        .map(|value| value.to_string_lossy().into_owned())
        .collect()
}

impl fmt::Display for CompatProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CompatProfile::V2_32 => write!(f, "v2-32"),
            CompatProfile::V2_47 => write!(f, "v2-47"),
            CompatProfile::Modern => write!(f, "modern"),
        }
    }
}

fn render_text_report(report: &CompatibilityReport) -> String {
    let mut out = String::new();
    let expected = report
        .expected_count
        .map(|count| count.to_string())
        .unwrap_or_else(|| "n/a".to_owned());

    let _ = writeln!(out, "Compatibility profile: {}", report.profile);
    let _ = writeln!(
        out,
        "Commands: expected {}, implemented {}, matching baseline {}, missing {}, extra {}",
        expected,
        report.implemented_count,
        report.baseline_match_count,
        report.missing_count,
        report.extra_count
    );
    let _ = writeln!(
        out,
        "Ready commands: {} (explicitly not ready: {})",
        report.implemented_count - report.not_ready_count,
        report.not_ready_count
    );

    if !report.missing.is_empty() {
        let _ = writeln!(out, "Missing baseline commands:");
        for name in &report.missing {
            let _ = writeln!(out, "- {name}");
        }
    }
    if !report.extra.is_empty() {
        let _ = writeln!(out, "Additional commands:");
        for name in &report.extra {
            let _ = writeln!(out, "- {name}");
        }
    }

    let _ = writeln!(out);
    for command in &report.entries {
        let status = if command.ready { "ready" } else { "not-ready" };
        let _ = write!(out, "{}: {}", command.name, status);
        if let Some(note) = command.readiness_note.as_deref() {
            let _ = writeln!(out, " ({note})");
        } else {
            let _ = writeln!(out);
        }

        if !command.spec.aliases.is_empty() {
            let _ = writeln!(out, "  aliases: {}", command.spec.aliases.join(", "));
        }
        if !command.spec.positional_order.is_empty() {
            let _ = writeln!(
                out,
                "  positional sequence: {}",
                command.spec.positional_order.join(", ")
            );
        }
        if !command.spec.groups.is_empty() {
            let _ = writeln!(out, "  argument groups:");
            for group in &command.spec.groups {
                let _ = writeln!(
                    out,
                    "    {} (required={}) [{}]",
                    group.id,
                    group.required,
                    group.args.join(", ")
                );
            }
        }
        if !command.spec.args.is_empty() {
            let _ = writeln!(out, "  arguments:");
            for arg in &command.spec.args {
                let long = arg.long.as_deref().unwrap_or("n/a");
                let short = arg.short.as_deref().unwrap_or("n/a");
                let defaults = if arg.default_values.is_empty() {
                    "none".to_owned()
                } else {
                    arg.default_values.join(", ")
                };
                let conflicts = if arg.conflicts_with.is_empty() {
                    "none".to_owned()
                } else {
                    arg.conflicts_with.join(", ")
                };
                let _ = writeln!(
                    out,
                    "    {}: long={long} short={short} num_args={} required={} positional={} action={} defaults=[{defaults}] conflicts=[{conflicts}]",
                    arg.id, arg.num_args, arg.required, arg.positional, arg.action
                );
            }
        }
        let _ = writeln!(out);
    }

    out
}

fn render_json_report(report: &CompatibilityReport) -> String {
    let mut out = String::new();
    let expected = report
        .expected_count
        .map(|count| count.to_string())
        .unwrap_or_else(|| "null".to_owned());

    let _ = writeln!(out, "{{");
    let _ = writeln!(out, "  \"profile\": \"{}\",", escape_json(&report.profile));
    let _ = writeln!(out, "  \"expected\": {},", expected);
    let _ = writeln!(
        out,
        "  \"counts\": {{\"implemented\": {}, \"matching_baseline\": {}, \"missing\": {}, \"extra\": {}}},",
        report.implemented_count,
        report.baseline_match_count,
        report.missing_count,
        report.extra_count
    );
    let _ = writeln!(out, "  \"explicit_not_ready\": {},", report.not_ready_count);
    let _ = writeln!(
        out,
        "  \"missing\": [{}],",
        report
            .missing
            .iter()
            .map(|name| format!("\"{}\"", escape_json(name)))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let _ = writeln!(
        out,
        "  \"additional\": [{}],",
        report
            .extra
            .iter()
            .map(|name| format!("\"{}\"", escape_json(name)))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let _ = writeln!(out, "  \"commands\": [");

    for (index, command) in report.entries.iter().enumerate() {
        if index > 0 {
            let _ = writeln!(out, "    ,");
        }
        let _ = writeln!(out, "    {{");
        let _ = writeln!(out, "      \"name\": \"{}\",", escape_json(&command.name));
        let _ = writeln!(
            out,
            "      \"ready\": {},",
            if command.ready { "true" } else { "false" }
        );
        let _ = writeln!(
            out,
            "      \"readiness_note\": {},",
            command.readiness_note.as_deref().map_or_else(
                || "null".to_owned(),
                |note| format!("\"{}\"", escape_json(note))
            )
        );
        let _ = writeln!(
            out,
            "      \"aliases\": [{}],",
            command
                .spec
                .aliases
                .iter()
                .map(|alias| format!("\"{}\"", escape_json(alias)))
                .collect::<Vec<_>>()
                .join(", ")
        );
        let _ = writeln!(
            out,
            "      \"positional_order\": [{}],",
            command
                .spec
                .positional_order
                .iter()
                .map(|arg| format!("\"{}\"", escape_json(arg)))
                .collect::<Vec<_>>()
                .join(", ")
        );
        let _ = writeln!(
            out,
            "      \"groups\": [{}],",
            command
                .spec
                .groups
                .iter()
                .map(|group| {
                    format!(
                        "{{\"id\":\"{}\",\"required\":{},\"args\":[{}]}}",
                        escape_json(&group.id),
                        if group.required { "true" } else { "false" },
                        group
                            .args
                            .iter()
                            .map(|arg| format!("\"{}\"", escape_json(arg)))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                })
                .collect::<Vec<_>>()
                .join(", ")
        );
        let _ = writeln!(out, "      \"args\": [");
        for (arg_index, arg) in command.spec.args.iter().enumerate() {
            if arg_index > 0 {
                let _ = writeln!(out, "        ,");
            }
            let _ = writeln!(out, "        {{");
            let _ = writeln!(out, "          \"id\": \"{}\",", escape_json(&arg.id));
            let _ = writeln!(
                out,
                "          \"long\": {},",
                arg.long.as_deref().map_or_else(
                    || "null".to_owned(),
                    |value| format!("\"{}\"", escape_json(value))
                )
            );
            let _ = writeln!(
                out,
                "          \"short\": {},",
                arg.short.as_deref().map_or_else(
                    || "null".to_owned(),
                    |value| format!("\"{}\"", escape_json(value))
                )
            );
            let _ = writeln!(out, "          \"num_args\": \"{}\",", arg.num_args);
            let _ = writeln!(out, "          \"required\": {},", arg.required);
            let _ = writeln!(out, "          \"positional\": {},", arg.positional);
            let _ = writeln!(
                out,
                "          \"action\": \"{}\",",
                escape_json(&arg.action)
            );
            let _ = writeln!(
                out,
                "          \"default_values\": [{}],",
                arg.default_values
                    .iter()
                    .map(|value| format!("\"{}\"", escape_json(value)))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            let _ = writeln!(
                out,
                "          \"conflicts_with\": [{}]",
                arg.conflicts_with
                    .iter()
                    .map(|value| format!("\"{}\"", escape_json(value)))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            let _ = writeln!(out, "        }}");
        }
        let _ = writeln!(out, "      ]");
        let _ = write!(out, "    }}");
        let _ = writeln!(out);
    }

    let _ = writeln!(out, "  ]");
    let _ = writeln!(out, "}}");
    out
}

fn escape_json(value: &str) -> String {
    let mut escaped = String::new();
    for byte in value.bytes() {
        match byte {
            b'\\' => escaped.push_str("\\\\"),
            b'"' => escaped.push_str("\\\""),
            b'\n' => escaped.push_str("\\n"),
            b'\r' => escaped.push_str("\\r"),
            b'\t' => escaped.push_str("\\t"),
            _ => escaped.push(byte as char),
        }
    }
    escaped
}
