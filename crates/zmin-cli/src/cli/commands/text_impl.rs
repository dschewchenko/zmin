use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ColumnMode {
    Plain,
    Column,
    Row,
}

pub(crate) fn column(
    mode: Option<&str>,
    raw_mode: Option<u32>,
    width: Option<usize>,
    padding: Option<usize>,
) -> Result<()> {
    let mode = parse_column_mode(mode, raw_mode)?;
    let width = width.unwrap_or(80);
    let padding = padding.unwrap_or(1);
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    let items = input
        .split_terminator('\n')
        .map(|line| line.trim_end_matches('\r').to_owned())
        .collect::<Vec<_>>();
    let output = render_columns(&items, mode, width, padding);
    if !output.is_empty() {
        print!("{output}");
    }
    Ok(())
}

fn parse_column_mode(mode: Option<&str>, raw_mode: Option<u32>) -> Result<ColumnMode> {
    if let Some(raw_mode) = raw_mode {
        return Ok(if raw_mode == 0 {
            ColumnMode::Plain
        } else {
            ColumnMode::Column
        });
    }
    let Some(mode) = mode else {
        return Ok(ColumnMode::Plain);
    };
    for token in mode.split(',') {
        match token {
            "plain" => return Ok(ColumnMode::Plain),
            "column" => return Ok(ColumnMode::Column),
            "row" => return Ok(ColumnMode::Row),
            "dense" | "nodense" => {}
            "" => {}
            other => {
                return Err(CliError::Fatal {
                    code: 129,
                    message: format!("unsupported option '{other}'"),
                });
            }
        }
    }
    Ok(ColumnMode::Plain)
}

fn render_columns(items: &[String], mode: ColumnMode, width: usize, padding: usize) -> String {
    if items.is_empty() {
        return String::new();
    }
    if mode == ColumnMode::Plain {
        let mut out = items.join("\n");
        out.push('\n');
        return out;
    }
    let columns = best_column_count(items, mode, width, padding);
    let rows = items.len().div_ceil(columns);
    let column_widths = column_widths(items, mode, columns, rows);
    let mut out = String::new();
    for row in 0..rows {
        let mut last_col = 0;
        for col in 0..columns {
            if column_item(items, mode, columns, rows, row, col).is_some() {
                last_col = col;
            }
        }
        for (col, column_width) in column_widths.iter().enumerate().take(last_col + 1) {
            let Some(item) = column_item(items, mode, columns, rows, row, col) else {
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

fn best_column_count(items: &[String], mode: ColumnMode, width: usize, padding: usize) -> usize {
    for columns in (1..=items.len()).rev() {
        if columns != items.len() && !items.len().is_multiple_of(columns) {
            continue;
        }
        let rows = items.len().div_ceil(columns);
        let widths = column_widths(items, mode, columns, rows);
        let total =
            widths.iter().sum::<usize>() + padding.saturating_mul(columns.saturating_sub(1));
        if total <= width {
            return columns;
        }
    }
    1
}

fn column_widths(items: &[String], mode: ColumnMode, columns: usize, rows: usize) -> Vec<usize> {
    let mut widths = vec![0; columns];
    for row in 0..rows {
        for (col, width) in widths.iter_mut().enumerate() {
            if let Some(item) = column_item(items, mode, columns, rows, row, col) {
                *width = (*width).max(item.len());
            }
        }
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
