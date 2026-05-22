use std::process::Command;

use anyhow::{Context as _, Result};
use assert_cmd::prelude::*;

#[test]
fn exec_print_uses_boxed_display() -> Result<()> {
    let output = Command::cargo_bin("wq")
        .context("cargo_bin('wq') failed")?
        .args(["exec", "reshape[1..=6;(2;3)]", "-p"])
        .output()
        .context("run wq exec")?;

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).context("stdout is utf8")?;
    assert!(stdout.contains("  a1 0 1 2"));
    assert!(stdout.contains("a0   - - -"));
    assert!(stdout.contains(" 0 | 1 2 3"));
    Ok(())
}

#[test]
fn script_print_uses_boxed_display() -> Result<()> {
    let path = std::env::temp_dir().join(format!(
        "wq-box-print-{}-script.wq",
        std::process::id()
    ));
    std::fs::write(&path, "reshape[1..=6;(2;3)]\n").context("write script")?;

    let output = Command::cargo_bin("wq")
        .context("cargo_bin('wq') failed")?
        .arg(&path)
        .arg("-p")
        .output()
        .context("run wq script")?;

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).context("stdout is utf8")?;
    assert!(stdout.contains("  a1 0 1 2"));
    assert!(stdout.contains("a0   - - -"));
    assert!(stdout.contains(" 0 | 1 2 3"));
    Ok(())
}

#[test]
fn box_flag_can_disable_boxed_printing() -> Result<()> {
    let output = Command::cargo_bin("wq")
        .context("cargo_bin('wq') failed")?
        .args(["--box", "-box", "exec", "reshape[1..=6;(2;3)]", "-p"])
        .output()
        .context("run wq exec")?;

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).context("stdout is utf8")?;
    assert!(stdout.contains("((1;2;3);(4;5;6))"));
    Ok(())
}

#[test]
fn box_flag_can_rewrite_display_config() -> Result<()> {
    let output = Command::cargo_bin("wq")
        .context("cargo_bin('wq') failed")?
        .args(["--box", "box,color", "exec", "reshape[1..=4;(2;2)]", "-p"])
        .output()
        .context("run wq exec")?;

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).context("stdout is utf8")?;
    assert!(stdout.contains("1 2\n3 4"));
    assert!(!stdout.contains("\x1b["));
    Ok(())
}

#[test]
fn box_flag_can_enable_xray_printing() -> Result<()> {
    let output = Command::cargo_bin("wq")
        .context("cargo_bin('wq') failed")?
        .args(["--box", "+xray", "exec", "reshape[1..=4;(2;2)]", "-p"])
        .output()
        .context("run wq exec")?;

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).context("stdout is utf8")?;
    assert!(stdout.contains("[xray] list"));
    assert!(stdout.contains("uniform?: true"));
    Ok(())
}
