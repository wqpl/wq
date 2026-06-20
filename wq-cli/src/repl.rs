pub mod editor;
pub mod input;

use std::cell::RefCell;
use std::collections::HashSet;
use std::io::Write as _;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use std::{env, thread};

use colored::Colorize as _;
use rand::RngExt as _;
use terminal_size::{Width, terminal_size};
use wqpl::builtins::{BuiltinPreset, Builtins};
use wqpl::completion as wq_completion;
use wqpl::doc;
use wqpl::format::{FormatConfig, Formatter};
use wqpl::interpret::InterpreterKind;
use wqpl::session::Session;
use wqpl::session::dbglog::{DebugLogFlags, get_debug_log_flags, set_debug_log_flags};
use wqpl::session::stdio::{
    WqStdinError, set_wqstdin, wqstdin_add_history, wqstdin_highlight_enabled,
    wqstdin_hints_enabled, wqstdin_readline, wqstdin_set_builtin_hints, wqstdin_set_global_hints,
    wqstdin_set_highlight, wqstdin_set_hints_enabled, wqstdin_set_repl_hints,
};
use wqpl::value::{Excerpt, Value};

use crate::arg::{BoxPrintConfig, FmtOpts, RuntimeFlags, apply_box_spec};
use crate::display::{format_non_cas_result, format_xray_info};
use crate::load::eval_inline_with_load;
use crate::msg::{
    MsgType, print_dry_run_status as raw_print_dry_run_status,
    print_load_error as raw_print_load_error, print_load_report as raw_print_load_report,
    system_msg_err as raw_system_msg_err, system_msg_out as raw_system_msg_out,
};
use crate::repl::editor::WqReplHighlighter;
use crate::repl::input::RustylineInput;
use crate::wqdb::enter_wqdb_after_err;
use crate::{apply_builtins_flag, apply_interpreter_flag, wqdb_pause_handler};

#[derive(Debug, Clone)]
enum ReplCommand {
    Exit,
    Bye,
    Goodbye,
    Highlight,
    Hint,
    Info,
    Dry,
    Fmt(Option<String>),
    Bfn(Option<String>),
    Gb,
    Reset,
    Box,
    BoxSet(String),
    Backtrace,
    Xray,
    Interpreter(Option<String>),
    Time,
    TimeOneshot,
    Wqdb,
    WqdbOneshot,
    Help(Option<String>),
    DebugShow,
    DebugToggle,
    DebugOneshot(String),
    DebugSet(String),
    // Exp,
    // ExpSet(String),
    DryQuery,
    BoxQuery,
    BacktraceQuery,
    XrayQuery,
    HighlightQuery,
    HintQuery,
    TimeQuery,
    WqdbQuery,
    FmtQuery,
    TypeShow,
    TypeQuery,
    Empty,
    Unknown,
}

impl ReplCommand {
    fn parse(input: &str) -> Self {
        let trimmed = input.trim();
        match trimmed {
            "" => Self::Empty,
            "!exit" | "!e" | "!!" => Self::Exit,
            "!bye" => Self::Bye,
            "!goodbye" => Self::Goodbye,
            "!highlight" | "!hl" => Self::Highlight,
            "!hint" => Self::Hint,
            "!info" => Self::Info,
            "!dry" => Self::Dry,
            "!fmt" => Self::Fmt(None),
            "!gb" | "!g" => Self::Gb,
            "!reset" | "!r" => Self::Reset,
            "!box" | "!b" => Self::Box,
            "!backtrace" | "!bt" => Self::Backtrace,
            "!xray" | "!x" => Self::Xray,
            "!interpreter" | "!i" => Self::Interpreter(None),
            "!time" | "!t" => Self::Time,
            "!t." | "!time." => Self::TimeOneshot,
            "!wqdb" | "!w" => Self::Wqdb,
            "!wqdb." | "!w." => Self::WqdbOneshot,
            "!debug" => Self::DebugShow,
            "!d" => Self::DebugToggle,
            // "!exp" => Self::Exp,
            "!dry?" => Self::DryQuery,
            "!box?" | "!b?" => Self::BoxQuery,
            "!backtrace?" | "!bt?" => Self::BacktraceQuery,
            "!xray?" | "!x?" => Self::XrayQuery,
            "!highlight?" | "!hl?" => Self::HighlightQuery,
            "!hint?" => Self::HintQuery,
            "!time?" | "!t?" => Self::TimeQuery,
            "!wqdb?" | "!w?" => Self::WqdbQuery,
            "!fmt?" => Self::FmtQuery,
            "!help" | "!h" => Self::Help(None),
            "!type" => Self::TypeShow,
            "!type?" => Self::TypeQuery,
            _ => {
                if let Some(rest) = trimmed.strip_prefix("!fmt ") {
                    Self::Fmt(Some(rest.to_string()))
                } else if let Some(rest) = trimmed.strip_prefix("!box ") {
                    Self::BoxSet(rest.to_string())
                } else if let Some(rest) = trimmed.strip_prefix("!b ") {
                    Self::BoxSet(rest.to_string())
                } else if trimmed == "!bfn" || trimmed == "!" {
                    Self::Bfn(None)
                } else if let Some(rest) = trimmed.strip_prefix("!bfn ") {
                    Self::Bfn(Some(rest.to_string()))
                } else if let Some(rest) = trimmed.strip_prefix("!interpreter ") {
                    Self::Interpreter(Some(rest.to_string()))
                } else if let Some(rest) = trimmed.strip_prefix("!i ") {
                    Self::Interpreter(Some(rest.to_string()))
                } else if let Some(rest) = trimmed.strip_prefix("!help ") {
                    Self::Help(Some(rest.to_string()))
                } else if let Some(rest) = trimmed.strip_prefix("!h ") {
                    Self::Help(Some(rest.to_string()))
                } else if let Some(rest) = trimmed.strip_prefix("!d.") {
                    Self::DebugOneshot(rest.to_string())
                } else if let Some(rest) = trimmed.strip_prefix("!debug.") {
                    Self::DebugOneshot(rest.to_string())
                } else if let Some(rest) = trimmed.strip_prefix("!d ") {
                    Self::DebugSet(rest.to_string())
                } else if let Some(rest) = trimmed.strip_prefix("!debug ") {
                    Self::DebugSet(rest.to_string())
                }
                // else if let Some(rest) = trimmed.strip_prefix("!exp ") {
                //     Self::ExpSet(rest.to_string())
                // }
                else if let Some(rest) = trimmed.strip_prefix("!d") {
                    Self::DebugSet(rest.to_string())
                } else {
                    Self::Unknown
                }
            }
        }
    }

