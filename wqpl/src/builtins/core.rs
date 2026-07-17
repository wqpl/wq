use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
#[cfg(not(target_arch = "wasm32"))]
use std::io::{Read, Write};
#[cfg(not(target_arch = "wasm32"))]
use std::process::{Command, Stdio};
use std::str::FromStr;
use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use std::time::{Duration, Instant};

#[cfg(not(target_arch = "wasm32"))]
use indexmap::IndexMap;
use num_bigint::BigInt;
use num_traits::Num;
#[cfg(not(target_arch = "wasm32"))]
use num_traits::ToPrimitive;

#[cfg(not(target_arch = "wasm32"))]
use crate::builtins::at_least_arity_error;
use crate::builtins::{
    BuiltinContext, BuiltinEnum, BuiltinFnArgs, check_arity, check_arity_named, check_named_args,
    type_mismatch,
};
use crate::session::stdio::WqIoError;
use crate::value::{Excerpt, IntoWqValue, Value, WqResult, expected_string1, into_wq_string};
use crate::wqerror::{Bound, Requirement, WqError, WqErrorType};

fn host_io_error(builtin: BuiltinEnum, error: WqIoError) -> WqError {
    WqError::new(WqErrorType::Io)
        .src(builtin)
        .attach_note(format!("host I/O error: {error}"))
}

pub(super) fn print(vm: &mut dyn BuiltinContext, args: BuiltinFnArgs) -> WqResult<Value> {
    if args.is_empty() {
        return Ok(Value::unit());
    }
    for arg in args {
        if let Some(s) = arg.try_flatten_to_rust_string() {
            vm.write_stdout(&s)
                .map_err(|error| host_io_error(BuiltinEnum::Print, error))?;
        } else {
            vm.write_stdout(&arg.to_string())
                .map_err(|error| host_io_error(BuiltinEnum::Print, error))?;
        }
    }
    Ok(Value::unit())
}

pub(super) fn echo(vm: &mut dyn BuiltinContext, args: BuiltinFnArgs) -> WqResult<Value> {
    check_named_args(&args, BuiltinEnum::Echo, super::ECHO_NAMED_ARGS)?;

    if args.is_empty() {
        vm.write_stdout_line("")
            .map_err(|error| host_io_error(BuiltinEnum::Echo, error))?;
        return Ok(Value::unit());
    }

    if let Some(sep_val) = args.named("sep") {
        let sep = sep_val.try_to_rust_string().ok_or_else(|| {
            expected_string1(sep_val)
                .src(BuiltinEnum::Echo)
                .at_named_arg("sep")
        })?;
        let mut out = String::new();
        for (i, arg) in args.iter().enumerate() {
            if i > 0 {
                out.push_str(&sep);
            }
            if let Some(s) = arg.try_flatten_to_rust_string() {
                out.push_str(&s);
            } else {
                out.push_str(&arg.to_string());
            }
        }
        vm.write_stdout_line(&out)
            .map_err(|error| host_io_error(BuiltinEnum::Echo, error))?;
    } else {
        for arg in args {
            if let Some(s) = arg.try_flatten_to_rust_string() {
                vm.write_stdout_line(&s)
                    .map_err(|error| host_io_error(BuiltinEnum::Echo, error))?;
            } else {
                vm.write_stdout_line(&arg.to_string())
                    .map_err(|error| host_io_error(BuiltinEnum::Echo, error))?;
            }
        }
    }
    Ok(Value::unit())
}

pub(super) fn input(vm: &mut dyn BuiltinContext, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BuiltinEnum::Input, [0, 1], &args)?;
    let prompt = if args.len() == 1 {
        args[0]
            .try_to_rust_string()
            .ok_or_else(|| expected_string1(&args[0]).src(BuiltinEnum::Input))?
    } else {
        String::new()
    };
    match vm.read_line(&prompt) {
        Ok(line) => Ok(into_wq_string(line)),
        Err(WqIoError::Eof | WqIoError::Interrupted) => Ok(Value::unit()),
        Err(error) => Err(host_io_error(BuiltinEnum::Input, error)),
    }
}

pub(super) fn bfn(vm: &mut dyn BuiltinContext, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BuiltinEnum::Bfn, [0], &args)?;
    let mut funcs = vm.list_enabled_builtins();
    funcs.sort();
    let funcstr = funcs.into_iter().map(into_wq_string).collect();
    Ok(Value::List(Arc::new(funcstr)))
}

pub(super) fn chr(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BuiltinEnum::Chr, [1], &args)?;
    args[0].chr().map_err(|e| e.src(BuiltinEnum::Chr))
}

