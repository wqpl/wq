use crate::style::{AnsiColor, ColorMode, TextStyle, paint};
use crate::value::Value;
use crate::value::meta::ShapeMeta;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoxFormatOptions {
    pub axes: bool,
    pub color: bool,
}

impl Default for BoxFormatOptions {
    fn default() -> Self {
        Self {
            axes: true,
            color: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CellRole {
    Plain,
    Axis(usize),
    Index { axis: usize, alternate: bool },
    Fence,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Cell {
    text: String,
    role: CellRole,
}

impl Cell {
    fn new(text: impl Into<String>, role: CellRole) -> Self {
        Self {
            text: text.into(),
            role,
        }
    }

    fn plain(text: impl Into<String>) -> Self {
        Self::new(text, CellRole::Plain)
    }
}

fn expand_as_cells(row: &Value) -> Option<Vec<String>> {
    // Only expand non-string lists with >= 2 elements
    if row.len() < 2 || row.is_string_like() {
        return None;
    }
    match row {
        Value::List(cells) => Some(cells.iter().map(ToString::to_string).collect()),
        Value::IntList(items) => Some(items.iter().map(|n| n.to_string()).collect()),
        Value::FloatList(items) => Some(
            items
                .iter()
                .copied()
                .map(Value::Float)
                .map(|v| v.to_string())
                .collect(),
        ),
        _ => None,
    }
}

fn expand_axis_cells(row: &Value) -> Option<Vec<String>> {
    if row.is_string_like() {
        return None;
    }
    match row {
        Value::List(cells) => Some(cells.iter().map(ToString::to_string).collect()),
        Value::IntList(items) => Some(items.iter().map(|n| n.to_string()).collect()),
        Value::FloatList(items) => Some(
            items
                .iter()
                .copied()
                .map(Value::Float)
                .map(|v| v.to_string())
                .collect(),
        ),
        _ => Some(vec![row.to_string()]),
    }
}

fn expand_simple_1d(v: &Value) -> Option<Vec<String>> {
    match v {
        Value::IntList(items) if items.len() >= 2 => {
            Some(items.iter().map(|n| n.to_string()).collect())
        }
        Value::FloatList(items) if items.len() >= 2 => Some(
            items
                .iter()
                .copied()
                .map(Value::Float)
                .map(|v| v.to_string())
                .collect(),
        ),
        Value::List(items) if items.len() >= 2 && items.iter().all(Value::is_atom) => {
            Some(items.iter().map(ToString::to_string).collect())
        }
        _ => None,
    }
}

fn render_table(table: &[Vec<String>]) -> String {
    let cells: Vec<Vec<Cell>> = table
        .iter()
        .map(|row| row.iter().map(Cell::plain).collect())
        .collect();
    render_cells(&cells, false)
}

fn render_cells(table: &[Vec<Cell>], color: bool) -> String {
    let ncols = table.iter().map(|r| r.len()).max().unwrap_or(0);
    if ncols == 0 {
        return String::new();
    }
    let mut widths = vec![0usize; ncols];
    for row in table {
        for (j, cell) in row.iter().enumerate() {
            widths[j] = widths[j].max(cell.text.len()); // byte width; swap for unicode-width if needed
        }
    }
    let mut lines = Vec::with_capacity(table.len());
    for row in table {
        let mut parts = Vec::with_capacity(ncols);
        for (j, &w) in widths.iter().enumerate() {
            let cell = row
                .get(j)
                .cloned()
                .unwrap_or_else(|| Cell::plain(String::new()));
            let text = format!("{:<w$}", cell.text);
            parts.push(style_text(&text, cell.role, color));
        }
        lines.push(parts.join(" ").trim_end().to_string());
    }
    lines.join("\n")
}

fn style_text(text: &str, role: CellRole, color: bool) -> String {
    if !color || text.is_empty() {
        return text.to_string();
    }
    paint(text, cell_style(role), ColorMode::Always)
}

fn cell_style(role: CellRole) -> TextStyle {
    match role {
        CellRole::Plain => TextStyle::new(),
        CellRole::Axis(axis) => axis_style(axis).bold(),
        CellRole::Index { axis, alternate } if alternate => axis_style(axis).dimmed(),
        CellRole::Index { axis, .. } => axis_style(axis),
        CellRole::Fence => TextStyle::new().dimmed(),
    }
}

fn axis_style(axis: usize) -> TextStyle {
    TextStyle::new().fg(axis_color(axis))
}

fn axis_color(axis: usize) -> AnsiColor {
    match axis % 6 {
        0 => AnsiColor::Cyan,
        1 => AnsiColor::Yellow,
        2 => AnsiColor::Magenta,
        3 => AnsiColor::Green,
        4 => AnsiColor::Blue,
        _ => AnsiColor::Red,
    }
}

fn pad_cell(text: &str, width: usize, role: CellRole, color: bool) -> String {
    style_text(&format!("{text:<width$}"), role, color)
}

fn pad_cell_right(text: &str, width: usize, role: CellRole, color: bool) -> String {
    style_text(&format!("{text:>width$}"), role, color)
}

fn join_padded_cells<'a>(
    cells: impl Iterator<Item = (&'a str, usize, CellRole)>,
    color: bool,
) -> String {
    cells
        .map(|(text, width, role)| pad_cell(text, width, role, color))
        .collect::<Vec<_>>()
        .join(" ")
        .trim_end()
        .to_string()
}

fn join_plain_cells<'a>(cells: impl Iterator<Item = (&'a str, usize)>) -> String {
    cells
        .map(|(text, width)| format!("{text:<width$}"))
        .collect::<Vec<_>>()
        .join(" ")
        .trim_end()
        .to_string()
}

fn axis_index_role(axis: usize, index: usize) -> CellRole {
    CellRole::Index {
        axis,
        alternate: index % 2 == 1,
    }
}

fn format_ragged_value(v: &Value) -> String {
    let Value::List(items) = v else {
        return format_compact(v);
    };
    items
        .iter()
        .map(|item| {
            if item.len() >= 2
                && !item.is_string_like()
                && matches!(
                    item,
                    Value::List(_) | Value::IntList(_) | Value::FloatList(_)
                )
            {
                format!("({})", format_compact(item))
            } else {
                item.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn format_matrix(
    v: &Value,
    row_axis: usize,
    dims: &[usize],
    options: BoxFormatOptions,
) -> Option<String> {
    let col_axis = row_axis + 1;
    let rows_len = dims[row_axis];
    let cols_len = dims[col_axis];
    let Value::List(rows) = v else {
        return None;
    };
    if rows.len() != rows_len {
        return None;
    }

    let mut row_cells = Vec::with_capacity(rows_len);
    for row in rows.iter() {
        let cells = expand_axis_cells(row)?;
        if cells.len() != cols_len {
            return None;
        }
        row_cells.push(cells);
    }

    let row_axis_label = format!("a{row_axis}");
    let col_axis_label = format!("a{col_axis}");
    let row_label_width = row_axis_label
        .len()
        .max(rows_len.saturating_sub(1).to_string().len());
    let mut col_widths = vec![1usize; cols_len];
    for (col, width) in col_widths.iter_mut().enumerate() {
        *width = (*width).max(col.to_string().len());
    }
    for row in &row_cells {
        for (col, cell) in row.iter().enumerate() {
            col_widths[col] = col_widths[col].max(cell.len());
        }
    }

    let mut lines = Vec::with_capacity(rows_len + 2);
    let col_indices: Vec<String> = (0..cols_len).map(|col| col.to_string()).collect();
    let col_header = join_padded_cells(
        std::iter::once((
            col_axis_label.as_str(),
            col_axis_label.len(),
            CellRole::Axis(col_axis),
        ))
        .chain(col_indices.iter().enumerate().map(|(col, text)| {
            (
                text.as_str(),
                col_widths[col],
                axis_index_role(col_axis, col),
            )
        })),
        options.color,
    );
    lines.push(format!("{}{}", " ".repeat(row_label_width), col_header));

    let fence = join_padded_cells(
        col_widths
            .iter()
            .map(|&width| ("-", width, CellRole::Fence)),
        options.color,
    );
    lines.push(format!(
        "{}   {}",
        pad_cell(
            &row_axis_label,
            row_label_width,
            CellRole::Axis(row_axis),
            options.color
        ),
        fence
    ));

    for (row, cells) in row_cells.iter().enumerate() {
        let row_label = pad_cell_right(
            &row.to_string(),
            row_label_width,
            axis_index_role(row_axis, row),
            options.color,
        );
        let pipe = style_text("|", CellRole::Fence, options.color);
        let rendered_cells = join_plain_cells(
            cells
                .iter()
                .enumerate()
                .map(|(col, text)| (text.as_str(), col_widths[col])),
        );
        lines.push(format!("{row_label} {pipe} {rendered_cells}"));
    }

    Some(lines.join("\n"))
}

fn slice_title(prefix: &[(usize, usize)], color: bool) -> String {
    prefix
        .iter()
        .map(|(axis, index)| {
            style_text(&format!("a{axis} = {index}"), CellRole::Axis(*axis), color)
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn collect_slices(
    v: &Value,
    dims: &[usize],
    axis: usize,
    prefix: &mut Vec<(usize, usize)>,
    sections: &mut Vec<String>,
    options: BoxFormatOptions,
) -> Option<()> {
    if axis + 2 == dims.len() {
        let matrix = format_matrix(v, axis, dims, options)?;
        let title = slice_title(prefix, options.color);
        if title.is_empty() {
            sections.push(matrix);
        } else {
            sections.push(format!("{title}\n{matrix}"));
        }
        return Some(());
    }

    let Value::List(items) = v else {
        return None;
    };
    if items.len() != dims[axis] {
        return None;
    }
    for (i, item) in items.iter().enumerate() {
        prefix.push((axis, i));
        collect_slices(item, dims, axis + 1, prefix, sections, options)?;
        prefix.pop();
    }
    Some(())
}

fn format_uniform_axes(v: &Value, dims: &[usize], options: BoxFormatOptions) -> Option<String> {
    if dims.len() < 2 || v.is_string_like() {
        return None;
    }
    let mut sections = Vec::new();
    collect_slices(v, dims, 0, &mut Vec::new(), &mut sections, options)?;
    Some(sections.join("\n\n"))
}

fn format_ragged_rows(v: &Value, options: BoxFormatOptions) -> Option<String> {
    let Value::List(rows) = v else {
        return None;
    };
    if rows.len() < 2
        || rows.iter().any(Value::is_string_like)
        || !rows.iter().any(|row| {
            matches!(
                row,
                Value::List(_) | Value::IntList(_) | Value::FloatList(_)
            )
        })
    {
        return None;
    }

    let index_width = rows.len().saturating_sub(1).to_string().len();
    let mut lines = Vec::with_capacity(rows.len());
    for (i, row) in rows.iter().enumerate() {
        let index = pad_cell_right(
            &i.to_string(),
            index_width,
            axis_index_role(0, i),
            options.color,
        );
        let pipe = style_text("|", CellRole::Fence, options.color);
        lines.push(format!("{index} {pipe} {}", format_ragged_value(row)));
    }
    Some(lines.join("\n"))
}

pub fn format_compact(v: &Value) -> String {
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
        Value::FloatList(items) => {
            let table: Vec<Vec<String>> = items
                .iter()
                .copied()
                .map(Value::Float)
                .map(|e| vec![e.to_string()])
                .collect();
            render_table(&table)
        }
        _ => v.to_string(),
    }
}

pub fn format_boxed_with(v: &Value, options: BoxFormatOptions) -> String {
    if !options.axes {
        return format_compact(v);
    }

    let meta = v.display_meta();
    match &meta.shape {
        ShapeMeta::Uniform(dims) if dims.len() >= 2 => {
            if let Some(body) = format_uniform_axes(v, dims, options) {
                return body;
            }
        }
        ShapeMeta::Ragged => {
            if let Some(body) = format_ragged_rows(v, options) {
                return body;
            }
        }
        ShapeMeta::Uniform(_) => {}
    }

    format_compact(v)
}

pub fn format_boxed(v: &Value) -> String {
    format_boxed_with(v, BoxFormatOptions::default())
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
        // First row: two cells; second row: single 1-elem list -> inline value
        let v = Value::List(Arc::new(vec![
            Value::List(Arc::new(vec![Value::Int(1), Value::Int(2)])),
            Value::List(Arc::new(vec![Value::Int(42)])),
        ]));
        assert_eq!(format_boxed(&v), "0 | 1 2\n1 | 42");
    }

    #[test]
    fn ragged_rows_are_terse() {
        let v = Value::List(Arc::new(vec![
            Value::Int(1),
            Value::List(Arc::new(vec![
                Value::List(Arc::new(vec![Value::Int(3), Value::Int(2)])),
                Value::Int(3),
                Value::List(Arc::new(vec![Value::Int(3), Value::Int(4)])),
            ])),
        ]));
        assert_eq!(format_boxed(&v), "0 | 1\n1 | (3 2) 3 (3 4)");
    }

    #[test]
    fn ragged_rows_align_value_column() {
        let v = Value::List(Arc::new(vec![
            Value::Int(1),
            Value::List(Arc::new(vec![Value::Int(2), Value::Int(3)])),
            Value::List(Arc::new(vec![
                Value::Int(4),
                Value::Int(5),
                Value::List(Arc::new(vec![Value::Int(3), Value::Int(4)])),
            ])),
        ]));
        assert_eq!(format_boxed(&v), "0 | 1\n1 | 2 3\n2 | 4 5 (3 4)");
    }

    #[test]
    fn ragged_rows_right_align_index_column() {
        let rows = (0..12)
            .map(|i| {
                if i == 0 {
                    Value::Int(i)
                } else {
                    Value::List(Arc::new(vec![Value::Int(i)]))
                }
            })
            .collect();
        let v = Value::List(Arc::new(rows));
        assert_eq!(
            format_boxed(&v),
            " 0 | 0\n 1 | 1\n 2 | 2\n 3 | 3\n 4 | 4\n 5 | 5\n 6 | 6\n 7 | 7\n 8 | 8\n 9 | 9\n10 | 10\n11 | 11"
        );
    }

    #[test]
    fn matrix_shows_shape_and_axes() {
        let v = Value::List(Arc::new(vec![
            Value::List(Arc::new(vec![Value::Int(1), Value::Int(2), Value::Int(3)])),
            Value::List(Arc::new(vec![Value::Int(4), Value::Int(5), Value::Int(6)])),
        ]));
        assert_eq!(
            format_boxed(&v),
            "  a1 0 1 2\na0   - - -\n 0 | 1 2 3\n 1 | 4 5 6"
        );
    }

    #[test]
    fn three_dimensional_array_shows_slices() {
        let v = Value::List(Arc::new(vec![
            Value::List(Arc::new(vec![
                Value::List(Arc::new(vec![Value::Int(1), Value::Int(2), Value::Int(3)])),
                Value::List(Arc::new(vec![Value::Int(4), Value::Int(5), Value::Int(6)])),
            ])),
            Value::List(Arc::new(vec![
                Value::List(Arc::new(vec![Value::Int(7), Value::Int(8), Value::Int(9)])),
                Value::List(Arc::new(vec![
                    Value::Int(10),
                    Value::Int(11),
                    Value::Int(12),
                ])),
            ])),
        ]));
        assert_eq!(
            format_boxed(&v),
            "a0 = 0\n  a2 0 1 2\na1   - - -\n 0 | 1 2 3\n 1 | 4 5 6\n\na0 = 1\n  a2 0  1  2\na1   -  -  -\n 0 | 7  8  9\n 1 | 10 11 12"
        );
    }

    #[test]
    fn axis_labels_use_bold_axis_color() {
        assert_eq!(
            style_text("a1", CellRole::Axis(1), true),
            "\x1b[1;33ma1\x1b[0m"
        );
    }

    #[test]
    fn color_is_reinforcement_only() {
        let v = Value::List(Arc::new(vec![
            Value::List(Arc::new(vec![Value::Int(1), Value::Int(2)])),
            Value::List(Arc::new(vec![Value::Int(3), Value::Int(4)])),
        ]));
        let col_axis = style_text("a1", CellRole::Axis(1), true);
        let row_axis = style_text("a0", CellRole::Axis(0), true);
        let fence = style_text("|", CellRole::Fence, true);
        let alternate = style_text(" 1", axis_index_role(0, 1), true);

        assert_eq!(col_axis, "\x1b[1;33ma1\x1b[0m");
        assert_eq!(row_axis, "\x1b[1;36ma0\x1b[0m");
        assert_eq!(fence, "\x1b[2m|\x1b[0m");
        assert_eq!(alternate, "\x1b[2;36m 1\x1b[0m");
        assert_eq!(
            format_boxed_with(&v, BoxFormatOptions::default()),
            format_boxed(&v)
        );
    }
}
