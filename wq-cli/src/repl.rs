mod command;
pub mod editor;
pub mod input;

use std::cell::RefCell;
use std::collections::HashSet;
use std::io::Write as _;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use std::{env, thread};

use rand::RngExt as _;
use terminal_size::{Width, terminal_size};
use wqpl::builtins::{BuiltinPreset, Builtins};
use wqpl::format::{FormatConfig, Formatter};
use wqpl::interpret::InterpreterKind;
use wqpl::session::Session;
use wqpl::session::dbglog::{DebugLogFlags, get_debug_log_flags, set_debug_log_flags};
use wqpl::session::stdio::{
    WqStdinError, set_wqstdin, wqstdin_add_history, wqstdin_highlight_enabled,
    wqstdin_hints_enabled, wqstdin_readline, wqstdin_set_builtin_hints, wqstdin_set_global_hints,
    wqstdin_set_highlight, wqstdin_set_hints_enabled, wqstdin_set_repl_hints,
};
use wqpl::style::{AnsiColor, ColorMode, TextStyle, paint};
use wqpl::value::{Excerpt, Value};
use wqpl::{completion as wq_completion, doc};

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

fn repl_color(text: &str, color: AnsiColor) -> String {
    repl_paint(text, TextStyle::new().fg(color))
}

fn repl_dim(text: &str) -> String {
    repl_paint(text, TextStyle::new().dimmed())
}

fn repl_underline(text: &str) -> String {
    repl_paint(text, TextStyle::new().underline())
}

fn repl_status(on: bool) -> String {
    repl_underline(if on { "on" } else { "off" })
}

fn repl_bold_underline(text: &str) -> String {
    repl_paint(text, TextStyle::new().bold().underline())
}

fn repl_paint(text: &str, style: TextStyle) -> String {
    repl_paint_with_color_mode(text, style, ColorMode::Auto)
}

fn repl_paint_with_color_mode(text: &str, style: TextStyle, color_mode: ColorMode) -> String {
    paint(text, style, color_mode)
}

fn repl_rgb(text: &str, red: u8, green: u8, blue: u8) -> String {
    repl_rgb_with_color_mode(text, red, green, blue, ColorMode::Auto)
}

