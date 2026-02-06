//! File-based communication protocol implementation
//! Provides IPC communication using files and file locking
//! Works on all platforms and doesn't require special permissions

use super::*;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::Duration;

/// File-based communication protocol implementation
#[derive(Debug)]
pub struct FileBasedProtocol;

#[async_trait::async_trait]
impl CommunicationProtocol for FileBasedProtocol {
    fn protocol_type(&self) -> ProtocolType {
        ProtocolType::FileBased
    }

    async fn create_server(&self, config: &CommunicationConfig) -> Result<Box<dyn CommunicationServer>, CommunicationError> {
        Ok(Box::new(FileBasedServer::new(config)?))
    }

    async fn create_client(&self, config: &CommunicationConfig) -> Result<Box<dyn CommunicationClient>, CommunicationError> {
        Ok(Box::new(FileBasedClient::new(config)?))
    }
}

/// File-based server implementation
#[derive(Debug)]
pub struct FileBasedServer {
    config: CommunicationConfig,
    message_file: String,
    lock_file: String,
    is_running: Arc<Mutex<bool>>,
}

impl FileBasedServer {
    pub fn new(config: &CommunicationConfig) -> Result<Self, CommunicationError> {
        let message_file = Self::get_message_file(&config.identifier);
        let lock_file = Self::get_lock_file(&config.identifier);

        // Clean up any existing files
        if Path::new(&message_file).exists() {
            std::fs::remove_file(&message_file)?;
        }
        if Path::new(&lock_file).exists() {
            std::fs::remove_file(&lock_file)?;
        }

        Ok(Self {
            config: config.clone(),
            message_file,
            lock_file,
            is_running: Arc::new(Mutex::new(false)),
        })
    }

    fn get_message_file(identifier: &str) -> String {
        format!("/tmp/{}.msg", identifier)
    }

    fn get_lock_file(identifier: &str) -> String {
        format!("/tmp/{}.lock", identifier)
    }

    async fn wait_for_message(&self) -> Result<CommunicationMessage, CommunicationError> {
        let timeout_duration = Duration::from_millis(self.config.timeout_ms);
        
        loop {
            if !*self.is_running.blocking_lock() {
                return Err(CommunicationError::ConnectionFailed("Server stopped".to_string()));
            }

            // Check if message file exists
            if Path::new(&self.message_file).exists() {
                // Read the message
                let mut file = File::open(&self.message_file)?;
                let mut content = String::new();
                file.read_to_string(&mut content)?;

                // Remove the message file
                std::fs::remove_file(&self.message_file)?;

                // Parse and return the message
                let message: CommunicationMessage = serde_json::from_str(&content)
                    .map_err(|e| CommunicationError::DeserializationFailed(e.to_string()))?;
                return Ok(message);
            }

            // Wait a bit before checking again
            tokio::time::sleep(Duration::from_millis(100)).await;
            
            // Check timeout
            if timeout_duration < Duration::from_millis(100) {
                return Err(CommunicationError::Timeout("No message received".to_string()));
            }
        }
    }

    async fn send_response(&self, response: &CommunicationMessage) -> Result<(), CommunicationError> {
        let response_file = format!("{}.response", self.message_file);
        
        // Create response file
        let mut file = File::create(&response_file)?;
        let response_json = serde_json::to_string(response)?;
        file.write_all(response_json.as_bytes())?;
        
        Ok(())
    }
}

