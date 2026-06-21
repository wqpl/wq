use std::process::Command;

use anyhow::{Context as _, Result};
use assert_cmd::prelude::*;

#[test]
fn fmt_wrap_only_preserves_source_spelling() -> Result<()> {
    let path = std::env::temp_dir().join(format!("wq-fmt-wrap-only-{}.wq", std::process::id()));
    std::fs::write(&path, "f[(1; 2; 3; 4; 5)]\n").context("write script")?;

    let output = Command::cargo_bin("wq")
        .context("cargo_bin('wq') failed")?
        .args([
            "fmt",
            "--wrap-only",
            "--width",
            "8",
            path.to_str().context("temp path is utf8")?,
        ])
        .output()
        .context("run wq fmt --wrap-only")?;

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).context("stdout is utf8")?;
    assert_eq!(stdout, "f[(1; 2;\n    3;\n    4;\n    5)]\n");
    Ok(())
}
