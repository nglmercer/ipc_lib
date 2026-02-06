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
#[derive(Debug, serde::Serialize, serde::Deserialize)]
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
