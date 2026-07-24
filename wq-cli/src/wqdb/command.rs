use std::fmt;

use wqpl::wqdb::StepGranularity;

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
        args: &[UsageArg::Required("action")],
        summary: "manage symbol trackers",
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
    Track,
    TrackAdd,
    TrackAddGlobal,
    TrackAddLocal,
    TrackAddCapture,
    TrackList,
    TrackDelete,
    TrackClear,
    StopHook,
    StopHookAdd,
    StopHookList,
    StopHookDelete,
    StopHookClear,
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
            Self::Track => formatter.write_str(
                "track add <global|local|capture> <target> | track list | track delete <id> | track clear",
            ),
            Self::TrackAdd => {
                formatter.write_str("track add <global|local|capture> <target>")
            }
            Self::TrackAddGlobal => formatter.write_str("track add global <name>"),
            Self::TrackAddLocal => formatter.write_str("track add local <name>"),
            Self::TrackAddCapture => formatter.write_str("track add capture <slot>"),
            Self::TrackList => formatter.write_str("track list"),
            Self::TrackDelete => formatter.write_str("track delete <id>"),
            Self::TrackClear => formatter.write_str("track clear"),
            Self::StopHook => formatter.write_str(
                "stop-hook add <command...> | stop-hook list | stop-hook delete <id> | stop-hook clear",
            ),
            Self::StopHookAdd => formatter.write_str("stop-hook add <command...>"),
            Self::StopHookList => formatter.write_str("stop-hook list"),
            Self::StopHookDelete => formatter.write_str("stop-hook delete <id>"),
            Self::StopHookClear => formatter.write_str("stop-hook clear"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ArgumentCandidate {
    pub(crate) value: &'static str,
    pub(crate) description: &'static str,
    pub(crate) kind: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct CommandForm {
    pub(super) candidate: ArgumentCandidate,
    pub(super) usage: Usage,
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

pub(super) const TRACK_ACTIONS: &[CommandForm] = &[
    CommandForm {
        candidate: ArgumentCandidate {
            value: "add",
            description: "add a symbol tracker",
            kind: "action",
        },
        usage: Usage::TrackAdd,
    },
    CommandForm {
        candidate: ArgumentCandidate {
            value: "list",
            description: "list symbol trackers",
            kind: "action",
        },
        usage: Usage::TrackList,
    },
    CommandForm {
        candidate: ArgumentCandidate {
            value: "delete",
            description: "delete a symbol tracker",
            kind: "action",
        },
        usage: Usage::TrackDelete,
    },
    CommandForm {
        candidate: ArgumentCandidate {
            value: "clear",
            description: "clear all symbol trackers",
            kind: "action",
        },
        usage: Usage::TrackClear,
    },
];

pub(super) const TRACK_SCOPES: &[CommandForm] = &[
    CommandForm {
        candidate: ArgumentCandidate {
            value: "global",
            description: "track a global name",
            kind: "scope",
        },
        usage: Usage::TrackAddGlobal,
    },
    CommandForm {
        candidate: ArgumentCandidate {
            value: "local",
            description: "track a local name",
            kind: "scope",
        },
        usage: Usage::TrackAddLocal,
    },
    CommandForm {
        candidate: ArgumentCandidate {
            value: "capture",
            description: "track a capture slot",
            kind: "scope",
        },
        usage: Usage::TrackAddCapture,
    },
];

pub(super) const STOP_HOOK_ACTIONS: &[CommandForm] = &[
    CommandForm {
        candidate: ArgumentCandidate {
            value: "add",
            description: "add a command to every stop",
            kind: "action",
        },
        usage: Usage::StopHookAdd,
    },
    CommandForm {
        candidate: ArgumentCandidate {
            value: "list",
            description: "list stop hooks",
            kind: "action",
        },
        usage: Usage::StopHookList,
    },
    CommandForm {
        candidate: ArgumentCandidate {
            value: "delete",
            description: "delete a stop hook",
            kind: "action",
        },
        usage: Usage::StopHookDelete,
    },
    CommandForm {
        candidate: ArgumentCandidate {
            value: "clear",
            description: "clear all stop hooks",
            kind: "action",
        },
        usage: Usage::StopHookClear,
    },
];

pub(super) fn argument_candidates(
    command_name: &str,
    previous_args: &[&str],
    prefix: &str,
) -> Vec<ArgumentCandidate> {
    let Some(command) = command_spec(command_name).map(|spec| spec.command) else {
        return Vec::new();
    };
    if command == Command::StopHook
        && let ["add", nested_command, nested_args @ ..] = previous_args
    {
        return argument_candidates(nested_command, nested_args, prefix);
    }
    match (command, previous_args) {
        (Command::Granularity, []) => GRANULARITIES
            .iter()
            .copied()
            .filter(|candidate| candidate.value.starts_with(prefix))
            .collect(),
        (Command::Track, []) => matching_form_candidates(TRACK_ACTIONS, prefix),
        (Command::Track, ["add"]) => matching_form_candidates(TRACK_SCOPES, prefix),
        (Command::StopHook, []) => matching_form_candidates(STOP_HOOK_ACTIONS, prefix),
        _ => Vec::new(),
    }
}

fn matching_form_candidates(forms: &[CommandForm], prefix: &str) -> Vec<ArgumentCandidate> {
    forms
        .iter()
        .map(|form| form.candidate)
        .filter(|candidate| candidate.value.starts_with(prefix))
        .collect()
}

pub(super) fn dynamic_argument_kind(
    command_name: &str,
    previous_args: &[&str],
) -> Option<DynamicArgumentKind> {
    let command = command_spec(command_name)?.command;
    if command == Command::StopHook
        && let ["add", nested_command, nested_args @ ..] = previous_args
    {
        return dynamic_argument_kind(nested_command, nested_args);
    }
    match (command, previous_args) {
        (Command::BreakFunction, []) => Some(DynamicArgumentKind::Function),
        (Command::Track, ["add", "global"]) => Some(DynamicArgumentKind::Symbol),
        (Command::StopHook, ["add"]) => Some(DynamicArgumentKind::Command),
        _ => None,
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum ParsedLine<'a> {
    Empty,
    Command(ParsedCommand<'a>),
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum ParsedCommand<'a> {
    Continue,
    StepIn,
    StepOver,
    Finish,
    Granularity(Option<StepGranularity>),
    BreakFunction { name: &'a str, pc: Option<usize> },
    BreakPc(usize),
    Breakpoints,
    ResetBreakpoints(Option<usize>),
    Track(TrackCommand<'a>),
    StopHook(StopHookCommand<'a>),
    Backtrace,
    Peek(usize),
    Instructions(usize),
    Locals,
    Globals,
    Help,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum TrackCommand<'a> {
    Add(TrackTarget<'a>),
    List,
    Delete { id: usize },
    Clear,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum TrackTarget<'a> {
    Global(&'a str),
    Local(&'a str),
    Capture(u16),
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum StopHookCommand<'a> {
    Add { command: &'a str },
    List,
    Delete { id: usize },
    Clear,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum CommandParseError<'a> {
    UnknownCommand(&'a str),
    MissingArgument {
        name: &'static str,
        usage: Usage,
    },
    UnexpectedArgument {
        argument: &'a str,
        usage: Usage,
    },
    InvalidValue {
        name: &'static str,
        value: &'a str,
        usage: Usage,
    },
    LegacySyntax {
        syntax: &'static str,
        replacement: &'static str,
    },
}

impl fmt::Display for CommandParseError<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownCommand(name) => {
                write!(
                    formatter,
                    "unknown wqdb command '{name}', type 'h' for help"
                )
            }
            Self::MissingArgument { name, usage } => {
                write!(formatter, "missing argument '{name}'; usage: {usage}")
            }
            Self::UnexpectedArgument { argument, usage } => {
                write!(
                    formatter,
                    "unexpected argument '{argument}'; usage: {usage}"
                )
            }
            Self::InvalidValue { name, value, usage } => {
                write!(formatter, "invalid {name} '{value}'; usage: {usage}")
            }
            Self::LegacySyntax {
                syntax,
                replacement,
            } => {
                write!(
                    formatter,
                    "syntax '{syntax}' is no longer supported; use {replacement}"
                )
            }
        }
    }
}

pub(super) fn parse_line(line: &str) -> Result<ParsedLine<'_>, CommandParseError<'_>> {
    let mut words = Words::new(line);
    let Some(name) = words.next().map(|word| word.text) else {
        return Ok(ParsedLine::Empty);
    };
    if let Some(error) = parse_legacy_command(name, &mut words) {
        return Err(error);
    }
    let Some(command) = Command::parse(name) else {
        return Err(CommandParseError::UnknownCommand(name));
    };
    let parsed = match command {
        Command::Continue => {
            reject_extra(&mut words, Usage::Command(command))?;
            ParsedCommand::Continue
        }
        Command::StepIn => {
            reject_extra(&mut words, Usage::Command(command))?;
            ParsedCommand::StepIn
        }
        Command::StepOver => {
            reject_extra(&mut words, Usage::Command(command))?;
            ParsedCommand::StepOver
        }
        Command::Finish => {
            reject_extra(&mut words, Usage::Command(command))?;
            ParsedCommand::Finish
        }
        Command::Granularity => {
            let granularity = words
                .next()
                .map(|word| parse_granularity(word.text, Usage::Command(command)))
                .transpose()?;
            reject_extra(&mut words, Usage::Command(command))?;
            ParsedCommand::Granularity(granularity)
        }
        Command::BreakFunction => {
            let name = require_word(&mut words, "func", Usage::Command(command))?.text;
            let pc = words
                .next()
                .map(|word| parse_usize(word.text, "pc", Usage::Command(command)))
                .transpose()?;
            reject_extra(&mut words, Usage::Command(command))?;
            ParsedCommand::BreakFunction { name, pc }
        }
        Command::BreakPc => {
            let word = require_word(&mut words, "pc", Usage::Command(command))?;
            let pc = parse_usize(word.text, "pc", Usage::Command(command))?;
            reject_extra(&mut words, Usage::Command(command))?;
            ParsedCommand::BreakPc(pc)
        }
        Command::Breakpoints => {
            reject_extra(&mut words, Usage::Command(command))?;
            ParsedCommand::Breakpoints
        }
        Command::ResetBreakpoints => {
            let target = words
                .next()
                .map(|word| parse_usize(word.text, "id or line", Usage::Command(command)))
                .transpose()?;
            reject_extra(&mut words, Usage::Command(command))?;
            ParsedCommand::ResetBreakpoints(target)
        }
        Command::Track => ParsedCommand::Track(parse_track(&mut words)?),
        Command::StopHook => ParsedCommand::StopHook(parse_stop_hook(line, &mut words)?),
        Command::Backtrace => {
            reject_extra(&mut words, Usage::Command(command))?;
            ParsedCommand::Backtrace
        }
        Command::Peek => {
            let count = parse_optional_count(&mut words, command, 3)?;
            ParsedCommand::Peek(count)
        }
        Command::Instructions => {
            let count = parse_optional_count(&mut words, command, 5)?;
            ParsedCommand::Instructions(count)
        }
        Command::Locals => {
            reject_extra(&mut words, Usage::Command(command))?;
            ParsedCommand::Locals
        }
        Command::Globals => {
            reject_extra(&mut words, Usage::Command(command))?;
            ParsedCommand::Globals
        }
        Command::Help => {
            reject_extra(&mut words, Usage::Command(command))?;
            ParsedCommand::Help
        }
    };
    Ok(ParsedLine::Command(parsed))
}

fn parse_legacy_command<'a>(name: &str, words: &mut Words<'a>) -> Option<CommandParseError<'a>> {
    match name {
        "tracks" => Some(legacy("tracks", "'track list'")),
        "it" => Some(legacy("it", "'track list'")),
        "untrack" | "ut" => {
            let clear = words.next().is_some_and(|word| word.text == "all");
            Some(if clear {
                legacy("untrack all", "'track clear'")
            } else {
                legacy("untrack <id>", "'track delete <id>'")
            })
        }
        _ => None,
    }
}

fn parse_granularity<'a>(
    value: &'a str,
    usage: Usage,
) -> Result<StepGranularity, CommandParseError<'a>> {
    match value {
        "line" => Ok(StepGranularity::Line),
        "expr" => Ok(StepGranularity::Expr),
        "inst" => Ok(StepGranularity::Inst),
        _ => Err(invalid("granularity", value, usage)),
    }
}

fn parse_optional_count<'a>(
    words: &mut Words<'a>,
    command: Command,
    default: usize,
) -> Result<usize, CommandParseError<'a>> {
    let usage = Usage::Command(command);
    let count = words
        .next()
        .map(|word| parse_usize(word.text, "count", usage))
        .transpose()?
        .unwrap_or(default);
    reject_extra(words, usage)?;
    Ok(count)
}

fn parse_track<'a>(words: &mut Words<'a>) -> Result<TrackCommand<'a>, CommandParseError<'a>> {
    let action = require_word(words, "action", Usage::Track)?.text;
    match action {
        "add" => parse_track_add(words),
        "list" => {
            reject_extra(words, Usage::TrackList)?;
            Ok(TrackCommand::List)
        }
        "delete" => {
            let word = require_word(words, "id", Usage::TrackDelete)?;
            let id = parse_usize(word.text, "id", Usage::TrackDelete)?;
            reject_extra(words, Usage::TrackDelete)?;
            Ok(TrackCommand::Delete { id })
        }
        "clear" => {
            reject_extra(words, Usage::TrackClear)?;
            Ok(TrackCommand::Clear)
        }
        "global" => Err(legacy("track global <name>", "'track add global <name>'")),
        "local" => Err(legacy("track local <name>", "'track add local <name>'")),
        "capture" => Err(legacy("track capture <slot>", "'track add capture <slot>'")),
        "g" | "l" | "cap" => Err(legacy(
            "track <scope> <target>",
            "'track add <global|local|capture> <target>'",
        )),
        _ if words.next().is_none() => Err(legacy(
            "track <name>",
            "'track add local <name>' or 'track add global <name>'",
        )),
        _ => Err(invalid("track action", action, Usage::Track)),
    }
}

fn parse_track_add<'a>(words: &mut Words<'a>) -> Result<TrackCommand<'a>, CommandParseError<'a>> {
    let scope = require_word(words, "scope", Usage::TrackAdd)?.text;
    let target = match scope {
        "global" => {
            let name = require_word(words, "name", Usage::TrackAddGlobal)?.text;
            reject_extra(words, Usage::TrackAddGlobal)?;
            TrackTarget::Global(name)
        }
        "local" => {
            let name = require_word(words, "name", Usage::TrackAddLocal)?.text;
            reject_extra(words, Usage::TrackAddLocal)?;
            TrackTarget::Local(name)
        }
        "capture" => {
            let word = require_word(words, "slot", Usage::TrackAddCapture)?;
            let slot = word
                .text
                .parse()
                .map_err(|_| invalid("slot", word.text, Usage::TrackAddCapture))?;
            reject_extra(words, Usage::TrackAddCapture)?;
            TrackTarget::Capture(slot)
        }
        "g" | "l" | "cap" => {
            return Err(legacy(
                "track add <scope-alias> <target>",
                "'track add <global|local|capture> <target>'",
            ));
        }
        _ => {
            return Err(invalid("track scope", scope, Usage::TrackAdd));
        }
    };
    Ok(TrackCommand::Add(target))
}

fn parse_stop_hook<'a>(
    line: &'a str,
    words: &mut Words<'a>,
) -> Result<StopHookCommand<'a>, CommandParseError<'a>> {
    let action = require_word(words, "action", Usage::StopHook)?;
    match action.text {
        "add" => {
            let command = line[action.end..].trim();
            if command.is_empty() {
                return Err(CommandParseError::MissingArgument {
                    name: "command",
                    usage: Usage::StopHookAdd,
                });
            }
            if command
                .split_whitespace()
                .next()
                .is_some_and(|word| word == "-o")
            {
                return Err(legacy(
                    "stop-hook add -o <command>",
                    "'stop-hook add <command...>'",
                ));
            }
            Ok(StopHookCommand::Add { command })
        }
        "list" => {
            reject_extra(words, Usage::StopHookList)?;
            Ok(StopHookCommand::List)
        }
        "delete" => {
            let word = require_word(words, "id", Usage::StopHookDelete)?;
            let id = parse_usize(word.text, "id", Usage::StopHookDelete)?;
            reject_extra(words, Usage::StopHookDelete)?;
            Ok(StopHookCommand::Delete { id })
        }
        "clear" => {
            reject_extra(words, Usage::StopHookClear)?;
            Ok(StopHookCommand::Clear)
        }
        "ls" => Err(legacy("stop-hook ls", "'stop-hook list'")),
        "del" | "remove" | "rm" => Err(legacy(
            "stop-hook <delete-alias> <id>",
            "'stop-hook delete <id>'",
        )),
        _ => Err(invalid("stop-hook action", action.text, Usage::StopHook)),
    }
}

fn require_word<'a>(
    words: &mut Words<'a>,
    name: &'static str,
    usage: Usage,
) -> Result<Word<'a>, CommandParseError<'a>> {
    words
        .next()
        .ok_or(CommandParseError::MissingArgument { name, usage })
}

