use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::sync::Arc;

use indexmap::IndexMap;

use crate::builtins::{BuiltinContext, BuiltinEnum, BuiltinFnArgs};
use crate::value::seq::ListStorageSeq;
use crate::value::{Value, WqResult};
use crate::vm::builtin_frame::BuiltinFrameAction;
use crate::wqerror::{Requirement, WqError, WqErrorType};

const SPEC_FIELDS: &[&str] = &["name", "version", "about", "args"];
const ARG_FIELDS: &[&str] = &[
    "name",
    "kind",
    "short",
    "long",
    "help",
    "value_name",
    "parse",
    "default",
    "required",
    "multiple",
    "choices",
    "conflicts",
    "requires",
    "negatable",
    "hidden",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ArgKind {
    Flag,
    Count,
    Option,
    Positional,
}

impl ArgKind {
    fn parse(value: &Value, path: &str) -> WqResult<Self> {
        let Value::Tag(kind) = value else {
            return Err(spec_error(format!("'{path}' must be a tag")));
        };
        match kind.as_ref() {
            "flag" => Ok(Self::Flag),
            "count" => Ok(Self::Count),
            "option" => Ok(Self::Option),
            "positional" => Ok(Self::Positional),
            other => Err(spec_error(format!(
                "'{path}' must be \"flag\", \"count\", \"option\", or \"positional\"; got \"{other}\""
            ))),
        }
    }

    fn takes_value(self) -> bool {
        matches!(self, Self::Option | Self::Positional)
    }
}

#[derive(Clone)]
struct ArgSpec {
    name: Arc<str>,
    kind: ArgKind,
    short: Option<char>,
    long: Option<String>,
    help: Option<String>,
    value_name: Option<String>,
    parser: Option<Value>,
    default: Option<Value>,
    required: bool,
    multiple: bool,
    choices: Option<Vec<Value>>,
    conflicts: Vec<Arc<str>>,
    requires: Vec<Arc<str>>,
    negatable: bool,
    hidden: bool,
}

#[derive(Clone)]
struct CliSpec {
    name: String,
    version: Option<String>,
    about: Option<String>,
    args: Vec<ArgSpec>,
}

#[derive(Default)]
struct CollectedArg {
    values: Vec<Value>,
    occurrences: usize,
}

struct UsageError {
    code: &'static str,
    message: String,
    token: Option<String>,
    arg: Option<Arc<str>>,
}

enum ParseOutcome {
    Ok(Value),
    Help,
    Version,
    Error(UsageError),
}

trait CliCallback {
    fn call(&mut self, parser: &Value, args: BuiltinFnArgs) -> WqResult<Value>;
}

impl<T: BuiltinContext + ?Sized> CliCallback for T {
    fn call(&mut self, parser: &Value, args: BuiltinFnArgs) -> WqResult<Value> {
        BuiltinContext::call(self, parser, args)
    }
}

pub(super) fn argv(vm: &mut dyn BuiltinContext, _args: BuiltinFnArgs) -> WqResult<Value> {
    Ok(Value::List(Arc::new(
        vm.argv()
            .iter()
            .map(|arg| Value::String(Arc::new(arg.clone())))
            .collect(),
    )))
}

pub(super) fn argparse(vm: &mut dyn BuiltinContext, args: BuiltinFnArgs) -> WqResult<Value> {
    let spec = parse_cli_spec(&args[0]).map_err(|error| error.src(BuiltinEnum::Argparse))?;
    let argv = parse_argv_value(&args[1]).map_err(|error| error.src(BuiltinEnum::Argparse))?;
    let outcome = parse_args(vm, &spec, &argv).map_err(|error| error.src(BuiltinEnum::Argparse))?;
    Ok(outcome_value(&spec, outcome))
}

pub(super) fn cliargs(vm: &mut dyn BuiltinContext, args: BuiltinFnArgs) -> WqResult<Value> {
    let spec = parse_cli_spec(&args[0]).map_err(|error| error.src(BuiltinEnum::Cliargs))?;
    let argv = vm.argv().to_vec();
    match parse_args(vm, &spec, &argv).map_err(|error| error.src(BuiltinEnum::Cliargs))? {
        ParseOutcome::Ok(value) => Ok(value),
        ParseOutcome::Help => {
            write_stdout(vm, &render_help(&spec))?;
            vm.request_halt(0);
            Ok(Value::empty_list())
        }
        ParseOutcome::Version => {
            write_stdout(vm, &render_version(&spec))?;
            vm.request_halt(0);
            Ok(Value::empty_list())
        }
        ParseOutcome::Error(error) => {
            write_stderr(vm, &render_usage_error(&spec, &error))?;
            vm.request_halt(2);
            Ok(Value::empty_list())
        }
    }
}

fn write_stdout(vm: &dyn BuiltinContext, text: &str) -> WqResult<()> {
    vm.write_stdout_line(text).map_err(|error| {
        WqError::new(WqErrorType::Io)
            .src(BuiltinEnum::Cliargs)
            .attach_note(format!("host I/O error: {error}"))
    })
}

fn write_stderr(vm: &dyn BuiltinContext, text: &str) -> WqResult<()> {
    vm.write_stderr_line(text).map_err(|error| {
        WqError::new(WqErrorType::Io)
            .src(BuiltinEnum::Cliargs)
            .attach_note(format!("host I/O error: {error}"))
    })
}

fn parse_cli_spec(value: &Value) -> WqResult<CliSpec> {
    let fields = expect_dict(value, "spec")?;
    reject_unknown_fields(fields, SPEC_FIELDS, "spec")?;
    let name = required_string(fields, "name", "spec.name")?;
    if name.is_empty() {
        return Err(spec_error("'spec.name' must not be empty"));
    }
    let version = optional_string(fields, "version", "spec.version")?;
    let about = optional_string(fields, "about", "spec.about")?;
    let args = match fields.get("args") {
        None => Vec::new(),
        Some(value) if value.is_unit() => Vec::new(),
        Some(Value::List(items)) => items
            .iter()
            .enumerate()
            .map(|(index, item)| parse_arg_spec(item, index))
            .collect::<WqResult<Vec<_>>>()?,
        Some(_) => return Err(spec_error("'spec.args' must be a list of dicts")),
    };
    let spec = CliSpec {
        name,
        version,
        about,
        args,
    };
    validate_cli_spec(&spec)?;
    Ok(spec)
}

fn parse_arg_spec(value: &Value, index: usize) -> WqResult<ArgSpec> {
    let path = format!("spec.args[{index}]");
    let fields = expect_dict(value, &path)?;
    reject_unknown_fields(fields, ARG_FIELDS, &path)?;
    let name = required_tag(fields, "name", &format!("{path}.name"))?;
    let kind_value = fields
        .get("kind")
        .ok_or_else(|| spec_error(format!("'{path}.kind' is required")))?;
    let kind = ArgKind::parse(kind_value, &format!("{path}.kind"))?;
    let short = match fields.get("short") {
        None => None,
        Some(Value::Char(value)) => Some(*value),
        Some(_) => return Err(spec_error(format!("'{path}.short' must be a char"))),
    };
    let long = match fields.get("long") {
        Some(value) => Some(expect_string(value, &format!("{path}.long"))?),
        None if kind == ArgKind::Positional => None,
        None => Some(name.replace('_', "-")),
    };
    let help = optional_string(fields, "help", &format!("{path}.help"))?;
    let value_name = optional_string(fields, "value_name", &format!("{path}.value_name"))?;
    let parser = fields.get("parse").cloned();
    let default = fields.get("default").cloned();
    let required = optional_bool(fields, "required", false, &format!("{path}.required"))?;
    let multiple = optional_bool(fields, "multiple", false, &format!("{path}.multiple"))?;
    let choices = optional_value_list(fields, "choices", &format!("{path}.choices"))?;
    let conflicts = optional_tag_list(fields, "conflicts", &format!("{path}.conflicts"))?;
    let requires = optional_tag_list(fields, "requires", &format!("{path}.requires"))?;
    let negatable = optional_bool(fields, "negatable", false, &format!("{path}.negatable"))?;
    let hidden = optional_bool(fields, "hidden", false, &format!("{path}.hidden"))?;

    if let Some(long) = &long
        && (long.is_empty()
            || long.starts_with('-')
            || long.chars().any(|ch| ch.is_whitespace() || ch == '='))
    {
        return Err(spec_error(format!(
            "'{path}.long' must not be empty, start with '-', or contain '=' or whitespace"
        )));
    }
    if let Some(short) = short
        && (matches!(short, '-' | '=') || short.is_whitespace())
    {
        return Err(spec_error(format!(
            "'{path}.short' cannot be '-', '=', or whitespace"
        )));
    }
    if kind == ArgKind::Positional && (short.is_some() || fields.contains_key("long")) {
        return Err(spec_error(format!(
            "'{path}' positional arguments cannot define short or long options"
        )));
    }
    if !kind.takes_value() && parser.is_some() {
        return Err(spec_error(format!(
            "'{path}.parse' is only valid for options and positionals"
        )));
    }
    if !kind.takes_value() && value_name.is_some() {
        return Err(spec_error(format!(
            "'{path}.value_name' is only valid for options and positionals"
        )));
    }
    if !kind.takes_value() && choices.is_some() {
        return Err(spec_error(format!(
            "'{path}.choices' is only valid for options and positionals"
        )));
    }
    if let Some(parser) = &parser
        && !parser.is_runtime_callable()
    {
        return Err(WqError::new(WqErrorType::Domain)
            .expected(Requirement::CALLABLE)
            .attach_note(format!("at configuration field '{path}.parse'"))
            .got1(parser));
    }
    if !matches!(kind, ArgKind::Option | ArgKind::Positional) && multiple {
        return Err(spec_error(format!(
            "'{path}.multiple' is only valid for options and positionals"
        )));
    }
    if !matches!(kind, ArgKind::Option | ArgKind::Positional) && required {
        return Err(spec_error(format!(
            "'{path}.required' is only valid for options and positionals"
        )));
    }
    if negatable && kind != ArgKind::Flag {
        return Err(spec_error(format!(
            "'{path}.negatable' is only valid for flags"
        )));
    }
    if required && default.is_some() {
        return Err(spec_error(format!(
            "'{path}' cannot combine 'required' with 'default'"
        )));
    }
    if let Some(default) = &default {
        match kind {
            ArgKind::Flag if !matches!(default, Value::Bool(_)) => {
                return Err(spec_error(format!("'{path}.default' must be a bool")));
            }
            ArgKind::Count if !matches!(default, Value::Int(_)) => {
                return Err(spec_error(format!("'{path}.default' must be an int")));
            }
            _ if multiple && ListStorageSeq::from_value(default).is_none() => {
                return Err(spec_error(format!(
                    "'{path}.default' must be a list when 'multiple' is true"
                )));
            }
            _ => {}
        }
        if let Some(choices) = &choices {
            let valid = if multiple {
                ListStorageSeq::from_value(default)
                    .expect("multiple defaults were validated as lists")
                    .values()
                    .all(|value| choices.contains(&value))
            } else {
                choices.contains(default)
            };
            if !valid {
                return Err(spec_error(format!(
                    "'{path}.default' contains a value not included in 'choices'"
                )));
            }
        }
    }

    Ok(ArgSpec {
        name,
        kind,
        short,
        long,
        help,
        value_name,
        parser,
        default,
        required,
        multiple,
        choices,
        conflicts,
        requires,
        negatable,
        hidden,
    })
}

fn validate_cli_spec(spec: &CliSpec) -> WqResult<()> {
    let mut names = HashSet::new();
    let mut shorts = HashSet::new();
    let mut longs = HashSet::new();
    let mut optional_positional_seen = false;
    let mut multiple_positional_seen = false;

    for arg in &spec.args {
        if !names.insert(arg.name.to_string()) {
            return Err(spec_error(format!(
                "duplicate argument name '{}'",
                arg.name
            )));
        }
        if let Some(short) = arg.short {
            if matches!(short, 'h' | 'V') {
                return Err(spec_error(format!("short option '-{short}' is reserved")));
            }
            if !shorts.insert(short) {
                return Err(spec_error(format!("duplicate short option '-{short}'")));
            }
        }
        if let Some(long) = &arg.long {
            if long == "help" || long == "version" {
                return Err(spec_error(format!("long option '--{long}' is reserved")));
            }
            if !longs.insert(long.clone()) {
                return Err(spec_error(format!("duplicate long option '--{long}'")));
            }
        }
        if arg.kind == ArgKind::Positional {
            if multiple_positional_seen {
                return Err(spec_error(
                    "a multiple positional argument must be the final positional",
                ));
            }
            if optional_positional_seen && arg.required {
                return Err(spec_error(
                    "a required positional cannot follow an optional positional",
                ));
            }
            optional_positional_seen |= !arg.required;
            multiple_positional_seen |= arg.multiple;
        }
    }

    for arg in &spec.args {
        for referenced in arg.conflicts.iter().chain(&arg.requires) {
            if referenced == &arg.name {
                return Err(spec_error(format!(
                    "argument '{}' cannot reference itself",
                    arg.name
                )));
            }
            if !names.contains(referenced.as_ref()) {
                return Err(spec_error(format!(
                    "argument '{}' references unknown argument '{referenced}'",
                    arg.name
                )));
            }
        }
    }
    Ok(())
}

fn parse_argv_value(value: &Value) -> WqResult<Vec<String>> {
    if value.is_unit() {
        return Ok(Vec::new());
    }
    let Value::List(items) = value else {
        return Err(WqError::new(WqErrorType::Domain)
            .expected(Requirement::list(Requirement::STRING))
            .at_arg(1)
            .got1(value));
    };
    items
        .iter()
        .enumerate()
        .map(|(index, item)| match item {
            Value::String(value) => Ok(value.to_string()),
            _ => Err(WqError::new(WqErrorType::Domain)
                .expected(Requirement::STRING)
                .at_arg(1)
                .got_at_index(item, index)),
        })
        .collect()
}

fn parse_args(
    vm: &mut (impl CliCallback + ?Sized),
    spec: &CliSpec,
    argv: &[String],
) -> WqResult<ParseOutcome> {
    let mut long_options = HashMap::new();
    let mut short_options = HashMap::new();
    let mut positionals = Vec::new();
    for (index, arg) in spec.args.iter().enumerate() {
        if let Some(long) = &arg.long {
            long_options.insert(long.as_str(), index);
        }
        if let Some(short) = arg.short {
            short_options.insert(short, index);
        }
        if arg.kind == ArgKind::Positional {
            positionals.push(index);
        }
    }

    let mut collected: Vec<CollectedArg> = (0..spec.args.len())
        .map(|_| CollectedArg::default())
        .collect();
    let mut options_enabled = true;
    let mut positional_cursor = 0usize;
    let mut index = 0usize;
    while index < argv.len() {
        let token = &argv[index];
        if options_enabled && token == "--" {
            options_enabled = false;
            index += 1;
            continue;
        }
        if options_enabled && matches!(token.as_str(), "-h" | "--help") {
            return Ok(ParseOutcome::Help);
        }
        if options_enabled && spec.version.is_some() && matches!(token.as_str(), "-V" | "--version")
        {
            return Ok(ParseOutcome::Version);
        }
        if options_enabled && token.starts_with("--") && token.len() > 2 {
            let body = &token[2..];
            let (name, inline_value) = body
                .split_once('=')
                .map_or((body, None), |(name, value)| (name, Some(value)));
            let mut negated = false;
            let option_index = if let Some(option_index) = long_options.get(name) {
                Some(*option_index)
            } else if let Some(positive) = name.strip_prefix("no-") {
                long_options.get(positive).copied().filter(|option_index| {
                    spec.args[*option_index].kind == ArgKind::Flag
                        && spec.args[*option_index].negatable
                        && {
                            negated = true;
                            true
                        }
                })
            } else {
                None
            };
            let Some(option_index) = option_index else {
                return Ok(ParseOutcome::Error(usage_error(
                    "unknown_option",
                    format!("unknown option '{token}'"),
                    Some(token.clone()),
                    None,
                )));
            };
            let arg = &spec.args[option_index];
            match arg.kind {
                ArgKind::Flag => {
                    if inline_value.is_some() {
                        return Ok(ParseOutcome::Error(usage_error(
                            "unexpected_value",
                            format!("option '--{name}' does not take a value"),
                            Some(token.clone()),
                            Some(Arc::clone(&arg.name)),
                        )));
                    }
                    if let Err(error) = set_single(
                        &mut collected[option_index],
                        Value::Bool(!negated),
                        arg,
                        token,
                    ) {
                        return Ok(ParseOutcome::Error(error));
                    }
                }
                ArgKind::Count => {
                    if inline_value.is_some() {
                        return Ok(ParseOutcome::Error(usage_error(
                            "unexpected_value",
                            format!("option '--{name}' does not take a value"),
                            Some(token.clone()),
                            Some(Arc::clone(&arg.name)),
                        )));
                    }
                    increment_count(&mut collected[option_index]);
                }
                ArgKind::Option => {
                    let raw = if let Some(value) = inline_value {
                        value
                    } else {
                        index += 1;
                        let Some(value) = argv.get(index) else {
                            return Ok(ParseOutcome::Error(missing_value(arg, token)));
                        };
                        value
                    };
                    let value = match convert_value(vm, arg, raw) {
                        Ok(value) => value,
                        Err(error) => return Ok(ParseOutcome::Error(error)),
                    };
                    if let Some(error) =
                        collect_value(&mut collected[option_index], value, arg, token)
                    {
                        return Ok(ParseOutcome::Error(error));
                    }
                }
                ArgKind::Positional => unreachable!("positionals are not long options"),
            }
            index += 1;
            continue;
        }
        if options_enabled && token.starts_with('-') && token != "-" {
            for (offset, short) in token[1..].char_indices() {
                let Some(option_index) = short_options.get(&short).copied() else {
                    return Ok(ParseOutcome::Error(usage_error(
                        "unknown_option",
                        format!("unknown short option '-{short}' in '{token}'"),
                        Some(token.clone()),
                        None,
                    )));
                };
                let arg = &spec.args[option_index];
                match arg.kind {
                    ArgKind::Flag => {
                        if let Err(error) =
                            set_single(&mut collected[option_index], Value::Bool(true), arg, token)
                        {
                            return Ok(ParseOutcome::Error(error));
                        }
                    }
                    ArgKind::Count => increment_count(&mut collected[option_index]),
                    ArgKind::Option => {
                        let remainder_start = 1 + offset + short.len_utf8();
                        let remainder = token[remainder_start..]
                            .strip_prefix('=')
                            .unwrap_or(&token[remainder_start..]);
                        let raw = if remainder.is_empty() {
                            index += 1;
                            let Some(value) = argv.get(index) else {
                                return Ok(ParseOutcome::Error(missing_value(arg, token)));
                            };
                            value.as_str()
                        } else {
                            remainder
                        };
                        let value = match convert_value(vm, arg, raw) {
                            Ok(value) => value,
                            Err(error) => return Ok(ParseOutcome::Error(error)),
                        };
                        if let Some(error) =
                            collect_value(&mut collected[option_index], value, arg, token)
                        {
                            return Ok(ParseOutcome::Error(error));
                        }
                        break;
                    }
                    ArgKind::Positional => unreachable!("positionals are not short options"),
                }
            }
            index += 1;
            continue;
        }

        let Some(positional_index) = positionals.get(positional_cursor).copied() else {
            return Ok(ParseOutcome::Error(usage_error(
                "unexpected_argument",
                format!("unexpected positional argument \"{token}\""),
                Some(token.clone()),
                None,
            )));
        };
        let arg = &spec.args[positional_index];
        let value = match convert_value(vm, arg, token) {
            Ok(value) => value,
            Err(error) => return Ok(ParseOutcome::Error(error)),
        };
        if let Some(error) = collect_value(&mut collected[positional_index], value, arg, token) {
            return Ok(ParseOutcome::Error(error));
        }
        if !arg.multiple {
            positional_cursor += 1;
        }
        index += 1;
    }

    for (arg, collected_arg) in spec.args.iter().zip(&collected) {
        if arg.required && collected_arg.occurrences == 0 {
            return Ok(ParseOutcome::Error(usage_error(
                "missing_argument",
                format!("missing required argument '{}'", display_arg(arg)),
                None,
                Some(Arc::clone(&arg.name)),
            )));
        }
        if collected_arg.occurrences > 0 {
            for conflict in &arg.conflicts {
                let conflict_index = spec
                    .args
                    .iter()
                    .position(|candidate| candidate.name == *conflict)
                    .expect("validated conflict name");
                if collected[conflict_index].occurrences > 0 {
                    return Ok(ParseOutcome::Error(usage_error(
                        "conflict",
                        format!(
                            "argument '{}' conflicts with '{}'",
                            display_arg(arg),
                            display_arg(&spec.args[conflict_index])
                        ),
                        None,
                        Some(Arc::clone(&arg.name)),
                    )));
                }
            }
            for required in &arg.requires {
                let required_index = spec
                    .args
                    .iter()
                    .position(|candidate| candidate.name == *required)
                    .expect("validated requirement name");
                if collected[required_index].occurrences == 0 {
                    return Ok(ParseOutcome::Error(usage_error(
                        "requires",
                        format!(
                            "argument '{}' requires '{}'",
                            display_arg(arg),
                            display_arg(&spec.args[required_index])
                        ),
                        None,
                        Some(Arc::clone(&arg.name)),
                    )));
                }
            }
        }
    }

    let mut values = IndexMap::with_capacity(spec.args.len());
    for (arg, collected_arg) in spec.args.iter().zip(collected) {
        let value = if collected_arg.occurrences == 0 {
            arg.default.clone().unwrap_or_else(|| match arg.kind {
                ArgKind::Flag => Value::Bool(false),
                ArgKind::Count => Value::Int(0),
                _ if arg.multiple => Value::List(Arc::new(Vec::new())),
                _ => Value::empty_list(),
            })
        } else {
            match arg.kind {
                ArgKind::Count => Value::Int(
                    i64::try_from(collected_arg.occurrences)
                        .map_err(|_| spec_error("count option occurred too many times"))?,
                ),
                _ if arg.multiple => Value::from_items(collected_arg.values),
                _ => collected_arg
                    .values
                    .into_iter()
                    .next()
                    .expect("a seen argument has a value"),
            }
        };
        values.insert(Arc::clone(&arg.name), value);
    }
    Ok(ParseOutcome::Ok(value_dict([
        ("args", Value::Dict(Arc::new(values))),
        ("command", Value::empty_list()),
    ])))
}

fn set_single(
    collected: &mut CollectedArg,
    value: Value,
    arg: &ArgSpec,
    token: &str,
) -> Result<(), UsageError> {
    if collected.occurrences > 0 {
        return Err(duplicate_error(arg, token));
    }
    collected.values.push(value);
    collected.occurrences = 1;
    Ok(())
}

fn increment_count(collected: &mut CollectedArg) {
    collected.occurrences = collected.occurrences.saturating_add(1);
}

fn collect_value(
    collected: &mut CollectedArg,
    value: Value,
    arg: &ArgSpec,
    token: &str,
) -> Option<UsageError> {
    if !arg.multiple && collected.occurrences > 0 {
        return Some(duplicate_error(arg, token));
    }
    collected.values.push(value);
    collected.occurrences = collected.occurrences.saturating_add(1);
    None
}

fn convert_value(
    vm: &mut (impl CliCallback + ?Sized),
    arg: &ArgSpec,
    raw: &str,
) -> Result<Value, UsageError> {
    let raw_value = string_value(raw);
    let value = if let Some(parser) = &arg.parser {
        match vm.call(parser, BuiltinFnArgs::from(raw_value)) {
            Ok(value) => value,
            Err(error) => {
                let detail = error
                    .msg
                    .as_deref()
                    .unwrap_or_else(|| error.err_type.name());
                return Err(usage_error(
                    "invalid_value",
                    format!(
                        "invalid value \"{raw}\" for '{}': {detail}",
                        display_arg(arg)
                    ),
                    Some(raw.to_string()),
                    Some(Arc::clone(&arg.name)),
                ));
            }
        }
    } else {
        raw_value
    };
    if let Some(choices) = &arg.choices
        && !choices.contains(&value)
    {
        return Err(usage_error(
            "invalid_choice",
            format!(
                "value \"{raw}\" for '{}' is not one of the allowed choices",
                display_arg(arg)
            ),
            Some(raw.to_string()),
            Some(Arc::clone(&arg.name)),
        ));
    }
    Ok(value)
}

pub(crate) struct ArgparseFrame {
    spec: CliSpec,
    argv: Vec<String>,
    callback_values: Vec<WqResult<Value>>,
    callback_result: Option<WqResult<Value>>,
}

impl ArgparseFrame {
    pub(crate) fn new(args: &BuiltinFnArgs) -> WqResult<Self> {
        let spec = parse_cli_spec(&args[0]).map_err(|error| error.src(BuiltinEnum::Argparse))?;
        let argv = parse_argv_value(&args[1]).map_err(|error| error.src(BuiltinEnum::Argparse))?;
        Ok(Self {
            spec,
            argv,
            callback_values: Vec::new(),
            callback_result: None,
        })
    }

    pub(crate) fn accept_callback_result(&mut self, value: Value) {
        self.callback_result = Some(Ok(value));
    }

    pub(crate) fn accept_callback_error(&mut self, error: WqError) {
        self.callback_result = Some(Err(error));
    }

    pub(crate) fn step(&mut self) -> WqResult<BuiltinFrameAction> {
        if let Some(value) = self.callback_result.take() {
            self.callback_values.push(value);
            return Ok(BuiltinFrameAction::Continue);
        }

        let mut replay = ReplayCliCallbacks {
            values: &self.callback_values,
            next: 0,
            request: None,
        };
        let outcome = parse_args(&mut replay, &self.spec, &self.argv)
            .map_err(|error| error.src(BuiltinEnum::Argparse))?;
        if let Some((func, args)) = replay.request {
            return Ok(BuiltinFrameAction::Call { func, args });
        }
        Ok(BuiltinFrameAction::Ready(outcome_value(
            &self.spec, outcome,
        )))
    }
}

struct ReplayCliCallbacks<'a> {
    values: &'a [WqResult<Value>],
    next: usize,
    request: Option<(Value, BuiltinFnArgs)>,
}

