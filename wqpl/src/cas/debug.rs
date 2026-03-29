use crate::session::stdio::wqstderr_println;

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

/// Unified gating check for CAS debug logging.
/// Centralised so the trigger (runtime flag, env var, etc.) can be changed
/// in one place.
pub(crate) fn cas_debug_enabled(flag: u16) -> bool {
    crate::session::dbglog::get_debug_log_flags().contains(flag)
}

/// Emit a CAS debug line with depth-based indentation when `flag` is enabled.
pub(crate) fn cas_debug_log_depth(flag: u16, depth: usize, msg: impl AsRef<str>) {
    if cas_debug_enabled(flag) {
        let indent = "  ".repeat(depth);
        wqstderr_println(format!("{indent}{}", msg.as_ref()));
    }
}
