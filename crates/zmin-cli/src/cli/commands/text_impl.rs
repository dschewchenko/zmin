use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ColumnMode {
    Plain,
    Column,
    Row,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ColumnSpec {
    mode: ColumnMode,
    dense: bool,
}

pub(crate) fn column(
    _command: Option<&str>,
    _no_command: bool,
    mode: Option<&str>,
    _no_mode: bool,
    raw_mode: Option<String>,
    width: Option<String>,
    _no_width: bool,
    indent: Option<&str>,
    _no_indent: bool,
    nl: Option<&str>,
    _no_nl: bool,
    padding: Option<String>,
    _no_padding: bool,
    raw_args: &[String],
) -> Result<()> {
    let options = parse_column_options(raw_args, raw_mode, mode, width, indent, nl, padding)?;
    let command_name = parse_column_command_name(raw_args)?;
    let command_mode = column_command_mode(command_name.as_deref());
    let spec = parse_column_mode(
        options.mode.as_deref().or(command_mode.as_deref()),
        options.raw_mode,
    )?;
    let width = options.width.unwrap_or(80);
    let padding = options.padding.unwrap_or(1);
    let indent = options.indent.as_deref().unwrap_or("");
    let nl = options.nl.as_deref().unwrap_or("\n");
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    let items = input
        .split_terminator('\n')
        .map(|line| line.trim_end_matches('\r').to_owned())
        .collect::<Vec<_>>();
    let output = render_columns(&items, spec, width, indent, nl, padding);
    if !output.is_empty() {
        print!("{output}");
    }
    Ok(())
}

fn parse_column_command_name(raw_args: &[String]) -> Result<Option<String>> {
    let first = raw_args.get(1).map(String::as_str);
    for arg in raw_args.iter().skip(2).map(String::as_str) {
        if arg == "--command"
            || arg.starts_with("--command=")
            || arg == "--no-command"
            || arg.starts_with("--no-command=")
        {
            return Err(CliError::Fatal {
                code: 128,
                message: "--command must be the first argument".into(),
            });
        }
    }
    match first {
        Some("--command") => Err(CliError::Fatal {
            code: 128,
            message: "--command must be the first argument".into(),
        }),
        Some(value) if value.starts_with("--command=") => {
            Ok(Some(value.trim_start_matches("--command=").to_owned()))
        }
        _ => Ok(None),
    }
}

fn column_command_mode(command_name: Option<&str>) -> Option<String> {
    let command_name = command_name?;
    if command_name.is_empty() {
        return None;
    }
    let repo = find_repo().ok()?;
    let value = read_config_value(&repo, &format!("column.{command_name}"))
        .ok()
        .flatten()?;
    Some(match value.as_str() {
        "dense" | "nodense" => "plain".to_owned(),
        _ => value,
    })
}

#[derive(Default)]
struct ParsedColumnOptions {
    mode: Option<String>,
    raw_mode: Option<u32>,
    width: Option<usize>,
    indent: Option<String>,
    nl: Option<String>,
    padding: Option<usize>,
}

fn parse_column_options(
    raw_args: &[String],
    raw_mode: Option<String>,
    parsed_mode: Option<&str>,
    parsed_width: Option<String>,
    parsed_indent: Option<&str>,
    parsed_nl: Option<&str>,
    parsed_padding: Option<String>,
) -> Result<ParsedColumnOptions> {
    let mut options = ParsedColumnOptions {
        mode: None,
        raw_mode: None,
        width: match parsed_width {
            Some(ref value) => Some(parse_column_usize(value, "width")?),
            None => None,
        },
        indent: parsed_indent.map(ToOwned::to_owned),
        nl: parsed_nl.map(ToOwned::to_owned),
        padding: match parsed_padding {
            Some(ref value) => Some(parse_column_usize(value, "padding")?),
            None => None,
        },
    };

    let mut idx = 1usize;
    while idx < raw_args.len() {
        let arg = raw_args[idx].as_str();
        match arg {
            "--no-command" => {}
            "--no-mode" => {
                options.mode = None;
                options.raw_mode = None;
            }
            "--mode" => {
                options.mode = Some(String::new());
                options.raw_mode = None;
            }
            "--raw-mode" => {
                idx += 1;
                let Some(value) = raw_args.get(idx) else {
                    return Err(CliError::Stderr {
                        code: 129,
                        text: "error: option `raw-mode' requires a value\n".into(),
                    });
                };
                options.raw_mode = Some(parse_column_raw_mode(value)?);
                options.mode = None;
            }
            "--no-width" => options.width = None,
            "--width" => {
                idx += 1;
                let Some(value) = raw_args.get(idx) else {
                    return Err(CliError::Stderr {
                        code: 129,
                        text: "error: option `width' requires a value\n".into(),
                    });
                };
                options.width = Some(parse_column_usize(value, "width")?);
            }
            "--no-indent" => options.indent = None,
            "--indent" => {
                idx += 1;
                let Some(value) = raw_args.get(idx) else {
                    return Err(CliError::Stderr {
                        code: 129,
                        text: "error: option `indent' requires a value\n".into(),
                    });
                };
                options.indent = Some(value.clone());
            }
            "--no-nl" => options.nl = None,
            "--nl" => {
                idx += 1;
                let Some(value) = raw_args.get(idx) else {
                    return Err(CliError::Stderr {
                        code: 129,
                        text: "error: option `nl' requires a value\n".into(),
                    });
                };
                options.nl = Some(value.clone());
            }
            "--no-padding" => options.padding = Some(0),
            "--padding" => {
                idx += 1;
                let Some(value) = raw_args.get(idx) else {
                    return Err(CliError::Stderr {
                        code: 129,
                        text: "error: option `padding' requires a value\n".into(),
                    });
                };
                options.padding = Some(parse_column_usize(value, "padding")?);
            }
            _ => {
                if let Some(value) = arg.strip_prefix("--mode=") {
                    options.mode = Some(value.to_owned());
                    options.raw_mode = None;
                } else if let Some(value) = arg.strip_prefix("--raw-mode=") {
                    options.raw_mode = Some(parse_column_raw_mode(value)?);
                    options.mode = None;
                } else if let Some(value) = arg.strip_prefix("--width=") {
                    options.width = Some(parse_column_usize(value, "width")?);
                } else if let Some(value) = arg.strip_prefix("--indent=") {
                    options.indent = Some(value.to_owned());
                } else if let Some(value) = arg.strip_prefix("--nl=") {
                    options.nl = Some(value.to_owned());
                } else if let Some(value) = arg.strip_prefix("--padding=") {
                    options.padding = Some(parse_column_usize(value, "padding")?);
                } else if arg.starts_with("--no-command=") {
                    return Err(CliError::Stderr {
                        code: 129,
                        text: "error: option `no-command' takes no value\n".into(),
                    });
                }
            }
        }
        idx += 1;
    }

    if options.mode.is_none() && options.raw_mode.is_none() {
        options.mode = parsed_mode.map(ToOwned::to_owned);
        options.raw_mode = match raw_mode {
            Some(value) => Some(parse_column_raw_mode(&value)?),
            None => None,
        };
    }

    Ok(options)
}

fn parse_column_usize(value: &str, name: &str) -> Result<usize> {
    value.parse::<usize>().map_err(|_| CliError::Stderr {
        code: 129,
        text: format!("error: option `{name}' expects a numerical value\n"),
    })
}

fn parse_column_raw_mode(value: &str) -> Result<u32> {
    value.parse::<u32>().map_err(|_| CliError::Stderr {
        code: 129,
        text: "error: option `raw-mode' expects a non-negative integer value with an optional k/m/g suffix\n".into(),
    })
}

fn parse_column_mode(mode: Option<&str>, raw_mode: Option<u32>) -> Result<ColumnSpec> {
    if let Some(raw_mode) = raw_mode {
        return Ok(if raw_mode == 16 {
            ColumnSpec {
                mode: ColumnMode::Column,
                dense: false,
            }
        } else if raw_mode == 17 {
            ColumnSpec {
                mode: ColumnMode::Row,
                dense: false,
            }
        } else {
            ColumnSpec {
                mode: ColumnMode::Plain,
                dense: false,
            }
        });
    }
    let mut spec = ColumnSpec {
        mode: ColumnMode::Plain,
        dense: false,
    };
    let Some(mode) = mode else {
        return Ok(spec);
    };
    for token in mode.split(',') {
        match token {
            "plain" => spec.mode = ColumnMode::Plain,
            "column" => spec.mode = ColumnMode::Column,
            "row" => spec.mode = ColumnMode::Row,
            "" => spec.mode = ColumnMode::Column,
            "dense" => {
                if spec.mode == ColumnMode::Plain {
                    spec.mode = ColumnMode::Column;
                }
                spec.dense = true;
            }
            "nodense" => {
                if spec.mode == ColumnMode::Plain {
                    spec.mode = ColumnMode::Column;
                }
                spec.dense = false;
            }
            other => {
                return Err(CliError::Stderr {
                    code: 129,
                    text: format!("error: unsupported option '{other}'\n"),
                });
            }
        }
    }
    Ok(spec)
}

fn render_columns(
    items: &[String],
    spec: ColumnSpec,
    width: usize,
    indent: &str,
    nl: &str,
    padding: usize,
) -> String {
    if items.is_empty() {
        return String::new();
    }
    if spec.mode == ColumnMode::Plain {
        let mut out = String::new();
        for item in items {
            out.push_str(item);
            out.push('\n');
        }
        return out;
    }
    let columns = best_column_count(items, spec, width, padding);
    let rows = items.len().div_ceil(columns);
    let column_widths = column_widths(items, spec.mode, columns, rows, spec.dense);
    let mut out = String::new();
    for row in 0..rows {
        out.push_str(indent);
        let mut last_col = 0;
        for col in 0..columns {
            if column_item(items, spec.mode, columns, rows, row, col).is_some() {
                last_col = col;
            }
        }
        for (col, column_width) in column_widths.iter().enumerate().take(last_col + 1) {
            let Some(item) = column_item(items, spec.mode, columns, rows, row, col) else {
                continue;
            };
            out.push_str(item);
            if col < last_col {
                let spaces = column_width.saturating_sub(item.len()) + padding;
                out.push_str(&" ".repeat(spaces));
            }
        }
        out.push_str(nl);
    }
    out
}

fn best_column_count(items: &[String], spec: ColumnSpec, width: usize, padding: usize) -> usize {
    for columns in (1..=items.len()).rev() {
        let rows = items.len().div_ceil(columns);
        let widths = column_widths(items, spec.mode, columns, rows, spec.dense);
        let total = if spec.dense {
            widths.iter().sum::<usize>() + padding.saturating_mul(columns.saturating_sub(1))
        } else {
            widths
                .first()
                .copied()
                .unwrap_or(0)
                .saturating_add(padding)
                .saturating_mul(columns)
        };
        if total <= width {
            return columns;
        }
    }
    1
}

fn column_widths(
    items: &[String],
    mode: ColumnMode,
    columns: usize,
    rows: usize,
    dense: bool,
) -> Vec<usize> {
    let mut widths = vec![0; columns];
    for row in 0..rows {
        for (col, width) in widths.iter_mut().enumerate() {
            if let Some(item) = column_item(items, mode, columns, rows, row, col) {
                *width = (*width).max(item.len());
            }
        }
    }
    if !dense {
        let width = widths.iter().copied().max().unwrap_or(0);
        widths.fill(width);
    }
    widths
}

fn column_item(
    items: &[String],
    mode: ColumnMode,
    columns: usize,
    rows: usize,
    row: usize,
    col: usize,
) -> Option<&str> {
    let idx = match mode {
        ColumnMode::Plain => return None,
        ColumnMode::Column => col * rows + row,
        ColumnMode::Row => row * columns + col,
    };
    items.get(idx).map(String::as_str)
}

pub(crate) fn stripspace(
    strip_comments: bool,
    comment_lines: bool,
    raw_args: &[String],
) -> Result<()> {
    if strip_comments && comment_lines {
        let (current, previous) = stripspace_conflicting_options(raw_args)
            .unwrap_or(("-c".to_owned(), "-s".to_owned()));
        return Err(CliError::Stderr {
            code: 129,
            text: format!("error: options '{current}' and '{previous}' cannot be used together\n"),
        });
    }

    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    if comment_lines {
        for line in input.lines() {
            if line.is_empty() {
                println!("#");
            } else {
                println!("# {line}");
            }
        }
        return Ok(());
    }

    let mut output = Vec::new();
    let mut pending_blank = false;
    for line in input.lines() {
        let trimmed = line.trim_end();
        if strip_comments && trimmed.starts_with('#') {
            continue;
        }
        if trimmed.is_empty() {
            if !output.is_empty() {
                pending_blank = true;
            }
            continue;
        }
        if pending_blank {
            output.push(String::new());
            pending_blank = false;
        }
        output.push(trimmed.to_owned());
    }
    if !output.is_empty() {
        println!("{}", output.join("\n"));
    }
    Ok(())
}

fn stripspace_conflicting_options(raw_args: &[String]) -> Option<(String, String)> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Mode {
        Strip,
        Comment,
    }

    let mut first: Option<(Mode, String)> = None;
    for arg in raw_args.iter().skip(1) {
        let mode = match arg.as_str() {
            "-s" | "--strip-comments" => Some(Mode::Strip),
            "-c" | "--comment-lines" => Some(Mode::Comment),
            _ => None,
        };
        let Some(mode) = mode else {
            continue;
        };
        if let Some((first_mode, first_arg)) = &first {
            if *first_mode != mode {
                return Some((arg.clone(), first_arg.clone()));
            }
        } else {
            first = Some((mode, arg.clone()));
        }
    }
    None
}
