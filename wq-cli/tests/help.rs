use std::process::Command;

use anyhow::{Context as _, Result};
use assert_cmd::prelude::*;

#[test]
fn top_level_help_command_preserves_cli_help() -> Result<()> {
    let output = Command::cargo_bin("wq")
        .context("cargo_bin('wq') failed")?
        .arg("help")
        .output()
        .context("run wq help")?;

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).context("stdout is utf8")?;
    assert!(stdout.contains("Usage: wq"));
    assert!(stdout.contains("Commands:"));
    Ok(())
}

#[test]
fn subcommand_help_is_still_available() -> Result<()> {
    let output = Command::cargo_bin("wq")
        .context("cargo_bin('wq') failed")?
        .args(["help", "exec"])
        .output()
        .context("run wq help exec")?;

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).context("stdout is utf8")?;
    assert!(stdout.contains("Usage: wq exec"));
    assert!(stdout.contains("Execute inline wq code"));
    Ok(())
}

#[test]
fn builtin_and_keyword_docs_render() -> Result<()> {
    let map = Command::cargo_bin("wq")
        .context("cargo_bin('wq') failed")?
        .args(["help", "--no-pager", "map"])
        .output()
        .context("run wq help map")?;
    assert!(map.status.success());
    let stdout = String::from_utf8(map.stdout).context("stdout is utf8")?;
    assert!(stdout.contains("map builtin"));
    assert!(stdout.contains("arity: `2 3`"));

    let ret = Command::cargo_bin("wq")
        .context("cargo_bin('wq') failed")?
        .args(["help", "--no-pager", "@r"])
        .output()
        .context("run wq help @r")?;
    assert!(ret.status.success());
    let stdout = String::from_utf8(ret.stdout).context("stdout is utf8")?;
    assert!(stdout.contains("@r Return"));
    assert!(stdout.contains("Return early"));
    Ok(())
}

#[test]
fn unknown_help_topic_errors() -> Result<()> {
    let output = Command::cargo_bin("wq")
        .context("cargo_bin('wq') failed")?
        .args(["help", "--no-pager", "not-a-topic"])
        .output()
        .context("run unknown help")?;

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).context("stderr is utf8")?;
    assert!(stderr.contains("unknown help topic"));
    Ok(())
}
