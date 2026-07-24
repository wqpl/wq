use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Command {
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

pub(super) struct CommandSpec {
    pub(super) command: Command,
    pub(super) aliases: &'static [&'static str],
    pub(super) args: &'static [UsageArg],
    pub(super) summary: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum UsageArg {
    Required(&'static str),
    Optional(&'static str),
}

pub(super) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        command: Command::Continue,
        aliases: &["c", "continue"],
        args: &[],
        summary: "continue",
    },
    CommandSpec {
        command: Command::StepOver,
        aliases: &["n", "next", "over"],
        args: &[],
        summary: "step over",
    },
    CommandSpec {
        command: Command::StepIn,
        aliases: &["s", "step"],
        args: &[],
        summary: "step in",
    },
    CommandSpec {
        command: Command::Finish,
        aliases: &["fin", "finish", "out"],
        args: &[],
        summary: "step out",
    },
    CommandSpec {
        command: Command::Granularity,
        aliases: &["g", "gran", "granularity"],
        args: &[UsageArg::Optional("line|expr|inst")],
        summary: "show or set stepping granularity",
    },
    CommandSpec {
        command: Command::BreakFunction,
        aliases: &["bf"],
        args: &[UsageArg::Required("func"), UsageArg::Optional("pc")],
        summary: "add breakpoint in a function",
    },
    CommandSpec {
        command: Command::BreakPc,
        aliases: &["b"],
        args: &[UsageArg::Required("pc")],
        summary: "add breakpoint in current chunk",
    },
    CommandSpec {
        command: Command::Breakpoints,
        aliases: &["ib"],
        args: &[],
        summary: "show breakpoints",
    },
    CommandSpec {
        command: Command::ResetBreakpoints,
        aliases: &["rs"],
        args: &[UsageArg::Optional("id|line")],
        summary: "toggle breakpoints",
    },
    CommandSpec {
        command: Command::Track,
        aliases: &["tr", "track"],
        args: &[
            UsageArg::Optional("global|local|capture"),
            UsageArg::Required("name-or-slot"),
        ],
        summary: "track a global, local, or capture",
    },
    CommandSpec {
        command: Command::Tracks,
        aliases: &["it", "tracks"],
        args: &[],
        summary: "show symbol trackers",
    },
    CommandSpec {
        command: Command::Untrack,
        aliases: &["ut", "untrack"],
        args: &[UsageArg::Required("id|all")],
        summary: "remove symbol trackers",
    },
    CommandSpec {
        command: Command::StopHook,
        aliases: &["sh", "stop-hook"],
        args: &[UsageArg::Required("action")],
        summary: "manage commands that run on each stop",
    },
    CommandSpec {
        command: Command::Backtrace,
        aliases: &["bt"],
        args: &[],
        summary: "show backtrace",
    },
    CommandSpec {
        command: Command::Peek,
        aliases: &["p", "peek"],
        args: &[UsageArg::Optional("n")],
        summary: "peek +-n lines (def=3)",
    },
    CommandSpec {
        command: Command::Instructions,
        aliases: &["i", "ins"],
        args: &[UsageArg::Optional("n")],
        summary: "peek +-n insts (def=5)",
    },
    CommandSpec {
        command: Command::Locals,
        aliases: &["lb", "locals"],
        args: &[],
        summary: "dump locals",
    },
    CommandSpec {
        command: Command::Globals,
        aliases: &["gb", "globals"],
        args: &[],
        summary: "dump globals",
    },
    CommandSpec {
        command: Command::Help,
        aliases: &["h", "help"],
        args: &[],
        summary: "show this help",
    },
];

impl CommandSpec {
    fn usage_alias(&self) -> &'static str {
        self.aliases
            .last()
            .copied()
            .expect("every command has at least one alias")
    }
}

impl Command {
    pub(super) fn parse(name: &str) -> Option<Self> {
        command_spec(name).map(|spec| spec.command)
    }