impl CliCallback for ReplayCliCallbacks<'_> {
    fn call(&mut self, parser: &Value, args: BuiltinFnArgs) -> WqResult<Value> {
        if let Some(value) = self.values.get(self.next) {
            self.next += 1;
            return value.clone();
        }
        debug_assert!(self.request.is_none());
        self.request = Some((parser.clone(), args));
        Err(WqError::new(WqErrorType::Vm).msg("argparse callback is pending"))
    }
}

pub(crate) struct CliargsFrame {
    spec: CliSpec,
    argv: Vec<String>,
    callback_values: Vec<WqResult<Value>>,
    callback_result: Option<WqResult<Value>>,
}

impl CliargsFrame {
    pub(crate) fn new(args: &BuiltinFnArgs, argv: Vec<String>) -> WqResult<Self> {
        let spec = parse_cli_spec(&args[0]).map_err(|error| error.src(BuiltinEnum::Cliargs))?;
        Ok(Self {
            spec,
            argv,
            callback_values: Vec::new(),
            callback_result: None,
        })
    }

    pub(crate) fn accept_callback_result(&mut self, value: Value) {
        self.callback_result = Some(Ok(value));
    }

    pub(crate) fn accept_callback_error(&mut self, error: WqError) {
        self.callback_result = Some(Err(error));
    }

