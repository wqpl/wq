use unicode_width::UnicodeWidthStr as _;

const COLUMN_GAP: &str = "  ";

pub(in crate::wqdb) fn enabled_marker(enabled: bool) -> &'static str {
    if enabled { "y" } else { "n" }
}

pub(in crate::wqdb) fn render_table(
    headers: &[&str],
    rows: &[Vec<String>],
    minimum_widths: &[usize],
) -> String {
    debug_assert!(!headers.is_empty());
    debug_assert!(rows.iter().all(|row| row.len() == headers.len()));
    debug_assert!(minimum_widths.len() <= headers.len());

    let widths = headers
        .iter()
        .enumerate()
        .map(|(column, header)| {
            rows.iter()
                .map(|row| row[column].width())
                .chain([
                    header.width(),
                    minimum_widths.get(column).copied().unwrap_or(0),
                ])
                .max()
                .unwrap_or(0)
        })
        .collect::<Vec<_>>();

    let mut output = render_row(
        &headers.iter().map(ToString::to_string).collect::<Vec<_>>(),
        &widths,
    );
    output.push('\n');
    output.push_str(
        &widths
            .iter()
            .map(|width| "-".repeat(*width))
            .collect::<Vec<_>>()
            .join(COLUMN_GAP),
    );
    for row in rows {
        output.push('\n');
        output.push_str(&render_row(row, &widths));
    }
    output
}

fn render_row(cells: &[String], widths: &[usize]) -> String {
    let mut output = String::new();
    for (column, cell) in cells.iter().enumerate() {
        if column > 0 {
            output.push_str(COLUMN_GAP);
        }
        output.push_str(cell);
        if column + 1 < cells.len() {
            output.push_str(&" ".repeat(widths[column].saturating_sub(cell.width())));
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_align_using_terminal_columns() {
        let rows = vec![
            vec!["alpha".to_string(), "界".to_string()],
            vec!["β".to_string(), "12".to_string()],
        ];

        assert_eq!(
            render_table(&["name", "value"], &rows, &[]),
            "name   value\n-----  -----\nalpha  界\nβ      12"
        );
    }

    #[test]
    fn enabled_markers_use_one_vocabulary() {
        assert_eq!(enabled_marker(true), "y");
        assert_eq!(enabled_marker(false), "n");
    }
}
