use std::sync::{Arc, Mutex, MutexGuard};

use wqpl::session::SessionInterruptHandle;

#[derive(Default)]
struct InterruptState {
    active: Mutex<Option<SessionInterruptHandle>>,
}

impl InterruptState {
    fn lock_active(&self) -> MutexGuard<'_, Option<SessionInterruptHandle>> {
        self.active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn request(&self) {
        if let Some(interrupt) = self.lock_active().as_ref() {
            interrupt.interrupt();
        }
    }

    fn arm(&self, interrupt: SessionInterruptHandle) -> InterruptGuard<'_> {
        let previous = self.lock_active().replace(interrupt);
        debug_assert!(previous.is_none());
        InterruptGuard { state: self }
    }
}

pub(super) struct ReplInterrupts {
    state: Arc<InterruptState>,
    #[cfg(unix)]
    signal_handle: signal_hook::iterator::Handle,
    #[cfg(unix)]
    signal_thread: Option<std::thread::JoinHandle<()>>,
}

impl ReplInterrupts {
    pub(super) fn install() -> Result<Self, String> {
        install_platform(Arc::new(InterruptState::default()))
    }

    pub(super) fn arm(&self, interrupt: SessionInterruptHandle) -> InterruptGuard<'_> {
        self.state.arm(interrupt)
    }
}

#[cfg(unix)]
fn install_platform(state: Arc<InterruptState>) -> Result<ReplInterrupts, String> {
    use signal_hook::consts::SIGINT;
    use signal_hook::iterator::Signals;

    let mut signals = Signals::new([SIGINT]).map_err(|error| error.to_string())?;
    let signal_handle = signals.handle();
    let handler_state = Arc::clone(&state);
    let signal_thread = std::thread::Builder::new()
        .name("wq-repl-sigint".into())
        .spawn(move || {
            for _ in signals.forever() {
                handler_state.request();
            }
        })
        .map_err(|error| error.to_string())?;
    Ok(ReplInterrupts {
        state,
        signal_handle,
        signal_thread: Some(signal_thread),
    })
}

#[cfg(windows)]
fn install_platform(state: Arc<InterruptState>) -> Result<ReplInterrupts, String> {
    let handler_state = Arc::clone(&state);
    ctrlc::try_set_handler(move || handler_state.request()).map_err(|error| error.to_string())?;
    Ok(ReplInterrupts { state })
}

#[cfg(not(any(unix, windows)))]
fn install_platform(_state: Arc<InterruptState>) -> Result<ReplInterrupts, String> {
    Err("Ctrl-C handling is unavailable on this platform".into())
}

#[cfg(unix)]
impl Drop for ReplInterrupts {
    fn drop(&mut self) {
        self.signal_handle.close();
        if let Some(signal_thread) = self.signal_thread.take() {
            let _ = signal_thread.join();
        }
    }
}

pub(super) struct InterruptGuard<'state> {
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

        state.request();
        session
            .eval_string("W[T;0]")
            .expect("interrupted evaluation should halt cleanly");
        drop(guard);

        assert!(session.take_interrupt());
        state.request();
        assert_eq!(
            session
                .eval_string("1")
                .expect("an inactive controller should not interrupt"),
            Value::Int(1)
        );
        assert!(!session.take_interrupt());
    }
}
