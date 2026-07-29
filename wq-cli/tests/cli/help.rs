use crate::strip_ansi;
use crate::support::{ResultContext as _, TestResult, wq_command};

#[test]
fn top_level_help_command_preserves_cli_help() -> TestResult {
    let output = wq_command().arg("help").output().context("run wq help")?;

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).context("stdout is utf8")?;
    let stdout = strip_ansi(&stdout);
    assert!(stdout.contains("Usage: wq"));
    assert!(stdout.contains("Commands:"));
    assert!(stdout.contains("130  Interrupted"));
    Ok(())
}

#[test]
fn subcommand_help_is_still_available() -> TestResult {
    let output = wq_command()
        .args(["help", "exec"])
        .output()
        .context("run wq help exec")?;

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).context("stdout is utf8")?;
    let stdout = strip_ansi(&stdout);
    assert!(stdout.contains("Usage: wq exec"));
    assert!(stdout.contains("Execute inline wq code"));
    Ok(())
}

#[test]
fn topic_flag_bypasses_subcommand_help() -> TestResult {
    let exec = wq_command()
        .args(["help", "--no-pager", "--topic", "exec"])
        .output()
        .context("run wq help --topic exec")?;
    assert!(exec.status.success());
    let stdout = String::from_utf8(exec.stdout).context("stdout is utf8")?;
    let stdout = strip_ansi(&stdout);
    assert!(stdout.contains("exec builtin"));
    assert!(stdout.contains("Run a host process"));

    let fmt = wq_command()
        .args(["help", "--no-pager", "--topic", "fmt"])
        .output()
        .context("run wq help --topic fmt")?;
    assert!(fmt.status.success());
    let stdout = String::from_utf8(fmt.stdout).context("stdout is utf8")?;
    let stdout = strip_ansi(&stdout);
    assert!(stdout.contains("fmt builtin"));
    assert!(stdout.contains("Build a string"));
    Ok(())
}

#[test]
fn reference_docs_render_auto_wraps_as_hard_breaks() -> TestResult {
    let output = wq_command()
        .args([
            "help",
            "--no-pager",
            "--topic",
            "rand",
            "--fold-width",
            "50",
        ])
        .output()
        .context("run wq help --topic rand --fold-width 50")?;

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).context("stdout is utf8")?;
    let stdout = strip_ansi(&stdout);
    assert!(stdout.contains("`rand[]` returns a float in the half-open range\n`0.0..1.0`."));
    Ok(())
}

#[test]
fn reference_docs_render_source_soft_breaks_as_spaces_without_folding() -> TestResult {
    let output = wq_command()
        .args(["help", "--no-pager", "named-arguments"])
        .output()
        .context("run wq help named-arguments")?;

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).context("stdout is utf8")?;
    let stdout = strip_ansi(&stdout);
    assert!(stdout.contains("identifier spelling rules. In a call,"));
    Ok(())
}

#[test]
fn builtin_and_keyword_docs_render() -> TestResult {
    let map = wq_command()
        .args(["help", "--no-pager", "map"])
        .output()
        .context("run wq help map")?;
    assert!(map.status.success());
    let stdout = String::from_utf8(map.stdout).context("stdout is utf8")?;
    let stdout = strip_ansi(&stdout);
    assert!(stdout.contains("map builtin"));
    assert!(stdout.contains("arity: `2 3`"));

    let rng = wq_command()
        .args(["help", "--no-pager", "rng"])
        .output()
        .context("run wq help rng")?;
    assert!(rng.status.success());
    let stdout = String::from_utf8(rng.stdout).context("stdout is utf8")?;
    let stdout = strip_ansi(&stdout);
    assert!(stdout.contains("rng builtin"));
    assert!(stdout.contains("wq-rng-v1"));

    let ret = wq_command()
        .args(["help", "--no-pager", "@r"])
        .output()
        .context("run wq help @r")?;
    assert!(ret.status.success());
    let stdout = String::from_utf8(ret.stdout).context("stdout is utf8")?;
    let stdout = strip_ansi(&stdout);
    assert!(stdout.contains("@r Return"));
    assert!(stdout.contains("Return early"));
    Ok(())
}

#[test]
fn unknown_help_topic_errors() -> TestResult {
    let output = wq_command()
        .args(["help", "--no-pager", "not-a-topic"])
        .output()
        .context("run unknown help")?;

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).context("stderr is utf8")?;
    let stderr = strip_ansi(&stderr);
    assert!(stderr.contains("unknown help topic"));
    Ok(())
}