fn repl_rgb_with_color_mode(
    text: &str,
    red: u8,
    green: u8,
    blue: u8,
    color_mode: ColorMode,
) -> String {
    if color_mode.should_colorize() {
        format!("\x1b[38;2;{red};{green};{blue}m{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

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
        match command::parse(input) {
            command::ParsedReplCommand::Empty => Self::Empty,
            command::ParsedReplCommand::Unknown | command::ParsedReplCommand::Directive => {
                Self::Unknown
            }
            command::ParsedReplCommand::Handled { kind, arg } => Self::from_kind(kind, arg),
        }
    }

    fn from_kind(kind: command::ReplCommandKind, arg: Option<String>) -> Self {
        match kind {
            command::ReplCommandKind::Exit => Self::Exit,
            command::ReplCommandKind::Bye => Self::Bye,
            command::ReplCommandKind::Goodbye => Self::Goodbye,
            command::ReplCommandKind::Highlight => Self::Highlight,
            command::ReplCommandKind::Hint => Self::Hint,
            command::ReplCommandKind::Info => Self::Info,
            command::ReplCommandKind::Dry => Self::Dry,
            command::ReplCommandKind::Fmt => Self::Fmt(arg),
            command::ReplCommandKind::Bfn => Self::Bfn(arg),
            command::ReplCommandKind::Gb => Self::Gb,
            command::ReplCommandKind::Reset => Self::Reset,
            command::ReplCommandKind::Box => Self::Box,
            command::ReplCommandKind::BoxSet => Self::BoxSet(arg.unwrap_or_default()),
            command::ReplCommandKind::Backtrace => Self::Backtrace,
            command::ReplCommandKind::Xray => Self::Xray,
            command::ReplCommandKind::Interpreter => Self::Interpreter(arg),
            command::ReplCommandKind::Time => Self::Time,
            command::ReplCommandKind::TimeOneshot => Self::TimeOneshot,
            command::ReplCommandKind::Wqdb => Self::Wqdb,
            command::ReplCommandKind::WqdbOneshot => Self::WqdbOneshot,
            command::ReplCommandKind::Help => Self::Help(arg),
            command::ReplCommandKind::DebugShow => Self::DebugShow,
            command::ReplCommandKind::DebugToggle => Self::DebugToggle,
            command::ReplCommandKind::DebugOneshot => Self::DebugOneshot(arg.unwrap_or_default()),
            command::ReplCommandKind::DebugSet => Self::DebugSet(arg.unwrap_or_default()),
            command::ReplCommandKind::DryQuery => Self::DryQuery,
            command::ReplCommandKind::BoxQuery => Self::BoxQuery,
            command::ReplCommandKind::BacktraceQuery => Self::BacktraceQuery,
            command::ReplCommandKind::XrayQuery => Self::XrayQuery,
            command::ReplCommandKind::HighlightQuery => Self::HighlightQuery,
            command::ReplCommandKind::HintQuery => Self::HintQuery,
            command::ReplCommandKind::TimeQuery => Self::TimeQuery,
            command::ReplCommandKind::WqdbQuery => Self::WqdbQuery,
            command::ReplCommandKind::FmtQuery => Self::FmtQuery,
            command::ReplCommandKind::TypeShow => Self::TypeShow,
            command::ReplCommandKind::TypeQuery => Self::TypeQuery,
        }
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
            format!(
                "{}[{}] ",
                repl_color("wq", AnsiColor::Magenta),
                repl_color(&line_number.to_string(), AnsiColor::Blue)
            )
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
                        system_msg_out(format!("hint -> {}", repl_status(on)), MsgType::Info);
                        continue;
                    }
                    ReplCommand::Info => {
                        print_repl_startup(&session, rtflags.stack_size_mebibyte);
                        continue;
                    }
                    ReplCommand::Dry => {
                        dry_mode = !dry_mode;
                        session.set_dry_mode(dry_mode);
                        system_msg_out(format!("dry -> {}", repl_status(dry_mode)), MsgType::Info);
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
                                    repl_bold_underline(current.name())
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
                            format!("box -> {}", repl_underline(&box_config.summary())),
                            MsgType::Info,
                        );
                        continue;
                    }
                    ReplCommand::BoxSet(spec) => {
                        match apply_box_spec(&mut box_config, &spec) {
                            Ok(()) => {
                                system_msg_out(
                                    format!("box -> {}", repl_underline(&box_config.summary())),
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
                            format!("backtrace -> {}", repl_status(bt_mode)),
                            MsgType::Info,
                        );
                        continue;
                    }
                    ReplCommand::Xray => {
                        box_config.toggle_xray();
                        system_msg_out(
                            format!("box -> {}", repl_underline(&box_config.summary())),
                            MsgType::Info,
                        );
                        continue;
                    }
                    ReplCommand::Interpreter(None) => {
                        let current = session.interpreter_name();
                        let list = format!(
                            "Current: {}\nAvailable: {}",
                            repl_underline(current),
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
                            format!("time -> {}", repl_status(time_mode)),
                            MsgType::Info,
                        );
                        continue;
                    }
                    ReplCommand::TimeOneshot => {
                        oneshot_time = true;
                        system_msg_out(
                            format!("time -> {}", repl_underline("on for next eval")),
                            MsgType::Info,
                        );
                        continue;
                    }
                    ReplCommand::Wqdb => {
                        session.set_wqdb(!session.is_wqdb_enabled());
                        system_msg_out(
                            format!("wqdb -> {}", repl_status(session.is_wqdb_enabled())),
                            MsgType::Info,
                        );
                        continue;
                    }
                    ReplCommand::WqdbOneshot => {
                        oneshot_wqdb = true;
                        system_msg_out(
                            format!("wqdb -> {}", repl_underline("on for next eval")),
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
                            println!("{}", repl_dim(&top));
                            for line in lines {
                                println!("│  {}  │", pad_vis(line.to_string(), width));
                            }
                            println!("{}", repl_dim(&bot));
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
                            format!(
                                "debug flags -> {}",
                                repl_underline(&format_debug_flags(next))
                            ),
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
                                        repl_underline(&format_debug_flags(flags))
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
                                        repl_underline(&format_debug_flags(flags))
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
                        system_msg_out(format!("dry: {}", repl_status(dry_mode)), MsgType::Info);
                        continue;
                    }
                    ReplCommand::BoxQuery => {
                        system_msg_out(
                            format!("box: {}", repl_underline(&box_config.summary())),
                            MsgType::Info,
                        );
                        continue;
                    }
                    ReplCommand::BacktraceQuery => {
                        let bt_mode = session.get_bt_mode();
                        system_msg_out(
                            format!("backtrace: {}", repl_status(bt_mode)),
                            MsgType::Info,
                        );
                        continue;
                    }
                    ReplCommand::XrayQuery => {
                        system_msg_out(
                            format!("xray: {}", repl_status(box_config.shows_xray())),
                            MsgType::Info,
                        );
                        continue;
                    }
                    ReplCommand::HighlightQuery => {
                        let on = wqstdin_highlight_enabled();
                        system_msg_out(format!("highlight: {}", repl_status(on)), MsgType::Info);
                        continue;
                    }
                    ReplCommand::HintQuery => {
                        let on = wqstdin_hints_enabled();
                        system_msg_out(format!("hint: {}", repl_status(on)), MsgType::Info);
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
                        system_msg_out(format!("time: {}", repl_underline(status)), MsgType::Info);
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
                        system_msg_out(format!("wqdb: {}", repl_underline(status)), MsgType::Info);
                        continue;
                    }
                    ReplCommand::FmtQuery => {
                        system_msg_out(format!("fmt: {}", fmt_state.summary()), MsgType::Info);
                        continue;
                    }
                    ReplCommand::TypeShow => {
                        show_type = !show_type;
                        system_msg_out(
                            format!("type -> {}", repl_status(show_type)),
                            MsgType::Info,
                        );
                        continue;
                    }
                    ReplCommand::TypeQuery => {
                        system_msg_out(format!("type: {}", repl_status(show_type)), MsgType::Info);
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
                if t.starts_with("\\") {
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
    let (names, descs) = command::repl_hint_vectors();
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

    lines.push(repl_dim(&top));
    let title = format!(
        "{}         {}",
        repl_color(&format!("wq {WQ_VERSION}"), AnsiColor::Magenta),
        repl_dim("(c) tttiw  (l) MIT")
    );
    lines.push(format!("│  {}  │", pad_vis(title, INNER)));
    let hints = format!(
        "{}  {}",
        repl_color(r"\help", AnsiColor::Green),
        repl_color(r"\exit", AnsiColor::Green)
    );
    lines.push(format!("│  {}  │", pad_vis(hints, INNER)));
    lines.push(repl_dim(&sep));

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
        pad_label(repl_color("host", AnsiColor::Blue), 4),
        repl_dim(RUSTC_HOST)
    );
    host_line.push_str(&" ".repeat(SECOND_COL.saturating_sub(vis_width(&host_line))));
    host_line.push_str(&format!(
        "{}  {}",
        pad_label(repl_color("avpa", AnsiColor::Blue), 5),
        repl_dim(&avpa)
    ));
    lines.push(format!("│  {}  │", pad_vis(host_line, INNER)));

    let mut pid_line = format!(
        "{}  {}",
        pad_label(repl_color("pid", AnsiColor::Blue), 4),
        repl_dim(&pid)
    );
    pid_line.push_str(&" ".repeat(SECOND_COL.saturating_sub(vis_width(&pid_line))));
    pid_line.push_str(&format!(
        "{}  {}",
        pad_label(repl_color("stack", AnsiColor::Blue), 5),
        repl_dim(&stack_size.to_string())
    ));
    lines.push(format!("│  {}  │", pad_vis(pid_line, INNER)));

    let cwd_prefix = format!("{}  ", pad_label(repl_color("cwd", AnsiColor::Blue), 4));
    let cwd_prefix_vis = vis_width(&cwd_prefix);
    let cwd_avail = INNER.saturating_sub(cwd_prefix_vis).max(1);
    let cwd_lines = wrap_text(&cwd, cwd_avail);
    for (i, chunk) in cwd_lines.iter().enumerate() {
        let content = if i == 0 {
            format!("{}{}", cwd_prefix, repl_dim(chunk))
        } else {
            format!("{}{}", " ".repeat(cwd_prefix_vis), repl_dim(chunk))
        };
        lines.push(format!("│  {}  │", pad_vis(content, INNER)));
    }

    let mut profile_line = format!(
        "{}  {}  o{}",
        repl_color("profile", AnsiColor::Red),
        repl_dim(BUILD_PROFILE),
        BUILD_OPT_LEVEL
    );
    profile_line.push_str(&" ".repeat(SECOND_COL.saturating_sub(vis_width(&profile_line))));
    profile_line.push_str(&format!(
        "{}  {}",
        pad_label(repl_color("llvm", AnsiColor::Red), 5),
        repl_dim(RUSTC_LLVM_VERSION)
    ));
    lines.push(format!("│  {}  │", pad_vis(profile_line, INNER)));

    lines.push(format!(
        "│  {}  │",
        pad_vis(
            format!(
                "{}  {}",
                repl_color("rustc", AnsiColor::Red),
                repl_dim(rustc_ver_short)
            ),
            INNER
        )
    ));

    let mut interp_line = format!(
        "{}  {}",
        repl_color("interpreter", AnsiColor::BrightYellow),
        repl_dim(evaluator.interpreter_name())
    );
    interp_line.push_str(&" ".repeat(SECOND_COL.saturating_sub(vis_width(&interp_line))));
    interp_line.push_str(&format!(
        "{}  {}",
        pad_label(repl_color("bfn", AnsiColor::BrightYellow), 5),
        repl_dim(evaluator.builtins_preset().name())
    ));
    lines.push(format!("│  {}  │", pad_vis(interp_line, INNER)));

    lines.push(repl_dim(&bot));

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
        .flat_map(|&ch| palette.iter().map(|(r, g, b)| repl_rgb(ch, *r, *g, *b)))
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
        .flat_map(|&ch| palette.iter().map(|(r, g, b)| repl_rgb(ch, *r, *g, *b)))
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
        out.push_str(&repl_dim(type_str));
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
        resstr.push_str(&repl_dim(type_str));
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
                    self.opts.wrap_only = false;
                    self.opts.nlcd = !self.opts.nlcd;
                    saw_mode_toggle = true;
                }
                "olw" => {
                    self.opts.wrap_only = false;
                    self.opts.olw = !self.opts.olw;
                    saw_mode_toggle = true;
                }
                "wrap" | "wrap-only" => {
                    self.opts.wrap_only = !self.opts.wrap_only;
                    if self.opts.wrap_only {
                        self.opts.nlcd = false;
                        self.opts.olw = false;
                    }
                    saw_mode_toggle = true;
                }
                "full" | "nowrap" | "no-wrap" => {
                    self.opts.wrap_only = false;
                    saw_mode_toggle = true;
                }
                other => {
                    if let Some(width) = parse_wrap_only_width_mode(other)? {
                        self.opts.max_width = Some(width);
                        self.opts.wrap_only = true;
                        self.opts.nlcd = false;
                        self.opts.olw = false;
                        saw_mode_toggle = true;
                    } else if let Some(width_mode) = parse_width_mode(other)? {
                        match width_mode {
                            WidthMode::Set(width) => self.opts.max_width = Some(width),
                            WidthMode::Clear => self.opts.max_width = None,
                        }
                        saw_mode_toggle = true;
                    } else {
                        return Err(format!(
                            "unknown fmt mode '{other}'\nAvailable: on, off, nlcd, olw, wrap-only, width=COLS, nowrap"
                        ));
                    }
                }
            }
        }
        if saw_mode_toggle {
            self.enabled = true;
        }
        Ok(())
    }

    fn config(&self) -> FormatConfig {
        let mut config = FormatConfig {
            indent_size: 2,
            nlcd: self.opts.nlcd,
            one_line_wizard: self.opts.olw,
            ..FormatConfig::default()
        };
        if let Some(width) = self.opts.max_width {
            config.max_width = width;
        }
        config.wrap_only = self.opts.wrap_only;
        config
    }

    fn modes(&self) -> String {
        let mut modes = Vec::new();
        if self.opts.wrap_only {
            modes.push("wrap-only".to_string());
        }
        if let Some(width) = self.opts.max_width {
            modes.push(format!("width={width}"));
        }
        if self.opts.nlcd {
            modes.push("nlcd".to_string());
        }
        if self.opts.olw {
            modes.push("olw".to_string());
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WidthMode {
    Set(usize),
    Clear,
}

fn parse_wrap_only_width_mode(mode: &str) -> Result<Option<usize>, String> {
    let width = mode
        .strip_prefix("wrap=")
        .or_else(|| mode.strip_prefix("wrap:"))
        .or_else(|| mode.strip_prefix("wrap "));
    let Some(width) = width else {
        return Ok(None);
    };
    parse_fmt_width(width.trim()).map(Some)
}

fn parse_width_mode(mode: &str) -> Result<Option<WidthMode>, String> {
    let width = mode
        .strip_prefix("width=")
        .or_else(|| mode.strip_prefix("width:"))
        .or_else(|| mode.strip_prefix("width "));
    let Some(width) = width else {
        return Ok(None);
    };
    let width = width.trim();
    if matches!(width, "default" | "off" | "none") {
        return Ok(Some(WidthMode::Clear));
    }
    parse_fmt_width(width).map(WidthMode::Set).map(Some)
}

fn parse_fmt_width(width: &str) -> Result<usize, String> {
    match width.parse::<usize>() {
        Ok(n) if n > 0 => Ok(n),
        Ok(_) => Err("fmt wrap width must be at least 1".to_string()),
        Err(_) => Err(format!("invalid fmt wrap width: {width}")),
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
    print!("{}", repl_color("\u{258D} goodbye! ", AnsiColor::Cyan));
    stdout.flush().unwrap();
    thread::sleep(Duration::from_millis(250));
    for &face in &frames {
        print!(
            "\r{}",
            repl_color(&format!("\u{258D} goodbye! {face}"), AnsiColor::Cyan)
        );
        stdout.flush().unwrap();
        thread::sleep(Duration::from_millis(300));
    }
    print!(
        "\r{}",
        repl_color("\u{258D} goodbye!        ", AnsiColor::Cyan)
    );
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
            repl_dim(&"-".repeat(left)),
            repl_color(&title, AnsiColor::Magenta),
            repl_dim(&"-".repeat(right))
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
    fn repl_styles_use_explicit_renderer() {
        assert_eq!(
            repl_paint_with_color_mode(
                "wq",
                TextStyle::new().fg(AnsiColor::Magenta),
                ColorMode::Always,
            ),
            "\x1b[35mwq\x1b[0m"
        );
        assert_eq!(
            repl_paint_with_color_mode("on", TextStyle::new().underline(), ColorMode::Never),
            "on"
        );
        assert_eq!(
            repl_rgb_with_color_mode("*", 1, 2, 3, ColorMode::Always),
            "\x1b[38;2;1;2;3m*\x1b[0m"
        );
    }

    #[test]
    fn debug_repl_commands_accept_modifier_specs() {
        assert!(
            matches!(ReplCommand::parse(r"\d +ast"), ReplCommand::DebugSet(spec) if spec == "+ast")
        );
        assert!(
            matches!(ReplCommand::parse(r"\d-inst"), ReplCommand::DebugSet(spec) if spec == "-inst")
        );
        assert!(matches!(
            ReplCommand::parse(r"\debug +value"),
            ReplCommand::DebugSet(spec) if spec == "+value"
        ));
        assert!(matches!(
            ReplCommand::parse(r"\d.-inst"),
            ReplCommand::DebugOneshot(spec) if spec == "-inst"
        ));
        assert!(matches!(
            ReplCommand::parse(r"\d."),
            ReplCommand::DebugOneshot(spec) if spec.is_empty()
        ));
    }

    #[test]
    fn fmt_repl_wrap_only_mode_sets_width() {
        let mut state = ReplFmtState::default();
        state.toggle_modes("width=32").expect("width mode parses");
        state.toggle_modes("wrap-only").expect("wrap mode parses");

        assert!(state.enabled);
        assert_eq!(state.opts.max_width, Some(32));
        assert!(state.opts.wrap_only);
        assert_eq!(state.modes(), "wrap-only,width=32");
        let config = state.config();
        assert!(config.wrap_only);
        assert_eq!(config.max_width, 32);
    }

    #[test]
    fn fmt_repl_normal_modes_clear_wrap_only() {
        let mut state = ReplFmtState::default();
        state.toggle_modes("wrap 32").expect("wrap mode parses");
        state.toggle_modes("nlcd").expect("nlcd mode parses");

        assert_eq!(state.opts.max_width, Some(32));
        assert!(!state.opts.wrap_only);
        assert!(state.opts.nlcd);
        assert!(!state.config().wrap_only);
    }

    #[test]
    fn fmt_repl_width_without_wrap_only_uses_normal_formatter() {
        let mut state = ReplFmtState::default();
        state.toggle_modes("width=44").expect("width mode parses");

        assert!(state.enabled);
        assert_eq!(state.opts.max_width, Some(44));
        assert!(!state.opts.wrap_only);
        assert_eq!(state.modes(), "width=44");
        let config = state.config();
        assert_eq!(config.max_width, 44);
        assert!(!config.wrap_only);
    }
}
