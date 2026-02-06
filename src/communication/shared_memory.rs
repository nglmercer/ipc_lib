//! Shared memory communication protocol implementation
//! Provides IPC communication using memory-mapped files
//! Works on most platforms and provides fast inter-process communication

use super::*;
use std::fs::OpenOptions;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::Duration;
use memmap2::{MmapMut, MmapOptions};

/// Shared memory communication protocol implementation
#[derive(Debug)]
pub struct SharedMemoryProtocol;

#[async_trait::async_trait]
impl CommunicationProtocol for SharedMemoryProtocol {
    fn protocol_type(&self) -> ProtocolType {
        ProtocolType::SharedMemory
    }

    async fn create_server(&self, config: &CommunicationConfig) -> Result<Box<dyn CommunicationServer>, CommunicationError> {
        Ok(Box::new(SharedMemoryServer::new(config)?))
    }

    async fn create_client(&self, config: &CommunicationConfig) -> Result<Box<dyn CommunicationClient>, CommunicationError> {
        Ok(Box::new(SharedMemoryClient::new(config)?))
    }
}

/// Shared memory server implementation
#[derive(Debug)]
pub struct SharedMemoryServer {
    config: CommunicationConfig,
    shm_file: String,
    #[allow(dead_code)]
    mmap: Arc<Mutex<Option<MmapMut>>>,
    is_running: Arc<Mutex<bool>>,
}

impl SharedMemoryServer {
    pub fn new(config: &CommunicationConfig) -> Result<Self, CommunicationError> {
        let shm_file = Self::get_shm_file(&config.identifier);

        // Clean up any existing shared memory file
        if Path::new(&shm_file).exists() {
            std::fs::remove_file(&shm_file)?;
        }

        // Create the shared memory file
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .mode(0o600)
            .open(&shm_file)?;

        // Set file size to accommodate our message buffer
        file.set_len(4096)?;

        Ok(Self {
            config: config.clone(),
            shm_file,
            mmap: Arc::new(Mutex::new(None)),
            is_running: Arc::new(Mutex::new(false)),
        })
    }

    fn get_shm_file(identifier: &str) -> String {
        format!("/tmp/{}.shm", identifier)
    }

