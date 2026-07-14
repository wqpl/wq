use std::process::Command;

use anyhow::{Context, Result};
use assert_cmd::cargo::CommandCargoExt as _;

#[test]
fn seeded_rng_values_are_callable_and_reproducible() -> Result<()> {
    let output = Command::cargo_bin("wq")
        .context("cargo_bin('wq') failed")?
        .args([
            "exec",
            "a:rng 42;b:rng 42;((a[];a[10];a[-5;5])=(b[];b[10];b[-5;5]);type a;str a)",
            "-p",
        ])
        .output()
        .context("run seeded rng program")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stdout}{stderr}");
    assert!(stdout.contains("T\n\"rng\"\n\"<rng>\""), "{stdout}{stderr}");
    Ok(())
}

#[test]
fn rng_assignment_aliases_generator_state() -> Result<()> {
    let output = Command::cargo_bin("wq")
        .context("cargo_bin('wq') failed")?
        .args(["exec", "a:rng 7;b:a;c:rng 7;(a[];b[])=(c[];c[])", "-p"])
        .output()
        .context("run rng alias program")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stdout}{stderr}");
    assert!(stdout.contains('T'), "{stdout}{stderr}");
    Ok(())
}

#[test]
fn cli_seed_reproduces_the_default_rand_stream() -> Result<()> {
    let run = |program: &str| -> Result<Vec<u8>> {
        let output = Command::cargo_bin("wq")
            .context("cargo_bin('wq') failed")?
            .args(["--seed", "42", "exec", program, "-p"])
            .output()
            .context("run seeded rand program")?;
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{stderr}");
        Ok(output.stdout)
    };

    let default_stream = run("(rand[];rand[100];rand[-10;10])")?;
    assert_eq!(default_stream, run("r:rng 42;(r[];r[100];r[-10;10])")?);
    assert_eq!(default_stream, run("(rand[];rand[100];rand[-10;10])")?);
    Ok(())
}
