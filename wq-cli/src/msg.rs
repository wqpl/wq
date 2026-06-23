use wqpl::session::Session;
use wqpl::style::{AnsiColor, ColorMode, TextStyle, paint};

use crate::load::report::{LoadError, LoadErrorKind, LoadReport};

pub enum MsgType {
    Info,
    Error,
    Success,
}

fn format_msg(msg: impl Into<String>, msg_type: MsgType) -> String {
    format_msg_with_color_mode(msg, msg_type, ColorMode::Auto)
}

fn format_msg_with_color_mode(
    msg: impl Into<String>,
    msg_type: MsgType,
    color_mode: ColorMode,
) -> String {
    let msg = msg.into();
    let mut lines = msg.lines();
    let mut formatted = String::new();
    if let Some(first) = lines.next() {
        let prompt = "\u{258D}";
        let colored_prompt = match msg_type {
            MsgType::Info => paint(prompt, TextStyle::new().fg(AnsiColor::Cyan), color_mode),
            MsgType::Error => paint(prompt, TextStyle::new().fg(AnsiColor::Red), color_mode),
            MsgType::Success => paint(prompt, TextStyle::new().fg(AnsiColor::Green), color_mode),
        };
        let colored_first = match msg_type {
            MsgType::Info => paint(first, TextStyle::new().fg(AnsiColor::Cyan), color_mode),
            MsgType::Error => paint(first, TextStyle::new().fg(AnsiColor::Red), color_mode),
            MsgType::Success => first.to_string(),
        };
        formatted.push_str(&format!("{colored_prompt} {colored_first}\n"));
    }
    for line in lines {
        formatted.push_str(&format!("  {line}\n"));
    }
    if formatted.ends_with('\n') {
        formatted.pop();
    }
    formatted
}

pub fn system_msg_out(msg: impl Into<String>, msg_type: MsgType) {
    println!("{}", format_msg(msg, msg_type));
}

pub fn system_msg_err(msg: impl Into<String>, msg_type: MsgType) {
    eprintln!("{}", format_msg(msg, msg_type));
}

pub fn print_load_error(err: &LoadError, session: &mut Session) {
    match &err.kind {
        LoadErrorKind::Cycle(path) => {
            system_msg_err(
                format!("Cycle load, aborting: {}", path.display()),
                MsgType::Error,
            );
        }
        LoadErrorKind::Io(path, e) => {
            system_msg_err(
                format!("Cannot load {}: {}", path.display(), e),
                MsgType::Error,
            );
        }
        LoadErrorKind::Eval(label, e) => {
            system_msg_err(format!("Error at {label}\n{e}"), MsgType::Error);
            if session.get_bt_mode() && e.err_type.is_runtime() {
                session.dbg_print_bt();
            }
        }
        LoadErrorKind::Directive(cmd) => {
            system_msg_err(format!("Unknown directive: {cmd}"), MsgType::Error);
        }
    }
    if !err.stack.is_empty() {
        system_msg_err(
            format!("Import stack: {}", err.stack.join(" -> ")),
            MsgType::Info,
        );
    }
}

pub fn print_load_report(report: &LoadReport) {
    for w in &report.warnings {
        system_msg_err(format!("warning: {w}"), MsgType::Info);
    }
    if report.new_bindings.is_empty() && report.overridden.is_empty() {
        system_msg_err(
            format!("no new bindings from '{}'", report.label),
            MsgType::Info,
        );
        return;
    }
    if !report.new_bindings.is_empty() {
        system_msg_err(
            format!(
                "new bindings from '{}': {}",
                report.label,
                report.new_bindings.join(", ")
            ),
            MsgType::Info,
        );
    }
    if !report.overridden.is_empty() {
        system_msg_err(
            format!(
                "overridden bindings from '{}': {}",
                report.label,
                report.overridden.join(", ")
            ),
            MsgType::Info,
        );
    }
}

pub fn print_dry_run_status() {
    system_msg_out("dry: skipped execution".to_string(), MsgType::Info);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_message_uses_explicit_style_renderer() {
        assert_eq!(
            format_msg_with_color_mode("oops\nagain", MsgType::Error, ColorMode::Always),
            "\x1b[31m\u{258D}\x1b[0m \x1b[31moops\x1b[0m\n  again"
        );
    }

    #[test]
    fn success_message_can_render_without_color() {
        assert_eq!(
            format_msg_with_color_mode("loaded", MsgType::Success, ColorMode::Never),
            "\u{258D} loaded"
        );
    }
}