    fn all_names_and_descs() -> Vec<(&'static str, &'static str)> {
        vec![
            ("!exit", "exit the repl"),
            ("!e", "exit the repl"),
            ("!!", "exit the repl"),
            ("!bye", "exit the repl"),
            ("!goodbye", "exit with style"),
            ("!highlight", "toggle syntax highlighting"),
            ("!hl", "toggle syntax highlighting"),
            ("!highlight?", "show highlight status"),
            ("!hl?", "show highlight status"),
            ("!hint", "toggle hints"),
            ("!hint?", "show hint status"),
            ("!info", "show repl info"),
            ("!dry", "toggle dry mode"),
            ("!dry?", "show dry mode status"),
            ("!fmt", "toggle formatter"),
            ("!fmt?", "show formatter status"),
            ("!bfn", "show or set builtins preset"),
            ("!", "show builtins preset"),
            ("!p", "load prelude"),
            ("!load", "load embedded script or file"),
            ("!l", "load embedded script or file"),
            ("!gb", "show global bindings"),
            ("!g", "show global bindings"),
            ("!reset", "reset session"),
            ("!r", "reset session"),
            ("!box", "toggle all display config"),
            ("!b", "toggle all display config"),
            ("!box <spec>", "set display config; on/off or +/- modifies"),
            ("!b <spec>", "set display config; on/off or +/- modifies"),
            ("!box?", "show display config"),
            ("!b?", "show display config"),
            ("!backtrace", "toggle backtrace"),
            ("!bt", "toggle backtrace"),
            ("!backtrace?", "show backtrace status"),
            ("!bt?", "show backtrace status"),
            ("!xray", "toggle xray"),
            ("!x", "toggle xray"),
            ("!xray?", "show xray status"),
            ("!x?", "show xray status"),
            ("!interpreter", "show or set interpreter"),
            ("!i", "show or set interpreter"),
            ("!time", "toggle time mode"),
            ("!t", "toggle time mode"),
            ("!time?", "show time mode status"),
            ("!t?", "show time mode status"),
            ("!t.", "time mode for next eval"),
            ("!time.", "time mode for next eval"),
            ("!wqdb", "toggle wqdb"),
            ("!w", "toggle wqdb"),
            ("!wqdb?", "show wqdb status"),
            ("!w?", "show wqdb status"),
            ("!wqdb.", "wqdb for next eval"),
            ("!w.", "wqdb for next eval"),
            ("!help", "show help"),
            ("!h", "show help"),
            ("!type", "toggle type mode"),
            ("!type?", "show type mode status"),
            ("!debug", "show debug flags help"),
            ("!d", "toggle debug flags"),
            ("!d <spec>", "set debug flags; +/- modifies"),
            ("!exp", "show or toggle experimental features"),
        ]
    }
}

