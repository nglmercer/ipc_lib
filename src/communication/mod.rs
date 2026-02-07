//! Communication protocol abstraction layer
//! Provides a unified interface for different IPC communication methods

use std::error::Error;
use std::fmt;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Type alias for message handler functions that can optionally return a response
pub type MessageHandler =
    Arc<dyn Fn(CommunicationMessage) -> Option<CommunicationMessage> + Send + Sync>;
/// Type alias for the shared, optional message handler
pub type SharedMessageHandler = Arc<Mutex<Option<MessageHandler>>>;

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

/// Format for message serialization
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SerializationFormat {
    /// JSON format (text-based, human readable)
    Json,
    /// MessagePack format (binary, compact, faster)
    MsgPack,
}

/// Communication configuration
#[derive(Debug, Clone)]
pub struct CommunicationConfig {
    /// Protocol to use
    pub protocol: ProtocolType,
    /// Serialization format to use (default: Json)
    pub serialization_format: SerializationFormat,
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
            protocol: Self::default_protocol(),
            serialization_format: SerializationFormat::Json,
            identifier: "default".to_string(),
            timeout_ms: 5000,
            enable_fallback: true,
            fallback_protocols: Self::default_fallback_protocols(),
        }
    }
}

impl CommunicationConfig {
    /// Get the default protocol for the current platform
    pub fn default_protocol() -> ProtocolType {
        #[cfg(windows)]
        {
            ProtocolType::SharedMemory // SharedMemory now works on Windows!
        }
        #[cfg(unix)]
        {
            ProtocolType::SharedMemory // SharedMemory is now the default on Unix too!
        }
    }

    /// Get the default fallback protocols for the current platform
    pub fn default_fallback_protocols() -> Vec<ProtocolType> {
        #[cfg(unix)]
        {
            vec![
                ProtocolType::FileBased,
                ProtocolType::InMemory,
                ProtocolType::SharedMemory,
            ]
        }

        #[cfg(windows)]
        {
            vec![ProtocolType::FileBased, ProtocolType::InMemory]
        }
    }
}

/// Message structure for communication
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CommunicationMessage {
    /// Unique identifier for this message
    pub id: String,
    /// Message type (e.g., "command", "event", "query")
    pub message_type: String,
    /// Message payload (generic JSON)
    pub payload: serde_json::Value,
    /// Timestamp in milliseconds
    pub timestamp: u64,
    /// Source identifier
    pub source_id: String,
    /// ID of the message this is replying to (for Request-Response)
    pub reply_to: Option<String>,
    /// Protocol-specific or custom metadata
    pub metadata: serde_json::Value,
}

impl CommunicationMessage {
    /// Create a new generic message
    pub fn new(message_type: &str, payload: serde_json::Value) -> Self {
        let id = uuid_v4();
        Self {
            id,
            message_type: message_type.to_string(),
            payload,
            timestamp: current_timestamp(),
            source_id: "unknown".to_string(),
            reply_to: None,
            metadata: serde_json::json!(null),
        }
    }

    /// Create a response to a specific message
    pub fn create_reply(&self, payload: serde_json::Value) -> Self {
        let mut reply = Self::new("response", payload);
        reply.reply_to = Some(self.id.clone());
        reply
    }

    /// Set the source ID
    pub fn with_source(mut self, source_id: &str) -> Self {
        self.source_id = source_id.to_string();
        self
    }

    /// Legacy helper for command line args
    pub fn command_line_args(args: Vec<String>) -> Self {
        Self::new("command_line_args", serde_json::json!(args)).with_source("client")
    }

    /// Legacy helper for simple responses
    pub fn response(content: String) -> Self {
        Self::new("response", serde_json::json!(content)).with_source("server")
    }

    /// Legacy helper for simple errors
    pub fn error(error: String) -> Self {
        Self::new("error", serde_json::json!(error)).with_source("server")
    }
}

/// Simple UUID v4 generator for message IDs (internal fallback if no uuid crate)
fn uuid_v4() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let bytes: [u8; 16] = rng.gen();
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5],
        bytes[6] & 0x0f | 0x40, bytes[7] & 0x3f | 0x80,
        bytes[8], bytes[9],
        bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
    )
}

/// Get current timestamp in milliseconds
pub fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

/// Get a platform-independent temporary file path for the given identifier and extension
pub fn get_temp_path(identifier: &str, extension: &str) -> String {
    let temp_dir = std::env::temp_dir();
    format!("{}/{}.{}", temp_dir.display(), identifier, extension)
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

/// Factory for creating communication protocols
pub struct CommunicationFactory;

impl CommunicationFactory {
    /// Create a protocol implementation
    pub fn create_protocol(
        protocol_type: ProtocolType,
    ) -> Result<Box<dyn CommunicationProtocol>, CommunicationError> {
        match protocol_type {
            ProtocolType::UnixSocket => {
                #[cfg(unix)]
                {
                    Ok(Box::new(UnixSocketProtocol))
                }
                #[cfg(not(unix))]
                {
                    Err(CommunicationError::ProtocolNotSupported(
                        "UnixSocket is only supported on Unix-like systems".to_string(),
                    ))
                }
            }
            ProtocolType::SharedMemory => {
                #[cfg(any(unix, windows))]
                {
                    Ok(Box::new(SharedMemoryProtocol))
                }
                #[cfg(not(any(unix, windows)))]
                {
                    Err(CommunicationError::ProtocolNotSupported(
                        "SharedMemory is only supported on Unix-like systems and Windows"
                            .to_string(),
                    ))
                }
            }
            ProtocolType::FileBased => Ok(Box::new(FileBasedProtocol)),
            ProtocolType::InMemory => Ok(Box::new(InMemoryProtocol)),
            ProtocolType::NamedPipe => {
                #[cfg(windows)]
                {
                    // NamedPipe not yet implemented, but could be in future
                    Err(CommunicationError::ProtocolNotSupported(
                        "NamedPipe protocol is not yet implemented".to_string(),
                    ))
                }
                #[cfg(not(windows))]
                {
                    Err(CommunicationError::ProtocolNotSupported(
                        "NamedPipe is only supported on Windows".to_string(),
                    ))
                }
            }
        }
    }

    /// Get available protocols for the current platform
    #[cfg(unix)]
    pub fn get_available_protocols() -> Vec<ProtocolType> {
        vec![
            ProtocolType::UnixSocket,
            ProtocolType::SharedMemory,
            ProtocolType::FileBased,
            ProtocolType::InMemory,
        ]
    }

    /// Get available protocols for the current platform (Windows)
    #[cfg(not(unix))]
    pub fn get_available_protocols() -> Vec<ProtocolType> {
        vec![
            ProtocolType::SharedMemory,
            ProtocolType::FileBased,
            ProtocolType::InMemory,
        ]
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