    pub(crate) fn step(&mut self) -> WqResult<BuiltinFrameAction> {
        if let Some(value) = self.callback_result.take() {
            self.callback_values.push(value);
            return Ok(BuiltinFrameAction::Continue);
        }

        let mut replay = ReplayCliCallbacks {
            values: &self.callback_values,
            next: 0,
            request: None,
        };
        let outcome = parse_args(&mut replay, &self.spec, &self.argv)
            .map_err(|error| error.src(BuiltinEnum::Cliargs))?;
        if let Some((func, args)) = replay.request {
            return Ok(BuiltinFrameAction::Call { func, args });
        }
        Ok(match outcome {
            ParseOutcome::Ok(value) => BuiltinFrameAction::Ready(value),
            ParseOutcome::Help => BuiltinFrameAction::HostComplete {
                text: render_help(&self.spec),
                stderr: false,
                status: Some(0),
            },
            ParseOutcome::Version => BuiltinFrameAction::HostComplete {
                text: render_version(&self.spec),
                stderr: false,
                status: Some(0),
            },
            ParseOutcome::Error(error) => BuiltinFrameAction::HostComplete {
                text: render_usage_error(&self.spec, &error),
                stderr: true,
                status: Some(2),
            },
        })
    }
}

fn outcome_value(spec: &CliSpec, outcome: ParseOutcome) -> Value {
    match outcome {
        ParseOutcome::Ok(value) => value_dict([
            ("kind", Value::Tag(Arc::from("ok"))),
            ("status", Value::Int(0)),
            ("value", value),
        ]),
        ParseOutcome::Help => value_dict([
            ("kind", Value::Tag(Arc::from("help"))),
            ("status", Value::Int(0)),
            ("text", string_value(render_help(spec))),
        ]),
        ParseOutcome::Version => value_dict([
            ("kind", Value::Tag(Arc::from("version"))),
            ("status", Value::Int(0)),
            ("text", string_value(render_version(spec))),
        ]),
        ParseOutcome::Error(error) => value_dict([
            ("kind", Value::Tag(Arc::from("error"))),
            ("status", Value::Int(2)),
            ("text", string_value(render_usage_error(spec, &error))),
            ("error", usage_error_value(error)),
        ]),
    }
}

