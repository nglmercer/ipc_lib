//! IPC client wrapper for simple, high-level usage.
//!
//! [`IpcClient`] provides a builder API for configuring a client connection and
//! several convenience methods for connecting, sending, and receiving messages.

use crate::communication::{
    CommunicationConfig, CommunicationError, CommunicationFactory, CommunicationMessage,
    ProtocolType, SerializationFormat,
};
use crate::messenger::Messenger;
use crate::session::IpcSession;

/// IPC client with a builder-style configuration API.
pub struct IpcClient {
    config: CommunicationConfig,
}

impl IpcClient {
    /// Create a new `IpcClient` for the given application identifier.
    pub fn new(identifier: &str) -> Result<Self, CommunicationError> {
        Ok(Self {
            config: CommunicationConfig {
                identifier: identifier.to_string(),
                ..Default::default()
            },
        })
    }

    /// Return a reference to the current configuration.
    pub fn config(&self) -> &CommunicationConfig {
        &self.config
    }

    /// Configure the communication protocol.
    pub fn with_protocol(mut self, protocol: ProtocolType) -> Self {
        self.config.protocol = protocol;
        self
    }

    /// Configure the serialization format.
    pub fn with_serialization_format(mut self, format: SerializationFormat) -> Self {
        self.config.serialization_format = format;
        self
    }

    /// Configure the timeout (milliseconds) for communication operations.
    pub fn with_timeout(mut self, timeout_ms: u64) -> Self {
        self.config.timeout_ms = timeout_ms;
        self
    }

    /// Send a message to the primary instance and return its response.
    pub async fn send_message(
        &mut self,
        message: CommunicationMessage,
    ) -> Result<CommunicationMessage, CommunicationError> {
        let session = self.connect_persistent().await?;
        let messenger = Messenger::new(session);
        messenger.request(message).await
    }

    /// Connect to the primary instance and keep the connection alive,
    /// returning an [`IpcSession`] for low-level use.
    pub async fn connect_persistent(&self) -> Result<IpcSession, CommunicationError> {
        let protocol = CommunicationFactory::create_protocol(self.config.protocol)?;
        let mut client = protocol.create_client(&self.config).await?;
        client.connect().await?;
        Ok(IpcSession { client })
    }

    /// Connect and return a high-level [`Messenger`].
    pub async fn connect_messenger(&self) -> Result<Messenger, CommunicationError> {
        let session = self.connect_persistent().await?;
        Ok(Messenger::new(session))
    }
}
