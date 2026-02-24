//! Single-instance enforcement logic.
//!
//! The [`SingleInstanceApp`] struct is the primary entry-point for ensuring that
//! only one copy of an application runs at a time. It tries the configured
//! communication protocol first, falls back to alternative protocols when
//! enabled, and handles stale lock/PID files gracefully.

use std::sync::Arc;
use tokio::time::{timeout, Duration};

use crate::communication::{
    get_temp_path, CommunicationConfig, CommunicationError, CommunicationFactory,
    CommunicationMessage, MessageHandler, ProtocolType,
};

/// Enhanced single instance enforcement with multiple communication protocols.
pub struct SingleInstanceApp {
    pub(crate) identifier: String,
    pub(crate) config: CommunicationConfig,
    pub(crate) server: Option<Box<dyn crate::communication::CommunicationServer>>,
    pub(crate) is_primary: bool,
    pub(crate) message_handler: Option<MessageHandler>,
}

impl SingleInstanceApp {
    /// Create a new single instance application with the given identifier.
    pub fn new(identifier: &str) -> Self {
        Self {
            identifier: identifier.to_string(),
            config: CommunicationConfig {
                identifier: identifier.to_string(),
                ..Default::default()
            },
            server: None,
            is_primary: false,
            message_handler: None,
        }
    }

    /// Set a handler for incoming messages (primary instance only).
    pub fn on_message<F>(mut self, handler: F) -> Self
    where
        F: Fn(CommunicationMessage) -> Option<CommunicationMessage> + Send + Sync + 'static,
    {
        self.message_handler = Some(Arc::new(handler));
        self
    }

    /// Configure the communication protocol.
    pub fn with_protocol(mut self, protocol: ProtocolType) -> Self {
        self.config.protocol = protocol;
        self
    }

    /// Configure the serialization format.
    pub fn with_serialization_format(
        mut self,
        format: crate::communication::SerializationFormat,
    ) -> Self {
        self.config.serialization_format = format;
        self
    }

    /// Configure the timeout (milliseconds) for communication operations.
    pub fn with_timeout(mut self, timeout_ms: u64) -> Self {
        self.config.timeout_ms = timeout_ms;
        self
    }

    /// Disable fallback protocols.
    pub fn without_fallback(mut self) -> Self {
        self.config.enable_fallback = false;
        self
    }

    /// Set custom fallback protocols.
    pub fn with_fallback_protocols(mut self, protocols: Vec<ProtocolType>) -> Self {
        self.config.fallback_protocols = protocols;
        self
    }

    /// Return the current communication endpoint, if the server has started.
    pub fn endpoint(&self) -> Option<String> {
        self.server.as_ref().map(|s| s.endpoint())
    }

    /// Return a reference to the current configuration.
    pub fn config(&self) -> &CommunicationConfig {
        &self.config
    }

