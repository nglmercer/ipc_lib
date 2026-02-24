//! IPC server wrapper for simple, high-level usage.

use crate::communication::{CommunicationError, ProtocolType};
use crate::single_instance::SingleInstanceApp;

/// IPC server with a thin API around [`SingleInstanceApp`].
pub struct IpcServer {
    app: SingleInstanceApp,
}

impl IpcServer {
    /// Create a new IPC server for the given application identifier.
    pub fn new(identifier: &str) -> Result<Self, CommunicationError> {
        let app = SingleInstanceApp::new(identifier);
        Ok(Self { app })
    }

    /// Configure the communication protocol.
    pub fn with_protocol(mut self, protocol: ProtocolType) -> Self {
        self.app = self.app.with_protocol(protocol);
        self
    }

    /// Start the server using the configured protocol.
    pub async fn start(&mut self) -> Result<(), CommunicationError> {
        self.app.start_server().await
    }

    /// Return the server endpoint, if the server has started.
    pub fn endpoint(&self) -> Option<String> {
        self.app.endpoint()
    }
}
