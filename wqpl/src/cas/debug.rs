/// Guarded print for CAS subsystem debugging.
/// Usage: `cas_trace!(CAS, "msg {}", val)` or `cas_trace!(CAS_VERBOSE, "...")`.
macro_rules! cas_trace {
    ($flag:expr, $($arg:tt)*) => {
        if crate::cas::cas_debug_enabled($flag) {
            let _msg = format!($($arg)*);
            crate::session::stdio::wqstderr_println(&_msg);
        }
    };
}

/// Guarded print for depth-indented CAS subsystem debugging.
/// Usage: `cas_trace_depth!(CAS_VERBOSE, depth, "msg {}", val)`.
macro_rules! cas_trace_depth {
    ($flag:expr, $depth:expr, $($arg:tt)*) => {
        if crate::cas::cas_debug_enabled($flag) {
            let _indent = "  ".repeat($depth);
            let _msg = format!($($arg)*);
            crate::session::stdio::wqstderr_println(format!("{_indent}{_msg}"));
        }
    };
}

/// Unified gating check for CAS debug logging.
/// Centralised so the trigger (runtime flag, env var, etc.) can be changed
/// in one place.
pub(crate) fn cas_debug_enabled(flag: u16) -> bool {
    crate::session::dbglog::get_debug_log_flags().contains(flag)
}
