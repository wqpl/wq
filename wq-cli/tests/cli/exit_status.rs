use std::process::Command;

use crate::strip_ansi;
use crate::support::{ResultContext as _, TestResult, test_error, wq_command};

#[test]
fn exec_success_exits_zero() -> TestResult {
    let output = wq_command()
        .args(["exec", "1+1", "-p"])
        .output()
        .context("run successful wq exec")?;

    let stdout = String::from_utf8(output.stdout).context("stdout is utf8")?;
    let stderr = String::from_utf8(output.stderr).context("stderr is utf8")?;
    let visible_stdout = strip_ansi(&stdout);
    let visible_stderr = strip_ansi(&stderr);
    assert!(output.status.success(), "{stdout}{stderr}");
    assert!(
        visible_stdout.contains("2"),
        "unexpected output:\n{visible_stdout}{visible_stderr}"
    );
    Ok(())
}

#[test]
fn exec_runtime_error_exits_one() -> TestResult {
    let output = wq_command()
        .args(["exec", "raise \"boom\""])
        .output()
        .context("run failing wq exec")?;

    let stdout = String::from_utf8(output.stdout).context("stdout is utf8")?;
    let stderr = String::from_utf8(output.stderr).context("stderr is utf8")?;
    let visible_stderr = strip_ansi(&stderr);
    assert_eq!(
        output.status.code(),
        Some(1),
        "unexpected status for runtime error:\n{stdout}{stderr}",
    );
    assert!(visible_stderr.contains("raise: boom"), "{stdout}{stderr}");
    Ok(())
}

#[test]
fn exec_parse_error_exits_one() -> TestResult {
    let output = wq_command()
        .args(["exec", "(1;2;3"])
        .output()
        .context("run parse-error wq exec")?;

    let stdout = String::from_utf8(output.stdout).context("stdout is utf8")?;
    let stderr = String::from_utf8(output.stderr).context("stderr is utf8")?;
    let visible_stderr = strip_ansi(&stderr);
    assert_eq!(
        output.status.code(),
        Some(1),
        "unexpected status for parse error:\n{stdout}{stderr}",
    );
    assert!(
        visible_stderr.contains("unexpected end of input"),
        "parse error was not reported:\n{stdout}{stderr}",
    );
    Ok(())
}

#[test]
fn script_runtime_error_exits_one() -> TestResult {
    let dir = std::env::temp_dir().join(format!("wq-exit-status-{}", std::process::id()));
    std::fs::create_dir_all(&dir).context("create temp script dir")?;
    let script = dir.join("bad.wq");
    std::fs::write(&script, "raise \"script boom\"\n").context("write failing script")?;

    let output = wq_command()
        .arg(&script)
        .output()
        .context("run failing wq script")?;

    let stdout = String::from_utf8(output.stdout).context("stdout is utf8")?;
    let stderr = String::from_utf8(output.stderr).context("stderr is utf8")?;
    let visible_stderr = strip_ansi(&stderr);
    assert_eq!(
        output.status.code(),
        Some(1),
        "unexpected status for script runtime error:\n{stdout}{stderr}",
    );
    assert!(visible_stderr.contains("script boom"), "{stdout}{stderr}");
    Ok(())
}

