use crate::strip_ansi;
use crate::support::{ResultContext as _, TestResult, wq_command};

#[test]
fn learn_opens_the_start_chapter_and_hides_example_metadata() -> TestResult {
    let output = wq_command()
        .args(["learn", "--no-pager"])
        .output()
        .context("run wq learn")?;

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).context("stdout is utf8")?;
    let stdout = strip_ansi(&stdout);
    assert!(stdout.contains("# Start Here"));
    assert!(stdout.contains("Read the Result Panel"));
    assert!(stdout.contains("- Standalone blocks start fresh"));
    assert!(stdout.contains("----------------------------------------\nNext:"));
    assert!(stdout.contains("Next: `Values and Display` with `wq learn values`."));
    assert!(!stdout.contains("wq-example"));
    Ok(())
}

#[test]
fn learn_lists_and_opens_named_chapters() -> TestResult {
    let list = wq_command()
        .args(["learn", "--list", "--no-pager"])
        .output()
        .context("list book chapters")?;
    assert!(list.status.success());
    let stdout = String::from_utf8(list.stdout).context("stdout is utf8")?;
    let stdout = strip_ansi(&stdout);
    assert!(stdout.contains("The wq Programming Language"));
    assert!(stdout.contains("`modules`"));
    assert!(stdout.contains("Symbolic Math with CAS (optional)"));

    let chapter = wq_command()
        .args(["learn", "calls", "--no-pager"])
        .output()
        .context("open calls chapter")?;
    assert!(chapter.status.success());
    let stdout = String::from_utf8(chapter.stdout).context("stdout is utf8")?;
    let stdout = strip_ansi(&stdout);
    assert!(stdout.contains("# Calls, Indexing, and Postfix"));
    assert!(stdout.contains("wq learn lists"));
    Ok(())
}

#[test]
fn learn_reports_unknown_chapters() -> TestResult {
    let output = wq_command()
        .args(["learn", "not-a-chapter", "--no-pager"])
        .output()
        .context("open unknown book chapter")?;

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).context("stderr is utf8")?;
    assert!(stderr.contains("wq learn --list"));
    Ok(())
}