pub fn enter_repl(rtflags: RuntimeFlags) {
    let mut session = Session::new();
    session.set_pause_callback(Some(wqdb_pause_handler));
    let mut time_mode = false;
    let mut box_config = rtflags.box_print;
    let mut dry_mode = rtflags.dry;
    let mut show_type = true;
    let mut fmt_state = ReplFmtState::default();
    let highlighter = WqReplHighlighter::new();
    set_debug_log_flags(rtflags.debug_flags);
    session.set_bt_mode(rtflags.bt);
    session.set_wqdb(rtflags.wqdb);
    session.set_dry_mode(dry_mode);
    apply_builtins_flag(&mut session, &rtflags);
    apply_interpreter_flag(&mut session, &rtflags);
    set_wqstdin(Box::new(RustylineInput::new().unwrap()));
    sync_builtin_hints(&session);

    let mut line_number = 1;
    // one-time controls for next input
    let mut oneshot_time = false;
    let mut oneshot_debug: Option<DebugLogFlags> = None;
    let mut oneshot_wqdb = false;
    // Unified loader state for directive lines handled by load
    let repl_loading = RefCell::new(HashSet::new());
    print_repl_startup(&session, rtflags.stack_size_mebibyte);
    sync_global_hints(&session);
    sync_repl_hints();

    loop {
        let prompt = if cfg!(windows) {
            format!("wq[{line_number}] ")
        } else {
            format!("{}[{}] ", "wq".magenta(), line_number.to_string().blue())
        };

        match wqstdin_readline(&prompt) {
            Ok(line) => {
                let input = line.trim_end_matches('\r');
                if !input.is_empty() {
                    wqstdin_add_history(input);
                }
                // Handle repl commands
                match ReplCommand::parse(input) {
                    ReplCommand::Exit | ReplCommand::Bye => {
                        system_msg_out("bye..".to_string(), MsgType::Info);
                        break;
                    }
                    ReplCommand::Goodbye => {
                        print_goodbye();
                        break;
                    }
                    ReplCommand::Highlight => {
                        wqstdin_set_highlight(!wqstdin_highlight_enabled());
                        continue;
                    }
                    ReplCommand::Hint => {
                        let on = !wqstdin_hints_enabled();
                        wqstdin_set_hints_enabled(on);
                        system_msg_out(
                            format!("hint -> {}", (if on { "on" } else { "off" }).underline()),
                            MsgType::Info,
                        );
                        continue;
                    }
                    ReplCommand::Info => {
                        print_repl_startup(&session, rtflags.stack_size_mebibyte);
                        continue;
                    }
                    ReplCommand::Dry => {
                        dry_mode = !dry_mode;
                        session.set_dry_mode(dry_mode);
                        system_msg_out(
                            format!(
                                "dry -> {}",
                                (if dry_mode { "on" } else { "off" }).underline()
                            ),
                            MsgType::Info,
                        );
                        continue;
                    }
                    ReplCommand::Fmt(None) => {
                        fmt_state.enabled = !fmt_state.enabled;
                        system_msg_out(format!("fmt -> {}", fmt_state.summary()), MsgType::Info);
                        continue;
                    }
                    ReplCommand::Fmt(Some(spec)) => {
                        match fmt_state.toggle_modes(&spec) {
                            Ok(()) => {
                                system_msg_out(
                                    format!("fmt -> {}", fmt_state.summary()),
                                    MsgType::Info,
                                );
                            }
                            Err(err) => {
                                system_msg_err(err, MsgType::Error);
                            }
                        }
                        continue;
                    }
                    ReplCommand::Bfn(opt) => {
                        let names = BuiltinPreset::names().join(", ");
                        if let Some(preset) = opt {
                            match BuiltinPreset::from_name(&preset) {
                                Some(preset) => {
                                    session.set_builtins_preset(preset);
                                    sync_builtin_hints(&session);
                                    system_msg_out(
                                        format!("bfn -> {}", preset.name()),
                                        MsgType::Info,
                                    );
                                }
                                None => {
                                    system_msg_err(
                                        format!(
                                            "unknown bfn preset '{preset}'\nAvailable: {names}"
                                        ),
                                        MsgType::Error,
                                    );
                                }
                            }
                        } else {
                            let current = session.builtins_preset();
                            system_msg_err(
                                format!(
                                    "Current: {}\nAvailable: {names}",
                                    current.name().bold().underline()
                                ),
                                MsgType::Info,
                            );
                            dump_builtins(session.builtins());
                        }
                        continue;
                    }
                    ReplCommand::Gb => {
                        let env = session.env_vars();
                        if env.is_empty() {
                            system_msg_out("no global bindings".to_string(), MsgType::Info);
                        } else {
                            let mut name_w = "name".len();
                            let mut value_w = "value".len();
                            let mut type_w = "type".len();
                            for (name, v) in &env {
                                name_w = name_w.max(name.len());
                                value_w = value_w.max(v.to_string().len());
                                type_w = type_w.max(v.type_name().len());
                            }
                            eprintln!(
                                "{:<name_w$}  {:<value_w$}  {:<type_w$}",
                                "name",
                                "value",
                                "type",
                                name_w = name_w,
                                value_w = value_w,
                                type_w = type_w
                            );
                            eprintln!(
                                "{:-<name_w$}  {:-<value_w$}  {:-<type_w$}",
                                "",
                                "",
                                "",
                                name_w = name_w,
                                value_w = value_w,
                                type_w = type_w
                            );
                            // Print rows
                            for (name, v) in &env {
                                eprintln!(
                                    "{:<name_w$}  {:<value_w$}  {:<type_w$}",
                                    name,
                                    v.to_string(),
                                    v.type_name(),
                                    name_w = name_w,
                                    value_w = value_w,
                                    type_w = type_w
                                );
                            }
                        }
                        continue;
                    }
                    ReplCommand::Reset => {
                        session.reset_session();
                        sync_global_hints(&session);
                        system_msg_out("session reset".to_string(), MsgType::Info);
                        continue;
                    }
                    ReplCommand::Box => {
                        box_config.toggle_box();
                        system_msg_out(
                            format!("box -> {}", box_config.summary().underline()),
                            MsgType::Info,
                        );
                        continue;
                    }
                    ReplCommand::BoxSet(spec) => {
                        match apply_box_spec(&mut box_config, &spec) {
                            Ok(()) => {
                                system_msg_out(
                                    format!("box -> {}", box_config.summary().underline()),
                                    MsgType::Info,
                                );
                            }
                            Err(err) => system_msg_err(err, MsgType::Error),
                        }
                        continue;
                    }
                    ReplCommand::Backtrace => {
                        let bt_mode = !session.get_bt_mode();
                        session.set_bt_mode(bt_mode);
                        system_msg_out(
                            format!(
                                "backtrace -> {}",
                                (if bt_mode { "on" } else { "off" }).underline()
                            ),
                            MsgType::Info,
                        );
                        continue;
                    }
                    ReplCommand::Xray => {
                        box_config.toggle_xray();
                        system_msg_out(
                            format!("box -> {}", box_config.summary().underline()),
                            MsgType::Info,
                        );
                        continue;
                    }
                    ReplCommand::Interpreter(None) => {
                        let current = session.interpreter_name();
                        let list = format!(
                            "Current: {}\nAvailable: {}",
                            current.underline(),
                            InterpreterKind::names().join(", ")
                        );
                        system_msg_out(list, MsgType::Info);
                        continue;
                    }
                    ReplCommand::Interpreter(Some(name)) => {
                        match session.set_interpreter_by_name(&name) {
                            Ok(selected) => {
                                system_msg_out(format!("interpreter -> {selected}"), MsgType::Info);
                            }
                            Err(err) => {
                                let list = InterpreterKind::names().join(", ");
                                system_msg_err(format!("{err}\nAvailable: {list}"), MsgType::Error);
                            }
                        }
                        continue;
                    }
                    ReplCommand::Time => {
                        time_mode = !time_mode;
                        system_msg_out(
                            format!(
                                "time -> {}",
                                (if time_mode { "on" } else { "off" }).underline()
                            ),
                            MsgType::Info,
                        );
                        continue;
                    }
                    ReplCommand::TimeOneshot => {
                        oneshot_time = true;
                        system_msg_out(
                            format!("time -> {}", "on for next eval".underline()),
                            MsgType::Info,
                        );
                        continue;
                    }
                    ReplCommand::Wqdb => {
                        session.set_wqdb(!session.is_wqdb_enabled());
                        system_msg_out(
                            format!(
                                "wqdb -> {}",
                                (if session.is_wqdb_enabled() {
                                    "on"
                                } else {
                                    "off"
                                })
                                .underline()
                            ),
                            MsgType::Info,
                        );
                        continue;
                    }
                    ReplCommand::WqdbOneshot => {
                        oneshot_wqdb = true;
                        system_msg_out(
                            format!("wqdb -> {}", "on for next eval".underline()),
                            MsgType::Info,
                        );
                        continue;
                    }
                    ReplCommand::Help(opt) => {
                        if let Some(name) = opt {
                            if let Some(topic) = doc::resolve(&name) {
                                let fold_width = crate::help::auto_fold_width(
                                    terminal_size().map(|(Width(width), _)| width as usize),
                                );
                                println!(
                                    "{}",
                                    crate::help::render_reference_topic(
                                        &topic,
                                        fold_width,
                                        &highlighter
                                    )
                                );
                            } else {
                                system_msg_out(
                                    format!("unknown help topic '{name}'"),
                                    MsgType::Info,
                                );
                            }
                        } else {
                            let refcard = include_str!("../../d/refcard");
                            let lines: Vec<&str> = refcard.lines().collect();
                            let width = lines.iter().map(|l| vis_width(l)).max().unwrap_or(0);
                            let top = format!("┌{}┐", "─".repeat(width + 4));
                            let bot = format!("└{}┘", "─".repeat(width + 4));
                            println!("{}", top.dimmed());
                            for line in lines {
                                println!("│  {}  │", pad_vis(line.to_string(), width));
                            }
                            println!("{}", bot.dimmed());
                        }
                        continue;
                    }
                    ReplCommand::DebugShow => {
                        system_msg_out(debug_help_table(get_debug_log_flags()), MsgType::Info);
                        continue;
                    }
                    ReplCommand::DebugToggle => {
                        let next = if get_debug_log_flags().is_empty() {
                            DebugLogFlags::from_alias(1).expect("debug alias 1 exists")
                        } else {
                            DebugLogFlags::empty()
                        };
                        set_debug_log_flags(next);
                        system_msg_out(
                            format!("debug flags -> {}", format_debug_flags(next).underline()),
                            MsgType::Info,
                        );
                        continue;
                    }
                    ReplCommand::DebugOneshot(rest) => {
                        let mut flags = get_debug_log_flags();
                        match flags.apply_spec(&rest) {
                            Ok(()) => {
                                oneshot_debug = Some(flags);
                                system_msg_out(
                                    format!(
                                        "debug flags -> {} for next eval",
                                        format_debug_flags(flags).underline()
                                    ),
                                    MsgType::Info,
                                );
                            }
                            Err(e) => {
                                system_msg_err(e, MsgType::Error);
                            }
                        }
                        continue;
                    }
                    ReplCommand::DebugSet(rest) => {
                        let mut flags = get_debug_log_flags();
                        match flags.apply_spec(&rest) {
                            Ok(()) => {
                                set_debug_log_flags(flags);
                                system_msg_out(
                                    format!(
                                        "debug flags -> {}",
                                        format_debug_flags(flags).underline()
                                    ),
                                    MsgType::Info,
                                );
                            }
                            Err(e) => system_msg_err(e, MsgType::Error),
                        }
                        continue;
                    }
                    // ReplCommand::Exp => {
                    //     system_msg_out("exp nyi", MsgType::Info);
                    //     continue;
                    // }
                    // ReplCommand::ExpSet(_) => {
                    //     system_msg_out("exp nyi", MsgType::Info);
                    //     continue;
                    // }
                    ReplCommand::DryQuery => {
                        system_msg_out(
                            format!("dry: {}", (if dry_mode { "on" } else { "off" }).underline()),
                            MsgType::Info,
                        );
                        continue;
                    }
                    ReplCommand::BoxQuery => {
                        system_msg_out(
                            format!("box: {}", box_config.summary().underline()),
                            MsgType::Info,
                        );
                        continue;
                    }
                    ReplCommand::BacktraceQuery => {
                        let bt_mode = session.get_bt_mode();
                        system_msg_out(
                            format!(
                                "backtrace: {}",
                                (if bt_mode { "on" } else { "off" }).underline()
                            ),
                            MsgType::Info,
                        );
                        continue;
                    }
                    ReplCommand::XrayQuery => {
                        system_msg_out(
                            format!(
                                "xray: {}",
                                (if box_config.shows_xray() { "on" } else { "off" }).underline()
                            ),
                            MsgType::Info,
                        );
                        continue;
                    }
                    ReplCommand::HighlightQuery => {
                        let on = wqstdin_highlight_enabled();
                        system_msg_out(
                            format!("highlight: {}", (if on { "on" } else { "off" }).underline()),
                            MsgType::Info,
                        );
                        continue;
                    }
                    ReplCommand::HintQuery => {
                        let on = wqstdin_hints_enabled();
                        system_msg_out(
                            format!("hint: {}", (if on { "on" } else { "off" }).underline()),
                            MsgType::Info,
                        );
                        continue;
                    }
                    ReplCommand::TimeQuery => {
                        let status = if time_mode {
                            "on"
                        } else if oneshot_time {
                            "on for next eval"
                        } else {
                            "off"
                        };
                        system_msg_out(format!("time: {}", status.underline()), MsgType::Info);
                        continue;
                    }
                    ReplCommand::WqdbQuery => {
                        let status = if session.is_wqdb_enabled() {
                            "on"
                        } else if oneshot_wqdb {
                            "on for next eval"
                        } else {
                            "off"
                        };
                        system_msg_out(format!("wqdb: {}", status.underline()), MsgType::Info);
                        continue;
                    }
                    ReplCommand::FmtQuery => {
                        system_msg_out(format!("fmt: {}", fmt_state.summary()), MsgType::Info);
                        continue;
                    }
                    ReplCommand::TypeShow => {
                        show_type = !show_type;
                        system_msg_out(
                            format!(
                                "type -> {}",
                                (if show_type { "on" } else { "off" }).underline()
                            ),
                            MsgType::Info,
                        );
                        continue;
                    }
                    ReplCommand::TypeQuery => {
                        system_msg_out(
                            format!(
                                "type: {}",
                                (if show_type { "on" } else { "off" }).underline()
                            ),
                            MsgType::Info,
                        );
                        continue;
                    }
                    ReplCommand::Empty => {
                        continue;
                    }
                    ReplCommand::Unknown => {}
                }
                let mut input_for_eval = input;
                if let Some((first, rest)) = input_for_eval.split_once('\n') {
                    if first.trim_start().starts_with("#!") {
                        if rest.trim().is_empty() {
                            continue;
                        }
                        input_for_eval = rest;
                    }
                } else if input_for_eval.trim_start().starts_with("#!") {
                    continue;
                }
                let t = input_for_eval.trim_start();
                if t.starts_with("!") {
                    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                    match eval_inline_with_load(
                        &mut session,
                        input_for_eval,
                        &cwd,
                        &repl_loading,
                        false,
                    ) {
                        Ok(report) => {
                            print_load_report(&report);
                            maybe_dump_formatted_input(&fmt_state, &highlighter, input_for_eval);
                            if dry_mode {
                                print_dry_run_status();
                            }
                            if let Some(result) = report.result
                                && !dry_mode
                            {
                                let resstr = format_repl_result_with_type(
                                    &result,
                                    &box_config,
                                    &highlighter,
                                    show_type,
                                );
                                print_repl_result_msg(resstr);
                                if box_config.shows_xray() {
                                    let info = format_xray_info(&result, &box_config);
                                    system_msg_out(info, MsgType::Info);
                                }
                            }
                            sync_global_hints(&session);
                        }
                        Err(err) => {
                            print_load_error(&err, &mut session);
                        }
                    }

                    continue;
                }
                let src_eval = input_for_eval.trim();
                // Prepare for one-time cmds
                let prev_dbg_flags = get_debug_log_flags();
                if let Some(flags) = oneshot_debug.take() {
                    set_debug_log_flags(flags);
                }
                if oneshot_wqdb {
                    session.set_wqdb(true);
                }

                // Ensure interactive inputs map to a unique source label per iteration
                let source_label = format!("wq[{}]", line_number);
                session.dbg_set_source(&source_label, src_eval);
                session.dbg_set_offset(0);
                // Note whether wqdb was active for this evaluation (persistent or one-time)
                let wqdb_active_for_eval = session.is_wqdb_enabled() || oneshot_wqdb;

                let start_t = Instant::now();
                let attempt = session.eval_string(src_eval);
                let elapsed_t = start_t.elapsed();

                // reset one-time cmds and wqdb
                if oneshot_wqdb {
                    session.set_wqdb(false);
                    oneshot_wqdb = false;
                }
                // reset one-time dbg level
                set_debug_log_flags(prev_dbg_flags);
                // handle eval result
                match attempt {
                    Ok(result) => {
                        maybe_dump_formatted_input(&fmt_state, &highlighter, src_eval);
                        if dry_mode {
                            print_dry_run_status();
                        } else {
                            let resstr = format_repl_result_with_type(
                                &result,
                                &box_config,
                                &highlighter,
                                show_type,
                            );
                            print_repl_result_msg(resstr);
                            if box_config.shows_xray() {
                                let info = format_xray_info(&result, &box_config);
                                system_msg_out(info, MsgType::Info);
                            }
                        }
                        if time_mode || oneshot_time {
                            system_msg_out(format!("time elapsed: {elapsed_t:?}"), MsgType::Info);
                            // reset one-time time mode
                            oneshot_time = false;
                        }
                        sync_global_hints(&session);
                    }
                    Err(error) => {
                        system_msg_err(format!("{error}"), MsgType::Error);
                        // Only show backtrace for runtime errors; skip for parse/EOF errors
                        if session.get_bt_mode() && error.err_type.is_runtime() {
                            session.dbg_print_bt();
                        }
                        if wqdb_active_for_eval && error.err_type.is_runtime() {
                            enter_wqdb_after_err(&mut session);
                        }
                        if time_mode || oneshot_time {
                            system_msg_out(format!("time elapsed: {elapsed_t:?}"), MsgType::Info);
                            oneshot_time = false;
                        }
                    }
                }
                line_number += 1;
            }
            Err(WqStdinError::Eof) => {
                break;
            }
            Err(WqStdinError::Interrupted) => {
                // Cancel one-time settings
                oneshot_time = false;
                oneshot_debug = None;
                oneshot_wqdb = false;
                continue;
            }
            Err(WqStdinError::Other(error)) => {
                system_msg_err(format!("Error reading input: {error}"), MsgType::Error);
                break;
            }
        }
    }
}

