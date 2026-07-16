use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use assert_cmd::prelude::*;

type DapMessageReceiver = Receiver<Result<String, String>>;
type LaunchedDap = (ChildGuard, ChildStdin, DapMessageReceiver);

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
    let reader = spawn_message_reader(stdout);

    // Send initialize request
    let init_req =
        r#"{"seq":1,"type":"request","command":"initialize","arguments":{"adapterID":"wq-dap"}}"#;
    send_message(&mut stdin, init_req)?;

    // Read initialize response
    let resp = read_message_with_timeout(&reader, Duration::from_secs(5))
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
    let event = read_message_with_timeout(&reader, Duration::from_secs(5))
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
    let resp = read_message_with_timeout(&reader, Duration::from_secs(2))
        .context("read disconnect response")?;
    assert!(
        resp.is_none() || resp.as_ref().unwrap().contains("\"success\":true"),
        "disconnect failed: {:?}",
        resp
    );

    let _ = child.wait();
    Ok(())
}

#[test]
fn dap_running_requests_and_disconnect_are_bounded() -> Result<()> {
    let script = std::env::temp_dir().join(format!("wq-dap-running-{}.wq", std::process::id()));
    std::fs::write(&script, "W[true;0]\n").context("write running DAP script")?;
    let mut child = Command::cargo_bin("wq")
        .context("cargo_bin('wq') failed")?
        .args(["dap", script.to_str().context("script path is utf8")?])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("spawning running wq dap")?;
    let mut stdin = child.stdin.take().context("no stdin")?;
    let stdout = child.stdout.take().context("no stdout")?;
    let reader = spawn_message_reader(stdout);

    send_message(
        &mut stdin,
        r#"{"seq":1,"type":"request","command":"initialize","arguments":{"adapterID":"wq-dap"}}"#,
    )?;
    recv_until(&reader, Duration::from_secs(5), |message| {
        message.contains("\"command\":\"initialize\"")
    })?
    .context("initialize response timeout")?;
    recv_until(&reader, Duration::from_secs(5), |message| {
        message.contains("\"event\":\"initialized\"")
    })?
    .context("initialized event timeout")?;

    send_message(
        &mut stdin,
        r#"{"seq":2,"type":"request","command":"launch","arguments":{}}"#,
    )?;
    recv_until(&reader, Duration::from_secs(5), |message| {
        message.contains("\"command\":\"launch\"")
    })?
    .context("launch response timeout")?;
    recv_until(&reader, Duration::from_secs(5), |message| {
        message.contains("\"event\":\"stopped\"")
    })?
    .context("entry stop timeout")?;

    send_message(
        &mut stdin,
        r#"{"seq":3,"type":"request","command":"continue","arguments":{"threadId":1}}"#,
    )?;
    recv_until(&reader, Duration::from_secs(2), |message| {
        message.contains("\"command\":\"continue\"")
    })?
    .context("continue response timeout")?;

    send_message(
        &mut stdin,
        r#"{"seq":4,"type":"request","command":"stackTrace","arguments":{"threadId":1}}"#,
    )?;
    let running_response = recv_until(&reader, Duration::from_secs(2), |message| {
        message.contains("\"command\":\"stackTrace\"")
    })?
    .context("running stackTrace response timeout")?;
    assert!(running_response.contains("\"success\":false"));
    assert!(running_response.contains("debuggee is running"));

    send_message(
        &mut stdin,
        r#"{"seq":5,"type":"request","command":"disconnect","arguments":{"terminateDebuggee":true}}"#,
    )?;
    let disconnect = recv_until(&reader, Duration::from_secs(2), |message| {
        message.contains("\"command\":\"disconnect\"")
    })?
    .context("disconnect response timeout")?;
    assert!(disconnect.contains("\"success\":true"));

    let status = child.wait().context("wait for DAP process")?;
    assert!(status.success());
    Ok(())
}

