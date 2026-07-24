pub(crate) use super::command::{ArgumentCandidate, DynamicArgumentKind};
use super::command::{
    COMMANDS, CommandSpec, argument_candidates as command_argument_candidates, command_spec,
    dynamic_argument_kind as command_dynamic_argument_kind,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CommandEntry {
    pub(crate) name: &'static str,
    pub(crate) usage: String,
    pub(crate) summary: &'static str,
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

fn command_usage(spec: &CommandSpec, name: &str) -> String {
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
    let mut entries = COMMANDS
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
    command_argument_candidates(command_name, previous_args, prefix)
}

pub(crate) fn dynamic_argument_kind(
    command_name: &str,
    previous_args: &[&str],
) -> Option<DynamicArgumentKind> {
    command_dynamic_argument_kind(command_name, previous_args)
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
        let input = "  stop-hook   add   gr";
        let target = cursor_target(input, input.len());

        assert_eq!(
            target,
            CursorTarget::Argument {
                command: "stop-hook",
                start: 20,
                prefix: "gr",
                previous_args: vec!["add"],
            }
        );
    }

    #[test]
    fn token_spans_use_command_context_for_subcommands() {
        let spans = token_spans("stop-hook add g 12");

        assert_eq!(
            spans.iter().map(|span| span.kind).collect::<Vec<_>>(),
            vec![
                TokenKind::Command,
                TokenKind::Subcommand,
                TokenKind::Command,
                TokenKind::Number,
            ]
        );
        assert_eq!(&"stop-hook add g 12"[spans[2].start..spans[2].end], "g");
    }

    #[test]
    fn token_spans_follow_nested_stop_hook_command_context() {
        let spans = token_spans("stop-hook add track add local total");

        assert_eq!(
            spans.iter().map(|span| span.kind).collect::<Vec<_>>(),
            vec![
                TokenKind::Command,
                TokenKind::Subcommand,
                TokenKind::Command,
                TokenKind::Subcommand,
                TokenKind::Subcommand,
                TokenKind::Argument,
            ]
        );
    }

    #[test]
    fn argument_candidates_follow_the_track_command_tree() {
        assert_eq!(
            argument_candidates("track", &[], "")
                .iter()
                .map(|candidate| candidate.value)
                .collect::<Vec<_>>(),
            vec!["add", "list", "delete", "clear"]
        );
        assert_eq!(
            argument_candidates("track", &["add"], "l"),
            vec![ArgumentCandidate {
                value: "local",
                description: "track a local name",
                kind: "scope",
            }]
        );
    }
}