fn reject_extra<'a>(words: &mut Words<'a>, usage: Usage) -> Result<(), CommandParseError<'a>> {
    if let Some(word) = words.next() {
        Err(CommandParseError::UnexpectedArgument {
            argument: word.text,
            usage,
        })
    } else {
        Ok(())
    }
}

fn parse_usize<'a>(
    value: &'a str,
    name: &'static str,
    usage: Usage,
) -> Result<usize, CommandParseError<'a>> {
    value.parse().map_err(|_| invalid(name, value, usage))
}

fn invalid<'a>(name: &'static str, value: &'a str, usage: Usage) -> CommandParseError<'a> {
    CommandParseError::InvalidValue { name, value, usage }
}

fn legacy<'a>(syntax: &'static str, replacement: &'static str) -> CommandParseError<'a> {
    CommandParseError::LegacySyntax {
        syntax,
        replacement,
    }
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
            format!("usage: {}", Usage::Command(Command::Granularity)),
            "usage: granularity [line|expr|inst]"
        );
        assert_eq!(
            format!("usage: {}", Usage::Command(Command::BreakFunction)),
            "usage: bf <func> [pc]"
        );
        assert_eq!(
            format!("usage: {}", Usage::Track),
            "usage: track add <global|local|capture> <target> | track list | track delete <id> | track clear"
        );
        assert_eq!(
            format!("usage: {}", Usage::StopHook),
            "usage: stop-hook add <command...> | stop-hook list | stop-hook delete <id> | stop-hook clear"
        );
    }

    #[test]
    fn parser_enforces_arity_and_numeric_arguments() {
        assert_eq!(
            parse_line("c"),
            Ok(ParsedLine::Command(ParsedCommand::Continue))
        );
        assert_eq!(
            parse_line("p"),
            Ok(ParsedLine::Command(ParsedCommand::Peek(3)))
        );
        assert_eq!(
            parse_line("i 8"),
            Ok(ParsedLine::Command(ParsedCommand::Instructions(8)))
        );
        assert_eq!(
            parse_line("c ignored")
                .expect_err("trailing argument")
                .to_string(),
            "unexpected argument 'ignored'; usage: continue"
        );
        assert_eq!(
            parse_line("p nope").expect_err("invalid count").to_string(),
            "invalid count 'nope'; usage: peek [n]"
        );
        assert_eq!(
            parse_line("i 8 ignored")
                .expect_err("trailing argument")
                .to_string(),
            "unexpected argument 'ignored'; usage: ins [n]"
        );
        assert_eq!(
            parse_line("g l")
                .expect_err("canonical granularity")
                .to_string(),
            "invalid granularity 'l'; usage: granularity [line|expr|inst]"
        );
        assert_eq!(
            parse_line("bf worker nope")
                .expect_err("invalid program counter")
                .to_string(),
            "invalid pc 'nope'; usage: bf <func> [pc]"
        );
    }

    #[test]
    fn parser_requires_explicit_typed_track_commands() {
        assert_eq!(
            parse_line("track add global total"),
            Ok(ParsedLine::Command(ParsedCommand::Track(
                TrackCommand::Add(TrackTarget::Global("total"))
            )))
        );
        assert_eq!(
            parse_line("tr add local subtotal"),
            Ok(ParsedLine::Command(ParsedCommand::Track(
                TrackCommand::Add(TrackTarget::Local("subtotal"))
            )))
        );
        assert_eq!(
            parse_line("track add capture 2"),
            Ok(ParsedLine::Command(ParsedCommand::Track(
                TrackCommand::Add(TrackTarget::Capture(2))
            )))
        );
        assert_eq!(
            parse_line("track list"),
            Ok(ParsedLine::Command(ParsedCommand::Track(
                TrackCommand::List
            )))
        );
        assert_eq!(
            parse_line("track delete 4"),
            Ok(ParsedLine::Command(ParsedCommand::Track(
                TrackCommand::Delete { id: 4 }
            )))
        );
        assert_eq!(
            parse_line("track clear"),
            Ok(ParsedLine::Command(ParsedCommand::Track(
                TrackCommand::Clear
            )))
        );
        assert_eq!(
            parse_line("track total")
                .expect_err("implicit tracking")
                .to_string(),
            "syntax 'track <name>' is no longer supported; use 'track add local <name>' or 'track add global <name>'"
        );
        assert_eq!(
            parse_line("track global total")
                .expect_err("legacy tracking")
                .to_string(),
            "syntax 'track global <name>' is no longer supported; use 'track add global <name>'"
        );
        assert_eq!(
            parse_line("tracks")
                .expect_err("legacy list command")
                .to_string(),
            "syntax 'tracks' is no longer supported; use 'track list'"
        );
    }

    #[test]
    fn parser_captures_stop_hook_remainder_without_an_option() {
        assert_eq!(
            parse_line("stop-hook add track add local total"),
            Ok(ParsedLine::Command(ParsedCommand::StopHook(
                StopHookCommand::Add {
                    command: "track add local total"
                }
            )))
        );
        assert_eq!(
            parse_line("sh list"),
            Ok(ParsedLine::Command(ParsedCommand::StopHook(
                StopHookCommand::List
            )))
        );
        assert_eq!(
            parse_line("stop-hook delete 3"),
            Ok(ParsedLine::Command(ParsedCommand::StopHook(
                StopHookCommand::Delete { id: 3 }
            )))
        );
        assert_eq!(
            parse_line("stop-hook clear"),
            Ok(ParsedLine::Command(ParsedCommand::StopHook(
                StopHookCommand::Clear
            )))
        );
        assert_eq!(
            parse_line("stop-hook add -o c")
                .expect_err("legacy option")
                .to_string(),
            "syntax 'stop-hook add -o <command>' is no longer supported; use 'stop-hook add <command...>'"
        );
        assert_eq!(
            parse_line("stop-hook list ignored")
                .expect_err("trailing argument")
                .to_string(),
            "unexpected argument 'ignored'; usage: stop-hook list"
        );
    }

    #[test]
    fn parser_rejects_invalid_values_and_trailing_arguments_at_every_level() {
        assert_eq!(parse_error("b -1"), "invalid pc '-1'; usage: b <pc>");
        assert_eq!(
            parse_error("rs nope"),
            "invalid id or line 'nope'; usage: rs [id|line]"
        );
        assert_eq!(
            parse_error("bf worker 2 extra"),
            "unexpected argument 'extra'; usage: bf <func> [pc]"
        );
        assert_eq!(
            parse_error("track add global"),
            "missing argument 'name'; usage: track add global <name>"
        );
        assert_eq!(
            parse_error("track add capture -1"),
            "invalid slot '-1'; usage: track add capture <slot>"
        );
        assert_eq!(
            parse_error("track delete nope"),
            "invalid id 'nope'; usage: track delete <id>"
        );
        assert_eq!(
            parse_error("stop-hook delete 2 extra"),
            "unexpected argument 'extra'; usage: stop-hook delete <id>"
        );
        assert_eq!(
            parse_error("help extra"),
            "unexpected argument 'extra'; usage: help"
        );
    }

    #[test]
    fn parser_rejects_noncanonical_nested_aliases_with_migrations() {
        assert_eq!(
            parse_error("track add g total"),
            "syntax 'track add <scope-alias> <target>' is no longer supported; use 'track add <global|local|capture> <target>'"
        );
        assert_eq!(
            parse_error("stop-hook ls"),
            "syntax 'stop-hook ls' is no longer supported; use 'stop-hook list'"
        );
        assert_eq!(
            parse_error("stop-hook rm 2"),
            "syntax 'stop-hook <delete-alias> <id>' is no longer supported; use 'stop-hook delete <id>'"
        );
    }

    #[test]
    fn parser_distinguishes_empty_unknown_and_legacy_commands() {
        assert_eq!(parse_line("   "), Ok(ParsedLine::Empty));
        assert_eq!(
            parse_line("wat arg")
                .expect_err("unknown command")
                .to_string(),
            "unknown wqdb command 'wat', type 'h' for help"
        );
        assert_eq!(
            parse_line("untrack 3")
                .expect_err("legacy delete command")
                .to_string(),
            "syntax 'untrack <id>' is no longer supported; use 'track delete <id>'"
        );
        assert_eq!(
            parse_line("untrack all")
                .expect_err("legacy clear command")
                .to_string(),
            "syntax 'untrack all' is no longer supported; use 'track clear'"
        );
    }

    fn spec(command: Command) -> &'static CommandSpec {
        COMMANDS
            .iter()
            .find(|spec| spec.command == command)
            .expect("command spec")
    }

    fn parse_error(input: &str) -> String {
        parse_line(input).expect_err("invalid command").to_string()
    }
}
