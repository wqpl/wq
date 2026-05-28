use std::fmt::Write as _;
use std::io::IsTerminal as _;

use colored::Colorize as _;
use wqpl::boxmode::{BoxFormatOptions, format_boxed_with};
use wqpl::value::Value;
use wqpl::value::display::format_table_value;

use crate::arg::BoxPrintConfig;

fn box_format_options(config: &BoxPrintConfig) -> BoxFormatOptions {
    BoxFormatOptions {
        axes: config.axis,
        color: color_enabled(config),
    }
}

fn color_enabled(config: &BoxPrintConfig) -> bool {
    config.color && std::io::stdout().is_terminal()
}

pub(crate) fn format_print_result(result: &Value, config: &BoxPrintConfig) -> String {
    if result.is_cas() {
        format!("{result}")
    } else {
        format_non_cas_result(result, config)
    }
}

pub(crate) fn format_non_cas_result(result: &Value, config: &BoxPrintConfig) -> String {
    if config.boxed {
        format_table_value(result)
            .unwrap_or_else(|| format_boxed_with(result, box_format_options(config)))
    } else {
        format!("{result}")
    }
}

pub(crate) fn format_xray_info(v: &Value, config: &BoxPrintConfig) -> String {
    let pairs = [
        (
            "strong",
            v.strong_count()
                .map_or_else(|| "N/A".to_string(), |v| v.to_string()),
        ),
        ("len", format!("{}", v.len())),
        ("depth", format!("{}", v.depth())),
        ("shape", format!("{}", v.shape())),
        ("axes", format!("{}", v.axes())),
        ("uniform?", format!("{}", Value::Bool(v.is_uniform()))),
    ];
    let color = color_enabled(config);
    let xray_label = style_label("[xray]", color);
    format!(
        "{} {}\n{}",
        xray_label,
        v.type_name(),
        two_col_item_values(&pairs, 4, color)
    )
}

fn style_label(text: &str, color: bool) -> String {
    if color {
        text.dimmed().to_string()
    } else {
        text.to_string()
    }
}

fn two_col_item_values(pairs: &[(&str, String)], gutter: usize, color: bool) -> String {
    if pairs.is_empty() {
        return String::new();
    }
    let left_key_w = pairs
        .iter()
        .enumerate()
        .filter(|(i, _)| i % 2 == 0)
        .map(|(_, (k, _))| k.len())
        .max()
        .unwrap_or(0);
    let right_key_w = pairs
        .iter()
        .enumerate()
        .filter(|(i, _)| i % 2 == 1)
        .map(|(_, (k, _))| k.len())
        .max()
        .unwrap_or(0);
    let left_label_w = left_key_w + 2;
    let right_label_w = right_key_w + 2;
    let mut left_cells: Vec<String> = Vec::new();
    for (i, (k, v)) in pairs.iter().enumerate() {
        if i % 2 == 0 {
            let label = format!("{k}: ");
            let label = format!("{label:<left_label_w$}");
            left_cells.push(format!("{label}{v}"));
        }
    }
    let left_col_w = left_cells.iter().map(|s| s.len()).max().unwrap_or(0);
    let mut out = String::new();
    let rows = pairs.len().div_ceil(2);
    for r in 0..rows {
        let li = 2 * r;
        let ri = li + 1;
        let left_label = format!("{}: ", pairs[li].0);
        let left_label = format!("{left_label:<left_label_w$}");
        let left_cell_raw = format!("{}{}", left_label, pairs[li].1);
        let left_pad_len = left_col_w.saturating_sub(left_cell_raw.len());
        let left_cell = format!(
            "{}{}{}",
            style_label(&left_label, color),
            pairs[li].1,
            " ".repeat(left_pad_len)
        );
        if ri < pairs.len() {
            let right_label = format!("{}: ", pairs[ri].0);
            let right_label = format!("{right_label:<right_label_w$}");
            let _ = writeln!(
                out,
                "{}{}{}{}",
                left_cell,
                " ".repeat(gutter),
                style_label(&right_label, color),
                pairs[ri].1
            );
        } else {
            let _ = writeln!(out, "{}", left_cell.trim_end());
        }
    }
    out
}
