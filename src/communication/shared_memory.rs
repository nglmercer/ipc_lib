#![allow(unused)]
//! Shared memory communication protocol implementation
//! Provides IPC communication using memory-mapped files
//! Works on Unix-like systems and Windows

use super::*;
use crate::ipc_log;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::Duration;

#[cfg(unix)]
use memmap2::MmapOptions;

#[cfg(windows)]
use winapi::um::handleapi::CloseHandle;
#[cfg(windows)]
use winapi::um::memoryapi::MapViewOfFile;
#[cfg(windows)]
use winapi::um::memoryapi::UnmapViewOfFile;
#[cfg(windows)]
use winapi::um::processthreadsapi::GetCurrentProcessId;
#[cfg(windows)]
use winapi::um::winbase::CreateFileMappingA;
#[cfg(windows)]
use winapi::um::winbase::FILE_MAP_ALL_ACCESS;
#[cfg(windows)]
use winapi::um::winbase::FILE_MAP_READ;
#[cfg(windows)]
use winapi::um::winbase::FILE_MAP_WRITE;
#[cfg(windows)]
use winapi::um::winnt::PAGE_READWRITE;
#[cfg(windows)]
use std::ptr::null_mut;

/// Shared memory communication protocol implementation
#[derive(Debug)]
pub struct SharedMemoryProtocol;

#[async_trait::async_trait]
impl CommunicationProtocol for SharedMemoryProtocol {
    fn protocol_type(&self) -> ProtocolType {
        ProtocolType::SharedMemory
    }

    async fn create_server(
        &self,
        config: &CommunicationConfig,
    ) -> Result<Box<dyn CommunicationServer>, CommunicationError> {
        Ok(Box::new(SharedMemoryServer::new(config)?))
    }

    async fn create_client(
        &self,
        config: &CommunicationConfig,
    ) -> Result<Box<dyn CommunicationClient>, CommunicationError> {
        Ok(Box::new(SharedMemoryClient::new(config)?))
    }
}

/// Shared memory server implementation
#[allow(dead_code)]
pub struct SharedMemoryServer {
    config: CommunicationConfig,
    shm_name: String,
    is_running: Arc<Mutex<bool>>,
    message_handler: SharedMessageHandler,
    #[cfg(windows)]
    mapping_handle: *mut std::ffi::c_void,
}

impl std::fmt::Debug for SharedMemoryServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedMemoryServer")
            .field("config", &self.config)
            .field("shm_name", &self.shm_name)
            .finish()
    }
}

impl SharedMemoryServer {
    pub fn new(config: &CommunicationConfig) -> Result<Self, CommunicationError> {
        let shm_name = Self::get_shm_name(&config.identifier);

        #[cfg(unix)]
        {
            let shm_file = Self::get_shm_file(&config.identifier);

            // Clean up any existing shared memory file
            if Path::new(&shm_file).exists() {
                std::fs::remove_file(&shm_file)?;
            }

            // Create the shared memory file
            use std::fs::OpenOptions;
            use std::os::unix::fs::OpenOptionsExt;

            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&shm_file)?;

            // Set file size to accommodate our message buffer
            file.set_len(4096)?;

            Ok(Self {
                config: config.clone(),
                shm_file,
                is_running: Arc::new(Mutex::new(false)),
                message_handler: Arc::new(Mutex::new(None)),
            })
        }

