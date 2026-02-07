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
    #[cfg(unix)]
    shm_file: String,
    #[cfg(windows)]
    mapping_handle: usize,
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
                shm_name,
                is_running: Arc::new(Mutex::new(false)),
                message_handler: Arc::new(Mutex::new(None)),
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
                mapping_handle: mapping as usize,
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
            if !*self.is_running.lock().await {
                return Err(CommunicationError::ConnectionFailed(
                    "Server stopped".to_string(),
                ));
            }

            #[cfg(unix)]
            {
                // Check if shared memory file exists
                if !Path::new(&self.shm_file).exists() {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    if start_time.elapsed() >= timeout_duration {
                        return Err(CommunicationError::Timeout(
                            "No message received".to_string(),
                        ));
                    }
                    continue;
                }

                // Map the shared memory (blocking operation)
                let shm_file = self.shm_file.clone();
                let format = self.config.serialization_format;

                let result = tokio::task::spawn_blocking(move || {
                    use memmap2::MmapOptions;
                    use std::fs::OpenOptions;

                    let file = OpenOptions::new().read(true).write(true).open(&shm_file)?;
                    let mut mmap = unsafe { MmapOptions::new().map_mut(&file)? };

                    // Check if there's data in shared memory
                    let data = mmap.as_ref();
                    if data[0] != 0 {
                        // First byte indicates if there's data
                        // Read the message length (first 4 bytes)
                        let len = u32::from_ne_bytes([data[1], data[2], data[3], data[4]]) as usize;

                        if len > 0 && len < 4096 {
                            // Read the message data (Clone to vector to avoid borrow checker issues)
                            let message_bytes = data[5..5 + len].to_vec();

                            // Clear the shared memory flag immediately
                            mmap.as_mut()[0] = 0; // Clear data flag
                            mmap.flush()?;

                            // Parse the message based on format (using the owned vector)
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
                    Ok(None)
                })
                .await;

                match result {
                    Ok(Ok(Some(message))) => return Ok(message),
                    Ok(Ok(None)) => {
                        // No message yet, continue waiting
                    }
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
                // Check if mapping exists (other process has created it)
                let mapping = unsafe {
                    OpenFileMappingA(FILE_MAP_ALL_ACCESS, 0, self.shm_name.as_ptr() as *const i8)
                };

                if mapping.is_null() {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    if start_time.elapsed() >= timeout_duration {
                        return Err(CommunicationError::Timeout(
                            "No message received".to_string(),
                        ));
                    }
                    continue;
                }

                // Map the view
                let view = unsafe { MapViewOfFile(mapping, FILE_MAP_ALL_ACCESS, 0, 0, 4096) };

                if view.is_null() {
                    unsafe {
                        CloseHandle(mapping);
                    }
                    return Err(CommunicationError::IoError(
                        "Failed to map view".to_string(),
                    ));
                }

                let format = self.config.serialization_format;

                // Read data from shared memory
                let data: &[u8; 4096] = unsafe { &*(view as *const [u8; 4096]) };

                if data[0] != 0 {
                    let len = u32::from_ne_bytes([data[1], data[2], data[3], data[4]]) as usize;

                    if len > 0 && len < 4096 {
                        let message_bytes = data[5..5 + len].to_vec();

                        // Clear the shared memory flag
                        unsafe {
                            *(view as *mut u8) = 0;
                        }

                        // Parse the message
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

                        unsafe {
                            UnmapViewOfFile(view);
                        }
                        unsafe {
                            CloseHandle(mapping);
                        }

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
                    CloseHandle(mapping);
                }
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
        {
            let shm_file = self.shm_file.clone();
            let result = tokio::task::spawn_blocking(move || {
                use memmap2::MmapOptions;
                use std::fs::OpenOptions;

                let file = OpenOptions::new().read(true).write(true).open(&shm_file)?;
                let mut mmap = unsafe { MmapOptions::new().map_mut(&file)? };

                // Write the response to shared memory
                // Format: [data_flag(1)][length(4)][data(n)]
                mmap.as_mut()[0] = 1; // Set data flag
                mmap.as_mut()[1..5].copy_from_slice(&(response_bytes.len() as u32).to_ne_bytes());
                mmap.as_mut()[5..5 + response_bytes.len()].copy_from_slice(&response_bytes);

                mmap.flush()?;

                Ok::<(), CommunicationError>(())
            })
            .await;

            match result {
                Ok(inner) => inner,
                Err(_) => Err(CommunicationError::IoError(
                    "Spawn blocking failed".to_string(),
                )),
            }
        }

        #[cfg(windows)]
        {
            // Reopen the file mapping
            let mapping = unsafe {
                OpenFileMappingA(FILE_MAP_ALL_ACCESS, 0, self.shm_name.as_ptr() as *const i8)
            };

            if mapping.is_null() {
                return Err(CommunicationError::IoError(
                    "Failed to open file mapping".to_string(),
                ));
            }

            // Map the view
            let view = unsafe { MapViewOfFile(mapping, FILE_MAP_ALL_ACCESS, 0, 0, 4096) };

            if view.is_null() {
                unsafe {
                    CloseHandle(mapping);
                }
                return Err(CommunicationError::IoError(
                    "Failed to map view".to_string(),
                ));
            }

            // Write the response
            let view_slice: &mut [u8] =
                unsafe { std::slice::from_raw_parts_mut(view as *mut u8, 4096) };
            view_slice[0] = 1; // Set data flag
            view_slice[1..5].copy_from_slice(&(response_bytes.len() as u32).to_ne_bytes());
            view_slice[5..5 + response_bytes.len()].copy_from_slice(&response_bytes);

            unsafe {
                UnmapViewOfFile(view);
            }
            unsafe {
                CloseHandle(mapping);
            }
            Ok(())
        }
    }
}

#[async_trait::async_trait]
impl CommunicationServer for SharedMemoryServer {
    async fn start(&mut self) -> Result<(), CommunicationError> {
        *self.is_running.lock().await = true;

        // Spawn a task to handle incoming messages
        let is_running = self.is_running.clone();
        let config = self.config.clone();
        let shm_name = self.shm_name.clone();
        #[cfg(unix)]
        let shm_file = self.shm_file.clone();
        let message_handler = self.message_handler.clone();
        #[cfg(windows)]
        let mapping_handle = self.mapping_handle;

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
                    };
                    match server.wait_for_message().await {
                        Ok(message) => {
                            ipc_log!("Received message type: {}", message.message_type);

                            let response = if let Some(ref handler) = *message_handler.lock().await
                            {
                                handler(message.clone())
                            } else {
                                None
                            };

                            let response = response.unwrap_or_else(|| {
                                message.create_reply(serde_json::json!("Received"))
                            });

                            let _ = server.send_response(&response).await;
                        }
                        Err(e) => {
                            if *is_running.lock().await {
                                eprintln!("Error handling message: {}", e);
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
                    };
                    match server.wait_for_message().await {
                        Ok(message) => {
                            ipc_log!("Received message type: {}", message.message_type);

                            let response = if let Some(ref handler) = *message_handler.lock().await
                            {
                                handler(message.clone())
                            } else {
                                None
                            };

                            let response = response.unwrap_or_else(|| {
                                message.create_reply(serde_json::json!("Received"))
                            });

                            let _ = server.send_response(&response).await;
                        }
                        Err(e) => {
                            if *is_running.lock().await {
                                eprintln!("Error handling message: {}", e);
                            }
                        }
                    }
                }
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
    mapping_handle: usize,
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
            })
        }

        #[cfg(windows)]
        {
            // Pre-create the file mapping (this will be used by the server)
            let mapping = unsafe {
                CreateFileMappingA(
                    INVALID_HANDLE_VALUE,
                    std::ptr::null_mut(),
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
                connected: false,
                mapping_handle: mapping as usize,
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

    async fn wait_for_response(&self) -> Result<CommunicationMessage, CommunicationError> {
        let timeout_duration = Duration::from_millis(self.config.timeout_ms);
        let start_time = std::time::Instant::now();

        loop {
            #[cfg(unix)]
            {
                // Check if shared memory file exists
                if !Path::new(&self.shm_name).exists() {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    if start_time.elapsed() >= timeout_duration {
                        return Err(CommunicationError::Timeout(
                            "No response received".to_string(),
                        ));
                    }
                    continue;
                }

                // Map the shared memory (blocking operation)
                let shm_file = self.shm_name.clone();
                let format = self.config.serialization_format;

                let result = tokio::task::spawn_blocking(move || {
                    use memmap2::MmapOptions;
                    use std::fs::OpenOptions;

                    let file = OpenOptions::new().read(true).write(true).open(&shm_file)?;
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
                    Ok(None)
                })
                .await;

                match result {
                    Ok(Ok(Some(message))) => return Ok(message),
                    Ok(Ok(None)) => {
                        // No response yet, continue waiting
                    }
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
                // Reopen the file mapping (to read from server's data)
                let mapping = unsafe {
                    OpenFileMappingA(FILE_MAP_ALL_ACCESS, 0, self.shm_name.as_ptr() as *const i8)
                };

                if mapping.is_null() {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    if start_time.elapsed() >= timeout_duration {
                        return Err(CommunicationError::Timeout(
                            "No response received".to_string(),
                        ));
                    }
                    continue;
                }

                // Map the view
                let view = unsafe { MapViewOfFile(mapping, FILE_MAP_ALL_ACCESS, 0, 0, 4096) };

                if view.is_null() {
                    unsafe {
                        CloseHandle(mapping);
                    }
                    return Err(CommunicationError::IoError(
                        "Failed to map view".to_string(),
                    ));
                }

                let format = self.config.serialization_format;

                // Read data from shared memory
                let data: &[u8; 4096] = unsafe { &*(view as *const [u8; 4096]) };

                if data[0] != 0 {
                    let len = u32::from_ne_bytes([data[1], data[2], data[3], data[4]]) as usize;

                    if len > 0 && len < 4096 {
                        let message_bytes = data[5..5 + len].to_vec();

                        // Clear the shared memory flag
                        unsafe {
                            *(view as *mut u8) = 0;
                        }

                        // Parse the message
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

                        unsafe {
                            UnmapViewOfFile(view);
                        }
                        unsafe {
                            CloseHandle(mapping);
                        }

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
                    CloseHandle(mapping);
                }
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
        #[cfg(unix)]
        {
            // Check if shared memory file exists
            if !Path::new(&self.shm_name).exists() {
                return Err(CommunicationError::ConnectionFailed(
                    "Server not running".to_string(),
                ));
            }
        }

        #[cfg(windows)]
        {
            // Check if file mapping exists (server is running)
            let mapping = unsafe {
                OpenFileMappingA(FILE_MAP_ALL_ACCESS, 0, self.shm_name.as_ptr() as *const i8)
            };

            if mapping.is_null() {
                return Err(CommunicationError::ConnectionFailed(
                    "Server not running".to_string(),
                ));
            }

            unsafe {
                CloseHandle(mapping);
            }
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

        #[cfg(unix)]
        {
            let shm_file = self.shm_name.clone();
            let result = tokio::task::spawn_blocking(move || {
                use memmap2::MmapOptions;
                use std::fs::OpenOptions;

                let file = OpenOptions::new().read(true).write(true).open(&shm_file)?;
                let mut mmap = unsafe { MmapOptions::new().map_mut(&file)? };

                // Write the message to shared memory
                // Format: [data_flag(1)][length(4)][data(n)]
                mmap.as_mut()[0] = 1; // Set data flag
                mmap.as_mut()[1..5].copy_from_slice(&(message_bytes.len() as u32).to_ne_bytes());
                mmap.as_mut()[5..5 + message_bytes.len()].copy_from_slice(&message_bytes);

                mmap.flush()?;

                Ok::<(), CommunicationError>(())
            })
            .await;

            match result {
                Ok(inner) => inner,
                Err(_) => Err(CommunicationError::IoError(
                    "Spawn blocking failed".to_string(),
                )),
            }
        }

        #[cfg(windows)]
        {
            // Use the pre-created mapping handle
            let view = unsafe {
                MapViewOfFile(
                    self.mapping_handle as *mut c_void,
                    FILE_MAP_ALL_ACCESS,
                    0,
                    0,
                    4096,
                )
            };

            if view.is_null() {
                return Err(CommunicationError::IoError(
                    "Failed to map view".to_string(),
                ));
            }

            // Write the message
            let view_slice: &mut [u8] =
                unsafe { std::slice::from_raw_parts_mut(view as *mut u8, 4096) };
            view_slice[0] = 1; // Set data flag
            view_slice[1..5].copy_from_slice(&(message_bytes.len() as u32).to_ne_bytes());
            view_slice[5..5 + message_bytes.len()].copy_from_slice(&message_bytes);

            unsafe {
                UnmapViewOfFile(view);
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
            }
        }
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.connected
    }
}