    fn spec(self) -> &'static CommandSpec {
        COMMANDS
            .iter()
            .find(|spec| spec.command == self)
            .expect("every command has a specification")
    }
}

impl UsageArg {
    pub(super) fn plain(self) -> String {
        match self {
            Self::Required(name) => format!("<{name}>"),
            Self::Optional(name) => format!("[{name}]"),
        }
    }
}

pub(super) fn command_spec(name: &str) -> Option<&'static CommandSpec> {
    COMMANDS.iter().find(|spec| spec.aliases.contains(&name))
}

pub(super) fn command_usage_plain(spec: &CommandSpec) -> String {
    let mut usage = spec.aliases.join(" | ");
    for arg in spec.args {
        usage.push(' ');
        usage.push_str(&arg.plain());
    }
    usage
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Usage {
    Command(Command),
    TrackCapture,
    StopHook,
    StopHookAdd,
    StopHookDelete,
}

impl fmt::Display for Usage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Command(command) => {
                let spec = command.spec();
                formatter.write_str(spec.usage_alias())?;
                for arg in spec.args {
                    write!(formatter, " {}", arg.plain())?;
                }
                Ok(())
            }
            Self::TrackCapture => formatter.write_str("track capture <slot>"),
            Self::StopHook => formatter.write_str(
                "stop-hook add -o <cmd> | stop-hook list | stop-hook delete <id|all> | stop-hook clear",
            ),
            Self::StopHookAdd => formatter.write_str("stop-hook add -o <cmd>"),
            Self::StopHookDelete => formatter.write_str("stop-hook delete <id|all>"),
        }
    }
}