fn usage_error_value(error: UsageError) -> Value {
    value_dict([
        ("code", Value::Tag(Arc::from(error.code))),
        ("message", string_value(error.message)),
        (
            "token",
            error
                .token
                .map(string_value)
                .unwrap_or_else(Value::empty_list),
        ),
        (
            "arg",
            error.arg.map(Value::Tag).unwrap_or_else(Value::empty_list),
        ),
    ])
}

fn render_help(spec: &CliSpec) -> String {
    let mut output = String::new();
    let _ = writeln!(output, "{}", usage_line(spec));
    if let Some(about) = &spec.about {
        let _ = writeln!(output, "\n{about}");
    }
    let visible_positionals: Vec<_> = spec
        .args
        .iter()
        .filter(|arg| arg.kind == ArgKind::Positional && !arg.hidden)
        .collect();
    if !visible_positionals.is_empty() {
        let _ = writeln!(output, "\nArguments:");
        write_help_rows(&mut output, &visible_positionals);
    }
    let visible_options: Vec<_> = spec
        .args
        .iter()
        .filter(|arg| arg.kind != ArgKind::Positional && !arg.hidden)
        .collect();
    let _ = writeln!(output, "\nOptions:");
    write_help_rows(&mut output, &visible_options);
    let _ = writeln!(output, "  -h, --help  Print help");
    if spec.version.is_some() {
        let _ = writeln!(output, "  -V, --version  Print version");
    }
    output.trim_end().to_string()
}

