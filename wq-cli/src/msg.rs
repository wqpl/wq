use colored::Colorize;
use wqpl::session::Session;

use crate::load::report::{LoadError, LoadErrorKind, LoadReport};

pub enum MsgType {
    Info,
    Error,
    Success,
}

fn format_msg(msg: impl Into<String>, msg_type: MsgType) -> String {
    let msg = msg.into();
    let mut lines = msg.lines();
    let mut formatted = String::new();
    if let Some(first) = lines.next() {
        let prompt = "\u{258D}";
        let colored_prompt = match msg_type {
            MsgType::Info => prompt.cyan(),
            MsgType::Error => prompt.red(),
            MsgType::Success => prompt.green(),
        };
        let colored_first = match msg_type {
            MsgType::Info => first.cyan(),
            MsgType::Error => first.red(),
            MsgType::Success => first.into(),
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
    // match msg_type {
    //     MsgType::Info => formatted.cyan().to_string(),
    //     MsgType::Error => formatted.black().on_bright_red().to_string(),
    //     MsgType::Success => formatted,
    // }
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
