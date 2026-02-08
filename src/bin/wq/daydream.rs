// daydream: cli arg parser for wq
// err code:
// help/ver query    -> 0
// wrong usage       -> 2

use std::{ffi::OsString, path::PathBuf};

use wqpl::debug_flags::DebugFlags;

#[derive(Debug, Clone, Default)]
pub struct RuntimeFlags {
    pub wqdb: bool,
    pub bt: bool, // default: true
    pub debug_flags: DebugFlags,
    pub interpreter: Option<String>,
    pub builtins: Option<String>,
    pub print: bool, // default: false
}

impl RuntimeFlags {
    pub fn new() -> Self {
        Self {
            wqdb: false,
            bt: true,
            debug_flags: DebugFlags::empty(),
            interpreter: None,
            builtins: None,
            print: false,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct FmtOpts {
    pub nlcd: bool,
    pub nbc: bool,
    pub olw: bool,
}

#[derive(Debug, Clone)]
pub enum ExecSource {
    Stdin,
    Inline(String),
}

#[derive(Debug, Clone)]
pub enum Command {
    Repl,
    Script(PathBuf),
    Exec(ExecSource),
    Fmt { script: PathBuf, opts: FmtOpts },
}

#[derive(Debug)]
pub enum ParseOutcome {
    Succeed(Parsed),
    ShowHelp,
    ShowVersion,
    Error { msg: String, code: i32 },
}

#[derive(Debug)]
pub struct Parsed {
    pub runtime: RuntimeFlags,
    pub command: Command,
}

/// Parse args (excluding argv[0]) into a `ParseOutcome`.
pub fn parse_args<I>(args: I) -> ParseOutcome
where
    I: IntoIterator<Item = OsString>,
{
    use ParseOutcome::*;
    // 1) Collect once
    let buf: Vec<std::ffi::OsString> = args.into_iter().collect();
    // 2) Short-circuit help/version by scanning the slice
    let saw_help = buf
        .iter()
        .any(|a| matches!(a.to_str(), Some("-h" | "--help")));
    if saw_help {
        return ShowHelp;
    }
    let saw_version = buf
        .iter()
        .any(|a| matches!(a.to_str(), Some("-v" | "-V" | "--version")));
    if saw_version {
        return ShowVersion;
    }
    // 3) Iterate
    let mut it = buf.into_iter().peekable();
    let mut runtime = RuntimeFlags::new();
    let mut fmt_opts = FmtOpts::default();
    let mut mode_fmt = false;
    let mut command: Option<Command> = None;
    let mut script: Option<PathBuf> = None;
    let mut fmt_script: Option<PathBuf> = None;
    let mut end_of_flags = false;
    // Collect extras/unknowns for mode-aware errors
    let mut unknown: Vec<String> = Vec::new();
    let mut extras: Vec<String> = Vec::new();
    while let Some(os) = it.next() {
        let s = os.to_string_lossy();
        if !end_of_flags && s == "--" {
            end_of_flags = true;
            continue;
        }
        // Subcommands/positionals
        if !s.starts_with('-') || end_of_flags {
            if s == "fmt" && command.is_none() && !end_of_flags {
                mode_fmt = true;
                continue;
            }
            if s == "exec" && command.is_none() && !end_of_flags {
                // next token may be '-' (stdin) or inline code (non-flag)
                if let Some(nxt) = it.peek() {
                    let nxts = nxt.to_string_lossy();
                    if nxts == "-" {
                        it.next();
                        command = Some(Command::Exec(ExecSource::Stdin));
                        continue;
                    } else if !nxts.starts_with('-') {
                        let val = it.next().unwrap().to_string_lossy().into_owned();
                        command = Some(Command::Exec(ExecSource::Inline(val)));
                        continue;
                    }
                }
                command = Some(Command::Exec(ExecSource::Stdin));
                continue;
            }
            // fmt script positional (only one)
            if mode_fmt && fmt_script.is_none() {
                fmt_script = Some(PathBuf::from(s.as_ref()));
                continue;
            }
            // main script positional (only when no subcommand chosen)
            if !mode_fmt && command.is_none() && script.is_none() {
                script = Some(PathBuf::from(s.as_ref()));
                continue;
            }
            // Anything beyond the expected positionals is an error
            extras.push(s.into_owned());
            continue;
        }
        // Flags
        match s.as_ref() {
            "-p" | "--print" => {
                runtime.print = true;
            }
            "-w" | "--wqdb" => {
                runtime.wqdb = true;
            }
            "--builtins" => {
                if let Some(preset) = it.next() {
                    runtime.builtins = Some(preset.to_string_lossy().into_owned());
                } else {
                    unknown.push(s.into_owned());
                }
            }
            "--no-bt" => {
                runtime.bt = false;
            }
            "--nlcd" => {
                fmt_opts.nlcd = true;
            }
            "--nbc" => {
                fmt_opts.nbc = true;
            }
            "--olw" => {
                fmt_opts.olw = true;
            }
            "--interp" | "--interpreter" => {
                if let Some(val) = it.peek().and_then(|p| p.to_str())
                    && !val.starts_with('-')
                {
                    runtime.interpreter = Some(it.next().unwrap().to_string_lossy().into_owned());
                } else {
                    return Error {
                        msg: format!("{s}: missing interpreter name"),
                        code: 2,
                    };
                }
            }
            "-d" | "--debug" => match parse_debug_arg(it.peek().and_then(|p| p.to_str())) {
                Ok(Some(flags)) => {
                    it.next();
                    runtime.debug_flags = flags;
                }
                Ok(None) => {
                    runtime.debug_flags = DebugFlags::from_alias(1).expect("debug alias 1 exists");
                }
                Err(e) => return Error { msg: e, code: 2 },
            },
            _ if s.starts_with("-d") => {
                let val = &s[2..];
                match DebugFlags::parse(val) {
                    Ok(flags) => runtime.debug_flags = flags,
                    Err(e) => return Error { msg: e, code: 2 },
                }
            }
            other => unknown.push(other.to_string()),
        }
    }
    // Default command if nothing provided.
    if command.is_none() && fmt_script.is_none() && script.is_none() {
        command = Some(Command::Repl);
    }
    // Mode-specific validations.
    if let Some(f) = fmt_script {
        if !unknown.is_empty() {
            return Error {
                msg: format!("fmt: unknown flags: {}", unknown.join(", ")),
                code: 2,
            };
        }
        if !extras.is_empty() {
            return Error {
                msg: format!("fmt: unexpected positional arguments: {}", extras.join(" ")),
                code: 2,
            };
        }
        // Disallow runtime flags in fmt mode
        if runtime.wqdb
            || !runtime.bt
            || !runtime.debug_flags.is_empty()
            || runtime.interpreter.is_some()
            || runtime.builtins.is_some()
        {
            return Error {
                msg: "fmt: runtime flags are not supported".to_string(),
                code: 2,
            };
        }
        return Succeed(Parsed {
            runtime, // defaults
            command: Command::Fmt {
                script: f,
                opts: fmt_opts,
            },
        });
    }
    if mode_fmt {
        return Error {
            msg: "fmt: missing <script>".into(),
            code: 2,
        };
    }
    if !unknown.is_empty() {
        return Error {
            msg: format!("unknown flags: {}", unknown.join(", ")),
            code: 2,
        };
    }
    if !extras.is_empty() {
        return Error {
            msg: format!("unexpected positional arguments: {}", extras.join(" ")),
            code: 2,
        };
    }
    let cmd = if let Some(c) = command {
        c
    } else if let Some(path) = script {
        Command::Script(path)
    } else {
        Command::Repl
    };
    Succeed(Parsed {
        runtime,
        command: cmd,
    })
}

fn parse_debug_arg(next: Option<&str>) -> Result<Option<DebugFlags>, String> {
    match next {
        Some(val) if !val.starts_with('-') => match DebugFlags::parse(val) {
            Ok(flags) => Ok(Some(flags)),
            Err(e) => {
                // Heuristic: if it looks like an intended debug flag spec, return the error.
                // Otherwise, assume it's a positional argument (script) and return Ok(None).
                if val.contains(',') || val.chars().all(|c| c.is_ascii_digit()) {
                    Err(e)
                } else {
                    Ok(None)
                }
            }
        },
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    fn v(xs: &[&str]) -> Vec<OsString> {
        xs.iter().map(OsString::from).collect()
    }

    fn ok(p: ParseOutcome) -> Parsed {
        match p {
            ParseOutcome::Succeed(parsed) => parsed,
            other => panic!("expected Ok(..), got: {other:?}"),
        }
    }

    fn err(p: ParseOutcome) -> (String, i32) {
        match p {
            ParseOutcome::Error { msg, code } => (msg, code),
            other => panic!("expected Error(..), got: {other:?}"),
        }
    }

    #[test]
    fn help_and_version_short_circuit() {
        assert!(matches!(parse_args(v(&["--help"])), ParseOutcome::ShowHelp));
        assert!(matches!(
            parse_args(v(&["--version"])),
            ParseOutcome::ShowVersion
        ));
        assert!(matches!(
            parse_args(v(&["foo.wq", "-h"])),
            ParseOutcome::ShowHelp
        ));
    }

    #[test]
    fn script_and_extras_error() {
        let p = ok(parse_args(v(&["main.wq"])));
        matches!(p.command, Command::Script(_));
        let (m, c) = err(parse_args(v(&["main.wq", "extra", "stuff"])));
        assert_eq!(c, 2);
        assert!(m.contains("unexpected positional"));
    }

    #[test]
    fn fmt_happy_path_and_mode_errors() {
        let p = ok(parse_args(v(&["fmt", "--nbc", "f.wq"])));
        match p.command {
            Command::Fmt { script, opts } => {
                assert_eq!(script, PathBuf::from("f.wq"));
                assert!(opts.nbc);
                assert!(!opts.nlcd);
                assert!(!opts.olw);
            }
            _ => panic!("expected Fmt"),
        }
        let (m, _) = err(parse_args(v(&["fmt"])));
        assert!(m.contains("fmt: missing <script>"));
        let (m, _) = err(parse_args(v(&["fmt", "f.wq", "extra"])));
        assert!(m.starts_with("fmt:"));
        assert!(m.contains("unexpected positional"));
        let (m, _) = err(parse_args(v(&["fmt", "-d3", "f.wq"])));
        assert!(m.contains("fmt: runtime flags"));
    }

    #[test]
    fn exec_inline_stdin_and_extras() {
        let p = ok(parse_args(v(&["exec", "1+1"])));
        matches!(p.command, Command::Exec(ExecSource::Inline(_)));
        let p = ok(parse_args(v(&["exec", "-"])));
        matches!(p.command, Command::Exec(ExecSource::Stdin));
        let (m, _) = err(parse_args(v(&["exec", "1+1", "oops"])));
        assert!(m.contains("unexpected positional"));
    }

    #[test]
    fn interpreter_flag_parses() {
        let p = ok(parse_args(v(&["--interp", "sample", "a.wq"])));
        assert_eq!(p.runtime.interpreter.as_deref(), Some("sample"));
    }

    #[test]
    fn debug_forms_and_last_wins() {
        let p = ok(parse_args(v(&["-d3", "a.wq"])));
        assert_eq!(p.runtime.debug_flags, DebugFlags::from_alias(3).unwrap());
        assert!(matches!(
            parse_args(v(&["-d", "7", "a.wq"])),
            ParseOutcome::Error { .. }
        ));
        let p = ok(parse_args(v(&["-d", "a.wq"])));
        assert_eq!(p.runtime.debug_flags, DebugFlags::from_alias(1).unwrap());
        assert!(matches!(
            parse_args(v(&["-d1", "-d9", "a.wq"])),
            ParseOutcome::Error { .. }
        ));
    }

    #[test]
    fn debug_names_parse() {
        let p = ok(parse_args(v(&["--debug", "token,inst,wqdb-1", "a.wq"])));
        let expected = DebugFlags::from_names(["token", "inst", "wqdb-1"]);
        assert_eq!(p.runtime.debug_flags, expected);
    }

    #[test]
    fn debug_help_names_stay_in_sync() {
        assert_eq!(wqpl::debug_flags::DEBUG_FLAG_NAMES.len(), 5);
    }

    #[test]
    fn dashdash_stops_flag_parsing() {
        let (m, _) = err(parse_args(v(&["-d", "2", "--", "-file", "rest"])));
        // "-file" becomes script; "rest" is extra -> error mentions "rest"
        assert!(m.contains("unexpected positional") && m.contains("rest"));
    }

    #[test]
    fn repl_default_when_empty() {
        let p = ok(parse_args(v(&[])));
        matches!(p.command, Command::Repl);
    }

    #[test]
    fn unknown_flags_are_reported() {
        let (m, _) = err(parse_args(v(&["--bogus", "a.wq"])));
        assert!(m.contains("unknown flags") && m.contains("--bogus"));
    }
}