        #[cfg(windows)]
        {
            // Create file mapping for shared memory
            let mapping = unsafe {
                CreateFileMappingA(
                    winapi::um::winbase::INVALID_HANDLE_VALUE,
                    null_mut(),
                    PAGE_READWRITE,
                    0,
                    4096,
                    shm_name.as_ptr() as *const i8,
                )
            };

            if mapping.is_null() {
                return Err(CommunicationError::IoError(
                    "Failed to create file mapping".to_string(),
                ));
            }

            Ok(Self {
                config: config.clone(),
                shm_name,
                is_running: Arc::new(Mutex::new(false)),
                message_handler: Arc::new(Mutex::new(None)),
                mapping_handle: mapping as *mut std::ffi::c_void,
            })
        }
    }

    #[cfg(unix)]
    fn get_shm_file(identifier: &str) -> String {
        super::get_temp_path(identifier, "shm")
    }

    #[cfg(windows)]
    fn get_shm_name(identifier: &str) -> String {
        format!("Local\\IPC_LIB_{}", identifier)
    }

    #[cfg(unix)]
    fn get_shm_name(identifier: &str) -> String {
        super::get_temp_path(identifier, "shm")
    }

    async fn wait_for_message(&self) -> Result<CommunicationMessage, CommunicationError> {
        let timeout_duration = Duration::from_millis(self.config.timeout_ms);
        let start_time = std::time::Instant::now();

        loop {
            // Check if server is still running
                        let message_bytes = data[5..5 + len].to_vec();

                        // Clear the shared memory flag immediately
                        mmap.as_mut()[0] = 0; // Clear data flag
                        mmap.flush()?;

                        // Parse the message based on format (using the owned vector)
                        let message_res = match format {
                            SerializationFormat::Json => {
                                let message_str = String::from_utf8_lossy(&message_bytes);
                                serde_json::from_str::<CommunicationMessage>(&message_str).map_err(
                                    |e| CommunicationError::DeserializationFailed(e.to_string()),
                                )
                            }
                            SerializationFormat::MsgPack => {
                                rmp_serde::from_slice::<CommunicationMessage>(&message_bytes)
                                    .map_err(|e| {
                                        CommunicationError::DeserializationFailed(e.to_string())
                                    })
                            }
                        };
                        return Ok(Some(message_res?));
                    }
                }
                Ok(None)
            }).await;
            
            #[cfg(not(unix))]
            let result = Err(CommunicationError::ProtocolNotSupported(
                "SharedMemory is only supported on Unix-like systems".to_string(),
            ));

            match result {
                Ok(Ok(Some(message))) => return Ok(message),
                Ok(Ok(None)) => {
                    // No message yet, continue waiting
                }
                Ok(Err(e)) => return Err(e),
                Err(_) => return Err(CommunicationError::IoError("Spawn blocking failed".to_string())),
            }

            // Wait a bit before checking again
            tokio::time::sleep(Duration::from_millis(100)).await;

            // Check timeout
            if start_time.elapsed() >= timeout_duration {
                return Err(CommunicationError::Timeout(
                    "No message received".to_string(),
                ));
            }
        }
    }

    async fn send_response(
        &self,
        response: &CommunicationMessage,
    ) -> Result<(), CommunicationError> {
        let shm_file = self.shm_file.clone();
        let format = self.config.serialization_format;

        let response_bytes = match format {
            SerializationFormat::Json => {
                let s = serde_json::to_string(response)?;
                s.into_bytes()
            }
            SerializationFormat::MsgPack => rmp_serde::to_vec(response)
                .map_err(|e| CommunicationError::SerializationFailed(e.to_string()))?,
        };

        #[cfg(unix)]
        let result = tokio::task::spawn_blocking(move || {
            use std::fs::OpenOptions;
            use memmap2::MmapOptions;

            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .open(&shm_file)?;
            let mut mmap = unsafe { MmapOptions::new().map_mut(&file)? };

            // Write the response to shared memory
            // Format: [data_flag(1)][length(4)][data(n)]
            mmap.as_mut()[0] = 1; // Set data flag
            mmap.as_mut()[1..5].copy_from_slice(&(response_bytes.len() as u32).to_ne_bytes());
            mmap.as_mut()[5..5 + response_bytes.len()].copy_from_slice(&response_bytes);

            mmap.flush()?;

            Ok::<(), CommunicationError>(())
        }).await;
        
        #[cfg(not(unix))]
        let result = Err(CommunicationError::ProtocolNotSupported(
            "SharedMemory is only supported on Unix-like systems".to_string(),
        ));

        match result {
            Ok(inner) => inner,
            Err(_) => Err(CommunicationError::IoError("Spawn blocking failed".to_string())),
        }
    }
}

