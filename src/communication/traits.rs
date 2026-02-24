use crate::communication::error::CommunicationError;
use crate::communication::message::CommunicationMessage;
use crate::communication::types::{CommunicationConfig, ProtocolType};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Type alias for message handler functions that can optionally return a response
pub type MessageHandler =
    Arc<dyn Fn(CommunicationMessage) -> Option<CommunicationMessage> + Send + Sync>;
/// Type alias for the shared, optional message handler
pub type SharedMessageHandler = Arc<Mutex<Option<MessageHandler>>>;

/// Trait for communication protocols
#[async_trait::async_trait]
pub trait CommunicationProtocol: Send + Sync {
    /// Get the protocol type
    fn protocol_type(&self) -> ProtocolType;

    /// Create a new server instance
    async fn create_server(
        &self,
        config: &CommunicationConfig,
    ) -> Result<Box<dyn CommunicationServer>, CommunicationError>;

    /// Create a new client instance
    async fn create_client(
        &self,
        config: &CommunicationConfig,
    ) -> Result<Box<dyn CommunicationClient>, CommunicationError>;
}

/// Server interface for communication protocols
#[async_trait::async_trait]
pub trait CommunicationServer: Send + Sync {
    /// Start the server and listen for connections
    async fn start(&mut self) -> Result<(), CommunicationError>;

    /// Stop the server
    async fn stop(&mut self) -> Result<(), CommunicationError>;

    /// Check if the server is running
    fn is_running(&self) -> bool;

    /// Get the server address/endpoint
    fn endpoint(&self) -> String;

    /// Broadcast a message to all connected clients
    async fn broadcast(&self, _message: CommunicationMessage) -> Result<(), CommunicationError> {
        Ok(())
    }

    /// Set a handler for incoming messages
    fn set_message_handler(&self, _handler: MessageHandler) -> Result<(), CommunicationError> {
        Ok(())
    }
}

/// Client interface for communication protocols
#[async_trait::async_trait]
pub trait CommunicationClient: Send + Sync {
    /// Connect to the server
    async fn connect(&mut self) -> Result<(), CommunicationError>;

    /// Send a message to the server
    async fn send_message(&self, message: &CommunicationMessage) -> Result<(), CommunicationError>;

    /// Receive a message from the server
    async fn receive_message(&self) -> Result<CommunicationMessage, CommunicationError>;

    /// Disconnect from the server
    async fn disconnect(&mut self) -> Result<(), CommunicationError>;

    /// Check if connected
    fn is_connected(&self) -> bool;
}
