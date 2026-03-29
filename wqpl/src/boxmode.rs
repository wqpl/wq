use crate::value::Value;

fn expand_as_cells(row: &Value) -> Option<Vec<String>> {
    // Only expand non-string lists with >= 2 elements
    if row.len() < 2 || row.is_string_like() {
        return None;
    }
    match row {
        Value::List(cells) => Some(cells.iter().map(ToString::to_string).collect()),
        Value::IntList(items) => Some(items.iter().map(|n| n.to_string()).collect()),
        _ => None,
    }
}

fn expand_simple_1d(v: &Value) -> Option<Vec<String>> {
    match v {
        Value::IntList(items) if items.len() >= 2 => {
            Some(items.iter().map(|n| n.to_string()).collect())
        }
        Value::List(items) if items.len() >= 2 && items.iter().all(Value::is_atom) => {
            Some(items.iter().map(ToString::to_string).collect())
        }
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
    if v.len() < 2 || v.is_string_like() {
        return v.to_string();
    }
    if let Some(cells) = expand_simple_1d(v) {
        return render_table(&[cells]);
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
    use std::sync::Arc;

    use super::*;
    use crate::value::into_wq_string;

    #[test]
    fn single_string_value_as_line() {
        let v = into_wq_string("aa");
        assert_eq!(format_boxed(&v), "\"aa\"");
    }

    #[test]
    fn string_then_code_as_two_rows() {
        let msg = into_wq_string("abc");
        let code = Value::Int(42);
        let v = Value::List(Arc::new(vec![msg, code]));
        // String is non-atom, so it renders as its own row (same as old List<Char>)
        assert_eq!(format_boxed(&v), "\"abc\"\n42");
    }

    #[test]
    fn string_and_nested_list_two_rows() {
        // When one element is a string (atom) and another is a sub-list,
        // they render as two rows.
        let msg = into_wq_string("abc");
        let code_row = Value::List(Arc::new(vec![Value::Int(42), Value::Int(99)]));
        let v = Value::List(Arc::new(vec![msg, code_row]));
        // table cell padding: "abc" width 5, so 42 gets padded to 5 chars
        assert_eq!(format_boxed(&v), "\"abc\"\n42    99");
    }

    #[test]
    fn one_elem_intlist_row_uses_display_comma() {
        let v = Value::IntList(Arc::new(vec![1]));
        assert_eq!(format_boxed(&v), ",1");
    }

    #[test]
    fn flat_intlist_renders_as_single_row() {
        let v = Value::IntList(Arc::new(vec![1, 2, 3]));
        assert_eq!(format_boxed(&v), "1 2 3");
    }

    #[test]
    fn flat_list_of_atoms_renders_as_single_row() {
        let v = Value::List(Arc::new(vec![Value::Int(1), Value::Int(2), Value::Int(3)]));
        assert_eq!(format_boxed(&v), "1 2 3");
    }

    #[test]
    fn mixed_rows_keep_display_for_singletons() {
        // First row: two cells; second row: single 1-elem list -> uses Display (",42")
        let v = Value::List(Arc::new(vec![
            Value::List(Arc::new(vec![Value::Int(1), Value::Int(2)])),
            Value::List(Arc::new(vec![Value::Int(42)])),
        ]));
        assert_eq!(format_boxed(&v), "1   2\n,42");
    }
}