#[async_trait::async_trait]
impl CommunicationServer for SharedMemoryServer {
    async fn start(&mut self) -> Result<(), CommunicationError> {
        *self.is_running.lock().await = true;

        // Spawn a task to handle incoming messages
        let is_running = self.is_running.clone();
        let shm_file = self.shm_file.clone();
        let message_handler = self.message_handler.clone();
        let config = self.config.clone();

        tokio::spawn(async move {
            while *is_running.lock().await {
                // Wait for message - clone config for each iteration
                let server_config = config.clone();
                let server = Self {
                    config: server_config,
                    shm_file: shm_file.clone(),
                    is_running: is_running.clone(),
                    message_handler: message_handler.clone(),
                };
                match server.wait_for_message().await {
                    Ok(message) => {
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
                    Err(e) => {
                        // Only log if server is still running
                        if *is_running.lock().await {
                            eprintln!("Error handling message: {}", e);
                        }
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
        // Use a non-blocking attempt to check running status
        match self.is_running.try_lock() {
            Ok(guard) => *guard,
            Err(_) => {
                // If we can't acquire the lock, assume it's running
                true
            }
        }
    }

    fn endpoint(&self) -> String {
        format!("shm://{}", self.shm_file)
    }

    fn set_message_handler(&self, handler: MessageHandler) -> Result<(), CommunicationError> {
        let message_handler = self.message_handler.clone();
        tokio::spawn(async move {
            *message_handler.lock().await = Some(handler);
        });
        Ok(())
    }
}

/// Shared memory client implementation
#[allow(dead_code)]
#[derive(Debug)]
pub struct SharedMemoryClient {
    config: CommunicationConfig,
    shm_file: String,
    connected: bool,
}

impl SharedMemoryClient {
    pub fn new(config: &CommunicationConfig) -> Result<Self, CommunicationError> {
        #[cfg(unix)]
        {
            Ok(Self {
                config: config.clone(),
                shm_file: Self::get_shm_file(&config.identifier),
                connected: false,
            })
        }
        #[cfg(not(unix))]
        {
            Err(CommunicationError::ProtocolNotSupported(
                "SharedMemory is only supported on Unix-like systems".to_string(),
            ))
        }
    }

    fn get_shm_file(identifier: &str) -> String {
        super::get_temp_path(identifier, "shm")
    }

    async fn wait_for_response(&self) -> Result<CommunicationMessage, CommunicationError> {
        let timeout_duration = Duration::from_millis(self.config.timeout_ms);
        let start_time = std::time::Instant::now();

        loop {
            // Check if shared memory file exists
            if !Path::new(&self.shm_file).exists() {
                tokio::time::sleep(Duration::from_millis(100)).await;
                if start_time.elapsed() >= timeout_duration {
                    return Err(CommunicationError::Timeout(
                        "No response received".to_string(),
                    ));
                }
                continue;
            }

            // Map the shared memory (blocking operation)
            let shm_file = self.shm_file.clone();
            let format = self.config.serialization_format;
            
            #[cfg(unix)]
            let result = tokio::task::spawn_blocking(move || {
                use std::fs::OpenOptions;
                use memmap2::MmapOptions;

                let file = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&shm_file)?;
                let mut mmap = unsafe { MmapOptions::new().map_mut(&file)? };

                // Check if there's data in shared memory
                if mmap.as_ref()[0] != 0 {
                    // First byte indicates if there's data
                    // Read the message length (first 4 bytes)
                    let len = u32::from_ne_bytes([
                        mmap.as_ref()[1],
                        mmap.as_ref()[2],
                        mmap.as_ref()[3],
                        mmap.as_ref()[4],
                    ]) as usize;

                    if len > 0 && len < 4096 {
                        // Read the message bytes (Clone to vector to avoid borrow checker issues)
                        let message_bytes = mmap.as_ref()[5..5 + len].to_vec();

                        // Clear the shared memory flag immediately
                        mmap.as_mut()[0] = 0; // Clear data flag
                        mmap.flush()?;

                        // Parse and return the message
                        let message_res = match format {
                            SerializationFormat::Json => {
                                let message_str = String::from_utf8_lossy(&message_bytes);
                                serde_json::from_str::<CommunicationMessage>(&message_str).map_err(
                                    |e| CommunicationError::DeserializationFailed(e.to_string()),
                                )
                            }
                            SerializationFormat::MsgPack => {
                                rmp_serde::from_slice::<CommunicationMessage>(&message_bytes)
                                    .map_err(|e| {
                                        CommunicationError::DeserializationFailed(e.to_string())
                                    })
                            }
                        };
                        return Ok(Some(message_res?));
                    }
                }
                Ok(None)
            }).await;
            
            #[cfg(not(unix))]
            let result = Err(CommunicationError::ProtocolNotSupported(
                "SharedMemory is only supported on Unix-like systems".to_string(),
            ));

            match result {
                Ok(Ok(Some(message))) => return Ok(message),
                Ok(Ok(None)) => {
                    // No response yet, continue waiting
                }
                Ok(Err(e)) => return Err(e),
                Err(_) => return Err(CommunicationError::IoError("Spawn blocking failed".to_string())),
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
}

#[async_trait::async_trait]
impl CommunicationClient for SharedMemoryClient {
    async fn connect(&mut self) -> Result<(), CommunicationError> {
        // Check if shared memory file exists
        if !Path::new(&self.shm_file).exists() {
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

        let shm_file = self.shm_file.clone();
        let format = self.config.serialization_format;

        let message_bytes = match format {
            SerializationFormat::Json => {
                let s = serde_json::to_string(message)?;
                s.into_bytes()
            }
            SerializationFormat::MsgPack => rmp_serde::to_vec(message)
                .map_err(|e| CommunicationError::SerializationFailed(e.to_string()))?,
        };

        #[cfg(unix)]
        let result = tokio::task::spawn_blocking(move || {
            use std::fs::OpenOptions;
            use memmap2::MmapOptions;

            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .open(&shm_file)?;
            let mut mmap = unsafe { MmapOptions::new().map_mut(&file)? };

            // Write the message to shared memory
            // Format: [data_flag(1)][length(4)][data(n)]
            mmap.as_mut()[0] = 1; // Set data flag
            mmap.as_mut()[1..5].copy_from_slice(&(message_bytes.len() as u32).to_ne_bytes());
            mmap.as_mut()[5..5 + message_bytes.len()].copy_from_slice(&message_bytes);

            mmap.flush()?;

            Ok::<(), CommunicationError>(())
        }).await;
        
        #[cfg(not(unix))]
        let result = Err(CommunicationError::ProtocolNotSupported(
            "SharedMemory is only supported on Unix-like systems".to_string(),
        ));

        match result {
            Ok(inner) => inner,
            Err(_) => Err(CommunicationError::IoError("Spawn blocking failed".to_string())),
        }
    }

    async fn receive_message(&self) -> Result<CommunicationMessage, CommunicationError> {
        if !self.connected {
            return Err(CommunicationError::ConnectionFailed(
                "Not connected".to_string(),
            ));
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
