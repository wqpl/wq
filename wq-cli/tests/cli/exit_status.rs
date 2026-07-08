use std::process::Command;

use anyhow::{Context as _, Result};
use assert_cmd::prelude::*;

use crate::strip_ansi;

#[test]
fn exec_success_exits_zero() -> Result<()> {
    let output = Command::cargo_bin("wq")
        .context("cargo_bin('wq') failed")?
        .args(["exec", "1+1", "-p"])
        .output()
        .context("run successful wq exec")?;

    let stdout = String::from_utf8(output.stdout).context("stdout is utf8")?;
    let stderr = String::from_utf8(output.stderr).context("stderr is utf8")?;
    let visible_stdout = strip_ansi(&stdout);
    let visible_stderr = strip_ansi(&stderr);
    assert!(output.status.success(), "{stdout}{stderr}");
    assert!(
        visible_stdout.contains("2"),
        "unexpected output:\n{visible_stdout}{visible_stderr}"
    );
    Ok(())
}

#[test]
fn exec_runtime_error_exits_one() -> Result<()> {
    let output = Command::cargo_bin("wq")
        .context("cargo_bin('wq') failed")?
        .args(["exec", "raise \"boom\""])
        .output()
        .context("run failing wq exec")?;

    let stdout = String::from_utf8(output.stdout).context("stdout is utf8")?;
    let stderr = String::from_utf8(output.stderr).context("stderr is utf8")?;
    let visible_stderr = strip_ansi(&stderr);
    assert_eq!(
        output.status.code(),
        Some(1),
        "unexpected status for runtime error:\n{stdout}{stderr}",
    );
    assert!(visible_stderr.contains("raise: boom"), "{stdout}{stderr}");
    Ok(())
}

#[test]
fn exec_parse_error_exits_one() -> Result<()> {
    let output = Command::cargo_bin("wq")
        .context("cargo_bin('wq') failed")?
        .args(["exec", "(1;2;3"])
        .output()
        .context("run parse-error wq exec")?;

    let stdout = String::from_utf8(output.stdout).context("stdout is utf8")?;
    let stderr = String::from_utf8(output.stderr).context("stderr is utf8")?;
    let visible_stderr = strip_ansi(&stderr);
    assert_eq!(
        output.status.code(),
        Some(1),
        "unexpected status for parse error:\n{stdout}{stderr}",
    );
    assert!(
        visible_stderr.contains("unexpected end of input"),
        "parse error was not reported:\n{stdout}{stderr}",
    );
    Ok(())
}

#[test]
fn script_runtime_error_exits_one() -> Result<()> {
    let dir = std::env::temp_dir().join(format!("wq-exit-status-{}", std::process::id()));
    std::fs::create_dir_all(&dir).context("create temp script dir")?;
    let script = dir.join("bad.wq");
    std::fs::write(&script, "raise \"script boom\"\n").context("write failing script")?;

    let output = Command::cargo_bin("wq")
        .context("cargo_bin('wq') failed")?
        .arg(&script)
        .output()
        .context("run failing wq script")?;

    let stdout = String::from_utf8(output.stdout).context("stdout is utf8")?;
    let stderr = String::from_utf8(output.stderr).context("stderr is utf8")?;
    let visible_stderr = strip_ansi(&stderr);
    assert_eq!(
        output.status.code(),
        Some(1),
        "unexpected status for script runtime error:\n{stdout}{stderr}",
    );
    assert!(visible_stderr.contains("script boom"), "{stdout}{stderr}");
    Ok(())
}
