use unicode_width::UnicodeWidthStr as _;

use crate::style::{AnsiColor, ColorMode, TextStyle, paint};
use crate::value::Value;
use crate::value::meta::ShapeMeta;
use crate::value::seq::ValueSeq;

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
    Source,
    Axis(usize),
    Index(usize),
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

    fn source(text: impl Into<String>) -> Self {
        Self::new(text, CellRole::Source)
    }
}

#[derive(Clone, Copy)]
struct RenderContext<'a> {
    options: BoxFormatOptions,
    max_width: Option<usize>,
    source_styler: Option<&'a dyn Fn(&str) -> String>,
}

impl RenderContext<'_> {
    fn plain(options: BoxFormatOptions) -> Self {
        Self {
            options,
            max_width: None,
            source_styler: None,
        }
    }
}

fn expand_as_cells(row: &Value) -> Option<Vec<String>> {
    // Only expand non-string lists with >= 2 elements
    if row.len() < 2 || row.is_string() {
        return None;
    }
    Some(
        ValueSeq::from_value(row)?
            .values()
            .map(|value| value.to_string())
            .collect(),
    )
}

fn expand_axis_cells(row: &Value) -> Option<Vec<String>> {
    if row.is_string() {
        return None;
    }
    if let Some(items) = ValueSeq::from_value(row) {
        Some(items.values().map(|value| value.to_string()).collect())
    } else {
        Some(vec![row.to_string()])
    }
}

fn expand_simple_1d(v: &Value) -> Option<Vec<String>> {
    if v.is_string() {
        return None;
    }
    let items = ValueSeq::from_value(v)?;
    if items.len() < 2 || !items.values().all(|value| value.is_atom()) {
        return None;
    }
    Some(items.values().map(|value| value.to_string()).collect())
}

fn render_table(table: &[Vec<String>], context: RenderContext<'_>) -> String {
    let cells: Vec<Vec<Cell>> = table
        .iter()
        .map(|row| row.iter().map(Cell::source).collect())
        .collect();
    render_cells(&cells, context)
}

fn render_cells(table: &[Vec<Cell>], context: RenderContext<'_>) -> String {
    let ncols = table.iter().map(|r| r.len()).max().unwrap_or(0);
    if ncols == 0 {
        return String::new();
    }
    let mut widths = vec![0usize; ncols];
    for row in table {
        for (j, cell) in row.iter().enumerate() {
            widths[j] = widths[j].max(display_width(&cell.text));
        }
    }
    let mut lines = Vec::with_capacity(table.len());
    for row in table {
        let mut parts = Vec::with_capacity(row.len());
        for (j, cell) in row.iter().enumerate() {
            let width = if j + 1 == row.len() {
                display_width(&cell.text)
            } else {
                widths[j]
            };
            let text = pad_plain(&cell.text, width, false);
            parts.push(style_cell(&text, cell.role, context));
        }
        lines.push(parts.join(" "));
    }
    lines.join("\n")
}

fn display_width(text: &str) -> usize {
    text.width()
}

fn pad_plain(text: &str, width: usize, right: bool) -> String {
    let padding = " ".repeat(width.saturating_sub(display_width(text)));
    if right {
        format!("{padding}{text}")
    } else {
        format!("{text}{padding}")
    }
}

fn style_cell(text: &str, role: CellRole, context: RenderContext<'_>) -> String {
    if text.is_empty() {
        return String::new();
    }
    match role {
        CellRole::Source => context
            .source_styler
            .map_or_else(|| text.to_string(), |styler| styler(text)),
        _ => style_text(text, role, context.options.color),
    }
}

fn style_text(text: &str, role: CellRole, color: bool) -> String {
    if !color || text.is_empty() {
        return text.to_string();
    }
    paint(text, cell_style(role), ColorMode::Always)
}

