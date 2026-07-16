#![cfg(not(target_arch = "wasm32"))]

use std::cell::RefCell;
use std::ops::{Deref, DerefMut};

use wqpl::debugger::{DebugResume, Debugger};
use wqpl::session::stdio::WqIoError;
use wqpl::session::{EvaluationFailure, Session};
use wqpl::style::{AnsiColor, ColorMode, TextStyle, paint};
use wqpl::value::Excerpt;
use wqpl::wqdb::data::{CodeLoc, DebugInfo, DebugLocalsFrame, Span};
use wqpl::wqdb::model::StepGranularity;
use wqpl::wqdb::{format_crash_frame, format_span_snippet};

use crate::repl::InteractiveOutputSpacing;
use crate::repl::input::{RustylineInput, WqInputMode};

pub(crate) mod editor;

/// Enter wqdb shell after a crash for inspection.
/// Print a short notice, then reuse the interactive shell.
pub fn enter_wqdb_after_err(
    session: &mut Session,
    failure: &EvaluationFailure,
    editor: &RustylineInput,
) {
    let Some(mut debugger) = session.postmortem_debugger(failure) else {
        return;
    };
    {
        let mut host = WqdbHost::new(&mut debugger, editor);
        host.write_line(format!(
            "{}: {}",
            wqdb_title("wqdb", host.color_mode()),
            wqdb_color("error occurred", AnsiColor::Red, host.color_mode()),
        ));
        print_crash_locals(&mut host);
        let action = wqdb_shell_inner(&mut host);
        drop(host);
        debugger.apply_resume(action);
    }
}

struct WqdbHost<'debugger, 'vm> {
    debugger: &'debugger mut Debugger<'vm>,
    editor: &'debugger RustylineInput,
    output_error: RefCell<Option<WqIoError>>,
}

impl<'debugger, 'vm> WqdbHost<'debugger, 'vm> {
    fn new(debugger: &'debugger mut Debugger<'vm>, editor: &'debugger RustylineInput) -> Self {
        Self {
            debugger,
            editor,
            output_error: RefCell::new(None),
        }
    }

    fn write_line(&self, text: impl AsRef<str>) {
        if self.output_error.borrow().is_some() {
            return;
        }
        if let Err(error) = self.debugger.write_stderr_line(text.as_ref()) {
            self.output_error.replace(Some(error));
        }
    }

    fn output_failed(&self) -> bool {
        self.output_error.borrow().is_some()
    }
}

impl<'debugger, 'vm> Deref for WqdbHost<'debugger, 'vm> {
    type Target = Debugger<'vm>;

    fn deref(&self) -> &Self::Target {
        self.debugger
    }
}

impl DerefMut for WqdbHost<'_, '_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.debugger
    }
}