    /// Enforce single instance and start the application.
    ///
    /// Returns `Ok(true)` when this process becomes the primary instance,
    /// `Ok(false)` when another instance is already running, and
    /// `Err(…)` when enforcement failed on all protocols.
    pub async fn enforce_single_instance(&mut self) -> Result<bool, CommunicationError> {
        let initial_protocol = self.config.protocol;

        crate::ipc_log!(
            "Attempting single instance enforcement for '{}' (primary protocol: {:?})",
            self.identifier,
            initial_protocol
        );

        // 1. Fast path – try to connect to an existing server.
        match self.connect_to_primary().await {
            Ok(response) => {
                crate::ipc_log!(
                    "Connected to existing primary instance. Response: {}",
                    response
                );
                return Ok(false);
            }
            Err(e) => {
                crate::ipc_log!(
                    "No existing instance responded on {:?}: {}. Checking for stale resources...",
                    initial_protocol,
                    e
                );
                self.cleanup_stale_resources(initial_protocol).await;
            }
        }

        // 2. Try to start the server with the primary protocol.
        println!(
            "📡 Starting new primary instance on {:?}...",
            initial_protocol
        );
        match self.start_server().await {
            Ok(_) => {
                println!("🏠 Successfully started as host!");
                let _ = self.write_pid_to_lock();
                self.is_primary = true;
                Ok(true)
            }
            Err(e) => self.handle_server_start_failure(e, initial_protocol).await,
        }
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    /// Start the communication server.
    pub(crate) async fn start_server(&mut self) -> Result<(), CommunicationError> {
        let protocol = CommunicationFactory::create_protocol(self.config.protocol)?;
        let mut server = protocol.create_server(&self.config).await?;

        if let Some(ref handler) = self.message_handler {
            server.set_message_handler(handler.clone())?;
        }

        server.start().await?;
        self.server = Some(server);
        Ok(())
    }

    /// Broadcast a message to all connected clients (primary instance only).
    pub async fn broadcast(&self, message: CommunicationMessage) -> Result<(), CommunicationError> {
        if let Some(ref server) = self.server {
            server.broadcast(message).await
        } else {
            Err(CommunicationError::ConnectionFailed(
                "Server not started".to_string(),
            ))
        }
    }

    /// Connect to the primary instance and perform the opening handshake.
    pub(crate) async fn connect_to_primary(&self) -> Result<String, CommunicationError> {
        let mut config = self.config.clone();
        config.timeout_ms = 2000;

        let protocol = CommunicationFactory::create_protocol(config.protocol)?;
        let mut client = protocol.create_client(&config).await?;

        println!(
            "🔗 Attempting to connect to existing session ({:?})...",
            config.protocol
        );

        let handshake = async {
            client.connect().await?;
            let args = std::env::args().collect();
            let message = CommunicationMessage::command_line_args(args);
            client.send_message(&message).await?;
            let resp = client.receive_message().await?;
            client.disconnect().await?;
            Ok::<CommunicationMessage, CommunicationError>(resp)
        };

        match timeout(Duration::from_millis(config.timeout_ms), handshake).await {
            Ok(Ok(response)) => {
                println!("✅ Connected to existing session!");
                match response.message_type.as_str() {
                    "response" => Ok(response.payload.as_str().unwrap_or("Received").to_string()),
                    _ => Ok("Connected".to_string()),
                }
            }
            Ok(Err(e)) => {
                println!("❌ No existing session found: {}", e);
                Err(e)
            }
            Err(_) => {
                println!("⏳ Connection handshake timed out");
                Err(CommunicationError::Timeout(
                    "Handshake timed out".to_string(),
                ))
            }
        }
    }

    /// Handle the case where the initial server start failed, including lock-file
    /// stale-resource cleanup and fallback protocol attempts.
    async fn handle_server_start_failure(
        &mut self,
        e: CommunicationError,
        initial_protocol: ProtocolType,
    ) -> Result<bool, CommunicationError> {
        let is_lock_exists = matches!(&e, CommunicationError::ConnectionFailed(msg)
            if msg.contains("Server lock file already exists") || msg.contains("AlreadyExists"));

        if is_lock_exists {
            if let Some(result) = self.try_reclaim_after_lock_exists(initial_protocol).await {
                return result;
            }
        }

        println!("⚠️ Failed to start server: {}.", e);
        self.try_fallback_protocols(initial_protocol, e).await
    }

    /// Try to either connect to the legitimate owner or clean up and reclaim.
    /// Returns `Some(result)` when a definitive outcome was reached.
    async fn try_reclaim_after_lock_exists(
        &mut self,
        initial_protocol: ProtocolType,
    ) -> Option<Result<bool, CommunicationError>> {
        println!("🔍 Lock file exists, checking if another process is the legitimate owner...");
        tokio::time::sleep(Duration::from_millis(50)).await;

        if self.connect_to_primary().await.is_ok() {
            println!("✅ Connected to existing legitimate instance");
            return Some(Ok(false));
        }

        println!("⚠️ Lock file exists but no server responding. Checking PID...");
        let pid_file = get_temp_path(&self.identifier, "pid");

        if std::path::Path::new(&pid_file).exists() {
            if let Ok(pid_str) = std::fs::read_to_string(&pid_file) {
                if let Ok(pid) = pid_str.trim().parse::<u32>() {
                    if self.is_process_running(pid) {
                        println!("🕐 PID {} is alive but not responding, waiting...", pid);
                        tokio::time::sleep(Duration::from_millis(500)).await;

                        if self.connect_to_primary().await.is_ok() {
                            return Some(Ok(false));
                        }
                    }
                }
            }
        }

        println!("⚠️ No legitimate owner found. Cleaning up stale resources...");
        self.cleanup_stale_resources(initial_protocol).await;
        tokio::time::sleep(Duration::from_millis(100)).await;

        if self.connect_to_primary().await.is_ok() {
            return Some(Ok(false));
        }

        if self.start_server().await.is_ok() {
            println!("🏠 Successfully started as host after cleanup!");
            let _ = self.write_pid_to_lock();
            self.is_primary = true;
            return Some(Ok(true));
        }

        println!("⚠️ Could not start server even after cleanup");
        None
    }

    /// Iterate over fallback protocols when the primary one failed.
    async fn try_fallback_protocols(
        &mut self,
        initial_protocol: ProtocolType,
        original_error: CommunicationError,
    ) -> Result<bool, CommunicationError> {
        if self.config.enable_fallback {
            let fallback_protocols = self.config.fallback_protocols.clone();
            for protocol in fallback_protocols {
                if protocol == initial_protocol {
                    continue;
                }

                crate::ipc_log!("Trying fallback protocol: {:?}", protocol);
                self.config.protocol = protocol;

                if self.connect_to_primary().await.is_ok() {
                    crate::ipc_log!("Connected to existing instance via fallback {:?}", protocol);
                    return Ok(false);
                }

                if self.start_server().await.is_ok() {
                    crate::ipc_log!("Successfully started server via fallback {:?}", protocol);
                    let _ = self.write_pid_to_lock();
                    self.is_primary = true;
                    return Ok(true);
                }
            }
        }

        crate::ipc_log!("All protocols failed. Error: {}", original_error);
        Err(original_error)
    }

    /// Remove stale socket / lock / shared-memory files left by a dead process.
    async fn cleanup_stale_resources(&self, protocol: ProtocolType) {
        match protocol {
            ProtocolType::UnixSocket => {
                self.cleanup_unix_socket(protocol).await;
            }
            ProtocolType::FileBased => {
                self.cleanup_file_based().await;
            }
            ProtocolType::SharedMemory => {
                self.cleanup_shared_memory().await;
            }
            _ => {} // Other protocols have no stale files to clean up yet.
        }
    }

    async fn cleanup_unix_socket(&self, protocol: ProtocolType) {
        let socket_path = get_temp_path(&self.identifier, "sock");
        if !std::path::Path::new(&socket_path).exists() {
            return;
        }

        crate::ipc_log!("Unix socket exists, checking if server is responsive...");
        tokio::time::sleep(Duration::from_millis(50)).await;

        let mut config = self.config.clone();
        config.timeout_ms = 500;

        if let Ok(protocol_impl) = CommunicationFactory::create_protocol(protocol) {
            if let Ok(mut client) = protocol_impl.create_client(&config).await {
                if client.connect().await.is_ok() {
                    crate::ipc_log!("Server is responsive, skipping cleanup");
                    let _ = client.disconnect().await;
                    return;
                }
            }
        }

        let _ = std::fs::remove_file(&socket_path);
    }

    async fn cleanup_file_based(&self) {
        let lock_file = get_temp_path(&self.identifier, "lock");
        let pid_file = get_temp_path(&self.identifier, "pid");
        let message_file = get_temp_path(&self.identifier, "msg");

        if std::path::Path::new(&lock_file).exists() {
            tokio::time::sleep(Duration::from_millis(50)).await;

            if self.connect_to_primary().await.is_ok() {
                crate::ipc_log!("Server is responsive, skipping cleanup");
                return;
            }

            crate::ipc_log!("Server not responsive, checking PID file...");
        }

        if std::path::Path::new(&pid_file).exists() {
            if let Ok(pid_str) = std::fs::read_to_string(&pid_file) {
                if let Ok(pid) = pid_str.trim().parse::<u32>() {
                    if self.is_process_running(pid) {
                        crate::ipc_log!(
                            "PID {} is running, waiting for it to become responsive...",
                            pid
                        );
                        tokio::time::sleep(Duration::from_millis(200)).await;

                        if self.connect_to_primary().await.is_ok() {
                            crate::ipc_log!("Server became responsive");
                            return;
                        }

                        crate::ipc_log!("PID {} is alive but not responding, waiting more...", pid);
                        tokio::time::sleep(Duration::from_millis(500)).await;

                        if self.connect_to_primary().await.is_ok() {
                            crate::ipc_log!("Server became responsive after waiting");
                            return;
                        }

                        crate::ipc_log!(
                            "Process {} is alive but unresponsive for extended time",
                            pid
                        );
                    }
                }
            }
        }

        crate::ipc_log!("Cleaning up stale resources for {}...", self.identifier);
        let _ = std::fs::remove_file(&lock_file);
        let _ = std::fs::remove_file(&message_file);
        let _ = std::fs::remove_file(&pid_file);
        let response_file = format!("{}.response", message_file);
        let _ = std::fs::remove_file(response_file);
    }

    async fn cleanup_shared_memory(&self) {
        let shm_file = get_temp_path(&self.identifier, "shm");
        if std::path::Path::new(&shm_file).exists() {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let _ = std::fs::remove_file(&shm_file);
        }
    }

    /// Returns `true` if the process with the given PID is currently running.
    pub fn is_process_running(&self, pid: u32) -> bool {
        #[cfg(unix)]
        {
            // Signal 0 tests whether a process exists without sending a real signal.
            unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
        }

        #[cfg(windows)]
        {
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

    /// Write the current process PID to the lock/PID file.
    fn write_pid_to_lock(&self) -> Result<(), CommunicationError> {
        use std::fs::OpenOptions;
        use std::io::Write;

        let lock_file = get_temp_path(&self.identifier, "pid");
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&lock_file)
            .map_err(|e| CommunicationError::ConnectionFailed(e.to_string()))?;

        file.write_all(std::process::id().to_string().as_bytes())
            .map_err(|e| CommunicationError::ConnectionFailed(e.to_string()))
    }
}
