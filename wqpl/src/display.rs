use std::fmt::Write as _;

use crate::boxmode::{BoxFormatOptions, format_boxed_for_display};
use crate::style::{ColorMode, TextStyle, paint};
use crate::value::Value;
use crate::value::display::format_table_value_for_display;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoxPrintConfig {
    pub boxed: bool,
    pub xray: bool,
    pub color: bool,
    pub axis: bool,
}

#[derive(Clone, Copy, Default)]
pub struct ResultFormatOptions<'a> {
    pub color: bool,
    pub max_width: Option<usize>,
    pub source_styler: Option<&'a dyn Fn(&str) -> String>,
}

impl Default for BoxPrintConfig {
    fn default() -> Self {
        Self {
            boxed: true,
            xray: false,
            color: true,
            axis: true,
        }
    }
}

impl BoxPrintConfig {
    pub const fn off() -> Self {
        Self {
            boxed: false,
            xray: false,
            color: false,
            axis: false,
        }
    }

    pub fn summary(&self) -> String {
        format!("[{}]", self.display_names().join(","))
    }

    pub fn spec(&self) -> String {
        self.display_names().join(",")
    }

    pub fn display_names(&self) -> Vec<&'static str> {
        let mut parts = Vec::new();
        if self.boxed {
            parts.push("box");
        }
        if self.xray {
            parts.push("xray");
        }
        if self.axis {
            parts.push("axis");
        }
        if self.color {
            parts.push("color");
        }
        parts
    }

    pub fn shows_xray(&self) -> bool {
        self.xray
    }

    pub fn toggle_box(&mut self) {
        if self.boxed || self.xray || self.axis || self.color {
            *self = Self::off();
        } else {
            *self = Self::default();
        }
    }

    pub fn toggle_xray(&mut self) {
        self.xray = !self.xray;
    }
}

pub fn apply_box_spec(config: &mut BoxPrintConfig, spec: &str) -> Result<(), String> {
    let mut rewrite = false;
    for raw_part in spec.split(',') {
        let part = raw_part.trim();
        if part.is_empty() {
            continue;
        }

        match part {
            "on" => {
                *config = BoxPrintConfig::default();
                rewrite = true;
                continue;
            }
            "off" => {
                *config = BoxPrintConfig::off();
                rewrite = true;
                continue;
            }
            _ => {}
        }

        let (enabled, feature) = if let Some(feature) = part.strip_prefix('+') {
            (true, feature)
        } else if let Some(feature) = part.strip_prefix('-') {
            (false, feature)
        } else {
            if !rewrite {
                *config = BoxPrintConfig::off();
                rewrite = true;
            }
            (true, part)
        };

        match feature {
            "box" => config.boxed = enabled,
            "xray" => config.xray = enabled,
            "axis" => config.axis = enabled,
            "color" => config.color = enabled,
            _ => {
                return Err(format!(
                    "unknown box mode '{part}'\nAvailable: on, off, box, axis, xray, color; prefix with + or - to modify"
                ));
            }
        }
    }
    Ok(())
}

pub fn format_print_result(result: &Value, config: &BoxPrintConfig, color: bool) -> String {
    format_print_result_with(
        result,
        config,
        ResultFormatOptions {
            color,
            ..ResultFormatOptions::default()
        },
    )
}

pub fn format_print_result_with(
    result: &Value,
    config: &BoxPrintConfig,
    options: ResultFormatOptions<'_>,
) -> String {
    let source_styler = options.source_styler;
    if config.boxed {
        format_table_value_for_display(result, source_styler).unwrap_or_else(|| {
            format_boxed_for_display(
                result,
                BoxFormatOptions {
                    axes: config.axis,
                    color: options.color,
                },
                options.max_width,
                source_styler,
            )
        })
    } else {
        let source = result.to_string();
        match source_styler {
            Some(styler) => styler(&source),
            None => source,
        }
    }
}

pub fn format_xray_info(v: &Value, color: bool) -> String {
    let pairs = [
        ("category", v.category().to_string()),
        ("kind", v.debug_kind().to_string()),
        (
            "strong",
            v.strong_count()
                .map_or_else(|| "N/A".to_string(), |v| v.to_string()),
        ),
        ("len", v.len().to_string()),
        ("depth", v.depth().to_string()),
        ("shape", v.shape().to_string()),
        ("axes", v.axes().to_string()),
        ("uniform?", Value::Bool(v.is_uniform()).to_string()),
    ];

    let mut out = String::new();
    let _ = write!(out, "[xray]\n{}", two_col_item_values(&pairs, 4, color));

    if let Value::Float(f) = v {
        let _ = write!(
            out,
            "\n{} {:#018x}\n{} {f:.17}",
            style_label("bits", color),
            f.to_bits(),
            style_label(".17 ", color)
        );
    }

    out
}

fn style_label(text: &str, color: bool) -> String {
    if color {
        paint(text, TextStyle::new().dimmed(), ColorMode::Always)
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
    let mut left_cells = Vec::new();
    for (i, (k, v)) in pairs.iter().enumerate() {
        if i % 2 == 0 {
            let label = format!("{k} ");
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
        let left_label = format!("{} ", pairs[li].0);
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
            let right_label = format!("{} ", pairs[ri].0);
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use indexmap::IndexMap;

    use super::*;

    #[test]
    fn box_spec_updates_display_config() {
        let mut config = BoxPrintConfig::default();

        apply_box_spec(&mut config, "+xray,-color").expect("apply box spec");
        assert_eq!(config.summary(), "[box,xray,axis]");

        apply_box_spec(&mut config, "box,color").expect("rewrite box spec");
        assert_eq!(config.summary(), "[box,color]");

        apply_box_spec(&mut config, "-box").expect("remove box");
        assert_eq!(config.summary(), "[color]");

        apply_box_spec(&mut config, "off").expect("disable all box config");
        assert_eq!(config.summary(), "[]");

        apply_box_spec(&mut config, "on,-color").expect("enable default config without color");
        assert_eq!(config.summary(), "[box,axis]");

        config.toggle_box();
        assert_eq!(config.summary(), "[]");
        config.toggle_box();
        assert_eq!(config.summary(), "[box,axis,color]");
    }

    #[test]
    fn unboxed_result_is_one_independently_styled_source_cell() {
        let config = BoxPrintConfig::off();
        let styler = |source: &str| format!("<{source}>");

        assert_eq!(
            format_print_result_with(
                &Value::Int(42),
                &config,
                ResultFormatOptions {
                    source_styler: Some(&styler),
                    ..ResultFormatOptions::default()
                }
            ),
            "<42>"
        );
    }

    #[test]
    fn table_cells_are_padded_before_independent_styling() {
        let value = Value::Dict(Arc::new(IndexMap::from([
            ("a".into(), Value::IntList(Arc::new(vec![1, 22]))),
            ("b".into(), Value::IntList(Arc::new(vec![3, 4]))),
        ])));
        let styler = |source: &str| format!("<{source}>");

        assert_eq!(
            format_print_result_with(
                &value,
                &BoxPrintConfig::default(),
                ResultFormatOptions {
                    source_styler: Some(&styler),
                    ..ResultFormatOptions::default()
                }
            ),
            "a  b\n< 1> <3>\n<22> <4>"
        );
    }
}