pub(super) fn usage_error(usage: Usage) -> String {
    format!("usage: {usage}")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ArgumentCandidate {
    pub(crate) value: &'static str,
    pub(crate) description: &'static str,
    pub(crate) kind: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DynamicArgumentKind {
    Function,
    Symbol,
    Command,
}

pub(super) const GRANULARITIES: &[ArgumentCandidate] = &[
    ArgumentCandidate {
        value: "line",
        description: "pause once per source line",
        kind: "granularity",
    },
    ArgumentCandidate {
        value: "expr",
        description: "pause at each expression (default)",
        kind: "granularity",
    },
    ArgumentCandidate {
        value: "inst",
        description: "pause before every VM instruction",
        kind: "granularity",
    },
];

const TRACK_SCOPES: &[ArgumentCandidate] = &[
    ArgumentCandidate {
        value: "global",
        description: "track a global name",
        kind: "scope",
    },
    ArgumentCandidate {
        value: "local",
        description: "track a local name",
        kind: "scope",
    },
    ArgumentCandidate {
        value: "capture",
        description: "track a capture slot",
        kind: "scope",
    },
];

const STOP_HOOK_ACTIONS: &[ArgumentCandidate] = &[
    ArgumentCandidate {
        value: "add",
        description: "add a command to every stop",
        kind: "action",
    },
    ArgumentCandidate {
        value: "list",
        description: "list stop hooks",
        kind: "action",
    },
    ArgumentCandidate {
        value: "delete",
        description: "delete a stop hook",
        kind: "action",
    },
    ArgumentCandidate {
        value: "clear",
        description: "clear all stop hooks",
        kind: "action",
    },
];

const OPTION_O: &[ArgumentCandidate] = &[ArgumentCandidate {
    value: "-o",
    description: "wqdb command to run",
    kind: "option",
}];

const ALL: &[ArgumentCandidate] = &[ArgumentCandidate {
    value: "all",
    description: "remove all entries",
    kind: "value",
}];

pub(super) fn argument_candidates(
    command_name: &str,
    previous_args: &[&str],
    prefix: &str,
) -> Vec<ArgumentCandidate> {
    let Some(command) = command_spec(command_name).map(|spec| spec.command) else {
        return Vec::new();
    };
    let candidates = match (command, previous_args) {
        (Command::Granularity, []) => GRANULARITIES,
        (Command::Track, []) => TRACK_SCOPES,
        (Command::Untrack, []) => ALL,
        (Command::StopHook, []) => STOP_HOOK_ACTIONS,
        (Command::StopHook, ["add"]) => OPTION_O,
        (Command::StopHook, [action])
            if StopHookAction::parse(action) == Some(StopHookAction::Delete) =>
        {
            ALL
        }
        _ => &[],
    };
    candidates
        .iter()
        .copied()
        .filter(|candidate| candidate.value.starts_with(prefix))
        .collect()
}

pub(super) fn dynamic_argument_kind(
    command_name: &str,
    previous_args: &[&str],
) -> Option<DynamicArgumentKind> {
    let command = command_spec(command_name)?.command;
    match (command, previous_args) {
        (Command::BreakFunction, []) => Some(DynamicArgumentKind::Function),
        (Command::Track, []) => Some(DynamicArgumentKind::Symbol),
        (Command::Track, [scope]) if TrackScope::parse(scope) == Some(TrackScope::Global) => {
            Some(DynamicArgumentKind::Symbol)
        }
        (Command::StopHook, ["add", "-o"]) => Some(DynamicArgumentKind::Command),
        _ => None,
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum ParsedLine<'a> {
    Empty,
    Unknown(&'a str),
    Command(ParsedCommand<'a>),
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum ParsedCommand<'a> {
    Continue,
    StepIn,
    StepOver,
    Finish,
    Granularity(Option<&'a str>),
    BreakFunction {
        name: Option<&'a str>,
        pc: Option<&'a str>,
    },
    BreakPc(Option<&'a str>),
    Breakpoints,
    ResetBreakpoints(Option<&'a str>),
    Track {
        target: Option<&'a str>,
        name: Option<&'a str>,
    },
    Tracks,
    Untrack(Option<&'a str>),
    StopHook(ParsedStopHook<'a>),
    Backtrace,
    Peek(usize),
    Instructions(usize),
    Locals,
    Globals,
    Help,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum ParsedStopHook<'a> {
    Add { command: Option<&'a str> },
    List,
    Delete { target: Option<&'a str> },
    Clear,
    Invalid,
}

pub(super) fn parse_line(line: &str) -> ParsedLine<'_> {
    let mut words = Words::new(line);
    let Some(name) = words.next().map(|word| word.text) else {
        return ParsedLine::Empty;
    };
    let Some(command) = Command::parse(name) else {
        return ParsedLine::Unknown(name);
    };
    let mut next = || words.next().map(|word| word.text);
    let parsed = match command {
        Command::Continue => ParsedCommand::Continue,
        Command::StepIn => ParsedCommand::StepIn,
        Command::StepOver => ParsedCommand::StepOver,
        Command::Finish => ParsedCommand::Finish,
        Command::Granularity => ParsedCommand::Granularity(next()),
        Command::BreakFunction => ParsedCommand::BreakFunction {
            name: next(),
            pc: next(),
        },
        Command::BreakPc => ParsedCommand::BreakPc(next()),
        Command::Breakpoints => ParsedCommand::Breakpoints,
        Command::ResetBreakpoints => ParsedCommand::ResetBreakpoints(next()),
        Command::Track => ParsedCommand::Track {
            target: next(),
            name: next(),
        },
        Command::Tracks => ParsedCommand::Tracks,
        Command::Untrack => ParsedCommand::Untrack(next()),
        Command::StopHook => ParsedCommand::StopHook(parse_stop_hook(line, next(), &mut next)),
        Command::Backtrace => ParsedCommand::Backtrace,
        Command::Peek => ParsedCommand::Peek(next().and_then(|x| x.parse().ok()).unwrap_or(3)),
        Command::Instructions => {
            ParsedCommand::Instructions(next().and_then(|x| x.parse().ok()).unwrap_or(5))
        }
        Command::Locals => ParsedCommand::Locals,
        Command::Globals => ParsedCommand::Globals,
        Command::Help => ParsedCommand::Help,
    };
    ParsedLine::Command(parsed)
}

fn parse_stop_hook<'a>(
    line: &'a str,
    action: Option<&str>,
    next: &mut impl FnMut() -> Option<&'a str>,
) -> ParsedStopHook<'a> {
    match action.and_then(StopHookAction::parse) {
        Some(StopHookAction::Add) => ParsedStopHook::Add {
            command: remainder_after_word(line, "-o"),
        },
        Some(StopHookAction::List) => ParsedStopHook::List,
        Some(StopHookAction::Delete) => ParsedStopHook::Delete { target: next() },
        Some(StopHookAction::Clear) => ParsedStopHook::Clear,
        None => ParsedStopHook::Invalid,
    }
}

fn remainder_after_word<'a>(line: &'a str, target: &str) -> Option<&'a str> {
    Words::new(line)
        .find(|word| word.text == target)
        .map(|word| line[word.end..].trim())
}

#[derive(Clone, Copy)]
struct Word<'a> {
    text: &'a str,
    end: usize,
}

struct Words<'a> {
    source: &'a str,
    offset: usize,
}

impl<'a> Words<'a> {
    fn new(source: &'a str) -> Self {
        Self { source, offset: 0 }
    }
}

impl<'a> Iterator for Words<'a> {
    type Item = Word<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        while self.offset < self.source.len() {
            let ch = self.source[self.offset..]
                .chars()
                .next()
                .expect("offset is within source");
            if !ch.is_whitespace() {
                break;
            }
            self.offset += ch.len_utf8();
        }
        if self.offset == self.source.len() {
            return None;
        }
        let start = self.offset;
        while self.offset < self.source.len() {
            let ch = self.source[self.offset..]
                .chars()
                .next()
                .expect("offset is within source");
            if ch.is_whitespace() {
                break;
            }
            self.offset += ch.len_utf8();
        }
        Some(Word {
            text: &self.source[start..self.offset],
            end: self.offset,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TrackScope {
    Global,
    Local,
    Capture,
}

impl TrackScope {
    pub(super) fn parse(name: &str) -> Option<Self> {
        match name {
            "global" | "g" => Some(Self::Global),
            "local" | "l" => Some(Self::Local),
            "capture" | "cap" => Some(Self::Capture),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum StopHookAction {
    Add,
    List,
    Delete,
    Clear,
}

impl StopHookAction {
    pub(super) fn parse(name: &str) -> Option<Self> {
        match name {
            "add" => Some(Self::Add),
            "list" | "ls" => Some(Self::List),
            "delete" | "del" | "remove" | "rm" => Some(Self::Delete),
            "clear" => Some(Self::Clear),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn aliases_parse_to_typed_commands() {
        assert_eq!(Command::parse("c"), Some(Command::Continue));
        assert_eq!(Command::parse("continue"), Some(Command::Continue));
        assert_eq!(Command::parse("over"), Some(Command::StepOver));
        assert_eq!(Command::parse("track"), Some(Command::Track));
        assert_eq!(Command::parse("sh"), Some(Command::StopHook));
        assert_eq!(Command::parse("granularity"), Some(Command::Granularity));
        assert_eq!(Command::parse("unknown"), None);
    }

    #[test]
    fn aliases_are_unique() {
        let mut aliases = HashSet::new();
        for spec in COMMANDS {
            for alias in spec.aliases {
                assert!(aliases.insert(*alias), "duplicate wqdb alias: {alias}");
            }
        }
    }

    #[test]
    fn usage_renders_pipe_separated_aliases_and_optional_defaults() {
        let continue_spec = spec(Command::Continue);
        let break_fn_spec = spec(Command::BreakFunction);
        let granularity_spec = spec(Command::Granularity);
        let peek_spec = spec(Command::Peek);
        let instructions_spec = spec(Command::Instructions);

        assert_eq!(command_usage_plain(continue_spec), "c | continue");
        assert_eq!(command_usage_plain(break_fn_spec), "bf <func> [pc]");
        assert_eq!(
            command_usage_plain(granularity_spec),
            "g | gran | granularity [line|expr|inst]"
        );
        assert_eq!(command_usage_plain(peek_spec), "p | peek [n]");
        assert_eq!(command_usage_plain(instructions_spec), "i | ins [n]");
    }

    #[test]
    fn errors_use_the_same_typed_usage_vocabulary() {
        assert_eq!(
            usage_error(Usage::Command(Command::Granularity)),
            "usage: granularity [line|expr|inst]"
        );
        assert_eq!(
            usage_error(Usage::Command(Command::BreakFunction)),
            "usage: bf <func> [pc]"
        );
        assert_eq!(
            usage_error(Usage::Command(Command::Track)),
            "usage: track [global|local|capture] <name-or-slot>"
        );
        assert_eq!(
            usage_error(Usage::StopHook),
            "usage: stop-hook add -o <cmd> | stop-hook list | stop-hook delete <id|all> | stop-hook clear"
        );
    }

    #[test]
    fn parser_preserves_permissive_trailing_arguments_and_numeric_defaults() {
        assert_eq!(
            parse_line("c ignored"),
            ParsedLine::Command(ParsedCommand::Continue)
        );
        assert_eq!(
            parse_line("p nope"),
            ParsedLine::Command(ParsedCommand::Peek(3))
        );
        assert_eq!(
            parse_line("i 8 ignored"),
            ParsedLine::Command(ParsedCommand::Instructions(8))
        );
    }

    #[test]
    fn parser_preserves_the_nested_stop_hook_command() {
        assert_eq!(
            parse_line("stop-hook add -o track local total"),
            ParsedLine::Command(ParsedCommand::StopHook(ParsedStopHook::Add {
                command: Some("track local total")
            }))
        );
        assert_eq!(
            parse_line("stop-hook add -option ignored -o c ignored"),
            ParsedLine::Command(ParsedCommand::StopHook(ParsedStopHook::Add {
                command: Some("c ignored")
            }))
        );
    }

    #[test]
    fn parser_distinguishes_empty_and_unknown_commands() {
        assert_eq!(parse_line("   "), ParsedLine::Empty);
        assert_eq!(parse_line("wat arg"), ParsedLine::Unknown("wat"));
    }

    #[test]
    fn track_scope_aliases_parse() {
        assert_eq!(TrackScope::parse("global"), Some(TrackScope::Global));
        assert_eq!(TrackScope::parse("g"), Some(TrackScope::Global));
        assert_eq!(TrackScope::parse("local"), Some(TrackScope::Local));
        assert_eq!(TrackScope::parse("l"), Some(TrackScope::Local));
        assert_eq!(TrackScope::parse("capture"), Some(TrackScope::Capture));
        assert_eq!(TrackScope::parse("cap"), Some(TrackScope::Capture));
        assert_eq!(TrackScope::parse("x"), None);
    }

    #[test]
    fn stop_hook_aliases_parse() {
        assert_eq!(StopHookAction::parse("add"), Some(StopHookAction::Add));
        assert_eq!(StopHookAction::parse("list"), Some(StopHookAction::List));
        assert_eq!(StopHookAction::parse("ls"), Some(StopHookAction::List));
        assert_eq!(
            StopHookAction::parse("delete"),
            Some(StopHookAction::Delete)
        );
        assert_eq!(StopHookAction::parse("rm"), Some(StopHookAction::Delete));
        assert_eq!(StopHookAction::parse("clear"), Some(StopHookAction::Clear));
        assert_eq!(StopHookAction::parse("x"), None);
    }

    fn spec(command: Command) -> &'static CommandSpec {
        COMMANDS
            .iter()
            .find(|spec| spec.command == command)
            .expect("command spec")
    }
}
