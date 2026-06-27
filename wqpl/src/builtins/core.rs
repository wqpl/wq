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

use crate::builtins::{
    BuiltinContext, BuiltinEnum, BuiltinFnArgs, check_arity, check_named_args, type_mismatch,
};
use crate::session::stdio::{
    WqStdinError, wqstdin_readline, wqstdin_with_highlight_off, wqstdout_print, wqstdout_println,
};
use crate::value::{Excerpt, IntoWqValue, Value, WqResult, into_wq_string};
use crate::wqerror::{WqError, WqErrorType};

pub(super) fn print(args: BuiltinFnArgs) -> WqResult<Value> {
    if args.is_empty() {
        return Ok(Value::unit());
    }
    for arg in args {
        if let Some(s) = arg.try_flatten_to_string() {
            wqstdout_print(s);
        } else {
            wqstdout_print(arg.to_string());
        }
    }
    Ok(Value::unit())
}

pub(super) fn echo(args: BuiltinFnArgs) -> WqResult<Value> {
    check_named_args(&args, BuiltinEnum::Echo, super::ECHO_NAMED_ARGS)?;

    if args.is_empty() {
        wqstdout_println("");
        return Ok(Value::unit());
    }

    if let Some(sep_val) = args.named("sep") {
        let sep = sep_val.to_rust_string_with_note().map_err(|e| {
            e.src(BuiltinEnum::Echo)
                .attach_note("named arg 'sep' must be a string")
        })?;
        let mut out = String::new();
        for (i, arg) in args.iter().enumerate() {
            if i > 0 {
                out.push_str(&sep);
            }
            if let Some(s) = arg.try_flatten_to_string() {
                out.push_str(&s);
            } else {
                out.push_str(&arg.to_string());
            }
        }
        wqstdout_println(out);
    } else {
        for arg in args {
            if let Some(s) = arg.try_flatten_to_string() {
                wqstdout_println(s);
            } else {
                wqstdout_println(arg.to_string());
            }
        }
    }
    Ok(Value::unit())
}

pub(super) fn input(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BuiltinEnum::Input, [0, 1], &args)?;
    let prompt = if args.len() == 1 {
        args[0]
            .to_rust_string_with_note()
            .map_err(|e| e.src(BuiltinEnum::Input))?
    } else {
        String::new()
    };
    let res = wqstdin_with_highlight_off(|| wqstdin_readline(&prompt));
    match res {
        Ok(line) => Ok(into_wq_string(line)),
        Err(WqStdinError::Eof) => Ok(Value::unit()),
        Err(WqStdinError::Interrupted) => Ok(Value::unit()),
        Err(WqStdinError::Other(e)) => Err(WqError::new(WqErrorType::Io)
            .src(BuiltinEnum::Input)
            .attach_note(format!("original error: {}", e))),
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
            .msg("unexpected base")
            .attach_note("base should only be provided when parsing text")
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
                .msg("expected valid base")
                .attach_note("valid base is int in 2..=36")
                .at_arg(1)
                .attach_note(format!("got {}", b.excerpt())));
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
                .to_rust_string_with_note()
                .map_err(|e| e.src(BuiltinEnum::Int))?;
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
                    .msg("expected valid int literal")
                    .at_arg(0)
                    .attach_note("digits are required after optional sign or prefix"));
            }
            let mut mag = BigInt::from_str_radix(&digits, base).map_err(|e| {
                WqError::new(WqErrorType::Domain)
                    .src(BuiltinEnum::Int)
                    .msg("expected valid int literal")
                    .at_arg(0)
                    .attach_note(format!("original error: {}", e))
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
                .msg("provided value cannot be converted to float")
                .got1(input)
        });
    }

    let s = input
        .to_rust_string_with_note()
        .map_err(|e| e.src(BuiltinEnum::Float).at_arg(0))?;
    let s = s.trim();
    if s.is_empty() {
        return Ok(Value::unit());
    }

    f64::from_str(s).map(Value::float).map_err(|e| {
        WqError::new(WqErrorType::Domain)
            .src(BuiltinEnum::Float)
            .msg("expected valid float literal")
            .at_arg(0)
            .attach_note(format!("original error: {e}"))
    })
}

