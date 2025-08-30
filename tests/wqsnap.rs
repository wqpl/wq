use anyhow::{Context, Result};
use assert_cmd::prelude::*;
use std::{fs, path::Path, process::Command};

fn run_wq(path: &Path) -> Result<String> {
    let out = Command::cargo_bin("wq")
        .context("cargo_bin('wq') failed — is [[bin]] wq defined in this workspace?")?
        .arg(path)
        .output()
        .with_context(|| format!("running wq on {}", path.display()))?;

    let mut text = String::new();
    text.push_str(&String::from_utf8_lossy(&out.stdout));
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    Ok(text)
}

fn is_excluded(path: &Path) -> bool {
    let exclude_comment = ["//exclude", "// exclude"];
    fs::read_to_string(path)
        .ok()
        .and_then(|s| {
            s.lines().next().map(|l| {
                let trimmed = l.trim_start().to_ascii_lowercase();
                exclude_comment.iter().any(|&pat| trimmed.starts_with(pat))
            })
        })
        .unwrap_or(false)
}

#[test]
fn wq_snapshots() {
    // Requires: insta = { version = "1", features = ["glob"] }
    insta::glob!("wq/*.wq", |path| {
        if is_excluded(path) {
            eprintln!("(skipped) {}", path.display());
            return;
        }

        let output = run_wq(path).expect("wq run failed");

        // Stabilize
        insta::with_settings!({
            filters => vec![
                (r"\r\n", "\n"), // normalize line endings
                (r"\b\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?Z\b", "<TIMESTAMP>"),
                (r"0x[0-9a-fA-F]+", "<ADDR>"),
                (r"(?:/|[A-Za-z]:\\)[^\s\n]+/target/[^\s\n]+", "<TARGET_PATH>"),
            ],
        }, {
            let name = path.file_stem().unwrap().to_string_lossy();
            insta::assert_snapshot!(format!("wq__{}", name), output);
        });
    });
}
