use crate::support::{ResultContext as _, TestResult, wq_command};

#[test]
fn load_directive_loads_between_code_chunks() -> TestResult {
    let dir = std::env::temp_dir().join(format!("wq-script-directive-{}", std::process::id()));
    std::fs::create_dir_all(&dir).context("create temp script dir")?;
    let lib = dir.join("lib.wq");
    std::fs::write(&lib, "answer:41\n").context("write loaded script")?;

    let source = format!("seed:1\n\\l {}\nanswer+seed\n", lib.display());
    let output = wq_command()
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
fn quoted_load_directive_loads_path_with_spaces() -> TestResult {
    let dir = std::env::temp_dir().join(format!("wq-script-directive-{}", std::process::id()));
    std::fs::create_dir_all(&dir).context("create temp script dir")?;
    let lib = dir.join("quoted lib.wq");
    std::fs::write(&lib, "answer:40\n").context("write loaded script")?;

    let source = format!("seed:2\n\\load \"{}\"\nanswer+seed\n", lib.display());
    let output = wq_command()
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
fn directive_line_inside_incomplete_code_is_not_a_load_directive() -> TestResult {
    let source = "$[true;\n\\l definitely_missing.wq\n;1]\n";
    let output = wq_command()
        .args(["exec", source, "-p"])
        .output()
        .context("run wq exec")?;

    let stdout = String::from_utf8(output.stdout).context("stdout is utf8")?;
    let stderr = String::from_utf8(output.stderr).context("stderr is utf8")?;
    assert!(
        stderr.contains("unrecognized character"),
        "inner directive line should produce a parser error:\n{stdout}{stderr}",
    );
    assert!(
        !stderr.contains("Cannot load"),
        "inner directive line should be parsed as code, not loaded:\n{stdout}{stderr}",
    );
    Ok(())
}
