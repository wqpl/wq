#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ReplArgKind {
    BuiltinPreset,
    Interpreter,
    HelpTopic,
    DebugFlags,
    BoxSpec,
    FmtMode,
    LoadTarget,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ReplCommandKind {
    Exit,
    Bye,
    Goodbye,
    Highlight,
    Hint,
    Info,
    Dry,
    Fmt,
    Bfn,
    Gb,
    Reset,
    Box,
    BoxSet,
    Backtrace,
    Xray,
    Interpreter,
    Time,
    TimeOneshot,
    Wqdb,
    WqdbOneshot,
    Help,
    DebugShow,
    DebugToggle,
    DebugOneshot,
    DebugSet,
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReplCommandTarget {
    Handled(ReplCommandKind),
    Directive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReplArgStyle {
    Space,
    Suffix,
    SpaceOrSuffix,
}

#[derive(Clone, Copy, Debug)]
struct ReplArgSpec {
    target: ReplCommandTarget,
    kind: ReplArgKind,
    style: ReplArgStyle,
}

#[derive(Clone, Copy, Debug)]
struct ReplUsage {
    name: &'static str,
    desc: &'static str,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ReplCommandSpec {
    aliases: &'static [&'static str],
    desc: &'static str,
    exact: Option<ReplCommandTarget>,
    arg: Option<ReplArgSpec>,
    usages: &'static [ReplUsage],
}

impl ReplCommandSpec {
    pub(super) fn arg_kind(&self) -> Option<ReplArgKind> {
        self.arg.map(|arg| arg.kind)
    }
}

pub(super) enum ParsedReplCommand {
    Empty,
    Unknown,
    Directive,
    Handled {
        kind: ReplCommandKind,
        arg: Option<String>,
    },
}

const fn exact(
    aliases: &'static [&'static str],
    desc: &'static str,
    kind: ReplCommandKind,
) -> ReplCommandSpec {
    ReplCommandSpec {
        aliases,
        desc,
        exact: Some(ReplCommandTarget::Handled(kind)),
        arg: None,
        usages: &[],
    }
}

const fn optional_space_arg(
    aliases: &'static [&'static str],
    desc: &'static str,
    exact: ReplCommandKind,
    arg: ReplCommandKind,
    arg_kind: ReplArgKind,
) -> ReplCommandSpec {
    ReplCommandSpec {
        aliases,
        desc,
        exact: Some(ReplCommandTarget::Handled(exact)),
        arg: Some(ReplArgSpec {
            target: ReplCommandTarget::Handled(arg),
            kind: arg_kind,
            style: ReplArgStyle::Space,
        }),
        usages: &[],
    }
}

const fn directive(
    aliases: &'static [&'static str],
    desc: &'static str,
    arg_kind: Option<ReplArgKind>,
) -> ReplCommandSpec {
    let arg = match arg_kind {
        Some(kind) => Some(ReplArgSpec {
            target: ReplCommandTarget::Directive,
            kind,
            style: ReplArgStyle::Space,
        }),
        None => None,
    };
    ReplCommandSpec {
        aliases,
        desc,
        exact: Some(ReplCommandTarget::Directive),
        arg,
        usages: &[],
    }
}

const fn hint_only(aliases: &'static [&'static str], desc: &'static str) -> ReplCommandSpec {
    ReplCommandSpec {
        aliases,
        desc,
        exact: None,
        arg: None,
        usages: &[],
    }
}

const fn exact_and_space_arg_with_usages(
    aliases: &'static [&'static str],
    desc: &'static str,
    exact: ReplCommandKind,
    arg: ReplCommandKind,
    arg_kind: ReplArgKind,
    usages: &'static [ReplUsage],
) -> ReplCommandSpec {
    ReplCommandSpec {
        aliases,
        desc,
        exact: Some(ReplCommandTarget::Handled(exact)),
        arg: Some(ReplArgSpec {
            target: ReplCommandTarget::Handled(arg),
            kind: arg_kind,
            style: ReplArgStyle::Space,
        }),
        usages,
    }
}

const fn suffix_arg(
    aliases: &'static [&'static str],
    desc: &'static str,
    arg: ReplCommandKind,
    arg_kind: ReplArgKind,
) -> ReplCommandSpec {
    ReplCommandSpec {
        aliases,
        desc,
        exact: None,
        arg: Some(ReplArgSpec {
            target: ReplCommandTarget::Handled(arg),
            kind: arg_kind,
            style: ReplArgStyle::Suffix,
        }),
        usages: &[],
    }
}