pub(super) fn ord(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BuiltinEnum::Ord, [1], &args)?;
    args[0].ord().map_err(|e| e.src(BuiltinEnum::Ord))
}

pub(super) fn int(args: BuiltinFnArgs) -> WqResult<Value> {
    fn unexpected_base() -> WqError {
        WqError::new(WqErrorType::Domain)
            .src(BuiltinEnum::Int)
            .msg("base is only accepted when parsing a string")
            .at_arg(0)
    }

    check_arity(BuiltinEnum::Int, [1, 2], &args)?;
    // Parse args
    let (val, base_opt) = match &*args {
        [v] => (v, None),
        [v, Value::Int(b)] if (2..=36).contains(b) => (v, Some(u32::try_from(*b).unwrap())),
        [_, b] => {
            return Err(WqError::new(WqErrorType::Domain)
                .src(BuiltinEnum::Int)
                .expected(Requirement::int_range(
                    Bound::Included(2),
                    Bound::Included(36),
                ))
                .at_arg(1)
                .got1(b));
        }
        _ => unreachable!(),
    };
    let base = base_opt.unwrap_or(10);
    match val {
        Value::Int(n) => {
            if base_opt.is_some() {
                Err(unexpected_base())
            } else {
                Ok(Value::Int(*n))
            }
        }
        Value::BigInt(n) => {
            if base_opt.is_some() {
                Err(unexpected_base())
            } else {
                Ok(Value::BigInt(n.clone()))
            }
        }
        Value::Bool(b) => {
            if base_opt.is_some() {
                Err(unexpected_base())
            } else {
                Ok(Value::Int(i64::from(*b)))
            }
        }
        v => {
            let s = v
                .try_to_rust_string()
                .ok_or_else(|| expected_string1(v).src(BuiltinEnum::Int))?;
            let s = s.trim();
            if s.is_empty() {
                return Ok(Value::unit());
            }
            // sign
            let (neg, rest) = match s.as_bytes().first() {
                Some(b'+') => (false, &s[1..]),
                Some(b'-') => (true, &s[1..]),
                _ => (false, s),
            };
            // canonical prefixes (only if base matches)
            let rest = match base {
                16 => rest
                    .strip_prefix("0x")
                    .or_else(|| rest.strip_prefix("0X"))
                    .unwrap_or(rest),
                2 => rest
                    .strip_prefix("0b")
                    .or_else(|| rest.strip_prefix("0B"))
                    .unwrap_or(rest),
                8 => rest
                    .strip_prefix("0o")
                    .or_else(|| rest.strip_prefix("0O"))
                    .unwrap_or(rest),
                _ => rest,
            };
            // ignore underscores
            let digits: String = rest.chars().filter(|&c| c != '_').collect();
            if digits.is_empty() {
                return Err(WqError::new(WqErrorType::Domain)
                    .src(BuiltinEnum::Int)
                    .msg("could not parse int literal")
                    .at_arg(0)
                    .attach_note("digits are required after optional sign or prefix"));
            }
            let mut mag = BigInt::from_str_radix(&digits, base).map_err(|e| {
                WqError::new(WqErrorType::Domain)
                    .src(BuiltinEnum::Int)
                    .msg("could not parse int literal")
                    .at_arg(0)
                    .attach_note(format!("parser error: {e}"))
            })?;
            if neg {
                mag = -mag;
            }
            Ok(Value::from_bigint(mag))
        }
    }
}

pub(super) fn float(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BuiltinEnum::Float, [1], &args)?;
    let input = &args[0];

    if matches!(input, Value::Int(_) | Value::BigInt(_) | Value::Float(_)) || input.is_fraction() {
        return input.as_f64().map(Value::float).ok_or_else(|| {
            WqError::new(WqErrorType::Domain)
                .src(BuiltinEnum::Float)
                .msg("value cannot be represented as a float")
                .got1(input)
        });
    }

    let s = input
        .try_to_rust_string()
        .ok_or_else(|| expected_string1(input).src(BuiltinEnum::Float).at_arg(0))?;
    let s = s.trim();
    if s.is_empty() {
        return Ok(Value::unit());
    }

    f64::from_str(s).map(Value::float).map_err(|e| {
        WqError::new(WqErrorType::Domain)
            .src(BuiltinEnum::Float)
            .msg("could not parse float literal")
            .at_arg(0)
            .attach_note(format!("parser error: {e}"))
    })
}

pub(super) fn bin(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BuiltinEnum::Bin, [1, 2], &args)?;
    let (target, with_prefix) = match args.len() {
        1 => (&args[0], true),
        2 => match args[1] {
            Value::Bool(b) => (&args[0], b),
            ref v => {
                return Err(type_mismatch(BuiltinEnum::Bin, 1, Requirement::BOOL, v));
            }
        },
        _ => unreachable!(),
    };
    target
        .to_bin_repr(with_prefix)
        .map_err(|e| e.src(BuiltinEnum::Bin))
}