fn cell_style(role: CellRole) -> TextStyle {
    match role {
        CellRole::Source => TextStyle::new(),
        CellRole::Axis(axis) | CellRole::Index(axis) => axis_style(axis).dimmed(),
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

fn pad_cell(text: &str, width: usize, role: CellRole, context: RenderContext<'_>) -> String {
    style_cell(&pad_plain(text, width, false), role, context)
}

fn pad_cell_right(text: &str, width: usize, role: CellRole, context: RenderContext<'_>) -> String {
    style_cell(&pad_plain(text, width, true), role, context)
}

fn join_padded_cells<'a>(
    cells: impl Iterator<Item = (&'a str, usize, CellRole)>,
    context: RenderContext<'_>,
) -> String {
    let cells = cells.collect::<Vec<_>>();
    let last = cells.len().saturating_sub(1);
    cells
        .into_iter()
        .enumerate()
        .map(|(index, (text, width, role))| {
            let width = if index == last {
                display_width(text)
            } else {
                width
            };
            pad_cell(text, width, role, context)
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn axis_index_role(axis: usize) -> CellRole {
    CellRole::Index(axis)
}

fn format_matrix(
    v: &Value,
    row_axis: usize,
    dims: &[usize],
    context: RenderContext<'_>,
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

    let row_axis_label = format!("x{row_axis}");
    let col_axis_label = format!("x{col_axis}");
    let row_label_width = row_axis_label
        .width()
        .max(rows_len.saturating_sub(1).to_string().len());
    let mut col_widths = vec![1usize; cols_len];
    for (col, width) in col_widths.iter_mut().enumerate() {
        *width = (*width).max(col.to_string().len());
    }
    for row in &row_cells {
        for (col, cell) in row.iter().enumerate() {
            col_widths[col] = col_widths[col].max(display_width(cell));
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
        .chain(
            col_indices
                .iter()
                .enumerate()
                .map(|(col, text)| (text.as_str(), col_widths[col], axis_index_role(col_axis))),
        ),
        context,
    );
    lines.push(format!("{}{}", " ".repeat(row_label_width), col_header));

    let fence = join_padded_cells(
        col_widths
            .iter()
            .map(|&width| ("-", width, CellRole::Fence)),
        context,
    );
    lines.push(format!(
        "{}   {}",
        pad_cell(
            &row_axis_label,
            row_label_width,
            CellRole::Axis(row_axis),
            context
        ),
        fence
    ));

    for (row, cells) in row_cells.iter().enumerate() {
        let row_label = pad_cell_right(
            &row.to_string(),
            row_label_width,
            axis_index_role(row_axis),
            context,
        );
        let pipe = style_cell("|", CellRole::Fence, context);
        let rendered_cells = join_padded_cells(
            cells
                .iter()
                .enumerate()
                .map(|(col, text)| (text.as_str(), col_widths[col], CellRole::Source)),
            context,
        );
        lines.push(format!("{row_label} {pipe} {rendered_cells}"));
    }

    Some(lines.join("\n"))
}

fn slice_title(prefix: &[(usize, usize)], context: RenderContext<'_>) -> String {
    prefix
        .iter()
        .map(|(axis, index)| {
            style_cell(
                &format!("x{axis} = {index}"),
                CellRole::Axis(*axis),
                context,
            )
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
    context: RenderContext<'_>,
) -> Option<()> {
    if axis + 2 == dims.len() {
        let matrix = format_matrix(v, axis, dims, context)?;
        let title = slice_title(prefix, context);
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
        collect_slices(item, dims, axis + 1, prefix, sections, context)?;
        prefix.pop();
    }
    Some(())
}

fn format_uniform_axes(v: &Value, dims: &[usize], context: RenderContext<'_>) -> Option<String> {
    if dims.len() < 2 || v.is_string() {
        return None;
    }
    let mut sections = Vec::new();
    collect_slices(v, dims, 0, &mut Vec::new(), &mut sections, context)?;
    Some(sections.join("\n\n"))
}

fn format_rank_one(v: &Value, dims: &[usize], context: RenderContext<'_>) -> Option<String> {
    if dims.len() != 1 || dims[0] == 0 || v.is_string() {
        return None;
    }
    let cells = ValueSeq::from_value(v)?
        .values()
        .map(|value| value.to_string())
        .collect::<Vec<_>>();
    if cells.len() != dims[0] {
        return None;
    }

    let indices = (0..cells.len())
        .map(|index| index.to_string())
        .collect::<Vec<_>>();
    let widths = cells
        .iter()
        .zip(&indices)
        .map(|(cell, index)| display_width(cell).max(display_width(index)))
        .collect::<Vec<_>>();
    let axis_label = "x0";
    let axis_width = display_width(axis_label);
    let max_width = context.max_width.unwrap_or(usize::MAX);
    let mut sections = Vec::new();
    let mut start = 0;
    while start < cells.len() {
        let mut end = start;
        let mut section_width = axis_width + 1;
        while end < cells.len() {
            let candidate_width = section_width + usize::from(end > start) + widths[end];
            if end > start && candidate_width > max_width {
                break;
            }
            section_width = candidate_width;
            end += 1;
        }

        let header = join_padded_cells(
            std::iter::once((axis_label, axis_width, CellRole::Axis(0))).chain(
                indices[start..end]
                    .iter()
                    .enumerate()
                    .map(|(offset, index)| {
                        (index.as_str(), widths[start + offset], axis_index_role(0))
                    }),
            ),
            context,
        );
        let prefix = " ".repeat(axis_width + 1);
        let fence = join_padded_cells(
            widths[start..end]
                .iter()
                .map(|&width| ("-", width, CellRole::Fence)),
            context,
        );
        let values = join_padded_cells(
            cells[start..end]
                .iter()
                .enumerate()
                .map(|(offset, cell)| (cell.as_str(), widths[start + offset], CellRole::Source)),
            context,
        );
        sections.push(format!("{header}\n{prefix}{fence}\n{prefix}{values}"));
        start = end;
    }
    Some(sections.join("\n"))
}

fn format_ragged_rows(v: &Value, context: RenderContext<'_>) -> Option<String> {
    let Value::List(rows) = v else {
        return None;
    };
    if rows.len() < 2
        || rows.iter().any(Value::is_string)
        || !rows.iter().any(|row| row.is_list() && !row.is_string())
    {
        return None;
    }

    let axis_label = "x0";
    let index_width = display_width(axis_label).max(rows.len().saturating_sub(1).to_string().len());
    let mut lines = Vec::with_capacity(rows.len() + 1);
    lines.push(pad_cell(
        axis_label,
        index_width,
        CellRole::Axis(0),
        context,
    ));
    for (i, row) in rows.iter().enumerate() {
        let index = pad_cell_right(&i.to_string(), index_width, axis_index_role(0), context);
        let pipe = style_cell("|", CellRole::Fence, context);
        let source = style_cell(&row.to_string(), CellRole::Source, context);
        lines.push(format!("{index} {pipe} {source}"));
    }
    Some(lines.join("\n"))
}

fn format_compact_with(v: &Value, context: RenderContext<'_>) -> String {
    if v.len() < 2 || v.is_string() {
        return style_cell(&v.to_string(), CellRole::Source, context);
    }
    if let Some(cells) = expand_simple_1d(v) {
        return render_table(&[cells], context);
    }
    match v {
        Value::List(rows) => {
            let table: Vec<Vec<String>> = rows
                .iter()
                .map(|row| expand_as_cells(row).unwrap_or_else(|| vec![row.to_string()]))
                .collect();
            render_table(&table, context)
        }
        Value::IntList(items) => {
            let table: Vec<Vec<String>> = items.iter().map(|e| vec![e.to_string()]).collect();
            render_table(&table, context)
        }
        Value::FloatList(items) => {
            let table: Vec<Vec<String>> = items
                .iter()
                .copied()
                .map(Value::Float)
                .map(|e| vec![e.to_string()])
                .collect();
            render_table(&table, context)
        }
        _ => style_cell(&v.to_string(), CellRole::Source, context),
    }
}

pub fn format_compact(v: &Value) -> String {
    format_compact_with(v, RenderContext::plain(BoxFormatOptions::default()))
}

fn format_boxed_in_context(v: &Value, context: RenderContext<'_>) -> String {
    if !context.options.axes {
        return format_compact_with(v, context);
    }

    let meta = v.display_meta();
    match &meta.shape {
        ShapeMeta::Uniform(dims) if dims.len() == 1 => {
            if let Some(body) = format_rank_one(v, dims, context) {
                return body;
            }
        }
        ShapeMeta::Uniform(dims) if dims.len() >= 2 => {
            if let Some(body) = format_uniform_axes(v, dims, context) {
                return body;
            }
        }
        ShapeMeta::Ragged => {
            if let Some(body) = format_ragged_rows(v, context) {
                return body;
            }
        }
        ShapeMeta::Uniform(_) => {}
    }

    format_compact_with(v, context)
}

pub fn format_boxed_with(v: &Value, options: BoxFormatOptions) -> String {
    format_boxed_in_context(v, RenderContext::plain(options))
}

pub(crate) fn format_boxed_for_display(
    v: &Value,
    options: BoxFormatOptions,
    max_width: Option<usize>,
    source_styler: Option<&dyn Fn(&str) -> String>,
) -> String {
    format_boxed_in_context(
        v,
        RenderContext {
            options,
            max_width,
            source_styler,
        },
    )
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
        // String is non-atom, so it renders as its own row (same as the old char-list)
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
    fn one_element_rank_one_list_has_an_axis() {
        let v = Value::IntList(Arc::new(vec![1]));
        assert_eq!(format_boxed(&v), "x0 0\n   -\n   1");
    }

    #[test]
    fn rank_one_int_list_has_an_index_row() {
        let v = Value::IntList(Arc::new(vec![1, 2, 3]));
        assert_eq!(format_boxed(&v), "x0 0 1 2\n   - - -\n   1 2 3");
    }

    #[test]
    fn packed_bool_and_range_lists_expand_like_other_rows() {
        let bools = Value::BoolList(Arc::new(vec![true, false]));
        assert_eq!(format_boxed(&bools), "x0 0 1\n   - -\n   T F");

        let range = Value::IntRange(Arc::new(crate::value::seq::IntRangeData::new(1, 1, 3)));
        assert_eq!(format_boxed(&range), "x0 0 1 2\n   - - -\n   1 2 3");
    }

    #[test]
    fn rank_one_general_list_has_an_index_row() {
        let v = Value::List(Arc::new(vec![Value::Int(1), Value::Int(2), Value::Int(3)]));
        assert_eq!(format_boxed(&v), "x0 0 1 2\n   - - -\n   1 2 3");
    }

    #[test]
    fn ragged_rows_keep_each_row_reparsable() {
        let v = Value::List(Arc::new(vec![
            Value::List(Arc::new(vec![Value::Int(1), Value::Int(2)])),
            Value::List(Arc::new(vec![Value::Int(42)])),
        ]));
        assert_eq!(format_boxed(&v), "x0\n 0 | (1;2)\n 1 | ,42");
    }

    #[test]
    fn nested_ragged_rows_keep_wq_list_syntax() {
        let v = Value::List(Arc::new(vec![
            Value::Int(1),
            Value::List(Arc::new(vec![
                Value::List(Arc::new(vec![Value::Int(3), Value::Int(2)])),
                Value::Int(3),
                Value::List(Arc::new(vec![Value::Int(3), Value::Int(4)])),
            ])),
        ]));
        assert_eq!(format_boxed(&v), "x0\n 0 | 1\n 1 | ((3;2);3;(3;4))");
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
        assert_eq!(format_boxed(&v), "x0\n 0 | 1\n 1 | (2;3)\n 2 | (4;5;(3;4))");
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
            "x0\n 0 | 0\n 1 | ,1\n 2 | ,2\n 3 | ,3\n 4 | ,4\n 5 | ,5\n 6 | ,6\n 7 | ,7\n 8 | ,8\n 9 | ,9\n10 | ,10\n11 | ,11"
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
            "  x1 0 1 2\nx0   - - -\n 0 | 1 2 3\n 1 | 4 5 6"
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
            "x0 = 0\n  x2 0 1 2\nx1   - - -\n 0 | 1 2 3\n 1 | 4 5 6\n\nx0 = 1\n  x2 0  1  2\nx1   -  -  -\n 0 | 7  8  9\n 1 | 10 11 12"
        );
    }

    #[test]
    fn axis_labels_use_dim_axis_color() {
        assert_eq!(
            style_text("x1", CellRole::Axis(1), true),
            "\x1b[2;33mx1\x1b[0m"
        );
    }

    #[test]
    fn color_is_reinforcement_only() {
        let v = Value::List(Arc::new(vec![
            Value::List(Arc::new(vec![Value::Int(1), Value::Int(2)])),
            Value::List(Arc::new(vec![Value::Int(3), Value::Int(4)])),
        ]));
        let col_axis = style_text("x1", CellRole::Axis(1), true);
        let row_axis = style_text("x0", CellRole::Axis(0), true);
        let fence = style_text("|", CellRole::Fence, true);
        let first_index = style_text(" 0", axis_index_role(0), true);
        let second_index = style_text(" 1", axis_index_role(0), true);

        assert_eq!(col_axis, "\x1b[2;33mx1\x1b[0m");
        assert_eq!(row_axis, "\x1b[2;36mx0\x1b[0m");
        assert_eq!(fence, "\x1b[2m|\x1b[0m");
        assert_eq!(first_index, "\x1b[2;36m 0\x1b[0m");
        assert_eq!(second_index, "\x1b[2;36m 1\x1b[0m");
        assert_eq!(
            format_boxed_with(&v, BoxFormatOptions::default()),
            format_boxed(&v)
        );
    }

    #[test]
    fn rank_one_chunks_repeat_the_axis_and_indices() {
        let v = Value::IntList(Arc::new(vec![1234, 5, 678]));
        assert_eq!(
            format_boxed_for_display(&v, BoxFormatOptions::default(), Some(9), None),
            "x0 0    1\n   -    -\n   1234 5\nx0 2\n   -\n   678"
        );
    }

    #[test]
    fn source_cells_are_padded_before_independent_styling() {
        let v = Value::List(Arc::new(vec![
            Value::List(Arc::new(vec![Value::Int(1), Value::Int(22)])),
            Value::List(Arc::new(vec![Value::Int(333), Value::Int(4)])),
        ]));
        let styler = |source: &str| format!("<{source}>");
        assert_eq!(
            format_boxed_for_display(&v, BoxFormatOptions::default(), None, Some(&styler)),
            "  x1 0   1\nx0   -   -\n 0 | <1  > <22>\n 1 | <333> <4>"
        );
    }
}
