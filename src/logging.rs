//! Logging helpers for the IPC library.
//!
//! Provides a global `ENABLE_LOGGING` flag and the [`ipc_log!`] macro so that
//! diagnostic output can be toggled at runtime without a recompile.

use std::sync::atomic::{AtomicBool, Ordering};

pub(crate) static ENABLE_LOGGING: AtomicBool = AtomicBool::new(false);

/// Enable IPC library logging.
pub fn enable_logging() {
    ENABLE_LOGGING.store(true, Ordering::Relaxed);
}

/// Disable IPC library logging.
pub fn disable_logging() {
    ENABLE_LOGGING.store(false, Ordering::Relaxed);
}

/// Returns `true` if IPC library logging is currently enabled.
pub fn is_logging_enabled() -> bool {
    ENABLE_LOGGING.load(Ordering::Relaxed)
}

/// Conditional logging macro. Writes to `stderr` when logging is enabled.
#[macro_export]
macro_rules! ipc_log {
    ($($arg:tt)*) => {
        if $crate::is_logging_enabled() {
            eprintln!("[IPC] {}", format_args!($($arg)*));
        }
    };
}