pub(super) fn oct(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BuiltinEnum::Oct, [1, 2], &args)?;
    let (target, with_prefix) = match args.len() {
        1 => (&args[0], true),
        2 => match args[1] {
            Value::Bool(b) => (&args[0], b),
            ref v => {
                return Err(type_mismatch(BuiltinEnum::Oct, 1, Requirement::BOOL, v));
            }
        },
        _ => unreachable!(),
    };
    target
        .to_oct_repr(with_prefix)
        .map_err(|e| e.src(BuiltinEnum::Oct))
}

pub(super) fn hex(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BuiltinEnum::Hex, [1, 2], &args)?;
    let (target, with_prefix) = match args.len() {
        1 => (&args[0], true),
        2 => match args[1] {
            Value::Bool(b) => (&args[0], b),
            ref v => {
                return Err(type_mismatch(BuiltinEnum::Hex, 1, Requirement::BOOL, v));
            }
        },
        _ => unreachable!(),
    };
    target
        .to_hex_repr(with_prefix)
        .map_err(|e| e.src(BuiltinEnum::Hex))
}

pub(super) fn hash(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BuiltinEnum::Hash, [1], &args)?;
    let mut hasher = DefaultHasher::new();
    args[0].hash(&mut hasher);
    let h = hasher.finish();
    Ok(h.into_wq_value())
}

fn assertion_message(
    args: &BuiltinFnArgs,
    index: usize,
    builtin: BuiltinEnum,
    default: &str,
) -> WqResult<String> {
    args.get_pos(index).map_or_else(
        || Ok(default.to_string()),
        |value| {
            value
                .try_to_rust_string()
                .ok_or_else(|| expected_string1(value).src(builtin).at_arg(index))
        },
    )
}

pub(super) fn assert_condition(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity_named(BuiltinEnum::Assert, [1, 2], &args, super::ASSERT_NAMED_ARGS)?;
    let condition = match args[0] {
        Value::Bool(condition) => condition,
        ref value => {
            return Err(type_mismatch(
                BuiltinEnum::Assert,
                0,
                Requirement::BOOL,
                value,
            ));
        }
    };
    let message = assertion_message(&args, 1, BuiltinEnum::Assert, "assertion failed")?;

    if condition {
        return Ok(Value::Bool(true));
    }

    let mut error = WqError::new(WqErrorType::Assert)
        .src(BuiltinEnum::Assert)
        .msg(message)
        .with_data("check", Value::Tag(Arc::from("truth")))
        .with_data("condition", Value::Bool(false));
    if let Some(context) = args.named("context") {
        error = error.with_data("context", context.clone());
    }
    Err(error)
}

pub(super) fn assert_equal(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity_named(
        BuiltinEnum::AssertEq,
        [2, 3],
        &args,
        super::ASSERT_NAMED_ARGS,
    )?;
    let message = assertion_message(&args, 2, BuiltinEnum::AssertEq, "values are not equal")?;
    let actual = &args[0];
    let expected = &args[1];

    if actual == expected {
        return Ok(actual.clone());
    }

    let mut error = WqError::new(WqErrorType::Assert)
        .src(BuiltinEnum::AssertEq)
        .msg(message)
        .attach_note(format!(
            "actual {} ({})",
            actual.excerpt(),
            actual.category()
        ))
        .attach_note(format!(
            "expected {} ({})",
            expected.excerpt(),
            expected.category()
        ))
        .with_data("check", Value::Tag(Arc::from("equal")))
        .with_data("actual", actual.clone())
        .with_data("expected", expected.clone());
    if let Some(context) = args.named("context") {
        error = error.with_data("context", context.clone());
    }
    Err(error)
}

