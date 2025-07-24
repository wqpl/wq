use super::valuei::Value;
use std::sync::atomic::{AtomicBool, Ordering};

static BOXED: AtomicBool = AtomicBool::new(false);

pub fn set_boxed(on: bool) {
    BOXED.store(on, Ordering::Release);
}
pub fn is_boxed() -> bool {
    BOXED.load(Ordering::Acquire)
}

fn repr(v: &Value) -> String {
    match v {
        Value::List(items) => {
            let inner: Vec<String> = items.iter().map(repr).collect();
            format!("({})", inner.join(" "))
        }
        other => other.to_string(),
    }
}

pub fn format_boxed(rows: &[Value]) -> Option<String> {
    if rows.is_empty() {
        return None;
    }

    let all_rows_are_strings = rows.iter().all(|row| match row {
        Value::List(chars) => chars.iter().all(|c| matches!(c, Value::Char(_))),
        Value::Char(_) => true,
        _ => false,
    });

    if all_rows_are_strings {
        let lines = rows
            .iter()
            .map(|row| match row {
                Value::Char(c) => format!("\"{c}\""),
                other => other.to_string(),
            })
            .collect::<Vec<_>>()
            .join("\n");
        return Some(lines);
    }

    let has_nested = rows.iter().any(|row| {
        if let Value::List(cells) = row {
            cells.iter().any(|c| matches!(c, Value::List(_)))
        } else {
            false
        }
    });

    if has_nested {
        let lines = rows.iter().map(|row| {
            if let Value::List(cells) = row {
                cells.iter().map(repr).collect::<Vec<_>>().join(" ")
            } else {
                repr(row)
            }
        });
        return Some(lines.collect::<Vec<_>>().join("\n"));
    }

    //actual alignment
    let table: Vec<Vec<String>> = rows
        .iter()
        .map(|row| {
            if let Value::List(cells) = row {
                cells.iter().map(repr).collect()
            } else {
                vec![repr(row)]
            }
        })
        .collect();

    let ncols = table.iter().map(Vec::len).max().unwrap_or(0);
    if ncols == 0 {
        return None;
    }

    let mut widths = vec![0; ncols];
    for row in &table {
        for (j, cell) in row.iter().enumerate() {
            widths[j] = widths[j].max(cell.len());
        }
    }

    let mut lines = Vec::with_capacity(table.len());
    for row in &table {
        let mut parts = Vec::with_capacity(ncols);
        for (j, &w) in widths.iter().enumerate() {
            let text = row.get(j).map(String::as_str).unwrap_or("");
            parts.push(format!("{text:<w$}"));
        }
        lines.push(parts.join(" ").trim_end().to_string());
    }

    Some(lines.join("\n"))
}
