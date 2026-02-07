//! File-based communication protocol implementation
//! Provides IPC communication using files and file locking
//! Works on all platforms and doesn't require special permissions

use super::*;
use crate::ipc_log;
use std::path::Path;
use std::sync::{Arc, Mutex as StdMutex, atomic::{AtomicU64, Ordering}};
use std::collections::HashSet;
use tokio::io::{AsyncReadExt, AsyncWriteExt, AsyncSeekExt, AsyncBufReadExt};
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
    broadcast_file: String,
}

impl std::fmt::Debug for FileBasedServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileBasedServer")
            .field("config", &self.config)
            .field("lock_file", &self.lock_file)
            .field("broadcast_file", &self.broadcast_file)
            .finish()
    }
}

impl FileBasedServer {
    pub fn new(config: &CommunicationConfig) -> Result<Self, CommunicationError> {
        let message_file = Self::get_message_file(&config.identifier);
        let lock_file = Self::get_lock_file(&config.identifier);
        let broadcast_file = Self::get_broadcast_file(&config.identifier);

        Ok(Self {
            config: config.clone(),
            message_file,
            lock_file,
            is_running: Arc::new(Mutex::new(false)),
            message_handler: Arc::new(Mutex::<Option<MessageHandler>>::new(None)),
            broadcast_file,
        })
    }

    fn get_broadcast_file(identifier: &str) -> String {
        super::get_temp_path(identifier, "out")
    }

    fn get_message_file(identifier: &str) -> String {
        super::get_temp_path(identifier, "msg")
    }

    fn get_lock_file(identifier: &str) -> String {
        super::get_temp_path(identifier, "lock")
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
                // Read the message using tokio async file operations with retry
                let content = match tokio::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&self.message_file)
                    .await
                {
                    Ok(mut file) => {
                        let mut content = String::new();
                        match file.read_to_string(&mut content).await {
                            Ok(_) => {
                                // Remove the message file after successful read
                                let _ = tokio::fs::remove_file(&self.message_file).await;
                                content
                            }
                            Err(_) => {
                                // File might have been deleted by another process
                                tokio::time::sleep(Duration::from_millis(10)).await;
                                continue;
                            }
                        }
                    }
                    Err(_) => {
                        // File might have been deleted by another process
                        tokio::time::sleep(Duration::from_millis(10)).await;
                        continue;
                    }
                };

                if content.trim().is_empty() {
                    continue;
                }

                // Parse and return the message
                let message: CommunicationMessage = serde_json::from_str(&content)
                    .map_err(|e| CommunicationError::DeserializationFailed(e.to_string()))?;
                return Ok(Some(message));
            }

            // Wait a bit before checking again
            tokio::time::sleep(Duration::from_millis(50)).await;

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

        // Create response file using atomic write (write to temp then rename)
        let response_json = serde_json::to_string(response)?;
        let temp_file = format!("{}.temp.{}", self.message_file, std::process::id());
        
        // Write to temp file first
        {
            let mut file = tokio::fs::File::create(&temp_file).await?;
            file.write_all(response_json.as_bytes()).await?;
            file.flush().await?;
        }
        
        // Atomically rename to final location
        tokio::fs::rename(&temp_file, &response_file).await.or_else(|_| {
            // If rename fails, try direct write
            let mut file = tokio::fs::File::create(&response_file).await?;
            file.write_all(response_json.as_bytes()).await
        })?;

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

        // Clean up any existing broadcast file
        let _ = tokio::fs::remove_file(&self.broadcast_file).await;

        // Spawn a task to handle incoming messages
        let is_running = self.is_running.clone();
        let message_file = self.message_file.clone();
        let lock_file = self.lock_file.clone();
        let broadcast_file = self.broadcast_file.clone();
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
                    broadcast_file: broadcast_file.clone(),
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
                        
                        // Broadcast the message to all clients (excluding the sender ideally, but clients filter)
                        let _ = server.broadcast(message.clone()).await;
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
            let _ = tokio::fs::remove_file(&broadcast_file).await;
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

    async fn broadcast(&self, message: CommunicationMessage) -> Result<(), CommunicationError> {
        let json = serde_json::to_string(&message)?;
        let line = format!("{}\n", json);
        
        // Write to a temp file first, then atomically rename
        // This prevents clients from reading a partially written file
        let temp_file = format!("{}.temp.{}", self.broadcast_file, std::process::id());
        
        // Retry logic for handling concurrent file access
        let max_retries = 5;
        for attempt in 0..max_retries {
            match tokio::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .open(&temp_file)
                .await
            {
                Ok(mut file) => {
                    if let Err(e) = file.write_all(line.as_bytes()).await {
                        ipc_log!("Broadcast write failed: {}", e);
                    }
                    file.flush().await.ok();
                    
                    // Atomically rename to final location
                    // Use append mode by renaming to a final file and then appending
                    // Since we can't atomically append, we read the current content and rewrite
                    match tokio::fs::OpenOptions::new()
                        .read(true)
                        .write(true)
                        .create(true)
                        .append(true)
                        .open(&self.broadcast_file)
                        .await
                    {
                        Ok(mut final_file) => {
                            if let Err(e) = final_file.write_all(line.as_bytes()).await {
                                ipc_log!("Final broadcast write failed: {}", e);
                            }
                        }
                        Err(e) => {
                            ipc_log!("Failed to open broadcast file for append: {}", e);
                        }
                    }
                    
                    // Clean up temp file
                    let _ = tokio::fs::remove_file(&temp_file).await;
                    return Ok(());
                }
                Err(e) => {
                    // File might be locked by another process, retry after brief delay
                    if attempt < max_retries - 1 {
                        tokio::time::sleep(Duration::from_millis(20)).await;
                        continue;
                    }
                    ipc_log!("Broadcast failed after {} attempts: {}", max_retries, e);
                }
            }
        }
        
        // Final fallback: try direct append
        match tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.broadcast_file)
            .await
        {
            Ok(mut file) => {
                let _ = file.write_all(line.as_bytes()).await;
            }
            Err(_) => {
                // Still failed, but that's okay - another process handled it
            }
        }
        
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
    broadcast_file: String,
    last_broadcast_size: AtomicU64,
    seen_message_ids: StdMutex<HashSet<String>>,
}

