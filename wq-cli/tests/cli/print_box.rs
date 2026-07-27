use crate::support::{ResultContext as _, TestResult, wq_command};

#[test]
fn exec_print_uses_boxed_display() -> TestResult {
    let output = wq_command()
        .args(["exec", "reshape[1..=6;(2;3)]", "-p"])
        .output()
        .context("run wq exec")?;

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).context("stdout is utf8")?;
    assert!(stdout.contains("  x1 0 1 2"));
    assert!(stdout.contains("x0   - - -"));
    assert!(stdout.contains(" 0 | 1 2 3"));
    Ok(())
}

#[test]
fn script_print_uses_boxed_display() -> TestResult {
    let path = std::env::temp_dir().join(format!("wq-box-print-{}-script.wq", std::process::id()));
    std::fs::write(&path, "reshape[1..=6;(2;3)]\n").context("write script")?;

    let output = wq_command()
        .arg(&path)
        .arg("-p")
        .output()
        .context("run wq script")?;

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).context("stdout is utf8")?;
    assert!(stdout.contains("  x1 0 1 2"));
    assert!(stdout.contains("x0   - - -"));
    assert!(stdout.contains(" 0 | 1 2 3"));
    Ok(())
}

#[test]
fn box_flag_can_disable_boxed_printing() -> TestResult {
    let output = wq_command()
        .args(["--box", "-box", "exec", "reshape[1..=6;(2;3)]", "-p"])
        .output()
        .context("run wq exec")?;

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).context("stdout is utf8")?;
    assert!(stdout.contains("((1;2;3);(4;5;6))"));
    Ok(())
}

#[test]
fn box_flag_can_rewrite_display_config() -> TestResult {
    let output = wq_command()
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
fn box_flag_can_enable_xray_printing() -> TestResult {
    let output = wq_command()
        .args(["--box", "+xray", "exec", "reshape[1..=4;(2;2)]", "-p"])
        .output()
        .context("run wq exec")?;

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).context("stdout is utf8")?;
    assert!(stdout.contains("[xray]\ncategory"));
    assert!(stdout.contains("kind"));
    assert!(stdout.contains("uniform?  T"));
    Ok(())
}

#[test]
fn ragged_print_uses_index_fence_and_values() -> TestResult {
    let output = wq_command()
        .args(["exec", "(1;(2;3);(4;5;(6;7)))", "-p"])
        .output()
        .context("run wq exec")?;

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).context("stdout is utf8")?;
    assert!(stdout.contains("x0"));
    assert!(stdout.contains("0 | 1"));
    assert!(stdout.contains("1 | (2;3)"));
    assert!(stdout.contains("2 | (4;5;(6;7))"));
    Ok(())
}

#[test]
fn box_flag_can_disable_all_box_config() -> TestResult {
    let output = wq_command()
        .args(["--box", "off", "exec", "reshape[1..=4;(2;2)]", "-p"])
        .output()
        .context("run wq exec")?;

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).context("stdout is utf8")?;
    assert!(stdout.contains("((1;2);(3;4))"));
    assert!(!stdout.contains("[xray]"));
    Ok(())
}