pub(super) fn raise(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BuiltinEnum::Raise, [0, 1], &args)?;
    match args.len() {
        0 => Err(WqError::new(WqErrorType::Raise).src(BuiltinEnum::Raise)),
        1 => {
            let msg = args[0]
                .try_to_rust_string()
                .ok_or_else(|| expected_string1(&args[0]).src(BuiltinEnum::Raise))?;
            Err(WqError::new(WqErrorType::Raise)
                .src(BuiltinEnum::Raise)
                .msg(msg))
        }
        _ => unreachable!(),
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn exec(args: BuiltinFnArgs) -> WqResult<Value> {
    if args.is_empty() {
        return Err(at_least_arity_error(BuiltinEnum::Exec, 1, 0)
            .attach_note("the first argument is the program name"));
    }

    let parts: Vec<String> = args
        .iter()
        .enumerate()
        .map(|(i, v)| {
            v.try_to_rust_string()
                .ok_or_else(|| expected_string1(v).src(BuiltinEnum::Exec).at_arg(i))
        })
        .collect::<Result<_, _>>()?;

    if parts[0].is_empty() {
        return Err(WqError::new(WqErrorType::Domain)
            .src(BuiltinEnum::Exec)
            .msg("program name cannot be empty"));
    }

    let has_named_options = args.has_named();
    let opts = exec_options_from_named(&args)?;

    if has_named_options {
        exec_extended(&parts, opts)
    } else {
        exec_simple(&parts)
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Default)]
struct ExecOptions {
    stdin: Option<String>,
    cwd: Option<String>,
    env: Option<Vec<(String, String)>>,
    timeout: Option<u64>,
    check: bool,
}

#[cfg(not(target_arch = "wasm32"))]
fn exec_options_from_named(args: &BuiltinFnArgs) -> WqResult<ExecOptions> {
    const SRC: BuiltinEnum = BuiltinEnum::Exec;
    check_named_args(args, SRC, super::EXEC_NAMED_ARGS)?;
    let mut opts = ExecOptions::default();

    if let Some(v) = args.named("stdin") {
        opts.stdin = Some(
            v.try_to_rust_string()
                .ok_or_else(|| exec_named_arg_error(expected_string1(v), "stdin"))?,
        );
    }
    if let Some(v) = args.named("cwd") {
        opts.cwd = Some(
            v.try_to_rust_string()
                .ok_or_else(|| exec_named_arg_error(expected_string1(v), "cwd"))?,
        );
    }
    if let Some(v) = args.named("env") {
        let Value::Dict(env_map) = v else {
            return Err(exec_named_type_error("env", Requirement::DICT, v));
        };
        let mut pairs = Vec::with_capacity(env_map.len());
        for (ek, ev) in env_map.iter() {
            let key = ek.to_string();
            let val = ev.try_to_rust_string().ok_or_else(|| {
                exec_named_arg_error(expected_string1(ev), "env")
                    .attach_note(format!("at env key `{key}"))
            })?;
            pairs.push((key, val));
        }
        opts.env = Some(pairs);
    }
    if let Some(v) = args.named("timeout") {
        let requirement =
            || Requirement::int_range(Bound::Included(0), Bound::Included(i128::from(u64::MAX)));
        opts.timeout = Some(match v {
            Value::Int(n) if *n >= 0 => {
                u64::try_from(*n).map_err(|_| exec_named_type_error("timeout", requirement(), v))?
            }
            Value::BigInt(n) => n
                .to_u64()
                .ok_or_else(|| exec_named_type_error("timeout", requirement(), v))?,
            _ => {
                return Err(exec_named_type_error("timeout", requirement(), v));
            }
        });
    }
    if let Some(v) = args.named("check") {
        let Value::Bool(b) = v else {
            return Err(exec_named_type_error("check", Requirement::BOOL, v));
        };
        opts.check = *b;
        // Restore default: when check is omitted, it's true.
    } else {
        opts.check = true;
    }

    Ok(opts)
}

#[cfg(not(target_arch = "wasm32"))]
fn exec_named_arg_error(err: WqError, name: &str) -> WqError {
    err.src(BuiltinEnum::Exec)
        .at_named_arg(name)
        .attach_note(format!("usage: {}", BuiltinEnum::Exec.usage()))
}

#[cfg(not(target_arch = "wasm32"))]
fn exec_named_type_error(name: &str, expected: Requirement, got: &Value) -> WqError {
    WqError::new(WqErrorType::Domain)
        .src(BuiltinEnum::Exec)
        .expected(expected)
        .at_named_arg(name)
        .got1(got)
        .attach_note(format!("usage: {}", BuiltinEnum::Exec.usage()))
}

#[cfg(not(target_arch = "wasm32"))]
const EXEC_OUTPUT_EXCERPT_LIMIT: usize = 8_192;

#[cfg(not(target_arch = "wasm32"))]
fn exec_output_excerpt(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    if text.len() <= EXEC_OUTPUT_EXCERPT_LIMIT {
        return text.into_owned();
    }

    let mut end = EXEC_OUTPUT_EXCERPT_LIMIT;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let mut excerpt = text[..end].to_string();
    excerpt.push_str("...");
    excerpt
}

#[cfg(not(target_arch = "wasm32"))]
fn exec_attach_output_excerpt(err: WqError, label: &str, bytes: &[u8]) -> WqError {
    let excerpt = exec_output_excerpt(bytes);
    if excerpt.is_empty() {
        err
    } else {
        err.attach_note(format!("{label} excerpt: {excerpt}"))
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn exec_simple(parts: &[String]) -> WqResult<Value> {
    let mut cmd = Command::new(&parts[0]);
    if parts.len() > 1 {
        cmd.args(&parts[1..]);
    }
    let output = cmd.stdin(Stdio::null()).output().map_err(|e| {
        WqError::new(WqErrorType::Exec)
            .src(BuiltinEnum::Exec)
            .msg(format!("cannot spawn \"{}\": {e}", parts[0]))
    })?;

    if !output.status.success() {
        let code = output
            .status
            .code()
            .map(|c| c.to_string())
            .unwrap_or_else(|| "terminated by signal".into());
        let mut wq_err = WqError::new(WqErrorType::Exec)
            .src(BuiltinEnum::Exec)
            .msg("exec failed")
            .attach_note(format!("exit code: {code}"));
        wq_err = exec_attach_output_excerpt(wq_err, "stderr", &output.stderr);
        wq_err = exec_attach_output_excerpt(wq_err, "stdout", &output.stdout);
        return Err(wq_err);
    }

    Ok(Value::List(Arc::new(stdout_to_lines(&output.stdout))))
}

#[cfg(not(target_arch = "wasm32"))]
fn exec_extended(parts: &[String], opts: ExecOptions) -> WqResult<Value> {
    use std::thread;

    let mut cmd = Command::new(&parts[0]);
    if parts.len() > 1 {
        cmd.args(&parts[1..]);
    }

    if let Some(cwd) = &opts.cwd {
        let meta = std::fs::metadata(cwd).map_err(|e| {
            WqError::new(WqErrorType::Domain)
                .src(BuiltinEnum::Exec)
                .msg(format!("invalid cwd \"{cwd}\": {e}"))
        })?;
        if !meta.is_dir() {
            return Err(WqError::new(WqErrorType::Domain)
                .src(BuiltinEnum::Exec)
                .msg(format!("cwd \"{cwd}\" is not a directory")));
        }
        cmd.current_dir(cwd);
    }

    if let Some(env_pairs) = &opts.env {
        cmd.envs(env_pairs.iter().cloned());
    }

    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| {
        WqError::new(WqErrorType::Exec)
            .src(BuiltinEnum::Exec)
            .msg(format!("cannot spawn \"{}\": {e}", parts[0]))
    })?;

    let mut stdout_pipe = child.stdout.take().unwrap();
    let mut stderr_pipe = child.stderr.take().unwrap();

    let stdout_thread = thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout_pipe.read_to_end(&mut buf);
        buf
    });
    let stderr_thread = thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stderr_pipe.read_to_end(&mut buf);
        buf
    });

    if let Some(stdin_text) = &opts.stdin
        && let Some(mut stdin) = child.stdin.take()
    {
        let text = stdin_text.clone();
        thread::spawn(move || {
            let _ = stdin.write_all(text.as_bytes());
        });
    }

    let status = if let Some(timeout_secs) = opts.timeout {
        let timeout = Duration::from_secs(timeout_secs);
        let start = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => {
                    if start.elapsed() >= timeout {
                        let _ = child.kill();
                        // Give the child a short grace period to die so we can
                        // reap it without blocking indefinitely (relevant on macOS
                        // where syspolicyd can delay process teardown).
                        let grace = Instant::now();
                        while grace.elapsed() < Duration::from_secs(5) {
                            if let Ok(Some(_)) = child.try_wait() {
                                break;
                            }
                            thread::sleep(Duration::from_millis(50));
                        }
                        let _ = stdout_thread.join();
                        let _ = stderr_thread.join();
                        return Err(WqError::new(WqErrorType::Exec)
                            .src(BuiltinEnum::Exec)
                            .msg("exec timed out")
                            .attach_note(format!("timeout: {timeout_secs}s")));
                    }
                    thread::sleep(Duration::from_millis(50));
                }
                Err(e) => {
                    let _ = stdout_thread.join();
                    let _ = stderr_thread.join();
                    return Err(WqError::new(WqErrorType::Exec)
                        .src(BuiltinEnum::Exec)
                        .msg(format!("error waiting for \"{}\": {e}", parts[0])));
                }
            }
        }
    } else {
        match child.wait() {
            Ok(s) => s,
            Err(e) => {
                let _ = stdout_thread.join();
                let _ = stderr_thread.join();
                return Err(WqError::new(WqErrorType::Exec)
                    .src(BuiltinEnum::Exec)
                    .msg(format!("error waiting for \"{}\": {e}", parts[0])));
            }
        }
    };

    let stdout = stdout_thread.join().unwrap_or_default();
    let stderr = stderr_thread.join().unwrap_or_default();

    let success = status.success();
    let code = status.code().unwrap_or(-1);

    if opts.check && !success {
        let code_str = status
            .code()
            .map(|c| c.to_string())
            .unwrap_or_else(|| "terminated by signal".into());
        let mut wq_err = WqError::new(WqErrorType::Exec)
            .src(BuiltinEnum::Exec)
            .msg("exec failed")
            .attach_note(format!("exit code: {code_str}"));
        wq_err = exec_attach_output_excerpt(wq_err, "stderr", &stderr);
        wq_err = exec_attach_output_excerpt(wq_err, "stdout", &stdout);
        return Err(wq_err);
    }

    let mut result = IndexMap::new();
    result.insert(
        Arc::from("stdout"),
        Value::List(Arc::new(stdout_to_lines(&stdout))),
    );
    result.insert(
        Arc::from("stderr"),
        Value::List(Arc::new(stdout_to_lines(&stderr))),
    );
    result.insert(Arc::from("code"), Value::Int(i64::from(code)));
    result.insert(Arc::from("success"), Value::Bool(success));
    Ok(Value::Dict(Arc::new(result)))
}

