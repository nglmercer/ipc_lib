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
use winapi::ctypes::c_void;
#[cfg(windows)]
use winapi::um::handleapi::CloseHandle;
#[cfg(windows)]
use winapi::um::memoryapi::MapViewOfFile;
#[cfg(windows)]
use winapi::um::memoryapi::UnmapViewOfFile;
#[cfg(windows)]
use winapi::um::winbase::CreateFileMappingA;
#[cfg(windows)]
use winapi::um::winbase::OpenFileMappingA;
#[cfg(windows)]
use winapi::um::winnt::PAGE_READWRITE;

// Constants for Windows shared memory
#[cfg(windows)]
const FILE_MAP_ALL_ACCESS: u32 = 0xf001f;
#[cfg(windows)]
const INVALID_HANDLE_VALUE: *mut c_void = -1isize as *mut c_void;

// Shared memory protocol constants
const SHM_SIZE: usize = 4096;
const DATA_FLAG_OFFSET: usize = 0;
const LENGTH_OFFSET: usize = 1;
const DATA_OFFSET: usize = 5;

// Flag values:
// 0 = empty/idle
// 1 = client message (request)
// 2 = server response to a request
// 3 = broadcast message from server

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
    broadcast_tx: tokio::sync::broadcast::Sender<CommunicationMessage>,
    #[cfg(unix)]
    shm_file: String,
    #[cfg(windows)]
    mapping_handle: isize,
    #[cfg(unix)]
    lock_file: String,
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
        let shm_file = Self::get_shm_file(&config.identifier);
        #[cfg(unix)]
        let lock_file = Self::get_lock_file(&config.identifier);

        #[cfg(unix)]
        {
            // Clean up any existing shared memory file
            if Path::new(&shm_file).exists() {
                std::fs::remove_file(&shm_file)?;
            }
            if Path::new(&lock_file).exists() {
                std::fs::remove_file(&lock_file)?;
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
            file.set_len(SHM_SIZE as u64)?;

            // Initialize the lock file
            let _ = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&lock_file)?;

            Ok(Self {
                config: config.clone(),
                shm_file,
                shm_name,
                lock_file,
                is_running: Arc::new(Mutex::new(false)),
                message_handler: Arc::new(Mutex::new(None)),
                mapping_handle: 0,
                broadcast_tx: tokio::sync::broadcast::channel(100).0,
            })
        }

        #[cfg(windows)]
        {
            // Create file mapping for shared memory
            let mapping = unsafe {
                CreateFileMappingA(
                    INVALID_HANDLE_VALUE,
                    std::ptr::null_mut(),
                    PAGE_READWRITE,
                    0,
                    SHM_SIZE as u32,
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
                mapping_handle: mapping as isize,
                broadcast_tx: tokio::sync::broadcast::channel(100).0,
            })
        }
    }

    #[cfg(unix)]
    fn get_shm_file(identifier: &str) -> String {
        super::get_temp_path(identifier, "shm")
    }

    #[cfg(unix)]
    fn get_lock_file(identifier: &str) -> String {
        super::get_temp_path(identifier, "lock")
    }

    #[cfg(windows)]
    fn get_shm_name(identifier: &str) -> String {
        format!("Local\\IPC_LIB_SHM_{}", identifier)
    }

    #[cfg(unix)]
    fn get_shm_name(identifier: &str) -> String {
        super::get_temp_path(identifier, "shm")
    }

    #[cfg(unix)]
    fn acquire_lock(&self) -> Result<std::fs::File, CommunicationError> {
        use std::fs::OpenOptions;
        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .open(&self.lock_file)?;
        use std::os::unix::fs::FileExt;
        file.lock_exclusive()?;
        Ok(file)
    }

    #[cfg(unix)]
    fn release_lock(&self, _file: std::fs::File) {}

    #[cfg(windows)]
    fn acquire_lock(&self) -> Result<(), CommunicationError> {
        Ok(())
    }

    #[cfg(windows)]
    fn release_lock(&self, _file: ()) {}

    async fn wait_for_message(&self) -> Result<CommunicationMessage, CommunicationError> {
        let timeout_duration = Duration::from_millis(self.config.timeout_ms);
        let start_time = std::time::Instant::now();
        let format = self.config.serialization_format;

        loop {
            if !*self.is_running.lock().await {
                return Err(CommunicationError::ConnectionFailed(
                    "Server stopped".to_string(),
                ));
            }

            #[cfg(unix)]
            {
                if !Path::new(&self.shm_file).exists() {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    if start_time.elapsed() >= timeout_duration {
                        return Err(CommunicationError::Timeout(
                            "No message received".to_string(),
                        ));
                    }
                    continue;
                }

                let _lock = match self.acquire_lock() {
                    Ok(lock) => lock,
                    Err(_) => {
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        continue;
                    }
                };

                let result = tokio::task::spawn_blocking(move || {
                    use memmap2::MmapOptions;
                    use std::fs::OpenOptions;
                    use std::os::unix::fs::FileExt;

                    let file = OpenOptions::new().read(true).write(true).open(&shm_file)?;
                    let mut mmap = unsafe { MmapOptions::new().map_mut(&file)? };
                    let data = mmap.as_ref();
                    let data_flag = data[DATA_FLAG_OFFSET];

                    drop(mmap);
                    drop(file);

                    if data_flag == 1 {
                        let len = u32::from_ne_bytes([
                            data[LENGTH_OFFSET],
                            data[LENGTH_OFFSET + 1],
                            data[LENGTH_OFFSET + 2],
                            data[LENGTH_OFFSET + 3],
                        ]) as usize;

                        if len > 0 && len < SHM_SIZE - DATA_OFFSET {
                            let file = OpenOptions::new().read(true).write(true).open(&shm_file)?;
                            let mut mmap = unsafe { MmapOptions::new().map_mut(&file)? };
                            let data = mmap.as_ref();
                            let message_bytes = data[DATA_OFFSET..DATA_OFFSET + len].to_vec();
                            mmap.as_mut()[DATA_FLAG_OFFSET] = 0;
                            mmap.flush()?;

                            let message_res = match format {
                                SerializationFormat::Json => {
                                    let message_str = String::from_utf8_lossy(&message_bytes);
                                    serde_json::from_str::<CommunicationMessage>(&message_str)
                                        .map_err(|e| {
                                            CommunicationError::DeserializationFailed(e.to_string())
                                        })
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
                    Ok::<Option<CommunicationMessage>, CommunicationError>(None)
                })
                .await;

                match result {
                    Ok(Ok(Some(message))) => return Ok(message),
                    Ok(Ok(None)) => {}
                    Ok(Err(e)) => return Err(e),
                    Err(_) => {
                        return Err(CommunicationError::IoError(
                            "Spawn blocking failed".to_string(),
                        ))
                    }
                }
            }

            #[cfg(windows)]
            {
                let mapping = unsafe {
                    OpenFileMappingA(FILE_MAP_ALL_ACCESS, 0, self.shm_name.as_ptr() as *const i8)
                };

                if mapping.is_null() {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    if start_time.elapsed() >= timeout_duration {
                        return Err(CommunicationError::Timeout(
                            "No message received".to_string(),
                        ));
                    }
                    continue;
                }

                let view = unsafe { MapViewOfFile(mapping, FILE_MAP_ALL_ACCESS, 0, 0, SHM_SIZE) };

                if view.is_null() {
                    unsafe {
                        CloseHandle(mapping as *mut c_void);
                    }
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    continue;
                }

                let data: &[u8; SHM_SIZE] = unsafe { &*(view as *const [u8; SHM_SIZE]) };
                let data_flag = data[DATA_FLAG_OFFSET];

                if data_flag == 1 {
                    let len = u32::from_ne_bytes([
                        data[LENGTH_OFFSET],
                        data[LENGTH_OFFSET + 1],
                        data[LENGTH_OFFSET + 2],
                        data[LENGTH_OFFSET + 3],
                    ]) as usize;

                    if len > 0 && len < SHM_SIZE - DATA_OFFSET {
                        let message_bytes = data[DATA_OFFSET..DATA_OFFSET + len].to_vec();

                        unsafe {
                            *(view as *mut u8) = 0;
                        }

                        unsafe {
                            UnmapViewOfFile(view);
                        }
                        unsafe {
                            CloseHandle(mapping as *mut c_void);
                        }

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

                        match message_res {
                            Ok(msg) => return Ok(msg),
                            Err(e) => return Err(e),
                        }
                    }
                }

                unsafe {
                    UnmapViewOfFile(view);
                }
                unsafe {
                    CloseHandle(mapping as *mut c_void);
                }
            }

            tokio::time::sleep(Duration::from_millis(50)).await;

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
        let format = self.config.serialization_format;

        let response_bytes = match format {
            SerializationFormat::Json => {
                let s = serde_json::to_string(response)?;
                s.into_bytes()
            }
            SerializationFormat::MsgPack => rmp_serde::to_vec(response)
                .map_err(|e| CommunicationError::SerializationFailed(e.to_string()))?,
        };

        if response_bytes.len() >= SHM_SIZE - DATA_OFFSET {
            return Err(CommunicationError::SerializationFailed(
                "Response too large".to_string(),
            ));
        }

        #[cfg(unix)]
        {
            use std::fs::OpenOptions;
            use std::os::unix::fs::FileExt;

            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .open(&self.shm_file)?;
            let mut mmap = unsafe { MmapOptions::new().map_mut(&file)? };

            mmap.as_mut()[DATA_FLAG_OFFSET] = 2; // Flag 2 = server response
            mmap.as_mut()[LENGTH_OFFSET..LENGTH_OFFSET + 4]
                .copy_from_slice(&(response_bytes.len() as u32).to_ne_bytes());
            mmap.as_mut()[DATA_OFFSET..DATA_OFFSET + response_bytes.len()]
                .copy_from_slice(&response_bytes);

            mmap.flush()?;
            Ok(())
        }

        #[cfg(windows)]
        {
            let mapping = unsafe {
                OpenFileMappingA(FILE_MAP_ALL_ACCESS, 0, self.shm_name.as_ptr() as *const i8)
            };

            if mapping.is_null() {
                return Err(CommunicationError::IoError(
                    "Failed to open file mapping".to_string(),
                ));
            }

            let view = unsafe { MapViewOfFile(mapping, FILE_MAP_ALL_ACCESS, 0, 0, SHM_SIZE) };

            if view.is_null() {
                unsafe {
                    CloseHandle(mapping as *mut c_void);
                }
                return Err(CommunicationError::IoError(
                    "Failed to map view".to_string(),
                ));
            }

            let view_slice: &mut [u8] =
                unsafe { std::slice::from_raw_parts_mut(view as *mut u8, SHM_SIZE) };
            view_slice[DATA_FLAG_OFFSET] = 2; // Flag 2 = server response
            view_slice[LENGTH_OFFSET..LENGTH_OFFSET + 4]
                .copy_from_slice(&(response_bytes.len() as u32).to_ne_bytes());
            view_slice[DATA_OFFSET..DATA_OFFSET + response_bytes.len()]
                .copy_from_slice(&response_bytes);

            unsafe {
                UnmapViewOfFile(view);
            }
            unsafe {
                CloseHandle(mapping as *mut c_void);
            }
            Ok(())
        }
    }

    async fn send_broadcast(&self, message: &CommunicationMessage) -> Result<(), CommunicationError> {
        let format = self.config.serialization_format;

        let message_bytes = match format {
            SerializationFormat::Json => {
                let s = serde_json::to_string(message)?;
                s.into_bytes()
            }
            SerializationFormat::MsgPack => rmp_serde::to_vec(message)
                .map_err(|e| CommunicationError::SerializationFailed(e.to_string()))?,
        };

        if message_bytes.len() >= SHM_SIZE - DATA_OFFSET {
            return Err(CommunicationError::SerializationFailed(
                "Broadcast message too large".to_string(),
            ));
        }

        #[cfg(unix)]
        {
            use std::fs::OpenOptions;
            use std::os::unix::fs::FileExt;

            let broadcast_file = format!("{}.broadcast", self.shm_file);
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .append(true)
                .open(&broadcast_file)?;

            // Write length prefix and message
            let len_bytes = (message_bytes.len() as u32).to_ne_bytes();
            file.write_all_at(&len_bytes, 0)?;
            file.write_all_at(&message_bytes, 4)?;
            file.sync_all()?;
            Ok(())
        }

        #[cfg(windows)]
        {
            // On Windows, use a separate broadcast event and shared memory
            // For simplicity, we'll use the same shared memory with flag 3
            let mapping = unsafe {
                OpenFileMappingA(FILE_MAP_ALL_ACCESS, 0, self.shm_name.as_ptr() as *const i8)
            };

            if mapping.is_null() {
                return Err(CommunicationError::IoError(
                    "Failed to open file mapping".to_string(),
                ));
            }

            let view = unsafe { MapViewOfFile(mapping, FILE_MAP_ALL_ACCESS, 0, 0, SHM_SIZE) };

            if view.is_null() {
                unsafe {
                    CloseHandle(mapping as *mut c_void);
                }
                return Err(CommunicationError::IoError(
                    "Failed to map view".to_string(),
                ));
            }

            let view_slice: &mut [u8] =
                unsafe { std::slice::from_raw_parts_mut(view as *mut u8, SHM_SIZE) };
            view_slice[DATA_FLAG_OFFSET] = 3; // Flag 3 = broadcast
            view_slice[LENGTH_OFFSET..LENGTH_OFFSET + 4]
                .copy_from_slice(&(message_bytes.len() as u32).to_ne_bytes());
            view_slice[DATA_OFFSET..DATA_OFFSET + message_bytes.len()]
                .copy_from_slice(&message_bytes);

            unsafe {
                UnmapViewOfFile(view);
            }
            unsafe {
                CloseHandle(mapping as *mut c_void);
            }
            Ok(())
        }
    }
}

#[async_trait::async_trait]
impl CommunicationServer for SharedMemoryServer {
    async fn start(&mut self) -> Result<(), CommunicationError> {
        *self.is_running.lock().await = true;

        let is_running = self.is_running.clone();
        let config = self.config.clone();
        let shm_name = self.shm_name.clone();
        #[cfg(unix)]
        let shm_file = self.shm_file.clone();
        let message_handler = self.message_handler.clone();
        #[cfg(windows)]
        let mapping_handle = self.mapping_handle;
        let broadcast_tx = self.broadcast_tx.clone();

        tokio::spawn(async move {
            while *is_running.lock().await {
                #[cfg(unix)]
                {
                    let server_config = config.clone();
                    let server = Self {
                        config: server_config,
                        shm_file: shm_file.clone(),
                        shm_name: shm_name.clone(),
                        is_running: is_running.clone(),
                        message_handler: message_handler.clone(),
                        mapping_handle: 0,
                        broadcast_tx: broadcast_tx.clone(),
                    };
                    match server.wait_for_message().await {
                        Ok(message) => {
                            ipc_log!("Server received message type: {}", message.message_type);

                            let response = if let Some(ref handler) = *message_handler.lock().await
                            {
                                handler(message.clone())
                            } else {
                                None
                            };

                            let response = response.unwrap_or_else(|| {
                                message.create_reply(serde_json::json!("Received"))
                            });

                            // Send response back to client
                            let _ = server.send_response(&response).await;

                            // Broadcast to all clients (excluding sender)
                            let _ = server.broadcast(message).await;
                        }
                        Err(e) => {
                            if *is_running.lock().await {
                                ipc_log!("Error handling message: {}", e);
                            }
                        }
                    }
                }

                #[cfg(windows)]
                {
                    let server_config = config.clone();
                    let server = Self {
                        config: server_config,
                        shm_name: shm_name.clone(),
                        is_running: is_running.clone(),
                        message_handler: message_handler.clone(),
                        mapping_handle,
                        broadcast_tx: broadcast_tx.clone(),
                    };
                    match server.wait_for_message().await {
                        Ok(message) => {
                            ipc_log!("Server received message type: {}", message.message_type);

                            let response = if let Some(ref handler) = *message_handler.lock().await
                            {
                                handler(message.clone())
                            } else {
                                None
                            };

                            let response = response.unwrap_or_else(|| {
                                message.create_reply(serde_json::json!("Received"))
                            });

                            // Send response back to client
                            let _ = server.send_response(&response).await;

                            // Broadcast to all clients
                            let _ = server.broadcast(message).await;
                        }
                        Err(e) => {
                            if *is_running.lock().await {
                                ipc_log!("Error handling message: {}", e);
                            }
                        }
                    }
                }

                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        });

        Ok(())
    }

    async fn stop(&mut self) -> Result<(), CommunicationError> {
        *self.is_running.lock().await = false;
        #[cfg(windows)]
        {
            if self.mapping_handle != 0 {
                unsafe {
                    CloseHandle(self.mapping_handle as *mut c_void);
                }
            }
        }
        Ok(())
    }

    fn is_running(&self) -> bool {
        match self.is_running.try_lock() {
            Ok(guard) => *guard,
            Err(_) => true,
        }
    }

    fn endpoint(&self) -> String {
        format!("shm://{}", self.shm_name)
    }

    fn set_message_handler(&self, handler: MessageHandler) -> Result<(), CommunicationError> {
        let message_handler = self.message_handler.clone();
        tokio::spawn(async move {
            *message_handler.lock().await = Some(handler);
        });
        Ok(())
    }

    async fn broadcast(&self, message: CommunicationMessage) -> Result<(), CommunicationError> {
        let _ = self.broadcast_tx.send(message.clone());
        self.send_broadcast(&message).await
    }
}

/// Shared memory client implementation
#[allow(dead_code)]
#[derive(Debug)]
pub struct SharedMemoryClient {
    config: CommunicationConfig,
    shm_name: String,
    connected: bool,
    #[cfg(unix)]
    shm_file: String,
    #[cfg(windows)]
    shm_file: String,
    #[cfg(windows)]
    mapping_handle: isize,
    seen_message_ids: std::sync::Mutex<std::collections::HashSet<String>>,
    last_broadcast_position: std::sync::atomic::AtomicU64,
    broadcast_rx: Arc<Mutex<tokio::sync::broadcast::Receiver<CommunicationMessage>>>,
}

impl SharedMemoryClient {
    pub fn new(config: &CommunicationConfig) -> Result<Self, CommunicationError> {
        let shm_name = Self::get_shm_name(&config.identifier);

        #[cfg(unix)]
        {
            let shm_file = Self::get_shm_file(&config.identifier);
            Ok(Self {
                config: config.clone(),
                shm_file,
                shm_name,
                connected: false,
                seen_message_ids: std::sync::Mutex::new(std::collections::HashSet::new()),
                last_broadcast_position: std::sync::atomic::AtomicU64::new(0),
                broadcast_rx: Arc::new(Mutex::new(tokio::sync::broadcast::channel(100).1)),
            })
        }

        #[cfg(windows)]
        {
            let shm_file = Self::get_shm_file(&config.identifier);
            Ok(Self {
                config: config.clone(),
                shm_name,
                shm_file,
                connected: false,
                mapping_handle: 0,
                seen_message_ids: std::sync::Mutex::new(std::collections::HashSet::new()),
                last_broadcast_position: std::sync::atomic::AtomicU64::new(0),
                broadcast_rx: Arc::new(Mutex::new(tokio::sync::broadcast::channel(100).1)),
            })
        }
    }

    #[cfg(unix)]
    fn get_shm_file(identifier: &str) -> String {
        super::get_temp_path(identifier, "shm")
    }

    #[cfg(windows)]
    fn get_shm_name(identifier: &str) -> String {
        format!("Local\\IPC_LIB_SHM_{}", identifier)
    }

    #[cfg(windows)]
    fn get_shm_file(identifier: &str) -> String {
        super::get_temp_path(identifier, "shm")
    }

    #[cfg(unix)]
    fn get_shm_name(identifier: &str) -> String {
        super::get_temp_path(identifier, "shm")
    }

    /// Check if server is still running by attempting to open the mapping
    async fn check_server_alive(&self) -> bool {
        #[cfg(unix)]
        {
            Path::new(&self.shm_file).exists()
        }

        #[cfg(windows)]
        {
            let mapping = unsafe {
                OpenFileMappingA(FILE_MAP_ALL_ACCESS, 0, self.shm_name.as_ptr() as *const i8)
            };
            if !mapping.is_null() {
                unsafe {
                    CloseHandle(mapping as *mut c_void);
                }
                true
            } else {
                false
            }
        }
    }

    async fn wait_for_response(&self) -> Result<CommunicationMessage, CommunicationError> {
        let timeout_duration = Duration::from_millis(self.config.timeout_ms);
        let start_time = std::time::Instant::now();
        let format = self.config.serialization_format;
        let shm_file = self.shm_file.clone();

        loop {
            // First check if server is still alive
            if !self.check_server_alive().await {
                return Err(CommunicationError::ConnectionFailed(
                    "Server not running".to_string(),
                ));
            }

            // Check for broadcasts from the broadcast file (Unix)
            #[cfg(unix)]
            {
                let broadcast_file = format!("{}.broadcast", shm_file);
                if Path::new(&broadcast_file).exists() {
                    use std::fs::OpenOptions;
                    use std::os::unix::fs::FileExt;

                    if let Ok(file) = OpenOptions::new().read(true).write(true).open(&broadcast_file) {
                        let metadata = file.metadata().ok().map(|m| m.len()).unwrap_or(0);
                        let last_pos = self.last_broadcast_position.load(std::sync::atomic::Ordering::SeqCst);
                        
                        if metadata > last_pos && metadata >= 4 {
                            // Read from last position
                            if last_pos < metadata.saturating_sub(4) {
                                let mut len_bytes = [0u8; 4];
                                if file.read_exact_at(&mut len_bytes, last_pos).is_ok() {
                                    let len = u32::from_ne_bytes(len_bytes) as usize;
                                    let msg_start = last_pos + 4;
                                    
                                    if msg_start + len <= metadata as usize {
                                        let mut msg_bytes = vec![0u8; len];
                                        if file.read_exact_at(&mut msg_bytes, msg_start).is_ok() {
                                            let message_res = match format {
                                                SerializationFormat::Json => {
                                                    let message_str = String::from_utf8_lossy(&msg_bytes);
                                                    serde_json::from_str::<CommunicationMessage>(&message_str)
                                                        .map_err(|e| CommunicationError::DeserializationFailed(e.to_string()))
                                                }
                                                SerializationFormat::MsgPack => {
                                                    rmp_serde::from_slice::<CommunicationMessage>(&msg_bytes)
                                                        .map_err(|e| CommunicationError::DeserializationFailed(e.to_string()))
                                                }
                                            };
                                            
                                            if let Ok(msg) = message_res {
                                                // Check if already seen
                                                let id = msg.id.clone();
                                                let already_seen = {
                                                    let seen = self.seen_message_ids.lock().unwrap();
                                                    seen.contains(&id)
                                                };
                                                
                                                if !already_seen {
                                                    self.seen_message_ids.lock().unwrap().insert(id);
                                                    self.last_broadcast_position.store(msg_start + len, std::sync::atomic::Ordering::SeqCst);
                                                    return Ok(msg);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Also check the broadcast channel (for in-process broadcasts)
            let broadcast_rx = self.broadcast_rx.lock().await;
            if let Ok(msg) = broadcast_rx.try_recv() {
                let id = msg.id.clone();
                let already_seen = {
                    let seen = self.seen_message_ids.lock().unwrap();
                    seen.contains(&id)
                };
                
                if !already_seen {
                    self.seen_message_ids.lock().unwrap().insert(id);
                    return Ok(msg);
                }
            }

            #[cfg(unix)]
            {
                if !Path::new(&shm_file).exists() {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    if start_time.elapsed() >= timeout_duration {
                        return Err(CommunicationError::Timeout(
                            "No response received".to_string(),
                        ));
                    }
                    continue;
                }

                let result = tokio::task::spawn_blocking(move || {
                    use memmap2::MmapOptions;
                    use std::fs::OpenOptions;

                    let file = OpenOptions::new().read(true).write(true).open(&shm_file)?;
                    let mut mmap = unsafe { MmapOptions::new().map_mut(&file)? };
                    let data = mmap.as_ref();
                    let data_flag = data[DATA_FLAG_OFFSET];

                    // Check for flag 2 (response) or flag 3 (broadcast)
                    if data_flag == 2 || data_flag == 3 {
                        let len = u32::from_ne_bytes([
                            data[LENGTH_OFFSET],
                            data[LENGTH_OFFSET + 1],
                            data[LENGTH_OFFSET + 2],
                            data[LENGTH_OFFSET + 3],
                        ]) as usize;

                        if len > 0 && len < SHM_SIZE - DATA_OFFSET {
                            let message_bytes = data[DATA_OFFSET..DATA_OFFSET + len].to_vec();
                            // Clear the flag after reading
                            mmap.as_mut()[DATA_FLAG_OFFSET] = 0;
                            mmap.flush()?;

                            let message_res = match format {
                                SerializationFormat::Json => {
                                    let message_str = String::from_utf8_lossy(&message_bytes);
                                    serde_json::from_str::<CommunicationMessage>(&message_str)
                                        .map_err(|e| CommunicationError::DeserializationFailed(e.to_string()))
                                }
                                SerializationFormat::MsgPack => {
                                    rmp_serde::from_slice::<CommunicationMessage>(&message_bytes)
                                        .map_err(|e| CommunicationError::DeserializationFailed(e.to_string()))
                                }
                            };
                            return Ok(Some(message_res?));
                        }
                    }
                    Ok::<Option<CommunicationMessage>, CommunicationError>(None)
                })
                .await;

                match result {
                    Ok(Ok(Some(message))) => {
                        // Check if already seen (for broadcasts)
                        let id = message.id.clone();
                        let already_seen = {
                            let seen = self.seen_message_ids.lock().unwrap();
                            seen.contains(&id)
                        };
                        
                        if !already_seen {
                            self.seen_message_ids.lock().unwrap().insert(id);
                            return Ok(message);
                        }
                    }
                    Ok(Ok(None)) => {}
                    Ok(Err(e)) => return Err(e),
                    Err(_) => {
                        return Err(CommunicationError::IoError(
                            "Spawn blocking failed".to_string(),
                        ))
                    }
                }
            }

            #[cfg(windows)]
            {
                let mapping_ptr = if self.mapping_handle != 0 {
                    self.mapping_handle as *mut c_void
                } else {
                    let mapping = unsafe {
                        OpenFileMappingA(
                            FILE_MAP_ALL_ACCESS,
                            0,
                            self.shm_name.as_ptr() as *const i8,
                        )
                    };
                    if mapping.is_null() {
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        if start_time.elapsed() >= timeout_duration {
                            return Err(CommunicationError::Timeout(
                                "No response received".to_string(),
                            ));
                        }
                        continue;
                    }
                    mapping as *mut c_void
                };

                let view =
                    unsafe { MapViewOfFile(mapping_ptr, FILE_MAP_ALL_ACCESS, 0, 0, SHM_SIZE) };

                if view.is_null() {
                    if self.mapping_handle == 0 {
                        unsafe {
                            CloseHandle(mapping_ptr as *mut c_void);
                        }
                    }
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    continue;
                }

                let data: &[u8; SHM_SIZE] = unsafe { &*(view as *const [u8; SHM_SIZE]) };
                let data_flag = data[DATA_FLAG_OFFSET];

                // Check for flag 2 (response) or flag 3 (broadcast)
                if data_flag == 2 || data_flag == 3 {
                    let len = u32::from_ne_bytes([
                        data[LENGTH_OFFSET],
                        data[LENGTH_OFFSET + 1],
                        data[LENGTH_OFFSET + 2],
                        data[LENGTH_OFFSET + 3],
                    ]) as usize;

                    if len > 0 && len < SHM_SIZE - DATA_OFFSET {
                        let message_bytes = data[DATA_OFFSET..DATA_OFFSET + len].to_vec();

                        unsafe {
                            *(view as *mut u8) = 0;
                        }

                        unsafe {
                            UnmapViewOfFile(view);
                        }
                        if self.mapping_handle == 0 {
                            unsafe {
                                CloseHandle(mapping_ptr as *mut c_void);
                            }
                        }

                        let message_res = match format {
                            SerializationFormat::Json => {
                                let message_str = String::from_utf8_lossy(&message_bytes);
                                serde_json::from_str::<CommunicationMessage>(&message_str).map_err(
                                    |e| CommunicationError::DeserializationFailed(e.to_string()),
                                )
                            }
                            SerializationFormat::MsgPack => {
                                rmp_serde::from_slice::<CommunicationMessage>(&message_bytes)
                                    .map_err(|e| CommunicationError::DeserializationFailed(e.to_string()))
                            }
                        };

                        match message_res {
                            Ok(msg) => {
                                // Check if already seen (for broadcasts)
                                let id = msg.id.clone();
                                let already_seen = {
                                    let seen = self.seen_message_ids.lock().unwrap();
                                    seen.contains(&id)
                                };
                                
                                if !already_seen {
                                    self.seen_message_ids.lock().unwrap().insert(id);
                                    return Ok(msg);
                                }
                            }
                            Err(e) => return Err(e),
                        }
                    }
                }

                unsafe {
                    UnmapViewOfFile(view);
                }
                if self.mapping_handle == 0 {
                    unsafe {
                        CloseHandle(mapping_ptr as *mut c_void);
                    }
                }
            }

            if start_time.elapsed() >= timeout_duration {
                return Err(CommunicationError::Timeout(
                    "No response received".to_string(),
                ));
            }

            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

#[async_trait::async_trait]
impl CommunicationClient for SharedMemoryClient {
    async fn connect(&mut self) -> Result<(), CommunicationError> {
        #[cfg(unix)]
        {
            if !Path::new(&self.shm_file).exists() {
                return Err(CommunicationError::ConnectionFailed(
                    "Server not running".to_string(),
                ));
            }
        }

        #[cfg(windows)]
        {
            let mapping = unsafe {
                OpenFileMappingA(FILE_MAP_ALL_ACCESS, 0, self.shm_name.as_ptr() as *const i8)
            };

            if mapping.is_null() {
                return Err(CommunicationError::ConnectionFailed(
                    "Server not running".to_string(),
                ));
            }

            self.mapping_handle = mapping as isize;
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

        let format = self.config.serialization_format;

        let message_bytes = match format {
            SerializationFormat::Json => {
                let s = serde_json::to_string(message)?;
                s.into_bytes()
            }
            SerializationFormat::MsgPack => rmp_serde::to_vec(message)
                .map_err(|e| CommunicationError::SerializationFailed(e.to_string()))?,
        };

        if message_bytes.len() >= SHM_SIZE - DATA_OFFSET {
            return Err(CommunicationError::SerializationFailed(
                "Message too large".to_string(),
            ));
        }

        #[cfg(unix)]
        {
            use std::fs::OpenOptions;

            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .open(&self.shm_file)?;
            let mut mmap = unsafe { MmapOptions::new().map_mut(&file)? };

            mmap.as_mut()[DATA_FLAG_OFFSET] = 1;
            mmap.as_mut()[LENGTH_OFFSET..LENGTH_OFFSET + 4]
                .copy_from_slice(&(message_bytes.len() as u32).to_ne_bytes());
            mmap.as_mut()[DATA_OFFSET..DATA_OFFSET + message_bytes.len()]
                .copy_from_slice(&message_bytes);

            mmap.flush()?;
            Ok(())
        }

        #[cfg(windows)]
        {
            let mapping = if self.mapping_handle != 0 {
                self.mapping_handle as *mut c_void
            } else {
                let mapping = unsafe {
                    OpenFileMappingA(FILE_MAP_ALL_ACCESS, 0, self.shm_name.as_ptr() as *const i8)
                };
                if mapping.is_null() {
                    return Err(CommunicationError::ConnectionFailed(
                        "Server not running".to_string(),
                    ));
                }
                mapping
            };

            let view = unsafe { MapViewOfFile(mapping, FILE_MAP_ALL_ACCESS, 0, 0, SHM_SIZE) };

            if view.is_null() {
                if self.mapping_handle == 0 {
                    unsafe {
                        CloseHandle(mapping as *mut c_void);
                    }
                }
                return Err(CommunicationError::IoError(
                    "Failed to map view".to_string(),
                ));
            }

            let view_slice: &mut [u8] =
                unsafe { std::slice::from_raw_parts_mut(view as *mut u8, SHM_SIZE) };
            view_slice[DATA_FLAG_OFFSET] = 1;
            view_slice[LENGTH_OFFSET..LENGTH_OFFSET + 4]
                .copy_from_slice(&(message_bytes.len() as u32).to_ne_bytes());
            view_slice[DATA_OFFSET..DATA_OFFSET + message_bytes.len()]
                .copy_from_slice(&message_bytes);

            unsafe {
                UnmapViewOfFile(view);
            }

            if self.mapping_handle == 0 {
                unsafe {
                    CloseHandle(mapping as *mut c_void);
                }
            }
            Ok(())
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
        #[cfg(windows)]
        {
            if self.mapping_handle != 0 {
                unsafe {
                    CloseHandle(self.mapping_handle as *mut c_void);
                }
                self.mapping_handle = 0;
            }
        }
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.connected
    }
}
