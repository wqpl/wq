use crate::value::Value;

fn expand_as_cells(row: &Value) -> Option<Vec<String>> {
    // Only expand non-string lists with >= 2 elements
    if row.len() < 2 || row.is_str() {
        return None;
    }
    match row {
        Value::List(cells) => Some(cells.iter().map(ToString::to_string).collect()),
        Value::IntList(items) => Some(items.iter().map(|n| n.to_string()).collect()),
        _ => None,
    }
}

fn render_table(table: &[Vec<String>]) -> String {
    let ncols = table.iter().map(|r| r.len()).max().unwrap_or(0);
    if ncols == 0 {
        return String::new();
    }
    let mut widths = vec![0usize; ncols];
    for row in table {
        for (j, cell) in row.iter().enumerate() {
            widths[j] = widths[j].max(cell.len()); // byte width; swap for unicode-width if needed
        }
    }
    let mut lines = Vec::with_capacity(table.len());
    for row in table {
        let mut parts = Vec::with_capacity(ncols);
        for (j, &w) in widths.iter().enumerate() {
            let text = row.get(j).map(String::as_str).unwrap_or("");
            parts.push(format!("{text:<w$}"));
        }
        lines.push(parts.join(" ").trim_end().to_string());
    }
    lines.join("\n")
}

pub fn format_boxed(v: &Value) -> String {
    if v.len() < 2 || v.is_str() {
        return v.to_string();
    }
    match v {
        Value::List(rows) => {
            let table: Vec<Vec<String>> = rows
                .iter()
                .map(|row| expand_as_cells(row).unwrap_or_else(|| vec![row.to_string()]))
                .collect();
            render_table(&table)
        }
        Value::IntList(items) => {
            let table: Vec<Vec<String>> = items.iter().map(|e| vec![e.to_string()]).collect();
            render_table(&table)
        }
        _ => v.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::into_wq_str;

    #[test]
    fn single_string_value_as_line() {
        let v = into_wq_str("aa");
        assert_eq!(format_boxed(&v), "\"aa\"");
    }

    #[test]
    fn string_then_code_as_two_rows() {
        let msg = into_wq_str("abc");
        let code = Value::Int(42);
        let v = Value::List(vec![msg, code]);
        assert_eq!(format_boxed(&v), "\"abc\"\n42");
    }

    #[test]
    fn one_elem_intlist_row_uses_display_comma() {
        let v = Value::IntList(vec![1]);
        assert_eq!(format_boxed(&v), ",1");
    }

    #[test]
    fn mixed_rows_keep_display_for_singletons() {
        // First row: two cells; second row: single 1-elem list -> uses Display (",42")
        let v = Value::List(vec![
            Value::List(vec![Value::Int(1), Value::Int(2)]),
            Value::List(vec![Value::Int(42)]),
        ]);
        assert_eq!(format_boxed(&v), "1   2\n,42");
    }
}
