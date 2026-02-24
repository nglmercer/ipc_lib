//! Single Instance Application Library
//!
//! Provides a robust single-instance enforcement mechanism together with
//! multiple IPC communication protocols.
//!
//! # Features
//! - Single-instance enforcement across multiple processes
//! - Multiple communication protocols (Unix sockets, file-based, shared memory, etc.)
//! - Automatic fallback mechanisms for reliability
//! - Cross-platform support (Unix & Windows)
//! - `async`/`await` support via Tokio
//!
//! # Quick Start
//! ```no_run
//! use ipc_lib::enforce_single_instance;
//!
//! #[tokio::main]
//! async fn main() {
//!     match enforce_single_instance("my-app").await {
//!         Ok(true)  => println!("Primary instance – running the app"),
//!         Ok(false) => println!("Secondary instance – another copy is already running"),
//!         Err(e)    => eprintln!("Enforcement error: {}", e),
//!     }
//! }
//! ```

// ---------------------------------------------------------------------------
// Sub-modules
// ---------------------------------------------------------------------------

pub mod communication;

mod broker;
mod client;
mod logging;
mod messenger;
mod server;
mod session;
mod single_instance;

#[cfg(test)]
mod tests;

// ---------------------------------------------------------------------------
// Public re-exports
// ---------------------------------------------------------------------------

pub use client::IpcClient;
pub use communication::current_timestamp;
pub use communication::CommunicationConfig;
pub use communication::CommunicationMessage;
pub use communication::ProtocolType;
pub use communication::SerializationFormat;
pub use logging::{disable_logging, enable_logging, is_logging_enabled};
pub use messenger::Messenger;
pub use server::IpcServer;
pub use session::IpcSession;
pub use single_instance::SingleInstanceApp;

// ---------------------------------------------------------------------------
// Legacy message type (kept for backwards compatibility)
// ---------------------------------------------------------------------------

/// Message types for IPC communication (legacy compatibility).
#[derive(Debug, PartialEq, Clone, serde::Serialize, serde::Deserialize)]
pub enum Message {
    CommandLineArgs(Vec<String>),
    Response(String),
    Error(String),
}

// ---------------------------------------------------------------------------
// Top-level convenience function
// ---------------------------------------------------------------------------

/// Convenience wrapper for simple single-instance enforcement.
///
/// Returns `Ok(true)` when this process becomes the primary instance,
/// `Ok(false)` when another instance is already running, and
/// `Err(…)` when enforcement failed on all configured protocols.
pub async fn enforce_single_instance(
    identifier: &str,
) -> Result<bool, communication::CommunicationError> {
    let mut app = SingleInstanceApp::new(identifier);
    app.enforce_single_instance().await
}
