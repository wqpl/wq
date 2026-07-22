use std::sync::{Arc, Mutex, MutexGuard};

use wqpl::session::SessionInterruptHandle;

pub(crate) const INTERRUPTED_EXIT_STATUS: i32 = 130;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InterruptAction {
    Inactive,
    Requested,
    Escalate,
}

struct ActiveInterrupt {
    handle: SessionInterruptHandle,
    requested: bool,
}

#[derive(Default)]
struct InterruptState {
    active: Mutex<Option<ActiveInterrupt>>,
}

impl InterruptState {
    fn lock_active(&self) -> MutexGuard<'_, Option<ActiveInterrupt>> {
        self.active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn request(&self) -> InterruptAction {
        let mut active = self.lock_active();
        let Some(active) = active.as_mut() else {
            return InterruptAction::Inactive;
        };
        if active.requested {
            return InterruptAction::Escalate;
        }
        active.requested = true;
        active.handle.interrupt();
        InterruptAction::Requested
    }

    fn arm(&self, interrupt: SessionInterruptHandle) -> InterruptGuard<'_> {
        let previous = self.lock_active().replace(ActiveInterrupt {
            handle: interrupt,
            requested: false,
        });
        debug_assert!(previous.is_none());
        InterruptGuard { state: self }
    }
}

pub(crate) struct CliInterrupts {
    state: Arc<InterruptState>,
    #[cfg(unix)]
    signal_handle: signal_hook::iterator::Handle,
    #[cfg(unix)]
    signal_thread: Option<std::thread::JoinHandle<()>>,
}

impl CliInterrupts {
    pub(crate) fn install() -> Result<Self, String> {
        install_platform(Arc::new(InterruptState::default()))
    }

    pub(crate) fn arm(&self, interrupt: SessionInterruptHandle) -> InterruptGuard<'_> {
        self.state.arm(interrupt)
    }
}

fn apply_interrupt_action(action: InterruptAction) {
    if action == InterruptAction::Escalate {
        std::process::exit(INTERRUPTED_EXIT_STATUS);
    }
}

#[cfg(unix)]
fn install_platform(state: Arc<InterruptState>) -> Result<CliInterrupts, String> {
    use signal_hook::consts::SIGINT;
    use signal_hook::iterator::Signals;

    let mut signals = Signals::new([SIGINT]).map_err(|error| error.to_string())?;
    let signal_handle = signals.handle();
    let handler_state = Arc::clone(&state);
    let signal_thread = std::thread::Builder::new()
        .name("wq-sigint".into())
        .spawn(move || {
            for _ in signals.forever() {
                apply_interrupt_action(handler_state.request());
            }
        })
        .map_err(|error| error.to_string())?;
    Ok(CliInterrupts {
        state,
        signal_handle,
        signal_thread: Some(signal_thread),
    })
}

#[cfg(windows)]
fn install_platform(state: Arc<InterruptState>) -> Result<CliInterrupts, String> {
    let handler_state = Arc::clone(&state);
    ctrlc::try_set_handler(move || apply_interrupt_action(handler_state.request()))
        .map_err(|error| error.to_string())?;
    Ok(CliInterrupts { state })
}

#[cfg(not(any(unix, windows)))]
fn install_platform(_state: Arc<InterruptState>) -> Result<CliInterrupts, String> {
    Err("Ctrl-C handling is unavailable on this platform".into())
}

#[cfg(unix)]
impl Drop for CliInterrupts {
    fn drop(&mut self) {
        self.signal_handle.close();
        if let Some(signal_thread) = self.signal_thread.take() {
            let _ = signal_thread.join();
        }
    }
}

pub(crate) struct InterruptGuard<'state> {
    state: &'state InterruptState,
}

impl Drop for InterruptGuard<'_> {
    fn drop(&mut self) {
        self.state.lock_active().take();
    }
}

#[cfg(test)]
mod tests {
    use wqpl::session::Session;
    use wqpl::value::Value;

    use super::*;

    #[test]
    fn requests_only_interrupt_an_armed_session() {
        let state = InterruptState::default();
        let mut session = Session::new();
        let guard = state.arm(session.interrupt_handle());

        assert_eq!(state.request(), InterruptAction::Requested);
        session
            .eval_string("W[T;0]")
            .expect("interrupted evaluation should halt cleanly");
        drop(guard);

        assert!(session.take_interrupt());
        assert_eq!(state.request(), InterruptAction::Inactive);
        assert_eq!(
            session
                .eval_string("1")
                .expect("an inactive controller should not interrupt"),
            Value::Int(1)
        );
        assert!(!session.take_interrupt());
    }

    #[test]
    fn a_repeated_request_escalates_while_the_same_evaluation_is_active() {
        let state = InterruptState::default();
        let session = Session::new();
        let _guard = state.arm(session.interrupt_handle());

        assert_eq!(state.request(), InterruptAction::Requested);
        assert_eq!(state.request(), InterruptAction::Escalate);
    }
}
