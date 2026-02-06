//! Single Instance Application Library
//! 
//! This library provides a robust single instance enforcement mechanism
//! with multiple IPC communication protocols.
//! 
//! Features:
//! - Single instance enforcement across multiple processes
//! - Multiple communication protocols (Unix sockets, file-based, shared memory, etc.)
//! - Fallback mechanisms for reliability
//! - Cross-platform support
//! - Async/await support for modern Rust applications

pub mod communication;

use communication::{
    CommunicationError, CommunicationFactory,
    CommunicationMessage,
};

// Re-export commonly used types for simpler imports
pub use communication::ProtocolType;
pub use communication::CommunicationConfig;

/// Message types for IPC communication (legacy compatibility)
#[derive(Debug, PartialEq, Clone, serde::Serialize, serde::Deserialize)]
pub enum Message {
    CommandLineArgs(Vec<String>),
    Response(String),
    Error(String),
}

/// Enhanced single instance enforcement with multiple communication protocols
pub struct SingleInstanceApp {
    identifier: String,
    config: CommunicationConfig,
    server: Option<Box<dyn communication::CommunicationServer>>,
    is_primary: bool,
}

impl SingleInstanceApp {
    /// Create a new single instance application with the given identifier
    pub fn new(identifier: &str) -> Self {
        Self {
            identifier: identifier.to_string(),
            config: CommunicationConfig {
                identifier: identifier.to_string(),
                ..Default::default()
            },
            server: None,
            is_primary: false,
        }
    }

    /// Configure the communication protocol
    pub fn with_protocol(mut self, protocol: ProtocolType) -> Self {
        self.config.protocol = protocol;
        self
    }

    /// Configure timeout for communication operations
    pub fn with_timeout(mut self, timeout_ms: u64) -> Self {
        self.config.timeout_ms = timeout_ms;
        self
    }

    /// Disable fallback protocols
    pub fn without_fallback(mut self) -> Self {
        self.config.enable_fallback = false;
        self
    }

    /// Set custom fallback protocols
    pub fn with_fallback_protocols(mut self, protocols: Vec<ProtocolType>) -> Self {
        self.config.fallback_protocols = protocols;
        self
    }

    /// Get the communication endpoint
    pub fn endpoint(&self) -> Option<String> {
        self.server.as_ref().map(|s| s.endpoint())
    }

    /// Get the current configuration
    pub fn config(&self) -> &CommunicationConfig {
        &self.config
    }

    /// Enforce single instance and start the application
    pub async fn enforce_single_instance(&mut self) -> Result<bool, CommunicationError> {
        // Try to create lock file first
        if let Err(_) = self.write_pid_to_lock() {
            // Lock file exists, check if process is running
            match self.read_pid_from_lock() {
                Ok(pid) => {
                    if self.is_process_running(pid) {
                        // Primary instance is running, connect to it
                        let response = self.connect_to_primary().await?;
                        println!("Secondary instance: {}", response);
                        return Ok(false);
                    } else {
                        // Stale lock, clean up and try again
                        self.cleanup_stale_lock()?;
                        self.write_pid_to_lock()?;
                    }
                }
                Err(_) => {
                    // Couldn't read lock file, try to create one
                    self.write_pid_to_lock()?;
                }
            }
        } else {
            // Successfully created lock file
            self.write_pid_to_lock()?;
        }

        // Try to start the server with the primary protocol
        if let Err(e) = self.start_server().await {
            if self.config.enable_fallback {
                // Try fallback protocols
                let fallback_protocols = self.config.fallback_protocols.clone();
                for fallback_protocol in fallback_protocols {
                    if fallback_protocol == self.config.protocol {
                        continue; // Skip the primary protocol
                    }

                    self.config.protocol = fallback_protocol;
                    if let Ok(()) = self.start_server().await {
                        break; // Success with fallback protocol
                    }
                }
            }

            // If we still can't start a server, try to connect to existing instance
            if self.server.is_none() {
                match self.connect_to_primary().await {
                    Ok(response) => {
                        println!("Secondary instance: {}", response);
                        return Ok(false);
                    }
                    Err(_) => {
                        return Err(e);
                    }
                }
            }
        }

        self.is_primary = true;
        Ok(true)
    }

    /// Start the communication server
    async fn start_server(&mut self) -> Result<(), CommunicationError> {
        let protocol = CommunicationFactory::create_protocol(self.config.protocol)?;
        self.server = Some(protocol.create_server(&self.config).await?);
        self.server.as_mut().unwrap().start().await?;
        Ok(())
    }