macro_rules! wqdb_println {
    ($host:expr, $text:expr) => {
        $host.write_line($text)
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WqdbCommand {
    Continue,
    StepIn,
    StepOver,
    Finish,
    Granularity,
    BreakFunction,
    BreakPc,
    Breakpoints,
    ResetBreakpoints,
    Track,
    Tracks,
    Untrack,
    StopHook,
    Backtrace,
    Peek,
    Instructions,
    Locals,
    Globals,
    Help,
}

struct WqdbCommandSpec {
    command: WqdbCommand,
    aliases: &'static [&'static str],
    args: &'static [WqdbUsageArg],
    summary: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WqdbUsageArg {
    Required(&'static str),
    Optional(&'static str),
}

const WQDB_COMMANDS: &[WqdbCommandSpec] = &[
    WqdbCommandSpec {
        command: WqdbCommand::Continue,
        aliases: &["c", "continue"],
        args: &[],
        summary: "continue",
    },
    WqdbCommandSpec {
        command: WqdbCommand::StepOver,
        aliases: &["n", "next", "over"],
        args: &[],
        summary: "step over",
    },
    WqdbCommandSpec {
        command: WqdbCommand::StepIn,
        aliases: &["s", "step"],
        args: &[],
        summary: "step in",
    },
    WqdbCommandSpec {
        command: WqdbCommand::Finish,
        aliases: &["fin", "finish", "out"],
        args: &[],
        summary: "step out",
    },
    WqdbCommandSpec {
        command: WqdbCommand::Granularity,
        aliases: &["g", "gran", "granularity"],
        args: &[WqdbUsageArg::Optional("line|expr|inst")],
        summary: "show or set stepping granularity",
    },
    WqdbCommandSpec {
        command: WqdbCommand::BreakFunction,
        aliases: &["bf"],
        args: &[WqdbUsageArg::Required("func"), WqdbUsageArg::Optional("pc")],
        summary: "add breakpoint in a function",
    },
    WqdbCommandSpec {
        command: WqdbCommand::BreakPc,
        aliases: &["b"],
        args: &[WqdbUsageArg::Required("pc")],
        summary: "add breakpoint in current chunk",
    },
    WqdbCommandSpec {
        command: WqdbCommand::Breakpoints,
        aliases: &["ib"],
        args: &[],
        summary: "show breakpoints",
    },
    WqdbCommandSpec {
        command: WqdbCommand::ResetBreakpoints,
        aliases: &["rs"],
        args: &[WqdbUsageArg::Optional("id|line")],
        summary: "toggle breakpoints",
    },
    WqdbCommandSpec {
        command: WqdbCommand::Track,
        aliases: &["tr", "track"],
        args: &[
            WqdbUsageArg::Optional("scope"),
            WqdbUsageArg::Required("name"),
        ],
        summary: "track a global, local, or capture",
    },
    WqdbCommandSpec {
        command: WqdbCommand::Tracks,
        aliases: &["it", "tracks"],
        args: &[],
        summary: "show symbol trackers",
    },
    WqdbCommandSpec {
        command: WqdbCommand::Untrack,
        aliases: &["ut", "untrack"],
        args: &[WqdbUsageArg::Required("id|all")],
        summary: "remove symbol trackers",
    },
    WqdbCommandSpec {
        command: WqdbCommand::StopHook,
        aliases: &["stop-hook", "sh"],
        args: &[WqdbUsageArg::Required("action")],
        summary: "manage commands that run on each stop",
    },
    WqdbCommandSpec {
        command: WqdbCommand::Backtrace,
        aliases: &["bt"],
        args: &[],
        summary: "show backtrace",
    },
    WqdbCommandSpec {
        command: WqdbCommand::Peek,
        aliases: &["p", "peek"],
        args: &[WqdbUsageArg::Required("n")],
        summary: "peek +-n lines (def=3)",
    },
    WqdbCommandSpec {
        command: WqdbCommand::Instructions,
        aliases: &["i", "ins"],
        args: &[WqdbUsageArg::Required("n")],
        summary: "peek +-n insts (def=5)",
    },
    WqdbCommandSpec {
        command: WqdbCommand::Locals,
        aliases: &["lb", "locals"],
        args: &[],
        summary: "dump locals",
    },
    WqdbCommandSpec {
        command: WqdbCommand::Globals,
        aliases: &["gb", "globals"],
        args: &[],
        summary: "dump globals",
    },
    WqdbCommandSpec {
        command: WqdbCommand::Help,
        aliases: &["h", "help"],
        args: &[],
        summary: "show this help",
    },
];

impl WqdbCommand {
    fn parse(name: &str) -> Option<Self> {
        WQDB_COMMANDS
            .iter()
            .find(|spec| spec.aliases.contains(&name))
            .map(|spec| spec.command)
    }
}

impl WqdbUsageArg {
    fn plain(self) -> String {
        match self {
            Self::Required(name) => format!("<{name}>"),
            Self::Optional(name) => format!("[{name}]"),
        }
    }

    fn styled(self, color_mode: ColorMode) -> String {
        match self {
            Self::Required(name) => styled_required_arg(name, color_mode),
            Self::Optional(name) => styled_optional_arg(name, color_mode),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrackScope {
    Global,
    Local,
    Capture,
}

impl TrackScope {
    fn parse(name: &str) -> Option<Self> {
        match name {
            "global" | "g" => Some(Self::Global),
            "local" | "l" => Some(Self::Local),
            "capture" | "cap" => Some(Self::Capture),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StopHookCommand {
    Add,
    List,
    Delete,
    Clear,
}

impl StopHookCommand {
    fn parse(name: &str) -> Option<Self> {
        match name {
            "add" => Some(Self::Add),
            "list" | "ls" => Some(Self::List),
            "delete" | "del" | "remove" | "rm" => Some(Self::Delete),
            "clear" => Some(Self::Clear),
            _ => None,
        }
    }
}

fn command_usage_plain(spec: &WqdbCommandSpec) -> String {
    let mut usage = spec.aliases.join(" | ");
    for arg in spec.args {
        usage.push(' ');
        usage.push_str(&arg.plain());
    }
    usage
}

fn command_usage_styled(spec: &WqdbCommandSpec, color_mode: ColorMode) -> String {
    let mut usage = String::new();
    for (idx, alias) in spec.aliases.iter().enumerate() {
        if idx > 0 {
            usage.push_str(&format!(" {} ", styled_separator(color_mode)));
        }
        usage.push_str(&styled_command_with_color_mode(alias, color_mode));
    }
    for arg in spec.args {
        usage.push(' ');
        usage.push_str(&arg.styled(color_mode));
    }
    usage
}

fn styled_command_with_color_mode(text: &str, color_mode: ColorMode) -> String {
    wqdb_paint_with_color_mode(text, TextStyle::new().fg(AnsiColor::Green), color_mode)
}

fn styled_subcommand(text: &str, color_mode: ColorMode) -> String {
    wqdb_color(text, AnsiColor::BrightCyan, color_mode)
}

fn styled_flag(text: &str, color_mode: ColorMode) -> String {
    wqdb_color(text, AnsiColor::BrightMagenta, color_mode)
}

fn styled_required_arg(name: &str, color_mode: ColorMode) -> String {
    format!(
        "{}{}{}",
        wqdb_dim("<", color_mode),
        wqdb_color(name, AnsiColor::BrightYellow, color_mode),
        wqdb_dim(">", color_mode)
    )
}

fn styled_optional_arg(name: &str, color_mode: ColorMode) -> String {
    format!(
        "{}{}{}",
        wqdb_dim("[", color_mode),
        wqdb_color(name, AnsiColor::BrightYellow, color_mode),
        wqdb_dim("]", color_mode)
    )
}

fn styled_separator(color_mode: ColorMode) -> String {
    wqdb_dim("|", color_mode)
}

fn wqdb_title(text: &str, color_mode: ColorMode) -> String {
    wqdb_paint_with_color_mode(
        text,
        TextStyle::new().fg(AnsiColor::BrightMagenta).bold(),
        color_mode,
    )
}

fn wqdb_bold(text: &str, color_mode: ColorMode) -> String {
    wqdb_paint_with_color_mode(text, TextStyle::new().bold(), color_mode)
}

fn wqdb_header(text: &str, color_mode: ColorMode) -> String {
    wqdb_paint_with_color_mode(text, TextStyle::new().bold().underline(), color_mode)
}

fn wqdb_dim(text: &str, color_mode: ColorMode) -> String {
    wqdb_color(text, AnsiColor::BrightBlack, color_mode)
}

fn wqdb_color(text: &str, color: AnsiColor, color_mode: ColorMode) -> String {
    wqdb_paint_with_color_mode(text, TextStyle::new().fg(color), color_mode)
}

fn wqdb_paint_with_color_mode(text: &str, style: TextStyle, color_mode: ColorMode) -> String {
    paint(text, style, color_mode)
}

fn styled_track_command(scope: &str, arg: &str, color_mode: ColorMode) -> String {
    format!(
        "{} {} {}",
        styled_command_with_color_mode("track", color_mode),
        styled_subcommand(scope, color_mode),
        styled_required_arg(arg, color_mode)
    )
}

fn styled_stop_hook_command(action: &str, suffix: Option<String>, color_mode: ColorMode) -> String {
    match suffix {
        Some(suffix) => format!(
            "{} {} {suffix}",
            styled_command_with_color_mode("stop-hook", color_mode),
            styled_subcommand(action, color_mode)
        ),
        None => format!(
            "{} {}",
            styled_command_with_color_mode("stop-hook", color_mode),
            styled_subcommand(action, color_mode)
        ),
    }
}

fn wqdb_help_row(spec: &WqdbCommandSpec, usage_width: usize, color_mode: ColorMode) -> String {
    let usage = command_usage_styled(spec, color_mode);
    let padding = usage_width - command_usage_plain(spec).len();
    format!("  {usage}{:padding$}  {}", "", spec.summary)
}

fn print_wqdb_help(host: &WqdbHost<'_, '_>) {
    let color_mode = host.color_mode();
    let usage_width = WQDB_COMMANDS
        .iter()
        .map(|spec| command_usage_plain(spec).len())
        .max()
        .unwrap_or(0);
    wqdb_println!(
        host,
        format!(
            "{} {}",
            wqdb_title("wqdb", color_mode),
            wqdb_dim("=======================================", color_mode)
        )
    );
    for spec in WQDB_COMMANDS {
        wqdb_println!(host, wqdb_help_row(spec, usage_width, color_mode));
    }
    wqdb_println!(host, "");
    wqdb_println!(host, wqdb_bold("stepping granularity", color_mode));
    wqdb_println!(
        host,
        format!(
            "  {}  {}",
            styled_subcommand("line", color_mode),
            wqdb_dim("pause once per source line", color_mode)
        )
    );
    wqdb_println!(
        host,
        format!(
            "  {}  {}",
            styled_subcommand("expr", color_mode),
            wqdb_dim("pause at each expression (default)", color_mode)
        )
    );
    wqdb_println!(
        host,
        format!(
            "  {}  {}",
            styled_subcommand("inst", color_mode),
            wqdb_dim("pause before every VM instruction", color_mode)
        )
    );
    wqdb_println!(host, "");
    wqdb_println!(host, wqdb_bold("track scopes", color_mode));
    let track_name = format!(
        "{} {}",
        styled_command_with_color_mode("track", color_mode),
        styled_required_arg("name", color_mode)
    );
    wqdb_println!(
        host,
        format!(
            "  {} {}",
            track_name,
            wqdb_dim(
                "resolves a current local by name, or a global if no local matches",
                color_mode,
            )
        )
    );
    wqdb_println!(
        host,
        format!(
            "  {} {} {} {} {}",
            styled_track_command("global", "name", color_mode),
            styled_separator(color_mode),
            styled_track_command("local", "name", color_mode),
            styled_separator(color_mode),
            styled_track_command("capture", "slot", color_mode)
        )
    );
    wqdb_println!(host, "");
    wqdb_println!(host, wqdb_bold("stop hooks", color_mode));
    wqdb_println!(
        host,
        format!(
            "  {} {} {} {} {} {} {}",
            styled_stop_hook_command(
                "add",
                Some(format!(
                    "{} {}",
                    styled_flag("-o", color_mode),
                    styled_required_arg("cmd", color_mode)
                )),
                color_mode,
            ),
            styled_separator(color_mode),
            styled_stop_hook_command("list", None, color_mode),
            styled_separator(color_mode),
            styled_stop_hook_command(
                "delete",
                Some(styled_required_arg("id|all", color_mode)),
                color_mode,
            ),
            styled_separator(color_mode),
            styled_stop_hook_command("clear", None, color_mode)
        )
    );
    wqdb_println!(host, "");
    wqdb_println!(host, wqdb_bold("batch commands", color_mode));
    wqdb_println!(
        host,
        format!(
            "  CLI {}{}{} commands run once at the first debugger stop.",
            styled_flag("-o", color_mode),
            wqdb_dim("/", color_mode),
            styled_flag("--wqdb-cmd", color_mode)
        )
    );
    wqdb_println!(
        host,
        format!(
            "  Use {} for commands that should run every time execution stops.",
            styled_stop_hook_command(
                "add",
                Some(format!(
                    "{} {}",
                    styled_flag("-o", color_mode),
                    styled_required_arg("cmd", color_mode)
                )),
                color_mode,
            )
        )
    );
}

fn exec_single_wqdb_cmd(host: &mut WqdbHost<'_, '_>, cmd: &str) -> Option<DebugResume> {
    let mut it = cmd.split_whitespace();
    let name = it.next()?;
    let Some(command) = WqdbCommand::parse(name) else {
        wqdb_println!(
            host,
            format!("unknown wqdb command '{name}', type 'h' for help")
        );
        return None;
    };
    match command {
        WqdbCommand::Continue => Some(DebugResume::Continue),
        WqdbCommand::StepIn => Some(DebugResume::StepIn),
        WqdbCommand::StepOver => Some(DebugResume::StepOver),
        WqdbCommand::Finish => Some(DebugResume::StepOut),
        WqdbCommand::Granularity => {
            set_step_granularity(host, it.next());
            None
        }
        WqdbCommand::BreakFunction => {
            if let Err(error) = set_breakpoint_at_function(host, it.next(), it.next()) {
                wqdb_println!(host, error);
            }
            None
        }
        WqdbCommand::BreakPc => {
            if let Err(error) = set_breakpoint_at_pc(host, it.next()) {
                wqdb_println!(host, error);
            }
            None
        }
        WqdbCommand::Track => {
            if let Err(error) = track_symbol(host, it.next(), it.next()) {
                wqdb_println!(host, error);
            }
            None
        }
        WqdbCommand::Tracks => {
            print_symbol_trackers(host);
            None
        }
        WqdbCommand::Untrack => {
            if let Err(error) = untrack_symbol(host, it.next()) {
                wqdb_println!(host, error);
            }
            None
        }
        WqdbCommand::StopHook => {
            if let Err(error) = stop_hook_cmd(host, cmd) {
                wqdb_println!(host, error);
            }
            None
        }
        WqdbCommand::Breakpoints => {
            let bps = host.breakpoints();
            if bps.is_empty() {
                wqdb_println!(host, "no breakpoints");
            } else {
                wqdb_println!(
                    host,
                    format!(
                        "{:<4}  {:<3}  {:<30}  {}",
                        "id", "en", "location", "function"
                    )
                );
                wqdb_println!(
                    host,
                    format!("{:-<4}  {:-<3}  {:-<30}  {:-<20}", "", "", "", "")
                );
            }
            for (id, en, b) in bps {
                let function = host
                    .debug_info()
                    .get_chunk(b.chunk)
                    .map_or("<?>", |meta| meta.name.as_ref());
                let en_str = if en { "y" } else { "n" };
                wqdb_println!(
                    host,
                    format!(
                        "{:<4}  {:<3}  {:<30}  {}",
                        id,
                        en_str,
                        format_breakpoint_loc(host.debug_info(), b),
                        function
                    )
                );
            }
            None
        }
        WqdbCommand::Backtrace => {
            let frames = host.backtrace();
            if frames.is_empty() {
                wqdb_println!(host, "no backtrace");
            }
            for (idx, frame) in frames.iter().enumerate() {
                wqdb_println!(host, format_crash_frame(frame, idx == 0, host.color_mode()));
            }
            None
        }
        WqdbCommand::ResetBreakpoints => {
            if let Some(arg) = it.next() {
                if let Ok(id) = arg.parse::<usize>() {
                    if let Some(new_state) = host.toggle_breakpoint_by_id(id) {
                        wqdb_println!(
                            host,
                            format!(
                                "breakpoint {id} -> {}",
                                if new_state { "enabled" } else { "disabled" }
                            )
                        );
                    } else {
                        let Some(here) = host.location() else {
                            wqdb_println!(
                                host,
                                format!(
                                    "breakpoint {id} not found; current location unavailable for line lookup"
                                )
                            );
                            return None;
                        };
                        let Some(file_id) = host
                            .debug_info()
                            .get_chunk(here.chunk)
                            .map(|meta| meta.file_id)
                        else {
                            wqdb_println!(
                                host,
                                format!(
                                    "breakpoint {id} not found; current location unavailable for line lookup"
                                )
                            );
                            return None;
                        };
                        let locs = host.debug_info().resolve_line(file_id, id);
                        if locs.is_empty() {
                            wqdb_println!(
                                host,
                                format!("no statement at line {id}, nor a valid breakpoint id")
                            );
                        } else {
                            let mut enabled_count = 0;
                            let mut disabled_count = 0;
                            for l in locs {
                                if host.toggle_breakpoint_at(l) {
                                    enabled_count += 1;
                                } else {
                                    disabled_count += 1;
                                }
                            }
                            wqdb_println!(
                                host,
                                format!(
                                    "toggled {enabled_count} on, {disabled_count} off at line {id}"
                                )
                            );
                        }
                    }
                } else {
                    wqdb_println!(host, "invalid breakpoint id or line number");
                }
            } else {
                let new_state = host.toggle_all_breakpoints();
                wqdb_println!(
                    host,
                    format!(
                        "all breakpoints -> {}",
                        if new_state { "enabled" } else { "disabled" }
                    )
                );
            }
            None
        }
        WqdbCommand::Peek => {
            let n = it.next().and_then(|x| x.parse::<usize>().ok()).unwrap_or(3);
            peek_context(host, n);
            None
        }
        WqdbCommand::Instructions => {
            let n = it.next().and_then(|x| x.parse::<usize>().ok()).unwrap_or(5);
            peek_instructions(host, n);
            None
        }
        WqdbCommand::Locals => {
            print_locals(host);
            None
        }
        WqdbCommand::Globals => {
            let globals = host.globals();
            if globals.is_empty() {
                wqdb_println!(host, "no globals");
                return None;
            }
            let mut name_w = "name".len();
            let mut value_w = "value".len();
            let mut type_w = "type".len();
            for (name, v) in &globals {
                name_w = name_w.max(name.len());
                value_w = value_w.max(v.to_string().len());
                type_w = type_w.max(v.type_name().len());
            }
            wqdb_println!(
                host,
                format!(
                    "{:<name_w$}  {:<value_w$}  {:<type_w$}",
                    "name",
                    "value",
                    "type",
                    name_w = name_w,
                    value_w = value_w,
                    type_w = type_w
                )
            );
            wqdb_println!(
                host,
                format!(
                    "{:-<name_w$}  {:-<value_w$}  {:-<type_w$}",
                    "",
                    "",
                    "",
                    name_w = name_w,
                    value_w = value_w,
                    type_w = type_w
                )
            );
            for (name, v) in &globals {
                wqdb_println!(
                    host,
                    format!(
                        "{:<name_w$}  {:<value_w$}  {:<type_w$}",
                        name,
                        v.excerpt(),
                        v.type_name(),
                        name_w = name_w,
                        value_w = value_w,
                        type_w = type_w
                    )
                );
            }
            None
        }
        WqdbCommand::Help => {
            print_wqdb_help(host);
            None
        }
    }
}

fn set_step_granularity(host: &mut WqdbHost<'_, '_>, arg: Option<&str>) {
    let Some(arg) = arg else {
        wqdb_println!(
            host,
            format!("stepping granularity: {}", host.step_granularity().as_str())
        );
        return;
    };
    let Some(granularity) = StepGranularity::parse(arg) else {
        wqdb_println!(host, "usage: granularity [line|expr|inst]");
        return;
    };
    host.set_step_granularity(granularity);
    wqdb_println!(
        host,
        format!("stepping granularity -> {}", granularity.as_str())
    );
    print_stop_card(host);
    print_stop_controls(host, granularity);
}

pub fn wqdb_shell(debugger: &mut Debugger<'_>, editor: &RustylineInput) -> DebugResume {
    let mut host = WqdbHost::new(debugger, editor);
    wqdb_shell_inner(&mut host)
}

fn wqdb_shell_inner(host: &mut WqdbHost<'_, '_>) -> DebugResume {
    let cmds = host.take_batch_commands();
    if !cmds.is_empty() {
        let action = exec_wqdb_cmds(host, &cmds);
        if host.output_failed() {
            return DebugResume::Continue;
        }
        return action.unwrap_or(DebugResume::Continue);
    }

    if let Some(action) = exec_stop_hooks(host) {
        return action;
    }
    if host.output_failed() {
        return DebugResume::Continue;
    }

    let mut dbg_line = 1usize;
    let mut output_spacing = InteractiveOutputSpacing::default();
    print_stop_card(host);
    print_stop_controls(host, host.step_granularity());
    if host.output_failed() {
        return DebugResume::Continue;
    }
    output_spacing.after_output();
    loop {
        let mut function_names = host
            .debug_info()
            .function_names()
            .map(str::to_string)
            .collect::<Vec<_>>();
        function_names.sort();
        host.editor.set_wqdb_function_hints(function_names);
        if output_spacing.before_prompt() {
            wqdb_println!(host, "");
            if host.output_failed() {
                return DebugResume::Continue;
            }
        }
        #[cfg(not(target_os = "windows"))]
        let prompt =
            wqdb_prompt_with_color_mode(host.step_granularity(), dbg_line, host.color_mode());
        #[cfg(target_os = "windows")]
        let prompt =
            wqdb_prompt_with_color_mode(host.step_granularity(), dbg_line, ColorMode::Never);

        let res = host
            .editor
            .with_input_mode(WqInputMode::Wqdb, || host.editor.read_line(&prompt));
        match res {
            Ok(line) => {
                dbg_line += 1;
                let s = line.trim();
                if output_spacing.after_input(s) {
                    wqdb_println!(host, "");
                }
                if s.is_empty() {
                    continue;
                }
                if let Some(action) = exec_single_wqdb_cmd(host, s) {
                    return action;
                }
                if host.output_failed() {
                    return DebugResume::Continue;
                }
            }
            Err(WqIoError::Interrupted) => continue,
            Err(_) => {
                return DebugResume::Continue;
            }
        }
    }
}

fn exec_wqdb_cmds(host: &mut WqdbHost<'_, '_>, cmds: &[String]) -> Option<DebugResume> {
    for cmd in cmds {
        let trimmed = cmd.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(action) = exec_single_wqdb_cmd(host, trimmed) {
            return Some(action);
        }
    }
    None
}

fn exec_stop_hooks(host: &mut WqdbHost<'_, '_>) -> Option<DebugResume> {
    let cmds: Vec<String> = host
        .stop_hook_commands()
        .into_iter()
        .map(|(_, cmd)| cmd)
        .collect();
    exec_wqdb_cmds(host, &cmds)
}

fn set_breakpoint_at_pc(
    host: &mut WqdbHost<'_, '_>,
    pc_arg: Option<&str>,
) -> Result<(), &'static str> {
    let Some(pc_arg) = pc_arg else {
        return Err("usage: b <pc>");
    };
    let Ok(pc) = pc_arg.parse::<usize>() else {
        return Err("usage: b <pc>");
    };
    let Some(loc) = host.location() else {
        return Err("current location unavailable");
    };
    let (len, name) = {
        let Some(meta) = host.debug_info().get_chunk(loc.chunk) else {
            return Err("current location unavailable");
        };
        (meta.len, meta.name.to_string())
    };
    if pc >= len {
        return Err("pc out of range for current chunk");
    }
    host.set_breakpoint(CodeLoc {
        chunk: loc.chunk,
        pc,
    });
    wqdb_println!(host, format!("breakpoint set at {name} pc={pc}"));
    Ok(())
}

fn track_symbol(
    host: &mut WqdbHost<'_, '_>,
    target_arg: Option<&str>,
    name_arg: Option<&str>,
) -> Result<(), String> {
    let Some(target_arg) = target_arg else {
        return Err("usage: track [global|local|capture] <name-or-slot>".to_string());
    };
    let msg = if let Some(name_arg) = name_arg {
        match TrackScope::parse(target_arg) {
            Some(TrackScope::Global) => host.track_global_symbol(name_arg),
            Some(TrackScope::Local) => host.track_local_symbol(name_arg)?,
            Some(TrackScope::Capture) => {
                let slot = name_arg
                    .parse::<u16>()
                    .map_err(|_| "usage: track capture <slot>".to_string())?;
                host.track_capture_slot(slot)
            }
            None => return Err("usage: track [global|local|capture] <name-or-slot>".to_string()),
        }
    } else {
        host.track_symbol(target_arg)?
    };
    if let Some(msg) = msg {
        wqdb_println!(host, msg);
    }
    Ok(())
}

fn untrack_symbol(host: &mut WqdbHost<'_, '_>, arg: Option<&str>) -> Result<(), String> {
    let Some(arg) = arg else {
        return Err("usage: untrack <id|all>".to_string());
    };
    if arg == "all" {
        host.clear_symbol_trackers();
        wqdb_println!(host, "cleared symbol trackers");
        return Ok(());
    }
    let id = arg
        .parse::<usize>()
        .map_err(|_| "usage: untrack <id|all>".to_string())?;
    if host.remove_symbol_tracker(id) {
        wqdb_println!(host, format!("removed symbol tracker {id}"));
    } else {
        wqdb_println!(host, format!("symbol tracker {id} not found"));
    }
    Ok(())
}

fn print_symbol_trackers(host: &WqdbHost<'_, '_>) {
    let trackers = host.symbol_trackers();
    if trackers.is_empty() {
        wqdb_println!(host, "no symbol trackers");
        return;
    }
    wqdb_println!(host, format!("{:<4}  {:<3}  target", "id", "en"));
    wqdb_println!(host, format!("{:-<4}  {:-<3}  {:-<20}", "", "", ""));
    for (id, enabled, target) in trackers {
        wqdb_println!(
            host,
            format!(
                "{:<4}  {:<3}  {}",
                id,
                if enabled { "y" } else { "n" },
                target
            )
        );
    }
}

fn stop_hook_cmd(host: &mut WqdbHost<'_, '_>, cmd: &str) -> Result<(), String> {
    let mut it = cmd.split_whitespace();
    let _ = it.next();
    let Some(action) = it.next().and_then(StopHookCommand::parse) else {
        return Err(
            "usage: stop-hook add -o <cmd> | stop-hook list | stop-hook delete <id|all> | stop-hook clear"
                .to_string(),
        );
    };
    match action {
        StopHookCommand::Add => add_stop_hook(host, cmd),
        StopHookCommand::List => {
            print_stop_hooks(host);
            Ok(())
        }
        StopHookCommand::Delete => delete_stop_hook(host, it.next()),
        StopHookCommand::Clear => {
            host.clear_stop_hooks();
            wqdb_println!(host, "cleared stop hooks");
            Ok(())
        }
    }
}

fn add_stop_hook(host: &mut WqdbHost<'_, '_>, cmd: &str) -> Result<(), String> {
    let hook_cmd =
        command_after_option_o(cmd).ok_or_else(|| "usage: stop-hook add -o <cmd>".to_string())?;
    if hook_cmd.is_empty() {
        return Err("usage: stop-hook add -o <cmd>".to_string());
    }
    let hook = host.add_stop_hook(hook_cmd);
    wqdb_println!(host, format!("stop hook #{} added", hook.id));
    Ok(())
}

fn command_after_option_o(cmd: &str) -> Option<String> {
    let mut offset = 0;
    while let Some(pos) = cmd[offset..].find("-o") {
        let pos = offset + pos;
        let before_ok = pos == 0
            || cmd[..pos]
                .chars()
                .next_back()
                .is_some_and(char::is_whitespace);
        let after = pos + 2;
        let after_ok = cmd[after..].chars().next().is_some_and(char::is_whitespace);
        if before_ok && after_ok {
            return Some(cmd[after..].trim().to_string());
        }
        offset = after;
    }
    None
}

fn delete_stop_hook(host: &mut WqdbHost<'_, '_>, arg: Option<&str>) -> Result<(), String> {
    let Some(arg) = arg else {
        return Err("usage: stop-hook delete <id|all>".to_string());
    };
    if arg == "all" {
        host.clear_stop_hooks();
        wqdb_println!(host, "cleared stop hooks");
        return Ok(());
    }
    let id = arg
        .parse::<usize>()
        .map_err(|_| "usage: stop-hook delete <id|all>".to_string())?;
    if host.remove_stop_hook(id) {
        wqdb_println!(host, format!("removed stop hook {id}"));
    } else {
        wqdb_println!(host, format!("stop hook {id} not found"));
    }
    Ok(())
}

fn print_stop_hooks(host: &WqdbHost<'_, '_>) {
    let hooks = host.stop_hooks();
    if hooks.is_empty() {
        wqdb_println!(host, "no stop hooks");
        return;
    }
    wqdb_println!(host, format!("{:<4}  {:<3}  command", "id", "en"));
    wqdb_println!(host, format!("{:-<4}  {:-<3}  {:-<20}", "", "", ""));
    for hook in hooks {
        wqdb_println!(
            host,
            format!(
                "{:<4}  {:<3}  {}",
                hook.id,
                if hook.enabled { "y" } else { "n" },
                hook.command
            )
        );
    }
}

fn set_breakpoint_at_function(
    host: &mut WqdbHost<'_, '_>,
    name_arg: Option<&str>,
    pc_arg: Option<&str>,
) -> Result<(), String> {
    let Some(fname) = name_arg else {
        return Err("usage: bf <func_name> [pc]".to_string());
    };
    let pc_opt = match pc_arg {
        Some(arg) => Some(
            arg.parse::<usize>()
                .map_err(|_| "usage: bf <func_name> [pc]".to_string())?,
        ),
        None => None,
    };
    let Some(chunk) = host.debug_info().function_chunk(fname) else {
        return Err(format!("function '{fname}' not found"));
    };
    let Some(meta) = host.debug_info().get_chunk(chunk) else {
        return Err(format!(
            "debug information for function '{fname}' is unavailable"
        ));
    };
    let pc = if let Some(pc) = pc_opt {
        if pc >= meta.len {
            return Err(format!("pc out of range for function '{fname}'"));
        }
        pc
    } else {
        (0..meta.len)
            .find(|&p| meta.line_table.is_stmt(p))
            .unwrap_or(0)
    };
    host.set_breakpoint(CodeLoc { chunk, pc });
    wqdb_println!(host, format!("breakpoint set at {fname} pc={pc}"));
    Ok(())
}

fn format_breakpoint_loc(di: &DebugInfo, loc: CodeLoc) -> String {
    let Some(resolved) = di.resolve_location(loc) else {
        return format!("pc {} (location unavailable)", loc.pc);
    };
    if let Some(source) = resolved.source {
        return format!(
            "pc {} ({}:{}:{})",
            loc.pc, source.path, source.line, source.column
        );
    }
    format!("pc {}", loc.pc)
}

fn print_crash_locals(host: &mut WqdbHost<'_, '_>) {
    let frames = (0..host.backtrace().len())
        .filter_map(|index| host.frame_locals(index))
        .collect::<Vec<_>>();
    if frames.is_empty() {
        return;
    }
    wqdb_println!(host, "locals before crash:");
    for (idx, frame) in frames.iter().enumerate() {
        if idx > 0 {
            wqdb_println!(host, "");
        }
        print_frame_locals(host, frame, true);
    }
}

fn print_locals(host: &mut WqdbHost<'_, '_>) {
    let Some(frame) = host.frame_locals(0) else {
        wqdb_println!(host, "no locals");
        return;
    };
    print_frame_locals(host, &frame, false);
}

fn print_frame_locals(host: &WqdbHost<'_, '_>, frame: &DebugLocalsFrame, include_header: bool) {
    let di = host.debug_info();
    if include_header {
        wqdb_println!(
            host,
            format_loc_hint(di, frame.loc, Some(frame.name.as_ref()))
        );
    }
    if frame.locals.is_empty() {
        wqdb_println!(host, "no locals");
        return;
    }
    let local_names = di
        .get_chunk(frame.loc.chunk)
        .and_then(|meta| meta.local_names.as_ref());
    let mut rows: Vec<(String, String, &str)> = Vec::new();
    match local_names {
        Some(names) => {
            for (i, v) in &frame.locals {
                let name = names
                    .get(*i)
                    .cloned()
                    .unwrap_or_else(|| format!("loc[{i}]"));
                rows.push((name, v.excerpt(), v.type_name()));
            }
        }
        None => {
            for (i, v) in &frame.locals {
                rows.push((format!("loc[{i}]"), v.excerpt(), v.type_name()));
            }
        }
    }
    let mut name_w = "name".len();
    let mut value_w = "value".len();
    let mut type_w = "type".len();
    for (name, value, ty) in &rows {
        name_w = name_w.max(name.len());
        value_w = value_w.max(value.len());
        type_w = type_w.max(ty.len());
    }
    wqdb_println!(
        host,
        format!(
            "{:<name_w$}  {:<value_w$}  {:<type_w$}",
            "name",
            "value",
            "type",
            name_w = name_w,
            value_w = value_w,
            type_w = type_w
        )
    );
    wqdb_println!(
        host,
        format!(
            "{:-<name_w$}  {:-<value_w$}  {:-<type_w$}",
            "",
            "",
            "",
            name_w = name_w,
            value_w = value_w,
            type_w = type_w
        )
    );
    for (name, value, ty) in rows {
        wqdb_println!(
            host,
            format!(
                "{:<name_w$}  {:<value_w$}  {:<type_w$}",
                name,
                value,
                ty,
                name_w = name_w,
                value_w = value_w,
                type_w = type_w
            )
        );
    }
}

fn wqdb_prompt_with_color_mode(
    granularity: StepGranularity,
    line: usize,
    color_mode: ColorMode,
) -> String {
    let title = wqdb_paint_with_color_mode(
        "wqdb",
        TextStyle::new().fg(AnsiColor::BrightMagenta).bold(),
        color_mode,
    );
    let granularity = wqdb_paint_with_color_mode(
        granularity.as_str(),
        TextStyle::new().fg(AnsiColor::BrightCyan),
        color_mode,
    );
    let line = wqdb_paint_with_color_mode(
        &line.to_string(),
        TextStyle::new().fg(AnsiColor::BrightBlue),
        color_mode,
    );
    format!("{title}[{granularity}:{line}] ")
}

fn mode_header_with_color_mode(label: &str, detail: &str, color_mode: ColorMode) -> String {
    let label = wqdb_paint_with_color_mode(
        label,
        TextStyle::new().fg(AnsiColor::BrightCyan).bold(),
        color_mode,
    );
    format!("{label}  {detail}")
}

fn resolved_stop_span(di: &DebugInfo, loc: CodeLoc) -> (Span, bool) {
    let Some(meta) = di.get_chunk(loc.chunk).filter(|meta| loc.pc < meta.len) else {
        return (Span::NONE, false);
    };
    if let Some(span) = meta.line_table.exact_pc_span.get(loc.pc)
        && span.file_id != u32::MAX
    {
        return (*span, true);
    }
    if meta.line_table.is_stmt(loc.pc)
        && let Some(span) = meta.line_table.pc_to_stmt_span.get(loc.pc)
        && span.file_id != u32::MAX
    {
        return (*span, true);
    }
    let span = meta.line_table.context_span_at(loc.pc);
    if span.file_id != u32::MAX {
        return (span, false);
    }
    for &pc in &meta.line_table.stmt_pcs {
        if pc >= loc.pc {
            let span = meta.line_table.span_at(pc);
            if span.file_id != u32::MAX {
                return (span, false);
            }
        }
    }
    (Span::NONE, false)
}

fn format_line_stop_card_with_color_mode(
    di: &DebugInfo,
    loc: CodeLoc,
    name: &str,
    radius: usize,
    color_mode: ColorMode,
) -> String {
    let (span, _) = resolved_stop_span(di, loc);
    let Some(source) = di.file(span.file_id) else {
        return mode_header_with_color_mode(
            "LINE",
            &format!("pc {} in {name}\n  source unavailable", loc.pc),
            color_mode,
        );
    };
    let (line, _) = source.line_col(span.start);
    let mut out = mode_header_with_color_mode(
        "LINE",
        &format!("{}:{line} in {name}", source.path()),
        color_mode,
    );
    let total = source
        .line_count()
        .saturating_sub(usize::from(source.text().ends_with('\n')))
        .max(1);
    let first = line.saturating_sub(radius).max(1);
    let last = line.saturating_add(radius).min(total);
    for current in first..=last {
        out.push('\n');
        let source_line = source.line_text(current);
        if current == line {
            out.push_str(&wqdb_paint_with_color_mode(
                &format!("{current:>4} -> {source_line}"),
                TextStyle::new().fg(AnsiColor::Green).bold(),
                color_mode,
            ));
        } else {
            out.push_str(&format!("{current:>4}    {source_line}"));
        }
    }
    out
}

fn format_expr_stop_card_with_color_mode(
    di: &DebugInfo,
    loc: CodeLoc,
    name: &str,
    instruction: Option<&str>,
    color_mode: ColorMode,
) -> String {
    let (span, _) = resolved_stop_span(di, loc);
    let Some(source) = di.file(span.file_id) else {
        let mut out =
            mode_header_with_color_mode("EXPR", &format!("pc {} in {name}", loc.pc), color_mode);
        out.push_str("\n  source unavailable");
        if let Some(instruction) = instruction {
            out.push('\n');
            out.push_str(&wqdb_paint_with_color_mode(
                &format!("pc {}  ", loc.pc),
                TextStyle::new().fg(AnsiColor::BrightBlack),
                color_mode,
            ));
            out.push_str(&compact_instruction(instruction));
        }
        return out;
    };
    let (line, col) = source.display_line_col(span.start);
    let mut out = mode_header_with_color_mode(
        "EXPR",
        &format!("{}:{line}:{col} in {name}", source.path()),
        color_mode,
    );
    out.push('\n');
    out.push_str(
        format_span_snippet(source, span.start, span.end, color_mode).trim_end_matches('\n'),
    );
    if let Some(instruction) = instruction {
        out.push('\n');
        out.push_str(&wqdb_paint_with_color_mode(
            &format!("pc {}  ", loc.pc),
            TextStyle::new().fg(AnsiColor::BrightBlack),
            color_mode,
        ));
        out.push_str(&compact_instruction(instruction));
    }
    out
}

fn compact_instruction(instruction: &str) -> String {
    const LIMIT: usize = 120;
    let instruction = instruction.replace(['\n', '\r'], " ");
    if ansi_visible_len(&instruction) <= LIMIT {
        return instruction;
    }
    let mut compact = String::with_capacity(instruction.len().min(LIMIT * 2));
    let mut visible = 0usize;
    let mut in_escape = false;
    for ch in instruction.chars() {
        if in_escape {
            compact.push(ch);
            if ch == 'm' {
                in_escape = false;
            }
        } else if ch == '\x1b' {
            compact.push(ch);
            in_escape = true;
        } else if visible == LIMIT - 1 {
            compact.push('…');
            break;
        } else {
            compact.push(ch);
            visible += 1;
        }
    }
    if instruction.contains('\x1b') {
        compact.push_str("\x1b[0m");
    }
    compact
}

fn ansi_visible_len(text: &str) -> usize {
    let mut visible = 0usize;
    let mut in_escape = false;
    for ch in text.chars() {
        if in_escape {
            if ch == 'm' {
                in_escape = false;
            }
        } else if ch == '\x1b' {
            in_escape = true;
        } else {
            visible += 1;
        }
    }
    visible
}

fn format_inst_stop_card_with_color_mode(
    di: &DebugInfo,
    loc: CodeLoc,
    name: &str,
    instruction_len: usize,
    instructions: &[(usize, String)],
    color_mode: ColorMode,
) -> String {
    let last_pc = instruction_len.saturating_sub(1);
    let mut out = mode_header_with_color_mode(
        "INST",
        &format!("{name}  pc {}/{last_pc}", loc.pc),
        color_mode,
    );
    for (pc, instruction) in instructions {
        out.push('\n');
        let prefix = if *pc == loc.pc {
            wqdb_paint_with_color_mode(
                &format!("{pc:>4} -> "),
                TextStyle::new().fg(AnsiColor::Green).bold(),
                color_mode,
            )
        } else {
            format!("{pc:>4}    ")
        };
        out.push_str(&prefix);
        out.push_str(&compact_instruction(instruction));
    }

    let (span, is_precise) = resolved_stop_span(di, loc);
    if let Some(source) = di.file(span.file_id) {
        let (line, col) = source.display_line_col(span.start);
        out.push_str("\n\n");
        out.push_str(&mode_header_with_color_mode(
            if is_precise { "SOURCE" } else { "CONTEXT" },
            &format!("{}:{line}:{col}", source.path()),
            color_mode,
        ));
        out.push('\n');
        out.push_str(
            format_span_snippet(source, span.start, span.end, color_mode).trim_end_matches('\n'),
        );
    } else {
        out.push_str("\n\n");
        out.push_str(&mode_header_with_color_mode(
            "SOURCE",
            "unavailable",
            color_mode,
        ));
    }
    out
}

fn render_stop_card_with_color_mode(host: &WqdbHost<'_, '_>, color_mode: ColorMode) -> String {
    let granularity = host.step_granularity();
    let Some(loc) = host.location() else {
        return unavailable_stop_card_with_color_mode(granularity, color_mode);
    };
    let di = host.debug_info();
    let Some(meta) = di.get_chunk(loc.chunk).filter(|meta| loc.pc < meta.len) else {
        return unavailable_stop_card_with_color_mode(granularity, color_mode);
    };
    let name = meta.name.as_ref();
    match granularity {
        StepGranularity::Line => {
            format_line_stop_card_with_color_mode(di, loc, name, 2, color_mode)
        }
        StepGranularity::Expr => format_expr_stop_card_with_color_mode(
            di,
            loc,
            name,
            host.instruction_at_with_color_mode(loc.pc, color_mode)
                .as_deref(),
            color_mode,
        ),
        StepGranularity::Inst => {
            let start = loc.pc.saturating_sub(3);
            let end = loc.pc.saturating_add(3).min(meta.len.saturating_sub(1));
            let instructions = (start..=end)
                .filter_map(|pc| {
                    host.instruction_at_with_color_mode(pc, color_mode)
                        .map(|instruction| (pc, instruction))
                })
                .collect::<Vec<_>>();
            format_inst_stop_card_with_color_mode(
                di,
                loc,
                name,
                meta.len,
                &instructions,
                color_mode,
            )
        }
    }
}

fn unavailable_stop_card_with_color_mode(
    granularity: StepGranularity,
    color_mode: ColorMode,
) -> String {
    let label = match granularity {
        StepGranularity::Line => "LINE",
        StepGranularity::Expr => "EXPR",
        StepGranularity::Inst => "INST",
    };
    mode_header_with_color_mode(label, "current location unavailable", color_mode)
}

fn print_stop_card(host: &WqdbHost<'_, '_>) {
    wqdb_println!(
        host,
        render_stop_card_with_color_mode(host, host.color_mode())
    );
}

fn print_stop_controls(host: &WqdbHost<'_, '_>, granularity: StepGranularity) {
    wqdb_println!(
        host,
        wqdb_paint_with_color_mode(
            &format!(
                "[n] next {} [s] step in [fin] step out [c] continue [g] <line|expr|inst>",
                granularity.as_str()
            ),
            TextStyle::new().dimmed(),
            host.color_mode(),
        )
    );
}

fn format_loc_hint(di: &DebugInfo, loc: CodeLoc, name_hint: Option<&str>) -> String {
    let Some(resolved) = di.resolve_location(loc) else {
        return format!(
            "pc {} in {} (location unavailable)",
            loc.pc,
            name_hint.unwrap_or("<?>")
        );
    };
    let name = name_hint.unwrap_or(resolved.function.as_ref());
    if let Some(source) = resolved.source {
        return format!(
            "{}:{}:{} in {}",
            source.path, source.line, source.column, name
        );
    }
    format!("pc {} in {}", loc.pc, name)
}

fn peek_context(host: &mut WqdbHost<'_, '_>, n: usize) {
    let di = host.debug_info();
    let Some(loc) = host.location() else {
        wqdb_println!(host, "current location unavailable");
        return;
    };
    let Some(meta) = di.get_chunk(loc.chunk).filter(|meta| loc.pc < meta.len) else {
        wqdb_println!(host, "current location unavailable");
        return;
    };
    // Prefer a span for the next statement if current pc has no span yet
    let mut span = meta.line_table.context_span_at(loc.pc);
    if span.file_id == u32::MAX {
        for pc in loc.pc..meta.len {
            if meta.line_table.is_stmt(pc) {
                span = meta.line_table.context_span_at(pc);
                break;
            }
        }
    }
    if let Some(sf) = di.file(span.file_id) {
        let (l, _) = sf.line_col(span.start);
        // Clamp 1-based line numbers within [1, total]
        let total = sf.line_count();
        let lo_ln = if l > n { l - n } else { 1 };
        let hi_ln = if l + n <= total { l + n } else { total };
        for ln in lo_ln..=hi_ln {
            if ln == l {
                wqdb_println!(
                    host,
                    wqdb_paint_with_color_mode(
                        &format!("{:>4} -> {}", ln, sf.line_text(ln)),
                        TextStyle::new().fg(AnsiColor::Green).bold(),
                        host.color_mode(),
                    )
                );
            } else {
                wqdb_println!(host, format!("{:>4}    {}", ln, sf.line_text(ln)));
            }
        }
    } else {
        wqdb_println!(host, "no source available");
    }
}

fn peek_instructions(host: &mut WqdbHost<'_, '_>, n: usize) {
    let di = host.debug_info();
    let Some(loc) = host.location() else {
        wqdb_println!(host, "current location unavailable");
        return;
    };
    let Some(meta) = di.get_chunk(loc.chunk).filter(|meta| loc.pc < meta.len) else {
        wqdb_println!(host, "current location unavailable");
        return;
    };
    let len = meta.len;
    if len == 0 {
        wqdb_println!(host, "no instructions");
        return;
    }

    wqdb_println!(host, wqdb_header("INST", host.color_mode()));

    let start = loc.pc.saturating_sub(n);
    let end = (loc.pc + n).min(len.saturating_sub(1));
    for pc in start..=end {
        let text = host
            .instruction_at_with_color_mode(pc, host.color_mode())
            .unwrap_or_else(|| "<unavailable>".to_string());
        let prefix = if pc == loc.pc {
            wqdb_paint_with_color_mode(
                &format!("{pc:>4} -> "),
                TextStyle::new().fg(AnsiColor::Green).bold(),
                host.color_mode(),
            )
        } else {
            format!("{pc:>4}    ")
        };
        wqdb_println!(host, format!("{prefix}{text}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stop_card_debug_info() -> (DebugInfo, CodeLoc) {
        let mut di = DebugInfo::default();
        let file_id = di.new_file("demo.wq", "first\n  total:price*qty\nlast\n");
        let chunk = di.new_chunk("calc", file_id, 5);
        assert!(di.set_statement_span(
            CodeLoc { chunk, pc: 2 },
            Span {
                file_id,
                start: 6,
                end: 23,
            },
        ));
        assert!(di.set_exact_span(
            CodeLoc { chunk, pc: 2 },
            Span {
                file_id,
                start: 14,
                end: 23,
            },
        ));
        (di, CodeLoc { chunk, pc: 2 })
    }

    #[test]
    fn command_aliases_parse_to_typed_commands() {
        assert_eq!(WqdbCommand::parse("c"), Some(WqdbCommand::Continue));
        assert_eq!(WqdbCommand::parse("continue"), Some(WqdbCommand::Continue));
        assert_eq!(WqdbCommand::parse("over"), Some(WqdbCommand::StepOver));
        assert_eq!(WqdbCommand::parse("track"), Some(WqdbCommand::Track));
        assert_eq!(WqdbCommand::parse("sh"), Some(WqdbCommand::StopHook));
        assert_eq!(
            WqdbCommand::parse("granularity"),
            Some(WqdbCommand::Granularity)
        );
        assert_eq!(WqdbCommand::parse("unknown"), None);
    }

    #[test]
    fn command_aliases_are_unique() {
        let mut aliases = std::collections::HashSet::new();
        for spec in WQDB_COMMANDS {
            for alias in spec.aliases {
                assert!(aliases.insert(*alias), "duplicate wqdb alias: {alias}");
            }
        }
    }

    #[test]
    fn command_usage_renders_pipe_separated_aliases() {
        let continue_spec = WQDB_COMMANDS
            .iter()
            .find(|spec| spec.command == WqdbCommand::Continue)
            .expect("continue command spec");
        let break_fn_spec = WQDB_COMMANDS
            .iter()
            .find(|spec| spec.command == WqdbCommand::BreakFunction)
            .expect("break function command spec");
        let granularity_spec = WQDB_COMMANDS
            .iter()
            .find(|spec| spec.command == WqdbCommand::Granularity)
            .expect("granularity command spec");

        assert_eq!(command_usage_plain(continue_spec), "c | continue");
        assert_eq!(command_usage_plain(break_fn_spec), "bf <func> [pc]");
        assert_eq!(
            command_usage_plain(granularity_spec),
            "g | gran | granularity [line|expr|inst]"
        );
    }

    #[test]
    fn command_styles_use_explicit_style_renderer() {
        assert_eq!(
            styled_command_with_color_mode("continue", ColorMode::Always),
            "\x1b[32mcontinue\x1b[0m"
        );
        assert_eq!(
            wqdb_paint_with_color_mode(
                "INST",
                TextStyle::new().bold().underline(),
                ColorMode::Never,
            ),
            "INST"
        );
    }

    #[test]
    fn command_help_rows_are_indented() {
        let usage_width = WQDB_COMMANDS
            .iter()
            .map(|spec| command_usage_plain(spec).len())
            .max()
            .expect("wqdb commands");
        let row = wqdb_help_row(&WQDB_COMMANDS[0], usage_width, ColorMode::Never);

        assert!(row.starts_with("  "));
    }

    #[test]
    fn prompt_keeps_the_active_granularity_visible() {
        assert_eq!(
            wqdb_prompt_with_color_mode(StepGranularity::Expr, 3, ColorMode::Never),
            "wqdb[expr:3] "
        );
    }

    #[test]
    fn unavailable_stop_cards_name_the_active_mode() {
        assert_eq!(
            unavailable_stop_card_with_color_mode(StepGranularity::Line, ColorMode::Never),
            "LINE  current location unavailable"
        );
        assert_eq!(
            unavailable_stop_card_with_color_mode(StepGranularity::Expr, ColorMode::Never),
            "EXPR  current location unavailable"
        );
        assert_eq!(
            unavailable_stop_card_with_color_mode(StepGranularity::Inst, ColorMode::Never),
            "INST  current location unavailable"
        );
    }

    #[test]
    fn stale_locations_render_without_panicking() {
        let di = DebugInfo::default();
        let loc = CodeLoc {
            chunk: wqpl::wqdb::data::ChunkId(u32::MAX),
            pc: 7,
        };

        assert_eq!(
            format_breakpoint_loc(&di, loc),
            "pc 7 (location unavailable)"
        );
        assert_eq!(
            format_loc_hint(&di, loc, Some("calc")),
            "pc 7 in calc (location unavailable)"
        );
        assert_eq!(resolved_stop_span(&di, loc), (Span::NONE, false));
    }

    #[test]
    fn line_stop_card_is_source_first_and_preserves_indentation() {
        let (di, loc) = stop_card_debug_info();

        let rendered = format_line_stop_card_with_color_mode(&di, loc, "calc", 1, ColorMode::Never);

        assert_eq!(
            rendered,
            "LINE  demo.wq:2 in calc\n   1    first\n   2 ->   total:price*qty\n   3    last"
        );
    }

    #[test]
    fn expression_stop_card_focuses_the_exact_span() {
        let (di, loc) = stop_card_debug_info();

        let rendered = format_expr_stop_card_with_color_mode(
            &di,
            loc,
            "calc",
            Some("BinaryOp(Multiply)"),
            ColorMode::Never,
        );

        assert_eq!(
            rendered,
            "EXPR  demo.wq:2:9 in calc\n  2 ->   total:price*qty\n               ~~~~~~~~~\npc 2  BinaryOp(Multiply)"
        );
    }

    #[test]
    fn expression_stop_card_preserves_pretty_instruction_color() {
        let (di, loc) = stop_card_debug_info();
        let instruction = "\x1b[35mBinaryOp\x1b[0m(Multiply)";

        let rendered = format_expr_stop_card_with_color_mode(
            &di,
            loc,
            "calc",
            Some(instruction),
            ColorMode::Always,
        );

        assert!(
            rendered.ends_with(&format!("\x1b[90mpc 2  \x1b[0m{instruction}")),
            "card was: {rendered:?}"
        );
    }

    #[test]
    fn instruction_stop_card_leads_with_disassembly_then_source() {
        let (di, loc) = stop_card_debug_info();
        let instructions = vec![
            (0, "LoadLocal(0)".to_string()),
            (1, "LoadLocal(1)".to_string()),
            (2, "BinaryOp(Multiply)".to_string()),
            (3, "StoreLocal(2)".to_string()),
        ];

        let rendered = format_inst_stop_card_with_color_mode(
            &di,
            loc,
            "calc",
            5,
            &instructions,
            ColorMode::Never,
        );

        assert_eq!(
            rendered,
            "INST  calc  pc 2/4\n   0    LoadLocal(0)\n   1    LoadLocal(1)\n   2 -> BinaryOp(Multiply)\n   3    StoreLocal(2)\n\nSOURCE  demo.wq:2:9\n  2 ->   total:price*qty\n               ~~~~~~~~~"
        );
    }

    #[test]
    fn instruction_stop_card_colors_prefix_without_overriding_opcode() {
        let (di, loc) = stop_card_debug_info();
        let instruction = "\x1b[35mBinaryOp\x1b[0m(Multiply)";

        let rendered = format_inst_stop_card_with_color_mode(
            &di,
            loc,
            "calc",
            5,
            &[(2, instruction.to_string())],
            ColorMode::Always,
        );

        assert!(
            rendered.contains(&format!("\x1b[1;32m   2 -> \x1b[0m{instruction}")),
            "card was: {rendered:?}"
        );
    }

    #[test]
    fn compact_instruction_preserves_complete_ansi_sequences() {
        let instruction = format!("\x1b[31mLoadConst\x1b[0m({})", "x".repeat(140));

        let compact = compact_instruction(&instruction);

        assert!(compact.starts_with("\x1b[31mLoadConst\x1b[0m("));
        assert!(compact.ends_with("…\x1b[0m"));
        assert_eq!(ansi_visible_len(&compact), 120);
    }

    #[test]
    fn expression_stop_card_clamps_a_multiline_span() {
        let mut di = DebugInfo::default();
        let file_id = di.new_file("demo.wq", "x:(1;\n  2)\n");
        let chunk = di.new_chunk("calc", file_id, 1);
        assert!(di.set_exact_span(
            CodeLoc { chunk, pc: 0 },
            Span {
                file_id,
                start: 2,
                end: 10,
            },
        ));

        let rendered = format_expr_stop_card_with_color_mode(
            &di,
            CodeLoc { chunk, pc: 0 },
            "calc",
            Some("MakeList(2)"),
            ColorMode::Never,
        );

        assert_eq!(
            rendered,
            "EXPR  demo.wq:1:3 in calc\n  1 -> x:(1;\n         ~~~\npc 0  MakeList(2)"
        );
    }

    #[test]
    fn stop_cards_keep_instruction_context_when_source_is_unavailable() {
        let mut di = DebugInfo::default();
        let file_id = di.new_file("demo.wq", "");
        let chunk = di.new_chunk("calc", file_id, 1);
        let loc = CodeLoc { chunk, pc: 0 };

        let expr = format_expr_stop_card_with_color_mode(
            &di,
            loc,
            "calc",
            Some("Return"),
            ColorMode::Never,
        );
        let inst = format_inst_stop_card_with_color_mode(
            &di,
            loc,
            "calc",
            1,
            &[(0, "Return".to_string())],
            ColorMode::Never,
        );

        assert_eq!(
            expr,
            "EXPR  pc 0 in calc\n  source unavailable\npc 0  Return"
        );
        assert!(
            inst.ends_with("\n\nSOURCE  unavailable"),
            "card was: {inst}"
        );
    }

    #[test]
    fn expression_stop_card_reports_display_columns() {
        let mut di = DebugInfo::default();
        let file_id = di.new_file("demo.wq", "α:1\n");
        let chunk = di.new_chunk("calc", file_id, 1);
        assert!(di.set_exact_span(
            CodeLoc { chunk, pc: 0 },
            Span {
                file_id,
                start: 3,
                end: 4,
            },
        ));

        let rendered = format_expr_stop_card_with_color_mode(
            &di,
            CodeLoc { chunk, pc: 0 },
            "calc",
            None,
            ColorMode::Never,
        );

        assert!(
            rendered.starts_with("EXPR  demo.wq:1:3 in calc"),
            "card was: {rendered}"
        );
    }

    #[test]
    fn line_stop_card_omits_a_phantom_line_after_final_newline() {
        let mut di = DebugInfo::default();
        let file_id = di.new_file("demo.wq", "a:1\nb:2\n");
        let chunk = di.new_chunk("calc", file_id, 1);
        assert!(di.set_statement_span(
            CodeLoc { chunk, pc: 0 },
            Span {
                file_id,
                start: 4,
                end: 7,
            },
        ));

        let rendered = format_line_stop_card_with_color_mode(
            &di,
            CodeLoc { chunk, pc: 0 },
            "calc",
            2,
            ColorMode::Never,
        );

        assert!(!rendered.contains("\n   3"), "card was: {rendered}");
    }

    #[test]
    fn track_scope_aliases_parse_to_typed_scopes() {
        assert_eq!(TrackScope::parse("global"), Some(TrackScope::Global));
        assert_eq!(TrackScope::parse("g"), Some(TrackScope::Global));
        assert_eq!(TrackScope::parse("local"), Some(TrackScope::Local));
        assert_eq!(TrackScope::parse("l"), Some(TrackScope::Local));
        assert_eq!(TrackScope::parse("capture"), Some(TrackScope::Capture));
        assert_eq!(TrackScope::parse("cap"), Some(TrackScope::Capture));
        assert_eq!(TrackScope::parse("x"), None);
    }

    #[test]
    fn stop_hook_aliases_parse_to_typed_commands() {
        assert_eq!(StopHookCommand::parse("add"), Some(StopHookCommand::Add));
        assert_eq!(StopHookCommand::parse("list"), Some(StopHookCommand::List));
        assert_eq!(StopHookCommand::parse("ls"), Some(StopHookCommand::List));
        assert_eq!(
            StopHookCommand::parse("delete"),
            Some(StopHookCommand::Delete)
        );
        assert_eq!(StopHookCommand::parse("rm"), Some(StopHookCommand::Delete));
        assert_eq!(
            StopHookCommand::parse("clear"),
            Some(StopHookCommand::Clear)
        );
        assert_eq!(StopHookCommand::parse("x"), None);
    }
}