pub(super) fn bin(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BuiltinEnum::Bin, [1, 2], &args)?;
    let (target, with_prefix) = match args.len() {
        1 => (&args[0], true),
        2 => match args[1] {
            Value::Bool(b) => (&args[0], b),
            ref v => {
                return Err(type_mismatch(BuiltinEnum::Bin, 1, "bool", v));
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
                return Err(type_mismatch(BuiltinEnum::Oct, 1, "bool", v));
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
                return Err(type_mismatch(BuiltinEnum::Hex, 1, "bool", v));
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

pub(super) fn raise(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BuiltinEnum::Raise, [0, 1], &args)?;
    match args.len() {
        0 => Err(WqError::new(WqErrorType::Raise).src(BuiltinEnum::Raise)),
        1 => {
            let msg = args[0]
                .to_rust_string_with_note()
                .map_err(|e| e.src(BuiltinEnum::Raise))?;
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
        return Err(WqError::new(WqErrorType::Arity)
            .src(BuiltinEnum::Exec)
            .msg("expected 1 or more args")
            .attach_note("require at least the program name"));
    }

    let parts: Vec<String> = args
        .iter()
        .enumerate()
        .map(|(i, v)| v.to_rust_string_with_note().map_err(|e| (i, e)))
        .collect::<Result<_, _>>()
        .map_err(|(i, e)| e.src(BuiltinEnum::Exec).at_arg(i))?;

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
            v.to_rust_string_with_note()
                .map_err(|e| exec_named_arg_error(e, "stdin", "a string"))?,
        );
    }
    if let Some(v) = args.named("cwd") {
        opts.cwd = Some(
            v.to_rust_string_with_note()
                .map_err(|e| exec_named_arg_error(e, "cwd", "a string"))?,
        );
    }
    if let Some(v) = args.named("env") {
        let Value::Dict(env_map) = v else {
            return Err(exec_named_type_error("env", "dict", v));
        };
        let mut pairs = Vec::with_capacity(env_map.len());
        for (ek, ev) in env_map.iter() {
            let key = ek.to_string();
            let val = ev.to_rust_string_with_note().map_err(|e| {
                exec_named_arg_error(e, "env", "a dict of string values")
                    .attach_note(format!("at env key '{key}'"))
            })?;
            pairs.push((key, val));
        }
        opts.env = Some(pairs);
    }
    if let Some(v) = args.named("timeout") {
        opts.timeout = Some(match v {
            Value::Int(n) if *n >= 0 => *n as u64,
            _ => {
                return Err(exec_named_type_error("timeout", "non-negative int", v));
            }
        });
    }
    if let Some(v) = args.named("check") {
        let Value::Bool(b) = v else {
            return Err(exec_named_type_error("check", "bool", v));
        };
        opts.check = *b;
        // Restore default: when check is omitted, it's true.
    } else {
        opts.check = true;
    }

    Ok(opts)
}

#[cfg(not(target_arch = "wasm32"))]
fn exec_named_arg_error(err: WqError, name: &str, expected: &str) -> WqError {
    err.src(BuiltinEnum::Exec)
        .attach_note(format!("at named arg '{name}'"))
        .attach_note(format!("named arg '{name}' must be {expected}"))
        .attach_note(format!("usage: {}", BuiltinEnum::Exec.usage()))
}

#[cfg(not(target_arch = "wasm32"))]
fn exec_named_type_error(name: &str, expected: &str, got: &Value) -> WqError {
    WqError::new(WqErrorType::Domain)
        .src(BuiltinEnum::Exec)
        .msg(format!("expected {expected}"))
        .attach_note(format!("at named arg '{name}'"))
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
            .msg(format!("cannot spawn '{}': {e}", parts[0]))
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
                .msg(format!("invalid cwd '{cwd}': {e}"))
        })?;
        if !meta.is_dir() {
            return Err(WqError::new(WqErrorType::Domain)
                .src(BuiltinEnum::Exec)
                .msg(format!("cwd '{cwd}' is not a directory")));
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
            .msg(format!("cannot spawn '{}': {e}", parts[0]))
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
                        .msg(format!("error waiting for '{}': {e}", parts[0])));
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
                    .msg(format!("error waiting for '{}': {e}", parts[0])));
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

        // Same value produce thw same hash
        let result2 = hash(BuiltinFnArgs::from(Value::Int(42))).unwrap();
        assert_eq!(result, result2);

        // Different values produce different hash
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
        assert_eq!(Value::Char('a').try_flatten_to_string(), Some("a".into()));
        assert_eq!(
            into_wq_string("hello").try_flatten_to_string(),
            Some("hello".into())
        );
        assert_eq!(into_wq_string("").try_flatten_to_string(), Some("".into()));
        assert_eq!(Value::unit().try_flatten_to_string(), Some("".into()));
        assert_eq!(
            Value::List(Arc::new(vec![])).try_flatten_to_string(),
            Some("".into())
        );

        assert_eq!(
            Value::List(Arc::new(vec![into_wq_string("ab"), into_wq_string("cd")]))
                .try_flatten_to_string(),
            Some("abcd".into())
        );
        assert_eq!(
            Value::List(Arc::new(vec![Value::Char('x'), Value::Char('y')])).try_flatten_to_string(),
            Some("xy".into())
        );
        // Mixed list is not flattened
        assert!(
            Value::List(Arc::new(vec![Value::Int(1), Value::Int(2)]))
                .try_flatten_to_string()
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
            fn assert_named_error(name: &str, value: Value) {
                let err = exec(BuiltinFnArgs::with_named(
                    smallvec![into_wq_string("printf"), into_wq_string("hello")],
                    vec![(Arc::from(name), value)],
                ))
                .unwrap_err();
                let text = err.to_string();
                assert!(
                    text.contains(&format!("at named arg '{name}'")),
                    "expected named arg note, got {text}"
                );
                assert!(
                    !text.contains("at arg[0]"),
                    "named arg error should not point at positional arg 0: {text}"
                );
            }

            assert_named_error("timeout", into_wq_string("soon"));
            assert_named_error("check", Value::Int(1));
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
                text.contains("at named arg 'env'"),
                "expected env named arg note, got {text}"
            );
            assert!(
                text.contains("at env key 'WQ_TEST_VAR'"),
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