const fn exact_and_space_or_suffix_arg_with_usages(
    aliases: &'static [&'static str],
    desc: &'static str,
    exact: ReplCommandKind,
    arg: ReplCommandKind,
    arg_kind: ReplArgKind,
    usages: &'static [ReplUsage],
) -> ReplCommandSpec {
    ReplCommandSpec {
        aliases,
        desc,
        exact: Some(ReplCommandTarget::Handled(exact)),
        arg: Some(ReplArgSpec {
            target: ReplCommandTarget::Handled(arg),
            kind: arg_kind,
            style: ReplArgStyle::SpaceOrSuffix,
        }),
        usages,
    }
}

const BOX_USAGES: &[ReplUsage] = &[
    ReplUsage {
        name: r"\box <spec>",
        desc: "set display config; on/off or +/- modifies",
    },
    ReplUsage {
        name: r"\b <spec>",
        desc: "set display config; on/off or +/- modifies",
    },
];

const DEBUG_USAGES: &[ReplUsage] = &[ReplUsage {
    name: r"\d <spec>",
    desc: "set debug flags; +/- modifies",
}];

const REPL_COMMAND_SPECS: &[ReplCommandSpec] = &[
    exact(
        &[r"\exit", r"\e", r"\\"],
        "exit the repl",
        ReplCommandKind::Exit,
    ),
    exact(&[r"\bye"], "exit the repl", ReplCommandKind::Bye),
    exact(&[r"\goodbye"], "exit with style", ReplCommandKind::Goodbye),
    exact(
        &[r"\highlight", r"\hl"],
        "toggle syntax highlighting",
        ReplCommandKind::Highlight,
    ),
    exact(
        &[r"\highlight?", r"\hl?"],
        "show highlight status",
        ReplCommandKind::HighlightQuery,
    ),
    exact(&[r"\hint"], "toggle hints", ReplCommandKind::Hint),
    exact(&[r"\hint?"], "show hint status", ReplCommandKind::HintQuery),
    exact(&[r"\info"], "show repl info", ReplCommandKind::Info),
    exact(&[r"\dry"], "toggle dry mode", ReplCommandKind::Dry),
    exact(
        &[r"\dry?"],
        "show dry mode status",
        ReplCommandKind::DryQuery,
    ),
    optional_space_arg(
        &[r"\fmt"],
        "toggle formatter",
        ReplCommandKind::Fmt,
        ReplCommandKind::Fmt,
        ReplArgKind::FmtMode,
    ),
    exact(
        &[r"\fmt?"],
        "show formatter status",
        ReplCommandKind::FmtQuery,
    ),
    optional_space_arg(
        &[r"\bfn"],
        "show or set builtins preset",
        ReplCommandKind::Bfn,
        ReplCommandKind::Bfn,
        ReplArgKind::BuiltinPreset,
    ),
    exact(&["\\"], "show builtins preset", ReplCommandKind::Bfn),
    directive(&[r"\p"], "load prelude", None),
    directive(
        &[r"\load", r"\l"],
        "load embedded script or file",
        Some(ReplArgKind::LoadTarget),
    ),
    exact(
        &[r"\gb", r"\g"],
        "show global bindings",
        ReplCommandKind::Gb,
    ),
    exact(&[r"\reset", r"\r"], "reset session", ReplCommandKind::Reset),
    exact_and_space_arg_with_usages(
        &[r"\box", r"\b"],
        "toggle all display config",
        ReplCommandKind::Box,
        ReplCommandKind::BoxSet,
        ReplArgKind::BoxSpec,
        BOX_USAGES,
    ),
    exact(
        &[r"\box?", r"\b?"],
        "show display config",
        ReplCommandKind::BoxQuery,
    ),
    exact(
        &[r"\backtrace", r"\bt"],
        "toggle backtrace",
        ReplCommandKind::Backtrace,
    ),
    exact(
        &[r"\backtrace?", r"\bt?"],
        "show backtrace status",
        ReplCommandKind::BacktraceQuery,
    ),
    exact(&[r"\xray", r"\x"], "toggle xray", ReplCommandKind::Xray),
    exact(
        &[r"\xray?", r"\x?"],
        "show xray status",
        ReplCommandKind::XrayQuery,
    ),
    optional_space_arg(
        &[r"\interpreter", r"\i"],
        "show or set interpreter",
        ReplCommandKind::Interpreter,
        ReplCommandKind::Interpreter,
        ReplArgKind::Interpreter,
    ),
    exact(
        &[r"\time", r"\t"],
        "toggle time mode",
        ReplCommandKind::Time,
    ),
    exact(
        &[r"\time?", r"\t?"],
        "show time mode status",
        ReplCommandKind::TimeQuery,
    ),
    exact(
        &[r"\t.", r"\time."],
        "time mode for next eval",
        ReplCommandKind::TimeOneshot,
    ),
    exact(&[r"\wqdb", r"\w"], "toggle wqdb", ReplCommandKind::Wqdb),
    exact(
        &[r"\wqdb?", r"\w?"],
        "show wqdb status",
        ReplCommandKind::WqdbQuery,
    ),
    exact(
        &[r"\wqdb.", r"\w."],
        "wqdb for next eval",
        ReplCommandKind::WqdbOneshot,
    ),
    optional_space_arg(
        &[r"\help", r"\h"],
        "show help",
        ReplCommandKind::Help,
        ReplCommandKind::Help,
        ReplArgKind::HelpTopic,
    ),
    exact(&[r"\type"], "toggle type mode", ReplCommandKind::TypeShow),
    exact(
        &[r"\type?"],
        "show type mode status",
        ReplCommandKind::TypeQuery,
    ),
    suffix_arg(
        &[r"\d.", r"\debug."],
        "debug flags for next eval",
        ReplCommandKind::DebugOneshot,
        ReplArgKind::DebugFlags,
    ),
    optional_space_arg(
        &[r"\debug"],
        "show debug flags help",
        ReplCommandKind::DebugShow,
        ReplCommandKind::DebugSet,
        ReplArgKind::DebugFlags,
    ),
    exact_and_space_or_suffix_arg_with_usages(
        &[r"\d"],
        "toggle debug flags",
        ReplCommandKind::DebugToggle,
        ReplCommandKind::DebugSet,
        ReplArgKind::DebugFlags,
        DEBUG_USAGES,
    ),
    hint_only(&[r"\exp"], "show or toggle experimental features"),
];

