//! Communication protocol abstraction layer
//! Provides a unified interface for different IPC communication methods

use std::error::Error;
use std::fmt;

/// Protocol types available for communication
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolType {
    /// Unix Domain Sockets (Unix-like systems)
    UnixSocket,
    /// Named Pipes (Windows)
    NamedPipe,
    /// Shared Memory (memory-mapped files)
    SharedMemory,
    /// File-based communication
    FileBased,
    /// In-memory communication (for testing)
    InMemory,
}

/// Communication error types
#[derive(Debug)]
pub enum CommunicationError {
    /// Connection failed
    ConnectionFailed(String),
    /// Message serialization failed
    SerializationFailed(String),
    /// Message deserialization failed
    DeserializationFailed(String),
    /// Protocol not supported on this platform
    ProtocolNotSupported(String),
    /// Communication timeout
    Timeout(String),
    /// Resource not found
    ResourceNotFound(String),
    /// Permission denied
    PermissionDenied(String),
    /// Other I/O errors
    IoError(String),
}

impl fmt::Display for CommunicationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CommunicationError::ConnectionFailed(msg) => write!(f, "Connection failed: {}", msg),
            CommunicationError::SerializationFailed(msg) => {
                write!(f, "Serialization failed: {}", msg)
            }
            CommunicationError::DeserializationFailed(msg) => {
                write!(f, "Deserialization failed: {}", msg)
            }
            CommunicationError::ProtocolNotSupported(msg) => {
                write!(f, "Protocol not supported: {}", msg)
            }
            CommunicationError::Timeout(msg) => write!(f, "Timeout: {}", msg),
            CommunicationError::ResourceNotFound(msg) => write!(f, "Resource not found: {}", msg),
            CommunicationError::PermissionDenied(msg) => write!(f, "Permission denied: {}", msg),
            CommunicationError::IoError(msg) => write!(f, "I/O error: {}", msg),
        }
    }
}

impl Error for CommunicationError {}

impl From<std::io::Error> for CommunicationError {
    fn from(error: std::io::Error) -> Self {
        CommunicationError::IoError(error.to_string())
    }
}

impl From<serde_json::Error> for CommunicationError {
    fn from(error: serde_json::Error) -> Self {
        CommunicationError::SerializationFailed(error.to_string())
    }
}

/// Communication configuration
#[derive(Debug, Clone)]
pub struct CommunicationConfig {
    /// Protocol to use
    pub protocol: ProtocolType,
    /// Identifier for this communication channel
    pub identifier: String,
    /// Timeout for operations (in milliseconds)
    pub timeout_ms: u64,
    /// Whether to enable fallback protocols
    pub enable_fallback: bool,
    /// List of fallback protocols (in order of preference)
    pub fallback_protocols: Vec<ProtocolType>,
}

impl Default for CommunicationConfig {
    fn default() -> Self {
        Self {
            protocol: ProtocolType::UnixSocket,
            identifier: "default".to_string(),
            timeout_ms: 5000,
            enable_fallback: true,
            fallback_protocols: vec![
                ProtocolType::UnixSocket,
                ProtocolType::FileBased,
                ProtocolType::SharedMemory,
            ],
        }
    }
}

/// Message structure for communication
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CommunicationMessage {
    /// Message type
    pub message_type: String,
    /// Message payload
    pub payload: serde_json::Value,
    /// Timestamp
    pub timestamp: u64,
    /// Source identifier
    pub source_id: String,
    /// Protocol-specific metadata
    pub metadata: serde_json::Value,
}

impl CommunicationMessage {
    /// Create a new command line args message
    pub fn command_line_args(args: Vec<String>) -> Self {
        Self {
            message_type: "command_line_args".to_string(),
            payload: serde_json::json!(args),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            source_id: "client".to_string(),
            metadata: serde_json::json!(null),
        }
    }

    /// Create a response message
    pub fn response(content: String) -> Self {
        Self {
            message_type: "response".to_string(),
            payload: serde_json::json!(content),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            source_id: "server".to_string(),
            metadata: serde_json::json!(null),
        }
    }

    /// Create an error message
    pub fn error(error: String) -> Self {
        Self {
            message_type: "error".to_string(),
            payload: serde_json::json!(error),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            source_id: "server".to_string(),
            metadata: serde_json::json!(null),
        }
    }
}

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
}

/// Client interface for communication protocols
#[async_trait::async_trait]
pub trait CommunicationClient: Send + Sync {
    /// Connect to the server
    async fn connect(&mut self) -> Result<(), CommunicationError>;

    /// Send a message to the server
    async fn send_message(
        &mut self,
        message: &CommunicationMessage,
    ) -> Result<(), CommunicationError>;

    /// Receive a message from the server
    async fn receive_message(&mut self) -> Result<CommunicationMessage, CommunicationError>;

    /// Disconnect from the server
    async fn disconnect(&mut self) -> Result<(), CommunicationError>;

    /// Check if connected
    fn is_connected(&self) -> bool;
}

/// Factory for creating communication protocols
pub struct CommunicationFactory;

impl CommunicationFactory {
    /// Create a protocol implementation
    pub fn create_protocol(
        protocol_type: ProtocolType,
    ) -> Result<Box<dyn CommunicationProtocol>, CommunicationError> {
        match protocol_type {
            ProtocolType::UnixSocket => Ok(Box::new(UnixSocketProtocol)),
            ProtocolType::SharedMemory => Ok(Box::new(SharedMemoryProtocol)),
            ProtocolType::FileBased => Ok(Box::new(FileBasedProtocol)),
            ProtocolType::InMemory => Ok(Box::new(InMemoryProtocol)),
            _ => Err(CommunicationError::ProtocolNotSupported(
                "Protocol not implemented".to_string(),
            )),
        }
    }

    /// Get available protocols for the current platform
    pub fn get_available_protocols() -> Vec<ProtocolType> {
        let mut protocols = Vec::new();

        // Unix sockets are available on Unix-like systems
        if cfg!(unix) {
            protocols.push(ProtocolType::UnixSocket);
        }

        // Named pipes are available on Windows
        if cfg!(windows) {
            protocols.push(ProtocolType::NamedPipe);
        }

        // Shared memory is available on most platforms
        protocols.push(ProtocolType::SharedMemory);

        // File-based communication is always available
        protocols.push(ProtocolType::FileBased);

        // In-memory is always available (for testing)
        protocols.push(ProtocolType::InMemory);

        protocols
    }
}

// Protocol implementations will be defined in separate modules
mod file_based;
mod in_memory;
mod shared_memory;
mod unix_socket;

pub use file_based::FileBasedProtocol;
pub use in_memory::InMemoryProtocol;
pub use shared_memory::SharedMemoryProtocol;
pub use unix_socket::UnixSocketProtocol;