fn write_help_rows(output: &mut String, args: &[&ArgSpec]) {
    for arg in args {
        let label = help_label(arg);
        if let Some(help) = &arg.help {
            let _ = writeln!(output, "  {label}  {help}");
        } else {
            let _ = writeln!(output, "  {label}");
        }
    }
}

fn usage_line(spec: &CliSpec) -> String {
    let mut usage = format!("Usage: {}", spec.name);
    if spec.args.iter().any(|arg| arg.kind != ArgKind::Positional) {
        usage.push_str(" [OPTIONS]");
    }
    for arg in spec
        .args
        .iter()
        .filter(|arg| arg.kind == ArgKind::Positional)
    {
        usage.push(' ');
        let name = value_name(arg);
        if arg.required {
            let _ = write!(usage, "<{name}>");
        } else {
            let _ = write!(usage, "[<{name}>]");
        }
        if arg.multiple {
            usage.push_str("...");
        }
    }
    usage
}

fn help_label(arg: &ArgSpec) -> String {
    if arg.kind == ArgKind::Positional {
        let mut label = format!("<{}>", value_name(arg));
        if arg.multiple {
            label.push_str("...");
        }
        return label;
    }
    let mut label = String::new();
    if let Some(short) = arg.short {
        let _ = write!(label, "-{short}");
        if arg.long.is_some() {
            label.push_str(", ");
        }
    }
    if let Some(long) = &arg.long {
        let _ = write!(label, "--{long}");
    }
    if arg.kind == ArgKind::Option {
        let _ = write!(label, " <{}>", value_name(arg));
    }
    label
}