fn sync_builtin_hints(session: &Session) {
    let candidates = wq_completion::builtin_completion_candidates(session.builtins(), false);
    let mut names = Vec::with_capacity(candidates.len());
    let mut usages = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        names.push(candidate.label);
        usages.push(candidate.detail.unwrap_or_default());
    }
    wqstdin_set_builtin_hints(names, usages);
}

fn sync_global_hints(session: &Session) {
    let env = session.env_vars();
    let mut names = Vec::with_capacity(env.len());
    let mut types = Vec::with_capacity(env.len());
    let mut excerpts = Vec::with_capacity(env.len());
    for (name, value) in env {
        names.push(name.clone());
        types.push(value.type_name().to_string());
        excerpts.push(value.excerpt());
    }
    wqstdin_set_global_hints(names, types, excerpts);
}

fn sync_repl_hints() {
    let mut names = Vec::new();
    let mut descs = Vec::new();
    for (name, desc) in ReplCommand::all_names_and_descs() {
        names.push(name.to_string());
        descs.push(desc.to_string());
    }
    wqstdin_set_repl_hints(names, descs);
}

fn fold_home_dir(path: &std::path::Path) -> String {
    let home = if cfg!(windows) {
        env::var("USERPROFILE").ok().map(PathBuf::from)
    } else {
        env::var("HOME").ok().map(PathBuf::from)
    };

    if let Some(home) = home
        && let Ok(rest) = path.strip_prefix(&home)
    {
        let rest = rest.to_string_lossy();
        if rest.is_empty() {
            return "~".into();
        }
        return format!(
            "~{MAIN_SEPARATOR}{rest}",
            MAIN_SEPARATOR = std::path::MAIN_SEPARATOR
        );
    }
    path.to_string_lossy().into_owned()
}

