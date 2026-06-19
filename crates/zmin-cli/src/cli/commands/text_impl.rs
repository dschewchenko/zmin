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
    mode: Option<&str>,
    raw_mode: Option<u32>,
    width: Option<usize>,
    padding: Option<usize>,
) -> Result<()> {
    let spec = parse_column_mode(mode, raw_mode)?;
    let width = width.unwrap_or(80);
    let padding = padding.unwrap_or(1);
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    let items = input
        .split_terminator('\n')
        .map(|line| line.trim_end_matches('\r').to_owned())
        .collect::<Vec<_>>();
    let output = render_columns(&items, spec, width, padding);
    if !output.is_empty() {
        print!("{output}");
    }
    Ok(())
}

fn parse_column_mode(mode: Option<&str>, raw_mode: Option<u32>) -> Result<ColumnSpec> {
    if let Some(raw_mode) = raw_mode {
        return Ok(if raw_mode == 0 {
            ColumnSpec {
                mode: ColumnMode::Plain,
                dense: false,
            }
        } else {
            ColumnSpec {
                mode: ColumnMode::Column,
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
            "" => {}
            other => {
                return Err(CliError::Fatal {
                    code: 129,
                    message: format!("unsupported option '{other}'"),
                });
            }
        }
    }
    Ok(spec)
}

fn render_columns(items: &[String], spec: ColumnSpec, width: usize, padding: usize) -> String {
    if items.is_empty() {
        return String::new();
    }
    if spec.mode == ColumnMode::Plain {
        let mut out = items.join("\n");
        out.push('\n');
        return out;
    }
    let columns = best_column_count(items, spec, width, padding);
    let rows = items.len().div_ceil(columns);
    let column_widths = column_widths(items, spec.mode, columns, rows, spec.dense);
    let mut out = String::new();
    for row in 0..rows {
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
        out.push('\n');
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

pub(crate) fn stripspace(strip_comments: bool, comment_lines: bool) -> Result<()> {
    if strip_comments && comment_lines {
        return Err(CliError::Fatal {
            code: 129,
            message: "options '-c' and '-s' cannot be used together".into(),
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