    /// Connect to the primary instance
    async fn connect_to_primary(&self) -> Result<String, CommunicationError> {
        let protocol = CommunicationFactory::create_protocol(self.config.protocol)?;
        let mut client = protocol.create_client(&self.config).await?;
        client.connect().await?;

        let args = std::env::args().collect();
        let message = CommunicationMessage::command_line_args(args);
        client.send_message(&message).await?;
        let response = client.receive_message().await?;
        client.disconnect().await?;

        match response.message_type.as_str() {
            "response" => Ok(response.payload.as_str().unwrap_or("").to_string()),
            "error" => Err(CommunicationError::ConnectionFailed(
                response.payload.as_str().unwrap_or("Unknown error").to_string()
            )),
            _ => Err(CommunicationError::ConnectionFailed("Unexpected response type".to_string())),
        }
    }

    /// Check if a process with the given PID is running
    pub fn is_process_running(&self, pid: u32) -> bool {
        #[cfg(unix)]
        {
            // Send signal 0 to check if process exists
            unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
        }
        
        #[cfg(windows)]
        {
            use std::ptr;
            use winapi::um::processthreadsapi::OpenProcess;
            use winapi::um::winnt::PROCESS_QUERY_INFORMATION;
            
            unsafe {
                let handle = OpenProcess(PROCESS_QUERY_INFORMATION, 0, pid);
                if handle.is_null() {
                    false
                } else {
                    winapi::um::handleapi::CloseHandle(handle);
                    true
                }
            }
        }
    }

    /// Write PID to lock file
    fn write_pid_to_lock(&self) -> Result<(), CommunicationError> {
        let lock_file = format!("/tmp/{}.pid", self.identifier);
        std::fs::write(&lock_file, std::process::id().to_string())
            .map_err(|e| CommunicationError::ConnectionFailed(e.to_string()))
    }

    /// Read PID from lock file
    fn read_pid_from_lock(&self) -> Result<u32, CommunicationError> {
        let lock_file = format!("/tmp/{}.pid", self.identifier);
        let pid_str = std::fs::read_to_string(&lock_file)
            .map_err(|e| CommunicationError::ConnectionFailed(e.to_string()))?;
        pid_str.parse::<u32>()
            .map_err(|e| CommunicationError::ConnectionFailed(e.to_string()))
    }

    /// Clean up stale lock file
    fn cleanup_stale_lock(&self) -> Result<(), CommunicationError> {
        let lock_file = format!("/tmp/{}.pid", self.identifier);
        std::fs::remove_file(&lock_file)
            .map_err(|e| CommunicationError::ConnectionFailed(e.to_string()))
    }
}

/// IPC Client wrapper for simple usage
pub struct IpcClient {
    config: CommunicationConfig,
}

impl IpcClient {
    /// Create a new IPC client
    pub fn new(identifier: &str) -> Result<Self, CommunicationError> {
        Ok(Self {
            config: CommunicationConfig {
                identifier: identifier.to_string(),
                ..Default::default()
            },
        })
    }

    /// Get the configuration
    pub fn config(&self) -> &CommunicationConfig {
        &self.config
    }

    /// Send arguments to the primary instance
    pub async fn send_args(&mut self, args: Vec<String>) -> Result<String, CommunicationError> {
        let protocol = CommunicationFactory::create_protocol(self.config.protocol)?;
        let mut client = protocol.create_client(&self.config).await?;
        client.connect().await?;

        let message = CommunicationMessage::command_line_args(args);
        client.send_message(&message).await?;
        let response = client.receive_message().await?;
        client.disconnect().await?;

        match response.message_type.as_str() {
            "response" => Ok(response.payload.as_str().unwrap_or("").to_string()),
            "error" => Err(CommunicationError::ConnectionFailed(
                response.payload.as_str().unwrap_or("Unknown error").to_string()
            )),
            _ => Err(CommunicationError::ConnectionFailed("Unexpected response type".to_string())),
        }
    }
}

/// IPC Server wrapper for simple usage
pub struct IpcServer {
    app: SingleInstanceApp,
}

impl IpcServer {
    /// Create a new IPC server
    pub fn new(identifier: &str) -> Result<Self, CommunicationError> {
        let app = SingleInstanceApp::new(identifier);
        Ok(Self { app })
    }