#[cfg(unix)]
fn interrupt_after_ready(
    command: &mut Command,
) -> TestResult<(std::process::ExitStatus, String, String)> {
    use std::io::{BufRead as _, BufReader, Read as _};
    use std::process::Stdio;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("spawn interruptible wq process")?;
    let stdout = child.stdout.take().context("capture stdout")?;
    let mut stderr = child.stderr.take().context("capture stderr")?;
    let (ready_sender, ready_receiver) = mpsc::channel();
    let stdout_thread = std::thread::spawn(move || -> std::io::Result<String> {
        let mut stdout_text = String::new();
        for line in BufReader::new(stdout).lines() {
            let line = line?;
            let _ = ready_sender.send(line.clone());
            stdout_text.push_str(&line);
            stdout_text.push('\n');
        }
        Ok(stdout_text)
    });
    let stderr_thread = std::thread::spawn(move || -> std::io::Result<String> {
        let mut stderr_text = String::new();
        stderr.read_to_string(&mut stderr_text)?;
        Ok(stderr_text)
    });

    let ready_deadline = Instant::now() + Duration::from_secs(5);
    let readiness_error = loop {
        let remaining = ready_deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break Some("timed out waiting for readiness output");
        }
        match ready_receiver.recv_timeout(remaining.min(Duration::from_millis(100))) {
            Ok(line) if line.contains("started") => break None,
            Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                break Some("stdout closed before readiness output");
            }
        }
    };
    if let Some(reason) = readiness_error {
        let _ = child.kill();
        let _ = child.wait();
        let stdout_text = join_reader(stdout_thread, "stdout")?;
        let stderr_text = join_reader(stderr_thread, "stderr")?;
        return Err(test_error(format!(
            "wq process did not report readiness ({reason}):\n{stdout_text}{stderr_text}"
        )));
    }

    let signal_status = Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .context("send SIGINT")?;
    assert!(signal_status.success(), "kill failed: {signal_status}");

    let exit_deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child.try_wait().context("poll interrupted process")? {
            break status;
        }
        if Instant::now() >= exit_deadline {
            child.kill().context("stop timed-out process")?;
            let _ = child.wait();
            let stdout_text = join_reader(stdout_thread, "stdout")?;
            let stderr_text = join_reader(stderr_thread, "stderr")?;
            return Err(test_error(format!(
                "interrupted wq process did not exit:\n{stdout_text}{stderr_text}"
            )));
        }
        std::thread::sleep(Duration::from_millis(20));
    };

    let stdout_text = join_reader(stdout_thread, "stdout")?;
    let stderr_text = join_reader(stderr_thread, "stderr")?;
    Ok((status, stdout_text, stderr_text))
}

#[cfg(unix)]
fn join_reader(
    reader: std::thread::JoinHandle<std::io::Result<String>>,
    stream: &str,
) -> TestResult<String> {
    Ok(reader
        .join()
        .map_err(|_| test_error(format!("{stream} reader panicked")))??)
}

#[cfg(unix)]
#[test]
fn interrupted_exec_exits_130_without_a_runtime_error() -> TestResult {
    let mut command = wq_command();
    command.args(["exec", "echo \"started\";W[T;0]"]);
    let (status, stdout, stderr) = interrupt_after_ready(&mut command)?;

    assert_eq!(status.code(), Some(130), "{stdout}{stderr}");
    assert!(
        !strip_ansi(&stderr).contains("Error at"),
        "{stdout}{stderr}"
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn interrupted_exec_builtin_terminates_its_child_and_exits_130() -> TestResult {
    let mut command = wq_command();
    command.args(["exec", "echo \"started\";exec[\"sleep\";\"5\"]"]);
    let (status, stdout, stderr) = interrupt_after_ready(&mut command)?;

    assert_eq!(status.code(), Some(130), "{stdout}{stderr}");
    assert!(
        !strip_ansi(&stderr).contains("exec failed"),
        "{stdout}{stderr}"
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn interrupted_script_exits_130_without_a_runtime_error() -> TestResult {
    let script =
        std::env::temp_dir().join(format!("wq-interrupted-script-{}.wq", std::process::id(),));
    std::fs::write(&script, "echo \"started\"\nW[T;0]\n").context("write interruptible script")?;
    let mut command = wq_command();
    command.arg(&script);
    let (status, stdout, stderr) = interrupt_after_ready(&mut command)?;

    assert_eq!(status.code(), Some(130), "{stdout}{stderr}");
    assert!(
        !strip_ansi(&stderr).contains("Error at"),
        "{stdout}{stderr}"
    );
    Ok(())
}
