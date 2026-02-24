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
