use crate::runtime;

pub(crate) fn dispatch(
    command: runtime::Command,
    raw_args: &[String],
) -> std::result::Result<(), runtime::CliError> {
    match command {
        runtime::Command::Column {
            command,
            no_command,
            mode,
            no_mode,
            raw_mode,
            width,
            no_width,
            indent,
            no_indent,
            nl,
            no_nl,
            padding,
            no_padding,
        } => run_column(
            command,
            no_command,
            mode,
            no_mode,
            raw_mode,
            width,
            no_width,
            indent,
            no_indent,
            nl,
            no_nl,
            padding,
            no_padding,
            raw_args,
        ),
        runtime::Command::Stripspace {
            strip_comments,
            comment_lines,
        } => run_stripspace(strip_comments > 0, comment_lines > 0, raw_args),
        _ => unreachable!("non-text command dispatched to text"),
    }
}

pub(crate) fn run_column(
    command: Option<String>,
    no_command: bool,
    mode: Option<String>,
    no_mode: bool,
    raw_mode: Option<String>,
    width: Option<String>,
    no_width: bool,
    indent: Option<String>,
    no_indent: bool,
    nl: Option<String>,
    no_nl: bool,
    padding: Option<String>,
    no_padding: bool,
    raw_args: &[String],
) -> std::result::Result<(), runtime::CliError> {
    super::text_commands::column(
        command.as_deref(),
        no_command,
        mode.as_deref(),
        no_mode,
        raw_mode,
        width,
        no_width,
        indent.as_deref(),
        no_indent,
        nl.as_deref(),
        no_nl,
        padding,
        no_padding,
        raw_args,
    )
}

pub(crate) fn run_stripspace(
    strip_comments: bool,
    comment_lines: bool,
    raw_args: &[String],
) -> std::result::Result<(), runtime::CliError> {
    super::text_commands::stripspace(strip_comments, comment_lines, raw_args)
}