pub(super) fn find_by_alias(alias: &str) -> Option<&'static ReplCommandSpec> {
    REPL_COMMAND_SPECS
        .iter()
        .find(|spec| spec.aliases.contains(&alias))
}

pub(super) fn repl_hint_vectors() -> (Vec<String>, Vec<String>) {
    let mut names = Vec::new();
    let mut descs = Vec::new();
    for spec in REPL_COMMAND_SPECS {
        for alias in spec.aliases {
            names.push((*alias).to_string());
            descs.push(spec.desc.to_string());
        }
        for usage in spec.usages {
            names.push(usage.name.to_string());
            descs.push(usage.desc.to_string());
        }
    }
    (names, descs)
}

pub(super) fn parse(input: &str) -> ParsedReplCommand {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return ParsedReplCommand::Empty;
    }

    for spec in REPL_COMMAND_SPECS {
        for alias in spec.aliases {
            if trimmed == *alias
                && let Some(target) = spec.exact
            {
                return parsed_from_target(target, None);
            }
            if let Some(arg) = spec.arg
                && let Some(value) = parse_arg(trimmed, alias, arg.style)
            {
                return parsed_from_target(arg.target, Some(value));
            }
        }
    }
    ParsedReplCommand::Unknown
}

fn parse_arg(input: &str, alias: &str, style: ReplArgStyle) -> Option<String> {
    let rest = input.strip_prefix(alias)?;
    match style {
        ReplArgStyle::Space => {
            if !rest.chars().next().is_some_and(char::is_whitespace) {
                return None;
            }
            let value = rest.trim();
            (!value.is_empty()).then(|| value.to_string())
        }
        ReplArgStyle::Suffix => Some(rest.to_string()),
        ReplArgStyle::SpaceOrSuffix => {
            if rest.chars().next().is_some_and(char::is_whitespace) {
                let value = rest.trim();
                (!value.is_empty()).then(|| value.to_string())
            } else {
                (!rest.is_empty()).then(|| rest.to_string())
            }
        }
    }
}

fn parsed_from_target(target: ReplCommandTarget, arg: Option<String>) -> ParsedReplCommand {
    match target {
        ReplCommandTarget::Handled(kind) => ParsedReplCommand::Handled { kind, arg },
        ReplCommandTarget::Directive => ParsedReplCommand::Directive,
    }
}
