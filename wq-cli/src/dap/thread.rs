use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use wq_dap::event::{BreakpointEventBody, Event, OutputEventBody};
use wq_dap::r#type::{
    Breakpoint, BreakpointEventReason, OutputEventCategory, Scope, StoppedEventReason, Variable,
};
use wqpl::debugger::{DebugResume, Debugger, PauseEvent, PauseReason};
use wqpl::session::stdio::{WqIoError, WqOutput};
use wqpl::session::{Session, SessionInterruptHandle};
use wqpl::style::ColorMode;

use crate::dap::adapter;
use crate::load::load_script;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VmState {
    Starting,
    Paused,
    Running,
    Terminating,
    Exited,
}

pub(crate) struct VmStatus {
    inner: Mutex<VmStatusInner>,
    changed: Condvar,
}

struct VmStatusInner {
    state: VmState,
    interrupt: Option<SessionInterruptHandle>,
}

impl VmStatus {
    pub(crate) fn new() -> Self {
        Self {
            inner: Mutex::new(VmStatusInner {
                state: VmState::Starting,
                interrupt: None,
            }),
            changed: Condvar::new(),
        }
    }

    pub(crate) fn set(&self, state: VmState) {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .state = state;
        self.changed.notify_all();
    }

    pub(crate) fn state(&self) -> VmState {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .state
    }

    fn install_interrupt(&self, interrupt: SessionInterruptHandle) {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if matches!(inner.state, VmState::Terminating | VmState::Exited) {
            interrupt.interrupt();
        }
        inner.interrupt = Some(interrupt);
    }

    pub(crate) fn request_terminate(&self) {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if inner.state != VmState::Exited {
            inner.state = VmState::Terminating;
            if let Some(interrupt) = &inner.interrupt {
                interrupt.interrupt();
            }
        }
        self.changed.notify_all();
    }

    fn mark_paused(&self) -> bool {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if matches!(inner.state, VmState::Terminating | VmState::Exited) {
            return false;
        }
        inner.state = VmState::Paused;
        self.changed.notify_all();
        true
    }

    fn mark_running(&self) {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !matches!(inner.state, VmState::Terminating | VmState::Exited) {
            inner.state = VmState::Running;
        }
        self.changed.notify_all();
    }

    pub(crate) fn begin_resume(&self) -> bool {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if inner.state != VmState::Paused {
            return false;
        }
        inner.state = VmState::Running;
        self.changed.notify_all();
        true
    }

    /// Wait for the initial entry pause. Requests made while already running
    /// or after exit fail immediately instead of being queued for a later stop.
    pub(crate) fn wait_until_paused(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        loop {
            match inner.state {
                VmState::Paused => return true,
                VmState::Running | VmState::Terminating | VmState::Exited => return false,
                VmState::Starting => {}
            }
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            inner = match self.changed.wait_timeout(inner, deadline - now) {
                Ok((inner, _)) => inner,
                Err(poisoned) => poisoned.into_inner().0,
            };
        }
    }
}

pub(crate) enum VmCommand {
    Continue,
    StepIn,
    StepOver,
    StepOut,
    SetBreakpoints {
        source_path: String,
        lines: Vec<usize>,
        tx: Sender<Vec<Breakpoint>>,
    },
    StackTrace {
        start_frame: Option<usize>,
        levels: Option<usize>,
        tx: Sender<adapter::StackTracePage>,
    },
    Scopes {
        frame_id: usize,
        tx: Sender<Vec<Scope>>,
    },
    Variables {
        variables_reference: usize,
        tx: Sender<Vec<Variable>>,
    },
}

struct DapOutput {
    event_tx: Sender<Event>,
    category: OutputEventCategory,
}

impl WqOutput for DapOutput {
    fn write(&mut self, text: &str) -> Result<(), WqIoError> {
        self.event_tx
            .send(Event::Output(OutputEventBody {
                category: Some(self.category.clone()),
                output: text.to_string(),
                ..Default::default()
            }))
            .map_err(|error| WqIoError::Other(format!("DAP output channel closed: {error}")))
    }
}

pub(crate) fn run_vm(
    script_path: &str,
    event_tx: Sender<Event>,
    cmd_rx: Receiver<VmCommand>,
    status: Arc<VmStatus>,
) {
    let mut session = Session::new();
    status.install_interrupt(session.interrupt_handle());
    session.set_stdout(Box::new(DapOutput {
        event_tx: event_tx.clone(),
        category: OutputEventCategory::Stdout,
    }));
    session.set_stderr(Box::new(DapOutput {
        event_tx: event_tx.clone(),
        category: OutputEventCategory::Stderr,
    }));
    session.set_color_mode(ColorMode::Never);
    session.set_wqdb(true);
    let pause_event_tx = event_tx.clone();
    let pause_status = Arc::clone(&status);
    session.set_pause_handler(move |event, debugger| {
        dap_on_pause(event, debugger, &pause_event_tx, &cmd_rx, &pause_status)
    });

    let loading = RefCell::new(HashSet::new());
    let exit_code = match load_script(&mut session, script_path, &loading, true) {
        Ok(_) => 0,
        Err(err) => {
            let _ = event_tx.send(Event::Output(OutputEventBody {
                category: Some(OutputEventCategory::Stderr),
                output: format!("[wq-dap] load error: {err:?}\n"),
                ..Default::default()
            }));
            1
        }
    };

    status.set(VmState::Exited);
    let _ = event_tx.send(Event::Exited(wq_dap::event::ExitedEventBody { exit_code }));
    let _ = event_tx.send(Event::Terminated(None));
}