fn print_repl_startup(evaluator: &Session, stack_size: usize) {
    const WQ_VERSION: &str = env!("CARGO_PKG_VERSION");
    const RUSTC_VER: &str = env!("RUSTC_VERSION");
    const RUSTC_HOST: &str = env!("RUSTC_HOST");
    const RUSTC_LLVM_VERSION: &str = env!("RUSTC_LLVM_VERSION");
    const BUILD_OPT_LEVEL: &str = env!("BUILD_OPT_LEVEL");
    const BUILD_PROFILE: &str = env!("BUILD_PROFILE");

    let cwd = match env::current_dir() {
        Ok(path) => fold_home_dir(&path),
        Err(_) => "?".into(),
    };
    let avpa = std::thread::available_parallelism()
        .map(|n| format!("{}", n.get()))
        .unwrap_or("?".to_string());
    let pid = format!("{}", std::process::id());
    let rustc_ver_short = RUSTC_VER.strip_prefix("rustc ").unwrap_or(RUSTC_VER);

    const INNER: usize = 44;

    let top = format!("┌{}┐", "─".repeat(INNER + 4));
    let sep = format!("├{}┤", "─".repeat(INNER + 4));
    let bot = format!("└{}┘", "─".repeat(INNER + 4));

    let mut lines: Vec<String> = Vec::new();

    lines.push(top.dimmed().to_string());
    let title = format!(
        "{}         {}",
        format!("wq {WQ_VERSION}").magenta(),
        "(c) tttiw  (l) MIT".dimmed()
    );
    lines.push(format!("│  {}  │", pad_vis(title, INNER)));
    let hints = format!("{}  {}", "!help".green(), "!exit".green());
    lines.push(format!("│  {}  │", pad_vis(hints, INNER)));
    lines.push(sep.dimmed().to_string());

    const SECOND_COL: usize = 31;

    let pad_label = |s: String, w: usize| {
        let v = vis_width(&s);
        if v < w {
            format!("{}{}", s, " ".repeat(w - v))
        } else {
            s
        }
    };

    let mut host_line = format!(
        "{}  {}",
        pad_label("host".blue().to_string(), 4),
        RUSTC_HOST.dimmed()
    );
    host_line.push_str(&" ".repeat(SECOND_COL.saturating_sub(vis_width(&host_line))));
    host_line.push_str(&format!(
        "{}  {}",
        pad_label("avpa".blue().to_string(), 5),
        avpa.dimmed()
    ));
    lines.push(format!("│  {}  │", pad_vis(host_line, INNER)));

    let mut pid_line = format!(
        "{}  {}",
        pad_label("pid".blue().to_string(), 4),
        pid.dimmed()
    );
    pid_line.push_str(&" ".repeat(SECOND_COL.saturating_sub(vis_width(&pid_line))));
    pid_line.push_str(&format!(
        "{}  {}",
        pad_label("stack".blue().to_string(), 5),
        stack_size.to_string().dimmed()
    ));
    lines.push(format!("│  {}  │", pad_vis(pid_line, INNER)));

    let cwd_prefix = format!("{}  ", pad_label("cwd".blue().to_string(), 4));
    let cwd_prefix_vis = vis_width(&cwd_prefix);
    let cwd_avail = INNER.saturating_sub(cwd_prefix_vis).max(1);
    let cwd_lines = wrap_text(&cwd, cwd_avail);
    for (i, chunk) in cwd_lines.iter().enumerate() {
        let content = if i == 0 {
            format!("{}{}", cwd_prefix, chunk.dimmed())
        } else {
            format!("{}{}", " ".repeat(cwd_prefix_vis), chunk.dimmed())
        };
        lines.push(format!("│  {}  │", pad_vis(content, INNER)));
    }

    let mut profile_line = format!(
        "{}  {}  o{}",
        "profile".red(),
        BUILD_PROFILE.dimmed(),
        BUILD_OPT_LEVEL
    );
    profile_line.push_str(&" ".repeat(SECOND_COL.saturating_sub(vis_width(&profile_line))));
    profile_line.push_str(&format!(
        "{}  {}",
        pad_label("llvm".red().to_string(), 5),
        RUSTC_LLVM_VERSION.dimmed()
    ));
    lines.push(format!("│  {}  │", pad_vis(profile_line, INNER)));

    lines.push(format!(
        "│  {}  │",
        pad_vis(
            format!("{}  {}", "rustc".red(), rustc_ver_short.dimmed()),
            INNER
        )
    ));

    let mut interp_line = format!(
        "{}  {}",
        "interpreter".bright_yellow(),
        evaluator.interpreter_name().dimmed()
    );
    interp_line.push_str(&" ".repeat(SECOND_COL.saturating_sub(vis_width(&interp_line))));
    interp_line.push_str(&format!(
        "{}  {}",
        pad_label("bfn".bright_yellow().to_string(), 5),
        evaluator.builtins_preset().name().dimmed()
    ));
    lines.push(format!("│  {}  │", pad_vis(interp_line, INNER)));

    lines.push(bot.dimmed().to_string());

    let term_w = terminal_size()
        .map(|(Width(w), _)| w as usize)
        .unwrap_or(80);
    const CARD_W: usize = INNER + 6;
    let sky_w = term_w.saturating_sub(CARD_W);

    if sky_w == 0 {
        for line in lines {
            println!("{}", line);
        }
        return;
    }

    let mut rng = rand::rng();
    let sky_bg = " ".to_string();
    let palette: [(u8, u8, u8); 18] = [
        (235, 166, 135), // #EBA687 light orange
        (122, 66, 191),  // #7A42BF purple
        (56, 78, 210),   // #384ED2 navy blue
        (178, 255, 180), // #B2FFB4 bright mint green
        (237, 28, 36),   // #ED1C24 red
        (236, 62, 192),  // #EC3EC0 pink
        (91, 158, 232),  // #5B9EE8 light blue
        (255, 231, 69),  // #FFE745 yellow
        (107, 203, 0),   // #6BCB00 green
        (254, 158, 35),  // #FE9E23 orange
        (179, 54, 201),  // #B336C9 purple
        (236, 200, 19),  // #ECC813 yellow
        (252, 95, 4),    // #FC5F04 orange
        (143, 215, 29),  // #8FD71D green
        (80, 222, 122),  // #50DE7A light green
        (84, 234, 245),  // #54EAF5 teal
        (237, 63, 133),  // #ED3F85 pink
        (254, 194, 250), // #FEC2FA light pink
    ];
    let sky_chars = ["·", ".", "*", "+", "•"];
    let sky_stars: Vec<String> = sky_chars
        .iter()
        .flat_map(|&ch| {
            palette
                .iter()
                .map(|(r, g, b)| ch.truecolor(*r, *g, *b).to_string())
        })
        .collect();

    let cat_rows: Vec<Vec<char>> = include_str!("../../d/wqcat")
        .lines()
        .skip_while(|l| l.trim().is_empty())
        .map(|l| l.chars().collect())
        .collect();
    let cat_h = cat_rows.len();
    let cat_w = cat_rows.iter().map(|r| r.len()).max().unwrap_or(0);
    let cat_x = if sky_w >= cat_w {
        (sky_w - cat_w) / 2
    } else {
        0
    };
    let cat_y_start = 0;
    let cat_chars = ["*", "•", "+"];
    let cat_stars: Vec<String> = cat_chars
        .iter()
        .flat_map(|&ch| {
            palette
                .iter()
                .map(|(r, g, b)| ch.truecolor(*r, *g, *b).to_string())
        })
        .collect();

    for (i, line) in lines.iter().enumerate() {
        let mut sky = String::new();
        let cat_row = if i >= cat_y_start && i < cat_y_start + cat_h {
            Some(&cat_rows[i - cat_y_start])
        } else {
            None
        };
        for x in 0..sky_w {
            let in_cat = if let Some(row) = cat_row {
                let cx = x.saturating_sub(cat_x);
                cx < row.len() && row[cx] == '*'
            } else {
                false
            };
            if in_cat && rng.random_bool(0.92) {
                sky.push_str(&cat_stars[rng.random_range(0..cat_stars.len())]);
            } else if rng.random_bool(0.05) {
                sky.push_str(&sky_stars[rng.random_range(0..sky_stars.len())]);
            } else {
                sky.push_str(&sky_bg);
            }
        }
        println!("{}{}", line, sky);
    }
}