fn value_name(arg: &ArgSpec) -> String {
    arg.value_name
        .clone()
        .unwrap_or_else(|| arg.name.to_uppercase())
}

fn render_version(spec: &CliSpec) -> String {
    match &spec.version {
        Some(version) => format!("{} {version}", spec.name),
        None => spec.name.clone(),
    }
}

fn render_usage_error(spec: &CliSpec, error: &UsageError) -> String {
    format!(
        "error: {}\n\n{}\n\nFor more information, try '--help'.",
        error.message,
        usage_line(spec)
    )
}

fn display_arg(arg: &ArgSpec) -> String {
    match (&arg.long, arg.short) {
        (Some(long), _) => format!("--{long}"),
        (None, Some(short)) => format!("-{short}"),
        (None, None) => format!("<{}>", value_name(arg)),
    }
}

fn missing_value(arg: &ArgSpec, token: &str) -> UsageError {
    usage_error(
        "missing_value",
        format!("option '{}' requires a value", display_arg(arg)),
        Some(token.to_string()),
        Some(Arc::clone(&arg.name)),
    )
}

fn duplicate_error(arg: &ArgSpec, token: &str) -> UsageError {
    usage_error(
        "duplicate_argument",
        format!(
            "argument '{}' cannot be used more than once",
            display_arg(arg)
        ),
        Some(token.to_string()),
        Some(Arc::clone(&arg.name)),
    )
}

fn usage_error(
    code: &'static str,
    message: String,
    token: Option<String>,
    arg: Option<Arc<str>>,
) -> UsageError {
    UsageError {
        code,
        message,
        token,
        arg,
    }
}

fn expect_dict<'a>(value: &'a Value, path: &str) -> WqResult<&'a IndexMap<Arc<str>, Value>> {
    let Value::Dict(fields) = value else {
        return Err(spec_error(format!("'{path}' must be a dict")));
    };
    Ok(fields)
}

fn reject_unknown_fields(
    fields: &IndexMap<Arc<str>, Value>,
    allowed: &[&str],
    path: &str,
) -> WqResult<()> {
    if let Some(field) = fields
        .keys()
        .find(|field| !allowed.contains(&field.as_ref()))
    {
        return Err(spec_error(format!("'{path}' has unknown field '{field}'")));
    }
    Ok(())
}

