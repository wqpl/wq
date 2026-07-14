use std::process::Command;

use anyhow::{Context as _, Result};
use assert_cmd::prelude::*;

use crate::strip_ansi;

#[test]
fn script_receives_only_args_after_separator() -> Result<()> {
    let dir = std::env::temp_dir().join(format!("wq-cli-args-{}", std::process::id()));
    std::fs::create_dir_all(&dir).context("create temp script dir")?;
    let script = dir.join("argv.wq");
    std::fs::write(
        &script,
        "args:argv[];echo (`len:#args;`first:args 0;`second:args 1;`third:args 2)\n",
    )
    .context("write argv script")?;

    let output = Command::cargo_bin("wq")
        .context("cargo_bin('wq') failed")?
        .arg(&script)
        .args(["--", "one", "--two", "three words"])
        .output()
        .context("run argv script")?;

    let stdout = String::from_utf8(output.stdout).context("stdout is utf8")?;
    let stderr = String::from_utf8(output.stderr).context("stderr is utf8")?;
    let visible = strip_ansi(&format!("{stdout}{stderr}"));
    assert!(output.status.success(), "{visible}");
    assert!(
        visible.contains("(`len:3;`first:\"one\";`second:\"--two\";`third:\"three words\")",),
        "unexpected output:\n{visible}",
    );
    Ok(())
}

#[test]
fn exec_receives_args_after_separator() -> Result<()> {
    let output = Command::cargo_bin("wq")
        .context("cargo_bin('wq') failed")?
        .args([
            "exec",
            "(`len:#argv[];`first:argv[] 0;`second:argv[] 1)",
            "-p",
            "--",
            "one",
            "-two",
        ])
        .output()
        .context("run argv exec")?;

    let stdout = String::from_utf8(output.stdout).context("stdout is utf8")?;
    let stderr = String::from_utf8(output.stderr).context("stderr is utf8")?;
    let visible = strip_ansi(&format!("{stdout}{stderr}"));
    assert!(output.status.success(), "{visible}");
    assert!(
        visible.contains("len first second") && visible.contains("2 \"one\" \"-two\""),
        "unexpected output:\n{visible}",
    );
    Ok(())
}

#[test]
fn cliargs_uses_help_and_usage_exit_codes_without_continuing() -> Result<()> {
    let dir = std::env::temp_dir().join(format!("wq-cliargs-{}", std::process::id()));
    std::fs::create_dir_all(&dir).context("create temp script dir")?;
    let script = dir.join("cliargs.wq");
    std::fs::write(
        &script,
        "spec:(`name:\"demo\";`about:\"demo app\";`args:,(`name:`value;`kind:`positional;`required:T));parsed:cliargs[spec];echo \"continued\"\n",
    )
    .context("write cliargs script")?;
    let wq = Command::cargo_bin("wq").context("cargo_bin('wq') failed")?;

    let help = Command::new(wq.get_program())
        .arg(&script)
        .args(["--", "--help"])
        .output()
        .context("run cliargs help")?;
    let help_output = strip_ansi(&format!(
        "{}{}",
        String::from_utf8(help.stdout).context("help stdout is utf8")?,
        String::from_utf8(help.stderr).context("help stderr is utf8")?,
    ));
    assert!(help.status.success(), "{help_output}");
    assert!(help_output.contains("Usage: demo <VALUE>"), "{help_output}");
    assert!(!help_output.contains("continued"), "{help_output}");

    let error = Command::new(wq.get_program())
        .arg(&script)
        .args(["--", "--wat"])
        .output()
        .context("run cliargs usage error")?;
    let error_output = strip_ansi(&format!(
        "{}{}",
        String::from_utf8(error.stdout).context("error stdout is utf8")?,
        String::from_utf8(error.stderr).context("error stderr is utf8")?,
    ));
    assert_eq!(error.status.code(), Some(2), "{error_output}");
    assert!(
        error_output.contains("unknown option '--wat'"),
        "{error_output}"
    );
    assert!(!error_output.contains("continued"), "{error_output}");
    assert!(!error_output.contains("backtrace"), "{error_output}");
    Ok(())
}