fn format_repl_result(
    result: &Value,
    box_config: &BoxPrintConfig,
    highlighter: &WqReplHighlighter,
) -> String {
    if result.is_cas() {
        let expr = format!("{result}");
        highlighter.highlight_text(&expr)
    } else {
        format_non_cas_result(result, box_config)
    }
}

fn print_repl_result_msg(msg: String) {
    system_msg_out(msg, MsgType::Success);
}

fn system_msg_out(msg: impl Into<String>, msg_type: MsgType) {
    println!();
    raw_system_msg_out(msg, msg_type);
    println!();
}

fn system_msg_err(msg: impl Into<String>, msg_type: MsgType) {
    eprintln!();
    raw_system_msg_err(msg, msg_type);
    eprintln!();
}

fn print_load_report(report: &crate::load::report::LoadReport) {
    println!();
    raw_print_load_report(report);
    println!();
}

fn print_load_error(err: &crate::load::report::LoadError, session: &mut Session) {
    eprintln!();
    raw_print_load_error(err, session);
    eprintln!();
}

fn print_dry_run_status() {
    println!();
    raw_print_dry_run_status();
    println!();
}

fn vis_width(s: &str) -> usize {
    let mut w = 0;
    let mut esc = false;
    for ch in s.chars() {
        if esc {
            if ch == 'm' {
                esc = false;
            }
        } else if ch == '\x1b' {
            esc = true;
        } else {
            w += 1;
        }
    }
    w
}

