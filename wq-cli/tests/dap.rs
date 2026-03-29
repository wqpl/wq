use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};

use anyhow::{Context, Result};
use assert_cmd::prelude::*;

#[test]
fn dap_initialize_handshake() -> Result<()> {
    let mut child = Command::cargo_bin("wq")
        .context("cargo_bin('wq') failed")?
        .arg("dap")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("spawning wq dap")?;

    let mut stdin = child.stdin.take().context("no stdin")?;
    let stdout = child.stdout.take().context("no stdout")?;
    let mut reader = BufReader::new(stdout);

    // Send initialize request
    let init_req =
        r#"{"seq":1,"type":"request","command":"initialize","arguments":{"adapterID":"wq-dap"}}"#;
    send_message(&mut stdin, init_req)?;

    // Read initialize response
    let resp = read_message_with_timeout(&mut reader, std::time::Duration::from_secs(5))
        .context("read initialize response")?
        .context("initialize response timeout")?;
    assert!(
        resp.contains("\"success\":true"),
        "initialize failed: {resp}"
    );
    assert!(
        resp.contains("\"command\":\"initialize\""),
        "missing command: {resp}"
    );

    // Read Initialized event
    let event = read_message_with_timeout(&mut reader, std::time::Duration::from_secs(5))
        .context("read initialized event")?
        .context("initialized event timeout")?;
    assert!(
        event.contains("\"event\":\"initialized\""),
        "missing initialized event: {event}"
    );

    // Send disconnect to clean up
    let disconnect_req = r#"{"seq":2,"type":"request","command":"disconnect"}"#;
    send_message(&mut stdin, disconnect_req)?;

    // Read disconnect response with a timeout to avoid blocking forever
    let resp = read_message_with_timeout(&mut reader, std::time::Duration::from_secs(2))
        .context("read disconnect response")?;
    assert!(
        resp.is_none() || resp.as_ref().unwrap().contains("\"success\":true"),
        "disconnect failed: {:?}",
        resp
    );

    let _ = child.wait();
    Ok(())
}

fn send_message(stdin: &mut std::process::ChildStdin, body: &str) -> Result<()> {
    let msg = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
    stdin.write_all(msg.as_bytes()).context("write stdin")?;
    stdin.flush().context("flush stdin")?;
    Ok(())
}

fn read_message_with_timeout(
    reader: &mut BufReader<std::process::ChildStdout>,
    timeout: std::time::Duration,
) -> Result<Option<String>> {
    let start = std::time::Instant::now();
    let mut header = String::new();
    loop {
        if start.elapsed() > timeout {
            return Ok(None);
        }
        header.clear();
        let bytes = reader.read_line(&mut header).context("read header line")?;
        if bytes == 0 {
            return Ok(None);
        }
        if header.trim().is_empty() {
            continue;
        }
        if header.starts_with("Content-Length:") {
            break;
        }
    }
    let len: usize = header
        .trim()
        .strip_prefix("Content-Length:")
        .context("missing Content-Length")?
        .trim()
        .parse()
        .context("parse Content-Length")?;

    // Read blank line
    let mut blank = String::new();
    reader.read_line(&mut blank).context("read blank line")?;

    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf).context("read body")?;
    Ok(Some(String::from_utf8_lossy(&buf).to_string()))
}