fn required_string(fields: &IndexMap<Arc<str>, Value>, name: &str, path: &str) -> WqResult<String> {
    let value = fields
        .get(name)
        .ok_or_else(|| spec_error(format!("'{path}' is required")))?;
    expect_string(value, path)
}

fn optional_string(
    fields: &IndexMap<Arc<str>, Value>,
    name: &str,
    path: &str,
) -> WqResult<Option<String>> {
    fields
        .get(name)
        .map(|value| expect_string(value, path))
        .transpose()
}

fn expect_string(value: &Value, path: &str) -> WqResult<String> {
    match value {
        Value::String(value) => Ok(value.to_string()),
        _ => Err(spec_error(format!("'{path}' must be a string"))),
    }
}

fn required_tag(fields: &IndexMap<Arc<str>, Value>, name: &str, path: &str) -> WqResult<Arc<str>> {
    match fields.get(name) {
        Some(Value::Tag(value)) => Ok(Arc::clone(value)),
        Some(_) => Err(spec_error(format!("'{path}' must be a tag"))),
        None => Err(spec_error(format!("'{path}' is required"))),
    }
}

fn optional_bool(
    fields: &IndexMap<Arc<str>, Value>,
    name: &str,
    default: bool,
    path: &str,
) -> WqResult<bool> {
    match fields.get(name) {
        None => Ok(default),
        Some(Value::Bool(value)) => Ok(*value),
        Some(_) => Err(spec_error(format!("'{path}' must be a bool"))),
    }
}

fn optional_value_list(
    fields: &IndexMap<Arc<str>, Value>,
    name: &str,
    path: &str,
) -> WqResult<Option<Vec<Value>>> {
    match fields.get(name) {
        None => Ok(None),
        Some(value) => ListStorageSeq::from_value(value)
            .map(|values| Some(values.to_values_vec()))
            .ok_or_else(|| spec_error(format!("'{path}' must be a list"))),
    }
}

fn optional_tag_list(
    fields: &IndexMap<Arc<str>, Value>,
    name: &str,
    path: &str,
) -> WqResult<Vec<Arc<str>>> {
    match fields.get(name) {
        None => Ok(Vec::new()),
        Some(value) => ListStorageSeq::from_value(value)
            .ok_or_else(|| spec_error(format!("'{path}' must be a list of tags")))?
            .values()
            .enumerate()
            .map(|(index, value)| match value {
                Value::Tag(value) => Ok(value),
                _ => Err(spec_error(format!("'{path}[{index}]' must be a tag"))),
            })
            .collect(),
    }
}

fn value_dict<const N: usize>(entries: [(&str, Value); N]) -> Value {
    let mut values = IndexMap::with_capacity(N);
    for (key, value) in entries {
        values.insert(Arc::from(key), value);
    }
    Value::Dict(Arc::new(values))
}

fn string_value(value: impl Into<String>) -> Value {
    Value::String(Arc::new(value.into()))
}

fn spec_error(message: impl Into<String>) -> WqError {
    WqError::new(WqErrorType::Domain).msg(message.into())
}

#[cfg(test)]
mod tests {
    use super::parse_argv_value;
    use crate::session::Session;
    use crate::session::stdio::{WqIoError, WqOutput};
    use crate::style::ColorMode;
    use crate::value::Value;

    const SPEC: &str = r#"(`name:"rgrep";`version:"1.0";`about:"search files";`args:((`name:`ignore_case;`kind:`flag;`short:"i";`help:"ignore case");(`name:`max_count;`kind:`option;`short:"m";`parse:int;`value_name:,"N");(`name:`pattern;`kind:`positional;`required:T);(`name:`paths;`kind:`positional;`required:T;`multiple:T)))"#;

    struct SinkOutput;

    impl WqOutput for SinkOutput {
        fn write(&mut self, _text: &str) -> Result<(), WqIoError> {
            Ok(())
        }
    }

    fn quiet_session() -> Session {
        let mut session = Session::new();
        session.set_stdout(Box::new(SinkOutput));
        session.set_stderr(Box::new(SinkOutput));
        session.set_color_mode(ColorMode::Never);
        session
    }

    #[test]
    fn argv_returns_session_arguments() {
        let mut session = Session::new();
        session.set_argv(vec!["one".into(), "--two".into()]);

        let value = session
            .eval_string("(#argv[];argv[] 0;argv[] 1)")
            .expect("evaluate argv");

        assert_eq!(value.to_string(), "(2;\"one\";\"--two\")");
    }

    #[test]
    fn argparse_parses_flags_values_and_positionals() {
        let mut session = Session::new();
        let source = format!(
            "spec:{SPEC};r:argparse[spec;(\"-i\";\"-m\";,\"3\";\"needle\";\"a.txt\";\"b.txt\")];(r[`kind];r[`status];r[`value][`args][`ignore_case];r[`value][`args][`max_count];r[`value][`args][`pattern];r[`value][`args][`paths])"
        );

        let value = session.eval_string(&source).expect("parse arguments");

        assert_eq!(
            value.to_string(),
            "(`ok;0;T;3;\"needle\";(\"a.txt\";\"b.txt\"))"
        );
    }

    #[test]
    fn argparse_returns_structured_help_and_errors() {
        let mut session = Session::new();
        let help = session
            .eval_string(&format!(
                "r:argparse[{SPEC};,\"--help\"];(r[`kind];r[`status])"
            ))
            .expect("parse help");
        assert_eq!(help.to_string(), "(`help;0)");

        let error = session
            .eval_string(&format!(
                "r:argparse[{SPEC};,\"--wat\"];(r[`kind];r[`status];r[`error][`code])"
            ))
            .expect("parse invalid option");
        assert_eq!(error.to_string(), "(`error;2;`unknown_option)");

        let invalid_value = session
            .eval_string(&format!(
                "r:argparse[{SPEC};(\"-m\";\"nope\";\"needle\";\"a.txt\")];(r[`kind];r[`status];r[`error][`code])"
            ))
            .expect("parse invalid value");
        assert_eq!(invalid_value.to_string(), "(`error;2;`invalid_value)");

        let missing = session
            .eval_string(&format!(
                "r:argparse[{SPEC};()];(r[`kind];r[`status];r[`error][`code])"
            ))
            .expect("parse missing positional");
        assert_eq!(missing.to_string(), "(`error;2;`missing_argument)");
    }