#[cfg(not(target_arch = "wasm32"))]
fn stdout_to_lines(bytes: &[u8]) -> Vec<Value> {
    let text = String::from_utf8_lossy(bytes);
    text.lines()
        .map(|line| {
            let ln = line.strip_suffix('\r').unwrap_or(line);
            ln.into_wq_value()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use num_bigint::BigInt;
    use smallvec::smallvec;

    use super::*;

    #[test]
    fn hash_basic_test() {
        let result = hash(BuiltinFnArgs::from(Value::Int(42))).unwrap();
        assert!(matches!(result, Value::Int(_) | Value::BigInt(_)));

        // The same value produces the same hash.
        let result2 = hash(BuiltinFnArgs::from(Value::Int(42))).unwrap();
        assert_eq!(result, result2);

        // Different values produce different hashes.
        let result3 = hash(BuiltinFnArgs::from(Value::Int(43))).unwrap();
        assert_ne!(result, result3);
    }

    #[test]
    fn hash_respects_complex_value_equality() {
        let lhs = Value::from_complex64(num_complex::Complex64::new(0.0, 0.0));
        let rhs = Value::from_complex64(num_complex::Complex64::new(-0.0, -0.0));

        assert_eq!(lhs, rhs);
        assert_eq!(
            hash(BuiltinFnArgs::from(lhs)).expect("hash should succeed"),
            hash(BuiltinFnArgs::from(rhs)).expect("hash should succeed")
        );
    }

    #[test]
    fn chr_single_int() {
        let result = chr(BuiltinFnArgs::from(Value::Int(65))).unwrap();
        assert_eq!(result, Value::Char('A'));
    }

    #[test]
    fn chr_int_list() {
        let result = chr(BuiltinFnArgs::from(Value::IntList(Arc::new(vec![65, 66])))).unwrap();
        assert_eq!(
            result,
            Value::List(Arc::new(vec![Value::Char('A'), Value::Char('B')]))
        );
    }

    #[test]
    fn int_builtin_parses_bigint() {
        let big = BigInt::from(i64::MAX) + BigInt::from(1);
        let value = into_wq_string(big.to_string());
        let result = int(BuiltinFnArgs::from(value)).unwrap();
        match result {
            Value::BigInt(n) => assert_eq!(*n, big),
            other => panic!("expected bigint result, got {other:?}"),
        }
    }

    #[test]
    fn int_builtin_converts_bool() {
        assert_eq!(
            int(BuiltinFnArgs::from(Value::Bool(false))).unwrap(),
            Value::Int(0)
        );
        assert_eq!(
            int(BuiltinFnArgs::from(Value::Bool(true))).unwrap(),
            Value::Int(1)
        );
    }

    #[test]
    fn int_builtin_reports_base_as_a_bounded_int_requirement() {
        let error = int(BuiltinFnArgs::from(smallvec![
            into_wq_string("10"),
            Value::Int(37),
        ]))
        .expect_err("a base above 36 should fail");

        assert_eq!(error.msg.as_deref(), Some("expected int from 2 through 36"));
        assert_eq!(error.notes.as_slice(), ["at argument 2", "got 37 (int)"]);
    }

    #[test]
    fn int_builtin_describes_literal_parse_failures_directly() {
        let error = int(BuiltinFnArgs::from(into_wq_string("xyz")))
            .expect_err("non-digits should fail int parsing");

        assert_eq!(error.msg.as_deref(), Some("could not parse int literal"));
        assert_eq!(
            error.notes.first().map(String::as_str),
            Some("at argument 1")
        );
    }

    #[test]
    fn float_builtin_converts_fraction_like_value() {
        let input = Value::from_fraction_parts(BigInt::from(3), BigInt::from(4));
        let result = float(BuiltinFnArgs::from(input)).unwrap();
        assert_eq!(result, Value::float(0.75));
    }

    #[test]
    fn ord_single_char() {
        let result = ord(BuiltinFnArgs::from(Value::Char('A'))).unwrap();
        assert_eq!(result, Value::Int(65));
    }

    #[test]
    fn ord_char_list() {
        let input = Value::List(Arc::new(vec![Value::Char('A'), Value::Char('B')]));
        let result = ord(BuiltinFnArgs::from(input)).unwrap();
        assert_eq!(result, Value::IntList(Arc::new(vec![65, 66])));
    }

    #[test]
    fn try_flatten_to_string_basic() {
        assert_eq!(
            Value::Char('a').try_flatten_to_rust_string(),
            Some("a".into())
        );
        assert_eq!(
            into_wq_string("hello").try_flatten_to_rust_string(),
            Some("hello".into())
        );
        assert_eq!(
            into_wq_string("").try_flatten_to_rust_string(),
            Some("".into())
        );
        assert_eq!(Value::unit().try_flatten_to_rust_string(), Some("".into()));
        assert_eq!(
            Value::List(Arc::new(vec![])).try_flatten_to_rust_string(),
            Some("".into())
        );

        assert_eq!(
            Value::List(Arc::new(vec![into_wq_string("ab"), into_wq_string("cd")]))
                .try_flatten_to_rust_string(),
            Some("abcd".into())
        );
        assert_eq!(
            Value::List(Arc::new(vec![Value::Char('x'), Value::Char('y')]))
                .try_flatten_to_rust_string(),
            Some("xy".into())
        );
        // Mixed list is not flattened
        assert!(
            Value::List(Arc::new(vec![Value::Int(1), Value::Int(2)]))
                .try_flatten_to_rust_string()
                .is_none()
        );
    }

    #[cfg(unix)]
    mod exec_tests {
        use super::*;

        #[test]
        fn exec_backward_compat() {
            let result = exec(BuiltinFnArgs::from(smallvec![
                into_wq_string("echo"),
                into_wq_string("hello")
            ]))
            .unwrap();
            match result {
                Value::List(lines) => {
                    assert_eq!(lines.len(), 1);
                    assert_eq!(lines[0], into_wq_string("hello"));
                }
                other => panic!("expected list, got {other:?}"),
            }
        }

        #[test]
        fn exec_check_true_option_returns_dict() {
            let result = exec(BuiltinFnArgs::with_named(
                smallvec![into_wq_string("printf"), into_wq_string("hello")],
                vec![(Arc::from("check"), Value::Bool(true))],
            ))
            .unwrap();
            let Value::Dict(dict) = result else {
                panic!("expected dict, got {result:?}");
            };
            assert_eq!(dict.get("success"), Some(&Value::Bool(true)));
            assert_eq!(dict.get("code"), Some(&Value::Int(0)));
            let stdout = dict.get("stdout").unwrap();
            match stdout {
                Value::List(lines) => assert_eq!(&**lines, &[into_wq_string("hello")]),
                other => panic!("expected list, got {other:?}"),
            }
        }

        #[test]
        fn exec_stdin_option() {
            let result = exec(BuiltinFnArgs::with_named(
                smallvec![into_wq_string("cat")],
                vec![(Arc::from("stdin"), into_wq_string("hi there"))],
            ))
            .unwrap();
            let Value::Dict(dict) = result else {
                panic!("expected dict, got {result:?}");
            };
            let stdout = dict.get("stdout").unwrap();
            match stdout {
                Value::List(lines) => {
                    assert_eq!(lines.len(), 1);
                    assert_eq!(lines[0], into_wq_string("hi there"));
                }
                other => panic!("expected list, got {other:?}"),
            }
        }

        #[test]
        fn exec_check_false_on_failure() {
            let result = exec(BuiltinFnArgs::with_named(
                smallvec![
                    into_wq_string("sh"),
                    into_wq_string("-c"),
                    into_wq_string("exit 42"),
                ],
                vec![(Arc::from("check"), Value::Bool(false))],
            ))
            .unwrap();
            let Value::Dict(dict) = result else {
                panic!("expected dict, got {result:?}");
            };
            assert_eq!(dict.get("success"), Some(&Value::Bool(false)));
            assert_eq!(dict.get("code"), Some(&Value::Int(42)));
        }

        #[test]
        fn exec_simple_failure_includes_stdout_and_stderr() {
            let err = exec(BuiltinFnArgs::from(smallvec![
                into_wq_string("sh"),
                into_wq_string("-c"),
                into_wq_string("printf out; printf err >&2; exit 7"),
            ]))
            .unwrap_err();
            let text = err.to_string();
            assert!(
                text.contains("stderr excerpt: err"),
                "expected stderr excerpt, got {text}"
            );
            assert!(
                text.contains("stdout excerpt: out"),
                "expected stdout excerpt, got {text}"
            );
        }

        #[test]
        fn exec_named_type_errors_name_option() {
            fn assert_named_error(name: &str, value: Value, expected: &str) {
                let err = exec(BuiltinFnArgs::with_named(
                    smallvec![into_wq_string("printf"), into_wq_string("hello")],
                    vec![(Arc::from(name), value)],
                ))
                .unwrap_err();
                assert_eq!(err.msg.as_deref(), Some(expected));
                let text = err.to_string();
                assert!(
                    text.contains(&format!("at named argument '{name}'")),
                    "expected named arg note, got {text}"
                );
                assert!(
                    !text.contains("at arg[0]"),
                    "named arg error should not point at positional arg 0: {text}"
                );
            }

            assert_named_error(
                "timeout",
                into_wq_string("soon"),
                "expected int from 0 through 18446744073709551615",
            );
            assert_named_error("check", Value::Int(1), "expected bool");
        }

        #[test]
        fn exec_timeout_accepts_bigints_that_fit_the_public_range() {
            let timeout = BigInt::from(i64::MAX) + BigInt::from(1_u8);
            let options = exec_options_from_named(&BuiltinFnArgs::with_named(
                smallvec![],
                vec![(
                    Arc::from("timeout"),
                    Value::BigInt(Arc::new(timeout.clone())),
                )],
            ))
            .expect("a bigint-backed timeout inside the stated range should parse");

            assert_eq!(options.timeout, timeout.to_u64());
        }

        #[test]
        fn exec_env_value_errors_name_key() {
            let mut env = indexmap::IndexMap::new();
            env.insert(Arc::from("WQ_TEST_VAR"), Value::Int(42));
            let err = exec(BuiltinFnArgs::with_named(
                smallvec![into_wq_string("printf"), into_wq_string("hello")],
                vec![(Arc::from("env"), Value::Dict(Arc::new(env)))],
            ))
            .unwrap_err();
            let text = err.to_string();
            assert!(
                text.contains("at named argument 'env'"),
                "expected env named arg note, got {text}"
            );
            assert!(
                text.contains("at env key `WQ_TEST_VAR"),
                "expected env key note, got {text}"
            );
        }

        #[test]
        fn exec_timeout_kills_child() {
            let err = exec(BuiltinFnArgs::with_named(
                smallvec![into_wq_string("sleep"), into_wq_string("1")],
                vec![(Arc::from("timeout"), Value::Int(0))],
            ))
            .unwrap_err();
            assert!(
                err.to_string().contains("timed out"),
                "expected timeout error, got {err}"
            );
        }

        #[test]
        fn exec_invalid_cwd() {
            let err = exec(BuiltinFnArgs::with_named(
                smallvec![into_wq_string("echo"), into_wq_string("hi")],
                vec![(Arc::from("cwd"), into_wq_string("/nonexistent/path/12345"))],
            ))
            .unwrap_err();
            assert!(
                err.to_string().contains("invalid cwd") || err.to_string().contains("cwd"),
                "expected cwd error, got {err}"
            );
        }

        #[test]
        fn exec_env_option() {
            let mut env = indexmap::IndexMap::new();
            env.insert(Arc::from("WQ_TEST_VAR"), into_wq_string("wq_value_42"));
            let result = exec(BuiltinFnArgs::with_named(
                smallvec![
                    into_wq_string("sh"),
                    into_wq_string("-c"),
                    into_wq_string("echo $WQ_TEST_VAR"),
                ],
                vec![(Arc::from("env"), Value::Dict(Arc::new(env)))],
            ))
            .unwrap();
            let Value::Dict(dict) = result else {
                panic!("expected dict, got {result:?}");
            };
            let stdout = dict.get("stdout").unwrap();
            match stdout {
                Value::List(lines) => {
                    assert_eq!(lines.len(), 1);
                    assert_eq!(lines[0], into_wq_string("wq_value_42"));
                }
                other => panic!("expected list, got {other:?}"),
            }
        }
    }
}