fn dap_on_pause(
    event: PauseEvent,
    debugger: &mut Debugger<'_>,
    event_tx: &Sender<Event>,
    cmd_rx: &Receiver<VmCommand>,
    status: &VmStatus,
) -> DebugResume {
    if !status.mark_paused() {
        return DebugResume::Continue;
    }
    for breakpoint in debugger.take_resolved_source_breakpoints() {
        let breakpoint = adapter::build_source_breakpoint(debugger.debug_info(), &breakpoint);
        let _ = event_tx.send(Event::Breakpoint(BreakpointEventBody {
            reason: BreakpointEventReason::Changed,
            breakpoint,
        }));
    }
    let (reason, description, hit_breakpoint_ids) = match event.reason {
        PauseReason::Entry => (StoppedEventReason::Entry, Some("entry".to_string()), None),
        PauseReason::Step => (
            StoppedEventReason::Step,
            Some("single step".to_string()),
            None,
        ),
        PauseReason::Breakpoint { id } => (
            StoppedEventReason::Breakpoint,
            Some("hit breakpoint".to_string()),
            Some(vec![id as i64]),
        ),
        PauseReason::TemporaryBreakpoint => (
            StoppedEventReason::Breakpoint,
            Some("hit temporary breakpoint".to_string()),
            None,
        ),
        PauseReason::ExplicitPause { .. } => (
            StoppedEventReason::Pause,
            Some("explicit pause".to_string()),
            None,
        ),
    };

    let _ = event_tx.send(Event::Stopped(wq_dap::event::StoppedEventBody {
        reason,
        description,
        thread_id: Some(1),
        all_threads_stopped: Some(true),
        preserve_focus_hint: None,
        text: None,
        hit_breakpoint_ids,
    }));

    loop {
        let cmd = cmd_rx.recv().ok();
        match cmd {
            Some(VmCommand::Continue) => return resume(status, DebugResume::Continue),
            Some(VmCommand::StepIn) => return resume(status, DebugResume::StepIn),
            Some(VmCommand::StepOver) => return resume(status, DebugResume::StepOver),
            Some(VmCommand::StepOut) => return resume(status, DebugResume::StepOut),
            Some(VmCommand::SetBreakpoints {
                source_path,
                lines,
                tx,
            }) => {
                let bps = adapter::set_breakpoints(debugger, &source_path, &lines);
                let _ = tx.send(bps);
            }
            Some(VmCommand::StackTrace {
                start_frame,
                levels,
                tx,
            }) => {
                let frames = adapter::build_stack_trace(debugger, start_frame, levels);
                let _ = tx.send(frames);
            }
            Some(VmCommand::Scopes { frame_id, tx }) => {
                let scopes = adapter::build_scopes(debugger, frame_id);
                let _ = tx.send(scopes);
            }
            Some(VmCommand::Variables {
                variables_reference,
                tx,
            }) => {
                let vars = adapter::build_variables(debugger, variables_reference);
                let _ = tx.send(vars);
            }
            None => return resume(status, DebugResume::Continue),
        }
    }
}

fn resume(status: &VmStatus, action: DebugResume) -> DebugResume {
    status.mark_running();
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vm_status_waits_for_initial_pause_but_rejects_running_state() {
        let status = Arc::new(VmStatus::new());
        let pause_status = Arc::clone(&status);
        let handle = std::thread::spawn(move || pause_status.set(VmState::Paused));

        assert!(status.wait_until_paused(Duration::from_secs(1)));
        handle.join().expect("status thread should finish");

        status.set(VmState::Running);
        assert!(!status.wait_until_paused(Duration::from_secs(1)));
    }

    #[test]
    fn terminated_status_is_not_overwritten_by_pause_handler_cleanup() {
        let status = VmStatus::new();
        status.set(VmState::Exited);

        assert_eq!(
            resume(&status, DebugResume::Continue),
            DebugResume::Continue
        );
        assert_eq!(status.state(), VmState::Exited);
    }

    #[test]
    fn dap_output_is_forwarded_as_a_protocol_event() {
        let (event_tx, event_rx) = std::sync::mpsc::channel();
        let mut output = DapOutput {
            event_tx,
            category: OutputEventCategory::Stderr,
        };

        output.write("problem\n").expect("DAP output write");

        let Event::Output(event) = event_rx.recv().expect("output event") else {
            panic!("expected output event");
        };
        assert!(matches!(event.category, Some(OutputEventCategory::Stderr)));
        assert_eq!(event.output, "problem\n");
    }
}
