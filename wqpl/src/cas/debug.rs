use crate::builtins::BuiltinContext;

/// Explicit debug sink for one CAS operation.
///
/// Library-only CAS callers use [`Self::disabled`]. Runtime builtins construct
/// this from their session-owned [`BuiltinContext`], so flags and output never
/// depend on process-wide or thread-local state.
#[derive(Clone, Copy, Default)]
pub(crate) struct CasDebug<'a> {
    context: Option<&'a dyn BuiltinContext>,
}

impl<'a> CasDebug<'a> {
    pub(crate) const fn disabled() -> Self {
        Self { context: None }
    }

    pub(crate) fn from_context(context: &'a dyn BuiltinContext) -> Self {
        Self {
            context: Some(context),
        }
    }

    pub(crate) fn enabled(self, flag: u16) -> bool {
        self.context
            .is_some_and(|context| context.debug_log_enabled(flag))
    }

    pub(crate) fn emit_line(self, line: impl AsRef<str>) {
        if let Some(context) = self.context {
            context.emit_debug_log_line(line.as_ref());
        }
    }
}

/// Guarded print for CAS subsystem debugging.
/// Usage: `cas_trace!(debug, CAS, "msg {}", val)`.
macro_rules! cas_trace {
    ($debug:expr, $flag:expr, $($arg:tt)*) => {{
        let _debug = $debug;
        if _debug.enabled($flag) {
            _debug.emit_line(format!($($arg)*));
        }
    }};
}

/// Guarded print for depth-indented CAS subsystem debugging.
/// Usage: `cas_trace_depth!(debug, CAS_VERBOSE, depth, "msg {}", val)`.
macro_rules! cas_trace_depth {
    ($debug:expr, $flag:expr, $depth:expr, $($arg:tt)*) => {{
        let _debug = $debug;
        if _debug.enabled($flag) {
            let _indent = "  ".repeat($depth);
            let _msg = format!($($arg)*);
            _debug.emit_line(format!("{_indent}{_msg}"));
        }
    }};
}
