use std::io::IsTerminal as _;

use wqpl::display;
use wqpl::value::Value;

use crate::arg::BoxPrintConfig;

fn color_enabled(config: &BoxPrintConfig) -> bool {
    config.color && std::io::stdout().is_terminal()
}

pub(crate) fn format_print_result(result: &Value, config: &BoxPrintConfig) -> String {
    display::format_print_result(result, config, color_enabled(config))
}

pub(crate) fn format_non_cas_result(result: &Value, config: &BoxPrintConfig) -> String {
    display::format_non_cas_result(result, config, color_enabled(config))
}

pub(crate) fn format_xray_info(v: &Value, config: &BoxPrintConfig) -> String {
    display::format_xray_info(v, color_enabled(config))
}
