//! File-based communication protocol implementation
//! Provides IPC communication using files and file locking
//! Works on all platforms and doesn't require special permissions

use super::*;
use crate::ipc_log;
use std::path::Path;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
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

    async fn create_server(
        &self,
        config: &CommunicationConfig,
    ) -> Result<Box<dyn CommunicationServer>, CommunicationError> {
        Ok(Box::new(FileBasedServer::new(config)?))
    }

    async fn create_client(
        &self,
        config: &CommunicationConfig,
    ) -> Result<Box<dyn CommunicationClient>, CommunicationError> {
        Ok(Box::new(FileBasedClient::new(config)?))
    }
}

/// File-based server implementation
pub struct FileBasedServer {
    config: CommunicationConfig,
    message_file: String,
    lock_file: String,
    is_running: Arc<Mutex<bool>>,
    message_handler: SharedMessageHandler,
}

impl std::fmt::Debug for FileBasedServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileBasedServer")
            .field("config", &self.config)
            .field("lock_file", &self.lock_file)
            .finish()
    }
}

impl FileBasedServer {
    pub fn new(config: &CommunicationConfig) -> Result<Self, CommunicationError> {
        let message_file = Self::get_message_file(&config.identifier);
        let lock_file = Self::get_lock_file(&config.identifier);

        Ok(Self {
            config: config.clone(),
            message_file,
            lock_file,
            is_running: Arc::new(Mutex::new(false)),
            message_handler: Arc::new(Mutex::new(None)),
        })
    }

    fn get_message_file(identifier: &str) -> String {
        format!("/tmp/{}.msg", identifier)
    }

    fn get_lock_file(identifier: &str) -> String {
        format!("/tmp/{}.lock", identifier)
    }

    async fn wait_for_message(&self) -> Result<Option<CommunicationMessage>, CommunicationError> {
        let timeout_duration = Duration::from_millis(self.config.timeout_ms);
        let start_time = std::time::Instant::now();

        loop {
            // Check if server is still running
            if !*self.is_running.lock().await {
                return Ok(None);
            }

            // Check if message file exists
            if Path::new(&self.message_file).exists() {
                // Read the message using tokio async file operations
                let content = match tokio::fs::read_to_string(&self.message_file).await {
                    Ok(c) => c,
                    Err(_) => {
                        // File might have been deleted by another process/instance
                        tokio::time::sleep(Duration::from_millis(10)).await;
                        continue;
                    }
                };

                // Remove the message file
                let _ = tokio::fs::remove_file(&self.message_file).await;

                if content.trim().is_empty() {
                    continue;
                }

                // Parse and return the message
                let message: CommunicationMessage = serde_json::from_str(&content)
                    .map_err(|e| CommunicationError::DeserializationFailed(e.to_string()))?;
                return Ok(Some(message));
            }

            // Wait a bit before checking again
            tokio::time::sleep(Duration::from_millis(100)).await;

            // Check timeout
            if start_time.elapsed() >= timeout_duration {
                return Ok(None);
            }
        }
    }

    async fn send_response(
        &self,
        response: &CommunicationMessage,
    ) -> Result<(), CommunicationError> {
        let response_file = format!("{}.response", self.message_file);

        // Create response file using tokio async file operations
        let mut file = tokio::fs::File::create(&response_file).await?;
        let response_json = serde_json::to_string(response)?;
        file.write_all(response_json.as_bytes()).await?;

        Ok(())
    }
}

#[async_trait::async_trait]
impl CommunicationServer for FileBasedServer {
    async fn start(&mut self) -> Result<(), CommunicationError> {
        ipc_log!("FileBasedServer starting, lock_file: {}", self.lock_file);
        // Try to create lock file exclusively to indicate server is running
        match tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&self.lock_file)
            .await
        {
            Ok(mut lock_file) => {
                lock_file.write_all(b"running").await?;
                lock_file.flush().await?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(CommunicationError::ConnectionFailed(
                    "Server lock file already exists".to_string(),
                ));
            }
            Err(e) => return Err(e.into()),
        }

        *self.is_running.lock().await = true;

        // Spawn a task to handle incoming messages
        let is_running = self.is_running.clone();
        let message_file = self.message_file.clone();
        let lock_file = self.lock_file.clone();
        let message_handler = self.message_handler.clone();
        let config = self.config.clone();

