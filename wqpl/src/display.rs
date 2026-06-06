use std::fmt::Write as _;

use colored::Colorize as _;

use crate::boxmode::{BoxFormatOptions, format_boxed_with};
use crate::value::Value;
use crate::value::display::format_table_value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoxPrintConfig {
    pub boxed: bool,
    pub xray: bool,
    pub color: bool,
    pub axis: bool,
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
                    "unknown box mode '{part}'\nAvailable: box, axis, xray, color; prefix with + or - to modify"
                ));
            }
        }
    }
    Ok(())
}

pub fn format_print_result(result: &Value, config: &BoxPrintConfig, color: bool) -> String {
    if result.is_cas() {
        format!("{result}")
    } else {
        format_non_cas_result(result, config, color)
    }
}

pub fn format_non_cas_result(result: &Value, config: &BoxPrintConfig, color: bool) -> String {
    if config.boxed {
        format_table_value(result).unwrap_or_else(|| {
            format_boxed_with(
                result,
                BoxFormatOptions {
                    axes: config.axis,
                    color,
                },
            )
        })
    } else {
        format!("{result}")
    }
}

pub fn format_xray_info(v: &Value, color: bool) -> String {
    let pairs = [
        (
            "strong",
            v.strong_count()
                .map_or_else(|| "N/A".to_string(), |v| v.to_string()),
        ),
        ("type-v", v.type_name_verbose().into()),
        ("len", v.len().to_string()),
        ("depth", v.depth().to_string()),
        ("shape", v.shape().to_string()),
        ("axes", v.axes().to_string()),
        ("uniform?", Value::Bool(v.is_uniform()).to_string()),
    ];

    let mut out = String::new();
    let _ = write!(
        out,
        "[xray] {}\n{}",
        v.type_name(),
        two_col_item_values(&pairs, 4, color)
    );

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

        config.toggle_box();
        assert_eq!(config.summary(), "[]");
        config.toggle_box();
        assert_eq!(config.summary(), "[box,axis,color]");
    }
}