    async fn wait_for_message(&self) -> Result<CommunicationMessage, CommunicationError> {
        let timeout_duration = Duration::from_millis(self.config.timeout_ms);
        
        loop {
            if !*self.is_running.blocking_lock() {
                return Err(CommunicationError::ConnectionFailed("Server stopped".to_string()));
            }

            // Map the shared memory
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .open(&self.shm_file)?;
            let mut mmap = unsafe { MmapOptions::new().map_mut(&file)? };

            // Check if there's data in shared memory
            let data = mmap.as_ref();
            if data[0] != 0 { // First byte indicates if there's data
                // Read the message length (first 4 bytes)
                let len = u32::from_ne_bytes([data[1], data[2], data[3], data[4]]) as usize;
                
                if len > 0 && len < 4096 {
                    // Read the message
                    let message_str = String::from_utf8_lossy(&data[5..5 + len]).into_owned();
                    
                    // Clear the shared memory
                    mmap.as_mut()[0] = 0; // Clear data flag
                    mmap.flush()?;

                    // Parse and return the message
                    let message: CommunicationMessage = serde_json::from_str(&message_str)
                        .map_err(|e| CommunicationError::DeserializationFailed(e.to_string()))?;
                    return Ok(message);
                }
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
        // Map the shared memory for writing
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.shm_file)?;
        let mut mmap = unsafe { MmapOptions::new().map_mut(&file)? };

        // Serialize the response
        let response_json = serde_json::to_string(response)?;
        let response_bytes = response_json.as_bytes();

        // Write the response to shared memory
        // Format: [data_flag(1)][length(4)][data(n)]
        mmap.as_mut()[0] = 1; // Set data flag
        mmap.as_mut()[1..5].copy_from_slice(&(response_bytes.len() as u32).to_ne_bytes());
        mmap.as_mut()[5..5 + response_bytes.len()].copy_from_slice(response_bytes);
        
        mmap.flush()?;
        
        Ok(())
    }
}

#[async_trait::async_trait]
impl CommunicationServer for SharedMemoryServer {
    async fn start(&mut self) -> Result<(), CommunicationError> {
        *self.is_running.lock().await = true;

        // Spawn a task to handle incoming messages
        let is_running = self.is_running.clone();
        let shm_file = self.shm_file.clone();

        tokio::spawn(async move {
            while *is_running.lock().await {
                // Wait for message
                match Self::wait_for_message(&Self {
                    config: CommunicationConfig::default(),
                    shm_file: shm_file.clone(),
                    mmap: Arc::new(Mutex::new(None)),
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
                                    shm_file: shm_file.clone(),
                                    mmap: Arc::new(Mutex::new(None)),
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
                                    shm_file: shm_file.clone(),
                                    mmap: Arc::new(Mutex::new(None)),
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

            // Clean up shared memory file when server stops
            let _ = std::fs::remove_file(&shm_file);
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
        format!("shm://{}", self.shm_file)
    }
}

/// Shared memory client implementation
#[derive(Debug)]
pub struct SharedMemoryClient {
    config: CommunicationConfig,
    shm_file: String,
    connected: bool,
}

impl SharedMemoryClient {
    pub fn new(config: &CommunicationConfig) -> Result<Self, CommunicationError> {
        Ok(Self {
            config: config.clone(),
            shm_file: Self::get_shm_file(&config.identifier),
            connected: false,
        })
    }

    fn get_shm_file(identifier: &str) -> String {
        format!("/tmp/{}.shm", identifier)
    }

    async fn wait_for_response(&self) -> Result<CommunicationMessage, CommunicationError> {
        let timeout_duration = Duration::from_millis(self.config.timeout_ms);
        
        let start_time = std::time::Instant::now();
        
        loop {
            // Map the shared memory
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .open(&self.shm_file)?;
            let mut mmap = unsafe { MmapOptions::new().map_mut(&file)? };

            // Check if there's data in shared memory
            if mmap.as_ref()[0] != 0 { // First byte indicates if there's data
                // Read the message length (first 4 bytes)
                let len = u32::from_ne_bytes([mmap.as_ref()[1], mmap.as_ref()[2], mmap.as_ref()[3], mmap.as_ref()[4]]) as usize;
                
                if len > 0 && len < 4096 {
                    // Read the message
                    let message_str = String::from_utf8_lossy(&mmap.as_ref()[5..5 + len]).into_owned();
                    
                    // Clear the shared memory
                    mmap.as_mut()[0] = 0; // Clear data flag
                    mmap.flush()?;

                    // Parse and return the message
                    let message: CommunicationMessage = serde_json::from_str(&message_str)
                        .map_err(|e| CommunicationError::DeserializationFailed(e.to_string()))?;
                    return Ok(message);
                }
            }

            // Check timeout
            if start_time.elapsed() >= timeout_duration {
                return Err(CommunicationError::Timeout("No response received".to_string()));
            }

            // Wait a bit before checking again
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

#[async_trait::async_trait]
impl CommunicationClient for SharedMemoryClient {
    async fn connect(&mut self) -> Result<(), CommunicationError> {
        // Check if shared memory file exists
        if !Path::new(&self.shm_file).exists() {
            return Err(CommunicationError::ConnectionFailed("Server not running".to_string()));
        }

        self.connected = true;
        Ok(())
    }

    async fn send_message(&mut self, message: &CommunicationMessage) -> Result<(), CommunicationError> {
        if !self.connected {
            return Err(CommunicationError::ConnectionFailed("Not connected".to_string()));
        }

        // Map the shared memory for writing
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.shm_file)?;
        let mut mmap = unsafe { MmapOptions::new().map_mut(&file)? };

        // Serialize the message
        let message_json = serde_json::to_string(message)?;
        let message_bytes = message_json.as_bytes();

        // Write the message to shared memory
        // Format: [data_flag(1)][length(4)][data(n)]
        mmap.as_mut()[0] = 1; // Set data flag
        mmap.as_mut()[1..5].copy_from_slice(&(message_bytes.len() as u32).to_ne_bytes());
        mmap.as_mut()[5..5 + message_bytes.len()].copy_from_slice(message_bytes);
        
        mmap.flush()?;
        
        Ok(())
    }

    async fn receive_message(&mut self) -> Result<CommunicationMessage, CommunicationError> {
        if !self.connected {
            return Err(CommunicationError::ConnectionFailed("Not connected".to_string()));
        }

        self.wait_for_response().await
    }

    async fn disconnect(&mut self) -> Result<(), CommunicationError> {
        self.connected = false;
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.connected
    }
}
