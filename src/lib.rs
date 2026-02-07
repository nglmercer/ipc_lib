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

use std::sync::atomic::{AtomicBool, Ordering};

static ENABLE_LOGGING: AtomicBool = AtomicBool::new(false);

/// Enable IPC library logging
pub fn enable_logging() {
    ENABLE_LOGGING.store(true, Ordering::Relaxed);
}

/// Disable IPC library logging
pub fn disable_logging() {
    ENABLE_LOGGING.store(false, Ordering::Relaxed);
}

/// Check if IPC library logging is enabled
pub fn is_logging_enabled() -> bool {
    ENABLE_LOGGING.load(Ordering::Relaxed)
}

#[macro_export]
macro_rules! ipc_log {
    ($($arg:tt)*) => {
        if $crate::is_logging_enabled() {
            eprintln!("[IPC] {}", format_args!($($arg)*));
        }
    };
}

use communication::{CommunicationError, CommunicationFactory, CommunicationMessage};
use std::sync::Arc;
use tokio::time::{timeout, Duration};

// Re-export commonly used types for simpler imports
pub use communication::current_timestamp;
pub use communication::CommunicationConfig;
pub use communication::ProtocolType;
pub use communication::SerializationFormat;

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
    message_handler: Option<communication::MessageHandler>,
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
            message_handler: None,
        }
    }

    /// Set a handler for incoming messages (Primary instance only)
    pub fn on_message<F>(mut self, handler: F) -> Self
    where
        F: Fn(CommunicationMessage) -> Option<CommunicationMessage> + Send + Sync + 'static,
    {
        self.message_handler = Some(Arc::new(handler));
        self
    }

    /// Configure the communication protocol
    pub fn with_protocol(mut self, protocol: ProtocolType) -> Self {
        self.config.protocol = protocol;
        self
    }

    /// Configure the serialization format
    pub fn with_serialization_format(mut self, format: SerializationFormat) -> Self {
        self.config.serialization_format = format;
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
        let initial_protocol = self.config.protocol;

        ipc_log!(
            "Attempting single instance enforcement for '{}' (primary protocol: {:?})",
            self.identifier,
            initial_protocol
        );

        // 1. Try to connect to existing server first (fast path)
        match self.connect_to_primary().await {
            Ok(response) => {
                ipc_log!(
                    "Connected to existing primary instance. Response: {}",
                    response
                );
                return Ok(false);
            }
            Err(e) => {
                ipc_log!(
                    "No existing instance responded on {:?}: {}. Checking for stale resources...",
                    initial_protocol,
                    e
                );
                // Aggressively clean up stale resources before trying to start server
                self.cleanup_stale_resources(initial_protocol).await;
            }
        }

        // 2. Try to start the server with primary protocol
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
            Err(e) => {
                // Check if the error is due to lock file already existing
                let is_lock_exists = matches!(&e, CommunicationError::ConnectionFailed(msg)
                    if msg.contains("Server lock file already exists") || msg.contains("AlreadyExists"));

                if is_lock_exists {
                    // Lock file exists - try to verify if it's a legitimate owner before cleaning
                    println!("🔍 Lock file exists, checking if another process is the legitimate owner...");

                    // Small sleep to give the legitimate process time to write its PID
                    tokio::time::sleep(Duration::from_millis(50)).await;

                    // Verify if another process is legitimately running
                    if self.connect_to_primary().await.is_ok() {
                        println!("✅ Connected to existing legitimate instance");
                        return Ok(false);
                    }

                    // Lock file exists but no server is responding - it might be stale or owned by a dead process
                    println!("⚠️ Lock file exists but no server responding. Checking PID...");

                    // Check PID file to see if the owning process is still alive
                    use communication::get_temp_path;
                    let pid_file = get_temp_path(&self.identifier, "pid");
                    if std::path::Path::new(&pid_file).exists() {
                        if let Ok(pid_str) = std::fs::read_to_string(&pid_file) {
                            if let Ok(pid) = pid_str.trim().parse::<u32>() {
                                if self.is_process_running(pid) {
                                    // Process is alive but not responding - wait longer
                                    println!(
                                        "🕐 PID {} is alive but not responding, waiting...",
                                        pid
                                    );
                                    tokio::time::sleep(Duration::from_millis(500)).await;

                                    // Try connecting one more time
                                    if self.connect_to_primary().await.is_ok() {
                                        return Ok(false);
                                    }
                                }
                            }
                        }
                    }

                    // No legitimate owner found, clean up and retry
                    println!("⚠️ No legitimate owner found. Cleaning up stale resources...");
                    self.cleanup_stale_resources(initial_protocol).await;

                    // Give the other process time to finish cleanup before we try
                    tokio::time::sleep(Duration::from_millis(100)).await;

                    // Final check - see if another process already claimed
                    if self.connect_to_primary().await.is_ok() {
                        return Ok(false);
                    }

                    // Try to start server again
                    if self.start_server().await.is_ok() {
                        println!("🏠 Successfully started as host after cleanup!");
                        let _ = self.write_pid_to_lock();
                        self.is_primary = true;
                        return Ok(true);
                    }

                    println!("⚠️ Could not start server even after cleanup");
                }

                println!("⚠️ Failed to start server: {}.", e);

                if self.config.enable_fallback {
                    let fallback_protocols = self.config.fallback_protocols.clone();
                    for protocol in fallback_protocols {
                        if protocol == initial_protocol {
                            continue;
                        }

                        ipc_log!("Trying fallback protocol: {:?}", protocol);
                        self.config.protocol = protocol;

                        // Check if we can connect via fallback first
                        if let Ok(_resp) = self.connect_to_primary().await {
                            ipc_log!("Connected to existing instance via fallback {:?}", protocol);
                            return Ok(false);
                        }

                        // Try to start server via fallback
                        if self.start_server().await.is_ok() {
                            ipc_log!("Successfully started server via fallback {:?}", protocol);
                            let _ = self.write_pid_to_lock();
                            self.is_primary = true;
                            return Ok(true);
                        }
                    }
                }

                ipc_log!("All protocols failed. Error: {}", e);
                Err(e)
            }
        }
    }

    /// Start the communication server
    async fn start_server(&mut self) -> Result<(), CommunicationError> {
        let protocol = CommunicationFactory::create_protocol(self.config.protocol)?;
        let mut server = protocol.create_server(&self.config).await?;

        if let Some(ref handler) = self.message_handler {
            server.set_message_handler(handler.clone())?;
        }

        server.start().await?;
        self.server = Some(server);
        Ok(())
    }

    /// Broadcast a message to all connected clients (Primary instance only)
    pub async fn broadcast(&self, message: CommunicationMessage) -> Result<(), CommunicationError> {
        if let Some(ref server) = self.server {
            server.broadcast(message).await
        } else {
            Err(CommunicationError::ConnectionFailed(
                "Server not started".to_string(),
            ))
        }
    }

    /// Connect to the primary instance
    async fn connect_to_primary(&self) -> Result<String, CommunicationError> {
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

    async fn cleanup_stale_resources(&self, protocol: ProtocolType) {
        use communication::get_temp_path;

        match protocol {
            ProtocolType::UnixSocket => {
                let socket_path = get_temp_path(&self.identifier, "sock");
                if std::path::Path::new(&socket_path).exists() {
                    // Try to verify if a server is actually running on this socket
                    // If we can't connect, it's stale
                    ipc_log!("Unix socket exists, checking if server is responsive...");

                    // Small delay to allow server to become responsive
                    tokio::time::sleep(Duration::from_millis(50)).await;

                    // Try to connect one more time before cleaning
                    let mut config = self.config.clone();
                    config.timeout_ms = 500;

                    if let Ok(protocol_impl) = CommunicationFactory::create_protocol(protocol) {
                        if let Ok(mut client) = protocol_impl.create_client(&config).await {
                            if client.connect().await.is_ok() {
                                ipc_log!("Server is responsive, skipping cleanup");
                                let _ = client.disconnect().await;
                                return;
                            }
                        }
                    }

                    // Server not responsive, clean up
                    let _ = std::fs::remove_file(&socket_path);
                }
            }
            ProtocolType::FileBased => {
                let lock_file = get_temp_path(&self.identifier, "lock");
                let pid_file = get_temp_path(&self.identifier, "pid");
                let message_file = get_temp_path(&self.identifier, "msg");

                // First, check if server is responsive via lock file
                if std::path::Path::new(&lock_file).exists() {
                    // Small delay to allow server to become responsive
                    tokio::time::sleep(Duration::from_millis(50)).await;

                    // Try to connect before cleaning
                    if self.connect_to_primary().await.is_ok() {
                        ipc_log!("Server is responsive, skipping cleanup");
                        return;
                    }

                    ipc_log!("Server not responsive, checking PID file...");
                }

                // Check if a PID file exists and if the process is still running
                if std::path::Path::new(&pid_file).exists() {
                    if let Ok(pid_str) = std::fs::read_to_string(&pid_file) {
                        if let Ok(pid) = pid_str.trim().parse::<u32>() {
                            if self.is_process_running(pid) {
                                // Process is still alive, don't delete lock file
                                // But check if it's actually responding
                                ipc_log!(
                                    "PID {} is running, waiting for it to become responsive...",
                                    pid
                                );
                                tokio::time::sleep(Duration::from_millis(200)).await;

                                // Try connecting one more time
                                if self.connect_to_primary().await.is_ok() {
                                    ipc_log!("Server became responsive");
                                    return;
                                }

                                // PID is alive but not responding - might be starting up
                                // Give it more time
                                ipc_log!(
                                    "PID {} is alive but not responding, waiting more...",
                                    pid
                                );
                                tokio::time::sleep(Duration::from_millis(500)).await;

                                if self.connect_to_primary().await.is_ok() {
                                    ipc_log!("Server became responsive after waiting");
                                    return;
                                }

                                // Process is alive but unresponsive for extended time
                                // This might be a zombie or stuck process
                                ipc_log!(
                                    "Process {} is alive but unresponsive for extended time",
                                    pid
                                );
                                // We'll still proceed with cleanup since it's unresponsive
                            }
                        }
                    }
                }

                // Either no PID file, process not alive, or process unresponsive
                // Clean up stale files
                ipc_log!("Cleaning up stale resources for {}...", self.identifier);
                let _ = std::fs::remove_file(&lock_file);
                let _ = std::fs::remove_file(&message_file);
                let _ = std::fs::remove_file(&pid_file);
                // Also clean up any response files
                let response_file = format!("{}.response", message_file);
                let _ = std::fs::remove_file(response_file);
            }
            ProtocolType::SharedMemory => {
                let shm_file = get_temp_path(&self.identifier, "shm");
                if std::path::Path::new(&shm_file).exists() {
                    // Try to verify if shared memory region is still in use
                    // If we can't access it, it's likely stale
                    tokio::time::sleep(Duration::from_millis(50)).await;

                    // For shared memory, we can't easily check responsiveness
                    // Just clean it up - it will be recreated if needed
                    let _ = std::fs::remove_file(&shm_file);
                }
            }
            _ => {} // Other protocols don't have stale files to clean up yet
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

    fn write_pid_to_lock(&self) -> Result<(), CommunicationError> {
        use communication::get_temp_path;
        let lock_file = get_temp_path(&self.identifier, "pid");
        use std::fs::OpenOptions;
        use std::io::Write;

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

use std::collections::HashMap;
use tokio::sync::{oneshot, Mutex as TokioMutex};

/// A broker that manages pending requests and their responses
pub struct RequestBroker {
    pending: Arc<TokioMutex<HashMap<String, oneshot::Sender<CommunicationMessage>>>>,
}

impl Default for RequestBroker {
    fn default() -> Self {
        Self::new()
    }
}

impl RequestBroker {
    pub fn new() -> Self {
        Self {
            pending: Arc::new(TokioMutex::new(HashMap::new())),
        }
    }

    /// Register a new request and return a receiver for the response
    pub async fn register_request(&self, id: String) -> oneshot::Receiver<CommunicationMessage> {
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);
        rx
    }

    /// Dispatch a received message if it's a response to a pending request
    /// Returns true if the message was handled as a response
    pub async fn dispatch_response(&self, message: CommunicationMessage) -> bool {
        if let Some(reply_to) = &message.reply_to {
            let mut pending = self.pending.lock().await;
            if let Some(tx) = pending.remove(reply_to) {
                let _ = tx.send(message);
                return true;
            }
        }
        false
    }
}

/// A wrapper around IPC communication that provides high-level messaging features
pub struct Messenger {
    session: Arc<IpcSession>,
    broker: Arc<RequestBroker>,
}

impl Messenger {
    pub fn new(session: IpcSession) -> Self {
        let messenger = Self {
            session: Arc::new(session),
            broker: Arc::new(RequestBroker::new()),
        };

        // Start background receiver task
        messenger.start_receiver();

        messenger
    }

    fn start_receiver(&self) {
        let session = self.session.clone();
        let broker = self.broker.clone();

        tokio::spawn(async move {
            loop {
                match session.receive().await {
                    Ok(msg) => {
                        if !broker.dispatch_response(msg.clone()).await {
                            // If not a response, it might be an unsolicited message
                            // In a real implementation we would have a separate
                            // channel for these.
                            ipc_log!("Unsolicited message received: {:?}", msg.message_type);
                        }
                    }
                    Err(e) => {
                        ipc_log!("Messenger receiver task error: {}", e);
                        break;
                    }
                }
            }
        });
    }

    /// Send a request and wait for a response
    pub async fn request(
        &self,
        message: CommunicationMessage,
    ) -> Result<CommunicationMessage, CommunicationError> {
        let id = message.id.clone();
        let rx = self.broker.register_request(id).await;

        self.session.send(message).await?;

        match rx.await {
            Ok(resp) => Ok(resp),
            Err(_) => Err(CommunicationError::Timeout(
                "Response channel closed".to_string(),
            )),
        }
    }

    /// Send a message without waiting for response
    pub async fn send(&self, message: CommunicationMessage) -> Result<(), CommunicationError> {
        self.session.send(message).await
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

    /// Configure the communication protocol
    pub fn with_protocol(mut self, protocol: ProtocolType) -> Self {
        self.config.protocol = protocol;
        self
    }

    /// Configure the serialization format
    pub fn with_serialization_format(mut self, format: communication::SerializationFormat) -> Self {
        self.config.serialization_format = format;
        self
    }

    /// Configure timeout for communication operations
    pub fn with_timeout(mut self, timeout_ms: u64) -> Self {
        self.config.timeout_ms = timeout_ms;
        self
    }

    /// Send a custom message to the primary instance (legacy simple version)
    pub async fn send_message(
        &mut self,
        message: CommunicationMessage,
    ) -> Result<CommunicationMessage, CommunicationError> {
        let session = self.connect_persistent().await?;
        let messenger = Messenger::new(session);
        messenger.request(message).await
    }

    /// Connect to the primary instance and keep the connection alive
    pub async fn connect_persistent(&self) -> Result<IpcSession, CommunicationError> {
        let protocol = CommunicationFactory::create_protocol(self.config.protocol)?;
        let mut client = protocol.create_client(&self.config).await?;
        client.connect().await?;
        Ok(IpcSession { client })
    }

    /// Connect and return a high-level Messenger
    pub async fn connect_messenger(&self) -> Result<Messenger, CommunicationError> {
        let session = self.connect_persistent().await?;
        Ok(Messenger::new(session))
    }
}

/// A persistent IPC session
pub struct IpcSession {
    client: Box<dyn communication::CommunicationClient>,
}

impl IpcSession {
    /// Send a message to the other end
    pub async fn send(&self, message: CommunicationMessage) -> Result<(), CommunicationError> {
        self.client.send_message(&message).await
    }

    /// Receive a message from the other end
    pub async fn receive(&self) -> Result<CommunicationMessage, CommunicationError> {
        self.client.receive_message().await
    }

    /// Try to reconnect to the server
    pub async fn reconnect(&mut self) -> Result<(), CommunicationError> {
        let _ = self.client.disconnect().await;
        self.client.connect().await
    }

    /// Check if the session is still connected
    pub fn is_connected(&self) -> bool {
        self.client.is_connected()
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
        let app = SingleInstanceApp::new("test_app").without_fallback();

        assert!(!app.config().enable_fallback);
    }

    #[test]
    fn test_single_instance_app_with_fallback_protocols() {
        let protocols = vec![ProtocolType::FileBased, ProtocolType::InMemory];
        let app = SingleInstanceApp::new("test_app").with_fallback_protocols(protocols.clone());

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
        #[cfg(unix)]
        assert_eq!(client.config().protocol, ProtocolType::UnixSocket);
        #[cfg(windows)]
        assert_eq!(client.config().protocol, ProtocolType::FileBased);
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
        let cloned = protocol;
        assert_eq!(protocol, cloned);
    }

    // ============ CommunicationConfig Tests ============

    #[test]
    fn test_communication_config_default() {
        let config = CommunicationConfig::default();

        #[cfg(unix)]
        assert_eq!(config.protocol, ProtocolType::UnixSocket);
        #[cfg(windows)]
        assert_eq!(config.protocol, ProtocolType::FileBased);

        assert_eq!(config.identifier, "default");
        assert_eq!(config.timeout_ms, 5000);
        assert!(config.enable_fallback);
        assert!(!config.fallback_protocols.is_empty());
    }

    #[test]
    fn test_communication_config_custom() {
        let config = CommunicationConfig {
            protocol: ProtocolType::FileBased,
            serialization_format: SerializationFormat::Json,
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

        #[cfg(unix)]
        assert!(debug_format.contains("UnixSocket"));
        #[cfg(windows)]
        assert!(debug_format.contains("FileBased"));

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

        let deserialized: Result<CommunicationMessage, _> =
            serde_json::from_str(&serialized.unwrap());
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
        // FileBased and InMemory are always available
        let always_available = vec![ProtocolType::FileBased, ProtocolType::InMemory];

        for protocol in always_available {
            let result = communication::CommunicationFactory::create_protocol(protocol);
            assert!(
                result.is_ok(),
                "Protocol {:?} should be available",
                protocol
            );
        }

        // Unix-specific protocols only on Unix
        #[cfg(unix)]
        {
            let unix_protocols = vec![ProtocolType::UnixSocket, ProtocolType::SharedMemory];

            for protocol in unix_protocols {
                let result = communication::CommunicationFactory::create_protocol(protocol);
                assert!(
                    result.is_ok(),
                    "Protocol {:?} should be available on Unix",
                    protocol
                );
            }
        }

        // Windows-specific protocols: NamedPipe not yet implemented, so it should return an error
        #[cfg(windows)]
        {
            let result =
                communication::CommunicationFactory::create_protocol(ProtocolType::NamedPipe);
            assert!(result.is_err(), "NamedPipe should not be implemented yet");
        }
    }

    #[test]
    fn test_communication_factory_get_available_protocols() {
        let protocols = communication::CommunicationFactory::get_available_protocols();

        // At minimum, should have FileBased and InMemory (always available)
        assert!(protocols.contains(&ProtocolType::FileBased));
        assert!(protocols.contains(&ProtocolType::InMemory));

        #[cfg(unix)]
        {
            assert!(protocols.contains(&ProtocolType::UnixSocket));
            assert!(protocols.contains(&ProtocolType::SharedMemory));
        }

        #[cfg(windows)]
        {
            // On Windows, only FileBased and InMemory are available
            // NamedPipe is not yet implemented
            assert!(!protocols.contains(&ProtocolType::NamedPipe));
        }
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
