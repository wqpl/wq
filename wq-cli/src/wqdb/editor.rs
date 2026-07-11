use super::{WQDB_COMMANDS, WqdbCommand, WqdbCommandSpec};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CommandEntry {
    pub(crate) name: &'static str,
    pub(crate) usage: String,
    pub(crate) summary: &'static str,
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CursorTarget<'a> {
    Empty {
        start: usize,
    },
    Command {
        start: usize,
        prefix: &'a str,
    },
    Argument {
        command: &'a str,
        start: usize,
        prefix: &'a str,
        previous_args: Vec<&'a str>,
    },
}

impl CursorTarget<'_> {
    pub(crate) fn start(&self) -> usize {
        match self {
            Self::Empty { start } | Self::Command { start, .. } | Self::Argument { start, .. } => {
                *start
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TokenKind {
    Command,
    UnknownCommand,
    Subcommand,
    Flag,
    Number,
    Argument,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TokenSpan {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) kind: TokenKind,
}

const GRANULARITIES: &[ArgumentCandidate] = &[
    ArgumentCandidate {
        value: "line",
        description: "pause once per source line",
        kind: "granularity",
    },
    ArgumentCandidate {
        value: "expr",
        description: "pause at each expression",
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

fn command_spec(name: &str) -> Option<&'static WqdbCommandSpec> {
    WQDB_COMMANDS
        .iter()
        .find(|spec| spec.aliases.contains(&name))
}

fn command_usage(spec: &WqdbCommandSpec, name: &str) -> String {
    let mut usage = name.to_string();
    for arg in spec.args {
        usage.push(' ');
        usage.push_str(&arg.plain());
    }
    usage
}

pub(crate) fn command_entry(name: &str) -> Option<CommandEntry> {
    let spec = command_spec(name)?;
    Some(CommandEntry {
        name: spec.aliases.iter().find(|alias| **alias == name).copied()?,
        usage: command_usage(spec, name),
        summary: spec.summary,
    })
}

pub(crate) fn command_entries(prefix: &str) -> Vec<CommandEntry> {
    let mut entries = WQDB_COMMANDS
        .iter()
        .flat_map(|spec| {
            spec.aliases
                .iter()
                .filter(|alias| alias.starts_with(prefix))
                .map(|alias| CommandEntry {
                    name: alias,
                    usage: command_usage(spec, alias),
                    summary: spec.summary,
                })
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.name.cmp(right.name));
    entries
}

pub(crate) fn argument_candidates(
    command_name: &str,
    previous_args: &[&str],
    prefix: &str,
) -> Vec<ArgumentCandidate> {
    let Some(command) = command_spec(command_name).map(|spec| spec.command) else {
        return Vec::new();
    };
    let candidates = match (command, previous_args) {
        (WqdbCommand::Granularity, []) => GRANULARITIES,
        (WqdbCommand::Track, []) => TRACK_SCOPES,
        (WqdbCommand::Untrack, []) => ALL,
        (WqdbCommand::StopHook, []) => STOP_HOOK_ACTIONS,
        (WqdbCommand::StopHook, ["add"]) => OPTION_O,
        (WqdbCommand::StopHook, ["delete" | "del" | "remove" | "rm"]) => ALL,
        _ => &[],
    };
    candidates
        .iter()
        .copied()
        .filter(|candidate| candidate.value.starts_with(prefix))
        .collect()
}

pub(crate) fn dynamic_argument_kind(
    command_name: &str,
    previous_args: &[&str],
) -> Option<DynamicArgumentKind> {
    let command = command_spec(command_name)?.command;
    match (command, previous_args) {
        (WqdbCommand::BreakFunction, []) => Some(DynamicArgumentKind::Function),
        (WqdbCommand::Track, [] | ["global" | "g"]) => Some(DynamicArgumentKind::Symbol),
        (WqdbCommand::StopHook, ["add", "-o"]) => Some(DynamicArgumentKind::Command),
        _ => None,
    }
}

fn is_command(token: &str) -> bool {
    command_spec(token).is_some()
}

pub(crate) fn cursor_target(line: &str, pos: usize) -> CursorTarget<'_> {
    let pos = pos.min(line.len());
    let before_cursor = &line[..pos];
    let trimmed = before_cursor.trim_start_matches(char::is_whitespace);
    let leading = before_cursor.len() - trimmed.len();
    if trimmed.is_empty() {
        return CursorTarget::Empty { start: pos };
    }
    let Some(command_end) = trimmed.find(char::is_whitespace) else {
        return CursorTarget::Command {
            start: leading,
            prefix: trimmed,
        };
    };
    let command = &trimmed[..command_end];
    let word_start = before_cursor
        .char_indices()
        .rev()
        .find(|(_, ch)| ch.is_whitespace())
        .map_or(leading + command_end, |(idx, ch)| idx + ch.len_utf8());
    let args_start = leading + command_end;
    CursorTarget::Argument {
        command,
        start: word_start,
        prefix: &before_cursor[word_start..],
        previous_args: before_cursor[args_start..word_start]
            .split_whitespace()
            .collect(),
    }
}

pub(crate) fn token_spans(line: &str) -> Vec<TokenSpan> {
    let mut spans = Vec::new();
    let mut offset = 0;
    let mut command = None;
    let mut args = Vec::new();
    while offset < line.len() {
        let rest = &line[offset..];
        let ch = rest.chars().next().expect("offset is within line");
        if ch.is_whitespace() {
            offset += ch.len_utf8();
            continue;
        }
        let token_len = rest
            .char_indices()
            .find(|(_, ch)| ch.is_whitespace())
            .map_or(rest.len(), |(idx, _)| idx);
        let token = &rest[..token_len];
        let kind = if let Some(command) = command {
            if dynamic_argument_kind(command, &args) == Some(DynamicArgumentKind::Command) {
                if is_command(token) {
                    TokenKind::Command
                } else {
                    TokenKind::UnknownCommand
                }
            } else if token.starts_with('-') {
                TokenKind::Flag
            } else if argument_candidates(command, &args, token)
                .iter()
                .any(|candidate| candidate.value == token)
            {
                TokenKind::Subcommand
            } else if token.parse::<i64>().is_ok() {
                TokenKind::Number
            } else {
                TokenKind::Argument
            }
        } else if is_command(token) {
            command = Some(token);
            TokenKind::Command
        } else {
            command = Some(token);
            TokenKind::UnknownCommand
        };
        spans.push(TokenSpan {
            start: offset,
            end: offset + token_len,
            kind,
        });
        if spans.len() > 1 {
            args.push(token);
        }
        offset += token_len;
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_usage_only_shows_the_typed_alias() {
        let entry = command_entry("g").expect("granularity command");

        assert_eq!(entry.usage, "g [line|expr|inst]");
    }

    #[test]
    fn cursor_target_handles_leading_and_repeated_whitespace() {
        let input = "  stop-hook   add  -o   gr";
        let target = cursor_target(input, input.len());

        assert_eq!(
            target,
            CursorTarget::Argument {
                command: "stop-hook",
                start: 24,
                prefix: "gr",
                previous_args: vec!["add", "-o"],
            }
        );
    }

    #[test]
    fn token_spans_use_command_context_for_subcommands() {
        let spans = token_spans("stop-hook add -o g 12");

        assert_eq!(
            spans.iter().map(|span| span.kind).collect::<Vec<_>>(),
            vec![
                TokenKind::Command,
                TokenKind::Subcommand,
                TokenKind::Flag,
                TokenKind::Command,
                TokenKind::Number,
            ]
        );
        assert_eq!(&"stop-hook add -o g 12"[spans[3].start..spans[3].end], "g");
    }
}
