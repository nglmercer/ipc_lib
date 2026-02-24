//! Persistent IPC session over a connected client transport.
//!
//! [`IpcSession`] is a thin wrapper around a boxed [`CommunicationClient`] that
//! exposes ergonomic `send` / `receive` / `reconnect` helpers.

use crate::communication::{CommunicationClient, CommunicationError, CommunicationMessage};

/// A persistent IPC session backed by a live client connection.
pub struct IpcSession {
    pub(crate) client: Box<dyn CommunicationClient>,
}

impl IpcSession {
    /// Send a message to the remote end.
    pub async fn send(&self, message: CommunicationMessage) -> Result<(), CommunicationError> {
        self.client.send_message(&message).await
    }

    /// Receive the next message from the remote end.
    pub async fn receive(&self) -> Result<CommunicationMessage, CommunicationError> {
        self.client.receive_message().await
    }

    /// Disconnect then reconnect using the same underlying client.
    pub async fn reconnect(&mut self) -> Result<(), CommunicationError> {
        let _ = self.client.disconnect().await;
        self.client.connect().await
    }

    /// Returns `true` if the underlying transport is still connected.
    pub fn is_connected(&self) -> bool {
        self.client.is_connected()
    }
}