fn pad_vis(s: String, width: usize) -> String {
    let v = vis_width(&s);
    if v < width {
        format!("{}{}", s, " ".repeat(width - v))
    } else {
        s
    }
}

fn wrap_text(s: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut line_w = 0;
    for ch in s.chars() {
        if line_w >= width && !line.is_empty() {
            lines.push(line);
            line = String::new();
            line_w = 0;
        }
        line.push(ch);
        line_w += 1;
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

fn format_repl_result_with_type(
    result: &Value,
    box_config: &BoxPrintConfig,
    highlighter: &WqReplHighlighter,
    show_type: bool,
) -> String {
    let mut resstr = format_repl_result(result, box_config, highlighter);
    if !show_type {
        return resstr;
    }
    let term_w = terminal_size()
        .map(|(Width(w), _)| w as usize)
        .unwrap_or(80);
    let type_str = result.type_name();
    let type_vis = vis_width(type_str);
    const PREFIX_W: usize = 2; // "▍ "

    let lines: Vec<&str> = resstr.split('\n').collect();
    let first_vis = vis_width(lines[0]);
    let total_needed = PREFIX_W + first_vis + 1 + type_vis;

    if term_w >= total_needed {
        let pad = term_w - total_needed;
        let mut out = String::with_capacity(resstr.len() + pad + type_str.len() + 20);
        out.push_str(lines[0]);
        out.push_str(&" ".repeat(pad));
        out.push_str(&type_str.dimmed().to_string());
        for line in &lines[1..] {
            out.push('\n');
            out.push_str(line);
        }
        resstr = out;
    } else {
        // Not enough space: place type on its own line, right-aligned.
        let pad = term_w.saturating_sub(PREFIX_W + type_vis);
        resstr.push('\n');
        resstr.push_str(&" ".repeat(pad));
        resstr.push_str(&type_str.dimmed().to_string());
    }

    resstr
}

#[derive(Debug, Clone, Default)]
struct ReplFmtState {
    enabled: bool,
    opts: FmtOpts,
}

impl ReplFmtState {
    fn toggle_modes(&mut self, spec: &str) -> Result<(), String> {
        if spec.is_empty() {
            self.enabled = !self.enabled;
            return Ok(());
        }
        let mut saw_mode_toggle = false;
        for raw_mode in spec.split(',') {
            let mode = raw_mode.trim();
            match mode {
                "" => {}
                "on" => self.enabled = true,
                "off" => self.enabled = false,
                "nlcd" => {
                    self.opts.nlcd = !self.opts.nlcd;
                    saw_mode_toggle = true;
                }
                "olw" => {
                    self.opts.olw = !self.opts.olw;
                    saw_mode_toggle = true;
                }
                other => {
                    return Err(format!(
                        "unknown fmt mode '{other}'\nAvailable: on, off, nlcd, olw"
                    ));
                }
            }
        }
        if saw_mode_toggle {
            self.enabled = true;
        }
        Ok(())
    }

    fn config(&self) -> FormatConfig {
        FormatConfig {
            indent_size: 2,
            nlcd: self.opts.nlcd,
            one_line_wizard: self.opts.olw,
            ..FormatConfig::default()
        }
    }

    fn modes(&self) -> String {
        let mut modes = Vec::new();
        if self.opts.nlcd {
            modes.push("nlcd");
        }
        if self.opts.olw {
            modes.push("olw");
        }
        if modes.is_empty() {
            "default".into()
        } else {
            modes.join(",")
        }
    }

    fn summary(&self) -> String {
        format!(
            "{} [{}]",
            if self.enabled { "on" } else { "off" },
            self.modes()
        )
    }
}

fn maybe_dump_formatted_input(state: &ReplFmtState, highlighter: &WqReplHighlighter, src: &str) {
    if !state.enabled {
        return;
    }
    let fmt = Formatter::new(state.config());
    match fmt.format_script(src) {
        Ok(formatted) => {
            system_msg_out(format!("formatter [{}]:", state.modes()), MsgType::Info);
            println!("{}", highlighter.highlight_text(&formatted));
        }
        Err(err) => {
            system_msg_err(format!("formatter failed: {err}"), MsgType::Error);
        }
    }
}

fn print_goodbye() {
    let mut rng = rand::rng();
    let mut stdout = std::io::stdout();
    let frames = if rng.random_bool(0.5) {
        [";D", ";D", ";D", ";D", ";)"]
    } else {
        [":D", ":D", ":D", ":D", ":)"]
    };
    print!("{}", "\u{258D} goodbye! ".cyan());
    stdout.flush().unwrap();
    thread::sleep(Duration::from_millis(250));
    for &face in &frames {
        print!("\r{}", format!("\u{258D} goodbye! {face}").cyan());
        stdout.flush().unwrap();
        thread::sleep(Duration::from_millis(300));
    }
    print!("\r{}", "\u{258D} goodbye!        ".cyan());
    println!();
}

fn format_debug_flags(flags: DebugLogFlags) -> String {
    let names = flags.display_names();
    if names.is_empty() {
        "off".to_string()
    } else {
        names.join(",")
    }
}

fn debug_help_table(active: DebugLogFlags) -> String {
    let rows = [
        ("active", format_debug_flags(active)),
        (
            "names",
            DebugLogFlags::from_names([
                "token", "cst", "ast", "ast-v", "inst", "inst-v", "wqdb", "wqdb-v", "value", "cas",
                "cas-v",
            ])
            .display_names()
            .join(","),
        ),
        ("0", "off".to_string()),
        ("1", "inst".to_string()),
        ("2", "ast,inst".to_string()),
        ("3", "ast,ast-v,inst,inst-v".to_string()),
        ("4", "token,ast,ast-v,inst,inst-v".to_string()),
    ];
    let left_w = rows.iter().map(|(l, _)| l.len()).max().unwrap_or(0);
    let right_w = rows.iter().map(|(_, r)| r.len()).max().unwrap_or(0);
    let rule = format!("+-{:-<left_w$}-+-{:-<right_w$}-+", "", "");
    let mut out = String::new();
    out.push_str(&rule);
    out.push('\n');
    out.push_str(&format!("| {:<left_w$} | {:<right_w$} |", "spec", "flags"));
    out.push('\n');
    out.push_str(&rule);
    out.push('\n');
    for (left, right) in rows {
        out.push_str(&format!("| {:<left_w$} | {:<right_w$} |", left, right));
        out.push('\n');
    }
    out.push_str(&rule);
    out
}

fn dump_builtins(builtins: &Builtins) {
    const TERM_WIDTH: usize = 80;
    const GUTTER: usize = 2;
    for (group_name, names) in builtins.list_functions_by_group() {
        let title = format!(" {} ", group_name);
        let dash_len = TERM_WIDTH.saturating_sub(title.len());
        let left = dash_len / 2;
        let right = dash_len - left;
        println!(
            "{}{}{}",
            "-".repeat(left).dimmed(),
            title.magenta(),
            "-".repeat(right).dimmed()
        );

        if names.is_empty() {
            println!();
            continue;
        }

        let max_len = names.iter().map(|s| s.len()).max().unwrap_or(0) + GUTTER;
        let columns = (TERM_WIDTH / max_len).max(1);

        if names.len() <= columns {
            // Single row: use equal spacing instead of fixed column width.
            for (i, name) in names.iter().enumerate() {
                if i > 0 {
                    print!("{:>GUTTER$}", "");
                }
                print!("{}", name);
            }
            println!();
        } else {
            for (i, name) in names.iter().enumerate() {
                print!("{name:<max_len$}");
                if (i + 1) % columns == 0 {
                    println!();
                }
            }
            if names.len() % columns != 0 {
                println!();
            }
        }
        println!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_repl_commands_accept_modifier_specs() {
        assert!(
            matches!(ReplCommand::parse("!d +ast"), ReplCommand::DebugSet(spec) if spec == "+ast")
        );
        assert!(
            matches!(ReplCommand::parse("!d-inst"), ReplCommand::DebugSet(spec) if spec == "-inst")
        );
        assert!(matches!(
            ReplCommand::parse("!debug +value"),
            ReplCommand::DebugSet(spec) if spec == "+value"
        ));
        assert!(matches!(
            ReplCommand::parse("!d.-inst"),
            ReplCommand::DebugOneshot(spec) if spec == "-inst"
        ));
    }
}
