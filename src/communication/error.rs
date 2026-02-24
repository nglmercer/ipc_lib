use std::error::Error;
use std::fmt;

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
