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
pub fn uuid_v4() -> String {
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