    #[test]
    fn argparse_supports_short_clusters_counts_repetition_and_choices() {
        let mut session = Session::new();
        let source = r#"
spec:(`name:"tool";`args:(
  (`name:`verbose;`kind:`count;`short:"v");
  (`name:`define;`kind:`option;`short:"D";`multiple:T);
  (`name:`mode;`kind:`option;`choices:("fast";"safe");`default:"safe");
  (`name:`input;`kind:`positional;`required:T)));
r:argparse[spec;("-vvv";"-Done=1";"-D";"two=2";"--mode=fast";"file")];
(r[`kind];r[`value][`args][`verbose];r[`value][`args][`define];r[`value][`args][`mode])
"#;

        let value = session.eval_string(source).expect("parse rich arguments");

        assert_eq!(value.to_string(), "(`ok;3;(\"one=1\";\"two=2\");\"fast\")");

        let duplicate = session
            .eval_string(
                r#"r:argparse[(`name:"tool";`args:,(`name:`quiet;`kind:`flag;`short:"q"));("-q";"-q")];r[`error][`code]"#,
            )
            .expect("parse duplicate flag");
        assert_eq!(duplicate.to_string(), "`duplicate_argument");

        let invalid_choice = session
            .eval_string(
                r#"r:argparse[(`name:"tool";`args:,(`name:`mode;`kind:`option;`choices:("fast";"safe")));("--mode";"slow")];(r[`error][`code];r[`error][`message])"#,
            )
            .expect("parse invalid choice");
        assert_eq!(
            invalid_choice.to_string(),
            "(`invalid_choice;\"value \\\"slow\\\" for '--mode' is not one of the allowed choices\")"
        );

        let multiple_default = session
            .eval_string(
                r#"r:argparse[(`name:"tool";`args:,(`name:`mode;`kind:`option;`multiple:T;`choices:("fast";"safe");`default:("fast";"safe")));()];r[`value][`args][`mode]"#,
            )
            .expect("parse multiple default choices");
        assert_eq!(multiple_default.to_string(), "(\"fast\";\"safe\")");
    }

    #[test]
    fn argparse_rejects_invalid_specs() {
        let mut session = Session::new();
        let err = session
            .eval_string(
                "argparse[(`name:\"bad\";`args:((`name:`one;`kind:`flag);(`name:`one;`kind:`flag)));()]",
            )
            .expect_err("duplicate argument names must fail");

        assert_eq!(err.err_type.name(), "domain");
        assert!(err.to_string().contains("duplicate argument name 'one'"));

        for (source, message) in [
            (
                "argparse[(`name:\"bad\";`args:,(`name:`quiet;`kind:`flag;`choices:,T));()]",
                "'spec.args[0].choices' is only valid for options and positionals",
            ),
            (
                "argparse[(`name:\"bad\";`args:,(`name:`quiet;`kind:`flag;`value_name:\"BOOL\"));()]",
                "'spec.args[0].value_name' is only valid for options and positionals",
            ),
            (
                "argparse[(`name:\"bad\";`args:,(`name:`quiet;`kind:`flag;`short:\"-\"));()]",
                "'spec.args[0].short' cannot be '-', '=', or whitespace",
            ),
        ] {
            let err = session.eval_string(source).expect_err("invalid spec");
            assert!(err.to_string().contains(message), "{err}");
        }

        let callable_error = session
            .eval_string(
                "argparse[(`name:\"bad\";`args:,(`name:`value;`kind:`option;`parse:1));()]",
            )
            .expect_err("non-callable parser should fail");
        assert_eq!(callable_error.msg.as_deref(), Some("expected callable"));
        assert_eq!(
            callable_error.notes.as_ref(),
            &["at configuration field 'spec.args[0].parse'", "got 1 (int)",]
        );
    }

    #[test]
    fn cliargs_attributes_spec_errors_to_cliargs() {
        let mut session = Session::new();
        let error = session
            .eval_string("cliargs[(`name:\"\")]")
            .expect_err("empty command name should fail");

        assert_eq!(error.src.as_deref(), Some("builtin-function 'cliargs'"));
    }

    #[test]
    fn argparse_reports_a_structured_argument_list_requirement() {
        let error =
            parse_argv_value(&Value::Int(1)).expect_err("non-list argument input should fail");

        assert_eq!(error.msg.as_deref(), Some("expected list of strings"));
        assert_eq!(error.notes.as_ref(), &["at argument 2", "got 1 (int)"]);
    }

    #[test]
    fn cliargs_returns_values_or_requests_an_uncatchable_halt() {
        let mut success = quiet_session();
        success.set_argv(vec!["needle".into(), "a.txt".into()]);
        let value = success
            .eval_string(&format!(
                "r:cliargs[{SPEC}];(r[`args][`pattern];r[`args][`paths])"
            ))
            .expect("parse session arguments");
        assert_eq!(value.to_string(), "(\"needle\";,\"a.txt\")");
        assert_eq!(success.halt_status(), None);

        let mut help = quiet_session();
        help.set_argv(vec!["--help".into()]);
        help.eval_string(&format!("@t cliargs[{SPEC}];after:1"))
            .expect("help halts without an error");
        assert!(!help.take_interrupt());
        assert_eq!(help.halt_status(), Some(0));
        assert!(!help.bindings().contains_key("after"));

        let mut error = quiet_session();
        error.set_argv(vec!["--wat".into()]);
        error
            .eval_string(&format!("cliargs[{SPEC}];after:1"))
            .expect("usage errors halt without a runtime error");
        assert_eq!(error.halt_status(), Some(2));
        assert!(!error.bindings().contains_key("after"));
    }
}
