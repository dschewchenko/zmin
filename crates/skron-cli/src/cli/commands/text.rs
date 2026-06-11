use crate::runtime;

pub(crate) fn dispatch(command: runtime::Command) -> std::result::Result<(), runtime::CliError> {
    match command {
        runtime::Command::Column {
            mode,
            raw_mode,
            width,
            padding,
        } => run_column(mode, raw_mode, width, padding),
        runtime::Command::Stripspace {
            strip_comments,
            comment_lines,
        } => run_stripspace(strip_comments, comment_lines),
        _ => unreachable!("non-text command dispatched to text"),
    }
}

pub(crate) fn run_column(
    mode: Option<String>,
    raw_mode: Option<u32>,
    width: Option<usize>,
    padding: Option<usize>,
) -> std::result::Result<(), runtime::CliError> {
    runtime::text_commands::column(mode.as_deref(), raw_mode, width, padding)
}

pub(crate) fn run_stripspace(
    strip_comments: bool,
    comment_lines: bool,
) -> std::result::Result<(), runtime::CliError> {
    runtime::text_commands::stripspace(strip_comments, comment_lines)
}