#[test]
fn dap_pending_breakpoint_resolves_in_a_later_script_region() -> Result<()> {
    let script = TempScript::new("pending-breakpoint", "\\p\nvalue:1\n")?;
    let (mut child, mut stdin, reader) = spawn_launched_dap(script.path())?;
    let set_breakpoints = format!(
        r#"{{"seq":3,"type":"request","command":"setBreakpoints","arguments":{{"source":{{"path":"{}"}},"breakpoints":[{{"line":2}}]}}}}"#,
        script.path().display()
    );
    send_message(&mut stdin, &set_breakpoints)?;
    let response = recv_until(&reader, Duration::from_secs(2), |message| {
        message.contains("\"command\":\"setBreakpoints\"")
    })?
    .context("setBreakpoints response timeout")?;
    assert!(response.contains("\"success\":true"), "{response}");
    assert!(response.contains("\"verified\":false"), "{response}");
    assert!(response.contains("has not been compiled yet"), "{response}");

    send_message(
        &mut stdin,
        r#"{"seq":4,"type":"request","command":"continue","arguments":{"threadId":1}}"#,
    )?;
    let messages = collect_until(&reader, Duration::from_secs(5), |messages| {
        messages
            .iter()
            .any(|message| message.contains("\"command\":\"continue\""))
            && messages.iter().any(|message| {
                message.contains("\"event\":\"breakpoint\"")
                    && message.contains("\"reason\":\"changed\"")
                    && message.contains("\"verified\":true")
            })
            && messages.iter().any(|message| {
                message.contains("\"event\":\"stopped\"")
                    && message.contains("\"reason\":\"breakpoint\"")
            })
    })?;
    assert!(
        messages
            .iter()
            .any(|message| message.contains("\"command\":\"continue\"")),
        "missing continue response: {messages:?}"
    );
    assert!(
        messages.iter().any(|message| {
            message.contains("\"event\":\"breakpoint\"")
                && message.contains("\"reason\":\"changed\"")
                && message.contains("\"verified\":true")
        }),
        "missing verified breakpoint event: {messages:?}"
    );
    assert!(
        messages.iter().any(|message| {
            message.contains("\"event\":\"stopped\"")
                && message.contains("\"reason\":\"breakpoint\"")
        }),
        "missing breakpoint stop: {messages:?}"
    );

    send_message(
        &mut stdin,
        r#"{"seq":5,"type":"request","command":"continue","arguments":{"threadId":1}}"#,
    )?;
    assert_exit_event_order(&reader)?;
    disconnect_dap(&mut child, &mut stdin, &reader, 6)
}

#[test]
fn dap_terminate_interrupts_a_running_debuggee() -> Result<()> {
    let script = TempScript::new("terminate", "W[true;0]\n")?;
    let (mut child, mut stdin, reader) = spawn_launched_dap(script.path())?;
    send_message(
        &mut stdin,
        r#"{"seq":3,"type":"request","command":"continue","arguments":{"threadId":1}}"#,
    )?;
    recv_until(&reader, Duration::from_secs(2), |message| {
        message.contains("\"command\":\"continue\"")
    })?
    .context("continue response timeout")?;

    send_message(
        &mut stdin,
        r#"{"seq":4,"type":"request","command":"terminate","arguments":{}}"#,
    )?;
    let messages = collect_until(&reader, Duration::from_secs(2), |messages| {
        messages.iter().any(|message| {
            message.contains("\"command\":\"terminate\"") && message.contains("\"success\":true")
        }) && messages
            .iter()
            .any(|message| message.contains("\"event\":\"terminated\""))
    })?;
    assert!(
        messages.iter().any(|message| {
            message.contains("\"command\":\"terminate\"") && message.contains("\"success\":true")
        }),
        "missing successful terminate response: {messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|message| message.contains("\"event\":\"exited\"")),
        "missing exited event after terminate: {messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|message| message.contains("\"event\":\"terminated\"")),
        "missing terminated event after terminate: {messages:?}"
    );
    disconnect_dap(&mut child, &mut stdin, &reader, 5)
}

#[test]
fn dap_normal_completion_emits_exited_then_terminated() -> Result<()> {
    let script = TempScript::new("normal-exit", "1\n")?;
    let (mut child, mut stdin, reader) = spawn_launched_dap(script.path())?;
    send_message(
        &mut stdin,
        r#"{"seq":3,"type":"request","command":"continue","arguments":{"threadId":1}}"#,
    )?;

    assert_exit_event_order(&reader)?;
    disconnect_dap(&mut child, &mut stdin, &reader, 4)
}

fn spawn_launched_dap(script: &Path) -> Result<LaunchedDap> {
    let child = Command::cargo_bin("wq")
        .context("cargo_bin('wq') failed")?
        .args(["dap", script.to_str().context("script path is utf8")?])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("spawning wq dap")?;
    let mut child = ChildGuard::new(child);
    let mut stdin = child.child.stdin.take().context("no stdin")?;
    let stdout = child.child.stdout.take().context("no stdout")?;
    let reader = spawn_message_reader(stdout);

    send_message(
        &mut stdin,
        r#"{"seq":1,"type":"request","command":"initialize","arguments":{"adapterID":"wq-dap"}}"#,
    )?;
    let initialize = recv_until(&reader, Duration::from_secs(5), |message| {
        message.contains("\"command\":\"initialize\"")
    })?
    .context("initialize response timeout")?;
    assert!(initialize.contains("\"success\":true"), "{initialize}");
    assert!(
        initialize.contains("\"supportsTerminateRequest\":true"),
        "terminate capability was not advertised: {initialize}"
    );
    recv_until(&reader, Duration::from_secs(5), |message| {
        message.contains("\"event\":\"initialized\"")
    })?
    .context("initialized event timeout")?;

    send_message(
        &mut stdin,
        r#"{"seq":2,"type":"request","command":"launch","arguments":{}}"#,
    )?;
    recv_until(&reader, Duration::from_secs(5), |message| {
        message.contains("\"command\":\"launch\"")
    })?
    .context("launch response timeout")?;
    recv_until(&reader, Duration::from_secs(5), |message| {
        message.contains("\"event\":\"stopped\"")
    })?
    .context("entry stop timeout")?;

    Ok((child, stdin, reader))
}