#[async_trait::async_trait]
impl CommunicationServer for FileBasedServer {
    async fn start(&mut self) -> Result<(), CommunicationError> {
        // Create lock file to indicate server is running
        let mut lock_file = File::create(&self.lock_file)?;
        lock_file.write_all(b"running")?;
        lock_file.flush()?;
        
        *self.is_running.lock().await = true;

        // Spawn a task to handle incoming messages
        let is_running = self.is_running.clone();
        let message_file = self.message_file.clone();
        let lock_file = self.lock_file.clone();

        tokio::spawn(async move {
            while *is_running.lock().await {
                // Wait for message
                match Self::wait_for_message(&Self {
                    config: CommunicationConfig::default(),
                    message_file: message_file.clone(),
                    lock_file: lock_file.clone(),
                    is_running: is_running.clone(),
                }).await {
                    Ok(message) => {
                        match message.message_type.as_str() {
                            "command_line_args" => {
                                let response = CommunicationMessage::response(
                                    "Command line arguments received successfully".to_string()
                                );
                                if let Err(e) = Self::send_response(&Self {
                                    config: CommunicationConfig::default(),
                                    message_file: message_file.clone(),
                                    lock_file: lock_file.clone(),
                                    is_running: is_running.clone(),
                                }, &response).await {
                                    eprintln!("Failed to send response: {}", e);
                                }
                            }
                            _ => {
                                let error = CommunicationMessage::error(
                                    "Unsupported message type".to_string()
                                );
                                if let Err(e) = Self::send_response(&Self {
                                    config: CommunicationConfig::default(),
                                    message_file: message_file.clone(),
                                    lock_file: lock_file.clone(),
                                    is_running: is_running.clone(),
                                }, &error).await {
                                    eprintln!("Failed to send error response: {}", e);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Error handling message: {}", e);
                    }
                }
            }

            // Clean up files when server stops
            let _ = std::fs::remove_file(&message_file);
            let _ = std::fs::remove_file(&lock_file);
            let _ = std::fs::remove_file(&format!("{}.response", message_file));
        });

        Ok(())
    }

    async fn stop(&mut self) -> Result<(), CommunicationError> {
        *self.is_running.lock().await = false;
        Ok(())
    }

    fn is_running(&self) -> bool {
        *self.is_running.blocking_lock()
    }

    fn endpoint(&self) -> String {
        format!("file://{}", self.lock_file)
    }
}

/// File-based client implementation
#[derive(Debug)]
pub struct FileBasedClient {
    config: CommunicationConfig,
    message_file: String,
    response_file: String,
    connected: bool,
}

impl FileBasedClient {
    pub fn new(config: &CommunicationConfig) -> Result<Self, CommunicationError> {
        let message_file = Self::get_message_file(&config.identifier);
        let response_file = format!("{}.response", message_file);

        Ok(Self {
            config: config.clone(),
            message_file,
            response_file,
            connected: false,
        })
    }

    fn get_message_file(identifier: &str) -> String {
        format!("/tmp/{}.msg", identifier)
    }
}

#[async_trait::async_trait]
impl CommunicationClient for FileBasedClient {
    async fn connect(&mut self) -> Result<(), CommunicationError> {
        // Check if lock file exists
        let lock_file = format!("/tmp/{}.lock", self.config.identifier);
        if !Path::new(&lock_file).exists() {
            return Err(CommunicationError::ConnectionFailed("Server not running".to_string()));
        }

        self.connected = true;
        Ok(())
    }

    async fn send_message(&mut self, message: &CommunicationMessage) -> Result<(), CommunicationError> {
        if !self.connected {
            return Err(CommunicationError::ConnectionFailed("Not connected".to_string()));
        }

        // Create message file
        let mut file = File::create(&self.message_file)?;
        let message_json = serde_json::to_string(message)?;
        file.write_all(message_json.as_bytes())?;

        Ok(())
    }

    async fn receive_message(&mut self) -> Result<CommunicationMessage, CommunicationError> {
        if !self.connected {
            return Err(CommunicationError::ConnectionFailed("Not connected".to_string()));
        }

        let timeout_duration = Duration::from_millis(self.config.timeout_ms);
        let start_time = std::time::Instant::now();

        loop {
            // Check if response file exists
            if Path::new(&self.response_file).exists() {
                // Read the response
                let mut file = File::open(&self.response_file)?;
                let mut content = String::new();
                file.read_to_string(&mut content)?;

                // Remove the response file
                std::fs::remove_file(&self.response_file)?;

                // Parse and return the message
                let message: CommunicationMessage = serde_json::from_str(&content)
                    .map_err(|e| CommunicationError::DeserializationFailed(e.to_string()))?;
                return Ok(message);
            }

            // Check timeout
            if start_time.elapsed() >= timeout_duration {
                return Err(CommunicationError::Timeout("No response received".to_string()));
            }

            // Wait a bit before checking again
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    async fn disconnect(&mut self) -> Result<(), CommunicationError> {
        self.connected = false;
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.connected
    }
}