    /// Start the server
    pub async fn start(&mut self) -> Result<(), CommunicationError> {
        self.app.start_server().await
    }

    /// Get the server endpoint
    pub fn endpoint(&self) -> Option<String> {
        self.app.endpoint()
    }
}

/// Convenience function for simple single instance enforcement
/// 
/// Returns Ok(true) if this is the primary instance,
/// Ok(false) if this is a secondary instance (another instance is already running),
/// Err(error) if enforcement failed.
pub async fn enforce_single_instance(identifier: &str) -> Result<bool, CommunicationError> {
    let mut app = SingleInstanceApp::new(identifier);
    app.enforce_single_instance().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use communication::ProtocolType;

    // ============ SingleInstanceApp Tests ============

    #[test]
    fn test_single_instance_app_new() {
        let app = SingleInstanceApp::new("test_app");
        
        // Verify the identifier is set correctly
        // We can't directly access private fields, but we can verify through public methods
        assert_eq!(app.config().identifier, "test_app");
    }

    #[test]
    fn test_single_instance_app_with_protocol() {
        let app = SingleInstanceApp::new("test_app")
            .with_protocol(ProtocolType::FileBased)
            .with_timeout(3000);
        
        assert_eq!(app.config().protocol, ProtocolType::FileBased);
        assert_eq!(app.config().timeout_ms, 3000);
    }

    #[test]
    fn test_single_instance_app_without_fallback() {
        let app = SingleInstanceApp::new("test_app")
            .without_fallback();
        
        assert!(!app.config().enable_fallback);
    }

    #[test]
    fn test_single_instance_app_with_fallback_protocols() {
        let protocols = vec![ProtocolType::FileBased, ProtocolType::InMemory];
        let app = SingleInstanceApp::new("test_app")
            .with_fallback_protocols(protocols.clone());
        
        assert_eq!(app.config().fallback_protocols, protocols);
    }

    #[test]
    fn test_single_instance_app_endpoint_none_when_not_started() {
        let app = SingleInstanceApp::new("test_app");
        assert!(app.endpoint().is_none());
    }

    // ============ IpcClient Tests ============

    #[test]
    fn test_ipc_client_new() {
        let client = IpcClient::new("test_client");
        assert!(client.is_ok());
        let client = client.unwrap();
        assert_eq!(client.config().identifier, "test_client");
    }

    #[test]
    fn test_ipc_client_default_protocol() {
        let client = IpcClient::new("test_client").unwrap();
        assert_eq!(client.config().protocol, ProtocolType::UnixSocket);
    }

    // ============ IpcServer Tests ============

    #[test]
    fn test_ipc_server_new() {
        let server = IpcServer::new("test_server");
        assert!(server.is_ok());
        let server = server.unwrap();
        assert!(server.endpoint().is_none());
    }

    // ============ Message Tests ============

    #[test]
    fn test_message_command_line_args() {
        let args = vec!["arg1".to_string(), "arg2".to_string()];
        let message = Message::CommandLineArgs(args.clone());
        
        match message {
            Message::CommandLineArgs(received_args) => {
                assert_eq!(received_args, args);
            }
            _ => panic!("Expected CommandLineArgs variant"),
        }
    }

    #[test]
    fn test_message_response() {
        let message = Message::Response("test response".to_string());
        
        match message {
            Message::Response(content) => {
                assert_eq!(content, "test response");
            }
            _ => panic!("Expected Response variant"),
        }
    }

    #[test]
    fn test_message_error() {
        let message = Message::Error("test error".to_string());
        
        match message {
            Message::Error(error_msg) => {
                assert_eq!(error_msg, "test error");
            }
            _ => panic!("Expected Error variant"),
        }
    }

    #[test]
    fn test_message_serialization() {
        let message = Message::CommandLineArgs(vec!["test".to_string()]);
        let serialized = serde_json::to_string(&message);
        assert!(serialized.is_ok());
        
        let deserialized: Result<Message, _> = serde_json::from_str(&serialized.unwrap());
        assert!(deserialized.is_ok());
        assert_eq!(deserialized.unwrap(), message);
    }

    #[test]
    fn test_message_debug_format() {
        let message = Message::Response("test".to_string());
        let debug_format = format!("{:?}", message);
        assert!(debug_format.contains("Response"));
        assert!(debug_format.contains("test"));
    }

    // ============ ProtocolType Tests ============

    #[test]
    fn test_protocol_type_variants() {
        let _ = ProtocolType::UnixSocket;
        let _ = ProtocolType::NamedPipe;
        let _ = ProtocolType::SharedMemory;
        let _ = ProtocolType::FileBased;
        let _ = ProtocolType::InMemory;
    }

    #[test]
    fn test_protocol_type_debug() {
        assert_eq!(format!("{:?}", ProtocolType::UnixSocket), "UnixSocket");
        assert_eq!(format!("{:?}", ProtocolType::FileBased), "FileBased");
        assert_eq!(format!("{:?}", ProtocolType::InMemory), "InMemory");
    }

    #[test]
    fn test_protocol_type_clone() {
        let protocol = ProtocolType::UnixSocket;
        let cloned = protocol.clone();
        assert_eq!(protocol, cloned);
    }

    // ============ CommunicationConfig Tests ============

    #[test]
    fn test_communication_config_default() {
        let config = CommunicationConfig::default();
        
        assert_eq!(config.protocol, ProtocolType::UnixSocket);
        assert_eq!(config.identifier, "default");
        assert_eq!(config.timeout_ms, 5000);
        assert!(config.enable_fallback);
        assert!(!config.fallback_protocols.is_empty());
    }

    #[test]
    fn test_communication_config_custom() {
        let config = CommunicationConfig {
            protocol: ProtocolType::FileBased,
            identifier: "custom".to_string(),
            timeout_ms: 10000,
            enable_fallback: false,
            fallback_protocols: vec![],
        };
        
        assert_eq!(config.protocol, ProtocolType::FileBased);
        assert_eq!(config.identifier, "custom");
        assert_eq!(config.timeout_ms, 10000);
        assert!(!config.enable_fallback);
        assert!(config.fallback_protocols.is_empty());
    }

    #[test]
    fn test_communication_config_debug() {
        let config = CommunicationConfig::default();
        let debug_format = format!("{:?}", config);
        assert!(debug_format.contains("UnixSocket"));
        assert!(debug_format.contains("default"));
    }

    #[test]
    fn test_communication_config_clone() {
        let config = CommunicationConfig::default();
        let cloned = config.clone();
        
        assert_eq!(config.protocol, cloned.protocol);
        assert_eq!(config.identifier, cloned.identifier);
        assert_eq!(config.timeout_ms, cloned.timeout_ms);
    }

    // ============ CommunicationMessage Tests ============

    #[test]
    fn test_communication_message_command_line_args() {
        let args = vec!["--flag".to_string(), "value".to_string()];
        let message = CommunicationMessage::command_line_args(args.clone());
        
        assert_eq!(message.message_type, "command_line_args");
        assert_eq!(message.source_id, "client");
        assert!(message.timestamp > 0);
        
        let payload_args: Vec<String> = serde_json::from_value(message.payload).unwrap();
        assert_eq!(payload_args, args);
    }

    #[test]
    fn test_communication_message_response() {
        let message = CommunicationMessage::response("Success!".to_string());
        
        assert_eq!(message.message_type, "response");
        assert_eq!(message.source_id, "server");
        assert!(message.timestamp > 0);
        
        let payload_content: String = serde_json::from_value(message.payload).unwrap();
        assert_eq!(payload_content, "Success!");
    }

    #[test]
    fn test_communication_message_error() {
        let message = CommunicationMessage::error("Something went wrong".to_string());
        
        assert_eq!(message.message_type, "error");
        assert_eq!(message.source_id, "server");
        
        let payload_error: String = serde_json::from_value(message.payload).unwrap();
        assert_eq!(payload_error, "Something went wrong");
    }

    #[test]
    fn test_communication_message_serialization() {
        let message = CommunicationMessage::response("test".to_string());
        let serialized = serde_json::to_string(&message);
        assert!(serialized.is_ok());
        
        let deserialized: Result<CommunicationMessage, _> = serde_json::from_str(&serialized.unwrap());
        assert!(deserialized.is_ok());
        
        let deserialized = deserialized.unwrap();
        assert_eq!(deserialized.message_type, "response");
        let content: String = serde_json::from_value(deserialized.payload).unwrap();
        assert_eq!(content, "test");
    }

    #[test]
    fn test_communication_message_metadata() {
        let message = CommunicationMessage::command_line_args(vec![]);
        assert_eq!(message.metadata, serde_json::json!(null));
    }

    #[test]
    fn test_communication_message_timestamp_ordering() {
        let message1 = CommunicationMessage::command_line_args(vec![]);
        std::thread::sleep(std::time::Duration::from_millis(1));
        let message2 = CommunicationMessage::command_line_args(vec![]);
        
        assert!(message2.timestamp >= message1.timestamp);
    }

    // ============ CommunicationError Tests ============

    #[test]
    fn test_communication_error_display() {
        let error = CommunicationError::ConnectionFailed("test error".to_string());
        let display = format!("{}", error);
        assert!(display.contains("Connection failed"));
        assert!(display.contains("test error"));
    }

    #[test]
    fn test_communication_error_variants() {
        let _ = CommunicationError::ConnectionFailed("test".to_string());
        let _ = CommunicationError::SerializationFailed("test".to_string());
        let _ = CommunicationError::DeserializationFailed("test".to_string());
        let _ = CommunicationError::ProtocolNotSupported("test".to_string());
        let _ = CommunicationError::Timeout("test".to_string());
        let _ = CommunicationError::ResourceNotFound("test".to_string());
        let _ = CommunicationError::PermissionDenied("test".to_string());
        let _ = CommunicationError::IoError("test".to_string());
    }

    #[test]
    fn test_communication_error_source() {
        use std::error::Error;
        let error = CommunicationError::ConnectionFailed("test".to_string());
        let source = error.source();
        assert!(source.is_none()); // CommunicationError doesn't implement Error::source
    }

    // ============ CommunicationFactory Tests ============

    #[test]
    fn test_communication_factory_create_protocols() {
        let protocols = vec![
            ProtocolType::UnixSocket,
            ProtocolType::SharedMemory,
            ProtocolType::FileBased,
            ProtocolType::InMemory,
        ];
        
        for protocol in protocols {
            let result = communication::CommunicationFactory::create_protocol(protocol);
            assert!(result.is_ok());
        }
    }

    #[test]
    fn test_communication_factory_get_available_protocols() {
        let protocols = communication::CommunicationFactory::get_available_protocols();
        
        // At minimum, should have FileBased and InMemory (always available)
        assert!(protocols.contains(&ProtocolType::FileBased));
        assert!(protocols.contains(&ProtocolType::InMemory));
        
        #[cfg(unix)]
        assert!(protocols.contains(&ProtocolType::UnixSocket));
        
        #[cfg(windows)]
        assert!(protocols.contains(&ProtocolType::NamedPipe));
    }

    // ============ Edge Cases and Integration Tests ============

    #[test]
    fn test_identifier_length_variations() {
        // Test short identifier
        let app = SingleInstanceApp::new("a");
        assert_eq!(app.config().identifier, "a");
        
        // Test long identifier
        let long_id = "a".repeat(100);
        let app = SingleInstanceApp::new(&long_id);
        assert_eq!(app.config().identifier, long_id);
        
        // Test identifier with special characters
        let special_id = "app-with-dots_and_underscores.123";
        let app = SingleInstanceApp::new(special_id);
        assert_eq!(app.config().identifier, special_id);
    }

    #[test]
    fn test_builder_pattern_chaining() {
        let app = SingleInstanceApp::new("test")
            .with_protocol(ProtocolType::FileBased)
            .with_timeout(1000)
            .without_fallback();
        
        assert_eq!(app.config().protocol, ProtocolType::FileBased);
        assert_eq!(app.config().timeout_ms, 1000);
        assert!(!app.config().enable_fallback);
    }

    #[test]
    fn test_empty_args_message() {
        let message = CommunicationMessage::command_line_args(vec![]);
        let payload_args: Vec<String> = serde_json::from_value(message.payload).unwrap();
        assert!(payload_args.is_empty());
    }

    #[test]
    fn test_multiline_response_message() {
        let multiline = "Line 1\nLine 2\nLine 3";
        let message = CommunicationMessage::response(multiline.to_string());
        let content: String = serde_json::from_value(message.payload).unwrap();
        assert_eq!(content, multiline);
    }

    #[test]
    fn test_special_characters_in_error() {
        let special_error = "Error with 'quotes' and \"double quotes\" and unicode: café";
        let message = CommunicationMessage::error(special_error.to_string());
        let content: String = serde_json::from_value(message.payload).unwrap();
        assert_eq!(content, special_error);
    }
}