fn assert_exit_event_order(reader: &DapMessageReceiver) -> Result<()> {
    let messages = collect_until(reader, Duration::from_secs(5), |messages| {
        messages
            .iter()
            .any(|message| message.contains("\"event\":\"terminated\""))
    })?;
    let exited = messages
        .iter()
        .position(|message| message.contains("\"event\":\"exited\""))
        .expect("exited event should be present");
    let terminated = messages
        .iter()
        .position(|message| message.contains("\"event\":\"terminated\""))
        .expect("terminated event should be present");
    assert!(
        exited < terminated,
        "terminated must follow exited: {messages:?}"
    );
    Ok(())
}

fn disconnect_dap(
    child: &mut ChildGuard,
    stdin: &mut ChildStdin,
    reader: &DapMessageReceiver,
    seq: usize,
) -> Result<()> {
    send_message(
        stdin,
        &format!(r#"{{"seq":{seq},"type":"request","command":"disconnect"}}"#),
    )?;
    let response = recv_until(reader, Duration::from_secs(2), |message| {
        message.contains("\"command\":\"disconnect\"")
    })?;
    let response = match response {
        Some(response) => response,
        None => {
            let status = child.child.try_wait().context("query DAP process status")?;
            let mut stderr = String::new();
            if status.is_some()
                && let Some(mut child_stderr) = child.child.stderr.take()
            {
                child_stderr
                    .read_to_string(&mut stderr)
                    .context("read DAP stderr")?;
                child.waited = true;
            }
            anyhow::bail!(
                "disconnect response timeout; process status: {status:?}; stderr: {stderr:?}"
            );
        }
    };
    assert!(response.contains("\"success\":true"), "{response}");
    let status = child.wait_for_exit().context("wait for DAP process")?;
    assert!(status.success());
    Ok(())
}

fn collect_until(
    reader: &DapMessageReceiver,
    timeout: Duration,
    predicate: impl Fn(&[String]) -> bool,
) -> Result<Vec<String>> {
    let deadline = Instant::now() + timeout;
    let mut messages = Vec::new();
    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let Some(message) = read_message_with_timeout(reader, remaining)? else {
            break;
        };
        messages.push(message);
        if predicate(&messages) {
            break;
        }
    }
    Ok(messages)
}

struct TempScript {
    path: PathBuf,
}

impl TempScript {
    fn new(label: &str, source: &str) -> Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "wq-dap-{label}-{}-{:?}.wq",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::write(&path, source).context("write DAP test script")?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempScript {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

struct ChildGuard {
    child: Child,
    waited: bool,
}

impl ChildGuard {
    const fn new(child: Child) -> Self {
        Self {
            child,
            waited: false,
        }
    }

    fn wait_for_exit(&mut self) -> std::io::Result<ExitStatus> {
        let status = self.child.wait()?;
        self.waited = true;
        Ok(status)
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if !self.waited {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn send_message(stdin: &mut std::process::ChildStdin, body: &str) -> Result<()> {
    let msg = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
    stdin.write_all(msg.as_bytes()).context("write stdin")?;
    stdin.flush().context("flush stdin")?;
    Ok(())
}

fn read_message_with_timeout(
    reader: &DapMessageReceiver,
    timeout: Duration,
) -> Result<Option<String>> {
    match reader.recv_timeout(timeout) {
        Ok(Ok(message)) => Ok(Some(message)),
        Ok(Err(error)) => Err(anyhow::anyhow!(error)),
        Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => Ok(None),
    }
}

fn recv_until(
    reader: &DapMessageReceiver,
    timeout: Duration,
    predicate: impl Fn(&str) -> bool,
) -> Result<Option<String>> {
    let deadline = Instant::now() + timeout;
    loop {
        let now = Instant::now();
        if now >= deadline {
            return Ok(None);
        }
        let Some(message) = read_message_with_timeout(reader, deadline - now)? else {
            return Ok(None);
        };
        if predicate(&message) {
            return Ok(Some(message));
        }
    }
}

fn spawn_message_reader(stdout: std::process::ChildStdout) -> DapMessageReceiver {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            match read_message(&mut reader) {
                Ok(Some(message)) => {
                    if tx.send(Ok(message)).is_err() {
                        break;
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    let _ = tx.send(Err(error.to_string()));
                    break;
                }
            }
        }
    });
    rx
}

fn read_message(reader: &mut BufReader<std::process::ChildStdout>) -> Result<Option<String>> {
    let mut header = String::new();
    loop {
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
