use std::io::IsTerminal as _;

use terminal_size::{Width, terminal_size};
use wqpl::display;
use wqpl::value::Value;

use crate::arg::BoxPrintConfig;

fn color_enabled(config: &BoxPrintConfig) -> bool {
    config.color && std::io::stdout().is_terminal()
}

pub(crate) fn terminal_width() -> Option<usize> {
    terminal_size().map(|(Width(width), _)| usize::from(width))
}

pub(crate) fn format_print_result(
    result: &Value,
    config: &BoxPrintConfig,
    max_width: Option<usize>,
    source_styler: Option<&dyn Fn(&str) -> String>,
) -> String {
    let color = color_enabled(config);
    display::format_print_result_with(
        result,
        config,
        display::ResultFormatOptions {
            color,
            max_width,
            source_styler: source_styler.filter(|_| color),
        },
    )
}

pub(crate) fn format_xray_info(v: &Value, config: &BoxPrintConfig) -> String {
    display::format_xray_info(v, color_enabled(config))
}
