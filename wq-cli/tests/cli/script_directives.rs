use std::process::Command;

use anyhow::{Context as _, Result};
use assert_cmd::prelude::*;

#[test]
fn legacy_load_directive_loads_between_code_chunks() -> Result<()> {
    let dir = std::env::temp_dir().join(format!("wq-script-directive-{}", std::process::id()));
    std::fs::create_dir_all(&dir).context("create temp script dir")?;
    let lib = dir.join("lib.wq");
    std::fs::write(&lib, "answer:41\n").context("write loaded script")?;

    let source = format!("seed:1\n!l {}\nanswer+seed\n", lib.display());
    let output = Command::cargo_bin("wq")
        .context("cargo_bin('wq') failed")?
        .args(["exec", &source, "-p"])
        .output()
        .context("run wq exec")?;

    let stdout = String::from_utf8(output.stdout).context("stdout is utf8")?;
    let stderr = String::from_utf8(output.stderr).context("stderr is utf8")?;
    assert!(output.status.success(), "{stdout}{stderr}");
    assert!(
        stdout.contains("42"),
        "unexpected output:\n{stdout}{stderr}"
    );
    Ok(())
}

#[test]
fn bang_line_inside_incomplete_code_is_not_a_load_directive() -> Result<()> {
    let source = "$[true;\n!l definitely_missing.wq\n;1]\n";
    let output = Command::cargo_bin("wq")
        .context("cargo_bin('wq') failed")?
        .args(["exec", source, "-p"])
        .output()
        .context("run wq exec")?;

    let stdout = String::from_utf8(output.stdout).context("stdout is utf8")?;
    let stderr = String::from_utf8(output.stderr).context("stderr is utf8")?;
    assert!(
        stderr.contains("unexpected token: Bang"),
        "inner bang line should produce a parser error:\n{stdout}{stderr}",
    );
    assert!(
        !stderr.contains("Cannot load"),
        "inner bang line should be parsed as code, not loaded:\n{stdout}{stderr}",
    );
    Ok(())
}