        tokio::spawn(async move {
            while *is_running.lock().await {
                // Wait for message - clone config for each iteration
                let server_config = config.clone();
                let server = Self {
                    config: server_config,
                    message_file: message_file.clone(),
                    lock_file: lock_file.clone(),
                    is_running: is_running.clone(),
                    message_handler: message_handler.clone(),
                };
                match server.wait_for_message().await {
                    Ok(Some(message)) => {
                        ipc_log!("Received message type: {}", message.message_type);

                        // Call message handler if set and get optional response
                        let response = if let Some(ref handler) = *message_handler.lock().await {
                            handler(message.clone())
                        } else {
                            None
                        };

                        // Default response if none provided by handler
                        let response = response
                            .unwrap_or_else(|| message.create_reply(serde_json::json!("Received")));

                        let _ = server.send_response(&response).await;
                    }
                    Ok(None) => {
                        // Timeout or server stopped, just continue
                    }
                    Err(e) => {
                        eprintln!("Error handling message: {}", e);
                        // Sleep a bit to avoid tight loop on persistent error
                        tokio::time::sleep(Duration::from_millis(500)).await;
                    }
                }
            }

            // Clean up files when server stops
            let _ = tokio::fs::remove_file(&message_file).await;
            let _ = tokio::fs::remove_file(&lock_file).await;
            let _ = tokio::fs::remove_file(&format!("{}.response", message_file)).await;
        });

        Ok(())
    }

    async fn stop(&mut self) -> Result<(), CommunicationError> {
        *self.is_running.lock().await = false;
        Ok(())
    }

    fn is_running(&self) -> bool {
        // Use a non-blocking attempt to check running status
        // This is a best-effort check; in async contexts, prefer await
        match self.is_running.try_lock() {
            Ok(guard) => *guard,
            Err(_) => {
                // If we can't acquire the lock, assume it's running
                true
            }
        }
    }

    fn endpoint(&self) -> String {
        format!("file://{}", self.lock_file)
    }

    fn set_message_handler(&self, handler: MessageHandler) -> Result<(), CommunicationError> {
        let message_handler = self.message_handler.clone();
        tokio::spawn(async move {
            *message_handler.lock().await = Some(handler);
        });
        Ok(())
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
        ipc_log!(
            "FileBasedClient connecting, checking lock_file: {}",
            lock_file
        );
        if !Path::new(&lock_file).exists() {
            return Err(CommunicationError::ConnectionFailed(
                "Server not running".to_string(),
            ));
        }

        self.connected = true;
        Ok(())
    }

    async fn send_message(
        &self,
        message: &CommunicationMessage,
    ) -> Result<(), CommunicationError> {
        if !self.connected {
            return Err(CommunicationError::ConnectionFailed(
                "Not connected".to_string(),
            ));
        }

        // Create message file using tokio async file operations
        let mut file = tokio::fs::File::create(&self.message_file).await?;
        let message_json = serde_json::to_string(message)?;
        file.write_all(message_json.as_bytes()).await?;

        Ok(())
    }

    async fn receive_message(&self) -> Result<CommunicationMessage, CommunicationError> {
        if !self.connected {
            return Err(CommunicationError::ConnectionFailed(
                "Not connected".to_string(),
            ));
        }

        let timeout_duration = Duration::from_millis(self.config.timeout_ms);
        let start_time = std::time::Instant::now();

        loop {
            // Check if response file exists
            if Path::new(&self.response_file).exists() {
                // Read the response using tokio async file operations
                let mut file = tokio::fs::File::open(&self.response_file).await?;
                let mut content = String::new();
                file.read_to_string(&mut content).await?;

                // Remove the response file
                tokio::fs::remove_file(&self.response_file).await?;

                // Parse and return the message
                let message: CommunicationMessage = serde_json::from_str(&content)
                    .map_err(|e| CommunicationError::DeserializationFailed(e.to_string()))?;
                return Ok(message);
            }

            // Check timeout
            if start_time.elapsed() >= timeout_duration {
                return Err(CommunicationError::Timeout(
                    "No response received".to_string(),
                ));
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