impl FileBasedClient {
    pub fn new(config: &CommunicationConfig) -> Result<Self, CommunicationError> {
        let message_file = Self::get_message_file(&config.identifier);
        let response_file = format!("{}.response", message_file);
        let broadcast_file = Self::get_broadcast_file(&config.identifier);

        Ok(Self {
            config: config.clone(),
            message_file,
            response_file,
            connected: false,
            broadcast_file,
            last_broadcast_size: AtomicU64::new(0),
            seen_message_ids: StdMutex::new(HashSet::new()),
        })
    }

    fn get_message_file(identifier: &str) -> String {
        super::get_temp_path(identifier, "msg")
    }

    fn get_broadcast_file(identifier: &str) -> String {
        super::get_temp_path(identifier, "out")
    }

    async fn check_response(&self) -> Result<Option<CommunicationMessage>, CommunicationError> {
        if Path::new(&self.response_file).exists() {
            let mut file = tokio::fs::File::open(&self.response_file).await?;
            let mut content = String::new();
            file.read_to_string(&mut content).await?;
            // Remove the response file after reading
            let _ = tokio::fs::remove_file(&self.response_file).await;
            if content.trim().is_empty() {
                return Ok(None);
            }
            let message: CommunicationMessage = serde_json::from_str(&content)
                .map_err(|e| CommunicationError::DeserializationFailed(e.to_string()))?;
            return Ok(Some(message));
        }
        Ok(None)
    }
}

#[async_trait::async_trait]
impl CommunicationClient for FileBasedClient {
    async fn connect(&mut self) -> Result<(), CommunicationError> {
        // Check if lock file exists
        let lock_file = super::get_temp_path(&self.config.identifier, "lock");
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

    async fn send_message(&self, message: &CommunicationMessage) -> Result<(), CommunicationError> {
        if !self.connected {
            return Err(CommunicationError::ConnectionFailed(
                "Not connected".to_string(),
            ));
        }

        let message_json = serde_json::to_string(message)?;
        
        // Retry logic for handling concurrent access with server
        let max_retries = 5;
        for attempt in 0..max_retries {
            match tokio::fs::File::create(&self.message_file).await {
                Ok(mut file) => {
                    if let Err(e) = file.write_all(message_json.as_bytes()).await {
                        ipc_log!("Message write failed: {}", e);
                    }
                    return Ok(());
                }
                Err(_) => {
                    // File might be locked by another process, retry after brief delay
                    if attempt < max_retries - 1 {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                        continue;
                    }
                    ipc_log!("Send failed after {} attempts", max_retries);
                }
            }
        }
        
        Ok(())
    }

    async fn receive_message(&self) -> Result<CommunicationMessage, CommunicationError> {
        if !self.connected {
            return Err(CommunicationError::ConnectionFailed(
                "Not connected".to_string(),
            ));
        }

        // No local imports needed

        loop {
            // Check broadcast file for new messages from server
            if let Ok(mut file) = tokio::fs::File::open(&self.broadcast_file).await {
                let file_size = file.metadata().await.map(|m| m.len()).unwrap_or(0);
                let mut current_size = self.last_broadcast_size.load(Ordering::SeqCst);
                // Handle file truncation (e.g., server restarted)
                if file_size < current_size {
                    current_size = 0;
                    self.last_broadcast_size.store(0, Ordering::SeqCst);
                }
                if file_size > current_size {
                    // Seek to where we left off
                    file.seek(std::io::SeekFrom::Start(current_size)).await?;
                    let mut reader = tokio::io::BufReader::new(file);
                    loop {
                        let mut line = String::new();
                        let bytes_read = reader.read_line(&mut line).await?;
                        if bytes_read == 0 {
                            break; // EOF
                        }
                        if line.trim().is_empty() {
                            continue;
                        }
                        if let Ok(msg) = serde_json::from_str::<CommunicationMessage>(&line) {
                            let msg_id = msg.id.clone();
                            // Check if already seen (release lock before await)
                            let already_seen = {
                                let seen = self.seen_message_ids.lock().unwrap();
                                seen.contains(&msg_id)
                            };
                            if !already_seen {
                                // Mark as seen
                                self.seen_message_ids.lock().unwrap().insert(msg_id);
                                // Update offset to current position in file
                                let new_offset = reader.stream_position().await?;
                                self.last_broadcast_size.store(new_offset, Ordering::SeqCst);
                                return Ok(msg);
                            }
                        }
                    }
                    // No new unseen messages in this batch, update offset to current file end
                    if let Ok(pos) = reader.stream_position().await {
                        self.last_broadcast_size.store(pos, Ordering::SeqCst);
                    }
                }
            }

            // Check response file (for handshake or request-response)
            if let Ok(Some(msg)) = self.check_response().await {
                return Ok(msg);
            }

            // Wait a bit before retrying
            tokio::time::sleep(Duration::from_millis(10)).await;
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
