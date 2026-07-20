#[cfg(unix)]
mod unix {
    use std::io::{BufRead as _, BufReader, Write as _};
    use std::ops::{Deref, DerefMut};
    use std::process::{Child, Command, Stdio};
    use std::sync::mpsc::{self, Receiver};
    use std::thread;
    use std::time::{Duration, Instant};

    use anyhow::{Context as _, Result, bail};
    use assert_cmd::prelude::*;

    struct ChildGuard(Child);

    impl Deref for ChildGuard {
        type Target = Child;

        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }

    impl DerefMut for ChildGuard {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.0
        }
    }

    impl Drop for ChildGuard {
        fn drop(&mut self) {
            if self.0.try_wait().ok().flatten().is_none() {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
    }

    fn forward_lines(
        reader: impl std::io::Read + Send + 'static,
        sender: mpsc::Sender<String>,
    ) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            for line in BufReader::new(reader).lines().map_while(Result::ok) {
                if sender.send(line).is_err() {
                    break;
                }
            }
        })
    }

    fn receive_until(
        receiver: &Receiver<String>,
        output: &mut Vec<String>,
        expected: &str,
    ) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            match receiver.recv_timeout(Duration::from_millis(100)) {
                Ok(line) => {
                    let found = line.contains(expected);
                    output.push(line);
                    if found {
                        return Ok(());
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        bail!(
            "timed out waiting for {expected:?}; output:\n{}",
            output.join("\n")
        )
    }

    #[test]
    fn ctrl_c_interrupts_only_the_current_repl_turn() -> Result<()> {
        let mut child = ChildGuard(
            Command::cargo_bin("wq")
                .context("cargo_bin('wq') failed")?
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .context("spawn wq REPL")?,
        );
        let mut stdin = child.stdin.take().context("capture REPL stdin")?;
        let stdout = child.stdout.take().context("capture REPL stdout")?;
        let stderr = child.stderr.take().context("capture REPL stderr")?;
        let (sender, receiver) = mpsc::channel();
        let stdout_thread = forward_lines(stdout, sender.clone());
        let stderr_thread = forward_lines(stderr, sender);
        let mut output = Vec::new();

        writeln!(stdin, "echo \"turn-started\";W[T;0]").context("start infinite turn")?;
        stdin.flush().context("flush infinite turn")?;
        receive_until(&receiver, &mut output, "turn-started")?;

        let signal_status = Command::new("kill")
            .args(["-INT", &child.id().to_string()])
            .status()
            .context("send SIGINT to REPL")?;
        assert!(signal_status.success(), "kill failed: {signal_status}");

        writeln!(stdin, "echo \"turn-survived\"").context("write following turn")?;
        stdin.flush().context("flush following turn")?;
        receive_until(&receiver, &mut output, "turn-survived")?;
        drop(stdin);

        let deadline = Instant::now() + Duration::from_secs(5);
        let status = loop {
            if let Some(status) = child.try_wait().context("poll REPL status")? {
                break status;
            }
            if Instant::now() >= deadline {
                child.kill().context("stop timed-out REPL")?;
                let _ = child.wait();
                bail!(
                    "REPL did not exit after EOF; output:\n{}",
                    output.join("\n")
                );
            }
            thread::sleep(Duration::from_millis(20));
        };

        stdout_thread
            .join()
            .expect("stdout reader should not panic");
        stderr_thread
            .join()
            .expect("stderr reader should not panic");
        output.extend(receiver.try_iter());
        assert!(
            status.success(),
            "REPL exited with {status}:\n{}",
            output.join("\n")
        );
        assert!(
            output.iter().any(|line| line.contains("Interrupted")),
            "missing interruption message:\n{}",
            output.join("\n")
        );
        Ok(())
    }
}
